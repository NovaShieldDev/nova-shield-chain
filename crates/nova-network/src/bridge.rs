//! GÖREV 86: IBC Token Bridging (Cosmos SDK ICS-20 pattern)
//!
//! ## Mimari
//!
//! ### Native vs Bridged Token Ayrımı (Cosmos SDK pattern)
//! - Native token: Nova zincirinde üretilmiş (NSC ve WASM kontratlı tokenler)
//! - Bridged token: Başka zincirden IBC ile köprülenmiş → voucher formatı
//!   `ibc/{channel_id}/{port_id}/{denom}` → hash ile kısaltma: `ibc/{hash}`
//!
//! ### ICS-20 Token Transfer (Cosmos IBC)
//! - `FungibleTokenPacket`: denom, amount, sender, receiver
//! - Escrow pattern: kaynak zincirde kilitle → hedef zincirde mint
//! - Unescrow pattern: hedef zincirde yak → kaynak zincirde serbest bırak
//!
//! ### Channel ve Port
//! - Port: `transfer` (ICS-20 standartı)
//! - Channel: iki zincir arasındaki bağlantı (channel-0, channel-1, ...)
//! - PacketCommitment: gönderilen paketin hash'i (kaybolma koruması)

use std::collections::BTreeMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// IBC token transfer port adı (ICS-20 standartı)
pub const IBC_TRANSFER_PORT: &str = "transfer";

/// IBC voucher prefix
pub const IBC_PREFIX: &str = "ibc/";

/// Maksimum denom uzunluğu
pub const MAX_DENOM_LENGTH: usize = 128;

/// Nova'nın native token denom'u
pub const NOVA_NATIVE_DENOM: &str = "unsc"; // micro-NSC

// ─── DenomTrace ───────────────────────────────────────────────────────────────

/// ICS-20 DenomTrace — köprülenmiş token'ın kaynağını takip eder
///
/// Cosmos SDK: `types.DenomTrace { Path, BaseDenom }`
/// - Path: "port1/channel1/port2/channel2"
/// - BaseDenom: orijinal token adı (ör. "atom", "eth")
/// - IBCDenom(): ibc/{SHA256(path/baseDenom)[0:32] hex}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DenomTrace {
    /// Zincirler arası yol: "transfer/channel-0/transfer/channel-1"
    pub path: String,
    /// Kaynak token adı: "atom", "eth", "usdc"
    pub base_denom: String,
}

impl DenomTrace {
    pub fn new(path: impl Into<String>, base_denom: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            base_denom: base_denom.into(),
        }
    }

    /// IBC voucher denom'u üret: `ibc/{hash}`
    /// Hash: BLAKE3(path + "/" + base_denom) ilk 32 byte hex
    pub fn ibc_denom(&self) -> String {
        let full = if self.path.is_empty() {
            self.base_denom.clone()
        } else {
            format!("{}/{}", self.path, self.base_denom)
        };
        let hash = blake3::hash(full.as_bytes());
        format!("ibc/{}", hex::encode(&hash.as_bytes()[..16])) // 32 hex chars
    }

    /// Bu token native mi?
    pub fn is_native(&self) -> bool {
        self.path.is_empty()
    }

    /// Bir hop geri döndür (unwrap path)
    pub fn remove_prefix(&self, port: &str, channel: &str) -> Option<DenomTrace> {
        let prefix_with_slash = format!("{port}/{channel}/");
        let prefix_exact = format!("{port}/{channel}");
        if self.path.starts_with(&prefix_with_slash) {
            // Çok hop: "transfer/channel-0/transfer/channel-1" → "transfer/channel-1"
            let new_path = self.path[prefix_with_slash.len()..].to_string();
            Some(DenomTrace::new(new_path, &self.base_denom))
        } else if self.path == prefix_exact {
            // Tek hop: "transfer/channel-0" → ""
            Some(DenomTrace::new("", &self.base_denom))
        } else {
            None
        }
    }

    /// Bir hop ekle (wrap path)
    pub fn add_prefix(&self, port: &str, channel: &str) -> DenomTrace {
        let new_path = if self.path.is_empty() {
            format!("{port}/{channel}")
        } else {
            format!("{port}/{channel}/{}", self.path)
        };
        DenomTrace::new(new_path, &self.base_denom)
    }
}

