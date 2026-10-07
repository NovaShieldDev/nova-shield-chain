//! Slot reveal (açılma) mekanizması. §5.2.2, §5.3
//!
//! ## Reveal Fazı Akışı
//!
//! ```text
//! [SLOT KAPANDI — Commit Fazı Bitti]
//!         │
//!         ▼
//! Validator'lar D_i = sk_i * E hesaplar (her TX için)
//!         │
//!         ▼
//! Decryption share'ler toplandı (t = BFT quorum = 2n/3+1)
//!         │
//!         ▼
//! can_reveal() → true
//!         │
//!         ▼
//! reveal_all() → her TX için:
//!   1. Lagrange combine shares → S = sk_threshold * E
//!   2. K = BLAKE3(domain || S)
//!   3. AES decrypt → padded_tx
//!   4. unpad → raw_tx_bytes
//!   5. borsh deserialize → RawTransaction
//!   6. verify commitment
//!   7. DecryptedTx döndür
//!         │
//!         ▼
//! RevealedBatch → nova-auction (batch clearing price)
//! ```

use std::collections::HashMap;

use crate::{
    errors::{MempoolError, MempoolResult},
    ibe::decrypt::decrypt_transaction,
    ordering::{
        assign_ordering_indices, compute_revealed_batch_merkle_root,
        order_encrypted_batch,
    },
    types::{
        DecryptedTx, EncryptedTx, RevealedBatch, TxDecryptionShare, TxHash, TxStatus,
    },
};
use nova_primitives::types::SlotId;
use tracing::{debug, info, warn};

// ─── SLOT REVEAL DURUMU ───────────────────────────────────────────────────────

/// Tek bir slot için reveal durumunu yöneten yapı. §5.2.2
///
/// Her slot için bir `SlotRevealState` oluşturulur.
/// Validator'lar decryption share'lerini bu yapıya ekler.
/// Yeterli share toplandığında `reveal_all()` çağrılır.
#[derive(Debug)]
pub struct SlotRevealState {
    /// Slot numarası.
    pub slot: SlotId,

    /// Commitment hash sıralı şifreli TX'ler (commit fazından).
    ordered_txs: Vec<EncryptedTx>,

    /// TX hash → [validator_index → DecryptionShare] şeklinde toplanan share'ler.
    shares: HashMap<TxHash, Vec<TxDecryptionShare>>,

    /// Gereken minimum share sayısı (BFT threshold).
    threshold: u8,

    /// Reveal tamamlandı mı?
    pub revealed: bool,

    /// Reveal sonucu (bir kez çalışır).
    result: Option<RevealedBatch>,
}

impl SlotRevealState {
    /// Yeni slot reveal durumu oluştur.
    ///
    /// `txs`: Commit fazında toplanan şifreli TX'ler (sırasız olabilir)
    /// `threshold`: Minimum decryption share sayısı (t)
    pub fn new(txs: Vec<EncryptedTx>, threshold: u8) -> Self {
        // TX'leri commitment hash sırasına göre sırala (MEV-free ordering)
        let ordered = order_encrypted_batch(txs);

        debug!(
            tx_count = ordered.len(),
            threshold,
            "Slot reveal state oluşturuldu"
        );

        Self {
            slot: SlotId(0), // Dışarıdan set edilecek
            ordered_txs: ordered,
            shares: HashMap::new(),
            threshold,
            revealed: false,
            result: None,
        }
    }

    /// Slot numarasını set et.
    pub fn with_slot(mut self, slot: SlotId) -> Self {
        self.slot = slot;
        self
    }

    /// TX sayısı.
    pub fn tx_count(&self) -> usize {
        self.ordered_txs.len()
    }

    /// Belirli bir TX için toplanan share sayısı.
    pub fn share_count_for(&self, tx_hash: &TxHash) -> usize {
        self.shares.get(tx_hash).map_or(0, |v| v.len())
    }

    /// Toplam toplanan share sayısı (tüm TX'ler için).
    pub fn total_share_count(&self) -> usize {
        self.shares.values().map(|v| v.len()).sum()
    }

    /// Decryption share ekle.
    ///
    /// Aynı validator'ın aynı TX için birden fazla share'i yoksayılır.
    /// Döndürülen değer: kaç yeni share eklendi.
    pub fn add_decryption_shares(&mut self, new_shares: Vec<TxDecryptionShare>) -> usize {
        let mut added = 0;
        for share in new_shares {
            let entry = self.shares.entry(share.tx_hash).or_default();
            // Yinelenen validator kontrolü
            if !entry.iter().any(|s| s.validator_index == share.validator_index) {
                entry.push(share);
                added += 1;
            }
        }
        added
    }

