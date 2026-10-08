//! GÖREV 94: XCM Cross-Chain Messaging (Polkadot XCM pattern)
//!
//! ## Mimari
//!
//! ### Polkadot XCM (Cross-Consensus Message Format)
//! - `Xcm<T>`: instruction listesi
//! - `MultiLocation`: zincirler arası kaynak/hedef adresleme
//! - `MultiAsset`: token türü + miktar
//! - Instruction seti: WithdrawAsset, BuyExecution, DepositAsset, Transact...
//! - `XcmExecutor`: instruction'ları sırayla çalıştırır
//!
//! ### Nova Uyarlaması
//! - `XcmLocation`: parachain_id + iç adres adresleme
//! - `XcmAsset`: token türü (Nova, Relay, Voucher) + miktar
//! - `XcmInstruction`: temel instruction seti
//! - `XcmMessage`: tam mesaj (instructions + weight limit)
//! - `XcmRouter`: mesajları doğru zincire yönlendirir
//! - `XcmExecutor`: yerel instruction'ları çalıştırır

use blake3;
use nova_primitives::Address;
use std::collections::HashMap;
use std::sync::Mutex;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Maksimum XCM instruction sayısı
pub const MAX_XCM_INSTRUCTIONS: usize = 32;

/// Maksimum XCM mesaj boyutu (bayt)
pub const MAX_XCM_MESSAGE_SIZE: usize = 64 * 1024;

/// Nova parachain ID
pub const NOVA_PARACHAIN_ID: u32 = 1000;

/// Relay chain ID (Polkadot = 0)
pub const RELAY_CHAIN_ID: u32 = 0;

/// Varsayılan XCM weight limiti
pub const DEFAULT_XCM_WEIGHT: u64 = 1_000_000_000;

// ─── XcmLocation ──────────────────────────────────────────────────────────────

/// Zincir ve hesap konumu
///
/// Polkadot: `MultiLocation { parents, interior: Junctions }`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct XcmLocation {
    /// Kaç üst zincir yukarı çıkılır (0=aynı zincir, 1=relay, 2=relay'in relay'i)
    pub parents: u8,
    /// Hedef zincir ID (parachain)
    pub parachain_id: Option<u32>,
    /// Hedef hesap (None=zincirin sovereign account)
    pub account: Option<[u8; 20]>,
}

impl XcmLocation {
    /// Aynı zincirde bir hesap
    pub fn here(account: [u8; 20]) -> Self {
        Self { parents: 0, parachain_id: None, account: Some(account) }
    }

    /// Relay chain
    pub fn relay() -> Self {
        Self { parents: 1, parachain_id: None, account: None }
    }

    /// Başka bir parachain'de hesap
    pub fn parachain(parachain_id: u32, account: [u8; 20]) -> Self {
        Self { parents: 1, parachain_id: Some(parachain_id), account: Some(account) }
    }

    /// Başka bir parachain'in sovereign account
    pub fn parachain_sovereign(parachain_id: u32) -> Self {
        Self { parents: 1, parachain_id: Some(parachain_id), account: None }
    }
}

// ─── XcmAsset ─────────────────────────────────────────────────────────────────

/// XCM varlığı (token türü + miktar)
///
/// Polkadot: `MultiAsset { id: AssetId, fun: Fungibility::Fungible(amount) }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XcmAsset {
    pub asset_id: XcmAssetId,
    pub amount: u128,
}

impl XcmAsset {
    pub fn new(asset_id: XcmAssetId, amount: u128) -> Self {
        Self { asset_id, amount }
    }

    pub fn native(amount: u128) -> Self {
        Self { asset_id: XcmAssetId::Native, amount }
    }
}

/// Varlık türü tanımlayıcısı
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum XcmAssetId {
    /// Nova native token (NSC)
    Native,
    /// Relay chain token (DOT/KSM)
    Relay,
    /// Başka bir parachain'in tokeni
    Parachain(u32),
    /// IBC voucher (denom hash)
    Voucher([u8; 32]),
}

// ─── XcmInstruction ───────────────────────────────────────────────────────────

