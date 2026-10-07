//! Peer el sıkışma protokolü
//!
//! Avalanchego pattern:
//! - `Handshake` mesajı: NodeID, IP, Version, UpgradeTime, TrackedSubnets, BLS key
//! - `gotHandshake` atomic flag (okuma goroutine)
//! - Versiyon uyumluluk kontrolü
//!
//! Rolling Shutter pattern:
//! - MessagePrototypes: belirli mesaj türleri tanımlanır
//! - AddMessageHandler: handler kayıt sistemi

use crate::errors::{NetworkError, NetworkResult};
use crate::peer::info::{PeerId, PeerRole};
use nova_crypto::{signatures::bls::BlsVerifier, BlsKeypair};
use nova_primitives::crypto::{BlsPublicKey, BlsSignature};
use serde::{Deserialize, Serialize};

// ─── HandshakeMessage ─────────────────────────────────────────────────────────

/// El sıkışma mesajı — Avalanchego `Handshake` proto mesajı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeMessage {
    /// Node kimliği (32 byte)
    pub node_id: [u8; 32],
    /// IP:port
    pub my_addr: String,
    /// Çalışan versiyon
    pub version: String,
    /// Protokol kimliği
    pub protocol_id: String,
    /// BLS imzası (validator için zorunlu) — node_id || genesis_hash üzerine
    pub bls_signature: Option<Vec<u8>>,
    /// BLS public key (validator için zorunlu) — imzanın doğrulanması için
    pub bls_public_key: Option<Vec<u8>>,
    /// Takip edilen subnetler — Avalanchego: maxNumTrackedSubnets=16
    pub tracked_subnets: Vec<String>,
    /// Node rolü
    pub role: PeerRole,
    /// Genesis hash (ağ uyumluluk kontrolü)
    pub genesis_hash: [u8; 32],
    /// Timestamp
    pub timestamp: u64,
}

impl HandshakeMessage {
    pub fn new(
        node_id: [u8; 32],
        my_addr: impl Into<String>,
        version: impl Into<String>,
        protocol_id: impl Into<String>,
        genesis_hash: [u8; 32],
        role: PeerRole,
    ) -> Self {
        Self {
            node_id,
            my_addr: my_addr.into(),
            version: version.into(),
            protocol_id: protocol_id.into(),
            bls_signature: None,
            bls_public_key: None,
            tracked_subnets: vec![],
            role,
            genesis_hash,
            timestamp: current_timestamp(),
        }
    }

    pub fn with_bls_signature(mut self, sig: Vec<u8>) -> Self {
        self.bls_signature = Some(sig);
        self
    }

    pub fn with_bls_public_key(mut self, pk: Vec<u8>) -> Self {
        self.bls_public_key = Some(pk);
        self
    }

    /// Validator handshake için BLS imzası oluştur: sign(node_id || genesis_hash)
    pub fn with_bls_validator_sig(
        mut self,
        keypair: &nova_crypto::BlsKeypair,
    ) -> Self {
        let msg = bls_handshake_message(&self.node_id, &self.genesis_hash);
        let sig = keypair.sign_raw(&msg);
        self.bls_signature = Some(sig.as_bytes().to_vec());
        self.bls_public_key = Some(keypair.bls_public_key().as_bytes().to_vec());
        self
    }

    pub fn with_subnets(mut self, subnets: Vec<String>) -> Self {
        // Avalanchego: maxNumTrackedSubnets=16 sınırı
        self.tracked_subnets = subnets.into_iter().take(16).collect();
        self
    }

    /// Serialle
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Deserialleştir
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        serde_json::from_slice(bytes).map_err(|e| NetworkError::SerializationError {
            reason: e.to_string(),
        })
    }
}

// ─── HandshakeResult ──────────────────────────────────────────────────────────

/// Başarılı el sıkışma sonucu
#[derive(Debug, Clone)]
pub struct HandshakeResult {
    pub peer_id: PeerId,
    pub peer_addr: String,
    pub peer_version: String,
    pub peer_role: PeerRole,
    pub peer_genesis_hash: [u8; 32],
    pub tracked_subnets: Vec<String>,
    pub is_validator: bool,
}