// ─── IBC Packet ───────────────────────────────────────────────────────────────

/// ICS-20 FungibleTokenPacket
///
/// Cosmos SDK: `types.FungibleTokenPacketData`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FungibleTokenPacket {
    /// Token denom (native veya IBC voucher)
    pub denom: String,
    /// Transfer miktarı (en küçük birim)
    pub amount: u128,
    /// Gönderici adresi (kaynak zincirde)
    pub sender: String,
    /// Alıcı adresi (hedef zincirde)
    pub receiver: String,
    /// Opsiyonel memo
    pub memo: String,
}

impl FungibleTokenPacket {
    pub fn new(
        denom: impl Into<String>,
        amount: u128,
        sender: impl Into<String>,
        receiver: impl Into<String>,
    ) -> Self {
        Self {
            denom: denom.into(),
            amount,
            sender: sender.into(),
            receiver: receiver.into(),
            memo: String::new(),
        }
    }

    /// Paket hash'i: PacketCommitment için
    pub fn commitment_hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.denom.as_bytes());
        hasher.update(&self.amount.to_le_bytes());
        hasher.update(self.sender.as_bytes());
        hasher.update(self.receiver.as_bytes());
        *hasher.finalize().as_bytes()
    }
}

// ─── IBC Channel ──────────────────────────────────────────────────────────────

/// IBC Channel durumu (ICS-4)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelState {
    /// Kanal açılıyor (henüz aktif değil)
    Init,
    /// Karşı zincirin onayı bekleniyor
    TryOpen,
    /// Kanal aktif
    Open,
    /// Kanal kapatılıyor
    Closing,
    /// Kanal kapalı
    Closed,
}

/// IBC Channel
#[derive(Debug, Clone)]
pub struct IbcChannel {
    pub channel_id: String,
    pub port_id: String,
    pub state: ChannelState,
    /// Karşı zincirin channel ID'si
    pub counterparty_channel_id: String,
    /// Karşı zincirin port ID'si
    pub counterparty_port_id: String,
    /// Bağlı olduğu connection
    pub connection_id: String,
    /// Sıradaki gönderilecek paket sekans numarası
    pub next_sequence_send: u64,
    /// Sıradaki alınacak paket sekans numarası
    pub next_sequence_recv: u64,
}

impl IbcChannel {
    pub fn new(
        channel_id: impl Into<String>,
        counterparty_channel_id: impl Into<String>,
        connection_id: impl Into<String>,
    ) -> Self {
        Self {
            channel_id: channel_id.into(),
            port_id: IBC_TRANSFER_PORT.into(),
            state: ChannelState::Open,
            counterparty_channel_id: counterparty_channel_id.into(),
            counterparty_port_id: IBC_TRANSFER_PORT.into(),
            connection_id: connection_id.into(),
            next_sequence_send: 1,
            next_sequence_recv: 1,
        }
    }
}

// ─── EscrowAccount ────────────────────────────────────────────────────────────

/// ICS-20 Escrow account — gönderilen native tokenler burada tutulur
///
/// Cosmos SDK: her channel için ayrı escrow adresi
/// `types.GetEscrowAddress(portID, channelID)` → deterministik adres
#[derive(Debug, Default)]
pub struct EscrowAccount {
    /// denom → kilitli miktar
    balances: BTreeMap<String, u128>,
}

impl EscrowAccount {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tokenları kilitle (gönderim)
    pub fn lock(&mut self, denom: &str, amount: u128) -> Result<(), BridgeError> {
        *self.balances.entry(denom.to_string()).or_insert(0) += amount;
        Ok(())
    }

    /// Tokenları serbest bırak (geri alım)
    pub fn unlock(&mut self, denom: &str, amount: u128) -> Result<(), BridgeError> {
        let balance = self.balances.get_mut(denom)
            .ok_or(BridgeError::InsufficientEscrowBalance { denom: denom.into(), amount })?;
        if *balance < amount {
            return Err(BridgeError::InsufficientEscrowBalance { denom: denom.into(), amount });
        }
        *balance -= amount;
        Ok(())
    }

    pub fn balance(&self, denom: &str) -> u128 {
        self.balances.get(denom).copied().unwrap_or(0)
    }
}

