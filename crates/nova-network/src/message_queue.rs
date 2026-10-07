//! GÖREV 106: MessageQueueChain Sıralama Garantisi (Hash Zinciri)
//!
//! ## Mimari
//!
//! ### Polkadot XCMP MessageQueueChain
//! - Her mesaj, önceki mesajın hash'ini içerir → zincir oluşur
//! - `MQC_head = hash(prev_head || sender || message_hash || sent_at)`
//! - FIFO sıralama: alıcı taraf mesajları zincir sırasıyla işler
//! - Sıra dışı mesaj tespit edilirse → reject
//!
//! ### Nova Uyarlaması
//! - `MessageQueueChain`: bir kanal için hash zinciri takibi
//! - `QueuedMessage`: zincirdeki bir mesaj kaydı
//! - `MessageQueueChannel`: gönderici–alıcı çifti için tam kanal yönetimi
//! - `MessageQueueRouter`: çoklu kanal yöneticisi

use blake3;
use nova_primitives::{Address, BlockHash, SlotId};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Maksimum mesaj boyutu (64 KB)
pub const MAX_MESSAGE_SIZE: usize = 65_536;

/// Kanal başına maksimum mesaj tamponu
pub const MAX_QUEUE_DEPTH: usize = 1_000;

/// Genesis MQC head (boş zincir başlangıcı)
pub const GENESIS_MQC_HEAD: [u8; 32] = [0u8; 32];

// ─── QueuedMessage ────────────────────────────────────────────────────────────

/// Kuyruktaki bir mesaj
///
/// Polkadot: `InboundHrmpMessage { sent_at, data }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    /// Mesaj içeriği
    pub data: Vec<u8>,
    /// Gönderim slotu
    pub sent_at: SlotId,
    /// Gönderici
    pub sender: Address,
    /// Alıcı
    pub recipient: Address,
    /// Bu mesajın zincirdeki hash'i (MQC head sonrası)
    pub mqc_head: [u8; 32],
    /// Sıra numarası (0-tabanlı)
    pub sequence: u64,
    /// Önceki MQC head (doğrulama için)
    pub prev_head: [u8; 32],
}

impl QueuedMessage {
    /// Mesaj içeriğinin hash'i
    pub fn message_hash(&self) -> [u8; 32] {
        *blake3::hash(&self.data).as_bytes()
    }
}

// ─── MessageQueueChain ────────────────────────────────────────────────────────

/// Tek yönlü mesaj kuyruğu hash zinciri
///
/// Polkadot: `MessageQueueChain` (pallet_hrmp)
/// `new_mqc_head = hash(prev_mqc_head || block_hash || message_hash)`
#[derive(Debug, Clone)]
pub struct MessageQueueChain {
    /// Mevcut zincir uç noktası (son mesajdan sonra)
    pub head: [u8; 32],
    /// Toplam mesaj sayısı
    pub message_count: u64,
    /// Toplam byte boyutu
    pub total_size: u64,
}

impl MessageQueueChain {
    /// Yeni boş zincir
    pub fn new() -> Self {
        Self {
            head: GENESIS_MQC_HEAD,
            message_count: 0,
            total_size: 0,
        }
    }

    /// Yeni MQC head hesapla
    ///
    /// Polkadot: `H(prev_head || block_hash || message_hash)`
    pub fn compute_next_head(
        prev_head: &[u8; 32],
        block_hash: &[u8; 32],
        message_hash: &[u8; 32],
        sequence: u64,
    ) -> [u8; 32] {
        *blake3::hash(&[
            b"nova-mqc-v1:".as_slice(),
            prev_head.as_slice(),
            block_hash.as_slice(),
            message_hash.as_slice(),
            &sequence.to_le_bytes(),
        ].concat()).as_bytes()
    }

    /// Mesaj ekle ve head güncelle
    ///
    /// Returns: yeni head değeri
    pub fn append(
        &mut self,
        message_hash: &[u8; 32],
        block_hash: &[u8; 32],
    ) -> [u8; 32] {
        let next_head = Self::compute_next_head(
            &self.head,
            block_hash,
            message_hash,
            self.message_count,
        );
        self.head = next_head;
        self.message_count += 1;
        next_head
    }

    /// Bu zincir boş mu?
    pub fn is_empty(&self) -> bool {
        self.message_count == 0
    }