/// XCM instruction
///
/// Polkadot: `xcm::v3::Instruction` enum (30+ varyant), Nova subset
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XcmInstruction {
    /// Gönderenden varlık çek
    WithdrawAsset { asset: XcmAsset },

    /// Varlık depozit et (teslim et)
    DepositAsset { asset: XcmAsset, beneficiary: XcmLocation },

    /// Mesajı ilet (başka zincire)
    InitiateTransfer { dest: XcmLocation, assets: Vec<XcmAsset>, remote_xcm: Box<XcmMessage> },

    /// Gas satın al (weight)
    BuyExecution { fees: XcmAsset, weight_limit: u64 },

    /// Arbitrary call (Transact)
    Transact {
        origin_kind: OriginKind,
        require_weight_at_most: u64,
        call: Vec<u8>,
    },

    /// Yerel zincire mesaj gönder (downward)
    ReceiveTeleportedAsset { asset: XcmAsset },

    /// Başarı bildirimi gönder
    ReportHolding { query_id: u64, dest: XcmLocation },

    /// Hepsini depozit et
    DepositReserveAsset { asset: XcmAsset, dest: XcmLocation },
}

/// Transact origin türü
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginKind {
    Native,
    SovereignAccount,
    Superuser,
    Xcm,
}

// ─── XcmMessage ───────────────────────────────────────────────────────────────

/// Tam XCM mesajı
///
/// Polkadot: `VersionedXcm::V3(Xcm(instructions))`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XcmMessage {
    /// XCM versiyon
    pub version: u8,
    /// Instruction listesi
    pub instructions: Vec<XcmInstruction>,
    /// Maksimum weight
    pub weight_limit: u64,
    /// Mesaj hash'i (routing için)
    pub message_hash: [u8; 32],
}

impl XcmMessage {
    pub fn new(instructions: Vec<XcmInstruction>) -> Self {
        let hash = Self::compute_hash(&instructions);
        Self {
            version: 3,
            instructions,
            weight_limit: DEFAULT_XCM_WEIGHT,
            message_hash: hash,
        }
    }

    pub fn with_weight(mut self, weight: u64) -> Self {
        self.weight_limit = weight;
        self
    }

    /// İçerik hash'i
    pub fn compute_hash(instructions: &[XcmInstruction]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-xcm-msg:");
        for (i, inst) in instructions.iter().enumerate() {
            hasher.update(&(i as u32).to_le_bytes());
            // Instruction tipini ve veri encode et
            match inst {
                XcmInstruction::WithdrawAsset { asset } => {
                    hasher.update(&[0u8]);
                    hasher.update(&asset.amount.to_le_bytes());
                }
                XcmInstruction::DepositAsset { asset, beneficiary } => {
                    hasher.update(&[1u8]);
                    hasher.update(&asset.amount.to_le_bytes());
                    // NOVA-RED-XCM-001: beneficiary dahil edilmeli
                    if let Some(pid) = beneficiary.parachain_id {
                        hasher.update(&pid.to_le_bytes());
                    }
                    if let Some(acc) = &beneficiary.account {
                        hasher.update(acc);
                    }
                }
                XcmInstruction::InitiateTransfer { dest, assets, .. } => {
                    hasher.update(&[2u8]);
                    for a in assets { hasher.update(&a.amount.to_le_bytes()); }
                    // NOVA-RED-XCM-001: hedef zincir dahil edilmeli
                    if let Some(pid) = dest.parachain_id {
                        hasher.update(&pid.to_le_bytes());
                    }
                    if let Some(acc) = &dest.account {
                        hasher.update(acc);
                    }
                }
                XcmInstruction::BuyExecution { fees, weight_limit } => {
                    hasher.update(&[3u8]);
                    hasher.update(&fees.amount.to_le_bytes());
                    hasher.update(&weight_limit.to_le_bytes());
                }
                XcmInstruction::Transact { call, require_weight_at_most, .. } => {
                    hasher.update(&[4u8]);
                    hasher.update(&require_weight_at_most.to_le_bytes());
                    hasher.update(call);
                }
                XcmInstruction::ReceiveTeleportedAsset { asset } => {
                    hasher.update(&[5u8]);
                    hasher.update(&asset.amount.to_le_bytes());
                }
                XcmInstruction::ReportHolding { query_id, dest } => {
                    hasher.update(&[6u8]);
                    hasher.update(&query_id.to_le_bytes());
                    // NOVA-RED-XCM-001: dest dahil edilmeli
                    if let Some(pid) = dest.parachain_id {
                        hasher.update(&pid.to_le_bytes());
                    }
                    if let Some(acc) = &dest.account {
                        hasher.update(acc);
                    }
                }
                XcmInstruction::DepositReserveAsset { asset, dest } => {
                    hasher.update(&[7u8]);
                    hasher.update(&asset.amount.to_le_bytes());
                    // NOVA-RED-XCM-001: dest dahil edilmeli
                    if let Some(pid) = dest.parachain_id {
                        hasher.update(&pid.to_le_bytes());
                    }
                    if let Some(acc) = &dest.account {
                        hasher.update(acc);
                    }
                }
            }
        }
        *hasher.finalize().as_bytes()
    }

