//! Nova Network hata türleri

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NetworkError {
    #[error("Peer bağlantısı reddedildi: {peer_id}")]
    ConnectionRefused { peer_id: String },

    #[error("Peer yasaklıydı: {peer_id}")]
    PeerBanned { peer_id: String },

    #[error("El sıkışma başarısız: {reason}")]
    HandshakeFailed { reason: String },

    #[error("Noise şifreleme hatası: {reason}")]
    NoiseError { reason: String },

    #[error("Gossip mesajı geçersiz: {reason}")]
    InvalidGossipMessage { reason: String },

    #[error("Shred geçersiz: indeks={index}")]
    InvalidShred { index: u32 },

    #[error("Kademlia sorgusu başarısız: {key_hex}")]
    KademliaQueryFailed { key_hex: String },

    #[error("Maksimum peer sayısına ulaşıldı: {max}")]
    MaxPeersReached { max: usize },

    #[error("Protokol uyumsuzluğu: beklenen={expected}, gelen={got}")]
    ProtocolMismatch { expected: String, got: String },

    #[error("Serializasyon hatası: {reason}")]
    SerializationError { reason: String },

    #[error("Kanal kapalı")]
    ChannelClosed,

    #[error("Zaman aşımı: {operation}")]
    Timeout { operation: String },

    #[error("Adres geçersiz: {addr}")]
    InvalidAddress { addr: String },

    #[error("I/O hatası: {reason}")]
    IoError { reason: String },

    #[error("Protokol hatası: {reason}")]
    ProtocolError { reason: String },
}

pub type NetworkResult<T> = Result<T, NetworkError>;
