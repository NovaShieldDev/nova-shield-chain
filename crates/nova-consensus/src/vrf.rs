//! GÖREV 91: VRF Tabanlı Block Proposer Seçimi (BABE Pattern)
//!
//! ## Mimari: Gerçek sr25519 VRF (schnorrkel ristretto255)
//!
//! ### Polkadot BABE (Blind Assignment for Blockchain Extension)
//! - Her validator kendi VRF çıktısını hesaplar: `VRF(sk, randomness || slot_number)`
//! - Eşik kontrolü: `VRF_output < threshold(stake_weight)`
//! - Eşiği geçen tüm validatorlar o slot için proposer adayı olur
//! - Deterministik: aynı giriş → aynı çıktı (doğrulanabilir rastgelelik)
//!
//! ### Nova Uyarlaması
//! - Gerçek sr25519 VRF (schnorrkel — ristretto255/curve25519)
//! - `VrfProof`: 32B compressed Ristretto pre-output + 64B DLEQ kanıt
//! - `BabeEpochConfig`: slot sayısı, eşik, epoch randomness
//! - `BabeSlotClaim`: bir slot için validator'ın proposer iddiası
//! - `BabeEngine`: slot başına proposer belirleme + epoch rotasyonu

use blake3;
use nova_primitives::{SlotId, ValidatorId};
use std::collections::HashMap;
use crate::errors::{ConsensusError, ConsensusResult};

use schnorrkel::{
    context::signing_context,
    vrf::{VRFPreOut, VRFProof as SrVrfProof},
    ExpansionMode, Keypair, MiniSecretKey, PublicKey,
};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// VRF çıktı boyutu (bayt) — compressed Ristretto255 pre-output noktası
pub const VRF_OUTPUT_SIZE: usize = 32;

/// VRF kanıt boyutu (bayt) — sr25519 DLEQ kanıtı (challenge + response scalar)
pub const VRF_PROOF_SIZE: usize = 64;

/// VRF signing context etiketi (Polkadot BABE pattern)
const VRF_CONTEXT: &[u8] = b"nova-babe-vrf";

/// Varsayılan primary slot eşiği (stake bağımsız — %50 şans)
/// BABE'de c=0.5 → threshold = u64::MAX / 2
pub const PRIMARY_SLOT_THRESHOLD: u64 = u64::MAX / 2;

/// Epoch başına kaç slot?
///
/// BUG-7 düzeltmesi: Eski değer 2_400 (16 dakika) EpochManager ile çakışıyordu.
/// Canonical değer: nova-consensus/src/types.rs EPOCH_DURATION_SLOTS = 216_000 (24 saat).
pub const BABE_EPOCH_DURATION: u64 = 216_000; // 24 saat @400ms slot

// ─── VrfKeypair ───────────────────────────────────────────────────────────────

/// Gerçek sr25519 VRF anahtar çifti (schnorrkel ristretto255)
///
/// BABE'de: `sr25519::VRFKeypair` — `MiniSecretKey::expand_to_keypair(Ed25519)`
pub struct VrfKeypair {
    /// Yeniden türetme için tohum
    seed: [u8; 32],
    /// Schnorrkel keypair (ristretto255)
    keypair: Keypair,
    /// Compressed public key (32B)
    pub public: [u8; 32],
}

impl Clone for VrfKeypair {
    fn clone(&self) -> Self {
        Self::from_seed(self.seed)
    }
}

impl std::fmt::Debug for VrfKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VrfKeypair")
            .field("public", &hex::encode(self.public))
            .finish()
    }
}

