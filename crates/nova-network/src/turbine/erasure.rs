//! Turbine Reed-Solomon Erasure Coding — Solana pattern, GF(2^8) gerçek implementasyon
//!
//! ## Araştırma Kaynakları
//! - Solana analizi (agent-09.md §10): Turbine shred, FEC seti, stake-weighted tree
//! - Solana tokenomics (01-SOLANA-NOVA-ARASTIRMA.md §D): Turbine shred mimarisi
//! - GAP-ANALIZ.md: Reed-Solomon gerçek implementasyon gereksinimi
//!
//! ## Mimari (Solana'dan)
//! - Her "erasure batch" = DATA_SHREDS_PER_FEC + CODE_SHREDS_PER_FEC shred
//! - DATA:CODE oranı varsayılan = 32:32
//! - k data shred + (n-k) code shred → herhangi k tanesiyle tüm veri yeniden kurulur
//! - GF(2^8) (Galois Field) üzerinde gerçek Reed-Solomon (`reed-solomon-erasure` crate)
//!
//! ## Solana ile Fark
//! - Solana: CODING_SHREDS_PER_FEC = DATA_SHREDS_PER_FEC (32+32)
//! - Nova: Yapılandırılabilir data/code oranı (test için 4+4, üretim için 32+32)
//! - API aynı: `ErasureEncoder::encode()`, `ErasureDecoder::decode()`

use super::shred::{Shred, ShredBuilder, ShredId, ShredType, SHRED_DATA_PAYLOAD_SIZE};
use reed_solomon_erasure::galois_8::ReedSolomon;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Her FEC setindeki varsayılan data shred sayısı (Solana: 32)
pub const DATA_SHREDS_PER_FEC: usize = 32;

/// Her FEC setindeki varsayılan code shred sayısı (Solana: 32)
pub const CODE_SHREDS_PER_FEC: usize = 32;

/// Veri kurtarma için gerekli minimum shred sayısı
pub const MIN_SHREDS_FOR_RECOVERY: usize = DATA_SHREDS_PER_FEC;

// ─── ErasureConfig ────────────────────────────────────────────────────────────

/// Reed-Solomon erasure yapılandırması
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErasureConfig {
    /// Data shred sayısı (k)
    pub num_data: usize,
    /// Code shred sayısı (m)
    pub num_code: usize,
}

impl ErasureConfig {
    pub fn new(num_data: usize, num_code: usize) -> Self {
        Self { num_data, num_code }
    }

    /// Varsayılan: 32 data + 32 code
    pub fn default_32_32() -> Self {
        Self::new(DATA_SHREDS_PER_FEC, CODE_SHREDS_PER_FEC)
    }

    /// Hafif: 16 data + 8 code (düşük bant genişliği)
    pub fn light_16_8() -> Self {
        Self::new(16, 8)
    }

    /// Toplam shred sayısı
    pub fn total(&self) -> usize {
        self.num_data + self.num_code
    }

    /// Maksimum tolere edilen kayıp (m shred kaybolabilir)
    pub fn max_erasures(&self) -> usize {
        self.num_code
    }

    /// Kurtarma oranı: kaç shred kaybolabilir / toplam
    pub fn recovery_ratio(&self) -> f64 {
        self.num_code as f64 / self.total() as f64
    }
}

impl Default for ErasureConfig {
    fn default() -> Self {
        Self::default_32_32()
    }
}

// ─── CodeShred ────────────────────────────────────────────────────────────────

/// Erasure code shred (RS GF(2^8) parity verisi)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeShred {
    pub slot: u64,
    pub fec_set_index: u32,
    /// Bu code shred'in FEC set içindeki indeksi
    pub code_index: u32,
    /// GF(2^8) Reed-Solomon parity verisi
    pub parity: Vec<u8>,
    /// Encode edildiğinde kaç data shred vardı
    pub num_data_shreds: u32,
    pub num_code_shreds: u32,
}

impl CodeShred {
    pub fn id(&self) -> ShredId {
        ShredId::new(self.slot, self.fec_set_index * 1000 + self.code_index, ShredType::Code)
    }
}

// ─── FecSet ───────────────────────────────────────────────────────────────────