    /// Bir mesajın bu zincirin beklenen N. elemanı olup olmadığını doğrula
    ///
    /// Polkadot: off-chain doğrulama pattern
    pub fn verify_next_head(
        current_head: &[u8; 32],
        block_hash: &[u8; 32],
        message_hash: &[u8; 32],
        expected_head: &[u8; 32],
        sequence: u64,
    ) -> bool {
        let computed = Self::compute_next_head(current_head, block_hash, message_hash, sequence);
        &computed == expected_head
    }
}

impl Default for MessageQueueChain {
    fn default() -> Self {
        Self::new()
    }
}

// ─── MessageQueueChannel ──────────────────────────────────────────────────────

/// Gönderici–alıcı çifti için tam mesaj kanalı
///
/// Polkadot: `HrmpChannel { sender, recipient, mqc_head, message_queue }`
#[derive(Debug)]
pub struct MessageQueueChannel {
    pub sender: Address,
    pub recipient: Address,
    /// Gönderme tarafı MQC (gönderici günceller)
    pub outbound_mqc: MessageQueueChain,
    /// Alım tarafı MQC (alıcı işledikçe günceller)
    pub inbound_mqc: MessageQueueChain,
    /// İşlenmemiş mesaj kuyruğu
    pub pending: std::collections::VecDeque<QueuedMessage>,
    /// Toplam gönderilen mesaj
    pub sent_count: u64,
    /// Toplam alınan (işlenen) mesaj
    pub processed_count: u64,
    /// Kanal açık mı?
    pub is_open: bool,
}

impl MessageQueueChannel {
    pub fn new(sender: Address, recipient: Address) -> Self {
        Self {
            sender,
            recipient,
            outbound_mqc: MessageQueueChain::new(),
            inbound_mqc: MessageQueueChain::new(),
            pending: std::collections::VecDeque::new(),
            sent_count: 0,
            processed_count: 0,
            is_open: true,
        }
    }

    /// Mesaj gönder
    ///
    /// Returns: mesajın MQC head'i
    pub fn send(
        &mut self,
        data: Vec<u8>,
        slot: SlotId,
        block_hash: &[u8; 32],
    ) -> Result<[u8; 32], MqcError> {
        if !self.is_open {
            return Err(MqcError::ChannelClosed);
        }
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(MqcError::MessageTooLarge { size: data.len() });
        }
        if self.pending.len() >= MAX_QUEUE_DEPTH {
            return Err(MqcError::QueueFull);
        }

        let message_hash = *blake3::hash(&data).as_bytes();
        let prev_head = self.outbound_mqc.head;
        let new_head = self.outbound_mqc.append(&message_hash, block_hash);
        self.outbound_mqc.total_size += data.len() as u64;

        let msg = QueuedMessage {
            data,
            sent_at: slot,
            sender: self.sender,
            recipient: self.recipient,
            mqc_head: new_head,
            sequence: self.sent_count,
            prev_head,
        };
        self.pending.push_back(msg);
        self.sent_count += 1;

        Ok(new_head)
    }

    /// Sıradaki mesajı işle (FIFO)
    ///
    /// Sıra kontrolü: inbound_mqc == pending mesajın prev_head olmalı
    pub fn process_next(&mut self, block_hash: &[u8; 32]) -> Result<QueuedMessage, MqcError> {
        let msg = self.pending.front().ok_or(MqcError::QueueEmpty)?;

        // Sıralama doğrulaması
        if msg.prev_head != self.inbound_mqc.head {
            return Err(MqcError::OutOfOrder {
                expected_seq: self.processed_count,
                got_seq: msg.sequence,
            });
        }

        // MQC head doğrula
        let message_hash = *blake3::hash(&msg.data).as_bytes();
        let expected_head = MessageQueueChain::compute_next_head(
            &self.inbound_mqc.head,
            block_hash,
            &message_hash,
            self.processed_count,
        );

        if expected_head != msg.mqc_head {
            return Err(MqcError::InvalidChainHead {
                expected: expected_head,
                got: msg.mqc_head,
            });
        }

        let msg = self.pending.pop_front().unwrap();
        self.inbound_mqc.head = msg.mqc_head;
        self.inbound_mqc.message_count += 1;
        self.processed_count += 1;

        Ok(msg)
    }

    /// Pending mesaj sayısı
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Kanal kapat
    pub fn close(&mut self) {
        self.is_open = false;
    }
}

// ─── MessageQueueRouter ───────────────────────────────────────────────────────

/// Çoklu kanal yöneticisi
///
/// Polkadot: `HrmpChannels` storage (pallet_hrmp)
pub struct MessageQueueRouter {
    /// (sender, recipient) → channel
    channels: std::collections::HashMap<(Address, Address), MessageQueueChannel>,
}