    /// Kaç instruction var?
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }
}

// ─── XcmOutcome ───────────────────────────────────────────────────────────────

/// XCM yürütme sonucu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XcmOutcome {
    /// Başarıyla tamamlandı
    Complete { weight_used: u64 },
    /// Kısmen tamamlandı (bazı instruction'lar başarısız)
    Incomplete { weight_used: u64, error: XcmError },
    /// Tamamen başarısız
    Error(XcmError),
}

impl XcmOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, XcmOutcome::Complete { .. })
    }
}

/// XCM hata türleri
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XcmError {
    /// Yetersiz bakiye
    InsufficientFunds,
    /// Weight limiti aşıldı
    WeightLimitReached,
    /// Hedef bulunamadı
    DestinationNotFound,
    /// Çok fazla instruction
    TooManyInstructions,
    /// Geçersiz mesaj
    InvalidMessage,
    /// Yetkisiz origin
    UnauthorizedOrigin,
    /// Mesaj boyutu aşıldı
    MessageTooLarge,
}

// ─── XcmHolding ───────────────────────────────────────────────────────────────

/// Execution sırasındaki varlık tutucusu
///
/// Polkadot: `xcm-executor` `Assets` holding register
#[derive(Debug, Default)]
struct XcmHolding {
    assets: HashMap<XcmAssetId, u128>,
}

impl XcmHolding {
    fn deposit(&mut self, asset: &XcmAsset) {
        *self.assets.entry(asset.asset_id.clone()).or_insert(0) += asset.amount;
    }

    fn withdraw(&mut self, asset: &XcmAsset) -> bool {
        let balance = self.assets.entry(asset.asset_id.clone()).or_insert(0);
        if *balance >= asset.amount {
            *balance -= asset.amount;
            true
        } else {
            false
        }
    }

    fn balance(&self, asset_id: &XcmAssetId) -> u128 {
        *self.assets.get(asset_id).unwrap_or(&0)
    }
}

// ─── XcmExecutor ──────────────────────────────────────────────────────────────

/// XCM instruction yürütücü (yerel)
///
/// Polkadot: `xcm-executor::XcmExecutor`
pub struct XcmExecutor {
    /// Toplam kullanılan weight
    weight_used: u64,
    /// Yürütülen instruction sayısı
    executed: usize,
}

impl XcmExecutor {
    pub fn new() -> Self {
        Self { weight_used: 0, executed: 0 }
    }

    /// XCM mesajını yürüt
    pub fn execute(&mut self, message: &XcmMessage, origin_balance: u128) -> XcmOutcome {
        if message.instructions.len() > MAX_XCM_INSTRUCTIONS {
            return XcmOutcome::Error(XcmError::TooManyInstructions);
        }

        let mut holding = XcmHolding::default();
        let mut weight_remaining = message.weight_limit;

        // Origin'in native token'ını holding'e ekle
        holding.deposit(&XcmAsset::native(origin_balance));

        for instruction in &message.instructions {
            let inst_weight = self.estimate_weight(instruction);

            if inst_weight > weight_remaining {
                return XcmOutcome::Incomplete {
                    weight_used: self.weight_used,
                    error: XcmError::WeightLimitReached,
                };
            }

            match self.execute_instruction(instruction, &mut holding) {
                Ok(()) => {
                    self.weight_used += inst_weight;
                    weight_remaining -= inst_weight;
                    self.executed += 1;
                }
                Err(e) => {
                    return XcmOutcome::Incomplete {
                        weight_used: self.weight_used,
                        error: e,
                    };
                }
            }
        }

        XcmOutcome::Complete { weight_used: self.weight_used }
    }