// ─── HandshakeValidator ───────────────────────────────────────────────────────

/// El sıkışma doğrulayıcısı
/// Avalanchego: `gotHandshake`, versiyon uyumluluk kontrolü
pub struct HandshakeValidator {
    /// Yerel protokol kimliği
    local_protocol_id: String,
    /// Yerel genesis hash
    local_genesis_hash: [u8; 32],
    /// Minimum desteklenen versiyon
    min_version: u32,
}

impl HandshakeValidator {
    pub fn new(
        local_protocol_id: impl Into<String>,
        local_genesis_hash: [u8; 32],
    ) -> Self {
        Self {
            local_protocol_id: local_protocol_id.into(),
            local_genesis_hash,
            min_version: 0,
        }
    }

    /// El sıkışma mesajını doğrula
    pub fn validate(&self, msg: &HandshakeMessage) -> NetworkResult<HandshakeResult> {
        // 1. Protokol kimliği kontrolü
        if msg.protocol_id != self.local_protocol_id {
            return Err(NetworkError::ProtocolMismatch {
                expected: self.local_protocol_id.clone(),
                got: msg.protocol_id.clone(),
            });
        }

        // 2. Genesis hash kontrolü (ağ ayrımı — Avalanchego pattern)
        if msg.genesis_hash != self.local_genesis_hash {
            return Err(NetworkError::HandshakeFailed {
                reason: "genesis hash uyumsuz — farklı ağ".into(),
            });
        }

        // 3. Versiyon kontrolü
        let version_num = parse_version(&msg.version);
        if version_num < self.min_version {
            return Err(NetworkError::HandshakeFailed {
                reason: format!("eski versiyon: {}", msg.version),
            });
        }

        // 4. Adres geçerliliği
        if msg.my_addr.is_empty() {
            return Err(NetworkError::InvalidAddress { addr: msg.my_addr.clone() });
        }

        // 5. Subnet sayısı kontrolü — Avalanchego: maxNumTrackedSubnets=16
        if msg.tracked_subnets.len() > 16 {
            return Err(NetworkError::HandshakeFailed {
                reason: "çok fazla subnet takibi".into(),
            });
        }

        // 6. NOVA-RED-NETWORK-002: Validator rolü iddiasını BLS ile kriptografik doğrula.
        // Sadece is_some() kontrolü yetmez — sahte 96-byte veri ile validator rolü alınabilirdi.
        let is_validator = if msg.role == PeerRole::Validator {
            match (&msg.bls_signature, &msg.bls_public_key) {
                (Some(sig_bytes), Some(pk_bytes)) => {
                    let handshake_msg = bls_handshake_message(&msg.node_id, &msg.genesis_hash);
                    let sig = bls_signature_from_bytes(sig_bytes)?;
                    let pk = bls_pubkey_from_bytes(pk_bytes)?;
                    BlsVerifier::verify(&handshake_msg, &sig, &pk).map_err(|e| {
                        NetworkError::HandshakeFailed {
                            reason: format!("BLS imzası geçersiz: {e}"),
                        }
                    })?;
                    true
                }
                _ => {
                    return Err(NetworkError::HandshakeFailed {
                        reason: "validator rolü için BLS imzası ve public key zorunlu".into(),
                    });
                }
            }
        } else {
            false
        };

        Ok(HandshakeResult {
            peer_id: PeerId::from_bytes(msg.node_id),
            peer_addr: msg.my_addr.clone(),
            peer_version: msg.version.clone(),
            peer_role: msg.role.clone(),
            peer_genesis_hash: msg.genesis_hash,
            tracked_subnets: msg.tracked_subnets.clone(),
            is_validator,
        })
    }
}

// ─── BLS Yardımcılar ──────────────────────────────────────────────────────────

/// Handshake BLS mesajı: node_id || genesis_hash (64 byte, sabit).
fn bls_handshake_message(node_id: &[u8; 32], genesis_hash: &[u8; 32]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(64);
    msg.extend_from_slice(node_id);
    msg.extend_from_slice(genesis_hash);
    msg
}