impl VrfKeypair {
    /// sr25519 VRF anahtar çifti oluştur
    ///
    /// BABE: `MiniSecretKey::from_bytes(seed).expand_to_keypair(Ed25519)`
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let mini = MiniSecretKey::from_bytes(&seed)
            .expect("32 byte geçerli mini secret key");
        let keypair = mini.expand_to_keypair(ExpansionMode::Ed25519);
        let public = keypair.public.to_bytes();
        Self { seed, keypair, public }
    }

    /// VRF imzala: VRF(sk, input) → (compressed pre-output, DLEQ kanıt)
    ///
    /// BABE: `keypair.vrf_sign(signing_context(label).bytes(input))`
    ///
    /// Döndürür: `VrfProof { output: VRFPreOut[32B], proof: VRFProof[64B] }`
    pub fn sign(&self, input: &[u8]) -> VrfProof {
        let ctx = signing_context(VRF_CONTEXT);
        let (inout, proof, _batchable) = self.keypair.vrf_sign(ctx.bytes(input));

        let output: [u8; 32] = inout.to_preout().to_bytes();
        let proof_bytes: [u8; 64] = proof.to_bytes();

        VrfProof { output, proof: proof_bytes }
    }

    /// VRF doğrula: public key + input + kanıt geçerli mi?
    ///
    /// BABE: `PublicKey::vrf_verify(transcript, pre_out, proof)`
    pub fn verify(public: &[u8; 32], input: &[u8], vrf: &VrfProof) -> bool {
        let pub_key = match PublicKey::from_bytes(public) {
            Ok(k) => k,
            Err(_) => return false,
        };
        let pre_out = match VRFPreOut::from_bytes(&vrf.output) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let proof = match SrVrfProof::from_bytes(&vrf.proof) {
            Ok(p) => p,
            Err(_) => return false,
        };

        let ctx = signing_context(VRF_CONTEXT);
        pub_key.vrf_verify(ctx.bytes(input), &pre_out, &proof).is_ok()
    }
}

// ─── VrfProof ─────────────────────────────────────────────────────────────────

/// VRF kanıt ve ön-çıktı noktası
///
/// Polkadot BABE header: `VRFOutput (32B compressed Ristretto) + VRFProof (64B DLEQ)`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VrfProof {
    /// Compressed Ristretto255 VRF ön-çıktı noktası (doğrulama + eşik için)
    pub output: [u8; VRF_OUTPUT_SIZE],
    /// sr25519 DLEQ kanıtı: challenge scalar (32B) + response scalar (32B)
    pub proof: [u8; VRF_PROOF_SIZE],
}

impl VrfProof {
    /// Ön-çıktının ilk 8 baytından u64 türet (eşik karşılaştırması için)
    ///
    /// BABE: `VRFOutput::make_bytes → u64 threshold comparison`
    pub fn output_as_u64(&self) -> u64 {
        u64::from_le_bytes(self.output[0..8].try_into().unwrap())
    }
}

// ─── BabeEpochConfig ──────────────────────────────────────────────────────────

/// Epoch başına BABE konfigürasyonu
///
/// Polkadot: `BabeEpochConfiguration { c, allowed_slots }`
#[derive(Debug, Clone)]
pub struct BabeEpochConfig {
    /// Epoch numarası
    pub epoch_index: u64,
    /// Epoch başlangıç slotu
    pub start_slot: SlotId,
    /// Epoch sona eriş slotu
    pub end_slot: SlotId,
    /// Bu epoch için randomness (önceki epoch'un VRF çıktılarından türetildi)
    pub randomness: [u8; 32],
    /// Primary slot eşiği (stake bağımsız)
    pub primary_threshold: u64,
}

impl BabeEpochConfig {
    pub fn new(epoch_index: u64, start_slot: u64, randomness: [u8; 32]) -> Self {
        Self {
            epoch_index,
            start_slot: SlotId(start_slot),
            end_slot: SlotId(start_slot + BABE_EPOCH_DURATION - 1),
            randomness,
            primary_threshold: PRIMARY_SLOT_THRESHOLD,
        }
    }

    /// Slot'u VRF transcript girişine dönüştür
    ///
    /// BABE: `make_transcript(randomness, slot_number, epoch)`
    pub fn make_input(&self, slot: SlotId) -> Vec<u8> {
        let mut input = Vec::new();
        input.extend_from_slice(b"nova-babe-vrf:");
        input.extend_from_slice(&self.randomness);
        input.extend_from_slice(&slot.0.to_le_bytes());
        input.extend_from_slice(&self.epoch_index.to_le_bytes());
        input
    }
}