// ─── VoucherMinter ────────────────────────────────────────────────────────────

/// IBC Voucher mint/burn kaydı
/// Gerçek implementasyonda bank modülü ile entegre olur
#[derive(Debug, Default)]
pub struct VoucherMinter {
    /// ibc_denom → toplam mintlenen miktar
    minted: BTreeMap<String, u128>,
    /// Kullanıcı → (ibc_denom → bakiye)
    user_balances: BTreeMap<String, BTreeMap<String, u128>>,
}

impl VoucherMinter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Voucher mint et (gelen köprü)
    pub fn mint(&mut self, receiver: &str, ibc_denom: &str, amount: u128) {
        *self.minted.entry(ibc_denom.to_string()).or_insert(0) += amount;
        *self.user_balances
            .entry(receiver.to_string())
            .or_default()
            .entry(ibc_denom.to_string())
            .or_insert(0) += amount;
    }

    /// Voucher yak (giden köprü)
    pub fn burn(&mut self, sender: &str, ibc_denom: &str, amount: u128) -> Result<(), BridgeError> {
        let user_denoms = self.user_balances.get_mut(sender)
            .ok_or(BridgeError::InsufficientBalance { user: sender.into(), denom: ibc_denom.into(), amount })?;
        let balance = user_denoms.get_mut(ibc_denom)
            .ok_or(BridgeError::InsufficientBalance { user: sender.into(), denom: ibc_denom.into(), amount })?;
        if *balance < amount {
            return Err(BridgeError::InsufficientBalance { user: sender.into(), denom: ibc_denom.into(), amount });
        }
        *balance -= amount;
        // NOVA-RED-BRIDGE-001: unwrap() panik riski → graceful error
        if let Some(total) = self.minted.get_mut(ibc_denom) {
            *total = total.saturating_sub(amount);
        }
        Ok(())
    }

    pub fn total_supply(&self, ibc_denom: &str) -> u128 {
        self.minted.get(ibc_denom).copied().unwrap_or(0)
    }

    pub fn user_balance(&self, user: &str, ibc_denom: &str) -> u128 {
        self.user_balances.get(user)
            .and_then(|d| d.get(ibc_denom))
            .copied()
            .unwrap_or(0)
    }
}

// ─── IbcTransferModule ────────────────────────────────────────────────────────

/// ICS-20 Token Transfer Modülü
///
/// Cosmos SDK: `x/ibc/apps/transfer/keeper`
/// - send_transfer: kaynak zincirde kilitle, paket gönder
/// - on_recv_packet: hedef zincirde mintla veya kilidi aç
/// - on_ack_packet / on_timeout_packet: hata durumlarını yönet
pub struct IbcTransferModule {
    /// Açık kanallar
    channels: BTreeMap<String, IbcChannel>,
    /// Her channel için escrow
    escrows: BTreeMap<String, EscrowAccount>,
    /// Voucher minter
    minter: VoucherMinter,
    /// DenomTrace kaydı: ibc_denom → trace
    denom_traces: BTreeMap<String, DenomTrace>,
    /// PacketCommitment: gönderilen paketlerin hash'i (kaybolma tespiti)
    packet_commitments: BTreeMap<(String, u64), [u8; 32]>,
}

impl IbcTransferModule {
    pub fn new() -> Self {
        Self {
            channels: BTreeMap::new(),
            escrows: BTreeMap::new(),
            minter: VoucherMinter::new(),
            denom_traces: BTreeMap::new(),
            packet_commitments: BTreeMap::new(),
        }
    }

    /// Kanal ekle
    pub fn register_channel(&mut self, channel: IbcChannel) {
        self.channels.insert(channel.channel_id.clone(), channel);
    }

