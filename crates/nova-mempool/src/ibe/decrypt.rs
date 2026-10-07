//! TX şifre çözme — Threshold ECDH Decryption. §5.2.2, §6.1.4
//!
//! Validator tarafında çalışır:
//! 1. Her validator kendi sk_share ile `D_i = sk_i * E` hesaplar
//! 2. t adet share Lagrange interpolasyonuyla birleştirilir → S = sk_threshold * E
//! 3. S'den AES anahtarı türetilir → TX açılır
//! 4. Commitment doğrulanır (bütünlük kontrol)

use crate::{
    errors::{MempoolError, MempoolResult},
    ibe::{encrypt::compute_commitment, ECDH_KEY_DOMAIN},
    types::{DecryptedTx, EncryptedTx, RawTransaction, TxDecryptionShare, TxHash},
};
use curve25519_dalek::{
    ristretto::{CompressedRistretto, RistrettoPoint},
    scalar::Scalar,
};
use nova_crypto::symmetric::{aes_gcm_decrypt, EncryptedData};
use nova_primitives::types::SlotId;
use zeroize::Zeroize;

// ─── VALIDATOR TARAFINDA: DECRYPTION SHARE HESAPLAMA ─────────────────────────

/// Bir validator'ın tek bir TX için decryption share hesaplar. §6.1.4
///
/// D_i = sk_share_i * E
///
/// ## Parametreler
/// - `sk_share_bytes`: Validator'ın secret key share'i (Scalar, 32 byte)
/// - `ephemeral_pubkey`: TX'teki ephemeral pubkey E (Ristretto255, 32 byte)
/// - `validator_index`: 1-indexed validator sırası
/// - `tx_hash`: Hangi TX için (audit trail)
/// - `slot`: Hangi slot için
///
/// ## Güvenlik
/// - `sk_share_bytes` çağrı sonrası sıfırlanmalıdır.
/// - Bu fonksiyon constant-time değil; timing leak olasılığı düşük
///   çünkü Ristretto scalar çarpımı sabit zamanlı implementasyon kullanır.
pub fn compute_tx_decryption_share(
    sk_share_bytes: &[u8; 32],
    ephemeral_pubkey: &[u8; 32],
    validator_index: u8,
    tx_hash: TxHash,
    slot: SlotId,
) -> MempoolResult<TxDecryptionShare> {
    if validator_index == 0 {
        return Err(MempoolError::InvalidShareIndex {
            index: 0,
            n: u8::MAX,
        });
    }

    // E noktasını deserialize et
    let e_point = CompressedRistretto::from_slice(ephemeral_pubkey)
        .map_err(|_| MempoolError::InvalidCurvePoint("E decompress hatası".into()))?
        .decompress()
        .ok_or_else(|| MempoolError::InvalidCurvePoint("E geçersiz Ristretto noktası".into()))?;

    // sk_share scalar
    let sk_share = Scalar::from_bytes_mod_order(*sk_share_bytes);

    // D_i = sk_share_i * E
    let d_i_point = sk_share * e_point;
    let d_i_bytes = d_i_point.compress().to_bytes();

    Ok(TxDecryptionShare {
        tx_hash,
        validator_index,
        d_i: d_i_bytes,
        slot,
    })
}

// ─── THRESHOLD DECRYPTION: LAGRANGE İNTERPOLASYONU ──────────────────────────

