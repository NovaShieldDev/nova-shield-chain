//! Transport katmanı — TCP + Noise XX + Yamux
//!
//! Polkadot `transport.rs`: build_transport() → TCP → DNS → Noise → Yamux → boxed

pub mod noise;
pub mod tcp;

pub use noise::{NoiseMessage, NoisePhase, NoiseSession};
pub use tcp::{TcpPeer, TcpTransport};

use crate::errors::{NetworkError, NetworkResult};
use serde::{Deserialize, Serialize};

// ─── ConnectionId ─────────────────────────────────────────────────────────────

/// Benzersiz bağlantı kimliği
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(pub u64);

impl ConnectionId {
    pub fn new(n: u64) -> Self {
        Self(n)
    }
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "conn#{}", self.0)
    }
}

// ─── TransportDirection ───────────────────────────────────────────────────────

/// Bağlantı yönü
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportDirection {
    Inbound,
    Outbound,
}

// ─── EstablishedConnection ────────────────────────────────────────────────────

/// Kurulmuş bağlantı — Polkadot `ConnectedPoint` benzeri
#[derive(Debug)]
pub struct EstablishedConnection {
    pub id: ConnectionId,
    pub remote_addr: String,
    pub direction: TransportDirection,
    pub noise_session: NoiseSession,
    /// Uzak peer'in kimliği (Noise el sıkışma sonrası)
    pub remote_peer_key: [u8; 32],
}

impl EstablishedConnection {
    /// Bağlantıda şifreli mesaj gönder (nonce sayacı artırılır)
    pub fn send_encrypted(&mut self, data: &[u8]) -> NetworkResult<Vec<u8>> {
        self.noise_session.encrypt(data)
    }

    /// Şifreli mesajı çöz (nonce sayacı artırılır)
    pub fn receive_encrypted(&mut self, data: &[u8]) -> NetworkResult<Vec<u8>> {
        self.noise_session.decrypt(data)
    }

    /// Uzak peer kimliği (hex)
    pub fn remote_peer_id_hex(&self) -> String {
        hex::encode(self.remote_peer_key)
    }
}

// ─── TransportLayer ───────────────────────────────────────────────────────────

/// Transport katmanı — bağlantı kurma ve yönetme
/// Polkadot: `build_transport()` pattern
pub struct TransportLayer {
    local_key: [u8; 32],
    next_conn_id: std::sync::atomic::AtomicU64,
}