/// Forward Error Correction seti — data + code shred'leri bir arada
#[derive(Debug, Clone)]
pub struct FecSet {
    pub slot: u64,
    pub fec_set_index: u32,
    pub data_shreds: Vec<Shred>,
    pub code_shreds: Vec<CodeShred>,
    pub config: ErasureConfig,
}

impl FecSet {
    pub fn data_count(&self) -> usize {
        self.data_shreds.len()
    }

    pub fn code_count(&self) -> usize {
        self.code_shreds.len()
    }

    /// Kurtarma mümkün mü?
    ///
    /// Reed-Solomon teoremi: k data shred ile m code shred'den herhangi k
    /// tanesiyle tüm batch yeniden kurulabilir.
    /// Yani data + code toplam >= num_data koşulu sağlanırsa kurtarma mümkündür.
    pub fn can_recover(&self) -> bool {
        self.data_shreds.len() + self.code_shreds.len() >= self.config.num_data
    }

    /// Tamamlandı mı? (tüm data shred'ler var)
    pub fn is_complete(&self) -> bool {
        self.data_shreds.len() == self.config.num_data
    }
}

// ─── ErasureEncoder ───────────────────────────────────────────────────────────

/// Data shred'lerden GF(2^8) Reed-Solomon code shred'leri üret
///
/// Solana Turbine kaynağından (turbine/src/broadcast_stage.rs):
/// - CODING_SHREDS_PER_FEC = DATA_SHREDS_PER_FEC (32+32 varsayılan)
/// - Gerçek GF(2^8) matris çarpımı, sistematik kod
pub struct ErasureEncoder {
    config: ErasureConfig,
}

impl ErasureEncoder {
    pub fn new(config: ErasureConfig) -> Self {
        Self { config }
    }

    /// Data shred'lerinden GF(2^8) Reed-Solomon code shred'leri üret
    ///
    /// Tüm payload'lar SHRED_DATA_PAYLOAD_SIZE'a pad edilir.
    /// RS encode: k data shard → m parity shard (GF(2^8) Vandermonde matrix)
    pub fn encode(&self, slot: u64, fec_set_index: u32, data_shreds: &[Shred]) -> Vec<CodeShred> {
        let num_data = data_shreds.len();
        if num_data == 0 {
            return vec![];
        }

        let num_code = self.config.num_code;
        let shard_size = SHRED_DATA_PAYLOAD_SIZE;

        // RS encoder: k=num_data, m=num_code
        let rs = ReedSolomon::new(num_data, num_code)
            .expect("RS encoder oluşturulamadı — geçersiz shard sayısı");

        // Flat shard listesi: [data_0, data_1, ..., data_k-1, parity_0, ..., parity_m-1]
        let mut shards: Vec<Vec<u8>> = Vec::with_capacity(num_data + num_code);

        // Data shred'lerin payload'larını shard_size'a pad et
        for shred in data_shreds {
            let mut p = shred.payload.clone();
            p.resize(shard_size, 0);
            shards.push(p);
        }

        // Boş parity shard'ları ekle (RS encode dolduracak)
        for _ in 0..num_code {
            shards.push(vec![0u8; shard_size]);
        }

        // GF(2^8) Reed-Solomon encode
        rs.encode(&mut shards)
            .expect("RS encode başarısız");

        // Parity shard'larını CodeShred'e dönüştür
        shards[num_data..]
            .iter()
            .enumerate()
            .map(|(code_idx, parity)| CodeShred {
                slot,
                fec_set_index,
                code_index: code_idx as u32,
                parity: parity.clone(),
                num_data_shreds: num_data as u32,
                num_code_shreds: num_code as u32,
            })
            .collect()
    }
}

// ─── ErasureDecoder ───────────────────────────────────────────────────────────

/// Eksik data shred'lerini GF(2^8) Reed-Solomon code shred'lerinden kurtar
///
/// Berlekamp-Welch / Gao algoritması (reed-solomon-erasure crate implementasyonu).
/// Yeterli shard varsa kayıp veri tamamen kurtarılır.
pub struct ErasureDecoder {
    config: ErasureConfig,
}

impl ErasureDecoder {
    pub fn new(config: ErasureConfig) -> Self {
        Self { config }
    }

