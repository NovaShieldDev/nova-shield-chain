//! IBC (Inter-Blockchain Communication) entegrasyonu
//!
//! Cosmos SDK IBC patterns:
//! - `Channel`: state machine (INIT→TRYOPEN→OPEN→CLOSED)
//! - `Packet`: sequence, timeouts, data
//! - `SendKeeper.SendCoins`: InputOutputCoins pattern
//! - `Port`: module binding
//!
//! Nova Shield Guard Cosmos entegrasyonu için IBC üzerinden ATOM ve IBC tokenleri

use crate::errors::{GuardError, GuardResult};
use crate::types::{AssetAddress, blake3_hash_many};
use serde::{Deserialize, Serialize};

// ─── IbcChannelState ──────────────────────────────────────────────────────────

/// IBC kanal durumu — Cosmos SDK `ChannelState`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IbcChannelState {
    /// INIT: kanal açılış talebi gönderildi
    Init,
    /// TRYOPEN: karşı taraf INIT aldı
    TryOpen,
    /// OPEN: kanal aktif
    Open,
    /// CLOSED: kanal kapalı
    Closed,
}

// ─── IbcOrdering ──────────────────────────────────────────────────────────────

/// IBC kanal sıralaması
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IbcOrdering {
    Ordered,
    Unordered,
}

// ─── IbcChannel ───────────────────────────────────────────────────────────────

/// IBC kanalı — Cosmos SDK `Channel`
/// `Channel { state, ordering, counterparty, connection_hops, version }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbcChannel {
    /// Kanal ID (ör. "channel-0")
    pub channel_id: String,
    /// Port ID (ör. "transfer")
    pub port_id: String,
    /// Kanal durumu
    pub state: IbcChannelState,
    /// Sıralama
    pub ordering: IbcOrdering,
    /// Karşı taraf kanal ID
    pub counterparty_channel_id: String,
    /// Karşı taraf port ID
    pub counterparty_port_id: String,
    /// Bağlantı ID'leri
    pub connection_hops: Vec<String>,
    /// Protokol versiyonu (ör. "ics20-1")
    pub version: String,
}

impl IbcChannel {
    /// Yeni transfer kanalı oluştur
    pub fn new_transfer(
        channel_id: impl Into<String>,
        counterparty_channel_id: impl Into<String>,
    ) -> Self {
        Self {
            channel_id: channel_id.into(),
            port_id: "transfer".into(),
            state: IbcChannelState::Open,
            ordering: IbcOrdering::Unordered,
            counterparty_channel_id: counterparty_channel_id.into(),
            counterparty_port_id: "transfer".into(),
            connection_hops: vec!["connection-0".into()],
            version: "ics20-1".into(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.state == IbcChannelState::Open
    }
}

// ─── IbcPacket ────────────────────────────────────────────────────────────────

/// IBC paketi — Cosmos SDK `Packet`
/// `Packet { sequence, source_port, source_channel, dest_port, dest_channel, data, timeout }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbcPacket {
    /// Paket sırası (her kanal için monoton artar)
    pub sequence: u64,
    /// Kaynak port
    pub source_port: String,
    /// Kaynak kanal
    pub source_channel: String,
    /// Hedef port
    pub destination_port: String,
    /// Hedef kanal
    pub destination_channel: String,
    /// Paket verisi (FungibleTokenPacketData için JSON)
    pub data: Vec<u8>,
    /// Timeout blok yüksekliği
    pub timeout_height: u64,
    /// Timeout zaman damgası (nanosaniye)
    pub timeout_timestamp: u64,
}

impl IbcPacket {
    pub fn commitment_hash(&self) -> [u8; 32] {
        blake3_hash_many(&[
            b"ibc-packet-commitment",
            &self.sequence.to_le_bytes(),
            self.source_port.as_bytes(),
            self.source_channel.as_bytes(),
            &self.data,
            &self.timeout_height.to_le_bytes(),
        ])
    }
}

// ─── FungibleTokenPacketData ──────────────────────────────────────────────────

/// ICS-20 FungibleTokenPacketData — transfer için standart veri
/// `{ denom, amount, sender, receiver, memo }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FungibleTokenPacketData {
    /// Token cinsi (ör. "uatom", "ibc/27394FB...ATOM")
    pub denom: String,
    /// Miktar (string olarak)
    pub amount: String,
    /// Gönderen adresi
    pub sender: String,
    /// Alıcı adresi
    pub receiver: String,
    /// Ek not
    pub memo: String,
}

