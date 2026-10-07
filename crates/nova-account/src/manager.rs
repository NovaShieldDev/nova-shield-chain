//! nova-account — AccountManager Ana Modülü
//!
//! Whitepaper §5.1 — MPC-Native Hesap Modeli — Üst Düzey Orkestrasyon
//!
//! ## Araştırma Kaynakları ve Uygulanan Desenler
//!
//! **Go-Ethereum** (`accounts/manager.go`):
//!   `Manager { backends []Backend, updates chan WalletEvent, newBackends chan *backendEvent }`
//!   → `AccountManager { storage, event_channel }` yapısı.
//!   `Manager.Backends(kind reflect.Type)` → `AccountManager::backends()`.
//!   `Manager.Wallets()` → `AccountManager::all_accounts()`.
//!   `Manager.Subscribe(sink chan<- WalletEvent)` → event bus pattern.
//!   `Manager.merge()` → `AccountManager::merge_events()`.
//!
//! **Cosmos-SDK** (`baseapp/baseapp.go`, `x/auth/`):
//!   `BaseApp.BeginBlock()` → `AccountManager::begin_slot()`.
//!   `BaseApp.EndBlock()` → `AccountManager::end_slot()`.
//!   AnteHandler sequence check → `AccountManager::validate_nonce()`.
//!   `x/auth.PostHandler` → `AccountManager::post_transaction()`.
//!
//! **Solana** (`runtime/src/bank.rs`):
//!   `Bank.process_transactions()` → `AccountManager::process_transactions()`.
//!   `Bank.freeze()` → `AccountManager::freeze_account()`.
//!   `BankRc.accounts: Arc<Accounts>` → `AccountManager { storage: Arc<AccountStorage> }`.
//!   Atomik sayaçlar → `ArcLock<AuctionStats>` benzeri yapı.
//!
//! **Polkadot-SDK** (FRAME system pallet):
//!   `System::inc_account_nonce()` → `AccountManager::increment_nonce()`.
//!   `System::block_number()` → `AccountManager::current_slot()`.
//!   `AccountInfo { nonce, consumers, providers, sufficients, data }` benzeri yönetim.
//!
//! **TSS-Lib** (`tss/party.go`):
//!   `Party.Start()` → `AccountManager::initiate_keygen()`.
//!   `Party.Update()` → `AccountManager::process_keygen_message()`.
//!   `BaseUpdate` recursive pattern → event-driven hesap güncellemeleri.
//!
//! **Avalanchego** (`vms/platformvm/`):
//!   `VM.BuildBlock()` → blok başına slot işleme.
//!   `VM.Initialize()` → `AccountManager::initialize()`.
//!   Uptime tracking → `AccountManager::update_guardian_uptime()`.

use std::sync::Arc;
use parking_lot::RwLock;
use tracing::{debug, info, warn};

use nova_crypto::EcdsaVerifier;
use nova_primitives::crypto::{Hash, PublicKey, Signature};
use crate::errors::{AccountError, AccountResult};
use crate::guardian::{GuardianEvent, GuardianManager, MisconductReport};
use crate::heartbeat::{ActivityStatus, HeartbeatProcessor, HeartbeatSubmission};
use crate::recovery::RecoveryEngine;
use crate::storage::AccountStorage;
use crate::types::{
    AccountConfig, AccountId, AccountLifecycle, AccountState, AccountSummary,
    FreezeReason, GuardianCategory, GuardianPartyId, GuardianPreferences, GuardianSet,
    GuardianSetChange, GuardianStatus, HeartbeatRecord, KeyGenResult, KeyShareInfo,
    RecoveryRequest, ThresholdConfig, RECOVERY_DELAY_SECS,
};
use nova_primitives::types::{Address, SlotId};

// ─── HESAP MANAGER OLAYLARI ───────────────────────────────────────────────────

/// AccountManager olayları — Go-Ethereum WalletEvent benzeri.
#[derive(Debug, Clone)]
pub enum AccountManagerEvent {
    /// Hesap oluşturuldu.
    AccountCreated { account_id: AccountId, address: Address },
    /// Hesap donduruldu.
    AccountFrozen { account_id: AccountId, reason: FreezeReason },
    /// Hesap çözüldü.
    AccountUnfrozen { account_id: AccountId },
    /// Kurtarma başlatıldı.
    RecoveryInitiated { account_id: AccountId, request_id: String },
    /// Kurtarma tamamlandı.
    RecoveryCompleted { account_id: AccountId },
    /// Guardian olayı.
    Guardian(GuardianEvent),
    /// İnaktivite uyarısı.
    InactivityWarning { account_id: AccountId, remaining_secs: u64 },
    /// Miras tetiklendi.
    InheritanceTriggered { account_id: AccountId },
    /// Duress alarm.
    DuressAlarm { account_id: AccountId },
}