fn bls_signature_from_bytes(bytes: &[u8]) -> NetworkResult<BlsSignature> {
    if bytes.len() != 96 {
        return Err(NetworkError::HandshakeFailed {
            reason: format!("BLS imzası 96 byte olmalı, gelen: {}", bytes.len()),
        });
    }
    let mut arr = [0u8; 96];
    arr.copy_from_slice(bytes);
    Ok(BlsSignature::from_bytes(arr))
}

fn bls_pubkey_from_bytes(bytes: &[u8]) -> NetworkResult<BlsPublicKey> {
    if bytes.len() != 48 {
        return Err(NetworkError::HandshakeFailed {
            reason: format!("BLS public key 48 byte olmalı, gelen: {}", bytes.len()),
        });
    }
    let mut arr = [0u8; 48];
    arr.copy_from_slice(bytes);
    Ok(BlsPublicKey::from_bytes(arr))
}

// ─── MessageHandler — Rolling Shutter pattern ─────────────────────────────────

/// Mesaj türü — Rolling Shutter `MessagePrototypes` benzeri
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageType {
    Handshake,
    Gossip,
    GossipResponse,
    Block,
    Transaction,
    /// MPC decryption key paylaşımı — Rolling Shutter DecryptionKeyShares
    MpcDecryptionShare,
    /// Eon public key — Rolling Shutter EonPublicKey
    EonPublicKey,
    /// Commitment — Rolling Shutter Commitment
    MpcCommitment,
    Ping,
    Pong,
}

/// Ağ mesajı zarfı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMessage {
    pub msg_type: MessageType,
    pub sender_id: [u8; 32],
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

impl NetworkMessage {
    pub fn new(msg_type: MessageType, sender_id: [u8; 32], payload: Vec<u8>) -> Self {
        Self { msg_type, sender_id, payload, timestamp: current_timestamp() }
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        serde_json::from_slice(bytes).map_err(|e| NetworkError::SerializationError {
            reason: e.to_string(),
        })
    }
}

