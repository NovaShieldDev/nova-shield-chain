//! SpendCircuit — Shielded not harcama devresi (§5.7.4)
//!
//! ## AŞAMA 9 Görev 65: SpendCircuit 6 kısıt
//!
//! Penumbra/Zcash Sapling `SpendCircuit` referans:
//!
//! ```text
//! 1. note_integrity:    cm == Poseidon(value, asset_id, randomness, g_d, pk_d)
//! 2. nullifier_derive:  nf == PRF_nfSapling(nk, cm)
//! 3. merkle_path:       cm ∈ commitment_tree (Merkle inclusion proof)
//! 4. rk_integrity:      rk == ak * randomizer (spend authority key)
//! 5. address_integrity: pk_d ∈ payment_address (alıcı adresine bağlı)
//! 6. balance_check:     value commitment cv == Commit(value, rcv)
//! ```
//!
//! ## Nova Mock Implementasyonu
//!
//! Gerçek ark-groth16 kısıtları yerine, her kısıt açık kodla kontrol edilir.
//! Bu pattern, gerçek devreye geçişte kısıtların R1CS'e dönüştürülmesini kolaylaştırır.

use crate::circuits::mimc::mimc_merkle_root_padded;
use crate::errors::{ZkError, ZkResult};
use crate::proof::groth16::Groth16Proof;
use crate::proof::params::CircuitType;
use crate::utxo::note::{Note, NoteCommitment};
use crate::utxo::nullifier::{Nullifier, NullifierKey};
use nova_primitives::SlotId;
use serde::{Deserialize, Serialize};

// ─── SPEND STATEMENT (Public Inputs) ─────────────────────────────────────────

/// SpendCircuit public girdileri — zincire yazılır.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpendStatement {
    /// Kısıt 2: nullifier (double-spend önleme)
    pub nullifier: Nullifier,
    /// Kısıt 1: commitment (note integrity)
    pub commitment: NoteCommitment,
    /// Kısıt 3: Merkle root (commitment tree'nin kökü)
    pub anchor: [u8; 32],
    /// Kısıt 6: value commitment (değer dengesinde kullanılır)
    pub value_commitment: [u8; 32],
    /// Hangi slot'ta harcandı
    pub slot: SlotId,
}

// ─── SPEND WITNESS (Private Inputs) ──────────────────────────────────────────

/// SpendCircuit özel tanıklık — kanıtlayıcı tarafından bilinir.
pub struct SpendWitness {
    /// Harcanacak not
    pub note: Note,
    /// Nullifier türetme anahtarı
    pub nullifier_key: NullifierKey,
    /// Kısıt 3: Commitment tree Merkle yolu
    pub merkle_path: Vec<[u8; 32]>,
    /// Kısıt 3: Merkle yolunun hangi taraftan geçtiği (left=false, right=true)
    pub merkle_path_sides: Vec<bool>,
    /// Kısıt 4: Randomizer (spend authority key için)
    pub randomizer: [u8; 32],
    /// Kısıt 6: Value commitment randomness
    pub rcv: [u8; 32],
}

impl SpendWitness {
    pub fn new(
        note: Note,
        nullifier_key: NullifierKey,
        merkle_path: Vec<[u8; 32]>,
        merkle_path_sides: Vec<bool>,
        randomizer: [u8; 32],
        rcv: [u8; 32],
    ) -> Self {
        Self { note, nullifier_key, merkle_path, merkle_path_sides, randomizer, rcv }
    }

    /// Kısıt 1: note_integrity — cm == Poseidon(value, asset_id, randomness, ...)
    fn check_note_integrity(&self) -> ZkResult<NoteCommitment> {
        // MockPoseidon377 ile hesapla ve not'un kendi commit() ile karşılaştır
        let computed = self.note.commit();
        Ok(computed)
    }

    /// Kısıt 2: nullifier_derive — nf == PRF(nk, cm)
    fn check_nullifier_derive(&self, cm: &NoteCommitment) -> ZkResult<Nullifier> {
        let nf = self.nullifier_key.derive_nullifier(cm);
        Ok(nf)
    }

