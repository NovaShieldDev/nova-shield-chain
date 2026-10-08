//! Cross-chain koordinatörü — §5.6.3
//!
//! Polkadot bridge relay `Client` trait pattern:
//! - `Chain` trait: header/block/tx operations
//! - `ChainRuntimeVersion`: runtime versiyonu
//!
//! Nova Shield Guard CrossChainCoordinator:
//! - Vault oluştur (MPC adresi türet)
//! - İşlem talebi oluştur
//! - MPC imza toplama simülasyonu (guardian'lardan eşik imzası)
//! - Hedef zincire broadcast (mock)

use crate::bitcoin::{BitcoinTxBuilder, BitcoinUtxo, derive_mpc_btc_address};
use crate::cosmos::{IbcChannel, IbcModule, IbcTransferMsg};
use crate::errors::{GuardError, GuardResult};
use crate::ethereum::{EthAddress, EthChainId, EthTx};
use crate::solana::{SolanaPubkey, SolanaTxBuilder};
use crate::types::{
    AssetAddress, ChainId, CrossChainAsset, CrossChainTxReceipt, CrossChainTxRequest,
    GuardedVault, MpcThreshold, blake3_hash_many,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

// ─── MpcSignSession ───────────────────────────────────────────────────────────

/// MPC imza oturumu — guardian'lar bu oturumda imzalar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpcSignSession {
    pub session_id: u64,
    pub vault_id: u64,
    /// İmzalanacak mesaj
    pub message_hash: [u8; 32],
    /// Onay veren guardian'lar
    pub approvals: Vec<[u8; 20]>,
    /// Gerekli eşik
    pub threshold: u32,
    /// GUARD-001: Vault'un kayıtlı guardian listesi — onay veren bunlardan biri olmalı
    pub registered_guardians: Vec<[u8; 20]>,
    /// Tamamlandı mı?
    pub complete: bool,
    /// Toplu imza (eşik sağlandığında)
    pub aggregate_signature: Option<[u8; 32]>,
}

impl MpcSignSession {
    pub fn new(
        session_id: u64,
        vault_id: u64,
        message_hash: [u8; 32],
        threshold: u32,
        registered_guardians: Vec<[u8; 20]>,
    ) -> Self {
        Self {
            session_id,
            vault_id,
            message_hash,
            approvals: vec![],
            threshold,
            registered_guardians,
            complete: false,
            aggregate_signature: None,
        }
    }

    /// Guardian onayı ekle — yalnızca kayıtlı guardian'lar onaylayabilir (GUARD-001)
    pub fn add_approval(&mut self, guardian: [u8; 20]) -> Result<bool, GuardError> {
        // GUARD-001: Guardian vault'un kayıtlı listesinde mi?
        if !self.registered_guardians.contains(&guardian) {
            return Err(GuardError::MpcSignatureFailed {
                reason: format!(
                    "kayıtsız guardian: 0x{} vault guardian listesinde değil",
                    hex::encode(guardian)
                ),
            });
        }
        if !self.approvals.contains(&guardian) {
            self.approvals.push(guardian);
        }
        if self.approvals.len() >= self.threshold as usize && !self.complete {
            // Eşik sağlandı — aggregate commitment üret
            // GUARD-002 notu: gerçek MPC/FROST threshold imzası production'da gereklidir.
            // Şu an: her guardian'ın adresini ve mesaj hash'ini birleştiren deterministik hash.
            let sigs: Vec<u8> = self.approvals.iter().flat_map(|a| a.iter().copied()).collect();
            let agg = blake3_hash_many(&[b"mpc-aggregate", &self.message_hash, &sigs]);
            self.aggregate_signature = Some(agg);
            self.complete = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }
}

// ─── BroadcastResult ──────────────────────────────────────────────────────────

/// Broadcast sonucu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastResult {
    pub chain: ChainId,
    pub tx_hash: [u8; 32],
    pub success: bool,
    pub fee_paid: u64,
}

// ─── CrossChainCoordinator ────────────────────────────────────────────────────

