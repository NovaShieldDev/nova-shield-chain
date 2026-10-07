//! Deadman switch (miras planı) SDK
//!
//! §5.4 — Nova Shield Deadman Protocol
//! Kullanım kolaylığı için yüksek seviye API:
//! - `DeadmanPlanBuilder`: plan oluştur
//! - `HeartbeatSender`: heartbeat gönder

use crate::errors::{SdkError, SdkResult};
use serde::{Deserialize, Serialize};

// ─── ReleaseStep ──────────────────────────────────────────────────────────────

/// Kademeli serbest bırakma adımı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseStep {
    /// Önceki adımdan gecikme (slot)
    pub delay_slots: u64,
    /// Bu adımda serbest bırakılacak oran (bps, 10000=100%)
    pub fraction_bps: u32,
    /// Alıcı
    pub recipient: [u8; 20],
}

// ─── TriggerConfig ────────────────────────────────────────────────────────────

/// Tetikleyici konfigürasyonu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// İnaktivite süresi (saniye) — §5.4.2 min 30 gün
    pub inactivity_period_secs: u64,
    /// Uyarı süresi (saniye) — §5.4.4
    pub warning_period_secs: u64,
    /// Grace period (saniye) — §5.4.5
    pub grace_period_secs: u64,
}

impl TriggerConfig {
    /// Varsayılan: 90 gün inaktivite, 30 gün uyarı, 30 gün grace
    pub fn default_90_days() -> Self {
        Self {
            inactivity_period_secs: 90 * 24 * 3600,
            warning_period_secs: 30 * 24 * 3600,
            grace_period_secs: 30 * 24 * 3600,
        }
    }

    /// Minimum trigger süresi (30 gün)
    pub const MIN_INACTIVITY_SECS: u64 = 30 * 24 * 3600;

    pub fn validate(&self) -> SdkResult<()> {
        if self.inactivity_period_secs < Self::MIN_INACTIVITY_SECS {
            return Err(SdkError::InvalidPlan {
                reason: format!(
                    "inaktivite süresi minimum {} saniye olmalı",
                    Self::MIN_INACTIVITY_SECS
                ),
            });
        }
        Ok(())
    }
}

// ─── DeadmanPlan ──────────────────────────────────────────────────────────────

/// Deadman switch planı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadmanPlan {
    pub plan_id: u64,
    pub owner: [u8; 20],
    pub trigger: TriggerConfig,
    pub release_steps: Vec<ReleaseStep>,
    pub created_at_slot: u64,
    pub active: bool,
}

impl DeadmanPlan {
    /// Toplam pay 10000 bps mı?
    pub fn validate_fractions(&self) -> SdkResult<()> {
        let total: u32 = self.release_steps.iter().map(|s| s.fraction_bps).sum();
        if total != 10_000 {
            return Err(SdkError::InvalidPlan {
                reason: format!("toplam pay {} bps, 10000 olmalı", total),
            });
        }
        Ok(())
    }

    /// Toplam mirasçı sayısı
    pub fn heir_count(&self) -> usize {
        self.release_steps.len()
    }
}

// ─── DeadmanPlanBuilder ───────────────────────────────────────────────────────

/// Deadman planı inşaatçısı — builder pattern
pub struct DeadmanPlanBuilder {
    owner: [u8; 20],
    trigger: TriggerConfig,
    steps: Vec<ReleaseStep>,
    plan_id_counter: u64,
}

impl DeadmanPlanBuilder {
    pub fn new(owner: [u8; 20]) -> Self {
        Self {
            owner,
            trigger: TriggerConfig::default_90_days(),
            steps: vec![],
            plan_id_counter: 1,
        }
    }

    /// Tetikleyici ayarla
    pub fn with_trigger(mut self, trigger: TriggerConfig) -> Self {
        self.trigger = trigger;
        self
    }

    /// Mirasçı adımı ekle
    pub fn add_heir(
        mut self,
        recipient: [u8; 20],
        fraction_bps: u32,
        delay_slots: u64,
    ) -> Self {
        self.steps.push(ReleaseStep { delay_slots, fraction_bps, recipient });
        self
    }

    /// Planı oluştur
    pub fn build(self, current_slot: u64) -> SdkResult<DeadmanPlan> {
        self.trigger.validate()?;

        if self.steps.is_empty() {
            return Err(SdkError::InvalidPlan {
                reason: "en az 1 mirasçı gerekli".into(),
            });
        }

        let plan = DeadmanPlan {
            plan_id: self.plan_id_counter,
            owner: self.owner,
            trigger: self.trigger,
            release_steps: self.steps,
            created_at_slot: current_slot,
            active: true,
        };

        plan.validate_fractions()?;
        Ok(plan)
    }
}

// ─── HeartbeatMsg ─────────────────────────────────────────────────────────────

/// Heartbeat mesajı — §5.4.3
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMsg {
    /// Hesap adresi
    pub account: [u8; 20],
    /// Plan ID
    pub plan_id: u64,
    /// Slot numarası
    pub slot: u64,
    /// Baskı altında mı? (duress)
    pub is_duress: bool,
    /// İmza (BLAKE3 mock)
    pub signature: [u8; 32],
}

// ─── HeartbeatSender ──────────────────────────────────────────────────────────

/// Heartbeat gönderici
pub struct HeartbeatSender {
    account: [u8; 20],
    private_key: [u8; 32],
    last_sent_slot: Option<u64>,
}