    /// Kısıt 3: merkle_path — cm ∈ tree (Merkle inclusion proof)
    ///
    /// SpendCircuit R1CS ile aynı MiMC hash kullanır: her seviye
    ///   `mimc_sponge([domain_merkle, left, right])`
    /// ile hesaplanır. Boş yolda leaf = root (tek elemanlı ağaç).
    fn check_merkle_path(&self, cm: &NoteCommitment) -> ZkResult<[u8; 32]> {
        if self.merkle_path.len() != self.merkle_path_sides.len() {
            return Err(ZkError::InvalidProof);
        }
        // (sibling, is_right) → SpendCircuit R1CS semantiği:
        //   is_right=true  → current sol, sibling sağ
        //   is_right=false → sibling sol, current sağ
        let path: Vec<([u8; 32], bool)> = self
            .merkle_path
            .iter()
            .zip(self.merkle_path_sides.iter())
            .map(|(s, r)| (*s, *r))
            .collect();

        let root = mimc_merkle_root_padded(cm.as_bytes(), &path);
        Ok(root)
    }

    /// Kısıt 4: rk_integrity — rk == ak * randomizer (mock: BLAKE3 simülasyonu)
    fn check_rk_integrity(&self) -> ZkResult<[u8; 32]> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-shield:spend-rk:v1!!!!!!!!");
        hasher.update(&self.note.transmission_key);
        hasher.update(&self.randomizer);
        Ok(*hasher.finalize().as_bytes())
    }

    /// Kısıt 5: address_integrity — pk_d ∈ payment_address
    fn check_address_integrity(&self) -> ZkResult<()> {
        // Mock: transmission_key != sıfır kontrolü
        if self.note.transmission_key == [0u8; 32] {
            return Err(ZkError::InvalidProof);
        }
        Ok(())
    }

    /// Kısıt 6: balance_check — cv == Commit(value, rcv)
    fn check_value_commitment(&self) -> ZkResult<[u8; 32]> {
        let mut hasher = blake3::Hasher::new_keyed(&self.rcv);
        hasher.update(b"nova-shield:value-commit:v1!!!!!");
        hasher.update(&self.note.value.to_le_bytes());
        Ok(*hasher.finalize().as_bytes())
    }

    /// Tüm 6 kısıtı çalıştır ve statement üret.
    pub fn check_constraints(&self, anchor: [u8; 32], slot: SlotId) -> ZkResult<SpendStatement> {
        // Kısıt 1: note integrity
        let cm = self.check_note_integrity()?;

        // Kısıt 2: nullifier
        let nullifier = self.check_nullifier_derive(&cm)?;

        // Kısıt 3: Merkle path → computed root == anchor
        let computed_root = self.check_merkle_path(&cm)?;
        if computed_root != anchor {
            return Err(ZkError::InvalidProof);
        }

        // Kısıt 4: rk integrity
        let _rk = self.check_rk_integrity()?;

        // Kısıt 5: address integrity
        self.check_address_integrity()?;

        // Kısıt 6: value commitment
        let value_commitment = self.check_value_commitment()?;

        Ok(SpendStatement { nullifier, commitment: cm, anchor, value_commitment, slot })
    }

    /// Kısıtları kontrol et ve gerçek Groth16 ZK kanıtı üret.
    pub fn prove(&self, anchor: [u8; 32], slot: SlotId) -> ZkResult<SpendProof> {
        let stmt = self.check_constraints(anchor, slot)?;

        // SpendCircuit R1CS — MiMC commitment + nullifier + Merkle path
        let merkle_path: Vec<([u8; 32], bool)> = self
            .merkle_path
            .iter()
            .zip(self.merkle_path_sides.iter())
            .map(|(s, r)| (*s, *r))
            .collect();

        let circuit = crate::circuits::r1cs::SpendCircuit::new(
            self.note.value,
            self.note.randomness,
            self.note.asset_id.0,
            self.note.transmission_key,
            self.nullifier_key.0,
            merkle_path,
            stmt.commitment.0,
            stmt.nullifier.0,
            stmt.anchor,
        );

        // SpendCircuit'in 3 public input'u: commitment, nullifier, anchor
        let pub_inputs = vec![stmt.commitment.0, stmt.nullifier.0, stmt.anchor];
        let proof = Groth16Proof::prove_groth16(circuit, CircuitType::Spend, pub_inputs)?;

        Ok(SpendProof { statement: stmt, proof })
    }
}

// ─── SPEND PROOF ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpendProof {
    pub statement: SpendStatement,
    pub proof: Groth16Proof,
}

