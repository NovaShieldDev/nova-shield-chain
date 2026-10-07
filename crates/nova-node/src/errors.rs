//! Node hata türleri

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NodeError {
    #[error("konfigürasyon hatası: {reason}")]
    ConfigError { reason: String },

    #[error("servis başlatma hatası: {service} — {reason}")]
    ServiceStartError { service: String, reason: String },

    #[error("servis durdurma hatası: {service} — {reason}")]
    ServiceStopError { service: String, reason: String },

    #[error("senkronizasyon hatası: {reason}")]
    SyncError { reason: String },

    #[error("validator anahtarı bulunamadı")]
    ValidatorKeyNotFound,

    #[error("bağlantı noktası zaten kullanımda: {port}")]
    PortInUse { port: u16 },

    #[error("yetkilendirme hatası: {reason}")]
    AuthorizationError { reason: String },

    #[error("genesis bloğu hatası: {reason}")]
    GenesisError { reason: String },

    #[error("serileştirme hatası: {reason}")]
    SerializationError { reason: String },

    #[error("veritabanı hatası: {reason}")]
    DatabaseError { reason: String },
}

pub type NodeResult<T> = Result<T, NodeError>;