impl TransportLayer {
    pub fn new(local_key: [u8; 32]) -> Self {
        Self {
            local_key,
            next_conn_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Giden bağlantı başlat (mock — gerçekte TCP dial + Noise XX)
    pub fn dial(&self, remote_addr: &str, remote_key: [u8; 32]) -> NetworkResult<EstablishedConnection> {
        let conn_id = ConnectionId::new(
            self.next_conn_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        );

        // Noise XX başlatıcı
        let mut initiator = NoiseSession::new_initiator(self.local_key);
        let mut responder = NoiseSession::new_responder(remote_key);

        // El sıkışma simülasyonu
        let msg1 = initiator.write_message_1()?;
        let msg2 = responder.process_message_1_and_respond(&msg1)?;
        let msg3 = initiator.process_message_2_and_finalize(&msg2)?;
        responder.process_message_3(&msg3)?;

        if !initiator.is_complete() {
            return Err(NetworkError::HandshakeFailed { reason: "Noise tamamlanmadı".into() });
        }

        let remote_peer_key = initiator.remote_public_key().unwrap();

        Ok(EstablishedConnection {
            id: conn_id,
            remote_addr: remote_addr.to_string(),
            direction: TransportDirection::Outbound,
            noise_session: initiator,
            remote_peer_key,
        })
    }

    /// Gelen bağlantı kabul et (mock)
    pub fn accept(&self, remote_addr: &str, initiator_key: [u8; 32]) -> NetworkResult<EstablishedConnection> {
        let conn_id = ConnectionId::new(
            self.next_conn_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        );

        let mut initiator = NoiseSession::new_initiator(initiator_key);
        let mut responder = NoiseSession::new_responder(self.local_key);

        let msg1 = initiator.write_message_1()?;
        let msg2 = responder.process_message_1_and_respond(&msg1)?;
        let msg3 = initiator.process_message_2_and_finalize(&msg2)?;
        responder.process_message_3(&msg3)?;

        let remote_peer_key = responder.remote_public_key().unwrap();

        Ok(EstablishedConnection {
            id: conn_id,
            remote_addr: remote_addr.to_string(),
            direction: TransportDirection::Inbound,
            noise_session: responder,
            remote_peer_key,
        })
    }

    pub fn local_key(&self) -> [u8; 32] {
        self.local_key
    }
}

// ─── FrameCodec ───────────────────────────────────────────────────────────────

/// Frame seviyesi MAC + nonce codec.
///
/// ## BUG-6 FIX: Frame-level MAC eksikliği
///
/// go-ethereum RLPx (`p2p/rlpx/rlpx.go`) frame formatı:
/// ```text
/// [header 16B encrypted][header MAC 16B][frame data encrypted][frame MAC 16B]
/// ```
/// MAC: `AES-encrypt(hash_state) XOR seed` → ingress/egress Keccak256 hashMAC
///
/// Nova implementasyonu (mock):
/// ```text
/// [frame_nonce: u64 LE 8B][payload: encrypted][mac: BLAKE3(session_key || nonce || payload) 16B]
/// ```
/// Nonce monoton artar → replay saldırılarını önler.
/// Her frame bağımsız doğrulanır → partial frame tampering tespit edilir.
pub struct FrameCodec {
    session_key: [u8; 32],
    /// Gönderilen frame sayısı (egress nonce)
    egress_nonce: u64,
    /// Beklenen sıradaki frame nonce (ingress)
    ingress_nonce: u64,
}

impl FrameCodec {
    /// Yeni frame codec — Noise el sıkışma sonrası session_key ile oluşturulur.
    pub fn new(session_key: [u8; 32]) -> Self {
        Self { session_key, egress_nonce: 0, ingress_nonce: 0 }
    }

    /// Veriyi framing + MAC ile kodla.
    ///
    /// Format: `[nonce: u64 LE][şifreli veri][frame MAC: 16B]`
    pub fn encode_frame(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let nonce = self.egress_nonce;
        self.egress_nonce += 1;

        // XOR stream şifreleme (mock — üretimde AES-CTR)
        let encrypted: Vec<u8> = plaintext
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ self.session_key[i % 32] ^ (nonce as u8).wrapping_add(i as u8))
            .collect();

        // Frame MAC: BLAKE3(session_key || nonce_bytes || encrypted)[..16]
        let nonce_bytes = nonce.to_le_bytes();
        let mut hasher = blake3::Hasher::new_keyed(&self.session_key);
        hasher.update(&nonce_bytes);
        hasher.update(&encrypted);
        let mac_full = hasher.finalize();
        let frame_mac: [u8; 16] = mac_full.as_bytes()[..16].try_into().unwrap();

        let mut out = Vec::with_capacity(8 + encrypted.len() + 16);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&encrypted);
        out.extend_from_slice(&frame_mac);
        out
    }

    /// Frame'i doğrula ve şifreyi çöz.
    ///
    /// Hata: nonce sırası bozuksa (replay) veya MAC yanlışsa.
    pub fn decode_frame(&mut self, frame: &[u8]) -> NetworkResult<Vec<u8>> {
        if frame.len() < 8 + 16 {
            return Err(NetworkError::NoiseError { reason: "frame çok kısa".into() });
        }

        let nonce_bytes: [u8; 8] = frame[..8].try_into().unwrap();
        let nonce = u64::from_le_bytes(nonce_bytes);

        // Replay koruması: nonce monoton artmalı
        if nonce != self.ingress_nonce {
            return Err(NetworkError::NoiseError {
                reason: format!("replay/sıra hatası: beklenen {} aldı {}", self.ingress_nonce, nonce),
            });
        }

        let encrypted = &frame[8..frame.len() - 16];
        let received_mac = &frame[frame.len() - 16..];

        // MAC doğrula
        let mut hasher = blake3::Hasher::new_keyed(&self.session_key);
        hasher.update(&nonce_bytes);
        hasher.update(encrypted);
        let expected_mac_full = hasher.finalize();
        let expected_mac = &expected_mac_full.as_bytes()[..16];

        if received_mac != expected_mac {
            return Err(NetworkError::NoiseError { reason: "frame MAC doğrulama başarısız".into() });
        }

        self.ingress_nonce += 1;

        // XOR decrypt
        let plaintext: Vec<u8> = encrypted
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ self.session_key[i % 32] ^ (nonce as u8).wrapping_add(i as u8))
            .collect();