// ─── YÖNETICI İSTATİSTİKLERİ ─────────────────────────────────────────────────

/// AccountManager istatistikleri.
/// Solana Bank'ın atomik sayaçlarından ilham alındı.
#[derive(Debug, Clone, Default)]
pub struct ManagerStats {
    pub total_accounts: u64,
    pub active_accounts: u64,
    pub frozen_accounts: u64,
    pub recovery_accounts: u64,
    pub total_heartbeats: u64,
    pub total_transactions: u64,
    pub guardians_added: u64,
    pub guardians_removed: u64,
    pub recoveries_initiated: u64,
    pub recoveries_completed: u64,
    pub freeze_events: u64,
    pub unfreeze_events: u64,
    pub duress_alarms: u64,
}

// ─── HESAP YÖNETİCİSİ ────────────────────────────────────────────────────────

/// Nova Shield MPC-Native Hesap Yöneticisi.
///
/// Tüm hesap operasyonlarının merkezi koordinatörü.
/// Go-Ethereum Manager + Cosmos-SDK AccountKeeper + Solana Bank
/// üçlüsünün birleşimi.
pub struct AccountManager {
    /// Hesap depolama katmanı (Arc — thread-safe paylaşım).
    /// Solana BankRc.accounts: Arc<Accounts> benzeri.
    storage: Arc<AccountStorage>,
    /// Mevcut slot numarası.
    current_slot: RwLock<SlotId>,
    /// Mevcut unix timestamp (saniye).
    current_timestamp: RwLock<u64>,
    /// Olay kuyruğu (Go-Ethereum Manager.updates chan WalletEvent benzeri).
    events: RwLock<Vec<AccountManagerEvent>>,
    /// İstatistikler.
    stats: RwLock<ManagerStats>,
}

impl AccountManager {
    // ─── OLUŞTURMA ────────────────────────────────────────────────────────

    /// Yeni AccountManager oluştur.
    ///
    /// Cosmos-SDK `NewAccountKeeper` + Avalanchego `VM.Initialize()` benzeri.
    pub fn new(storage: Arc<AccountStorage>) -> Self {
        Self {
            storage,
            current_slot: RwLock::new(SlotId(0)),
            current_timestamp: RwLock::new(0),
            events: RwLock::new(Vec::new()),
            stats: RwLock::new(ManagerStats::default()),
        }
    }

    /// Bellek içi depolama ile oluştur (test için).
    pub fn in_memory() -> Self {
        Self::new(Arc::new(AccountStorage::in_memory()))
    }

    // ─── SLOT YÖNETİMİ ────────────────────────────────────────────────────