/// t adet decryption share'i Lagrange interpolasyonuyla birleştir. §6.1.4
///
/// S = Σ_{i∈S} λ_i * D_i  (S bir t-elemanlı alt küme)
///
/// Burada λ_i (Lagrange katsayısı):
/// λ_i = Π_{j∈S, j≠i} x_j / (x_j - x_i)
///
/// Bu S = sk_threshold * E olur (ECDH shared secret).
pub fn lagrange_combine_shares(shares: &[&TxDecryptionShare]) -> MempoolResult<RistrettoPoint> {
    if shares.is_empty() {
        return Err(MempoolError::InsufficientDecryptionShares {
            required: 1,
            available: 0,
        });
    }

    // Her D_i noktasını deserialize et
    let d_points: Vec<RistrettoPoint> = shares
        .iter()
        .map(|s| {
            CompressedRistretto::from_slice(&s.d_i)
                .map_err(|_| {
                    MempoolError::InvalidDecryptionShare {
                        index: s.validator_index,
                        reason: "D_i decompress hatası".into(),
                    }
                })?
                .decompress()
                .ok_or_else(|| MempoolError::InvalidDecryptionShare {
                    index: s.validator_index,
                    reason: "D_i geçersiz Ristretto noktası".into(),
                })
        })
        .collect::<MempoolResult<_>>()?;

    // Lagrange katsayıları: indeksler 1-based (validator_index)
    let indices: Vec<Scalar> = shares
        .iter()
        .map(|s| Scalar::from(s.validator_index as u64))
        .collect();

    // S = Σ λ_i * D_i
    let mut s_point = curve25519_dalek::traits::Identity::identity();

    for (i, (d_i, &x_i)) in d_points.iter().zip(indices.iter()).enumerate() {
        // λ_i = Π_{j≠i} x_j / (x_j - x_i)
        let mut lambda = Scalar::ONE;
        for (j, &x_j) in indices.iter().enumerate() {
            if i != j {
                let diff = x_j - x_i;
                // diff.invert() — Ristretto scalar alanı üzerinde mod inverse
                lambda *= x_j * diff.invert();
            }
        }
        s_point += lambda * d_i;
    }

    Ok(s_point)
}

// ─── ANA THRESHOLD DECRYPT FONKSİYONU ────────────────────────────────────────

/// Threshold decryption ile şifreli TX'i aç. §5.2.2, §6.1.4
///
/// ## Parametreler
/// - `encrypted_tx`: Şifreli TX (mempool'dan alınır)
/// - `shares`: Toplandaki decryption share'ler (en az `threshold` adet)
/// - `threshold`: Gerekli minimum share sayısı
/// - `ordering_index`: Batch içindeki sıra (commitment sort'tan)
///
/// ## Akış
/// 1. Share sayısı kontrol et (≥ threshold)
/// 2. Share'lerin doğru TX'e ait olduğunu kontrol et
/// 3. Lagrange interpolasyon → S = sk_threshold * E
/// 4. K = BLAKE3(ECDH_KEY_DOMAIN || S.compress())
/// 5. AES-GCM decrypt → padded TX bytes
/// 6. Unpad → raw TX bytes
/// 7. Borsh deserialize → RawTransaction
/// 8. Commitment doğrula
/// 9. DecryptedTx döndür
pub fn decrypt_transaction(
    encrypted_tx: &EncryptedTx,
    shares: &[TxDecryptionShare],
    threshold: u8,
    ordering_index: u32,
) -> MempoolResult<DecryptedTx> {
    // ── 1. Share sayısı yeterli mi? ───────────────────────────────────────────
    if shares.len() < threshold as usize {
        return Err(MempoolError::InsufficientDecryptionShares {
            required: threshold,
            available: shares.len(),
        });
    }

    // ── 2. Share'lerin bu TX'e ait olduğunu doğrula ───────────────────────────
    for share in shares.iter() {
        if share.tx_hash != encrypted_tx.commitment {
            return Err(MempoolError::InvalidDecryptionShare {
                index: share.validator_index,
                reason: format!(
                    "yanlış tx_hash: beklenen {} gelen {}",
                    encrypted_tx.commitment,
                    share.tx_hash
                ),
            });
        }
    }

    // ── 3. Sadece ilk `threshold` share kullan ────────────────────────────────
    // Validator index'e göre sırala (deterministik)
    let mut sorted_shares: Vec<&TxDecryptionShare> = shares
        .iter()
        .take(threshold as usize)
        .collect();
    sorted_shares.sort_by_key(|s| s.validator_index);

    // ── 4. Lagrange interpolasyon → S ────────────────────────────────────────
    let s_point = lagrange_combine_shares(&sorted_shares)
        .map_err(|e| MempoolError::DecryptionFailed(e.to_string()))?;

    // ── 5. AES anahtarı türet ─────────────────────────────────────────────────
    // K = BLAKE3("nova-mempool-aes-key-v1" || S.compress())
    let s_bytes = s_point.compress().to_bytes();
    let mut aes_key = blake3::derive_key(ECDH_KEY_DOMAIN, &s_bytes);

    // ── 6. AES-256-GCM decrypt ───────────────────────────────────────────────
    let slot_aad = encrypted_tx.target_slot.0.to_le_bytes();
    let enc_data = EncryptedData {
        nonce: encrypted_tx.aes_nonce,
        ciphertext: encrypted_tx.aes_ciphertext.clone(),
    };
    let padded_plaintext = aes_gcm_decrypt(&aes_key, &enc_data, &slot_aad)
        .map_err(|e| MempoolError::AesDecryptionError(e.to_string()))?;

    // AES anahtarını sıfırla
    aes_key.zeroize();

    // ── 7. Length-prefix padding kaldır ─────────────────────────────────────
    // Format: [4 byte LE uzunluk][veri][sıfır dolgu → 512 byte toplam]
    let raw_bytes = crate::ibe::encrypt::length_prefix_unpad(&padded_plaintext)
        .map_err(|e| MempoolError::UnpadError(e))?;

    // ── 8. TX deserialize ─────────────────────────────────────────────────────
    let raw_tx = RawTransaction::from_bytes(raw_bytes)
        .map_err(|e| MempoolError::DeserializationError(e.to_string()))?;

    // ── 9. Commitment doğrula ─────────────────────────────────────────────────
    // Açılan TX'den yeniden hesaplanan commitment, saklanan ile eşleşmeli
    let recomputed = compute_commitment(
        &encrypted_tx.ephemeral_pubkey,
        &encrypted_tx.aes_ciphertext,
        &encrypted_tx.aes_nonce,
        encrypted_tx.target_slot,
    );

    if recomputed != encrypted_tx.commitment {
        return Err(MempoolError::CommitmentMismatch {
            expected: encrypted_tx.commitment.to_hex(),
            computed: recomputed.to_hex(),
        });
    }

    Ok(DecryptedTx {
        commitment: encrypted_tx.commitment,
        raw_tx,
        revealed_in_slot: encrypted_tx.target_slot,
        ordering_index,
    })
}