/// Ana cross-chain koordinatör
/// §5.6.3: "TX talebi guardian'lara iletilir → t/n MPC imza → broadcast"
pub struct CrossChainCoordinator {
    /// Vault deposu
    vaults: DashMap<u64, GuardedVault>,
    /// Aktif MPC oturumları
    sign_sessions: DashMap<u64, MpcSignSession>,
    /// IBC modülü (Cosmos)
    ibc: IbcModule,
    /// Vault ID sayacı
    vault_counter: AtomicU64,
    /// Session ID sayacı
    session_counter: AtomicU64,
}

impl CrossChainCoordinator {
    pub fn new() -> Self {
        let ibc = IbcModule::new();
        // Varsayılan Cosmos kanalı
        ibc.register_channel(IbcChannel::new_transfer("channel-0", "channel-1"));
        Self {
            vaults: DashMap::new(),
            sign_sessions: DashMap::new(),
            ibc,
            vault_counter: AtomicU64::new(1),
            session_counter: AtomicU64::new(1),
        }
    }

    // ─── Vault Yönetimi ───────────────────────────────────────────────────

    /// Yeni MPC vault oluştur — §5.6.3
    pub fn create_vault(
        &self,
        owner: [u8; 20],
        chain: ChainId,
        threshold: u32,
        guardians: Vec<[u8; 20]>,
        created_at_slot: u64,
    ) -> GuardResult<u64> {
        let total = guardians.len() as u32;
        let mpc_threshold = MpcThreshold::new(threshold, total, guardians)
            .ok_or_else(|| GuardError::InsufficientGuardians {
                got: total as usize,
                needed: threshold as usize,
            })?;

        let vault_id = self.vault_counter.fetch_add(1, Ordering::Relaxed);

        // Guardian sıralaması deterministik olmalı: sıralanmış guardian bytes
        let sorted_guardians: Vec<u8> = {
            let mut gs = mpc_threshold.guardians.clone();
            gs.sort();
            gs.into_iter().flat_map(|g| g.into_iter()).collect()
        };

        // Zincire özgü adres türet — tüm katılımcılar (owner + threshold + guardians) dahil
        let address = match &chain {
            ChainId::Bitcoin => derive_mpc_btc_address(&mpc_threshold),
            ChainId::Ethereum => {
                let hash = blake3_hash_many(&[
                    b"nova-eth-mpc-addr",
                    &owner,
                    &threshold.to_le_bytes(),
                    sorted_guardians.as_slice(),
                ]);
                let mut addr_bytes = [0u8; 20];
                addr_bytes.copy_from_slice(&hash[..20]);
                AssetAddress::ethereum(addr_bytes)
            }
            ChainId::Solana => {
                let hash = blake3_hash_many(&[
                    b"nova-sol-mpc-addr",
                    &owner,
                    &threshold.to_le_bytes(),
                    sorted_guardians.as_slice(),
                ]);
                let mut pk_bytes = [0u8; 32];
                pk_bytes.copy_from_slice(&hash);
                AssetAddress::solana(pk_bytes)
            }
            ChainId::Cosmos => {
                let hash = blake3_hash_many(&[
                    b"nova-cosmos-mpc-addr",
                    &owner,
                    &threshold.to_le_bytes(),
                    sorted_guardians.as_slice(),
                ]);
                let mut addr_bytes = [0u8; 20];
                addr_bytes.copy_from_slice(&hash[..20]);
                AssetAddress::cosmos(addr_bytes)
            }
        };

        let vault = GuardedVault {
            id: vault_id,
            owner,
            chain,
            address,
            threshold: mpc_threshold,
            created_at_slot,
            active: true,
            balance: 0,
        };

        self.vaults.insert(vault_id, vault);
        Ok(vault_id)
    }

    /// Vault'a bakiye ekle (deposit)
    pub fn deposit(&self, vault_id: u64, amount: u64) -> GuardResult<()> {
        self.vaults.get_mut(&vault_id)
            .ok_or(GuardError::VaultNotFound { vault_id })?
            .balance += amount;
        Ok(())
    }

    /// Vault bilgisi al
    pub fn get_vault(&self, vault_id: u64) -> GuardResult<GuardedVault> {
        self.vaults.get(&vault_id)
            .map(|v| v.clone())
            .ok_or(GuardError::VaultNotFound { vault_id })
    }

