//! Warp sync — checkpoint'ten hızlı senkronizasyon
//!
//! Polkadot SDK GRANDPA warp sync pattern:
//! Tüm blokları indirmek yerine justify edilmiş checkpoint'leri indir,
//! state snapshot'ını uygula, oradan devam et.

use super::{SyncEngine, SyncMode, SyncState};
use crate::errors::{NodeError, NodeResult};
use k256::ecdsa::{Signature, VerifyingKey, signature::Verifier};

// ─── WarpCheckpoint ───────────────────────────────────────────────────────────

/// Warp sync checkpoint — justify edilmiş blok
#[derive(Debug, Clone)]
pub struct WarpCheckpoint {
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
    /// Validator imzaları (2/3+1 quorum)
    pub justification: Vec<[u8; 64]>,
}

impl WarpCheckpoint {
    pub fn new(block_number: u64, block_hash: [u8; 32], state_root: [u8; 32]) -> Self {
        Self { block_number, block_hash, state_root, justification: vec![] }
    }

    /// İmza ekle
    pub fn add_justification(&mut self, sig: [u8; 64]) {
        self.justification.push(sig);
    }

    /// Temel sayım kontrolü: en az 3 justification var mı?
    pub fn is_valid(&self) -> bool {
        self.justification.len() >= 3
    }

    /// NOVA-RED-NODE-006: Bu checkpoint için imzalanacak mesajı hesapla.
    ///
    /// Format: blake3("nova-warp-v1" || le64(block_number) || block_hash || state_root)
    pub fn signing_message(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"nova-warp-v1");
        h.update(&self.block_number.to_le_bytes());
        h.update(&self.block_hash);
        h.update(&self.state_root);
        *h.finalize().as_bytes()
    }

    /// NOVA-RED-NODE-006: Gerçek ECDSA imza doğrulaması.
    ///
    /// Her imza, `signing_message()` üzerinde sıradaki validator public key ile
    /// doğrulanır. BFT quorum: `ceil(2/3 * n) + 1` geçerli imza gerekir.
    ///
    /// # Arguments
    /// * `validator_keys` — Doğrulayıcı secp256k1 public key listesi
    ///
    /// # Returns
    /// `true` iff yeterli sayıda imza doğrulandı.
    pub fn verify_justifications(&self, validator_keys: &[VerifyingKey]) -> bool {
        if validator_keys.is_empty() || self.justification.is_empty() {
            return false;
        }

        let msg = self.signing_message();
        // BFT quorum: 2/3 + 1 (yuvarlama: ceil(2n/3) + 1 — ancak en az 1)
        let n = validator_keys.len();
        let threshold = (n * 2 / 3) + 1;

        let mut valid_count = 0usize;
        for (i, sig_bytes) in self.justification.iter().enumerate() {
            let Some(key) = validator_keys.get(i) else { break };
            let Ok(sig) = Signature::from_slice(sig_bytes) else { continue };
            if key.verify(&msg, &sig).is_ok() {
                valid_count += 1;
            }
        }

        valid_count >= threshold
    }
}

// ─── WarpProof ────────────────────────────────────────────────────────────────

/// Warp proof: genesis'ten hedefe kadar checkpoint zinciri
#[derive(Debug, Clone)]
pub struct WarpProof {
    pub checkpoints: Vec<WarpCheckpoint>,
    pub target_block: u64,
}

impl WarpProof {
    pub fn new(target_block: u64) -> Self {
        Self { checkpoints: vec![], target_block }
    }

    pub fn add_checkpoint(&mut self, cp: WarpCheckpoint) {
        self.checkpoints.push(cp);
    }

    /// Proof geçerliliği: tüm checkpoint'ler valid mi?
    pub fn verify(&self) -> bool {
        !self.checkpoints.is_empty() && self.checkpoints.iter().all(|cp| cp.is_valid())
    }

    /// En yüksek checkpoint block number
    pub fn best_checkpoint_block(&self) -> u64 {
        self.checkpoints.iter().map(|cp| cp.block_number).max().unwrap_or(0)
    }
}

// ─── WarpSync ─────────────────────────────────────────────────────────────────

/// Warp sync motoru
pub struct WarpSync {
    state: SyncState,
    best_block: u64,
    /// NOVA-RED-NODE-007: Son alınan blok hash'i
    best_hash: [u8; 32],
    checkpoints_applied: u64,
    /// Warp'tan sonra full sync gerekli mi?
    needs_full_sync: bool,
}

impl WarpSync {
    pub fn new() -> Self {
        Self {
            state: SyncState::Idle,
            best_block: 0,
            best_hash: [0u8; 32],
            checkpoints_applied: 0,
            needs_full_sync: false,
        }
    }

    pub fn best_hash(&self) -> [u8; 32] { self.best_hash }