impl HeartbeatSender {
    pub fn new(account: [u8; 20], private_key: [u8; 32]) -> Self {
        Self { account, private_key, last_sent_slot: None }
    }

    /// Normal heartbeat gönder
    pub fn send_heartbeat(&mut self, plan_id: u64, slot: u64) -> HeartbeatMsg {
        self.last_sent_slot = Some(slot);
        self.build_msg(plan_id, slot, false)
    }

    /// Baskı altında heartbeat (duress — §5.4.3)
    pub fn send_duress_heartbeat(&mut self, plan_id: u64, slot: u64) -> HeartbeatMsg {
        self.last_sent_slot = Some(slot);
        self.build_msg(plan_id, slot, true)
    }

    fn build_msg(&self, plan_id: u64, slot: u64, is_duress: bool) -> HeartbeatMsg {
        let mut h = blake3::Hasher::new();
        h.update(b"nova-heartbeat-v1");
        h.update(&self.account);
        h.update(&plan_id.to_le_bytes());
        h.update(&slot.to_le_bytes());
        h.update(&[is_duress as u8]);
        h.update(&self.private_key);
        let signature = *h.finalize().as_bytes();

        HeartbeatMsg {
            account: self.account,
            plan_id,
            slot,
            is_duress,
            signature,
        }
    }

    pub fn last_sent_slot(&self) -> Option<u64> {
        self.last_sent_slot
    }

    /// Heartbeat doğrula
    pub fn verify_heartbeat(&self, msg: &HeartbeatMsg) -> bool {
        let expected = self.build_msg(msg.plan_id, msg.slot, msg.is_duress);
        expected.signature == msg.signature
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> [u8; 20] {
        [0x01u8; 20]
    }

    fn heir(n: u8) -> [u8; 20] {
        [n; 20]
    }

    #[test]
    fn test_trigger_config_validate_ok() {
        let t = TriggerConfig::default_90_days();
        assert!(t.validate().is_ok());
    }

    #[test]
    fn test_trigger_config_too_short() {
        let t = TriggerConfig {
            inactivity_period_secs: 7 * 24 * 3600, // 7 gün — çok kısa
            warning_period_secs: 1 * 24 * 3600,
            grace_period_secs: 1 * 24 * 3600,
        };
        assert!(matches!(t.validate(), Err(SdkError::InvalidPlan { .. })));
    }

    #[test]
    fn test_plan_builder_basic() {
        let plan = DeadmanPlanBuilder::new(owner())
            .add_heir(heir(1), 6_000, 100)
            .add_heir(heir(2), 4_000, 200)
            .build(1000)
            .unwrap();

        assert!(plan.active);
        assert_eq!(plan.heir_count(), 2);
    }

    #[test]
    fn test_plan_builder_no_heirs() {
        let result = DeadmanPlanBuilder::new(owner()).build(100);
        assert!(matches!(result, Err(SdkError::InvalidPlan { .. })));
    }

    #[test]
    fn test_plan_builder_invalid_fractions() {
        let result = DeadmanPlanBuilder::new(owner())
            .add_heir(heir(1), 3_000, 100) // toplam 3000 != 10000
            .build(100);
        assert!(matches!(result, Err(SdkError::InvalidPlan { .. })));
    }

    #[test]
    fn test_plan_validate_fractions() {
        let plan = DeadmanPlanBuilder::new(owner())
            .add_heir(heir(1), 5_000, 100)
            .add_heir(heir(2), 3_000, 200)
            .add_heir(heir(3), 2_000, 300)
            .build(100)
            .unwrap();

        plan.validate_fractions().unwrap();
    }

    #[test]
    fn test_heartbeat_sender_normal() {
        let mut sender = HeartbeatSender::new(owner(), [0x42u8; 32]);
        let msg = sender.send_heartbeat(1, 500);

        assert_eq!(msg.account, owner());
        assert!(!msg.is_duress);
        assert_eq!(sender.last_sent_slot(), Some(500));
    }

    #[test]
    fn test_heartbeat_sender_duress() {
        let mut sender = HeartbeatSender::new(owner(), [0x42u8; 32]);
        let msg = sender.send_duress_heartbeat(1, 600);
        assert!(msg.is_duress);
    }

    #[test]
    fn test_heartbeat_verify() {
        let sender = HeartbeatSender::new(owner(), [0x11u8; 32]);
        let mut s2 = HeartbeatSender::new(owner(), [0x11u8; 32]);
        let msg = s2.send_heartbeat(1, 700);
        assert!(sender.verify_heartbeat(&msg));
    }

    #[test]
    fn test_heartbeat_tampered() {
        let sender = HeartbeatSender::new(owner(), [0x11u8; 32]);
        let mut s2 = HeartbeatSender::new(owner(), [0x11u8; 32]);
        let mut msg = s2.send_heartbeat(1, 800);
        msg.slot = 999; // tamper
        assert!(!sender.verify_heartbeat(&msg));
    }

    #[test]
    fn test_heartbeat_different_keys() {
        let s1 = HeartbeatSender::new(owner(), [0x11u8; 32]);
        let mut s2 = HeartbeatSender::new(owner(), [0x22u8; 32]);
        let msg = s2.send_heartbeat(1, 900);
        assert!(!s1.verify_heartbeat(&msg));
    }
}
