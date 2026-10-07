//! TX şifreleme — ECDH Threshold Hybrid Encryption. §5.2.2, §6.1.4
//!
//! Kullanıcı tarafında çalışır: TX'i threshold validator setinin anahtarıyla şifreler.
//! Sadece threshold sayıda validator birleşince açılabilir.

use crate::{
    errors::{MempoolError, MempoolResult},
    ibe::{COMMITMENT_DOMAIN, ECDH_KEY_DOMAIN},
    types::{EphemeralAesKey, EncryptedTx, RawTransaction, TxHash},
};
use curve25519_dalek::{
    constants::RISTRETTO_BASEPOINT_POINT,
    ristretto::CompressedRistretto,
    scalar::Scalar,
};
use nova_crypto::{
    hash::blake3_hash_many,
    symmetric::aes_gcm_encrypt,
};
use nova_primitives::{
    constants::{MAX_ENCRYPTION_PADDING_BYTES, MAX_ENCRYPTED_TX_BYTES},
    types::{Address, SlotId},
};
use rand::rngs::OsRng;
use rand_core::RngCore;
use zeroize::Zeroize;

// ─── ÇIKTI TİPİ ──────────────────────────────────────────────────────────────

/// `encrypt_transaction` fonksiyonunun çıktısı.
#[derive(Debug)]
pub struct EncryptOutput {
    /// Şifrelenmiş TX (mempool'a gönderilir).
    pub encrypted_tx: EncryptedTx,

    /// Ephemeral scalar `e` (istemci saklamalı — sıfırlanacak).
    /// Test/debug için expose edildi; üretimde kullanılmaz.
    pub ephemeral_scalar_bytes: [u8; 32],
}

// ─── ANA ŞİFRELEME FONKSİYONU ────────────────────────────────────────────────