    /// Warp proof uygula
    pub fn apply_warp_proof(&mut self, proof: WarpProof) -> NodeResult<()> {
        if !proof.verify() {
            return Err(NodeError::SyncError {
                reason: "warp proof geçersiz: yeterli justification yok".into(),
            });
        }

        let best_cp_block = proof.best_checkpoint_block();
        self.best_block = best_cp_block;
        self.checkpoints_applied += proof.checkpoints.len() as u64;

        if best_cp_block < proof.target_block {
            // Checkpoint'ten hedefe kadar full sync gerekli
            self.needs_full_sync = true;
            self.state = SyncState::Syncing {
                from_block: best_cp_block,
                to_block: proof.target_block,
                current_block: best_cp_block,
            };
        } else {
            self.state = SyncState::Synced { best_block: best_cp_block };
        }

        Ok(())
    }

    pub fn checkpoints_applied(&self) -> u64 { self.checkpoints_applied }
    pub fn needs_full_sync(&self) -> bool { self.needs_full_sync }
}

impl SyncEngine for WarpSync {
    fn mode(&self) -> SyncMode { SyncMode::Warp }
    fn state(&self) -> &SyncState { &self.state }

    fn start_sync(&mut self, target_block: u64) -> NodeResult<()> {
        if target_block <= self.best_block {
            self.state = SyncState::Synced { best_block: self.best_block };
            return Ok(());
        }
        self.state = SyncState::Syncing {
            from_block: self.best_block,
            to_block: target_block,
            current_block: self.best_block,
        };
        Ok(())
    }

    fn process_block(&mut self, block_number: u64, block_hash: [u8; 32]) -> NodeResult<()> {
        // NOVA-RED-NODE-007: Sıfır hash reddedilir — warp sonrası full sync de aynı kural
        if block_hash == [0u8; 32] {
            return Err(NodeError::SyncError {
                reason: format!(
                    "geçersiz blok hash'i: blok {} için sıfır hash reddedildi (warp sync)",
                    block_number
                ),
            });
        }
        self.best_block = block_number;
        self.best_hash = block_hash;
        match &mut self.state {
            SyncState::Syncing { to_block, current_block, .. } => {
                *current_block = block_number;
                if block_number >= *to_block {
                    let best = *to_block;
                    self.state = SyncState::Synced { best_block: best };
                }
            }
            _ => {
                self.state = SyncState::Synced { best_block: block_number };
            }
        }
        Ok(())
    }

    fn best_block(&self) -> u64 { self.best_block }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_checkpoint(n: u64, sigs: usize) -> WarpCheckpoint {
        let mut cp = WarpCheckpoint::new(n, [n as u8; 32], [0u8; 32]);
        for _ in 0..sigs {
            cp.add_justification([0u8; 64]);
        }
        cp
    }

    /// NOVA-RED-NODE-006: Gerçek k256 ECDSA imzalarla checkpoint doğrulaması
    #[test]
    fn test_node006_verify_justifications_real_ecdsa() {
        use k256::ecdsa::{SigningKey, Signature as EcdsaSig, signature::Signer};
        use rand::rngs::OsRng;

        // 5 validator key oluştur
        let n = 5usize;
        let signing_keys: Vec<SigningKey> = (0..n)
            .map(|_| SigningKey::random(&mut OsRng))
            .collect();
        let verifying_keys: Vec<VerifyingKey> = signing_keys.iter()
            .map(|sk| *sk.verifying_key())
            .collect();

        let mut cp = WarpCheckpoint::new(1000, [0xABu8; 32], [0xCDu8; 32]);
        let msg = cp.signing_message();

        // Tüm 5 validator imzalar
        for sk in &signing_keys {
            let sig: EcdsaSig = sk.sign(&msg);
            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(sig.to_bytes().as_slice());
            cp.add_justification(sig_bytes);
        }

        // BFT quorum (2/3+1 = 4): 5 imza → geçerli
        assert!(cp.verify_justifications(&verifying_keys), "5/5 imza doğrulanmalı");
    }

    /// NOVA-RED-NODE-006: Sahte (sıfır) imzalar reddedilmeli
    #[test]
    fn test_node006_verify_justifications_fake_sigs_rejected() {
        use k256::ecdsa::VerifyingKey;
        use k256::SecretKey;
        use rand::rngs::OsRng;

        let n = 5usize;
        let verifying_keys: Vec<VerifyingKey> = (0..n)
            .map(|_| {
                use k256::elliptic_curve::sec1::ToEncodedPoint;
                let sk = SecretKey::random(&mut OsRng);
                let encoded = sk.public_key().to_encoded_point(true);
                VerifyingKey::from_sec1_bytes(encoded.as_bytes()).unwrap()
            })
            .collect();

        let mut cp = WarpCheckpoint::new(1000, [0xABu8; 32], [0xCDu8; 32]);
        // Sahte sıfır imzalar
        for _ in 0..5 {
            cp.add_justification([0u8; 64]);
        }

        // Sahte imzalar doğrulama başarısız olmalı
        assert!(!cp.verify_justifications(&verifying_keys), "sahte imzalar reddedilmeli");
    }