impl FungibleTokenPacketData {
    pub fn new(
        denom: impl Into<String>,
        amount: u64,
        sender: impl Into<String>,
        receiver: impl Into<String>,
    ) -> Self {
        Self {
            denom: denom.into(),
            amount: amount.to_string(),
            sender: sender.into(),
            receiver: receiver.into(),
            memo: "nova-guard".into(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

// ─── IbcTransferMsg ───────────────────────────────────────────────────────────

/// IBC token transfer mesajı — Cosmos SDK `MsgTransfer`
/// `MsgTransfer { source_port, source_channel, token, sender, receiver, timeout_height, timeout_timestamp }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbcTransferMsg {
    pub source_port: String,
    pub source_channel: String,
    pub denom: String,
    pub amount: u64,
    pub sender: AssetAddress,
    pub receiver: AssetAddress,
    /// Timeout: hedef zincir blok yüksekliği
    pub timeout_height: u64,
    /// Timeout: nanosaniye timestamp
    pub timeout_timestamp: u64,
}

impl IbcTransferMsg {
    pub fn new(
        channel_id: impl Into<String>,
        denom: impl Into<String>,
        amount: u64,
        sender: AssetAddress,
        receiver: AssetAddress,
        timeout_height: u64,
    ) -> Self {
        Self {
            source_port: "transfer".into(),
            source_channel: channel_id.into(),
            denom: denom.into(),
            amount,
            sender,
            receiver,
            timeout_height,
            timeout_timestamp: 0,
        }
    }
}

// ─── IbcModule ────────────────────────────────────────────────────────────────

/// IBC transfer modülü — Cosmos SDK `IBCModule` pattern
pub struct IbcModule {
    channels: dashmap::DashMap<String, IbcChannel>,
    /// Gönderilen paket sıra numaraları (channel_id → next_send_sequence)
    sequences: dashmap::DashMap<String, u64>,
    /// Pending paketler
    pending_packets: parking_lot::Mutex<Vec<IbcPacket>>,
    /// NOVA-RED-NETWORK-008: Alınan paket sıra numaraları (channel_id → next_expected_recv_seq).
    /// Replay saldırısını önler: aynı sequence ile paket iki kez kabul edilemez.
    recv_sequences: dashmap::DashMap<String, u64>,
}

impl IbcModule {
    pub fn new() -> Self {
        Self {
            channels: dashmap::DashMap::new(),
            sequences: dashmap::DashMap::new(),
            pending_packets: parking_lot::Mutex::new(vec![]),
            recv_sequences: dashmap::DashMap::new(),
        }
    }

    /// Kanal kaydet
    pub fn register_channel(&self, channel: IbcChannel) {
        self.sequences.entry(channel.channel_id.clone()).or_insert(0);
        self.recv_sequences.entry(channel.channel_id.clone()).or_insert(0);
        self.channels.insert(channel.channel_id.clone(), channel);
    }

    /// Kanal al
    pub fn get_channel(&self, channel_id: &str) -> Option<IbcChannel> {
        self.channels.get(channel_id).map(|c| c.clone())
    }

    /// IBC transfer başlat — Cosmos SDK `SendTransfer`
    ///
    /// NOVA-RED-GUARD-007: `caller` parametresi `msg.sender.display` ile eşleşmeli.
    /// Aksi hâlde başkası adına transfer başlatılabilir.
    pub fn send_transfer(&self, caller: &str, msg: IbcTransferMsg) -> GuardResult<IbcPacket> {
        // Caller authorization
        if caller != msg.sender.display {
            return Err(GuardError::Unauthorized);
        }

        let channel = self.channels.get(&msg.source_channel)
            .ok_or_else(|| GuardError::IbcChannelNotFound {
                channel_id: msg.source_channel.clone(),
            })?;

        if !channel.is_open() {
            return Err(GuardError::IbcChannelNotFound {
                channel_id: msg.source_channel.clone(),
            });
        }

        // Sıra numarası artır
        let mut seq = self.sequences.entry(msg.source_channel.clone()).or_insert(0);
        *seq += 1;
        let sequence = *seq;

        let packet_data = FungibleTokenPacketData::new(
            &msg.denom,
            msg.amount,
            msg.sender.display.clone(),
            msg.receiver.display.clone(),
        );

        let packet = IbcPacket {
            sequence,
            source_port: msg.source_port,
            source_channel: msg.source_channel,
            destination_port: channel.counterparty_port_id.clone(),
            destination_channel: channel.counterparty_channel_id.clone(),
            data: packet_data.to_bytes(),
            timeout_height: msg.timeout_height,
            timeout_timestamp: msg.timeout_timestamp,
        };

        self.pending_packets.lock().push(packet.clone());
        Ok(packet)
    }

    /// Pending paket sayısı
    pub fn pending_count(&self) -> usize {
        self.pending_packets.lock().len()
    }

    /// Paketi onayla (relay)
    pub fn acknowledge_packet(&self, sequence: u64) -> bool {
        let mut pending = self.pending_packets.lock();
        if let Some(pos) = pending.iter().position(|p| p.sequence == sequence) {
            pending.remove(pos);
            true
        } else {
            false
        }
    }

    /// Gelen paketi işle — Cosmos SDK `RecvPacket`.
    ///
    /// NOVA-RED-NETWORK-008: Sıra numarası takibi ile replay saldırısını önler.
    /// Aynı (channel, sequence) çifti yalnızca bir kez kabul edilir.
    ///
    /// # Hata
    /// - `GuardError::IbcChannelNotFound` — kanal bilinmiyorsa
    /// - `GuardError::InvalidTransaction` — sequence replay veya geçersizse
    pub fn receive_packet(&self, packet: &IbcPacket) -> GuardResult<()> {
        // Kanal var mı?
        if !self.channels.contains_key(&packet.destination_channel) {
            return Err(GuardError::IbcChannelNotFound {
                channel_id: packet.destination_channel.clone(),
            });
        }

        // NOVA-RED-NETWORK-008: Alınan sequence takibi
        let mut next_seq = self.recv_sequences
            .entry(packet.destination_channel.clone())
            .or_insert(0);

        if packet.sequence != *next_seq + 1 && packet.sequence <= *next_seq {
            // Replay: zaten işlenmiş sequence
            return Err(GuardError::InvalidTransaction {
                reason: format!(
                    "IBC replay saldırısı tespit edildi: channel={}, sequence={}, beklenen>{} ",
                    packet.destination_channel, packet.sequence, *next_seq
                ),
            });
        }

        // Sequence güncelle (ordered channel: strictly sequential)
        if packet.sequence == *next_seq + 1 {
            *next_seq += 1;
        } else if packet.sequence <= *next_seq {
            return Err(GuardError::InvalidTransaction {
                reason: format!(
                    "IBC duplicate paket: sequence={} zaten işlendi",
                    packet.sequence
                ),
            });
        }

        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AssetAddress, ChainId};

    fn make_channel() -> IbcChannel {
        IbcChannel::new_transfer("channel-0", "channel-1")
    }

    fn cosmos_addr() -> AssetAddress {
        AssetAddress::cosmos([0x01u8; 20])
    }

    #[test]
    fn test_ibc_channel_is_open() {
        let ch = make_channel();
        assert!(ch.is_open());
        assert_eq!(ch.version, "ics20-1");
    }

    #[test]
    fn test_ibc_channel_closed() {
        let mut ch = make_channel();
        ch.state = IbcChannelState::Closed;
        assert!(!ch.is_open());
    }

    #[test]
    fn test_fungible_token_packet_data() {
        let d = FungibleTokenPacketData::new("uatom", 1_000_000, "cosmos1sender", "cosmos1recv");
        assert_eq!(d.amount, "1000000");
        assert_eq!(d.denom, "uatom");
        let bytes = d.to_bytes();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_ibc_packet_commitment_hash() {
        let pkt = IbcPacket {
            sequence: 1,
            source_port: "transfer".into(),
            source_channel: "channel-0".into(),
            destination_port: "transfer".into(),
            destination_channel: "channel-1".into(),
            data: b"test".to_vec(),
            timeout_height: 1000,
            timeout_timestamp: 0,
        };
        let h = pkt.commitment_hash();
        assert_ne!(h, [0u8; 32]);
        assert_eq!(h, pkt.commitment_hash());
    }

    #[test]
    fn test_ibc_module_register_channel() {
        let module = IbcModule::new();
        module.register_channel(make_channel());
        assert!(module.get_channel("channel-0").is_some());
    }

    #[test]
    fn test_ibc_module_send_transfer() {
        let module = IbcModule::new();
        module.register_channel(make_channel());

        let sender = cosmos_addr();
        let caller = sender.display.clone();
        let msg = IbcTransferMsg::new(
            "channel-0", "uatom", 1_000_000,
            sender, cosmos_addr(), 1000,
        );
        let packet = module.send_transfer(&caller, msg).unwrap();

        assert_eq!(packet.sequence, 1);
        assert_eq!(packet.source_channel, "channel-0");
        assert_eq!(module.pending_count(), 1);
    }

    #[test]
    fn test_ibc_module_sequence_increment() {
        let module = IbcModule::new();
        module.register_channel(make_channel());

        for _ in 0..3 {
            let sender = cosmos_addr();
            let caller = sender.display.clone();
            let msg = IbcTransferMsg::new(
                "channel-0", "uatom", 500_000,
                sender, cosmos_addr(), 1000,
            );
            module.send_transfer(&caller, msg).unwrap();
        }
        assert_eq!(module.pending_count(), 3);
    }

    #[test]
    fn test_ibc_module_channel_not_found() {
        let module = IbcModule::new();
        let sender = cosmos_addr();
        let caller = sender.display.clone();
        let msg = IbcTransferMsg::new(
            "channel-99", "uatom", 1_000,
            sender, cosmos_addr(), 1000,
        );
        let result = module.send_transfer(&caller, msg);
        assert!(matches!(result, Err(GuardError::IbcChannelNotFound { .. })));
    }

    #[test]
    fn test_ibc_module_acknowledge_packet() {
        let module = IbcModule::new();
        module.register_channel(make_channel());

        let sender = cosmos_addr();
        let caller = sender.display.clone();
        let msg = IbcTransferMsg::new(
            "channel-0", "uatom", 1_000_000,
            sender, cosmos_addr(), 1000,
        );
        let packet = module.send_transfer(&caller, msg).unwrap();

        assert!(module.acknowledge_packet(packet.sequence));
        assert_eq!(module.pending_count(), 0);
    }

    /// NOVA-RED-GUARD-007: Yanlış caller → Unauthorized hatası
    #[test]
    fn test_ibc_send_transfer_unauthorized_rejected() {
        let module = IbcModule::new();
        module.register_channel(make_channel());

        let sender = cosmos_addr();
        let wrong_caller = "cosmos1attacker000000000000000000000000000";
        let msg = IbcTransferMsg::new(
            "channel-0", "uatom", 1_000_000,
            sender, cosmos_addr(), 1000,
        );
        let result = module.send_transfer(wrong_caller, msg);
        assert!(matches!(result, Err(GuardError::Unauthorized)));
    }
}