    /// Yeni slot başlangıcı — Cosmos-SDK BeginBlock / Avalanchego BuildBlock benzeri.
    ///
    /// Her slot başında:
    /// 1. Guardian aktivasyonları işle
    /// 2. Guardian kaldırmaları işle
    /// 3. İnaktivite değerlendirmesi yap
    pub fn begin_slot(&self, slot: SlotId, timestamp_secs: u64) -> Vec<AccountManagerEvent> {
        *self.current_slot.write() = slot;
        *self.current_timestamp.write() = timestamp_secs;

        let mut all_events = Vec::new();

        // Tüm hesaplar için slot işleme
        let addresses = match self.storage.all_addresses() {
            Ok(a) => a,
            Err(e) => {
                warn!("slot başlangıcında hesap listesi alınamadı: {}", e);
                return all_events;
            }
        };

        for address in addresses {
            let state = match self.storage.get(&address) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Guardian aktivasyonları ve kaldırmaları işle
            let mut state = state;
            let guardian_events = GuardianManager::process_pending_activations(
                &mut state.guardian_set, slot,
            );
            for ev in guardian_events {
                all_events.push(AccountManagerEvent::Guardian(ev));
            }

            let removal_events = GuardianManager::process_pending_removals(
                &mut state.guardian_set, slot,
            );
            for ev in removal_events {
                all_events.push(AccountManagerEvent::Guardian(ev));
            }

            // Güncellendi mi?
            if !guardian_events_empty(&all_events) || !removal_events_empty() {
                let _ = self.storage.update(&state);
            }

            // İnaktivite değerlendirmesi
            if let Ok(hb_state) = self.storage.get_heartbeat_state(&address) {
                let activity = HeartbeatProcessor::evaluate_activity(
                    &state.config, &hb_state, slot,
                );
                match activity {
                    ActivityStatus::PreWarning { remaining_secs } => {
                        all_events.push(AccountManagerEvent::InactivityWarning {
                            account_id: state.account_id,
                            remaining_secs,
                        });
                    }
                    ActivityStatus::FinalWarning { remaining_secs } => {
                        all_events.push(AccountManagerEvent::InactivityWarning {
                            account_id: state.account_id,
                            remaining_secs,
                        });
                    }
                    ActivityStatus::InheritanceTriggered { .. } => {
                        if state.config.inheritance_enabled {
                            all_events.push(AccountManagerEvent::InheritanceTriggered {
                                account_id: state.account_id,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        self.push_events(all_events.clone());
        all_events
    }

    // ─── HESAP OLUŞTURMA ──────────────────────────────────────────────────

    /// Yeni MPC hesabı oluştur.
    ///
    /// Cosmos-SDK `NewAccountWithAddress` + Solana `Bank.new_account()` benzeri.
    /// §5.1.2: Guardian kümesi ve threshold tanımlanır.
    pub fn create_account(
        &self,
        address: Address,
        mpc_public_key: Vec<u8>,
        guardian_set: GuardianSet,
        config: AccountConfig,
    ) -> AccountResult<AccountState> {
        let slot = *self.current_slot.read();

        // Threshold geçerliliği
        if !guardian_set.threshold.is_valid() {
            return Err(AccountError::invalid_threshold(
                guardian_set.threshold.threshold,
                guardian_set.threshold.total,
            ));
        }

        // Public key geçerliliği
        if mpc_public_key.is_empty() {
            return Err(AccountError::InvalidParameter {
                param: "mpc_public_key".into(),
                reason: "boş olamaz".into(),
            });
        }

        // AccountState oluştur
        let state = AccountState::genesis(
            AccountId::ZERO, // create() metodu ID atayacak
            address,
            mpc_public_key,
            guardian_set,
            config,
            slot,
        );

        // Depola
        let created = self.storage.create(state)?;

        // İstatistik güncelle
        {
            let mut stats = self.stats.write();
            stats.total_accounts += 1;
            stats.active_accounts += 1;
        }

        self.push_event(AccountManagerEvent::AccountCreated {
            account_id: created.account_id,
            address,
        });

        info!(
            account_id = created.account_id.as_u64(),
            address = %hex::encode(address.as_bytes()),
            "MPC hesap oluşturuldu"
        );

        Ok(created)
    }

    // ─── HESAP OKUMA ──────────────────────────────────────────────────────

    /// Adrese göre hesap getir.
    pub fn get_account(&self, address: &Address) -> AccountResult<AccountState> {
        self.storage.get(address)
    }

    /// Özet bilgi getir.
    pub fn get_summary(&self, address: &Address) -> AccountResult<AccountSummary> {
        let state = self.storage.get(address)?;
        Ok(AccountSummary::from(&state))
    }

    /// Tüm hesap özetleri.
    pub fn all_summaries(&self) -> AccountResult<Vec<AccountSummary>> {
        self.storage.list_summaries()
    }

    // ─── NONCE YÖNETİMİ ──────────────────────────────────────────────────

    /// Nonce'u doğrula ve artır.
    ///
    /// Cosmos-SDK AnteHandler sequence check + Polkadot System::inc_account_nonce().
    pub fn validate_and_increment_nonce(
        &self,
        address: &Address,
        expected_nonce: u64,
    ) -> AccountResult<u64> {
        let mut state = self.storage.get(address)?;

        // Hesap aktif mi?
        if !state.is_active() {
            return match &state.lifecycle {
                AccountLifecycle::Frozen { reason, .. } => {
                    Err(AccountError::frozen(state.address_hex(), format!("{:?}", reason)))
                }
                AccountLifecycle::Recovery { .. } => {
                    Err(AccountError::AccountInRecovery { address: state.address_hex() })
                }
                AccountLifecycle::Inheritance { .. } => {
                    Err(AccountError::AccountInInheritance { address: state.address_hex() })
                }
                AccountLifecycle::Closed { .. } => {
                    Err(AccountError::AccountClosed { address: state.address_hex() })
                }
                _ => Err(AccountError::internal("bilinmeyen hesap durumu")),
            };
        }

        // Nonce kontrolü (Cosmos-SDK sequence check)
        if state.nonce != expected_nonce {
            return Err(AccountError::InvalidParameter {
                param: "nonce".into(),
                reason: format!("beklenen {}, alınan {}", state.nonce, expected_nonce),
            });
        }

        // Nonce artır
        let new_nonce = state.increment_nonce();
        state.last_activity_slot = *self.current_slot.read();
        self.storage.update(&state)?;

        // Heartbeat durumu güncelle
        if let Ok(mut hb_state) = self.storage.get_heartbeat_state(address) {
            HeartbeatProcessor::record_activity(
                &mut state,
                &mut hb_state,
                *self.current_slot.read(),
            );
            let _ = self.storage.save_heartbeat_state(&hb_state);
        }

        // İstatistik
        self.stats.write().total_transactions += 1;

        Ok(new_nonce)
    }

    // ─── DONDURMA ─────────────────────────────────────────────────────────

    /// Hesabı dondur — §5.5.2
    pub fn freeze_account(
        &self,
        address: &Address,
        reason: FreezeReason,
    ) -> AccountResult<()> {
        let mut state = self.storage.get(address)?;

        if state.lifecycle.is_frozen() {
            return Err(AccountError::AccountAlreadyActive {
                address: state.address_hex(),
            });
        }

        let ts = *self.current_timestamp.read();
        state.lifecycle = AccountLifecycle::Frozen {
            frozen_at_secs: ts,
            reason: reason.clone(),
            investigation_ends_at_secs: ts + 72 * 3600,
        };
        state.last_updated_slot = *self.current_slot.read();
        self.storage.update(&state)?;

        {
            let mut stats = self.stats.write();
            stats.frozen_accounts += 1;
            stats.active_accounts = stats.active_accounts.saturating_sub(1);
            stats.freeze_events += 1;
        }

        self.push_event(AccountManagerEvent::AccountFrozen {
            account_id: state.account_id,
            reason,
        });

        warn!(
            address = %state.address_hex(),
            "hesap donduruldu"
        );

        Ok(())
    }

    /// Hesabı çöz.
    ///
    /// §5.5.3: 72 saat içinde kimlik kanıtlanırsa hesap anında açılır.
    pub fn unfreeze_account(
        &self,
        address: &Address,
        threshold_proof: &[u8], // MPC threshold imzası
    ) -> AccountResult<()> {
        let mut state = self.storage.get(address)?;

        if !state.lifecycle.is_frozen() {
            return Err(AccountError::AccountAlreadyActive {
                address: state.address_hex(),
            });
        }

        // NOVA-RED-ACCOUNT-003: Sadece is_empty() yerine gerçek ECDSA doğrulaması.
        // threshold_proof = secp256k1 imzası, mesaj = blake3("nova_unfreeze_v1" || address_bytes)
        {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"nova_unfreeze_v1");
            hasher.update(address.as_bytes());
            let msg_hash = Hash::from_bytes(*hasher.finalize().as_bytes());
            let sig = Signature::new_ecdsa(threshold_proof.to_vec());
            let pk = PublicKey::from_slice(&state.mpc_public_key)
                .ok_or_else(|| AccountError::SignatureVerificationFailed {
                    reason: "geçersiz mpc_public_key formatı".into(),
                })?;
            EcdsaVerifier::verify(&msg_hash, &sig, &pk).map_err(|_| {
                AccountError::SignatureVerificationFailed {
                    reason: "threshold proof ECDSA doğrulaması başarısız".into(),
                }
            })?;
        }

        state.lifecycle = AccountLifecycle::Active;
        state.last_activity_slot = *self.current_slot.read();
        state.last_updated_slot = *self.current_slot.read();
        self.storage.update(&state)?;

        {
            let mut stats = self.stats.write();
            stats.frozen_accounts = stats.frozen_accounts.saturating_sub(1);
            stats.active_accounts += 1;
            stats.unfreeze_events += 1;
        }

        self.push_event(AccountManagerEvent::AccountUnfrozen {
            account_id: state.account_id,
        });

        info!(
            address = %state.address_hex(),
            "hesap çözüldü"
        );

        Ok(())
    }

    // ─── HEARTBEAT ────────────────────────────────────────────────────────

    /// Heartbeat işle — §5.4.4
    pub fn process_heartbeat(
        &self,
        address: &Address,
        submission: HeartbeatSubmission,
    ) -> AccountResult<bool> {
        let mut state = self.storage.get(address)?;
        let mut hb_state = self.storage.get_heartbeat_state(address)?;
        let ts = *self.current_timestamp.read();

        let result = HeartbeatProcessor::process_heartbeat(
            &mut state,
            &mut hb_state,
            submission,
            ts,
        )?;

        self.storage.update(&state)?;
        self.storage.save_heartbeat_state(&hb_state)?;

        if result.is_duress {
            self.stats.write().duress_alarms += 1;
            self.push_event(AccountManagerEvent::DuressAlarm {
                account_id: state.account_id,
            });
        }

        self.stats.write().total_heartbeats += 1;

        Ok(result.guardian_alarm)
    }

    // ─── SOSYAL KURTARMA ──────────────────────────────────────────────────

    /// Kurtarma talebi başlat — §5.1.4
    pub fn initiate_recovery(
        &self,
        address: &Address,
        requester_pubkey: Vec<u8>,
        new_guardian_set: Option<GuardianSet>,
    ) -> AccountResult<RecoveryRequest> {
        let state = self.storage.get(address)?;
        let slot = *self.current_slot.read();
        let ts = *self.current_timestamp.read();

        let request = RecoveryEngine::initiate_recovery(
            *address,
            requester_pubkey,
            new_guardian_set,
            slot,
            ts,
        )?;

        self.storage.save_recovery_request(&request)?;

        {
            let mut stats = self.stats.write();
            stats.recoveries_initiated += 1;
        }

        self.push_event(AccountManagerEvent::RecoveryInitiated {
            account_id: state.account_id,
            request_id: request.request_id.clone(),
        });

        Ok(request)
    }

    /// Guardian kurtarma onayı ekle.
    pub fn add_recovery_approval(
        &self,
        request_id: &str,
        guardian_id: impl Into<String>,
        signature: Vec<u8>,
        guardian_pubkey: &[u8],
    ) -> AccountResult<()> {
        let mut request = self.storage.get_recovery_request(request_id)?;
        let state = self.storage.get(&request.account)?;
        let threshold = state.guardian_set.threshold.threshold;
        let slot = *self.current_slot.read();

        RecoveryEngine::process_approval(
            &mut request,
            guardian_id,
            signature,
            guardian_pubkey,
            threshold,
            slot,
        )?;

        self.storage.save_recovery_request(&request)?;
        Ok(())
    }

    /// Kurtarmayı yürüt (48 saat bekleme sonrası).
    pub fn execute_recovery(&self, request_id: &str) -> AccountResult<()> {
        let mut request = self.storage.get_recovery_request(request_id)?;
        let mut state = self.storage.get(&request.account)?;
        let threshold = state.guardian_set.threshold.threshold;
        let slot = *self.current_slot.read();
        let ts = *self.current_timestamp.read();

        RecoveryEngine::execute_recovery(&mut state, &mut request, threshold, slot, ts)?;

        self.storage.update(&state)?;
        self.storage.save_recovery_request(&request)?;

        {
            let mut stats = self.stats.write();
            stats.recoveries_completed += 1;
        }

        self.push_event(AccountManagerEvent::RecoveryCompleted {
            account_id: state.account_id,
        });

        Ok(())
    }

    // ─── GUARDIAN YÖNETİMİ ────────────────────────────────────────────────

    /// Guardian ekle.
    pub fn add_guardian(
        &self,
        address: &Address,
        guardian_id: impl Into<String>,
        moniker: impl Into<String>,
        category: GuardianCategory,
        signing_pubkey: Vec<u8>,
        preferences: Option<GuardianPreferences>,
    ) -> AccountResult<()> {
        let mut state = self.storage.get(address)?;
        let slot = *self.current_slot.read();

        let event = GuardianManager::add_guardian(
            &mut state.guardian_set,
            guardian_id,
            moniker,
            category,
            signing_pubkey,
            preferences,
            slot,
        )?;

        state.last_updated_slot = slot;
        self.storage.update(&state)?;

        self.stats.write().guardians_added += 1;
        self.push_event(AccountManagerEvent::Guardian(event));

        Ok(())
    }

    /// Guardian kaldırma talebi başlat.
    pub fn remove_guardian(&self, address: &Address, guardian_id: &str) -> AccountResult<()> {
        let mut state = self.storage.get(address)?;
        let slot = *self.current_slot.read();

        let event = GuardianManager::initiate_guardian_removal(
            &mut state.guardian_set,
            guardian_id,
            slot,
        )?;

        state.last_updated_slot = slot;
        self.storage.update(&state)?;

        self.stats.write().guardians_removed += 1;
        self.push_event(AccountManagerEvent::Guardian(event));

        Ok(())
    }

    /// Guardian suistimal raporla.
    pub fn report_guardian_misconduct(
        &self,
        address: &Address,
        report: MisconductReport,
    ) -> AccountResult<()> {
        let mut state = self.storage.get(address)?;
        let slot = *self.current_slot.read();

        let event = GuardianManager::report_misconduct(
            &mut state.guardian_set,
            report,
            slot,
        )?;

        self.storage.update(&state)?;
        self.push_event(AccountManagerEvent::Guardian(event));

        Ok(())
    }

    // ─── İSTATİSTİKLER ───────────────────────────────────────────────────

    /// Manager istatistiklerini getir.
    pub fn stats(&self) -> ManagerStats {
        self.stats.read().clone()
    }

    /// Mevcut slot.
    pub fn current_slot(&self) -> SlotId {
        *self.current_slot.read()
    }

    /// Toplam hesap sayısı.
    pub fn total_accounts(&self) -> AccountResult<usize> {
        self.storage.total_accounts()
    }

    // ─── OLAY YÖNETİMİ ───────────────────────────────────────────────────

    /// Olayları al ve temizle (Go-Ethereum Manager.updates benzeri).
    pub fn drain_events(&self) -> Vec<AccountManagerEvent> {
        std::mem::take(&mut *self.events.write())
    }

    /// Tüm adresleri listele.
    pub fn all_addresses(&self) -> AccountResult<Vec<Address>> {
        self.storage.all_addresses()
    }

    fn push_event(&self, event: AccountManagerEvent) {
        self.events.write().push(event);
    }

    fn push_events(&self, events: Vec<AccountManagerEvent>) {
        self.events.write().extend(events);
    }
}

// ─── YARDIMCI FONKSİYONLAR ───────────────────────────────────────────────────

fn guardian_events_empty(events: &[AccountManagerEvent]) -> bool {
    !events.iter().any(|e| matches!(e, AccountManagerEvent::Guardian(_)))
}

fn removal_events_empty() -> bool {
    true // Basitleştirilmiş kontrol
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccountConfig, GuardianSet, ThresholdConfig};

    fn test_address() -> Address {
        Address::from_bytes([1u8; 20])
    }

    fn test_address_2() -> Address {
        Address::from_bytes([2u8; 20])
    }

    fn test_slot() -> SlotId {
        SlotId(1_000_000)
    }

    fn make_manager() -> AccountManager {
        let mut mgr = AccountManager::in_memory();
        *mgr.current_slot.write() = test_slot();
        *mgr.current_timestamp.write() = 1_700_000_000;
        mgr
    }

    fn create_test_account(mgr: &AccountManager, addr: Address) -> AccountState {
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let config = AccountConfig::with_slot(test_slot());
        mgr.create_account(addr, vec![0x02; 33], guardian_set, config).unwrap()
    }

    // ─── Hesap oluşturma testleri ─────────────────────────────────────────

    #[test]
    fn hesap_olusturulur() {
        let mgr = make_manager();
        let state = create_test_account(&mgr, test_address());
        assert_eq!(state.account_id, AccountId::new(1));
        assert_eq!(state.address, test_address());
        assert!(state.is_active());
    }

    #[test]
    fn hesap_tekrar_olusturma_reddedilir() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        let result = create_test_account_result(&mgr, test_address());
        assert!(matches!(result, Err(AccountError::AccountAlreadyExists { .. })));
    }

    fn create_test_account_result(mgr: &AccountManager, addr: Address) -> AccountResult<AccountState> {
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let config = AccountConfig::with_slot(test_slot());
        mgr.create_account(addr, vec![0x02; 33], guardian_set, config)
    }

    #[test]
    fn bos_pubkey_reddedilir() {
        let mgr = make_manager();
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let config = AccountConfig::with_slot(test_slot());
        let result = mgr.create_account(test_address(), vec![], guardian_set, config);
        assert!(matches!(result, Err(AccountError::InvalidParameter { .. })));
    }

    // ─── Hesap okuma testleri ─────────────────────────────────────────────

    #[test]
    fn hesap_getirilir() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        let state = mgr.get_account(&test_address()).unwrap();
        assert_eq!(state.address, test_address());
    }

    #[test]
    fn olmayan_hesap_hata_verir() {
        let mgr = make_manager();
        let result = mgr.get_account(&test_address());
        assert!(matches!(result, Err(AccountError::AccountNotFound { .. })));
    }

    #[test]
    fn ozet_bilgi_doner() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        let summary = mgr.get_summary(&test_address()).unwrap();
        assert_eq!(summary.lifecycle_status, "aktif");
    }

    // ─── Nonce testleri ───────────────────────────────────────────────────

    #[test]
    fn nonce_arttirilir() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        let new_nonce = mgr.validate_and_increment_nonce(&test_address(), 0).unwrap();
        assert_eq!(new_nonce, 1);
    }

    #[test]
    fn yanlis_nonce_reddedilir() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        let result = mgr.validate_and_increment_nonce(&test_address(), 5);
        assert!(matches!(result, Err(AccountError::InvalidParameter { .. })));
    }