// ─── NOT ─────────────────────────────────────────────────────────────────────
// Padding/unpadding işlemleri `crate::ibe::encrypt::length_prefix_pad` ve
// `crate::ibe::encrypt::length_prefix_unpad` fonksiyonlarına taşındı.
// Eski PKCS#7-benzeri `unpad_pkcs7` kaldırıldı — 512 byte target için çalışmıyordu
// çünkü padding_len >= 256 durumunda pad_byte = padding_len % 256 olduğundan
// unpadding hatalı sonuç üretiyordu.

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ibe::encrypt::encrypt_transaction,
        types::{RawTransaction, TxDecryptionShare},
    };
    use curve25519_dalek::{constants::RISTRETTO_BASEPOINT_POINT, scalar::Scalar};
    use nova_primitives::types::{Address, Balance, Nonce, SlotId};
    use rand::rngs::OsRng;

    fn test_address(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    /// Test için basit t/n threshold key setup.
    /// Gerçekte DKG ile üretilir; burada Shamir secret sharing simülasyonu.
    fn setup_threshold_keys(t: u8, n: u8) -> (RistrettoPoint, Vec<(u8, Scalar)>) {
        // Gerçek sk_threshold = random secret
        let sk_threshold = Scalar::random(&mut OsRng);
        let pk_threshold = sk_threshold * RISTRETTO_BASEPOINT_POINT;

        // Shamir secret sharing: f(x) = sk_threshold + a1*x + a2*x^2 + ... (t-1 adet katsayı)
        let mut coeffs: Vec<Scalar> = vec![sk_threshold];
        for _ in 1..t {
            coeffs.push(Scalar::random(&mut OsRng));
        }

        // f(i) = share_i
        let shares: Vec<(u8, Scalar)> = (1..=n)
            .map(|i| {
                let x = Scalar::from(i as u64);
                let mut y = Scalar::ZERO;
                let mut x_pow = Scalar::ONE;
                for coeff in &coeffs {
                    y += coeff * x_pow;
                    x_pow *= x;
                }
                (i, y)
            })
            .collect();

        (pk_threshold, shares)
    }

    fn sample_raw_tx() -> RawTransaction {
        RawTransaction::new_transfer(
            test_address(0x01),
            test_address(0x02),
            Balance(500_000_000_000_000_000u128), // 0.5 NSC
            Nonce(3),
            1984,
        )
    }

    #[test]
    fn test_threshold_encrypt_decrypt_3_of_5() {
        let (pk_threshold, shares) = setup_threshold_keys(3, 5);
        let pk_bytes = pk_threshold.compress().to_bytes();

        let raw_tx = sample_raw_tx();
        let slot = SlotId(77);

        // Şifrele
        let enc_out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![])
            .expect("şifreleme başarılı olmalı");

        let encrypted_tx = &enc_out.encrypted_tx;

        // Herhangi 3 validator için share hesapla
        let tx_shares: Vec<TxDecryptionShare> = shares[..3]
            .iter()
            .map(|(idx, sk)| {
                let sk_bytes: [u8; 32] = sk.to_bytes();
                compute_tx_decryption_share(
                    &sk_bytes,
                    &encrypted_tx.ephemeral_pubkey,
                    *idx,
                    encrypted_tx.commitment,
                    slot,
                )
                .expect("share hesaplama başarılı olmalı")
            })
            .collect();

        // Threshold decrypt
        let decrypted = decrypt_transaction(encrypted_tx, &tx_shares, 3, 0)
            .expect("threshold decrypt başarılı olmalı");

        // Orijinal TX ile eşleşiyor mu?
        assert_eq!(decrypted.raw_tx.from, raw_tx.from);
        assert_eq!(decrypted.raw_tx.to, raw_tx.to);
        assert_eq!(decrypted.raw_tx.value, raw_tx.value);
        assert_eq!(decrypted.raw_tx.nonce, raw_tx.nonce);
        assert_eq!(decrypted.raw_tx.chain_id, raw_tx.chain_id);
    }

    #[test]
    fn test_threshold_decrypt_2_of_3() {
        let (pk_threshold, shares) = setup_threshold_keys(2, 3);
        let pk_bytes = pk_threshold.compress().to_bytes();

        let raw_tx = sample_raw_tx();
        let slot = SlotId(5);

        let enc_out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![])
            .expect("şifreleme başarılı");

        let encrypted_tx = &enc_out.encrypted_tx;

        // 2 validator yeterli (t=2)
        let tx_shares: Vec<TxDecryptionShare> = shares[..2]
            .iter()
            .map(|(idx, sk)| {
                let sk_bytes = sk.to_bytes();
                compute_tx_decryption_share(
                    &sk_bytes,
                    &encrypted_tx.ephemeral_pubkey,
                    *idx,
                    encrypted_tx.commitment,
                    slot,
                )
                .expect("share hesaplama başarılı")
            })
            .collect();

        let decrypted = decrypt_transaction(encrypted_tx, &tx_shares, 2, 0)
            .expect("2/3 threshold decrypt başarılı olmalı");

        assert_eq!(decrypted.raw_tx.nonce, raw_tx.nonce);
    }

    #[test]
    fn test_yetersiz_share_hatasi() {
        let (pk_threshold, shares) = setup_threshold_keys(3, 5);
        let pk_bytes = pk_threshold.compress().to_bytes();

        let raw_tx = sample_raw_tx();
        let slot = SlotId(1);

        let enc_out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![])
            .expect("şifreleme başarılı");

        let encrypted_tx = &enc_out.encrypted_tx;

        // Sadece 2 share (threshold = 3 → yeterli değil)
        let tx_shares: Vec<TxDecryptionShare> = shares[..2]
            .iter()
            .map(|(idx, sk)| {
                compute_tx_decryption_share(
                    &sk.to_bytes(),
                    &encrypted_tx.ephemeral_pubkey,
                    *idx,
                    encrypted_tx.commitment,
                    slot,
                )
                .expect("share hesaplama")
            })
            .collect();

        let result = decrypt_transaction(encrypted_tx, &tx_shares, 3, 0);
        assert!(
            matches!(result, Err(MempoolError::InsufficientDecryptionShares { .. })),
            "Yetersiz share → InsufficientDecryptionShares hatası bekleniyor"
        );
    }

    #[test]
    fn test_yanlis_share_hatasi() {
        let (pk_threshold, shares) = setup_threshold_keys(3, 5);
        let pk_bytes = pk_threshold.compress().to_bytes();

        let raw_tx = sample_raw_tx();
        let slot = SlotId(2);

        let enc_out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![])
            .expect("şifreleme başarılı");

        let encrypted_tx = &enc_out.encrypted_tx;

        // Yanlış tx_hash ile share
        let mut bad_share = TxDecryptionShare {
            tx_hash: TxHash::from_bytes([0xFF; 32]), // Yanlış hash
            validator_index: 1,
            d_i: [0u8; 32],
            slot,
        };
        let good_shares: Vec<TxDecryptionShare> = shares[1..3]
            .iter()
            .map(|(idx, sk)| {
                compute_tx_decryption_share(
                    &sk.to_bytes(),
                    &encrypted_tx.ephemeral_pubkey,
                    *idx,
                    encrypted_tx.commitment,
                    slot,
                )
                .expect("share hesaplama")
            })
            .collect();

        let mut all_shares = vec![bad_share.clone()];
        all_shares.extend(good_shares);

        let result = decrypt_transaction(encrypted_tx, &all_shares, 3, 0);
        assert!(result.is_err(), "Yanlış tx_hash'li share hata vermeli");
    }

    #[test]
    fn test_bozuk_sk_share_yanlis_sonuc() {
        let (pk_threshold, shares) = setup_threshold_keys(3, 5);
        let pk_bytes = pk_threshold.compress().to_bytes();

        let raw_tx = sample_raw_tx();
        let slot = SlotId(3);

        let enc_out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![])
            .expect("şifreleme başarılı");

        let encrypted_tx = &enc_out.encrypted_tx;

        // İlk share için yanlış sk kullan
        let wrong_sk = [0xAA; 32]; // Tamamen yanlış key
        let bad_share = compute_tx_decryption_share(
            &wrong_sk,
            &encrypted_tx.ephemeral_pubkey,
            1,
            encrypted_tx.commitment,
            slot,
        )
        .expect("share hesaplama şekli olarak çalışır");

        let mut tx_shares = vec![bad_share];
        for (idx, sk) in shares[1..3].iter() {
            tx_shares.push(
                compute_tx_decryption_share(
                    &sk.to_bytes(),
                    &encrypted_tx.ephemeral_pubkey,
                    *idx,
                    encrypted_tx.commitment,
                    slot,
                )
                .expect("share hesaplama"),
            );
        }

        // Yanlış share → decryption başarısız (AES authentication fail veya commitment mismatch)
        let result = decrypt_transaction(encrypted_tx, &tx_shares, 3, 0);
        assert!(result.is_err(), "Yanlış sk_share → decrypt başarısız olmalı");
    }

    #[test]
    fn test_compute_share_sifir_index_hatasi() {
        let result = compute_tx_decryption_share(
            &[0u8; 32],
            &[0u8; 32],
            0, // 0 geçersiz (1-indexed)
            TxHash::ZERO,
            SlotId(1),
        );
        assert!(
            matches!(result, Err(MempoolError::InvalidShareIndex { index: 0, .. })),
            "Index 0 → InvalidShareIndex hatası"
        );
    }

    #[test]
    fn test_unpad_gecerli() {
        // length_prefix_pad(data, 512) şemasıyla oluşturulmuş padding
        let data = vec![0xABu8; 10]; // 10 byte data
        let padded = crate::ibe::encrypt::length_prefix_pad(&data, 512)
            .expect("pad başarılı olmalı");
        assert_eq!(padded.len(), 512);

        let unpadded = crate::ibe::encrypt::length_prefix_unpad(&padded)
            .expect("unpad başarılı olmalı");
        assert_eq!(unpadded.len(), 10, "10 byte data geri gelmeli");
        assert_eq!(unpadded, &data[..], "veri birebir geri gelmeli");
    }

    #[test]
    fn test_unpad_bos_veri_hatasi() {
        let result = crate::ibe::encrypt::length_prefix_unpad(&[]);
        assert!(result.is_err(), "Boş veri → hata");
    }

    #[test]
    fn test_unpad_sifir_padding_hatasi() {
        // Sadece 3 byte (4 byte length prefix için yetersiz) → hata
        let data = vec![0u8; 3];
        let result = crate::ibe::encrypt::length_prefix_unpad(&data);
        assert!(result.is_err(), "Yetersiz boyut → hata");
    }

    #[test]
    fn test_lagrange_tek_share() {
        // t=1 durumu: sadece 1 share ile S doğru hesaplanmalı
        let sk = Scalar::random(&mut OsRng);
        let pk = sk * RISTRETTO_BASEPOINT_POINT;

        // Ephemeral key
        let e = Scalar::random(&mut OsRng);
        let e_point = e * RISTRETTO_BASEPOINT_POINT;

        // Beklenen S = sk * E
        let expected_s = sk * e_point;

        // Share: validator 1, D_1 = sk * E (t=1 → tek share = tam key)
        let d_1 = (sk * e_point).compress().to_bytes();
        let share = TxDecryptionShare {
            tx_hash: TxHash::ZERO,
            validator_index: 1,
            d_i: d_1,
            slot: SlotId(1),
        };

        let shares_ref = vec![&share];
        let s = lagrange_combine_shares(&shares_ref).expect("lagrange başarılı");

        // Lagrange(D_1) ile index=1 → λ_1 = 1 (tek eleman) → S = D_1
        // λ_1 = Π_{j≠1} ... (boş çarpı = 1)
        assert_eq!(
            s.compress().to_bytes(),
            expected_s.compress().to_bytes(),
            "Tek share Lagrange = share değeri"
        );
    }

    #[test]
    fn test_decrypt_ordering_index() {
        let (pk_threshold, shares) = setup_threshold_keys(2, 3);
        let pk_bytes = pk_threshold.compress().to_bytes();
        let raw_tx = sample_raw_tx();
        let slot = SlotId(1);

        let enc_out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![])
            .expect("şifreleme başarılı");

        let tx_shares: Vec<TxDecryptionShare> = shares[..2]
            .iter()
            .map(|(idx, sk)| {
                compute_tx_decryption_share(
                    &sk.to_bytes(),
                    &enc_out.encrypted_tx.ephemeral_pubkey,
                    *idx,
                    enc_out.encrypted_tx.commitment,
                    slot,
                )
                .expect("share")
            })
            .collect();

        let decrypted = decrypt_transaction(&enc_out.encrypted_tx, &tx_shares, 2, 42)
            .expect("decrypt");

        assert_eq!(decrypted.ordering_index, 42, "ordering_index aktarılmalı");
    }

    #[test]
    fn test_threshold_5_of_7() {
        let (pk_threshold, shares) = setup_threshold_keys(5, 7);
        let pk_bytes = pk_threshold.compress().to_bytes();

        let raw_tx = sample_raw_tx();
        let slot = SlotId(100);

        let enc_out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![])
            .expect("şifreleme başarılı");

        let tx_shares: Vec<TxDecryptionShare> = shares[..5]
            .iter()
            .map(|(idx, sk)| {
                compute_tx_decryption_share(
                    &sk.to_bytes(),
                    &enc_out.encrypted_tx.ephemeral_pubkey,
                    *idx,
                    enc_out.encrypted_tx.commitment,
                    slot,
                )
                .expect("share")
            })
            .collect();

        let decrypted = decrypt_transaction(&enc_out.encrypted_tx, &tx_shares, 5, 0)
            .expect("5/7 threshold decrypt");

        assert_eq!(decrypted.raw_tx.from, raw_tx.from);
        assert_eq!(decrypted.raw_tx.value, raw_tx.value);
    }
}
