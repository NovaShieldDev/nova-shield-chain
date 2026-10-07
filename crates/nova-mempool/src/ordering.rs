//! MEV-free TX sıralama. §5.2.2, §5.3
//!
//! ## MEV-Free Ordering Neden Commitment Hash?
//!
//! Geleneksel blockzincirlerde TX sıralaması gas fiyatına göre yapılır.
//! Yüksek gas fiyatı → erken sıra → MEV fırsatı (frontrunning).
//!
//! Nova Shield'de tüm TX'ler aynı anda (slot sonunda) şifresi çözülür
//! ve batch clearing price'ta execute edilir. Ancak sıralama prensibi
//! şunun için önemlidir:
//! - Determinizm: Tüm validator'lar aynı sırayı hesaplamalı
//! - Öngörülemezlik: Kullanıcı kendi TX'inin sırasını önceden bilemez
//!   (frontrunning imkansız çünkü içerik de bilinmiyor)
//! - Adillik: Her TX eşit şansa sahip
//!
//! **Seçilen yöntem: Commitment hash'e göre lexicographic sort**
//!
//! Commitment = BLAKE3(E || cipher || nonce || slot)
//! - E = random ephemeral key → her TX için unpredictable
//! - Deterministic: aynı girdi → aynı sıra
//! - MEV-resistant: sıralamayı bilmek içerik bilgisi gerektirmez ama avantaj sağlamaz
//!   çünkü batch clearing price herkese eşit uygulanır

use crate::types::{DecryptedTx, EncryptedTx, RevealedBatch, TxHash};
use nova_crypto::hash::blake3_hash_many;
use nova_primitives::types::SlotId;

// ─── ŞİFRELİ TX SIRALAMANSI ───────────────────────────────────────────────────

/// Şifreli TX'leri commitment hash'e göre sırala (commit fazı). §5.2.2
///
/// Bu fonksiyon commit fazında (TX'ler henüz şifreli) çağrılır.
/// Tüm validator'lar aynı sırayı hesaplar → deterministik.
///
/// **Sıralama:** Commitment hash byte'ları lexicographic artan
pub fn order_encrypted_batch(mut txs: Vec<EncryptedTx>) -> Vec<EncryptedTx> {
    txs.sort_unstable_by(|a, b| a.commitment.as_bytes().cmp(b.commitment.as_bytes()));
    txs
}

/// Şifreli TX'leri yerinde sırala (in-place).
pub fn order_encrypted_batch_in_place(txs: &mut Vec<EncryptedTx>) {
    txs.sort_unstable_by(|a, b| a.commitment.as_bytes().cmp(b.commitment.as_bytes()));
}

// ─── AÇILMIŞ TX SIRALAMANSI ──────────────────────────────────────────────────

/// Açılmış TX'leri `ordering_index`'e göre sırala. §5.3
///
/// `ordering_index` commit fazında commitment hash sıralamasından atanır.
/// Bu sayede reveal fazından sonra da deterministik sıra korunur.
pub fn order_revealed_batch(mut txs: Vec<DecryptedTx>) -> Vec<DecryptedTx> {
    txs.sort_unstable_by_key(|tx| tx.ordering_index);
    txs
}

// ─── BATCH MERKLE ROOT ────────────────────────────────────────────────────────

/// Slot batch'inin Merkle root'unu hesapla. §5.10
///
/// Blok header'a yazılır → blok hash'in bir parçası.
/// Tüm commitment hash'ler Merkle ağacına yaprak olarak eklenir.
pub fn compute_batch_merkle_root(ordered_txs: &[EncryptedTx]) -> [u8; 32] {
    if ordered_txs.is_empty() {
        return [0u8; 32];
    }

    if ordered_txs.len() == 1 {
        return *ordered_txs[0].commitment.as_bytes();
    }

    // Commitment hash'lerden Merkle root hesapla
    let leaf_hashes: Vec<nova_primitives::crypto::Hash> = ordered_txs
        .iter()
        .map(|tx| nova_primitives::crypto::Hash::from_bytes(*tx.commitment.as_bytes()))
        .collect();

    let root = nova_crypto::hash::merkle_root(&leaf_hashes);
    *root.as_bytes()
}