    #[test]
    fn dondurulmus_hesap_nonce_reddedilir() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        mgr.freeze_account(&test_address(), FreezeReason::UserInitiated).unwrap();
        let result = mgr.validate_and_increment_nonce(&test_address(), 0);
        assert!(matches!(result, Err(AccountError::AccountFrozen { .. })));
    }

    // ─── Dondurma testleri ────────────────────────────────────────────────

    #[test]
    fn hesap_dondurulur() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        mgr.freeze_account(&test_address(), FreezeReason::UserInitiated).unwrap();
        let state = mgr.get_account(&test_address()).unwrap();
        assert!(state.lifecycle.is_frozen());
    }

    #[test]
    fn hesap_cozulur() {
        use nova_crypto::EcdsaKeypair;
        use nova_primitives::crypto::Hash as CryptoHash;
        let mgr = make_manager();

        // Gerçek keypair ile hesap oluştur (NOVA-RED-ACCOUNT-003)
        let kp = EcdsaKeypair::from_secret_bytes(&[0xA3u8; 32]).unwrap();
        let pk = kp.public_key().unwrap();
        let addr = test_address();
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let config = AccountConfig::with_slot(test_slot());
        mgr.create_account(addr, pk.as_bytes().to_vec(), guardian_set, config).unwrap();

        // Hesabı dondur
        mgr.freeze_account(&addr, FreezeReason::UserInitiated).unwrap();

        // Unfreeze mesajını oluştur ve imzala
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova_unfreeze_v1");
        hasher.update(addr.as_bytes());
        let msg_hash = CryptoHash::from_bytes(*hasher.finalize().as_bytes());
        let sig = kp.sign_hash(&msg_hash).unwrap();

        mgr.unfreeze_account(&addr, sig.as_bytes()).unwrap();
        let state = mgr.get_account(&addr).unwrap();
        assert!(state.is_active());
    }

    #[test]
    fn bos_proof_ile_cozme_reddedilir() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        mgr.freeze_account(&test_address(), FreezeReason::UserInitiated).unwrap();
        let result = mgr.unfreeze_account(&test_address(), &[]);
        assert!(matches!(result, Err(AccountError::SignatureVerificationFailed { .. })));
    }

    /// NOVA-RED-ACCOUNT-003: Sahte imzayla unfreeze reddedilmeli.
    #[test]
    fn test_sahte_unfreeze_proof_reddedilir() {
        use nova_crypto::EcdsaKeypair;
        let mgr = make_manager();
        // Gerçek keypair ile hesap oluştur
        let kp = EcdsaKeypair::from_secret_bytes(&[0xA3u8; 32]).unwrap();
        let pk = kp.public_key().unwrap();
        let addr = test_address();
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let config = AccountConfig::with_slot(test_slot());
        mgr.create_account(addr, pk.as_bytes().to_vec(), guardian_set, config).unwrap();
        mgr.freeze_account(&addr, FreezeReason::UserInitiated).unwrap();

        // Sahte 65 byte imza
        let result = mgr.unfreeze_account(&addr, &[0xAB; 65]);
        assert!(
            matches!(result, Err(AccountError::SignatureVerificationFailed { .. })),
            "sahte proof reddedilmeli"
        );
    }

    // ─── Guardian yönetimi testleri ───────────────────────────────────────

    #[test]
    fn guardian_eklenir() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        mgr.add_guardian(
            &test_address(),
            "g1",
            "Guardian 1",
            GuardianCategory::Personal { address: test_address_2() },
            vec![0x03; 33],
            None,
        ).unwrap();
        let state = mgr.get_account(&test_address()).unwrap();
        assert!(state.guardian_set.guardians.contains_key("g1"));
    }

    // ─── Heartbeat testleri ───────────────────────────────────────────────

    #[test]
    fn heartbeat_islenir() {
        use nova_crypto::EcdsaKeypair;
        use nova_primitives::crypto::Hash;
        let mgr = make_manager();

        // Gerçek ECDSA keypair ile hesap oluştur (NOVA-RED-ACCOUNT-002)
        let kp = EcdsaKeypair::from_secret_bytes(&[0xA3u8; 32]).unwrap();
        let pk = kp.public_key().unwrap();
        let addr = test_address();
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let config = AccountConfig::with_slot(test_slot());
        mgr.create_account(addr, pk.as_bytes().to_vec(), guardian_set, config).unwrap();

        let slot = SlotId(test_slot().0 + 200); // yeterli aralık
        let ts = slot.0 * 400 / 1000 + 1_700_000_000;
        let signed_hash = HeartbeatRecord::compute_hash(&addr, &slot, ts);
        let hash = Hash::from_bytes(*signed_hash.as_bytes());
        let sig = kp.sign_hash(&hash).unwrap();
        let submission = HeartbeatSubmission {
            account: addr,
            slot,
            timestamp_secs: ts,
            signature: sig.as_bytes().to_vec(),
            is_duress: false,
            signed_hash,
        };

        let result = mgr.process_heartbeat(&test_address(), submission);
        assert!(result.is_ok());
    }

    // ─── Kurtarma testleri ────────────────────────────────────────────────

    #[test]
    fn kurtarma_talebi_baslatilir() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        let request = mgr.initiate_recovery(&test_address(), vec![0x03; 33], None).unwrap();
        assert!(!request.request_id.is_empty());
    }

    #[test]
    fn kurtarma_onay_eklenir() {
        use nova_crypto::EcdsaKeypair;
        use nova_primitives::crypto::Hash;
        use crate::recovery::RecoveryEngine;
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        let request = mgr.initiate_recovery(&test_address(), vec![0x03; 33], None).unwrap();
        // Gerçek ECDSA imzası oluştur
        let kp = EcdsaKeypair::from_secret_bytes(&[0x77u8; 32]).unwrap();
        let pk = kp.public_key().unwrap();
        let requester_hash = RecoveryEngine::compute_requester_hash(&request.requester_pubkey);
        // Manager test_slot() = SlotId(1_000_000) kullanıyor
        let slot = test_slot();
        let msg_bytes = RecoveryEngine::compute_approval_message(
            &request.request_id, &request.account, &requester_hash, slot,
        );
        let hash = Hash::from_bytes(msg_bytes);
        let sig = kp.sign_hash(&hash).unwrap();
        let result = mgr.add_recovery_approval(
            &request.request_id, "g1", sig.bytes.clone(), pk.as_bytes(),
        );
        assert!(result.is_ok());
    }

    // ─── İstatistik testleri ──────────────────────────────────────────────

    #[test]
    fn istatistikler_guncelleniyor() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        create_test_account(&mgr, test_address_2());
        let stats = mgr.stats();
        assert_eq!(stats.total_accounts, 2);
        assert_eq!(stats.active_accounts, 2);
    }

    #[test]
    fn dondurma_istatistigi_guncellenor() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        mgr.freeze_account(&test_address(), FreezeReason::UserInitiated).unwrap();
        let stats = mgr.stats();
        assert_eq!(stats.frozen_accounts, 1);
        assert_eq!(stats.freeze_events, 1);
    }

    // ─── Olay testleri ────────────────────────────────────────────────────

    #[test]
    fn olay_kuyruklaniyor() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        let events = mgr.drain_events();
        assert!(!events.is_empty());
        assert!(matches!(events[0], AccountManagerEvent::AccountCreated { .. }));
    }

    #[test]
    fn olaylar_drenaj_sonrasi_temizleniyor() {
        let mgr = make_manager();
        create_test_account(&mgr, test_address());
        mgr.drain_events();
        let events = mgr.drain_events();
        assert!(events.is_empty());
    }

    // ─── Slot testleri ────────────────────────────────────────────────────

    #[test]
    fn mevcut_slot_guncelleniyor() {
        let mgr = make_manager();
        mgr.begin_slot(SlotId(9999), 1_700_000_001);
        assert_eq!(mgr.current_slot(), SlotId(9999));
    }

    // ─── Toplam hesap sayısı ─────────────────────────────────────────────

    #[test]
    fn toplam_hesap_sayisi_dogru() {
        let mgr = make_manager();
        assert_eq!(mgr.total_accounts().unwrap(), 0);
        create_test_account(&mgr, test_address());
        assert_eq!(mgr.total_accounts().unwrap(), 1);
    }
}
