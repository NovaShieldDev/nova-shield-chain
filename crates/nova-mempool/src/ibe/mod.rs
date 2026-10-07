//! IBE — Threshold Şifreleme Modülü. §5.2.2, §6.1.4
//!
//! Rolling Shutter'dan ilham alan threshold IBE şifreleme:
//! ECDH tabanlı Ristretto255 threshold şifreleme (DLOG problemi yok).
//!
//! ## Kullanılan Şema: ECDH Threshold Hybrid Encryption
//!
//! **Şema seçimi neden ECDH (ElGamal değil)?**
//!
//! Klasik ElGamal threshold şifreleme `key_scalar * G` formundaki bir noktayı
//! döndürür; bu noktadan `key_scalar`'ı geri almak discrete logarithm (DLOG)
//! problemidir — hesaplanamaz. Bu yüzden büyük TX içerikleri için klasik ElGamal
//! direkt uygulanamaz.
//!
//! Nova Shield, Rolling Shutter'ın kullandığı yönteme benzer ECDH tabanlı
//! bir hibrit şema kullanır:
//!
//! 1. Client ephemeral keypair üretir: `(e, E = e*G)`
//! 2. ECDH shared secret: `S = e * threshold_pk`
//! 3. Simetrik anahtar: `K = BLAKE3("nova-mempool-aes-key" || S.compress())`
//! 4. TX içeriğini AES-256-GCM ile şifreler: `cipher = AES(K, tx_bytes, aad=slot)`
//! 5. Commitment: `H = BLAKE3(E || cipher || nonce || slot)`
//! 6. Sadece `(E, cipher, nonce, H)` broadcast edilir — K asla
//!
//! Threshold decryption:
//! 1. Her validator `D_i = sk_i * E` hesaplar (decryption share)
//! 2. t share Lagrange kombinasyonu: `S = Σ λ_i * D_i = sk_threshold * E = e * pk`
//! 3. `K = BLAKE3("nova-mempool-aes-key" || S.compress())`
//! 4. AES decrypt → TX

pub mod decrypt;
pub mod encrypt;

/// BLAKE3 domain separator — AES anahtarı türetmek için kullanılır.
/// Her protokol için farklı domain separator → key separation.
pub const ECDH_KEY_DOMAIN: &str = "nova-mempool-aes-key-v1";

/// BLAKE3 domain separator — commitment hesabında kullanılır.
pub const COMMITMENT_DOMAIN: &str = "nova-mempool-commitment-v1";
