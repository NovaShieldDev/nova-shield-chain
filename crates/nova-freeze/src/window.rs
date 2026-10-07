//! 72 saatlik soruşturma penceresi yönetimi — §5.5.3
//!
//! Araştırma kaynakları:
//! - Polkadot im-online `HeartbeatStatus { session_index, sent_at }`:
//!   "Bu heartbeat hâlâ geçerli mi?" sorusunu `is_recent()` ile yanıtlar.
//!   Bizde: "Bu soruşturma penceresi hâlâ açık mı?" → `is_open(current_slot)`.
//! - Cosmos SDK slashing `JailUntil(ctx, consAddr, jailTime)`:
//!   Temporal deadline yönetimi. Bizde: slot-tabanlı expiry.
//! - Polkadot im-online `on_before_session_ending()`:
//!   Session sonunda offline taraması. Bizde: `process_slot()` ile expired window taraması.
//!
//! İki pencere türü:
//! 1. `UserCancellationWindow` (24 saat): Guardian freeze sonrası kullanıcı iptal hakkı.
//! 2. `InvestigationWindow` (72 saat): Tam soruşturma süreci.

use crate::errors::{FreezeError, FreezeResult};
use crate::types::{FreezeId, INVESTIGATION_WINDOW_SLOTS, USER_CANCELLATION_WINDOW_SLOTS};
use dashmap::DashMap;
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── WindowType ───────────────────────────────────────────────────────────────

/// Soruşturma penceresi türü.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowType {
    /// 24 saatlik kullanıcı iptal penceresi.
    /// Guardian freeze sonrası kullanıcı 24 saat içinde iptal edebilir.
    UserCancellation,
    /// 72 saatlik tam soruşturma penceresi.
    /// Whitepaper §5.5.3 ana soruşturma süreci.
    FullInvestigation,
}

// ─── InvestigationWindow ─────────────────────────────────────────────────────

/// Açık bir soruşturma penceresi.
///
/// Pattern: Polkadot im-online `HeartbeatStatus`:
/// `{ session_index, sent_at }` → `{ freeze_id, started_at_slot, expires_at_slot }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationWindow {
    /// Bağlı freeze ID'si.
    pub freeze_id: FreezeId,
    /// Hesap adresi.
    pub account: Address,
    /// Pencerenin açıldığı slot.
    pub started_at_slot: SlotId,
    /// Pencerenin kapandığı slot.
    pub expires_at_slot: SlotId,
    /// Pencere türü.
    pub window_type: WindowType,
}

impl InvestigationWindow {
    /// Pencere hâlâ açık mı?
    /// Pattern: Polkadot im-online `is_recent(session_index, now)`.
    pub fn is_open(&self, current_slot: SlotId) -> bool {
        current_slot < self.expires_at_slot
    }

    /// Pencere doldu mu?
    pub fn is_expired(&self, current_slot: SlotId) -> bool {
        current_slot >= self.expires_at_slot
    }

    /// Kalan slot sayısı.
    pub fn remaining_slots(&self, current_slot: SlotId) -> u64 {
        if current_slot >= self.expires_at_slot {
            0
        } else {
            self.expires_at_slot.0 - current_slot.0
        }
    }

    /// Kalan süre (saniye cinsinden, SLOT_DURATION_MS = 400ms).
    pub fn remaining_secs(&self, current_slot: SlotId) -> u64 {
        self.remaining_slots(current_slot) * 400 / 1000
    }
}

// ─── ExpiredWindowEvent ───────────────────────────────────────────────────────

/// Süresi dolmuş pencere eventi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpiredWindowEvent {
    /// Hesap adresi.
    pub account: Address,
    /// Freeze ID.
    pub freeze_id: FreezeId,
    /// Dolduğu slot.
    pub expired_at_slot: SlotId,
    /// Pencere türü.
    pub window_type: WindowType,
}

// ─── WindowManager ────────────────────────────────────────────────────────────