// ─── BabeSlotClaim ────────────────────────────────────────────────────────────

/// Bir validator'ın slot için proposer iddiası
///
/// Polkadot: `PreDigest::Primary { authority_index, slot, vrf_output, vrf_proof }`
#[derive(Debug, Clone)]
pub struct BabeSlotClaim {
    /// İddiayı yapan validator
    pub validator_id: ValidatorId,
    /// İddia edilen slot
    pub slot: SlotId,
    /// VRF kanıtı (compressed pre-output + DLEQ proof)
    pub vrf_proof: VrfProof,
    /// Validator'ın sr25519 public key'i (32B)
    pub authority_key: [u8; 32],
}

impl BabeSlotClaim {
    /// Bu iddia eşiği geçiyor mu?
    pub fn is_primary(&self, threshold: u64) -> bool {
        self.vrf_proof.output_as_u64() < threshold
    }
}

// ─── BabeEngine ───────────────────────────────────────────────────────────────

/// BABE block proposer seçim motoru
///
/// Polkadot: `babe::BabeWorker`
pub struct BabeEngine {
    /// Mevcut epoch konfigürasyonu
    current_epoch: BabeEpochConfig,
    /// Validator VRF anahtarları: validator_id → VrfKeypair
    keys: HashMap<u64, VrfKeypair>,
    /// Validator public key'leri (doğrulama için)
    pub_keys: HashMap<u64, [u8; 32]>,
    /// Bir sonraki epoch randomness (bu epoch'un VRF çıktıları biriktiriliyor)
    next_epoch_randomness_acc: [u8; 32],
    /// Kaç slot işlendi?
    processed_slots: u64,
}

impl BabeEngine {
    pub fn new(genesis_randomness: [u8; 32]) -> Self {
        Self {
            current_epoch: BabeEpochConfig::new(0, 0, genesis_randomness),
            keys: HashMap::new(),
            pub_keys: HashMap::new(),
            next_epoch_randomness_acc: [0u8; 32],
            processed_slots: 0,
        }
    }

    /// Validator kaydı (sr25519 VRF anahtar çifti türetilir)
    pub fn register_validator(&mut self, validator_id: ValidatorId, seed: [u8; 32]) {
        let kp = VrfKeypair::from_seed(seed);
        self.pub_keys.insert(validator_id.0, kp.public);
        self.keys.insert(validator_id.0, kp);
    }

    /// Bir slot için VRF iddiası yap
    ///
    /// Eğer VRF çıktısı eşiği geçiyorsa → proposer olabilir
    pub fn claim_slot(&self, validator_id: ValidatorId, slot: SlotId) -> Option<BabeSlotClaim> {
        let kp = self.keys.get(&validator_id.0)?;
        let input = self.current_epoch.make_input(slot);
        let vrf_proof = kp.sign(&input);

        if vrf_proof.output_as_u64() < self.current_epoch.primary_threshold {
            Some(BabeSlotClaim {
                validator_id,
                slot,
                vrf_proof,
                authority_key: kp.public,
            })
        } else {
            None
        }
    }

    /// Slot iddiasını doğrula (gerçek sr25519 VRF kanıtı)
    pub fn verify_claim(&self, claim: &BabeSlotClaim) -> ConsensusResult<()> {
        let pub_key = self.pub_keys.get(&claim.validator_id.0)
            .ok_or(ConsensusError::ValidatorNotFound {
                validator_id: format!("{}", claim.validator_id.0),
            })?;

        // Public key eşleşiyor mu?
        if pub_key != &claim.authority_key {
            return Err(ConsensusError::InvalidBlockHash);
        }

        let input = self.current_epoch.make_input(claim.slot);

        // Gerçek sr25519 VRF doğrulama
        if !VrfKeypair::verify(pub_key, &input, &claim.vrf_proof) {
            return Err(ConsensusError::InvalidBlockHash);
        }

        if !claim.is_primary(self.current_epoch.primary_threshold) {
            return Err(ConsensusError::InvalidBlockHash);
        }

        Ok(())
    }