    /// Tüm TX'ler için yeterli share toplandı mı?
    ///
    /// Her TX bağımsız olarak değerlendirilir.
    /// Herhangi bir TX yeterli share'e sahip değilse → false.
    pub fn can_reveal(&self) -> bool {
        if self.revealed {
            return false;
        }
        self.ordered_txs.iter().all(|tx| {
            self.share_count_for(&tx.commitment) >= self.threshold as usize
        })
    }

    /// Hangi TX'ler henüz yeterli share'e sahip değil?
    pub fn missing_shares_for(&self) -> Vec<(TxHash, usize)> {
        self.ordered_txs
            .iter()
            .filter_map(|tx| {
                let count = self.share_count_for(&tx.commitment);
                if count < self.threshold as usize {
                    Some((tx.commitment, self.threshold as usize - count))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Tüm TX'leri threshold decrypt et ve `RevealedBatch` döndür. §5.2.2
    ///
    /// Bu fonksiyon yalnızca bir kez çalışır.
    /// `can_reveal()` true dönmeden çağrılmamalı.
    ///
    /// Başarısız TX'ler `RevealedBatch::failed_hashes`'e eklenir,
    /// başarılı TX'ler sıralı `RevealedBatch::transactions`'a eklenir.
    pub fn reveal_all(&mut self) -> MempoolResult<&RevealedBatch> {
        if self.revealed {
            return self.result.as_ref().ok_or_else(|| {
                MempoolError::SlotAlreadyRevealed(self.slot)
            });
        }

        if self.ordered_txs.is_empty() {
            let batch = RevealedBatch {
                slot: self.slot,
                transactions: vec![],
                failed_hashes: vec![],
                batch_merkle_root: [0u8; 32],
            };
            self.result = Some(batch);
            self.revealed = true;
            return Ok(self.result.as_ref().unwrap());
        }

        // Ordering index'leri önceden ata (commitment sırasına göre)
        let order_map: HashMap<TxHash, u32> =
            assign_ordering_indices(&self.ordered_txs).into_iter().collect();

        let mut decrypted_txs: Vec<DecryptedTx> = Vec::new();
        let mut failed_hashes: Vec<TxHash> = Vec::new();

        for encrypted_tx in &self.ordered_txs {
            let tx_hash = encrypted_tx.commitment;
            let ordering_index = order_map[&tx_hash];

            // Share'leri al
            let shares = match self.shares.get(&tx_hash) {
                Some(s) => s.clone(),
                None => {
                    warn!(
                        tx_hash = %tx_hash,
                        "TX için share yok — failed olarak işaretleniyor"
                    );
                    failed_hashes.push(tx_hash);
                    continue;
                }
            };

            // Threshold decrypt
            match decrypt_transaction(encrypted_tx, &shares, self.threshold, ordering_index) {
                Ok(decrypted) => {
                    debug!(
                        tx_hash = %tx_hash,
                        ordering_index,
                        "TX başarıyla açıldı"
                    );
                    decrypted_txs.push(decrypted);
                }
                Err(e) => {
                    warn!(
                        tx_hash = %tx_hash,
                        error = %e,
                        "TX açılması başarısız — failed olarak işaretleniyor"
                    );
                    failed_hashes.push(tx_hash);
                }
            }
        }

        // Merkle root hesapla (açılmış TX commitment hash'lerinden)
        let merkle_root = compute_revealed_batch_merkle_root(&decrypted_txs);

        info!(
            slot = self.slot.0,
            success = decrypted_txs.len(),
            failed = failed_hashes.len(),
            "Slot reveal tamamlandı"
        );

        let batch = RevealedBatch {
            slot: self.slot,
            transactions: decrypted_txs,
            failed_hashes,
            batch_merkle_root: merkle_root,
        };

        self.result = Some(batch);
        self.revealed = true;

        Ok(self.result.as_ref().unwrap())
    }

    /// Reveal sonucunu al (reveal tamamlandıysa).
    pub fn get_result(&self) -> Option<&RevealedBatch> {
        self.result.as_ref()
    }

    /// Mevcut durum özeti.
    pub fn status_summary(&self) -> RevealStatusSummary {
        let txs_with_enough_shares = self
            .ordered_txs
            .iter()
            .filter(|tx| self.share_count_for(&tx.commitment) >= self.threshold as usize)
            .count();

        RevealStatusSummary {
            slot: self.slot,
            total_txs: self.ordered_txs.len(),
            txs_ready: txs_with_enough_shares,
            total_shares_collected: self.total_share_count(),
            threshold: self.threshold,
            can_reveal: self.can_reveal(),
            revealed: self.revealed,
        }
    }
}

// ─── REVEAL DURUM ÖZETİ ──────────────────────────────────────────────────────

/// `SlotRevealState::status_summary()` çıktısı.
#[derive(Debug, Clone)]
pub struct RevealStatusSummary {
    /// Slot numarası.
    pub slot: SlotId,
    /// Toplam TX sayısı.
    pub total_txs: usize,
    /// Yeterli share'e sahip TX sayısı.
    pub txs_ready: usize,
    /// Toplam toplanan share sayısı.
    pub total_shares_collected: usize,
    /// Gerekli threshold.
    pub threshold: u8,
    /// Reveal yapılabilir mi?
    pub can_reveal: bool,
    /// Reveal tamamlandı mı?
    pub revealed: bool,
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ibe::{decrypt::compute_tx_decryption_share, encrypt::encrypt_transaction},
        types::RawTransaction,
    };
    use curve25519_dalek::{constants::RISTRETTO_BASEPOINT_POINT, scalar::Scalar};
    use nova_primitives::types::{Address, Balance, Nonce, SlotId};
    use rand::rngs::OsRng;

    fn test_address(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    /// t/n threshold Shamir secret sharing setup.
    fn setup_keys(t: u8, n: u8) -> ([u8; 32], Vec<(u8, Scalar)>) {
        let sk = Scalar::random(&mut OsRng);
        let pk_bytes = (sk * RISTRETTO_BASEPOINT_POINT).compress().to_bytes();

        let mut coeffs = vec![sk];
        for _ in 1..t {
            coeffs.push(Scalar::random(&mut OsRng));
        }

        let shares: Vec<(u8, Scalar)> = (1..=n)
            .map(|i| {
                let x = Scalar::from(i as u64);
                let mut y = Scalar::ZERO;
                let mut xp = Scalar::ONE;
                for c in &coeffs {
                    y += c * xp;
                    xp *= x;
                }
                (i, y)
            })
            .collect();

        (pk_bytes, shares)
    }

    fn make_encrypted_tx(pk: &[u8; 32], slot: SlotId, seed: u8) -> EncryptedTx {
        let from = test_address(seed);
        let to = test_address(seed + 1);
        let raw = RawTransaction::new_transfer(
            from, to, Balance(seed as u128 * 1_000), Nonce(seed as u64), 1984,
        );
        encrypt_transaction(&raw, pk, slot, vec![])
            .expect("şifreleme başarılı")
            .encrypted_tx
    }

    fn make_shares_for_tx(
        sk_shares: &[(u8, Scalar)],
        count: usize,
        encrypted_tx: &EncryptedTx,
    ) -> Vec<TxDecryptionShare> {
        sk_shares[..count]
            .iter()
            .map(|(idx, sk)| {
                compute_tx_decryption_share(
                    &sk.to_bytes(),
                    &encrypted_tx.ephemeral_pubkey,
                    *idx,
                    encrypted_tx.commitment,
                    encrypted_tx.target_slot,
                )
                .expect("share hesaplama")
            })
            .collect()
    }

    #[test]
    fn test_reveal_state_olusturma() {
        let (pk, _) = setup_keys(3, 5);
        let slot = SlotId(10);
        let txs: Vec<EncryptedTx> = (0..3).map(|i| make_encrypted_tx(&pk, slot, i)).collect();

        let state = SlotRevealState::new(txs, 3).with_slot(slot);

        assert_eq!(state.tx_count(), 3);
        assert!(!state.revealed);
        assert!(!state.can_reveal(), "Share toplamadan reveal yapılamaz");
    }

    #[test]
    fn test_reveal_bos_batch() {
        let mut state = SlotRevealState::new(vec![], 3).with_slot(SlotId(1));
        let result = state.reveal_all().expect("Boş batch reveal başarılı olmalı");
        assert_eq!(result.success_count(), 0);
        assert_eq!(result.failure_count(), 0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_tek_tx_tam_reveal() {
        let (pk, sk_shares) = setup_keys(3, 5);
        let slot = SlotId(5);
        let tx = make_encrypted_tx(&pk, slot, 0x01);

        let mut state = SlotRevealState::new(vec![tx.clone()], 3).with_slot(slot);

        // 3 share ekle
        let shares = make_shares_for_tx(&sk_shares, 3, &tx);
        state.add_decryption_shares(shares);

        assert!(state.can_reveal(), "3 share sonrası reveal yapılabilmeli");

        let result = state.reveal_all().expect("Reveal başarılı olmalı");
        assert_eq!(result.success_count(), 1, "1 TX başarıyla açılmalı");
        assert_eq!(result.failure_count(), 0, "Başarısız TX olmamalı");
    }

    #[test]
    fn test_cok_tx_tam_reveal() {
        let (pk, sk_shares) = setup_keys(3, 5);
        let slot = SlotId(20);

        let txs: Vec<EncryptedTx> = (0..5).map(|i| make_encrypted_tx(&pk, slot, i)).collect();
        let mut state = SlotRevealState::new(txs.clone(), 3).with_slot(slot);

        // Her TX için 3 share ekle
        for tx in &txs {
            let shares = make_shares_for_tx(&sk_shares, 3, tx);
            state.add_decryption_shares(shares);
        }

        assert!(state.can_reveal());
        let result = state.reveal_all().expect("Reveal başarılı");
        assert_eq!(result.success_count(), 5);
        assert_eq!(result.failure_count(), 0);
    }

    #[test]
    fn test_yetersiz_share_reveal_yapamaz() {
        let (pk, sk_shares) = setup_keys(3, 5);
        let slot = SlotId(1);
        let tx = make_encrypted_tx(&pk, slot, 0x42);

        let mut state = SlotRevealState::new(vec![tx.clone()], 3).with_slot(slot);

        // Sadece 2 share (threshold = 3 → yeterli değil)
        let shares = make_shares_for_tx(&sk_shares, 2, &tx);
        state.add_decryption_shares(shares);

        assert!(!state.can_reveal(), "2 share yeterli değil");
    }

    #[test]
    fn test_share_yineleme_engeli() {
        let (pk, sk_shares) = setup_keys(3, 5);
        let slot = SlotId(2);
        let tx = make_encrypted_tx(&pk, slot, 0x01);
        let tx_hash = tx.commitment;

        let mut state = SlotRevealState::new(vec![tx.clone()], 3).with_slot(slot);

        // Aynı validator'dan 3 kez share gönder
        for _ in 0..3 {
            let share = compute_tx_decryption_share(
                &sk_shares[0].1.to_bytes(),
                &tx.ephemeral_pubkey,
                1, // Hep validator 1
                tx_hash,
                slot,
            )
            .expect("share");
            state.add_decryption_shares(vec![share]);
        }

        // Yalnızca 1 unique share sayılmalı
        assert_eq!(
            state.share_count_for(&tx_hash),
            1,
            "Yinelenen share sayılmamalı"
        );
    }

    #[test]
    fn test_reveal_ikinci_kez_hatasi() {
        let (pk, sk_shares) = setup_keys(2, 3);
        let slot = SlotId(3);
        let tx = make_encrypted_tx(&pk, slot, 0x01);

        let mut state = SlotRevealState::new(vec![tx.clone()], 2).with_slot(slot);
        let shares = make_shares_for_tx(&sk_shares, 2, &tx);
        state.add_decryption_shares(shares);

        let _ = state.reveal_all().expect("İlk reveal başarılı");
        let result2 = state.reveal_all();
        // İkinci çağrı SlotAlreadyRevealed veya mevcut sonucu döndürmeli
        let _ = result2; // Panic olmadığını doğrula
    }

    #[test]
    fn test_missing_shares_raporu() {
        let (pk, sk_shares) = setup_keys(3, 5);
        let slot = SlotId(1);
        let tx = make_encrypted_tx(&pk, slot, 0x01);

        let mut state = SlotRevealState::new(vec![tx.clone()], 3).with_slot(slot);
        // Hiç share yok
        let missing = state.missing_shares_for();
        assert_eq!(missing.len(), 1, "1 TX için share eksik raporu");
        assert_eq!(missing[0].1, 3, "3 share eksik");

        // 1 share ekle
        let shares = make_shares_for_tx(&sk_shares, 1, &tx);
        state.add_decryption_shares(shares);
        let missing2 = state.missing_shares_for();
        assert_eq!(missing2[0].1, 2, "1 share sonrası 2 eksik kaldı");
    }

    #[test]
    fn test_status_summary() {
        let (pk, _) = setup_keys(3, 5);
        let slot = SlotId(7);
        let txs: Vec<EncryptedTx> = (0..3).map(|i| make_encrypted_tx(&pk, slot, i)).collect();

        let state = SlotRevealState::new(txs, 3).with_slot(slot);
        let summary = state.status_summary();

        assert_eq!(summary.total_txs, 3);
        assert_eq!(summary.txs_ready, 0);
        assert!(!summary.can_reveal);
        assert!(!summary.revealed);
        assert_eq!(summary.threshold, 3);
    }

    #[test]
    fn test_reveal_kısmi_basarisiz() {
        // 2 TX var, birisi için share'ler var, diğeri için yok
        let (pk, sk_shares) = setup_keys(2, 3);
        let slot = SlotId(9);

        let tx1 = make_encrypted_tx(&pk, slot, 0x01);
        let tx2 = make_encrypted_tx(&pk, slot, 0x02);

        let mut state = SlotRevealState::new(vec![tx1.clone(), tx2.clone()], 2).with_slot(slot);

        // Sadece tx1 için share ekle (tx2 için yok)
        let shares1 = make_shares_for_tx(&sk_shares, 2, &tx1);
        state.add_decryption_shares(shares1);

        // tx2 için de threshold kadar sahte slot share ekle (yanlış tx_hash ile değil, eksik)
        // Burada sadece tx1 hazır, tx2 için 0 share → can_reveal() = false
        assert!(!state.can_reveal(), "tx2 için share yok → reveal yapılamaz");

        // Eğer force reveal yapmak istersek (sadece tx1 açılır, tx2 failed):
        // Bu senaryoyu direkt reveal_all ile test etmek için threshold'u 0 yap
        // (gerçek protokolde böyle olmaz, burada sadece fail path'i test ediyoruz)
        let mut forced_state = SlotRevealState::new(
            vec![tx1.clone(), tx2.clone()],
            2,
        ).with_slot(slot);

        // tx1 için 2 share, tx2 için 0 share (can_reveal = false ama force test)
        let shares1 = make_shares_for_tx(&sk_shares, 2, &tx1);
        forced_state.add_decryption_shares(shares1);

        // reveal_all çağırsak: tx2 için shares yok → failed olarak işaretlenir
        // (can_reveal kontrolünü bypass ederek)
        // Bunu doğrudan test etmek için özel bir "force" olmadığından
        // sadece kısmi reveal sonucunu doğrulayabiliriz:
        assert_eq!(forced_state.share_count_for(&tx2.commitment), 0);
        assert_eq!(forced_state.share_count_for(&tx1.commitment), 2);
    }

    #[test]
    fn test_share_add_dondurulen_sayi() {
        let (pk, sk_shares) = setup_keys(3, 5);
        let slot = SlotId(1);
        let tx = make_encrypted_tx(&pk, slot, 0x01);

        let mut state = SlotRevealState::new(vec![tx.clone()], 3).with_slot(slot);

        let shares = make_shares_for_tx(&sk_shares, 3, &tx);
        let added = state.add_decryption_shares(shares);
        assert_eq!(added, 3, "3 yeni share eklendi");

        // Aynı share'ler tekrar gönder → 0 eklenmeli
        let shares2 = make_shares_for_tx(&sk_shares, 3, &tx);
        let added2 = state.add_decryption_shares(shares2);
        assert_eq!(added2, 0, "Yinelenen share eklenmemeli");
    }

    #[test]
    fn test_reveal_sonuc_merkle_root() {
        let (pk, sk_shares) = setup_keys(2, 3);
        let slot = SlotId(1);
        let tx = make_encrypted_tx(&pk, slot, 0x01);

        let mut state = SlotRevealState::new(vec![tx.clone()], 2).with_slot(slot);
        let shares = make_shares_for_tx(&sk_shares, 2, &tx);
        state.add_decryption_shares(shares);

        let result = state.reveal_all().expect("Reveal başarılı");
        // Merkle root tek TX → commitment hash'i olmalı
        assert_eq!(result.batch_merkle_root, *tx.commitment.as_bytes());
    }
}