    /// Token gönder (ICS-20 SendTransfer)
    ///
    /// Kaynak zincir davranışı:
    /// - Native token ise: escrow'a kilitle
    /// - Daha önce köprülenmiş voucher ise: yak (burn)
    pub fn send_transfer(
        &mut self,
        channel_id: &str,
        sender: &str,
        receiver: &str,
        denom: &str,
        amount: u128,
    ) -> Result<IbcPacketSent, BridgeError> {
        let channel = self.channels.get_mut(channel_id)
            .ok_or(BridgeError::ChannelNotFound(channel_id.into()))?;

        if channel.state != ChannelState::Open {
            return Err(BridgeError::ChannelClosed(channel_id.into()));
        }

        let sequence = channel.next_sequence_send;
        channel.next_sequence_send += 1;

        // Native mi yoksa voucher mi?
        // ICS-20: paket denom'u "transfer/{channel_id}/{base_denom}" formatındadır
        let packet_denom = if denom.starts_with(IBC_PREFIX) {
            // Voucher → yak (geri dönüyor)
            self.minter.burn(sender, denom, amount)?;
            // Karşı taraftaki orijinal denom'u bul (prefix sıyrılmış hali)
            if let Some(trace) = self.denom_traces.get(denom) {
                let counterparty_channel = self.channels[channel_id].counterparty_channel_id.clone();
                let unwrapped = trace.remove_prefix(IBC_TRANSFER_PORT, &counterparty_channel)
                    .unwrap_or_else(|| trace.clone());
                // Geri gönderilen voucher — base_denom olarak gönder
                unwrapped.base_denom.clone()
            } else {
                denom.to_string()
            }
        } else {
            // Native → escrow'a kilitle
            let escrow = self.escrows.entry(channel_id.to_string()).or_default();
            escrow.lock(denom, amount)?;
            // ICS-20: paket denom'u "transfer/{channel_id}/{denom}" formatı
            format!("{IBC_TRANSFER_PORT}/{channel_id}/{denom}")
        };

        let packet = FungibleTokenPacket::new(&packet_denom, amount, sender, receiver);
        let commitment = packet.commitment_hash();
        self.packet_commitments.insert((channel_id.to_string(), sequence), commitment);

        Ok(IbcPacketSent {
            channel_id: channel_id.to_string(),
            sequence,
            packet,
            commitment,
        })
    }

    /// Paket al (ICS-20 OnRecvPacket)
    ///
    /// Hedef zincir davranışı:
    /// - Denom kaynak channel prefix'i ile başlıyorsa: voucher mint et
    /// - Yoksa (geri dönüyor): escrow'dan serbest bırak
    pub fn on_recv_packet(
        &mut self,
        channel_id: &str,
        packet: &FungibleTokenPacket,
    ) -> Result<(), BridgeError> {
        let channel = self.channels.get(channel_id)
            .ok_or(BridgeError::ChannelNotFound(channel_id.into()))?;

        let counterparty_channel = channel.counterparty_channel_id.clone();
        let expected_prefix = format!("{IBC_TRANSFER_PORT}/{counterparty_channel}/");

        if packet.denom.starts_with(&expected_prefix) {
            // Kaynak zincirin prefix'i var → voucher mint et
            let trace = DenomTrace::new(
                format!("{IBC_TRANSFER_PORT}/{channel_id}"),
                packet.denom[expected_prefix.len()..].to_string(),
            );
            let ibc_denom = trace.ibc_denom();
            self.denom_traces.insert(ibc_denom.clone(), trace);
            self.minter.mint(&packet.receiver, &ibc_denom, packet.amount);
        } else {
            // Prefix yok → escrow'dan serbest bırak (nova token geri dönüyor)
            let escrow = self.escrows.entry(channel_id.to_string()).or_default();
            escrow.unlock(&packet.denom, packet.amount)?;
        }

        Ok(())
    }

    /// Timeout/Ack hatasında geri al
    pub fn on_timeout(&mut self, channel_id: &str, packet: &FungibleTokenPacket) -> Result<(), BridgeError> {
        // ICS-20: Native gönderimde paket denom "transfer/{channel_id}/{base_denom}" formatında
        let native_prefix = format!("{IBC_TRANSFER_PORT}/{channel_id}/");
        if packet.denom.starts_with(&native_prefix) {
            // Native idi, escrow'a kilitledik → kilidi aç
            let base_denom = &packet.denom[native_prefix.len()..];
            let escrow = self.escrows.entry(channel_id.to_string()).or_default();
            escrow.unlock(base_denom, packet.amount)?;
        } else if packet.denom.starts_with(IBC_PREFIX) {
            // Voucher idi, yaktık → geri mint et
            self.minter.mint(&packet.sender, &packet.denom, packet.amount);
        } else {
            // Base denom olarak kilitledik
            let escrow = self.escrows.entry(channel_id.to_string()).or_default();
            escrow.unlock(&packet.denom, packet.amount)?;
        }
        Ok(())
    }