        Ok(plaintext)
    }

    pub fn egress_nonce(&self) -> u64 { self.egress_nonce }
    pub fn ingress_nonce(&self) -> u64 { self.ingress_nonce }
    pub fn session_key(&self) -> [u8; 32] { self.session_key }

    /// Encode (egress) ve decode (ingress) yarılarına böl — TCP stream split için.
    ///
    /// Encode yarısı egress_nonce'u, decode yarısı ingress_nonce'u korur.
    pub fn into_split(self) -> (FrameCodec, FrameCodec) {
        let encoder = FrameCodec { session_key: self.session_key, egress_nonce: self.egress_nonce, ingress_nonce: 0 };
        let decoder = FrameCodec { session_key: self.session_key, egress_nonce: 0, ingress_nonce: self.ingress_nonce };
        (encoder, decoder)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use x25519_dalek::{PublicKey as X25519Public, StaticSecret};

    fn x25519_pub(seed: [u8; 32]) -> [u8; 32] {
        *X25519Public::from(&StaticSecret::from(seed)).as_bytes()
    }

    #[test]
    fn test_transport_dial_accept() {
        let local = TransportLayer::new([1u8; 32]);
        let remote_key = [2u8; 32];

        let conn = local.dial("127.0.0.1:30333", remote_key).unwrap();
        assert_eq!(conn.direction, TransportDirection::Outbound);
        // remote_peer_key artık X25519 public key (ham bayt değil)
        assert_eq!(conn.remote_peer_key, x25519_pub(remote_key));
    }

    #[test]
    fn test_transport_accept() {
        let local = TransportLayer::new([2u8; 32]);
        let conn = local.accept("192.168.1.1:30333", [1u8; 32]).unwrap();
        assert_eq!(conn.direction, TransportDirection::Inbound);
        // remote_peer_key = X25519 public key türetilmiş [1u8;32]'den
        assert_eq!(conn.remote_peer_key, x25519_pub([1u8; 32]));
    }

    #[test]
    fn test_transport_encrypt_decrypt() {
        let local = TransportLayer::new([1u8; 32]);
        let mut conn = local.dial("127.0.0.1:30333", [2u8; 32]).unwrap();

        let msg = b"Nova Shield mesaji";
        let ct = conn.send_encrypted(msg).unwrap();
        let pt = conn.receive_encrypted(&ct).unwrap();
        assert_eq!(pt, msg);
    }

    #[test]
    fn test_connection_id_display() {
        let id = ConnectionId::new(42);
        assert_eq!(format!("{}", id), "conn#42");
    }

    #[test]
    fn test_remote_peer_id_hex() {
        let local = TransportLayer::new([1u8; 32]);
        let conn = local.dial("127.0.0.1:30333", [2u8; 32]).unwrap();
        let hex = conn.remote_peer_id_hex();
        assert_eq!(hex.len(), 64);
    }

    // ─── FrameCodec Testleri ──────────────────────────────────────────────

    #[test]
    fn test_frame_codec_encode_decode() {
        let key = [0xABu8; 32];
        let mut encoder = FrameCodec::new(key);
        let mut decoder = FrameCodec::new(key);

        let msg = b"Nova Shield frame test mesaji";
        let frame = encoder.encode_frame(msg);
        let decoded = decoder.decode_frame(&frame).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_frame_codec_nonce_increments() {
        let key = [1u8; 32];
        let mut codec = FrameCodec::new(key);
        assert_eq!(codec.egress_nonce(), 0);
        codec.encode_frame(b"msg1");
        assert_eq!(codec.egress_nonce(), 1);
        codec.encode_frame(b"msg2");
        assert_eq!(codec.egress_nonce(), 2);
    }

    #[test]
    fn test_frame_codec_replay_rejected() {
        let key = [2u8; 32];
        let mut encoder = FrameCodec::new(key);
        let mut decoder = FrameCodec::new(key);

        let frame1 = encoder.encode_frame(b"frame1");
        let frame2 = encoder.encode_frame(b"frame2");

        // Doğru sıra
        decoder.decode_frame(&frame1).unwrap();
        // frame1'i tekrar gönder → replay saldırısı → reddedilmeli
        let result = decoder.decode_frame(&frame1);
        assert!(result.is_err(), "Replay saldırısı reddedilmeli");
        // frame2 decode edilmeli
        decoder.decode_frame(&frame2).unwrap();
    }

    #[test]
    fn test_frame_codec_tamper_detected() {
        let key = [3u8; 32];
        let mut encoder = FrameCodec::new(key);
        let mut decoder = FrameCodec::new(key);

        let mut frame = encoder.encode_frame(b"gizli mesaj");
        // Frame içeriğini boz (MAC'den önceki son byte)
        let len = frame.len();
        frame[len - 17] ^= 0xFF;

        let result = decoder.decode_frame(&frame);
        assert!(result.is_err(), "Tampered frame MAC doğrulamasında reddedilmeli");
    }

    #[test]
    fn test_frame_codec_multiple_messages() {
        let key = [4u8; 32];
        let mut enc = FrameCodec::new(key);
        let mut dec = FrameCodec::new(key);

        for i in 0u8..10 {
            let msg = vec![i; 100];
            let frame = enc.encode_frame(&msg);
            let decoded = dec.decode_frame(&frame).unwrap();
            assert_eq!(decoded, msg);
        }
        assert_eq!(enc.egress_nonce(), 10);
        assert_eq!(dec.ingress_nonce(), 10);
    }
}