    /// NOVA-RED-NODE-006: Quorum eksikse reddedilmeli
    #[test]
    fn test_node006_verify_justifications_insufficient_quorum() {
        use k256::ecdsa::{SigningKey, Signature as EcdsaSig, signature::Signer};
        use rand::rngs::OsRng;

        let n = 5usize;
        let signing_keys: Vec<SigningKey> = (0..n)
            .map(|_| SigningKey::random(&mut OsRng))
            .collect();
        let verifying_keys: Vec<VerifyingKey> = signing_keys.iter()
            .map(|sk| *sk.verifying_key())
            .collect();

        let mut cp = WarpCheckpoint::new(1000, [0xABu8; 32], [0xCDu8; 32]);
        let msg = cp.signing_message();

        // Sadece 2 validator imzalar (threshold=4, yetersiz)
        for sk in signing_keys.iter().take(2) {
            let sig: EcdsaSig = sk.sign(&msg);
            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(sig.to_bytes().as_slice());
            cp.add_justification(sig_bytes);
        }

        assert!(!cp.verify_justifications(&verifying_keys), "2/5 quorum yetersiz (threshold=4)");
    }

    #[test]
    fn test_checkpoint_valid() {
        let cp = make_checkpoint(1000, 3);
        assert!(cp.is_valid());
    }

    #[test]
    fn test_checkpoint_invalid_insufficient_sigs() {
        let cp = make_checkpoint(1000, 2);
        assert!(!cp.is_valid());
    }

    #[test]
    fn test_warp_proof_verify() {
        let mut proof = WarpProof::new(5000);
        proof.add_checkpoint(make_checkpoint(1000, 4));
        proof.add_checkpoint(make_checkpoint(3000, 5));
        assert!(proof.verify());
        assert_eq!(proof.best_checkpoint_block(), 3000);
    }

    #[test]
    fn test_warp_proof_empty_invalid() {
        let proof = WarpProof::new(1000);
        assert!(!proof.verify());
    }

    #[test]
    fn test_warp_sync_apply_proof() {
        let mut sync = WarpSync::new();

        let mut proof = WarpProof::new(5000);
        proof.add_checkpoint(make_checkpoint(4500, 4));

        sync.apply_warp_proof(proof).unwrap();
        assert_eq!(sync.best_block(), 4500);
        assert!(sync.needs_full_sync());
        assert_eq!(sync.checkpoints_applied(), 1);
    }

    #[test]
    fn test_warp_sync_apply_proof_at_target() {
        let mut sync = WarpSync::new();

        let mut proof = WarpProof::new(5000);
        proof.add_checkpoint(make_checkpoint(5000, 3));

        sync.apply_warp_proof(proof).unwrap();
        assert!(sync.state().is_synced());
        assert!(!sync.needs_full_sync());
    }

    #[test]
    fn test_warp_sync_invalid_proof() {
        let mut sync = WarpSync::new();
        let mut proof = WarpProof::new(5000);
        proof.add_checkpoint(make_checkpoint(4000, 1)); // sadece 1 imza — geçersiz
        let result = sync.apply_warp_proof(proof);
        assert!(result.is_err());
    }

    #[test]
    fn test_warp_sync_process_blocks_after_warp() {
        let mut sync = WarpSync::new();

        let mut proof = WarpProof::new(5000);
        proof.add_checkpoint(make_checkpoint(4900, 4));
        sync.apply_warp_proof(proof).unwrap();

        // 4900'den 5000'e full sync — NOVA-RED-NODE-007: sıfır olmayan hash kullan
        for i in 4901u64..=5000 {
            let mut hash = [0u8; 32];
            hash[..8].copy_from_slice(&i.to_le_bytes());
            sync.process_block(i, hash).unwrap();
        }
        assert!(sync.state().is_synced());
    }

    /// NOVA-RED-NODE-007: Warp sonrası sıfır hash reddedilmeli
    #[test]
    fn test_warp_sync_zero_hash_rejected() {
        let mut sync = WarpSync::new();
        let mut proof = WarpProof::new(5000);
        proof.add_checkpoint(make_checkpoint(4900, 4));
        sync.apply_warp_proof(proof).unwrap();

        let result = sync.process_block(4901, [0u8; 32]);
        assert!(result.is_err(), "sıfır hash reddedilmeli");
    }

    #[test]
    fn test_warp_sync_mode() {
        let sync = WarpSync::new();
        assert!(sync.mode().is_warp());
    }
}