impl MessageQueueRouter {
    pub fn new() -> Self {
        Self {
            channels: std::collections::HashMap::new(),
        }
    }

    /// Kanal aç (sender → recipient)
    pub fn open_channel(&mut self, sender: Address, recipient: Address) -> Result<(), MqcError> {
        let key = (sender, recipient);
        if self.channels.contains_key(&key) {
            return Err(MqcError::ChannelAlreadyExists);
        }
        self.channels.insert(key, MessageQueueChannel::new(sender, recipient));
        Ok(())
    }

    /// Mesaj gönder
    pub fn send(
        &mut self,
        sender: Address,
        recipient: Address,
        data: Vec<u8>,
        slot: SlotId,
        block_hash: &[u8; 32],
    ) -> Result<[u8; 32], MqcError> {
        let ch = self.channels.get_mut(&(sender, recipient))
            .ok_or(MqcError::ChannelNotFound)?;
        ch.send(data, slot, block_hash)
    }

    /// Mesaj işle
    pub fn process_next(
        &mut self,
        sender: Address,
        recipient: Address,
        block_hash: &[u8; 32],
    ) -> Result<QueuedMessage, MqcError> {
        let ch = self.channels.get_mut(&(sender, recipient))
            .ok_or(MqcError::ChannelNotFound)?;
        ch.process_next(block_hash)
    }

    /// Kanal bilgisi
    pub fn channel(&self, sender: &Address, recipient: &Address) -> Option<&MessageQueueChannel> {
        self.channels.get(&(*sender, *recipient))
    }

    /// Toplam kanal sayısı
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Tüm bekleyen mesaj sayısı (tüm kanallar)
    pub fn total_pending(&self) -> usize {
        self.channels.values().map(|ch| ch.pending_count()).sum()
    }
}

impl Default for MessageQueueRouter {
    fn default() -> Self {
        Self::new()
    }
}