    fn execute_instruction(&self, inst: &XcmInstruction, holding: &mut XcmHolding) -> Result<(), XcmError> {
        match inst {
            XcmInstruction::WithdrawAsset { asset } => {
                if !holding.withdraw(asset) {
                    return Err(XcmError::InsufficientFunds);
                }
            }
            XcmInstruction::DepositAsset { asset, .. } => {
                holding.deposit(asset);
            }
            XcmInstruction::BuyExecution { fees, .. } => {
                if !holding.withdraw(fees) {
                    return Err(XcmError::InsufficientFunds);
                }
            }
            XcmInstruction::ReceiveTeleportedAsset { asset } => {
                holding.deposit(asset);
            }
            XcmInstruction::Transact { .. } => {
                // Mock: başarıyla çalışır
            }
            XcmInstruction::ReportHolding { .. } => {
                // Mock: bildirim gönder
            }
            XcmInstruction::InitiateTransfer { assets, .. } => {
                for asset in assets {
                    if !holding.withdraw(asset) {
                        return Err(XcmError::InsufficientFunds);
                    }
                }
            }
            XcmInstruction::DepositReserveAsset { asset, .. } => {
                holding.deposit(asset);
            }
        }
        Ok(())
    }

    fn estimate_weight(&self, inst: &XcmInstruction) -> u64 {
        match inst {
            XcmInstruction::WithdrawAsset { .. } => 1_000,
            XcmInstruction::DepositAsset { .. } => 1_000,
            XcmInstruction::BuyExecution { .. } => 500,
            XcmInstruction::Transact { require_weight_at_most, .. } => *require_weight_at_most,
            XcmInstruction::InitiateTransfer { .. } => 5_000,
            XcmInstruction::ReceiveTeleportedAsset { .. } => 2_000,
            XcmInstruction::ReportHolding { .. } => 1_500,
            XcmInstruction::DepositReserveAsset { .. } => 2_500,
        }
    }

    pub fn executed_count(&self) -> usize {
        self.executed
    }

    pub fn weight_used(&self) -> u64 {
        self.weight_used
    }
}

impl Default for XcmExecutor {
    fn default() -> Self {
        Self::new()
    }
}

// ─── XcmRouter ────────────────────────────────────────────────────────────────

/// XCM mesajlarını doğru hedefe yönlendiren router
///
/// Polkadot: `xcm-builder::ParachainXcmRouter`
pub struct XcmRouter {
    /// Gönderilen mesajlar: dest parachain_id → mesaj listesi
    sent: Mutex<HashMap<u32, Vec<XcmMessage>>>,
    /// Toplam gönderilen mesaj sayısı
    total_sent: Mutex<u64>,
}

impl XcmRouter {
    pub fn new() -> Self {
        Self {
            sent: Mutex::new(HashMap::new()),
            total_sent: Mutex::new(0),
        }
    }

    /// Mesajı hedefe yönlendir
    pub fn send(&self, dest: &XcmLocation, message: XcmMessage) -> Result<(), XcmError> {
        // Mesaj boyut kontrolü
        if message.instructions.len() > MAX_XCM_INSTRUCTIONS {
            return Err(XcmError::TooManyInstructions);
        }

        let dest_id = dest.parachain_id.unwrap_or(RELAY_CHAIN_ID);

        let mut sent = self.sent.lock().unwrap();
        sent.entry(dest_id).or_default().push(message);
        *self.total_sent.lock().unwrap() += 1;

        Ok(())
    }