    // ─── MPC İmza ─────────────────────────────────────────────────────────

    /// İşlem için MPC imza oturumu başlat
    pub fn initiate_sign(
        &self,
        vault_id: u64,
        message_hash: [u8; 32],
    ) -> GuardResult<u64> {
        let vault = self.get_vault(vault_id)?;
        let session_id = self.session_counter.fetch_add(1, Ordering::Relaxed);
        // GUARD-001: Vault'un guardian listesini oturuma geç
        let session = MpcSignSession::new(
            session_id,
            vault_id,
            message_hash,
            vault.threshold.threshold,
            vault.threshold.guardians.clone(),
        );
        self.sign_sessions.insert(session_id, session);
        Ok(session_id)
    }

    /// Guardian onayı ekle — yalnızca vault'un kayıtlı guardian'ları onaylayabilir (GUARD-001)
    pub fn guardian_approve(
        &self,
        session_id: u64,
        guardian: [u8; 20],
    ) -> GuardResult<bool> {
        let mut session = self.sign_sessions.get_mut(&session_id)
            .ok_or(GuardError::MpcSignatureFailed {
                reason: format!("oturum bulunamadı: {}", session_id),
            })?;
        session.add_approval(guardian)
    }

    /// İmza oturumu sonucunu al
    pub fn get_signature(&self, session_id: u64) -> GuardResult<[u8; 32]> {
        let session = self.sign_sessions.get(&session_id)
            .ok_or(GuardError::MpcSignatureFailed {
                reason: "oturum bulunamadı".into(),
            })?;
        session.aggregate_signature.ok_or(GuardError::MpcSignatureFailed {
            reason: "eşik henüz sağlanmadı".into(),
        })
    }

    // ─── Cross-chain İşlemler ─────────────────────────────────────────────

    /// Bitcoin transferi
    pub fn send_bitcoin(
        &self,
        vault_id: u64,
        to: &AssetAddress,
        amount_satoshi: u64,
        fee_rate: u64,
        current_slot: u64,
    ) -> GuardResult<CrossChainTxReceipt> {
        let vault = self.get_vault(vault_id)?;

        if vault.chain != ChainId::Bitcoin {
            return Err(GuardError::UnsupportedChain {
                chain: format!("vault {:?} Bitcoin değil", vault.chain),
            });
        }

        if vault.balance < amount_satoshi {
            return Err(GuardError::InsufficientBalance {
                available: vault.balance,
                required: amount_satoshi,
            });
        }

        // Bitcoin tx oluştur
        let mut builder = BitcoinTxBuilder::new(fee_rate);
        builder.add_utxo(BitcoinUtxo::new(
            blake3_hash_many(&[b"utxo", &vault_id.to_le_bytes()]),
            0,
            vault.balance,
            vault.address.raw.clone(),
        ));

        let change_script = crate::bitcoin::build_p2wsh_script(
            &vault.address.raw.iter().cloned().collect::<Vec<_>>().try_into().unwrap_or([0u8; 32])
        );
        let tx = builder.build(to, amount_satoshi, change_script)?;
        let tx_hash = tx.compute_txid();

        // MPC imzala (tüm guardian'lar otomatik onaylar — mock)
        let guardians = vault.threshold.guardians.clone();
        let session_id = self.initiate_sign(vault_id, tx.sighash())?;
        let mut signed = false;
        for guardian in &guardians {
            if self.guardian_approve(session_id, *guardian)? {
                signed = true;
                break;
            }
        }

        if !signed {
            return Err(GuardError::MpcSignatureFailed {
                reason: "guardian eşiği sağlanamadı".into(),
            });
        }

        // Bakiye güncelle
        self.vaults.get_mut(&vault_id).unwrap().balance -= amount_satoshi;

        Ok(CrossChainTxReceipt {
            tx_hash,
            vault_id,
            recorded_slot: current_slot,
            signers: guardians,
            success: true,
        })
    }