// ─── MqcError ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MqcError {
    ChannelNotFound,
    ChannelAlreadyExists,
    ChannelClosed,
    QueueEmpty,
    QueueFull,
    MessageTooLarge { size: usize },
    OutOfOrder { expected_seq: u64, got_seq: u64 },
    InvalidChainHead { expected: [u8; 32], got: [u8; 32] },
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn block_hash(n: u8) -> [u8; 32] {
        [n; 32]
    }

    // 1. Boş zincir genesis head
    #[test]
    fn test_empty_chain_genesis() {
        let chain = MessageQueueChain::new();
        assert_eq!(chain.head, GENESIS_MQC_HEAD);
        assert!(chain.is_empty());
    }

    // 2. Mesaj append → head güncellenir
    #[test]
    fn test_append_updates_head() {
        let mut chain = MessageQueueChain::new();
        let msg_hash = [0xAAu8; 32];
        let bh = block_hash(1);

        let head1 = chain.append(&msg_hash, &bh);
        assert_ne!(head1, GENESIS_MQC_HEAD);
        assert_eq!(chain.message_count, 1);
        assert_eq!(chain.head, head1);
    }

    // 3. Zincir sırası deterministik
    #[test]
    fn test_chain_deterministic() {
        let msgs = vec![[0x11u8; 32], [0x22u8; 32], [0x33u8; 32]];
        let bh = block_hash(5);

        let build_chain = || {
            let mut c = MessageQueueChain::new();
            for m in &msgs { c.append(m, &bh); }
            c.head
        };

        assert_eq!(build_chain(), build_chain());
    }

    // 4. Farklı sıra → farklı head
    #[test]
    fn test_different_order_different_head() {
        let bh = block_hash(1);
        let mut c1 = MessageQueueChain::new();
        c1.append(&[0x11u8; 32], &bh);
        c1.append(&[0x22u8; 32], &bh);

        let mut c2 = MessageQueueChain::new();
        c2.append(&[0x22u8; 32], &bh);
        c2.append(&[0x11u8; 32], &bh);

        assert_ne!(c1.head, c2.head);
    }

    // 5. verify_next_head — geçerli
    #[test]
    fn test_verify_next_head_valid() {
        let prev = [0u8; 32];
        let bh = block_hash(1);
        let msg = [0xBBu8; 32];

        let expected = MessageQueueChain::compute_next_head(&prev, &bh, &msg, 0);
        assert!(MessageQueueChain::verify_next_head(&prev, &bh, &msg, &expected, 0));
    }

    // 6. verify_next_head — geçersiz
    #[test]
    fn test_verify_next_head_invalid() {
        let prev = [0u8; 32];
        let bh = block_hash(1);
        let msg = [0xBBu8; 32];
        let wrong_head = [0xFFu8; 32];

        assert!(!MessageQueueChain::verify_next_head(&prev, &bh, &msg, &wrong_head, 0));
    }

    // 7. Kanal gönderim ve işleme
    #[test]
    fn test_channel_send_and_process() {
        let mut ch = MessageQueueChannel::new(addr(1), addr(2));
        let bh = block_hash(10);
        let slot = SlotId(100);

        ch.send(b"hello world".to_vec(), slot, &bh).unwrap();
        assert_eq!(ch.pending_count(), 1);

        let msg = ch.process_next(&bh).unwrap();
        assert_eq!(msg.data, b"hello world");
        assert_eq!(ch.pending_count(), 0);
        assert_eq!(ch.processed_count, 1);
    }

    // 8. FIFO sıralama korunur
    #[test]
    fn test_fifo_ordering() {
        let mut ch = MessageQueueChannel::new(addr(1), addr(2));
        let bh = block_hash(1);
        let slot = SlotId(1);

        ch.send(b"msg1".to_vec(), slot, &bh).unwrap();
        ch.send(b"msg2".to_vec(), slot, &bh).unwrap();
        ch.send(b"msg3".to_vec(), slot, &bh).unwrap();

        let m1 = ch.process_next(&bh).unwrap();
        let m2 = ch.process_next(&bh).unwrap();
        let m3 = ch.process_next(&bh).unwrap();

        assert_eq!(m1.data, b"msg1");
        assert_eq!(m2.data, b"msg2");
        assert_eq!(m3.data, b"msg3");
    }

    // 9. Kapalı kanalda gönderim hatası
    #[test]
    fn test_closed_channel_send_error() {
        let mut ch = MessageQueueChannel::new(addr(1), addr(2));
        ch.close();

        let result = ch.send(b"test".to_vec(), SlotId(1), &block_hash(1));
        assert_eq!(result, Err(MqcError::ChannelClosed));
    }

    // 10. Boş kuyruk hata döndürür
    #[test]
    fn test_empty_queue_error() {
        let mut ch = MessageQueueChannel::new(addr(1), addr(2));
        let result = ch.process_next(&block_hash(1));
        assert_eq!(result, Err(MqcError::QueueEmpty));
    }

    // 11. Büyük mesaj hata döndürür
    #[test]
    fn test_message_too_large() {
        let mut ch = MessageQueueChannel::new(addr(1), addr(2));
        let big_data = vec![0u8; MAX_MESSAGE_SIZE + 1];
        let result = ch.send(big_data, SlotId(1), &block_hash(1));
        assert!(matches!(result, Err(MqcError::MessageTooLarge { .. })));
    }

    // 12. Router: kanal aç ve mesaj gönder
    #[test]
    fn test_router_send_and_receive() {
        let mut router = MessageQueueRouter::new();
        let bh = block_hash(1);

        router.open_channel(addr(1), addr(2)).unwrap();
        router.send(addr(1), addr(2), b"from 1 to 2".to_vec(), SlotId(5), &bh).unwrap();

        assert_eq!(router.total_pending(), 1);

        let msg = router.process_next(addr(1), addr(2), &bh).unwrap();
        assert_eq!(msg.data, b"from 1 to 2");
        assert_eq!(router.total_pending(), 0);
    }

    // 13. Router: bilinmeyen kanal hatası
    #[test]
    fn test_router_unknown_channel() {
        let mut router = MessageQueueRouter::new();
        let result = router.send(addr(1), addr(2), b"test".to_vec(), SlotId(1), &block_hash(1));
        assert_eq!(result, Err(MqcError::ChannelNotFound));
    }

    // 14. Inbound MQC head outbound ile eşleşir (tam proses)
    #[test]
    fn test_inbound_matches_outbound_after_process() {
        let mut ch = MessageQueueChannel::new(addr(1), addr(2));
        let bh = block_hash(99);
        let slot = SlotId(200);

        // 3 mesaj gönder
        for i in 0..3u8 {
            ch.send(vec![i; 10], slot, &bh).unwrap();
        }
        let outbound_head = ch.outbound_mqc.head;

        // 3 mesajı işle
        for _ in 0..3 {
            ch.process_next(&bh).unwrap();
        }

        // İşleme tamamlandı → inbound == outbound head
        assert_eq!(ch.inbound_mqc.head, outbound_head);
    }
}