// ─── Yardımcılar ──────────────────────────────────────────────────────────────

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// "nova-shield/0.1.0" → 1 gibi versiyon sayısı çıkar
fn parse_version(ver: &str) -> u32 {
    // "vX" veya "*/X.Y.Z" formatı
    ver.split('/').last()
        .and_then(|v| v.split('.').next())
        .and_then(|n| n.trim_start_matches('v').parse::<u32>().ok())
        .unwrap_or(0)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn genesis() -> [u8; 32] {
        [0xABu8; 32]
    }

    fn make_handshake(role: PeerRole) -> HandshakeMessage {
        HandshakeMessage::new(
            [1u8; 32],
            "127.0.0.1:30333",
            "nova-shield/0.1.0",
            "/nova-shield/1.0",
            genesis(),
            role,
        )
    }

    fn validator() -> HandshakeValidator {
        HandshakeValidator::new("/nova-shield/1.0", genesis())
    }

    #[test]
    fn test_valid_handshake() {
        let msg = make_handshake(PeerRole::Full);
        let result = validator().validate(&msg).unwrap();
        assert_eq!(result.peer_id, PeerId::from_bytes([1u8; 32]));
        assert_eq!(result.peer_role, PeerRole::Full);
        assert!(!result.is_validator);
    }

    /// Gerçek BLS imzalı validator handshake kabul edilmeli
    #[test]
    fn test_validator_handshake_with_real_bls() {
        let kp = nova_crypto::BlsKeypair::from_ikm(&[0x42u8; 32]).unwrap();
        let msg = make_handshake(PeerRole::Validator)
            .with_bls_validator_sig(&kp);
        let result = validator().validate(&msg).unwrap();
        assert!(result.is_validator, "Gerçek BLS imzalı validator kabul edilmeli");
    }

    /// NOVA-RED-NETWORK-002 regresyon: Sahte 96-byte BLS imzası reddedilmeli
    #[test]
    fn test_fake_bls_validator_rejected() {
        let fake_kp = nova_crypto::BlsKeypair::from_ikm(&[0x99u8; 32]).unwrap();
        let real_kp = nova_crypto::BlsKeypair::from_ikm(&[0x11u8; 32]).unwrap();

        // Gerçek node kimliği için farklı bir keypair ile imzala (sahtecilik)
        let mut msg = make_handshake(PeerRole::Validator);
        let bls_msg = bls_handshake_message(&msg.node_id, &msg.genesis_hash);
        let fake_sig = fake_kp.sign_raw(&bls_msg);
        // Gerçek pubkey ver ama yanlış keypair ile imzala
        msg.bls_signature = Some(fake_sig.as_bytes().to_vec());
        msg.bls_public_key = Some(real_kp.bls_public_key().as_bytes().to_vec());

        let err = validator().validate(&msg);
        assert!(
            matches!(err, Err(NetworkError::HandshakeFailed { .. })),
            "Sahte BLS imzası reddedilmeli: {err:?}"
        );
    }

    /// Validator rolü için BLS bilgisi eksikse reddedilmeli
    #[test]
    fn test_validator_without_bls_rejected() {
        let msg = make_handshake(PeerRole::Validator); // BLS yok
        let err = validator().validate(&msg);
        assert!(
            matches!(err, Err(NetworkError::HandshakeFailed { .. })),
            "BLS bilgisi olmayan validator reddedilmeli: {err:?}"
        );
    }

    #[test]
    fn test_wrong_protocol_id_rejected() {
        let msg = HandshakeMessage::new(
            [1u8; 32],
            "127.0.0.1:30333",
            "nova-shield/0.1.0",
            "/wrong-chain/1.0",
            genesis(),
            PeerRole::Full,
        );
        assert!(matches!(
            validator().validate(&msg),
            Err(NetworkError::ProtocolMismatch { .. })
        ));
    }

    #[test]
    fn test_wrong_genesis_rejected() {
        let msg = HandshakeMessage::new(
            [1u8; 32],
            "127.0.0.1:30333",
            "nova-shield/0.1.0",
            "/nova-shield/1.0",
            [0xFF; 32],
            PeerRole::Full,
        );
        assert!(matches!(
            validator().validate(&msg),
            Err(NetworkError::HandshakeFailed { .. })
        ));
    }

    #[test]
    fn test_handshake_encode_decode() {
        let msg = make_handshake(PeerRole::Validator).with_bls_signature(vec![0u8; 96]);
        let encoded = msg.encode();
        let decoded = HandshakeMessage::decode(&encoded).unwrap();
        assert_eq!(decoded.node_id, msg.node_id);
        assert_eq!(decoded.protocol_id, msg.protocol_id);
        assert!(decoded.bls_signature.is_some());
    }

    #[test]
    fn test_subnet_limit() {
        let subnets: Vec<String> = (0..20).map(|i| format!("subnet-{}", i)).collect();
        let msg = make_handshake(PeerRole::Full).with_subnets(subnets);
        // Maksimum 16 ile sınırlandırılmalı
        assert_eq!(msg.tracked_subnets.len(), 16);
    }

    #[test]
    fn test_too_many_subnets_in_validate() {
        let mut msg = make_handshake(PeerRole::Full);
        // with_subnets 16'ya kısar, direkt set ediyoruz
        msg.tracked_subnets = (0..20).map(|i| format!("s{}", i)).collect();
        assert!(matches!(
            validator().validate(&msg),
            Err(NetworkError::HandshakeFailed { .. })
        ));
    }

    #[test]
    fn test_network_message_encode_decode() {
        let msg = NetworkMessage::new(
            MessageType::MpcDecryptionShare,
            [5u8; 32],
            b"share_data".to_vec(),
        );
        let encoded = msg.encode();
        let decoded = NetworkMessage::decode(&encoded).unwrap();
        assert_eq!(decoded.msg_type, MessageType::MpcDecryptionShare);
        assert_eq!(decoded.payload, b"share_data");
    }

    #[test]
    fn test_message_types() {
        // Rolling Shutter mesaj tipleri mevcut mu?
        let types = [
            MessageType::MpcDecryptionShare,
            MessageType::EonPublicKey,
            MessageType::MpcCommitment,
        ];
        for t in &types {
            let msg = NetworkMessage::new(t.clone(), [0u8; 32], vec![]);
            let enc = msg.encode();
            let dec = NetworkMessage::decode(&enc).unwrap();
            assert_eq!(&dec.msg_type, t);
        }
    }
}