    pub fn denom_trace(&self, ibc_denom: &str) -> Option<&DenomTrace> {
        self.denom_traces.get(ibc_denom)
    }

    pub fn escrow_balance(&self, channel_id: &str, denom: &str) -> u128 {
        self.escrows.get(channel_id).map(|e| e.balance(denom)).unwrap_or(0)
    }

    pub fn voucher_balance(&self, user: &str, ibc_denom: &str) -> u128 {
        self.minter.user_balance(user, ibc_denom)
    }

    pub fn total_voucher_supply(&self, ibc_denom: &str) -> u128 {
        self.minter.total_supply(ibc_denom)
    }
}

impl Default for IbcTransferModule {
    fn default() -> Self {
        Self::new()
    }
}

/// Gönderilen paketin sonucu
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IbcPacketSent {
    pub channel_id: String,
    pub sequence: u64,
    pub packet: FungibleTokenPacket,
    pub commitment: [u8; 32],
}

// ─── BridgeError ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    ChannelNotFound(String),
    ChannelClosed(String),
    InsufficientEscrowBalance { denom: String, amount: u128 },
    InsufficientBalance { user: String, denom: String, amount: u128 },
    InvalidDenom(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChannelNotFound(id) => write!(f, "kanal bulunamadı: {id}"),
            Self::ChannelClosed(id) => write!(f, "kanal kapalı: {id}"),
            Self::InsufficientEscrowBalance { denom, amount } => {
                write!(f, "escrow yetersiz: {denom} {amount}")
            }
            Self::InsufficientBalance { user, denom, amount } => {
                write!(f, "bakiye yetersiz: {user} {denom} {amount}")
            }
            Self::InvalidDenom(d) => write!(f, "geçersiz denom: {d}"),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> IbcTransferModule {
        let mut module = IbcTransferModule::new();
        module.register_channel(IbcChannel::new("channel-0", "channel-1", "connection-0"));
        module
    }

    // 1. Native token gönderimi → escrow'a kilitle
    #[test]
    fn send_native_locks_in_escrow() {
        let mut module = setup();
        let result = module.send_transfer("channel-0", "alice", "cosmos1bob", "unsc", 1_000_000);
        assert!(result.is_ok(), "gönderim başarısız: {:?}", result);
        assert_eq!(module.escrow_balance("channel-0", "unsc"), 1_000_000);
    }

    // 2. Gelen native token → voucher mint
    #[test]
    fn recv_packet_mints_voucher() {
        let mut module = setup();
        let packet = FungibleTokenPacket::new(
            "transfer/channel-1/uatom",
            500_000,
            "cosmos1alice",
            "nova1bob",
        );
        module.on_recv_packet("channel-0", &packet).unwrap();

        // Voucher oluşturulmalı
        let trace = DenomTrace::new("transfer/channel-0", "uatom");
        let ibc_denom = trace.ibc_denom();
        assert_eq!(module.voucher_balance("nova1bob", &ibc_denom), 500_000);
        assert_eq!(module.total_voucher_supply(&ibc_denom), 500_000);
    }

    // 3. Voucher gönderimi → burn + orijinal denom ile paket
    #[test]
    fn send_voucher_burns_and_sends() {
        let mut module = setup();

        // Önce voucher mint et
        let trace = DenomTrace::new("transfer/channel-0", "uatom");
        let ibc_denom = trace.ibc_denom();
        module.minter.mint("nova1alice", &ibc_denom, 300_000);
        module.denom_traces.insert(ibc_denom.clone(), trace);

        // Voucher'ı geri gönder
        let result = module.send_transfer("channel-0", "nova1alice", "cosmos1bob", &ibc_denom, 200_000);
        assert!(result.is_ok(), "voucher gönderimi başarısız: {:?}", result);
        assert_eq!(module.voucher_balance("nova1alice", &ibc_denom), 100_000);
    }

    // 4. Escrow'a kilitlenmiş token geri dönüşü
    #[test]
    fn recv_packet_unlocks_escrow() {
        let mut module = setup();

        // Önce kilitle
        module.escrows.entry("channel-0".to_string()).or_default().lock("unsc", 1_000_000).unwrap();

        // Geri dön (prefix yok = nova token dönüyor)
        let packet = FungibleTokenPacket::new("unsc", 500_000, "cosmos1alice", "nova1bob");
        module.on_recv_packet("channel-0", &packet).unwrap();

        assert_eq!(module.escrow_balance("channel-0", "unsc"), 500_000);
    }

    // 5. Bilinmeyen kanal → hata
    #[test]
    fn unknown_channel_returns_error() {
        let mut module = setup();
        let result = module.send_transfer("channel-99", "alice", "bob", "unsc", 100);
        assert_eq!(result, Err(BridgeError::ChannelNotFound("channel-99".into())));
    }

    // 6. Yetersiz escrow → hata
    #[test]
    fn insufficient_escrow_returns_error() {
        let mut module = setup();
        module.escrows.entry("channel-0".to_string()).or_default().lock("unsc", 100).unwrap();

        let packet = FungibleTokenPacket::new("unsc", 200, "alice", "bob");
        let result = module.on_recv_packet("channel-0", &packet);
        assert!(matches!(result, Err(BridgeError::InsufficientEscrowBalance { .. })));
    }

    // 7. Timeout → geri al
    #[test]
    fn timeout_refunds_sender() {
        let mut module = setup();

        // Native gönder → kilitle
        let sent = module.send_transfer("channel-0", "nova1alice", "cosmos1bob", "unsc", 1_000_000).unwrap();
        assert_eq!(module.escrow_balance("channel-0", "unsc"), 1_000_000);

        // Paketin denom'u ICS-20 formatında: "transfer/channel-0/unsc"
        assert_eq!(sent.packet.denom, "transfer/channel-0/unsc");

        // Timeout → geri al
        module.on_timeout("channel-0", &sent.packet).unwrap();
        assert_eq!(module.escrow_balance("channel-0", "unsc"), 0);
    }

    // 8. DenomTrace IBC denom üretimi deterministik
    #[test]
    fn denom_trace_deterministic() {
        let trace = DenomTrace::new("transfer/channel-0", "uatom");
        let d1 = trace.ibc_denom();
        let d2 = trace.ibc_denom();
        assert_eq!(d1, d2);
        assert!(d1.starts_with("ibc/"));
    }

    // 9. DenomTrace prefix ekleme/çıkarma
    #[test]
    fn denom_trace_prefix_roundtrip() {
        let original = DenomTrace::new("", "uatom");
        let wrapped = original.add_prefix("transfer", "channel-0");
        assert_eq!(wrapped.path, "transfer/channel-0");

        let unwrapped = wrapped.remove_prefix("transfer", "channel-0").unwrap();
        assert_eq!(unwrapped.path, "");
        assert_eq!(unwrapped.base_denom, "uatom");
    }

    // 10. Native token kontrolü
    #[test]
    fn native_denom_detection() {
        assert!(DenomTrace::new("", "unsc").is_native());
        assert!(!DenomTrace::new("transfer/channel-0", "uatom").is_native());
    }

    // 11. Sequence numarası artar
    #[test]
    fn sequence_increments_on_send() {
        let mut module = setup();
        let s1 = module.send_transfer("channel-0", "alice", "bob", "unsc", 100).unwrap();
        let s2 = module.send_transfer("channel-0", "alice", "bob", "unsc", 100).unwrap();
        assert_eq!(s1.sequence, 1);
        assert_eq!(s2.sequence, 2);
    }

    // 12. EscrowAccount doğrudan test
    #[test]
    fn escrow_lock_unlock() {
        let mut escrow = EscrowAccount::new();
        escrow.lock("unsc", 1000).unwrap();
        escrow.lock("unsc", 500).unwrap();
        assert_eq!(escrow.balance("unsc"), 1500);

        escrow.unlock("unsc", 300).unwrap();
        assert_eq!(escrow.balance("unsc"), 1200);

        let result = escrow.unlock("unsc", 2000);
        assert!(matches!(result, Err(BridgeError::InsufficientEscrowBalance { .. })));
    }
}