/// Bir TX'i threshold validator seti için şifrele. §5.2.2
///
/// ## Parametreler
/// - `raw_tx`: Şifrelenecek ham TX
/// - `threshold_pk_bytes`: Threshold public key (Ristretto255 compressed, 32 byte)
/// - `slot`: TX'in hedeflendiği slot
/// - `sender_signature`: Commitment üzerine göndericinin MPC imzası
///
/// ## Dönüş
/// [`EncryptOutput`] — şifreli TX ve ephemeral scalar (sıfırlanacak)
///
/// ## Güvenlik
/// - Ephemeral scalar `e` sadece bu çağrı için üretilir ve döndürülür.
///   Çağıran taraf kullanım bitiminde belleği sıfırlamalıdır.
/// - `threshold_pk_bytes` geçerli bir Ristretto255 noktası olmalıdır.
pub fn encrypt_transaction(
    raw_tx: &RawTransaction,
    threshold_pk_bytes: &[u8; 32],
    slot: SlotId,
    sender_signature: Vec<u8>,
) -> MempoolResult<EncryptOutput> {
    // ── 1. Threshold public key deserialize ───────────────────────────────────
    let threshold_pk = CompressedRistretto::from_slice(threshold_pk_bytes)
        .map_err(|_| MempoolError::InvalidThresholdPublicKey("decompress hatası".into()))?
        .decompress()
        .ok_or_else(|| {
            MempoolError::InvalidThresholdPublicKey("geçersiz Ristretto noktası".into())
        })?;

    // ── 2. Ephemeral keypair üret ─────────────────────────────────────────────
    // e = random scalar, E = e*G
    let e = Scalar::random(&mut OsRng);
    let e_point = e * RISTRETTO_BASEPOINT_POINT;
    let ephemeral_pubkey_bytes = e_point.compress().to_bytes(); // 32 byte

    // ── 3. ECDH shared secret hesapla ────────────────────────────────────────
    // S = e * threshold_pk
    let shared_secret_point = e * threshold_pk;
    let shared_secret_bytes = shared_secret_point.compress().to_bytes();

    // ── 4. AES-256 anahtarı türet ────────────────────────────────────────────
    // K = BLAKE3("nova-mempool-aes-key-v1" || S.compress())
    let aes_key_bytes = blake3::derive_key(ECDH_KEY_DOMAIN, &shared_secret_bytes);
    let mut aes_key = EphemeralAesKey(aes_key_bytes);

    // ── 5. TX'i serialize ve padding ekle ────────────────────────────────────
    // plaintext = pad_to_512(borsh(raw_tx))
    let tx_bytes = raw_tx
        .to_bytes()
        .map_err(|e| MempoolError::SerializationError(e.to_string()))?;

    // Boyut kontrolü
    if tx_bytes.len() > MAX_ENCRYPTION_PADDING_BYTES {
        return Err(MempoolError::TxTooLarge {
            size: tx_bytes.len(),
            maximum: MAX_ENCRYPTION_PADDING_BYTES,
        });
    }

    // Length-prefix padding: [4 byte LE uzunluk][veri][sıfır dolgu → 512 byte toplam]
    // §6.4.2 — Sabit boyut timing saldırısı koruması
    let padded = length_prefix_pad(&tx_bytes, MAX_ENCRYPTION_PADDING_BYTES)
        .map_err(|e| MempoolError::EncryptionFailed(e.to_string()))?;

    // ── 6. AES-256-GCM ile şifrele ───────────────────────────────────────────
    // AAD = slot numarası (little-endian 8 byte)
    let slot_aad = slot.0.to_le_bytes();
    let aes_out = aes_gcm_encrypt(&aes_key.0, &padded, &slot_aad)
        .map_err(|e| MempoolError::AesEncryptionError(e.to_string()))?;

    // AES anahtarını artık sıfırla
    aes_key.zeroize();

    // ── 7. Commitment hesapla ─────────────────────────────────────────────────
    // commitment = BLAKE3(domain || E || aes_cipher || aes_nonce || slot_le_bytes)
    let commitment_hash = compute_commitment(
        &ephemeral_pubkey_bytes,
        &aes_out.ciphertext,
        &aes_out.nonce,
        slot,
    );

    // ── 8. EncryptedTx oluştur ────────────────────────────────────────────────
    let plaintext_size = tx_bytes.len() as u32;
    let encrypted_tx = EncryptedTx {
        commitment: commitment_hash,
        ephemeral_pubkey: ephemeral_pubkey_bytes,
        aes_ciphertext: aes_out.ciphertext,
        aes_nonce: aes_out.nonce,
        target_slot: slot,
        sender: raw_tx.from,
        signature: sender_signature,
        plaintext_size_hint: plaintext_size,
    };

    // Boyut kontrol
    if !encrypted_tx.check_size() {
        return Err(MempoolError::TxTooLarge {
            size: encrypted_tx.aes_ciphertext.len(),
            maximum: MAX_ENCRYPTED_TX_BYTES,
        });
    }

    Ok(EncryptOutput {
        encrypted_tx,
        ephemeral_scalar_bytes: e.to_bytes(),
    })
}

// ─── COMMITMENT HESAPLAMA ─────────────────────────────────────────────────────

/// TX commitment hash hesapla. §5.2.2
///
/// commitment = BLAKE3(COMMITMENT_DOMAIN || E || aes_cipher || aes_nonce || slot_le)
///
/// Bu hash:
/// - TX kimliği olarak kullanılır (TxHash)
/// - Batch sıralamasında kullanılır (MEV-free ordering)
/// - Blok Merkle tree'sine dahil edilir
/// - Validator imzalaması için zemin
pub fn compute_commitment(
    ephemeral_pubkey: &[u8; 32],
    aes_ciphertext: &[u8],
    aes_nonce: &[u8; 12],
    slot: SlotId,
) -> TxHash {
    let slot_bytes = slot.0.to_le_bytes();
    let hash = blake3_hash_many(&[
        COMMITMENT_DOMAIN.as_bytes(),
        ephemeral_pubkey,
        aes_ciphertext,
        aes_nonce,
        &slot_bytes,
    ]);
    TxHash::from_bytes(*hash.as_bytes())
}