    /// Eksik data shred'lerini kurtar
    ///
    /// - `data_shreds`: Alınan data shred'ler. `None` = eksik.
    /// - `code_shreds`: Alınan code shred'ler (code_index sıralı değil olabilir).
    /// - `slot`: Kurtarılan shred'lerin slot numarası.
    ///
    /// Başarılı olursa `num_data` adet kurtarılmış data shred döner.
    pub fn decode(
        &self,
        data_shreds: Vec<Option<Shred>>,
        code_shreds: &[CodeShred],
        slot: u64,
    ) -> Result<Vec<Shred>, ErasureError> {
        let num_data = data_shreds.len();
        let num_code = self.config.num_code;
        let shard_size = SHRED_DATA_PAYLOAD_SIZE;

        // Mevcut shred sayısını say
        let present_data = data_shreds.iter().filter(|s| s.is_some()).count();
        let total_present = present_data + code_shreds.len();

        // RS teoremi: data+code toplamı num_data'dan az → kurtarma imkânsız
        if total_present < num_data {
            return Err(ErasureError::InsufficientShreds {
                present: total_present,
                required: num_data,
            });
        }

        // Hızlı yol: tüm data shred'ler mevcutsa RS'e gerek yok
        if present_data == num_data {
            return Ok(data_shreds.into_iter().flatten().collect());
        }

        // RS decoder için flat shard listesi oluştur
        // [data_slot_0, ..., data_slot_k-1, parity_slot_0, ..., parity_slot_m-1]
        let mut shards: Vec<Option<Vec<u8>>> = Vec::with_capacity(num_data + num_code);

        // Data slot'larını doldur (None = eksik → RS kurtaracak)
        for opt_shred in &data_shreds {
            shards.push(opt_shred.as_ref().map(|s| {
                let mut p = s.payload.clone();
                p.resize(shard_size, 0);
                p
            }));
        }

        // Code shred'leri code_index'e göre parity slot'larına yerleştir
        let code_map: std::collections::HashMap<u32, &CodeShred> =
            code_shreds.iter().map(|c| (c.code_index, c)).collect();

        for i in 0..num_code as u32 {
            shards.push(code_map.get(&i).map(|c| c.parity.clone()));
        }

        // GF(2^8) Reed-Solomon reconstruct
        let rs = ReedSolomon::new(num_data, num_code)
            .map_err(|_| ErasureError::InvalidConfig)?;

        rs.reconstruct(&mut shards).map_err(|_| ErasureError::RecoveryFailed {
            recovered: present_data,
            required: num_data,
        })?;

        // Kurtarılan shard'lardan Shred nesneleri oluştur
        // Shred indeksini mevcut shred'lerden hesapla
        let base_index = data_shreds
            .iter()
            .enumerate()
            .find_map(|(i, opt)| opt.as_ref().map(|s| s.id.index.saturating_sub(i as u32)))
            .unwrap_or(0);

        let leader = data_shreds
            .iter()
            .flatten()
            .next()
            .map(|s| s.leader)
            .unwrap_or([0u8; 32]);

        let mut result = Vec::with_capacity(num_data);
        for i in 0..num_data {
            let payload = shards[i]
                .as_ref()
                .ok_or(ErasureError::RecoveryFailed {
                    recovered: i,
                    required: num_data,
                })?
                .clone();

            // Mevcut shred'se orijinalini al; kurtarıldıysa yeniden oluştur
            if let Some(Some(original)) = data_shreds.get(i) {
                result.push(original.clone());
            } else {
                result.push(Shred::new(
                    slot,
                    base_index + i as u32,
                    ShredType::Data,
                    leader,
                    payload,
                ));
            }
        }

        Ok(result)
    }
}

// ─── ErasureBatchBuilder ──────────────────────────────────────────────────────

/// Blok → FEC setlerine böl + GF(2^8) RS encode
pub struct ErasureBatchBuilder {
    config: ErasureConfig,
    builder: ShredBuilder,
}

impl ErasureBatchBuilder {
    pub fn new(leader_key: [u8; 32], config: ErasureConfig) -> Self {
        Self {
            config,
            builder: ShredBuilder::new(leader_key, SHRED_DATA_PAYLOAD_SIZE),
        }
    }