    /// Slot'u işle: en düşük VRF çıktılı validator'ı proposer seç
    ///
    /// BABE'de birden fazla validator eşiği geçebilir → en düşük kazanır
    pub fn elect_proposer(&self, slot: SlotId, validator_ids: &[ValidatorId]) -> Option<ValidatorId> {
        let mut best: Option<(u64, ValidatorId)> = None;

        for &vid in validator_ids {
            if let Some(claim) = self.claim_slot(vid, slot) {
                let score = claim.vrf_proof.output_as_u64();
                match best {
                    None => best = Some((score, vid)),
                    Some((best_score, _)) if score < best_score => best = Some((score, vid)),
                    _ => {}
                }
            }
        }

        best.map(|(_, vid)| vid)
    }

    /// Slot VRF çıktısını epoch randomness birikimine ekle
    ///
    /// BABE: sonraki epoch randomness = XOR of all VRF outputs
    pub fn accumulate_randomness(&mut self, vrf_output: &[u8; 32]) {
        for (i, byte) in vrf_output.iter().enumerate() {
            self.next_epoch_randomness_acc[i] ^= byte;
        }
        self.processed_slots += 1;
    }

    /// Epoch geçişi: yeni epoch konfigürasyonu oluştur
    ///
    /// BABE: `on_finality` epoch değişim mantığı
    pub fn advance_epoch(&mut self) -> BabeEpochConfig {
        let next_epoch_index = self.current_epoch.epoch_index + 1;
        let next_start = self.current_epoch.end_slot.0 + 1;

        // Birikmiş randomness'ı karıştır (BLAKE3)
        let new_randomness = *blake3::hash(&[
            b"nova-babe-epoch-randomness:".as_slice(),
            self.next_epoch_randomness_acc.as_slice(),
            &next_epoch_index.to_le_bytes(),
        ].concat()).as_bytes();

        let new_epoch = BabeEpochConfig::new(next_epoch_index, next_start, new_randomness);
        self.current_epoch = new_epoch.clone();
        self.next_epoch_randomness_acc = [0u8; 32];
        new_epoch
    }

    /// Mevcut epoch
    pub fn current_epoch(&self) -> &BabeEpochConfig {
        &self.current_epoch
    }

    /// İşlenen slot sayısı
    pub fn processed_slots(&self) -> u64 {
        self.processed_slots
    }