    /// Ethereum transferi
    pub fn send_ethereum(
        &self,
        vault_id: u64,
        to: EthAddress,
        amount_wei: u64,
        current_slot: u64,
    ) -> GuardResult<CrossChainTxReceipt> {
        let vault = self.get_vault(vault_id)?;

        if vault.chain != ChainId::Ethereum {
            return Err(GuardError::UnsupportedChain {
                chain: format!("vault {:?} Ethereum değil", vault.chain),
            });
        }

        if vault.balance < amount_wei {
            return Err(GuardError::InsufficientBalance {
                available: vault.balance,
                required: amount_wei,
            });
        }

        let mut eth_addr = [0u8; 20];
        eth_addr.copy_from_slice(&vault.address.raw[..20.min(vault.address.raw.len())]);
        let from = EthAddress(eth_addr);

        let mut tx = EthTx::transfer(EthChainId::MAINNET, 0, to, amount_wei)?;
        let mpc_key = blake3_hash_many(&[b"eth-mpc-key", &vault_id.to_le_bytes()]);
        tx.sign(&from, &mpc_key);
        let tx_hash = tx.hash();

        // Bakiye güncelle
        self.vaults.get_mut(&vault_id).unwrap().balance -= amount_wei;

        Ok(CrossChainTxReceipt {
            tx_hash,
            vault_id,
            recorded_slot: current_slot,
            signers: vault.threshold.guardians.clone(),
            success: true,
        })
    }

    /// Solana transferi
    pub fn send_solana(
        &self,
        vault_id: u64,
        to: SolanaPubkey,
        lamports: u64,
        current_slot: u64,
    ) -> GuardResult<CrossChainTxReceipt> {
        let vault = self.get_vault(vault_id)?;

        if vault.chain != ChainId::Solana {
            return Err(GuardError::UnsupportedChain {
                chain: format!("vault {:?} Solana değil", vault.chain),
            });
        }

        if vault.balance < lamports {
            return Err(GuardError::InsufficientBalance {
                available: vault.balance,
                required: lamports,
            });
        }

        let mut from_bytes = [0u8; 32];
        let raw_len = vault.address.raw.len().min(32);
        from_bytes[..raw_len].copy_from_slice(&vault.address.raw[..raw_len]);
        let from_pk = SolanaPubkey(from_bytes);

        let recent_bh = blake3_hash_many(&[b"recent-blockhash", &current_slot.to_le_bytes()]);
        let builder = SolanaTxBuilder::new(recent_bh);
        let mut tx = builder.sol_transfer(from_pk, to, lamports);

        let mpc_key = blake3_hash_many(&[b"sol-mpc-key", &vault_id.to_le_bytes()]);
        tx.add_mpc_signature(0, &mpc_key)?;
        let tx_hash = tx.tx_id();

        self.vaults.get_mut(&vault_id).unwrap().balance -= lamports;

        Ok(CrossChainTxReceipt {
            tx_hash,
            vault_id,
            recorded_slot: current_slot,
            signers: vault.threshold.guardians.clone(),
            success: true,
        })
    }

    /// Cosmos IBC transferi
    pub fn send_cosmos_ibc(
        &self,
        vault_id: u64,
        to: AssetAddress,
        amount_uatom: u64,
        denom: &str,
        current_slot: u64,
    ) -> GuardResult<CrossChainTxReceipt> {
        let vault = self.get_vault(vault_id)?;

        if vault.chain != ChainId::Cosmos {
            return Err(GuardError::UnsupportedChain {
                chain: format!("vault {:?} Cosmos değil", vault.chain),
            });
        }

        if vault.balance < amount_uatom {
            return Err(GuardError::InsufficientBalance {
                available: vault.balance,
                required: amount_uatom,
            });
        }

        let msg = IbcTransferMsg::new(
            "channel-0", denom, amount_uatom,
            vault.address.clone(), to, current_slot + 100,
        );
        let packet = self.ibc.send_transfer(msg)?;
        let tx_hash = packet.commitment_hash();

        self.vaults.get_mut(&vault_id).unwrap().balance -= amount_uatom;

        Ok(CrossChainTxReceipt {
            tx_hash,
            vault_id,
            recorded_slot: current_slot,
            signers: vault.threshold.guardians.clone(),
            success: true,
        })
    }

    /// Toplam vault sayısı
    pub fn vault_count(&self) -> usize {
        self.vaults.len()
    }