    /// Blok verisini FEC setlerine böl ve RS encode et
    pub fn build_fec_sets(&self, slot: u64, block_data: &[u8]) -> Vec<FecSet> {
        let all_shreds = self.builder.shred_block(slot, block_data);
        let encoder = ErasureEncoder::new(self.config.clone());

        let mut fec_sets = Vec::new();
        let num_data = self.config.num_data;

        for (set_idx, chunk) in all_shreds.chunks(num_data).enumerate() {
            let data_shreds: Vec<Shred> = chunk.to_vec();
            let code_shreds = encoder.encode(slot, set_idx as u32, &data_shreds);

            fec_sets.push(FecSet {
                slot,
                fec_set_index: set_idx as u32,
                data_shreds,
                code_shreds,
                config: self.config.clone(),
            });
        }

        fec_sets
    }
}

// ─── ErasureError ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErasureError {
    InsufficientShreds { present: usize, required: usize },
    RecoveryFailed { recovered: usize, required: usize },
    InvalidConfig,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shreds(slot: u64, count: usize) -> Vec<Shred> {
        let builder = ShredBuilder::new([1u8; 32], SHRED_DATA_PAYLOAD_SIZE);
        // Her shred tam SHRED_DATA_PAYLOAD_SIZE bayt payload taşır
        let data: Vec<u8> = (0..count * SHRED_DATA_PAYLOAD_SIZE)
            .map(|i| (i & 0xFF) as u8)
            .collect();
        let shreds = builder.shred_block(slot, &data);
        shreds.into_iter().take(count).collect()
    }

    // 1. ErasureConfig hesapları doğru
    #[test]
    fn erasure_config_calculations() {
        let cfg = ErasureConfig::new(32, 16);
        assert_eq!(cfg.total(), 48);
        assert_eq!(cfg.max_erasures(), 16);
        let ratio = cfg.recovery_ratio();
        assert!((ratio - 16.0 / 48.0).abs() < 1e-6);
    }

    // 2. Encoder doğru sayıda code shred üretir
    #[test]
    fn encoder_produces_correct_count() {
        let config = ErasureConfig::new(4, 4);
        let encoder = ErasureEncoder::new(config.clone());
        let shreds = make_shreds(100, 4);
        let codes = encoder.encode(100, 0, &shreds);
        assert_eq!(codes.len(), 4);
    }

    // 3. Code shred metadata doğru
    #[test]
    fn code_shred_metadata_correct() {
        let config = ErasureConfig::new(4, 2);
        let encoder = ErasureEncoder::new(config);
        let shreds = make_shreds(42, 4);
        let codes = encoder.encode(42, 5, &shreds);
        assert_eq!(codes[0].slot, 42);
        assert_eq!(codes[0].fec_set_index, 5);
        assert_eq!(codes[0].code_index, 0);
        assert_eq!(codes[1].code_index, 1);
        assert_eq!(codes[0].num_data_shreds, 4);
        assert_eq!(codes[0].num_code_shreds, 2);
    }

    // 4. Decoder: kayıp yok → tüm shred'ler döner
    #[test]
    fn decoder_no_loss_returns_all() {
        let config = ErasureConfig::new(4, 4);
        let encoder = ErasureEncoder::new(config.clone());
        let decoder = ErasureDecoder::new(config);
        let shreds = make_shreds(100, 4);
        let codes = encoder.encode(100, 0, &shreds.clone());

        let data_opts: Vec<Option<Shred>> = shreds.into_iter().map(Some).collect();
        let result = decoder.decode(data_opts, &codes, 100).unwrap();
        assert_eq!(result.len(), 4);
    }

    // 5. Decoder: yetersiz shred → hata
    #[test]
    fn decoder_insufficient_returns_error() {
        let config = ErasureConfig::new(4, 2);
        let encoder = ErasureEncoder::new(config.clone());
        let decoder = ErasureDecoder::new(config);
        let shreds = make_shreds(100, 4);
        let _codes = encoder.encode(100, 0, &shreds);

        // Sadece 1 data + 0 code shred → yetersiz (1 < 4)
        let data_opts = vec![
            Some(shreds[0].clone()),
            None,
            None,
            None,
        ];
        let result = decoder.decode(data_opts, &[], 100);
        assert!(matches!(result, Err(ErasureError::InsufficientShreds { .. })));
    }

    // 6. FecSet tamamlanma kontrolü
    #[test]
    fn fec_set_completion_check() {
        let config = ErasureConfig::new(4, 2);
        let encoder = ErasureEncoder::new(config.clone());
        let shreds = make_shreds(100, 4);
        let codes = encoder.encode(100, 0, &shreds.clone());

        let fec = FecSet {
            slot: 100,
            fec_set_index: 0,
            data_shreds: shreds,
            code_shreds: codes,
            config,
        };

        assert!(fec.is_complete());
        assert!(fec.can_recover());
    }

    // 7. FecSet kurtarma yeterliliği
    #[test]
    fn fec_set_partial_can_recover() {
        let config = ErasureConfig::new(4, 2);
        let shreds = make_shreds(100, 3); // sadece 3/4

        let fec = FecSet {
            slot: 100,
            fec_set_index: 0,
            data_shreds: shreds,
            code_shreds: vec![],
            config,
        };

        assert!(!fec.is_complete()); // eksik
        assert!(!fec.can_recover()); // code yok → 3 + 0 < 4
    }

    // 7b. data + code >= num_data → kurtarma mümkün
    #[test]
    fn fec_set_partial_data_with_codes_can_recover() {
        let config = ErasureConfig::new(4, 2);
        let data_shreds = make_shreds(100, 2); // sadece 2/4 data

        let encoder = ErasureEncoder::new(config.clone());
        let full_shreds = make_shreds(100, 4);
        let code_shreds = encoder.encode(100, 0, &full_shreds);
        let two_codes = code_shreds.into_iter().take(2).collect::<Vec<_>>();

        let fec = FecSet {
            slot: 100,
            fec_set_index: 0,
            data_shreds,
            code_shreds: two_codes,
            config,
        };

        assert!(!fec.is_complete());
        assert!(fec.can_recover(), "2 data + 2 code = 4 >= num_data(4) → kurtarılabilir");
    }

    // 7c. Toplam yetersiz → kurtarılamaz
    #[test]
    fn fec_set_insufficient_total_cannot_recover() {
        let config = ErasureConfig::new(4, 2);
        let data_shreds = make_shreds(100, 1); // sadece 1/4

        let fec = FecSet {
            slot: 100,
            fec_set_index: 0,
            data_shreds,
            code_shreds: vec![],
            config,
        };

        assert!(!fec.can_recover(), "1 + 0 < 4 → kurtarılamaz");
    }

    // 8. ErasureBatchBuilder FEC setleri üretir
    #[test]
    fn batch_builder_produces_fec_sets() {
        let config = ErasureConfig::new(4, 2);
        let builder = ErasureBatchBuilder::new([1u8; 32], config);

        let block_data = vec![0xABu8; 8 * SHRED_DATA_PAYLOAD_SIZE];
        let sets = builder.build_fec_sets(200, &block_data);

        assert!(sets.len() >= 2);
        for set in &sets {
            assert!(!set.code_shreds.is_empty());
        }
    }

    // 9. Parity verisi deterministik
    #[test]
    fn parity_is_deterministic() {
        let config = ErasureConfig::new(4, 2);
        let encoder = ErasureEncoder::new(config);
        let shreds = make_shreds(100, 4);

        let codes1 = encoder.encode(100, 0, &shreds);
        let codes2 = encoder.encode(100, 0, &shreds);

        assert_eq!(codes1[0].parity, codes2[0].parity);
        assert_eq!(codes1[1].parity, codes2[1].parity);
    }

    // 10. Farklı config → farklı sayıda code shred
    #[test]
    fn different_configs_different_code_count() {
        let shreds = make_shreds(100, 4);

        let enc1 = ErasureEncoder::new(ErasureConfig::new(4, 2));
        let enc2 = ErasureEncoder::new(ErasureConfig::new(4, 4));

        let codes1 = enc1.encode(100, 0, &shreds);
        let codes2 = enc2.encode(100, 0, &shreds);

        assert_eq!(codes1.len(), 2);
        assert_eq!(codes2.len(), 4);
    }

    // 11. light_16_8 config doğru
    #[test]
    fn light_config_is_correct() {
        let cfg = ErasureConfig::light_16_8();
        assert_eq!(cfg.num_data, 16);
        assert_eq!(cfg.num_code, 8);
        assert_eq!(cfg.max_erasures(), 8);
    }

    // 12. Boş veri ile encode → boş code
    #[test]
    fn encode_empty_returns_empty() {
        let enc = ErasureEncoder::new(ErasureConfig::default_32_32());
        let codes = enc.encode(1, 0, &[]);
        assert!(codes.is_empty());
    }

    // ── GERÇEk RS KURTARMA TESTLERİ ────────────────────────────────────────────

    // 13. GF(2^8) RS: 2 eksik data shred → 2 code shred ile tam kurtarma
    //
    // Bu test gerçek Reed-Solomon kurtarmanın çalıştığını kanıtlar.
    // XOR tabanlı mock ile imkânsız olan bu test artık geçer.
    #[test]
    fn decoder_recovers_with_loss() {
        let config = ErasureConfig::new(4, 4);
        let encoder = ErasureEncoder::new(config.clone());
        let decoder = ErasureDecoder::new(config);
        let shreds = make_shreds(100, 4);
        let codes = encoder.encode(100, 0, &shreds.clone());

        // Shred indeksi 1 ve 3 kayıp
        let data_opts = vec![
            Some(shreds[0].clone()),
            None,   // kayıp
            Some(shreds[2].clone()),
            None,   // kayıp
        ];

        // 2 code shred ile 2 eksik data kurtarılabilir (RS garantisi)
        let result = decoder.decode(data_opts, &codes[..2], 100).unwrap();

        assert_eq!(result.len(), 4, "tam 4 shred kurtarılmalı");

        // Kurtarılan payload'lar orijinalle eşleşmeli
        assert_eq!(result[0].payload, shreds[0].payload, "shred[0] payload eşleşmeli");
        assert_eq!(result[1].payload, shreds[1].payload, "shred[1] RS ile kurtarılmış payload eşleşmeli");
        assert_eq!(result[2].payload, shreds[2].payload, "shred[2] payload eşleşmeli");
        assert_eq!(result[3].payload, shreds[3].payload, "shred[3] RS ile kurtarılmış payload eşleşmeli");
    }

    // 14. Tüm data kayıp ama code shred'ler yeterli → tam kurtarma
    #[test]
    fn decoder_recovers_all_from_codes() {
        let config = ErasureConfig::new(3, 3);
        let encoder = ErasureEncoder::new(config.clone());
        let decoder = ErasureDecoder::new(config);
        let shreds = make_shreds(200, 3);
        let codes = encoder.encode(200, 0, &shreds.clone());

        // Tüm data kayıp, sadece code shred'ler var
        let data_opts: Vec<Option<Shred>> = vec![None, None, None];

        // 3 code shred → 3 data shred kurtarılabilir (k=3)
        let result = decoder.decode(data_opts, &codes, 200).unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].payload, shreds[0].payload);
        assert_eq!(result[1].payload, shreds[1].payload);
        assert_eq!(result[2].payload, shreds[2].payload);
    }

    // 15. Birden fazla FEC seti tutarlı
    #[test]
    fn multiple_fec_sets_consistent() {
        let config = ErasureConfig::new(4, 2);
        let builder = ErasureBatchBuilder::new([0xAA; 32], config);

        let block_data: Vec<u8> = (0..12 * SHRED_DATA_PAYLOAD_SIZE)
            .map(|i| (i % 251) as u8)
            .collect();

        let sets = builder.build_fec_sets(300, &block_data);
        assert_eq!(sets.len(), 3, "12 shred / 4 per set = 3 FEC seti");

        for set in &sets {
            assert_eq!(set.code_shreds.len(), 2, "Her set 2 code shred içermeli");
            assert!(set.is_complete(), "Her set tüm data shred'leri içermeli");
            assert!(set.can_recover(), "Her tamamlanmış set kurtarılabilir olmalı");
        }
    }

    // 16. RS parity verisi anlamlı (sıfır değil)
    #[test]
    fn parity_is_non_trivial() {
        let config = ErasureConfig::new(4, 4);
        let encoder = ErasureEncoder::new(config);
        let shreds = make_shreds(100, 4);
        let codes = encoder.encode(100, 0, &shreds);

        for code in &codes {
            // Parity sıfır değil (anlamlı veri taşımalı)
            let all_zeros = code.parity.iter().all(|&b| b == 0);
            assert!(!all_zeros, "RS parity sıfır olmamalı");
        }
    }
}