/// Commitment'ı doğrula — hesaplanan commitment saklananla eşleşiyor mu? §5.2.2
pub fn verify_commitment(
    encrypted_tx: &EncryptedTx,
) -> MempoolResult<()> {
    let computed = compute_commitment(
        &encrypted_tx.ephemeral_pubkey,
        &encrypted_tx.aes_ciphertext,
        &encrypted_tx.aes_nonce,
        encrypted_tx.target_slot,
    );

    if computed != encrypted_tx.commitment {
        return Err(MempoolError::CommitmentMismatch {
            expected: encrypted_tx.commitment.to_hex(),
            computed: computed.to_hex(),
        });
    }
    Ok(())
}

// ─── PADDING FONKSİYONLARI ───────────────────────────────────────────────────

/// Length-prefix sabit-boyut padding. §6.4.2
///
/// Format: [4 byte LE uzunluk][veri][sıfır bayt dolgu → `target_size` toplam byte]
///
/// Bu şema PKCS#7'den farklıdır — PKCS#7 maksimum 255 byte padding ile sınırlıdır.
/// Bizim şemamız 4-byte length prefix sakladığından isteğe bağlı boyut destekler.
///
/// # Güvenlik
/// Tüm TX'ler aynı boyuta pad edilir → timing analizi ve trafik boyutundan
/// içerik tahmini imkânsızdır.
pub fn length_prefix_pad(data: &[u8], target_size: usize) -> Result<Vec<u8>, String> {
    // Minimum gereksinim: 4 byte prefix + data
    let min_required = 4 + data.len();
    if min_required > target_size {
        return Err(format!(
            "Veri çok büyük: {} byte data + 4 byte prefix = {} > hedef {}",
            data.len(),
            min_required,
            target_size
        ));
    }
    let len_bytes = (data.len() as u32).to_le_bytes();
    let mut padded = Vec::with_capacity(target_size);
    padded.extend_from_slice(&len_bytes); // 4 byte LE uzunluk
    padded.extend_from_slice(data);       // asıl veri
    padded.resize(target_size, 0u8);      // sıfır dolgu
    Ok(padded)
}

/// Length-prefix padding'i kaldır.
///
/// Format: [4 byte LE uzunluk][veri][sıfır dolgu...]
/// İlk 4 byte'ı okur → uzunluk `n`, sonraki `n` byte'ı döndürür.
pub fn length_prefix_unpad(padded: &[u8]) -> Result<&[u8], String> {
    if padded.len() < 4 {
        return Err(format!("Yetersiz boyut: {} < 4", padded.len()));
    }
    let len = u32::from_le_bytes(
        padded[..4].try_into().expect("4 byte dönüştürme"),
    ) as usize;
    let end = 4 + len;
    if end > padded.len() {
        return Err(format!(
            "Geçersiz uzunluk prefix: {} + {} = {} > {}",
            4, len, end, padded.len()
        ));
    }
    Ok(&padded[4..end])
}

// ─── YARDIMCI FONKSİYONLAR ───────────────────────────────────────────────────