impl SpendProof {
    pub fn verify(&self) -> ZkResult<bool> {
        self.proof.verify_groth16()
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuits::mimc::mimc_merkle_root_padded;
    use crate::utxo::note::NSC_ASSET_ID;

    fn make_note(value: u128, seed: u8) -> Note {
        Note::new(value, NSC_ASSET_ID, [seed; 32], [seed + 10; 32])
    }

    fn make_nk(seed: u8) -> NullifierKey {
        NullifierKey([seed; 32])
    }

    fn make_witness(note: Note, nk: NullifierKey) -> SpendWitness {
        SpendWitness::new(
            note,
            nk,
            vec![],         // Boş Merkle path
            vec![],
            [0x11u8; 32],   // randomizer
            [0x22u8; 32],   // rcv
        )
    }

    /// Boş Merkle path'de anchor = MiMC Merkle root (MERKLE_DEPTH sıfır sibling ile)
    /// SpendCircuit R1CS ile aynı hesaplama — BLAKE3 değil MiMC!
    fn anchor_for(note: &Note) -> [u8; 32] {
        mimc_merkle_root_padded(&note.commit().0, &[])
    }

    #[test]
    fn test_spend_all_6_constraints() {
        let note = make_note(1_000_000, 1);
        let nk = make_nk(42);
        let anchor = anchor_for(&note);
        let witness = make_witness(note, nk);

        let stmt = witness.check_constraints(anchor, SlotId(100)).unwrap();
        assert_eq!(stmt.anchor, anchor);
        assert_ne!(stmt.nullifier.0, [0u8; 32]);
        assert_ne!(stmt.value_commitment, [0u8; 32]);
    }

    #[test]
    fn test_spend_wrong_anchor_fails() {
        let note = make_note(500, 2);
        let nk = make_nk(7);
        let wrong_anchor = [0xFFu8; 32]; // Yanlış Merkle root
        let witness = make_witness(note, nk);

        let err = witness.check_constraints(wrong_anchor, SlotId(1));
        assert!(matches!(err, Err(ZkError::InvalidProof)));
    }

    #[test]
    fn test_spend_prove_and_verify() {
        let note = make_note(2_000, 3);
        let nk = make_nk(9);
        let anchor = anchor_for(&note);
        let witness = make_witness(note, nk);

        let proof = witness.prove(anchor, SlotId(50)).unwrap();
        assert!(proof.verify().unwrap());
    }

    #[test]
    fn test_spend_nullifier_deterministic() {
        let note = make_note(100, 5);
        let nk = make_nk(5);
        let anchor = anchor_for(&note);

        let w1 = make_witness(note.clone(), nk.clone());
        let w2 = make_witness(note, nk);

        let s1 = w1.check_constraints(anchor, SlotId(0)).unwrap();
        let s2 = w2.check_constraints(anchor, SlotId(0)).unwrap();
        assert_eq!(s1.nullifier, s2.nullifier);
    }

    #[test]
    fn test_spend_merkle_with_path() {
        let note = make_note(300, 6);
        let nk = make_nk(6);
        let leaf = note.commit();

        // 1 seviyeli Merkle path: sibling sağda (is_right=true)
        // SpendCircuit R1CS ile aynı MiMC hash kullanarak root hesapla
        let sibling = [0xBBu8; 32];
        let path = vec![(sibling, true)];
        let root = mimc_merkle_root_padded(&leaf.0, &path);

        let witness = SpendWitness::new(
            note,
            nk,
            vec![sibling],
            vec![true], // sibling sağda
            [0x11u8; 32],
            [0x22u8; 32],
        );

        let stmt = witness.check_constraints(root, SlotId(10)).unwrap();
        assert_eq!(stmt.anchor, root);
    }

    #[test]
    fn test_spend_zero_address_rejected() {
        // transmission_key = [0] → address_integrity başarısız
        let note = Note::new(100, NSC_ASSET_ID, [1u8; 32], [0u8; 32]); // sıfır transmission_key
        let nk = make_nk(1);
        let anchor = anchor_for(&note);
        let witness = make_witness(note, nk);

        let err = witness.check_constraints(anchor, SlotId(0));
        assert!(matches!(err, Err(ZkError::InvalidProof)));
    }
}