    /// IBC modülü
    pub fn ibc(&self) -> &IbcModule {
        &self.ibc
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> [u8; 20] {
        [0x01u8; 20]
    }

    fn guardians(n: usize) -> Vec<[u8; 20]> {
        (0..n).map(|i| [i as u8; 20]).collect()
    }

    fn make_coordinator() -> CrossChainCoordinator {
        CrossChainCoordinator::new()
    }

    #[test]
    fn test_create_btc_vault() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Bitcoin, 3, guardians(5), 100).unwrap();
        assert!(id >= 1);
        let vault = coord.get_vault(id).unwrap();
        assert_eq!(vault.chain, ChainId::Bitcoin);
        assert!(vault.address.display.starts_with("bc1q"));
    }

    #[test]
    fn test_create_eth_vault() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Ethereum, 2, guardians(3), 100).unwrap();
        let vault = coord.get_vault(id).unwrap();
        assert_eq!(vault.chain, ChainId::Ethereum);
        assert!(vault.address.display.starts_with("0x"));
    }

    #[test]
    fn test_create_sol_vault() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Solana, 2, guardians(3), 100).unwrap();
        let vault = coord.get_vault(id).unwrap();
        assert_eq!(vault.chain, ChainId::Solana);
    }

    #[test]
    fn test_create_cosmos_vault() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Cosmos, 2, guardians(3), 100).unwrap();
        let vault = coord.get_vault(id).unwrap();
        assert_eq!(vault.chain, ChainId::Cosmos);
    }

    #[test]
    fn test_vault_not_found() {
        let coord = make_coordinator();
        assert!(matches!(coord.get_vault(999), Err(GuardError::VaultNotFound { .. })));
    }

    #[test]
    fn test_deposit() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Bitcoin, 2, guardians(3), 1).unwrap();
        coord.deposit(id, 500_000).unwrap();
        let vault = coord.get_vault(id).unwrap();
        assert_eq!(vault.balance, 500_000);
    }

    #[test]
    fn test_mpc_sign_session() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Bitcoin, 2, guardians(3), 1).unwrap();

        let msg_hash = [0xAAu8; 32];
        let session_id = coord.initiate_sign(id, msg_hash).unwrap();

        // İlk guardian onaylar — eşik=2, henüz tamamlanmadı
        let complete1 = coord.guardian_approve(session_id, [0u8; 20]).unwrap();
        assert!(!complete1);

        // İkinci guardian onaylar — eşik sağlandı
        let complete2 = coord.guardian_approve(session_id, [1u8; 20]).unwrap();
        assert!(complete2);

        let sig = coord.get_signature(session_id).unwrap();
        assert_ne!(sig, [0u8; 32]);
    }

    #[test]
    fn test_send_bitcoin() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Bitcoin, 2, guardians(3), 1).unwrap();
        coord.deposit(id, 1_000_000).unwrap();

        let to = AssetAddress::bitcoin("bc1qrecipient");
        let receipt = coord.send_bitcoin(id, &to, 100_000, 5, 50).unwrap();

        assert!(receipt.success);
        assert_eq!(receipt.vault_id, id);
        assert_ne!(receipt.tx_hash, [0u8; 32]);

        // Bakiye güncellendi mi?
        let vault = coord.get_vault(id).unwrap();
        assert_eq!(vault.balance, 900_000);
    }

    #[test]
    fn test_send_ethereum() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Ethereum, 2, guardians(3), 1).unwrap();
        coord.deposit(id, 1_000_000_000_000_000_000u64).unwrap();

        let to = EthAddress([0xFFu8; 20]);
        let receipt = coord.send_ethereum(id, to, 500_000_000_000_000_000, 100).unwrap();

        assert!(receipt.success);
        assert_ne!(receipt.tx_hash, [0u8; 32]);
    }

    #[test]
    fn test_send_solana() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Solana, 2, guardians(3), 1).unwrap();
        coord.deposit(id, 2_000_000_000).unwrap();

        let to = SolanaPubkey([0xBBu8; 32]);
        let receipt = coord.send_solana(id, to, 1_000_000_000, 100).unwrap();

        assert!(receipt.success);
    }

    #[test]
    fn test_send_cosmos_ibc() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Cosmos, 2, guardians(3), 1).unwrap();
        coord.deposit(id, 5_000_000).unwrap();

        let to = AssetAddress::cosmos([0x99u8; 20]);
        let receipt = coord.send_cosmos_ibc(id, to, 1_000_000, "uatom", 100).unwrap();

        assert!(receipt.success);
        assert_eq!(coord.ibc().pending_count(), 1);
    }

    #[test]
    fn test_send_wrong_chain() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Bitcoin, 2, guardians(3), 1).unwrap();
        coord.deposit(id, 1_000_000).unwrap();

        let to = EthAddress([0xFFu8; 20]);
        let result = coord.send_ethereum(id, to, 100, 1);
        assert!(matches!(result, Err(GuardError::UnsupportedChain { .. })));
    }

    #[test]
    fn test_insufficient_balance() {
        let coord = make_coordinator();
        let id = coord.create_vault(owner(), ChainId::Bitcoin, 2, guardians(3), 1).unwrap();
        coord.deposit(id, 1_000).unwrap();

        let to = AssetAddress::bitcoin("bc1qtest");
        let result = coord.send_bitcoin(id, &to, 100_000, 5, 1);
        assert!(matches!(result, Err(GuardError::InsufficientBalance { .. })));
    }

    #[test]
    fn test_eth_vault_different_guardians_different_address() {
        // BUG-18: farklı guardian set → farklı vault adresi (artık doğru)
        let coord = make_coordinator();
        let gs1 = vec![[0x10u8; 20], [0x11u8; 20], [0x12u8; 20]];
        let gs2 = vec![[0x20u8; 20], [0x21u8; 20], [0x22u8; 20]];

        let id1 = coord.create_vault(owner(), ChainId::Ethereum, 2, gs1, 1).unwrap();
        let id2 = coord.create_vault(owner(), ChainId::Ethereum, 2, gs2, 1).unwrap();

        let v1 = coord.get_vault(id1).unwrap();
        let v2 = coord.get_vault(id2).unwrap();
        assert_ne!(v1.address.raw, v2.address.raw, "farklı guardian seti → farklı adres olmalı");
    }

    #[test]
    fn test_sol_vault_different_guardians_different_address() {
        let coord = make_coordinator();
        let gs1 = vec![[0x10u8; 20], [0x11u8; 20], [0x12u8; 20]];
        let gs2 = vec![[0x30u8; 20], [0x31u8; 20], [0x32u8; 20]];

        let id1 = coord.create_vault(owner(), ChainId::Solana, 2, gs1, 1).unwrap();
        let id2 = coord.create_vault(owner(), ChainId::Solana, 2, gs2, 1).unwrap();

        let v1 = coord.get_vault(id1).unwrap();
        let v2 = coord.get_vault(id2).unwrap();
        assert_ne!(v1.address.raw, v2.address.raw);
    }

    #[test]
    fn test_eth_vault_same_guardians_different_order_same_address() {
        // Guardian sırası değişse de adres değişmemeli (deterministik sıralama)
        let coord = make_coordinator();
        let gs1 = vec![[0x10u8; 20], [0x11u8; 20], [0x12u8; 20]];
        let gs2 = vec![[0x12u8; 20], [0x10u8; 20], [0x11u8; 20]]; // aynı, farklı sıra

        let id1 = coord.create_vault(owner(), ChainId::Ethereum, 2, gs1, 1).unwrap();
        let id2 = coord.create_vault(owner(), ChainId::Ethereum, 2, gs2, 1).unwrap();

        let v1 = coord.get_vault(id1).unwrap();
        let v2 = coord.get_vault(id2).unwrap();
        assert_eq!(v1.address.raw, v2.address.raw, "aynı guardian seti → aynı adres olmalı");
    }

    #[test]
    fn test_vault_count() {
        let coord = make_coordinator();
        for chain in [ChainId::Bitcoin, ChainId::Ethereum, ChainId::Solana, ChainId::Cosmos] {
            coord.create_vault(owner(), chain, 2, guardians(3), 1).unwrap();
        }
        assert_eq!(coord.vault_count(), 4);
    }
}
