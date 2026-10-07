//! nova-mpc — Nova Shield Chain Threshold MPC Protokolü
//!
//! # Genel Bakış
//!
//! Bu crate, Nova Shield Chain'in MPC-Native Hesap Modeli'nin (§5.1) kriptografik
//! çekirdeğini oluşturur. CGGMP21 protokolünden ilham alan, FROST-style Schnorr
//! threshold imzası kullanır.
//!
//! ## Protokol Seçimi
//!
//! §6.1.1: "Nova Shield Chain yalnızca CGGMP21 kullanır."
//!
//! GG20 neden reddedildi:
//! - Identifiable abort sorunu: kötü niyetli taraf protokolü sabote edebilir
//! - Round sayısı fazla (GG20: 7 round, bizim: 3 keygen + 2 presign + 1 online)
//!
//! CGGMP21 avantajları:
//! - Identifiable abort çözümü: suçlu hem tespit edilir hem cezalandırılır
//! - Non-interactive pre-signing: guardian'ların aynı anda online olması gerekmez
//! - UC (Universal Composability) güvenlik kanıtı
//!
//! ## Paillier Neden Yok?
//!
//! Tam CGGMP21, MtA (Multiplicative-to-Additive) için Paillier homomorfik şifreleme
//! gerektirir. Nova Shield, daha basit FROST-style Schnorr threshold kullanır:
//! - Paillier gerektirmez
//! - Aynı pre-signing avantajını sunar
//! - secp256k1 + VSS + Lagrange interpolasyonu ile çalışır
//!
//! ## Modül Yapısı
//!
//! ```text
//! nova-mpc/
//! ├── errors       — Tüm hata tipleri (MpcError, MpcResult)
//! ├── types        — Temel veri yapıları (PartyIndex, SessionId, LocalKey, ...)
//! ├── crypto_utils — secp256k1 kriptografik yardımcılar
//! ├── session      — SSID hesaplama ve doğrulama
//! ├── vss          — Feldman VSS + Shamir reconstruct
//! ├── dlog         — Schnorr DLog ZK kanıtı
//! ├── p2p_encrypt  — ECDH + AES-256-GCM P2P şifreleme
//! ├── guardian     — Guardian tipleri ve yönetimi
//! ├── keygen/      — 3-round anahtar üretim protokolü
//! │   ├── round1   — Taahhüt + ECDH pk + DLog kanıtı
//! │   ├── round2   — VSS açılımı + şifreli P2P paylar
//! │   └── round3   — Doğrulama + LocalKey çıktısı
//! ├── sign/        — İmzalama protokolü
//! │   ├── presign  — Offline pre-signing (2 round)
//! │   └── online   — Online imzalama (~10ms)
//! └── reshare/     — Key resharing (epoch rotasyonu)
//!     ├── round1   — Eski komite: yeni VSS
//!     └── round2   — Yeni komite: birleştir + doğrula
//! ```

// ─── MODÜLLER ─────────────────────────────────────────────────────────────────

pub mod crypto_utils;
pub mod dlog;
pub mod errors;
pub mod guardian;
pub mod serde_helpers;
pub mod keygen;
pub mod p2p_encrypt;
pub mod reshare;
pub mod round_trait;
pub mod session;
pub mod sign;
pub mod types;
pub mod vss;

// ─── PRELUDE — SIK KULLANILAN TİPLER ──────────────────────────────────────────

pub use errors::{MpcError, MpcResult};
pub use types::{
    DlogProofBytes, EcdhPublicKey, KeygenRound1Msg, KeygenRound2BroadcastMsg,
    KeygenRound2P2pMsg, KeygenRound3Msg, LocalKey, PartialSignature, PartyIndex,
    PreSignShare, PreSignState, PresignRound1Msg, PresignRound2Msg,
    ReshareRound1Msg, ReshareRound2P2pMsg, SessionId, SigningMsg, ThresholdSignature,
    VssCommitment,
};
pub use guardian::{
    GuardianCategory, GuardianKeyShare, GuardianRecord, GuardianStatus, TrustScore,
};
pub use keygen::{KeygenOutput, KeygenState};
pub use sign::{PresignOutput, SignOutput};
pub use reshare::ReshareOutput;
pub use round_trait::{
    make_sorted_parties, sort_party_identities, PartyIdentity, ProtocolRound, ResharingParams,
    RoundMessage, RoundStateMachine,
};

// ─── VERSİYON BİLGİSİ ────────────────────────────────────────────────────────

/// nova-mpc protokol versiyonu.
pub const PROTOCOL_VERSION: u32 = 1;

/// Domain separation string — tüm Fiat-Shamir challenge'larında kullanılır.
pub const PROTOCOL_DOMAIN: &str = "nova-shield-mpc-v1";

/// Varsayılan threshold parametresi (3/5).
pub const DEFAULT_THRESHOLD: u8 = 3;

/// Varsayılan parti sayısı (3/5 yapısı için).
pub const DEFAULT_PARTY_COUNT: u32 = 5;

/// Pre-signing TTL — slot cinsinden (1 epoch = ~216,000 slot @ 400ms).
pub const PRESIGN_TTL_SLOTS: u64 = 216_000;