    /// Belirli bir zincire gönderilen mesajları al
    pub fn messages_for(&self, parachain_id: u32) -> Vec<XcmMessage> {
        self.sent.lock().unwrap()
            .get(&parachain_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Toplam gönderilen mesaj sayısı
    pub fn total_sent(&self) -> u64 {
        *self.total_sent.lock().unwrap()
    }

    /// Belirli bir zincire giden mesaj sayısı
    pub fn message_count(&self, parachain_id: u32) -> usize {
        self.messages_for(parachain_id).len()
    }
}

impl Default for XcmRouter {
    fn default() -> Self {
        Self::new()
    }
}

// ─── XcmChannel ───────────────────────────────────────────────────────────────

/// Çift yönlü XCM kanalı (iki parachain arasında)
///
/// Polkadot: HRMP (Horizontal Relay-routed Message Passing)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XcmChannel {
    /// Kaynak parachain ID
    pub sender: u32,
    /// Hedef parachain ID
    pub recipient: u32,
    /// Kanal açık mı?
    pub is_open: bool,
    /// Maksimum kapasite (bayt)
    pub max_capacity: usize,
}

impl XcmChannel {
    pub fn open(sender: u32, recipient: u32) -> Self {
        Self {
            sender,
            recipient,
            is_open: true,
            max_capacity: MAX_XCM_MESSAGE_SIZE,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer_msg(amount: u128, to: [u8; 20]) -> XcmMessage {
        XcmMessage::new(vec![
            XcmInstruction::WithdrawAsset { asset: XcmAsset::native(amount) },
            XcmInstruction::BuyExecution { fees: XcmAsset::native(1000), weight_limit: 1_000_000 },
            XcmInstruction::DepositAsset {
                asset: XcmAsset::native(amount - 1000),
                beneficiary: XcmLocation::here(to),
            },
        ])
    }

    // 1. XcmLocation oluşturma
    #[test]
    fn test_xcm_location_variants() {
        let here = XcmLocation::here([1u8; 20]);
        assert_eq!(here.parents, 0);
        assert!(here.parachain_id.is_none());

        let relay = XcmLocation::relay();
        assert_eq!(relay.parents, 1);

        let para = XcmLocation::parachain(2000, [2u8; 20]);
        assert_eq!(para.parachain_id, Some(2000));
    }

    // 2. XcmAsset
    #[test]
    fn test_xcm_asset_native() {
        let a = XcmAsset::native(1_000_000);
        assert_eq!(a.asset_id, XcmAssetId::Native);
        assert_eq!(a.amount, 1_000_000);
    }

    // 3. XcmMessage hash deterministik
    #[test]
    fn test_xcm_message_hash_deterministic() {
        let m1 = transfer_msg(1_000_000, [5u8; 20]);
        let m2 = transfer_msg(1_000_000, [5u8; 20]);
        assert_eq!(m1.message_hash, m2.message_hash);
    }

    // 4. Farklı mesajlar farklı hash
    #[test]
    fn test_xcm_message_hash_different() {
        let m1 = transfer_msg(1_000, [1u8; 20]);
        let m2 = transfer_msg(2_000, [1u8; 20]);
        assert_ne!(m1.message_hash, m2.message_hash);
    }

    // 5. Başarılı transfer execution
    #[test]
    fn test_xcm_executor_transfer_success() {
        let msg = transfer_msg(100_000, [9u8; 20]);
        let mut executor = XcmExecutor::new();
        let outcome = executor.execute(&msg, 200_000);
        assert!(outcome.is_success());
        assert_eq!(executor.executed_count(), 3);
    }

    // 6. Yetersiz bakiye → InsufficientFunds
    #[test]
    fn test_xcm_executor_insufficient_funds() {
        let msg = transfer_msg(1_000_000, [1u8; 20]);
        let mut executor = XcmExecutor::new();
        let outcome = executor.execute(&msg, 500); // çok az bakiye
        assert!(!outcome.is_success());
        assert!(matches!(outcome, XcmOutcome::Incomplete { error: XcmError::InsufficientFunds, .. }));
    }

    // 7. Çok fazla instruction
    #[test]
    fn test_xcm_executor_too_many_instructions() {
        let instructions: Vec<XcmInstruction> = (0..=MAX_XCM_INSTRUCTIONS)
            .map(|_| XcmInstruction::WithdrawAsset { asset: XcmAsset::native(1) })
            .collect();
        let msg = XcmMessage::new(instructions);
        let mut executor = XcmExecutor::new();
        let outcome = executor.execute(&msg, 1_000_000_000);
        assert!(matches!(outcome, XcmOutcome::Error(XcmError::TooManyInstructions)));
    }

    // 8. Weight limit
    #[test]
    fn test_xcm_executor_weight_limit() {
        let msg = XcmMessage::new(vec![
            XcmInstruction::WithdrawAsset { asset: XcmAsset::native(1000) },
        ]).with_weight(100); // çok düşük weight limit
        let mut executor = XcmExecutor::new();
        let outcome = executor.execute(&msg, 10_000);
        assert!(!outcome.is_success());
    }

    // 9. XcmRouter mesaj yönlendirme
    #[test]
    fn test_xcm_router_send() {
        let router = XcmRouter::new();
        let dest = XcmLocation::parachain(2000, [0u8; 20]);
        let msg = transfer_msg(5_000, [0u8; 20]);

        router.send(&dest, msg.clone()).unwrap();
        router.send(&dest, msg.clone()).unwrap();

        assert_eq!(router.message_count(2000), 2);
        assert_eq!(router.total_sent(), 2);
    }

    // 10. XcmRouter farklı destinasyonlar
    #[test]
    fn test_xcm_router_multiple_destinations() {
        let router = XcmRouter::new();
        router.send(&XcmLocation::parachain_sovereign(1001), transfer_msg(1000, [0u8; 20])).unwrap();
        router.send(&XcmLocation::parachain_sovereign(1002), transfer_msg(2000, [0u8; 20])).unwrap();
        router.send(&XcmLocation::parachain_sovereign(1001), transfer_msg(3000, [0u8; 20])).unwrap();

        assert_eq!(router.message_count(1001), 2);
        assert_eq!(router.message_count(1002), 1);
        assert_eq!(router.total_sent(), 3);
    }

    // 11. XcmChannel aç/kapat
    #[test]
    fn test_xcm_channel() {
        let channel = XcmChannel::open(NOVA_PARACHAIN_ID, 2000);
        assert!(channel.is_open);
        assert_eq!(channel.sender, NOVA_PARACHAIN_ID);
        assert_eq!(channel.recipient, 2000);
    }

    // 12. ReceiveTeleportedAsset
    #[test]
    fn test_receive_teleported_asset() {
        let msg = XcmMessage::new(vec![
            XcmInstruction::ReceiveTeleportedAsset { asset: XcmAsset::native(50_000) },
            XcmInstruction::DepositAsset {
                asset: XcmAsset::native(50_000),
                beneficiary: XcmLocation::here([1u8; 20]),
            },
        ]);
        let mut executor = XcmExecutor::new();
        let outcome = executor.execute(&msg, 0); // bakiye teleported asset'tan gelir
        assert!(outcome.is_success());
    }

    // 13. Transact instruction
    #[test]
    fn test_transact_instruction() {
        let msg = XcmMessage::new(vec![
            XcmInstruction::Transact {
                origin_kind: OriginKind::SovereignAccount,
                require_weight_at_most: 100_000,
                call: vec![0x01, 0x02, 0x03],
            },
        ]);
        let mut executor = XcmExecutor::new();
        let outcome = executor.execute(&msg, 0);
        assert!(outcome.is_success());
        assert_eq!(executor.weight_used(), 100_000);
    }

    // 14. InitiateTransfer
    #[test]
    fn test_initiate_transfer() {
        let remote_msg = XcmMessage::new(vec![
            XcmInstruction::DepositAsset {
                asset: XcmAsset::native(10_000),
                beneficiary: XcmLocation::here([9u8; 20]),
            },
        ]);
        let msg = XcmMessage::new(vec![
            XcmInstruction::InitiateTransfer {
                dest: XcmLocation::parachain(2000, [0u8; 20]),
                assets: vec![XcmAsset::native(10_000)],
                remote_xcm: Box::new(remote_msg),
            },
        ]);
        let mut executor = XcmExecutor::new();
        let outcome = executor.execute(&msg, 50_000);
        assert!(outcome.is_success());
    }
}