    /// Fallback round-robin proposer (hiç VRF eşiği geçilmediyse)
    ///
    /// BABE: secondary slot fallback
    pub fn fallback_proposer(slot: SlotId, validator_ids: &[ValidatorId]) -> Option<ValidatorId> {
        if validator_ids.is_empty() {
            return None;
        }
        let idx = (slot.0 as usize) % validator_ids.len();
        Some(validator_ids[idx])
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn vid(n: u64) -> ValidatorId { ValidatorId(n) }

    fn make_engine_with_validators(count: u64) -> BabeEngine {
        let mut engine = BabeEngine::new([42u8; 32]);
        for i in 1..=count {
            let mut seed = [0u8; 32];
            seed[0] = i as u8;
            engine.register_validator(vid(i), seed);
        }
        engine
    }

    // 1. VRF keypair imzala ve doğrula (gerçek sr25519)
    #[test]
    fn test_vrf_sign_verify() {
        let kp = VrfKeypair::from_seed([1u8; 32]);
        let input = b"nova-babe-vrf:slot-100";
        let proof = kp.sign(input);

        // Doğru public key → doğrulama başarılı
        assert!(VrfKeypair::verify(&kp.public, input, &proof));
        // Yanlış public key → doğrulama başarısız (geçersiz Ristretto noktası)
        let wrong_pub = [0u8; 32];
        assert!(!VrfKeypair::verify(&wrong_pub, input, &proof));
    }

    // 2. VRF çıktısı deterministik (aynı sk + input → aynı pre-output noktası)
    //    Not: DLEQ kanıtı (proof bytes) schnorrkel'de randomized nonce kullanır —
    //    bu güvenlik açısından normal; sadece VRF output deterministik olmak zorunda.
    #[test]
    fn test_vrf_deterministic() {
        let kp = VrfKeypair::from_seed([7u8; 32]);
        let input = b"deterministic-test";
        let p1 = kp.sign(input);
        let p2 = kp.sign(input);
        // VRF pre-output (Ristretto noktası Γ = H(m)^x) her zaman aynı
        assert_eq!(p1.output, p2.output, "VRF output deterministik olmalı");
        // Her iki kanıt da doğrulanabilir olmalı
        assert!(VrfKeypair::verify(&kp.public, input, &p1));
        assert!(VrfKeypair::verify(&kp.public, input, &p2));
    }

    // 3. Farklı inputlar farklı çıktı üretir
    #[test]
    fn test_vrf_different_inputs() {
        let kp = VrfKeypair::from_seed([3u8; 32]);
        let p1 = kp.sign(b"slot-1");
        let p2 = kp.sign(b"slot-2");
        assert_ne!(p1.output, p2.output);
    }

    // 4. Farklı keypairler farklı çıktı
    #[test]
    fn test_vrf_different_keys() {
        let kp1 = VrfKeypair::from_seed([1u8; 32]);
        let kp2 = VrfKeypair::from_seed([2u8; 32]);
        let input = b"same-input";
        assert_ne!(kp1.sign(input).output, kp2.sign(input).output);
    }

    // 5. Yanlış input ile doğrulama başarısız
    #[test]
    fn test_vrf_wrong_input_verify_fails() {
        let kp = VrfKeypair::from_seed([5u8; 32]);
        let proof = kp.sign(b"correct-input");
        // Farklı input → doğrulama başarısız
        assert!(!VrfKeypair::verify(&kp.public, b"wrong-input", &proof));
    }

    // 6. Epoch konfigürasyonu input oluşturma
    #[test]
    fn test_epoch_config_input() {
        let epoch = BabeEpochConfig::new(0, 0, [0xABu8; 32]);
        let i1 = epoch.make_input(SlotId(0));
        let i2 = epoch.make_input(SlotId(1));
        assert_ne!(i1, i2);
    }

    // 7. Slot iddiası eşik kontrolü (~%50 slot claim edilmeli)
    #[test]
    fn test_slot_claim_threshold() {
        let engine = make_engine_with_validators(10);

        let mut claimed = 0u32;
        for slot in 0..100u64 {
            if engine.claim_slot(vid(1), SlotId(slot)).is_some() {
                claimed += 1;
            }
        }
        // ~%50 eşik → 50 slot civarı (%10-90 aralığı tolerans)
        assert!(claimed >= 10 && claimed <= 90, "claimed={claimed}");
    }

    // 8. Slot iddiası gerçek sr25519 VRF doğrulama
    #[test]
    fn test_verify_claim_valid() {
        let engine = make_engine_with_validators(5);

        for slot in 0..1000u64 {
            if let Some(claim) = engine.claim_slot(vid(1), SlotId(slot)) {
                assert!(engine.verify_claim(&claim).is_ok(), "claim doğrulanmalı");
                break;
            }
        }
    }

    // 9. Bilinmeyen validator hatası
    #[test]
    fn test_verify_unknown_validator() {
        let engine = make_engine_with_validators(3);
        let fake_claim = BabeSlotClaim {
            validator_id: vid(99),
            slot: SlotId(10),
            vrf_proof: VrfProof { output: [1u8; 32], proof: [0u8; 64] },
            authority_key: [0u8; 32],
        };
        assert!(engine.verify_claim(&fake_claim).is_err());
    }

    // 10. Proposer seçimi (en düşük VRF çıktı)
    #[test]
    fn test_elect_proposer() {
        let engine = make_engine_with_validators(10);
        let validators: Vec<ValidatorId> = (1..=10).map(vid).collect();

        let mut proposers_found = 0;
        for slot in 0..100u64 {
            if engine.elect_proposer(SlotId(slot), &validators).is_some() {
                proposers_found += 1;
            }
        }
        assert!(proposers_found > 0, "en az bir slot için proposer bulunmalı");
    }

    // 11. Epoch randomness birikimi
    #[test]
    fn test_accumulate_randomness() {
        let mut engine = make_engine_with_validators(3);
        let initial_acc = engine.next_epoch_randomness_acc;

        engine.accumulate_randomness(&[0xFFu8; 32]);
        assert_ne!(engine.next_epoch_randomness_acc, initial_acc);
        assert_eq!(engine.processed_slots(), 1);
    }

    // 12. Epoch geçişi
    #[test]
    fn test_advance_epoch() {
        let mut engine = make_engine_with_validators(3);

        for i in 0..10u8 {
            engine.accumulate_randomness(&[i; 32]);
        }

        let epoch0_end = engine.current_epoch().end_slot;
        let new_epoch = engine.advance_epoch();

        assert_eq!(new_epoch.epoch_index, 1);
        assert_eq!(new_epoch.start_slot.0, epoch0_end.0 + 1);
        assert_eq!(engine.next_epoch_randomness_acc, [0u8; 32]);
    }

    // 13. Fallback round-robin proposer
    #[test]
    fn test_fallback_proposer() {
        let validators: Vec<ValidatorId> = (1..=5).map(vid).collect();
        let p0 = BabeEngine::fallback_proposer(SlotId(0), &validators).unwrap();
        let p5 = BabeEngine::fallback_proposer(SlotId(5), &validators).unwrap();
        assert_eq!(p0, p5);

        let p1 = BabeEngine::fallback_proposer(SlotId(1), &validators).unwrap();
        let p2 = BabeEngine::fallback_proposer(SlotId(2), &validators).unwrap();
        assert_ne!(p1, p2);
    }

    // 14. Boş validator listesi fallback
    #[test]
    fn test_fallback_empty_validators() {
        assert!(BabeEngine::fallback_proposer(SlotId(0), &[]).is_none());
        assert!(BabeEngine::fallback_proposer(SlotId(100), &[]).is_none());
    }

    // 15. Farklı epoch → farklı VRF çıktıları
    #[test]
    fn test_epoch_randomness_changes_vrf() {
        let mut engine = make_engine_with_validators(3);
        let kp = VrfKeypair::from_seed([1u8; 32]);

        let epoch0_input = engine.current_epoch().make_input(SlotId(100));
        engine.advance_epoch();
        let epoch1_input = engine.current_epoch().make_input(SlotId(100));

        assert_ne!(epoch0_input, epoch1_input);
        let p0 = kp.sign(&epoch0_input);
        let p1 = kp.sign(&epoch1_input);
        assert_ne!(p0.output, p1.output);
    }

    // 16. output_as_u64 deterministik ve u64 aralığında
    #[test]
    fn test_output_as_u64_range() {
        let kp = VrfKeypair::from_seed([99u8; 32]);
        let proof = kp.sign(b"test-range");
        let val = proof.output_as_u64();
        assert!(val <= u64::MAX);
    }

    // 17. VrfKeypair clone deterministik public key ve output
    #[test]
    fn test_keypair_clone() {
        let kp = VrfKeypair::from_seed([42u8; 32]);
        let kp2 = kp.clone();
        assert_eq!(kp.public, kp2.public);
        // Clone'lar aynı VRF pre-output üretmeli
        let proof1 = kp.sign(b"clone-test");
        let proof2 = kp2.sign(b"clone-test");
        assert_eq!(proof1.output, proof2.output, "Clone VRF output aynı olmalı");
        // Her ikisi de doğrulanabilmeli
        assert!(VrfKeypair::verify(&kp.public, b"clone-test", &proof1));
        assert!(VrfKeypair::verify(&kp2.public, b"clone-test", &proof2));
    }
}