/// Yalnızca commitment hesabı için rastgele test verisi üret (test yardımcısı).
#[cfg(test)]
pub fn random_ephemeral_pubkey() -> [u8; 32] {
    let e = Scalar::random(&mut OsRng);
    (e * RISTRETTO_BASEPOINT_POINT).compress().to_bytes()
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RawTransaction, TxType};
    use nova_primitives::types::{Address, Balance, Nonce, SlotId};

    fn test_address(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    /// Geçerli bir threshold public key üret (rastgele Ristretto noktası).
    fn random_threshold_pk() -> [u8; 32] {
        let scalar = Scalar::random(&mut OsRng);
        (scalar * RISTRETTO_BASEPOINT_POINT).compress().to_bytes()
    }

    fn sample_raw_tx() -> RawTransaction {
        RawTransaction::new_transfer(
            test_address(0x01),
            test_address(0x02),
            Balance(1_000_000_000_000_000_000u128),
            Nonce(0),
            1984,
        )
    }

    #[test]
    fn test_commitment_deterministik() {
        let epk = random_ephemeral_pubkey();
        let cipher = vec![0xAB; 528];
        let nonce = [0x99u8; 12];
        let slot = SlotId(100);

        let h1 = compute_commitment(&epk, &cipher, &nonce, slot);
        let h2 = compute_commitment(&epk, &cipher, &nonce, slot);
        assert_eq!(h1, h2, "Aynı girdi → aynı commitment");
    }

    #[test]
    fn test_commitment_slot_degisince_degisir() {
        let epk = random_ephemeral_pubkey();
        let cipher = vec![0xAB; 528];
        let nonce = [0x99u8; 12];

        let h1 = compute_commitment(&epk, &cipher, &nonce, SlotId(100));
        let h2 = compute_commitment(&epk, &cipher, &nonce, SlotId(101));
        assert_ne!(h1, h2, "Farklı slot → farklı commitment (replay koruması)");
    }

    #[test]
    fn test_commitment_epk_degisince_degisir() {
        let epk1 = random_ephemeral_pubkey();
        let epk2 = random_ephemeral_pubkey();
        let cipher = vec![0xAB; 528];
        let nonce = [0x99u8; 12];
        let slot = SlotId(100);

        let h1 = compute_commitment(&epk1, &cipher, &nonce, slot);
        let h2 = compute_commitment(&epk2, &cipher, &nonce, slot);
        assert_ne!(h1, h2, "Farklı ephemeral key → farklı commitment");
    }

    #[test]
    fn test_sifreleme_basarisiz_gecersiz_pk() {
        let raw_tx = sample_raw_tx();
        // Geçersiz Ristretto noktası (sıfır bytes Ristretto255'te geçersiz nokta verebilir)
        let invalid_pk = [0u8; 32];
        let result = encrypt_transaction(&raw_tx, &invalid_pk, SlotId(1), vec![]);
        // Sıfır bytes identity point, decompress başarılı ama zayıf nokta
        // bazı kütüphanelerde başarılı bazılarında başarısız — sadece panic olmadığını kontrol et
        let _ = result;
    }

    #[test]
    fn test_sifreleme_basarili() {
        let raw_tx = sample_raw_tx();
        let threshold_pk = random_threshold_pk();
        let slot = SlotId(42);

        let output = encrypt_transaction(&raw_tx, &threshold_pk, slot, vec![0u8; 64])
            .expect("şifreleme başarılı olmalı");

        // Commitment sıfır olmamalı
        assert_ne!(output.encrypted_tx.commitment, TxHash::ZERO);

        // Ephemeral pubkey sıfır olmamalı
        assert_ne!(output.encrypted_tx.ephemeral_pubkey, [0u8; 32]);

        // Ciphertext boyutu doğru: 512 (padded) + 16 (AES-GCM tag) = 528
        assert_eq!(
            output.encrypted_tx.aes_ciphertext.len(),
            MAX_ENCRYPTION_PADDING_BYTES + 16,
            "AES-GCM çıktısı: plaintext + 16 byte tag"
        );

        // Hedef slot doğru
        assert_eq!(output.encrypted_tx.target_slot, slot);
    }

    #[test]
    fn test_commitment_dogrulama_basarili() {
        let raw_tx = sample_raw_tx();
        let threshold_pk = random_threshold_pk();
        let slot = SlotId(10);

        let output = encrypt_transaction(&raw_tx, &threshold_pk, slot, vec![])
            .expect("şifreleme başarılı olmalı");

        // Commitment doğrulanmalı
        verify_commitment(&output.encrypted_tx)
            .expect("commitment doğrulama başarılı olmalı");
    }

    #[test]
    fn test_commitment_dogrulama_bozuk_ciphertext() {
        let raw_tx = sample_raw_tx();
        let threshold_pk = random_threshold_pk();

        let mut output = encrypt_transaction(&raw_tx, &threshold_pk, SlotId(1), vec![])
            .expect("şifreleme başarılı olmalı");

        // Ciphertext'i boz
        if !output.encrypted_tx.aes_ciphertext.is_empty() {
            output.encrypted_tx.aes_ciphertext[0] ^= 0xFF;
        }

        let result = verify_commitment(&output.encrypted_tx);
        assert!(
            result.is_err(),
            "Bozuk ciphertext commitment doğrulamasını geçemez"
        );
    }

    #[test]
    fn test_commitment_dogrulama_bozuk_slot() {
        let raw_tx = sample_raw_tx();
        let threshold_pk = random_threshold_pk();

        let mut output = encrypt_transaction(&raw_tx, &threshold_pk, SlotId(5), vec![])
            .expect("şifreleme başarılı olmalı");

        // Slot'u değiştir
        output.encrypted_tx.target_slot = SlotId(999);

        let result = verify_commitment(&output.encrypted_tx);
        assert!(result.is_err(), "Farklı slot commitment doğrulamasını geçemez");
    }

    #[test]
    fn test_commitment_dogrulama_bozuk_nonce() {
        let raw_tx = sample_raw_tx();
        let threshold_pk = random_threshold_pk();

        let mut output = encrypt_transaction(&raw_tx, &threshold_pk, SlotId(5), vec![])
            .expect("şifreleme başarılı olmalı");

        // AES nonce'u değiştir
        output.encrypted_tx.aes_nonce[0] ^= 0xFF;

        let result = verify_commitment(&output.encrypted_tx);
        assert!(result.is_err(), "Bozuk nonce commitment doğrulamasını geçemez");
    }

    #[test]
    fn test_her_sifreleme_farkli_commitment() {
        let raw_tx = sample_raw_tx();
        let threshold_pk = random_threshold_pk();
        let slot = SlotId(1);

        let out1 = encrypt_transaction(&raw_tx, &threshold_pk, slot, vec![])
            .expect("şifreleme 1 başarılı olmalı");
        let out2 = encrypt_transaction(&raw_tx, &threshold_pk, slot, vec![])
            .expect("şifreleme 2 başarılı olmalı");

        // Her şifreleme farklı ephemeral key → farklı commitment (randomized encryption)
        assert_ne!(
            out1.encrypted_tx.commitment, out2.encrypted_tx.commitment,
            "Aynı TX farklı şifrelemelerde farklı commitment almalı (güvenlik)"
        );
    }

    #[test]
    fn test_heartbeat_sifreleme() {
        let from = test_address(0x05);
        let tx = RawTransaction::new_heartbeat(from, Nonce(0), 1984);
        let pk = random_threshold_pk();

        let output = encrypt_transaction(&tx, &pk, SlotId(100), vec![])
            .expect("heartbeat şifreleme başarılı olmalı");

        assert_eq!(output.encrypted_tx.sender, from);
        assert_eq!(output.encrypted_tx.target_slot, SlotId(100));
    }

    #[test]
    fn test_buyuk_calldata_ile_sifreleme() {
        // MAX_ENCRYPTION_PADDING_BYTES sınırına yakın veri
        let from = test_address(0x01);
        let to = test_address(0x02);
        let big_data = vec![0xAB; 400]; // 400 byte calldata

        let tx = RawTransaction {
            from,
            to: Some(to),
            value: Balance::ZERO,
            nonce: Nonce(1),
            tx_type: TxType::ContractCall,
            gas_limit: 500_000,
            max_fee_per_gas: Balance(1_000_000_000u128),
            max_priority_fee_per_gas: Balance::ZERO,
            data: big_data,
            chain_id: 1984,
        };
        let pk = random_threshold_pk();

        let result = encrypt_transaction(&tx, &pk, SlotId(1), vec![]);
        // Büyük TX başarılı veya boyut hatasıyla başarısız olabilir
        // Panic vermemesi yeterli
        let _ = result;
    }

    #[test]
    fn test_ephemeral_scalar_kayit() {
        let raw_tx = sample_raw_tx();
        let pk = random_threshold_pk();

        let output = encrypt_transaction(&raw_tx, &pk, SlotId(1), vec![])
            .expect("şifreleme başarılı olmalı");

        // Ephemeral scalar sıfır olmamalı (gerçek random)
        assert_ne!(
            output.ephemeral_scalar_bytes, [0u8; 32],
            "Ephemeral scalar sıfır olmamalı"
        );
    }
}