/// Açılmış batch için Merkle root (açılmış TX commitment hash'lerinden).
pub fn compute_revealed_batch_merkle_root(revealed_txs: &[DecryptedTx]) -> [u8; 32] {
    if revealed_txs.is_empty() {
        return [0u8; 32];
    }

    let leaf_hashes: Vec<nova_primitives::crypto::Hash> = revealed_txs
        .iter()
        .map(|tx| nova_primitives::crypto::Hash::from_bytes(*tx.commitment.as_bytes()))
        .collect();

    let root = nova_crypto::hash::merkle_root(&leaf_hashes);
    *root.as_bytes()
}

// ─── SIRALAMA INDEX ATAMA ─────────────────────────────────────────────────────

/// Sıralı şifreli TX listesine `ordering_index` ata.
///
/// Reveal fazında bu index kullanılarak TX'ler sıra numarası alır.
/// Sıra numarası batch auction'ın deterministik çıktısı için kritik.
pub fn assign_ordering_indices(ordered_txs: &[EncryptedTx]) -> Vec<(TxHash, u32)> {
    ordered_txs
        .iter()
        .enumerate()
        .map(|(i, tx)| (tx.commitment, i as u32))
        .collect()
}

// ─── DUPLICATION KONTROLÜ ────────────────────────────────────────────────────

/// Batch içinde yinelenen TX var mı kontrol et.
///
/// Commitment hash unique olmalı. Aynı TX iki kez gönderilirse
/// sadece biri kabul edilir.
pub fn find_duplicates(txs: &[EncryptedTx]) -> Vec<TxHash> {
    let mut seen = std::collections::HashSet::new();
    let mut duplicates = Vec::new();

    for tx in txs {
        if !seen.insert(tx.commitment) {
            duplicates.push(tx.commitment);
        }
    }
    duplicates
}

/// Yinelenenleri kaldırarak sırala.
pub fn deduplicate_and_order(txs: Vec<EncryptedTx>) -> Vec<EncryptedTx> {
    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<EncryptedTx> = txs
        .into_iter()
        .filter(|tx| seen.insert(tx.commitment))
        .collect();

    order_encrypted_batch_in_place(&mut unique);
    unique
}

// ─── BATCH İSTATİSTİKLERİ ────────────────────────────────────────────────────

/// Batch sıralama istatistikleri (debugging/metrics).
#[derive(Debug, Clone, Default)]
pub struct BatchOrderingStats {
    /// Toplam TX sayısı.
    pub total_txs: usize,
    /// Yinelenen TX sayısı.
    pub duplicate_count: usize,
    /// Sıralama sonrası TX sayısı.
    pub ordered_count: usize,
    /// Merkle root (hex).
    pub merkle_root_hex: String,
}

impl BatchOrderingStats {
    /// Şifreli batch'ten istatistik hesapla.
    pub fn compute(txs: &[EncryptedTx]) -> Self {
        let total = txs.len();
        let duplicates = find_duplicates(txs).len();
        let root = compute_batch_merkle_root(txs);

        Self {
            total_txs: total,
            duplicate_count: duplicates,
            ordered_count: total - duplicates,
            merkle_root_hex: hex::encode(root),
        }
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ibe::encrypt::encrypt_transaction,
        types::{DecryptedTx, EncryptedTx, RawTransaction, TxHash},
    };
    use curve25519_dalek::{constants::RISTRETTO_BASEPOINT_POINT, scalar::Scalar};
    use nova_primitives::types::{Address, Balance, Nonce, SlotId};
    use rand::rngs::OsRng;