/// Soruşturma penceresi yöneticisi.
///
/// Pattern: Cosmos SDK slashing Keeper'ı — per-address state yönetimi.
/// DashMap ile thread-safe, lock-free concurrent erişim.
pub struct WindowManager {
    windows: Arc<DashMap<Address, InvestigationWindow>>,
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            windows: Arc::new(DashMap::new()),
        }
    }

    /// Kullanıcı iptal penceresi aç (24 saat).
    pub fn open_user_cancellation_window(
        &self,
        account: Address,
        freeze_id: FreezeId,
        slot: SlotId,
    ) -> InvestigationWindow {
        let window = InvestigationWindow {
            freeze_id,
            account,
            started_at_slot: slot,
            expires_at_slot: SlotId(slot.0 + USER_CANCELLATION_WINDOW_SLOTS),
            window_type: WindowType::UserCancellation,
        };
        self.windows.insert(account, window.clone());
        window
    }

    /// Tam soruşturma penceresi aç (72 saat).
    pub fn open_investigation_window(
        &self,
        account: Address,
        freeze_id: FreezeId,
        slot: SlotId,
    ) -> InvestigationWindow {
        let window = InvestigationWindow {
            freeze_id,
            account,
            started_at_slot: slot,
            expires_at_slot: SlotId(slot.0 + INVESTIGATION_WINDOW_SLOTS),
            window_type: WindowType::FullInvestigation,
        };
        self.windows.insert(account, window.clone());
        window
    }

    /// Hesap için açık pencere var mı?
    pub fn has_window(&self, account: &Address) -> bool {
        self.windows.contains_key(account)
    }

    /// Pencereyi al.
    pub fn get_window(&self, account: &Address) -> Option<InvestigationWindow> {
        self.windows.get(account).map(|w| w.clone())
    }

    /// Pencere açık mı?
    pub fn is_open(&self, account: &Address, current_slot: SlotId) -> bool {
        self.windows
            .get(account)
            .map_or(false, |w| w.is_open(current_slot))
    }

    /// Pencereyi kapat.
    pub fn close_window(&self, account: &Address) -> Option<InvestigationWindow> {
        self.windows.remove(account).map(|(_, w)| w)
    }

    /// Slot işleminde süresi dolmuş pencereleri tara ve kapat.
    ///
    /// Pattern: Polkadot im-online `on_before_session_ending()` —
    /// session sonunda offline olan'ları tara.
    /// Bizde: her slot'ta expired window'ları tespit et → handoff.
    pub fn process_slot(&self, current_slot: SlotId) -> Vec<ExpiredWindowEvent> {
        let mut expired = Vec::new();

        // Dolmuş pencereleri tespit et
        let expired_accounts: Vec<Address> = self
            .windows
            .iter()
            .filter(|entry| entry.is_expired(current_slot))
            .map(|entry| *entry.key())
            .collect();

        // Her dolmuş pencereyi kapat ve event üret
        for account in expired_accounts {
            if let Some(window) = self.windows.remove(&account).map(|(_, w)| w) {
                expired.push(ExpiredWindowEvent {
                    account,
                    freeze_id: window.freeze_id,
                    expired_at_slot: current_slot,
                    window_type: window.window_type,
                });
            }
        }

        expired
    }

    /// Tüm açık pencere sayısı.
    pub fn open_window_count(&self) -> usize {
        self.windows.len()
    }

    /// Açık pencere bilgisini doğrula (freeze_id eşleşmeli).
    pub fn validate_window(
        &self,
        account: &Address,
        freeze_id: &FreezeId,
        current_slot: SlotId,
    ) -> FreezeResult<InvestigationWindow> {
        let window = self
            .windows
            .get(account)
            .ok_or(FreezeError::NoInvestigationWindow)?;

        if window.is_expired(current_slot) {
            return Err(FreezeError::InvestigationWindowExpired {
                expired_at_slot: window.expires_at_slot.0,
            });
        }

        if &window.freeze_id != freeze_id {
            return Err(FreezeError::FreezeNotFound {
                freeze_id: crate::types::freeze_id_to_hex(freeze_id),
            });
        }

        Ok(window.clone())
    }
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_freeze_id(seed: u8) -> FreezeId {
        [seed; 32]
    }

    // ─── InvestigationWindow ──────────────────────────────────────────────────

    #[test]
    fn test_window_is_open_before_expiry() {
        let window = InvestigationWindow {
            freeze_id: make_freeze_id(1),
            account: addr(1),
            started_at_slot: SlotId(0),
            expires_at_slot: SlotId(INVESTIGATION_WINDOW_SLOTS),
            window_type: WindowType::FullInvestigation,
        };
        assert!(window.is_open(SlotId(0)));
        assert!(window.is_open(SlotId(INVESTIGATION_WINDOW_SLOTS - 1)));
        assert!(!window.is_open(SlotId(INVESTIGATION_WINDOW_SLOTS)));
    }

    #[test]
    fn test_window_remaining_slots() {
        let window = InvestigationWindow {
            freeze_id: make_freeze_id(1),
            account: addr(1),
            started_at_slot: SlotId(0),
            expires_at_slot: SlotId(1000),
            window_type: WindowType::FullInvestigation,
        };
        assert_eq!(window.remaining_slots(SlotId(0)), 1000);
        assert_eq!(window.remaining_slots(SlotId(400)), 600);
        assert_eq!(window.remaining_slots(SlotId(1000)), 0);
        assert_eq!(window.remaining_slots(SlotId(1001)), 0);
    }

    #[test]
    fn test_window_remaining_secs() {
        // SLOT_DURATION_MS = 400 → 1 slot = 0.4 sn
        let window = InvestigationWindow {
            freeze_id: make_freeze_id(1),
            account: addr(1),
            started_at_slot: SlotId(0),
            expires_at_slot: SlotId(1000), // 1000 slot = 400 sn
            window_type: WindowType::FullInvestigation,
        };
        assert_eq!(window.remaining_secs(SlotId(0)), 400);
        assert_eq!(window.remaining_secs(SlotId(500)), 200);
    }

    // ─── WindowManager ────────────────────────────────────────────────────────

    #[test]
    fn test_manager_open_cancellation_window_duration() {
        let manager = WindowManager::new();
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let slot = SlotId(1000);

        let window = manager.open_user_cancellation_window(account, freeze_id, slot);
        assert_eq!(window.window_type, WindowType::UserCancellation);
        assert_eq!(window.expires_at_slot.0, 1000 + USER_CANCELLATION_WINDOW_SLOTS);
    }

    #[test]
    fn test_manager_open_investigation_window_duration() {
        let manager = WindowManager::new();
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let slot = SlotId(5000);

        let window = manager.open_investigation_window(account, freeze_id, slot);
        assert_eq!(window.window_type, WindowType::FullInvestigation);
        assert_eq!(window.expires_at_slot.0, 5000 + INVESTIGATION_WINDOW_SLOTS);
    }

    #[test]
    fn test_manager_get_window() {
        let manager = WindowManager::new();
        let account = addr(1);
        let freeze_id = make_freeze_id(1);

        assert!(manager.get_window(&account).is_none());
        manager.open_investigation_window(account, freeze_id, SlotId(0));
        assert!(manager.get_window(&account).is_some());
    }

    #[test]
    fn test_manager_is_open() {
        let manager = WindowManager::new();
        let account = addr(1);
        let freeze_id = make_freeze_id(1);

        manager.open_investigation_window(account, freeze_id, SlotId(0));

        assert!(manager.is_open(&account, SlotId(100)));
        assert!(!manager.is_open(&account, SlotId(INVESTIGATION_WINDOW_SLOTS)));
    }

    #[test]
    fn test_manager_close_window() {
        let manager = WindowManager::new();
        let account = addr(1);
        let freeze_id = make_freeze_id(1);

        manager.open_investigation_window(account, freeze_id, SlotId(0));
        assert!(manager.has_window(&account));

        let closed = manager.close_window(&account);
        assert!(closed.is_some());
        assert!(!manager.has_window(&account));
    }

    #[test]
    fn test_manager_process_slot_closes_expired() {
        let manager = WindowManager::new();
        let a1 = addr(1);
        let a2 = addr(2);
        let fid1 = make_freeze_id(1);
        let fid2 = make_freeze_id(2);

        manager.open_investigation_window(a1, fid1, SlotId(0));
        manager.open_investigation_window(a2, fid2, SlotId(1_000_000));

        // İlk pencerenin expiry slot'unda işle
        let events = manager.process_slot(SlotId(INVESTIGATION_WINDOW_SLOTS));
        assert_eq!(events.len(), 1, "sadece ilk pencere dolmuş olmalı");
        assert_eq!(events[0].account, a1);
        assert!(!manager.has_window(&a1), "dolmuş pencere silinmeli");
        assert!(manager.has_window(&a2), "aktif pencere kalmalı");
    }

    #[test]
    fn test_manager_process_slot_no_expired() {
        let manager = WindowManager::new();
        let account = addr(1);
        manager.open_investigation_window(account, make_freeze_id(1), SlotId(0));

        let events = manager.process_slot(SlotId(100));
        assert!(events.is_empty(), "dolmamış pencerede event olmamalı");
    }

    #[test]
    fn test_manager_validate_window_success() {
        let manager = WindowManager::new();
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        manager.open_investigation_window(account, freeze_id, SlotId(0));

        let result = manager.validate_window(&account, &freeze_id, SlotId(100));
        assert!(result.is_ok());
    }

    #[test]
    fn test_manager_validate_window_no_window() {
        let manager = WindowManager::new();
        let err = manager.validate_window(&addr(1), &make_freeze_id(1), SlotId(100)).unwrap_err();
        assert!(matches!(err, FreezeError::NoInvestigationWindow));
    }

    #[test]
    fn test_manager_validate_window_expired() {
        let manager = WindowManager::new();
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        manager.open_investigation_window(account, freeze_id, SlotId(0));

        let err = manager
            .validate_window(&account, &freeze_id, SlotId(INVESTIGATION_WINDOW_SLOTS))
            .unwrap_err();
        assert!(matches!(err, FreezeError::InvestigationWindowExpired { .. }));
    }

    #[test]
    fn test_manager_validate_window_wrong_freeze_id() {
        let manager = WindowManager::new();
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let wrong_id = make_freeze_id(99);
        manager.open_investigation_window(account, freeze_id, SlotId(0));

        let err = manager.validate_window(&account, &wrong_id, SlotId(100)).unwrap_err();
        assert!(matches!(err, FreezeError::FreezeNotFound { .. }));
    }

    #[test]
    fn test_manager_open_window_count() {
        let manager = WindowManager::new();
        assert_eq!(manager.open_window_count(), 0);

        manager.open_investigation_window(addr(1), make_freeze_id(1), SlotId(0));
        manager.open_investigation_window(addr(2), make_freeze_id(2), SlotId(0));
        assert_eq!(manager.open_window_count(), 2);
    }
}