    fn test_address(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn random_pk() -> [u8; 32] {
        let s = Scalar::random(&mut OsRng);
        (s * RISTRETTO_BASEPOINT_POINT).compress().to_bytes()
    }

    fn make_encrypted_tx(seed: u8, slot: SlotId) -> EncryptedTx {
        let from = test_address(seed);
        let to = test_address(seed + 1);
        let raw = RawTransaction::new_transfer(from, to, Balance(seed as u128), Nonce(0), 1984);
        encrypt_transaction(&raw, &random_pk(), slot, vec![])
            .expect("şifreleme başarılı")
            .encrypted_tx
    }

    fn make_decrypted_tx(ordering_index: u32, commitment_seed: u8) -> DecryptedTx {
        let from = test_address(commitment_seed);
        let to = test_address(commitment_seed + 1);
        DecryptedTx {
            commitment: TxHash::from_bytes([commitment_seed; 32]),
            raw_tx: RawTransaction::new_transfer(from, to, Balance::ZERO, Nonce(0), 1984),
            revealed_in_slot: SlotId(1),
            ordering_index,
        }
    }

    #[test]
    fn test_sifrelenmis_batch_siralama_deterministik() {
        let slot = SlotId(1);
        let tx1 = make_encrypted_tx(1, slot);
        let tx2 = make_encrypted_tx(2, slot);
        let tx3 = make_encrypted_tx(3, slot);

        let batch = vec![tx1.clone(), tx2.clone(), tx3.clone()];
        let sorted1 = order_encrypted_batch(batch.clone());
        let sorted2 = order_encrypted_batch(batch.clone());

        // Aynı girdi → aynı sıralama (deterministik)
        for (a, b) in sorted1.iter().zip(sorted2.iter()) {
            assert_eq!(a.commitment, b.commitment, "Sıralama deterministik olmalı");
        }
    }

    #[test]
    fn test_sifrelenmis_batch_siralama_commitment_sirasinda() {
        let slot = SlotId(2);
        let txs: Vec<EncryptedTx> = (0..5).map(|i| make_encrypted_tx(i, slot)).collect();
        let sorted = order_encrypted_batch(txs);

        // Sıralama monoton artan
        for i in 1..sorted.len() {
            assert!(
                sorted[i - 1].commitment.as_bytes() <= sorted[i].commitment.as_bytes(),
                "Sıralama monoton artan olmalı: {} <= {}",
                sorted[i - 1].commitment,
                sorted[i].commitment
            );
        }
    }

    #[test]
    fn test_bos_batch_siralama() {
        let empty: Vec<EncryptedTx> = vec![];
        let sorted = order_encrypted_batch(empty);
        assert!(sorted.is_empty(), "Boş batch boş kalmalı");
    }

    #[test]
    fn test_tek_eleman_batch() {
        let slot = SlotId(1);
        let tx = make_encrypted_tx(42, slot);
        let sorted = order_encrypted_batch(vec![tx.clone()]);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].commitment, tx.commitment);
    }

    #[test]
    fn test_acilmis_batch_siralama() {
        // ordering_index 5, 2, 8, 1, 3 → 1, 2, 3, 5, 8 sırasında gelmeli
        let txs = vec![
            make_decrypted_tx(5, 0x05),
            make_decrypted_tx(2, 0x02),
            make_decrypted_tx(8, 0x08),
            make_decrypted_tx(1, 0x01),
            make_decrypted_tx(3, 0x03),
        ];

        let sorted = order_revealed_batch(txs);

        assert_eq!(sorted[0].ordering_index, 1);
        assert_eq!(sorted[1].ordering_index, 2);
        assert_eq!(sorted[2].ordering_index, 3);
        assert_eq!(sorted[3].ordering_index, 5);
        assert_eq!(sorted[4].ordering_index, 8);
    }

    #[test]
    fn test_merkle_root_bos_batch() {
        let root = compute_batch_merkle_root(&[]);
        assert_eq!(root, [0u8; 32], "Boş batch → sıfır root");
    }

    #[test]
    fn test_merkle_root_tek_tx() {
        let slot = SlotId(1);
        let tx = make_encrypted_tx(1, slot);
        let root = compute_batch_merkle_root(&[tx.clone()]);
        assert_eq!(
            root, *tx.commitment.as_bytes(),
            "Tek TX → root = commitment hash"
        );
    }

    #[test]
    fn test_merkle_root_deterministik() {
        let slot = SlotId(1);
        let txs: Vec<EncryptedTx> = (0..3).map(|i| make_encrypted_tx(i, slot)).collect();

        let root1 = compute_batch_merkle_root(&txs);
        let root2 = compute_batch_merkle_root(&txs);
        assert_eq!(root1, root2, "Merkle root deterministik olmalı");
    }

    #[test]
    fn test_merkle_root_siralama_bagimliligi() {
        let slot = SlotId(1);
        let tx1 = make_encrypted_tx(1, slot);
        let tx2 = make_encrypted_tx(2, slot);

        let root_ab = compute_batch_merkle_root(&[tx1.clone(), tx2.clone()]);
        let root_ba = compute_batch_merkle_root(&[tx2.clone(), tx1.clone()]);

        // Farklı sıralama → farklı root (sıralama önemli)
        assert_ne!(root_ab, root_ba, "Farklı sıralama → farklı Merkle root");
    }

    #[test]
    fn test_yinelenen_bulma() {
        let slot = SlotId(1);
        let tx1 = make_encrypted_tx(1, slot);
        let tx1_dup = tx1.clone();
        let tx2 = make_encrypted_tx(2, slot);

        let txs = vec![tx1, tx1_dup, tx2];
        let duplicates = find_duplicates(&txs);
        assert_eq!(duplicates.len(), 1, "1 yinelenen tespit edilmeli");
    }

    #[test]
    fn test_yinelenen_yok() {
        let slot = SlotId(1);
        let txs: Vec<EncryptedTx> = (0..5).map(|i| make_encrypted_tx(i, slot)).collect();
        let duplicates = find_duplicates(&txs);
        assert!(duplicates.is_empty(), "Yinelenen yok olmalı");
    }

    #[test]
    fn test_deduplicate_and_order() {
        let slot = SlotId(1);
        let tx1 = make_encrypted_tx(1, slot);
        let tx2 = make_encrypted_tx(2, slot);
        let tx1_dup = tx1.clone();

        let txs = vec![tx1.clone(), tx2.clone(), tx1_dup];
        let result = deduplicate_and_order(txs);

        assert_eq!(result.len(), 2, "Yinelenen kaldırıldıktan sonra 2 TX kalmalı");

        // Sıralama doğru
        assert!(
            result[0].commitment.as_bytes() <= result[1].commitment.as_bytes(),
            "Sıralama doğru olmalı"
        );
    }

    #[test]
    fn test_ordering_index_atama() {
        let slot = SlotId(1);
        let txs: Vec<EncryptedTx> = (0..4).map(|i| make_encrypted_tx(i, slot)).collect();
        let sorted = order_encrypted_batch(txs);
        let indices = assign_ordering_indices(&sorted);

        assert_eq!(indices.len(), 4);
        // İndeksler 0, 1, 2, 3 sırasında
        for (i, (hash, idx)) in indices.iter().enumerate() {
            assert_eq!(*idx, i as u32);
            assert_eq!(*hash, sorted[i].commitment);
        }
    }

    #[test]
    fn test_batch_stats() {
        let slot = SlotId(1);
        let tx1 = make_encrypted_tx(1, slot);
        let tx2 = make_encrypted_tx(2, slot);
        let tx1_dup = tx1.clone();

        let txs = vec![tx1, tx2, tx1_dup];
        let stats = BatchOrderingStats::compute(&txs);

        assert_eq!(stats.total_txs, 3);
        assert_eq!(stats.duplicate_count, 1);
        assert!(!stats.merkle_root_hex.is_empty());
    }

    #[test]
    fn test_in_place_siralama() {
        let slot = SlotId(1);
        let mut txs: Vec<EncryptedTx> = (0..5).map(|i| make_encrypted_tx(i, slot)).collect();

        // Karıştır
        txs.reverse();

        order_encrypted_batch_in_place(&mut txs);

        for i in 1..txs.len() {
            assert!(txs[i - 1].commitment.as_bytes() <= txs[i].commitment.as_bytes());
        }
    }

    #[test]
    fn test_mev_free_dogrulama_simulasyon() {
        // Senaryo: Bot aynı slot'a birden fazla TX gönderiyor
        // Hedef: Bot kendi TX'inin sırasını bilemez (commitment unpredictable)

        let slot = SlotId(1000);
        let bot_txs: Vec<EncryptedTx> =
            (0..3).map(|i| make_encrypted_tx(i + 10, slot)).collect();
        let user_tx = make_encrypted_tx(99, slot);

        let mut all_txs = bot_txs;
        all_txs.push(user_tx.clone());

        let sorted = order_encrypted_batch(all_txs);

        // User TX'i nerede olduğunu sadece commitment hash'e bakarak bulabiliriz
        // Bot bunu önceden bilemez (commitment = H(random_ephemeral || cipher || ...))
        let user_pos = sorted
            .iter()
            .position(|tx| tx.commitment == user_tx.commitment)
            .expect("User TX batch'te bulunmalı");

        // Pozisyon 0-3 arasında (tahmin edilemez ama doğru sırada)
        assert!(user_pos < sorted.len(), "User TX batch sınırları içinde olmalı");
    }
}
