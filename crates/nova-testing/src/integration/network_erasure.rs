//! nova-network turbine Reed-Solomon entegrasyon testi
//!
//! ## Test Akışı (Solana Turbine §5.12)
//! 1. ErasureEncoder ile data shredleri kodla → code shredler üret
//! 2. Bazı shredleri "kaybet" (ağ paketi kaybı simülasyonu)
//! 3. ErasureDecoder ile RS GF(2^8) kurtarma
//! 4. Orijinal veriyle bit-exact eşleşme doğrula
//!
//! ## Araştırma Kaynakları
//! - Solana Turbine (agent-09.md): FEC set, shred pipeline, 32+32 config
//! - RS GF(2^8): reed-solomon-erasure crate, §3.4 erasure coding

use nova_network::turbine::erasure::{
    ErasureConfig, ErasureDecoder, ErasureEncoder, FecSet,
};
use nova_network::turbine::shred::{Shred, ShredType};

// ─── Yardımcılar ──────────────────────────────────────────────────────────────

fn leader() -> [u8; 32] {
    [0u8; 32]
}

fn make_data_shred(slot: u64, index: u32, data: Vec<u8>) -> Shred {
    Shred::new(slot, index, ShredType::Data, leader(), data)
}

fn make_data_shreds(slot: u64, num_data: usize) -> Vec<Shred> {
    (0..num_data)
        .map(|i| {
            let payload = format!("block_data_shred_{:04}_{}", i, slot)
                .into_bytes()
                .into_iter()
                .chain(std::iter::repeat(0u8))
                .take(1024) // 1KB payload
                .collect();
            make_data_shred(slot, i as u32, payload)
        })
        .collect()
}

// ─── Testler ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Temel encode/decode döngüsü — kayıp yok
    #[test]
    fn erasure_encode_decode_no_loss() {
        let config = ErasureConfig::light_16_8(); // 16 data, 8 code
        let encoder = ErasureEncoder::new(config.clone());
        let decoder = ErasureDecoder::new(config.clone());

        let slot = 1000u64;
        let data_shreds = make_data_shreds(slot, 16);

        // Encode
        let code_shreds = encoder.encode(slot, 0, &data_shreds);
        assert_eq!(code_shreds.len(), 8, "16 data → 8 code shred");

        // Kayıp yok — tüm data shred mevcut
        let data_options: Vec<Option<Shred>> = data_shreds.iter().map(|s| Some(s.clone())).collect();
        let recovered = decoder.decode(data_options, &code_shreds, slot)
            .expect("Decode başarılı olmalı");

        assert_eq!(recovered.len(), 16, "16 data shred kurtarılmalı");

        // Bit-exact eşleşme
        for orig in &data_shreds {
            let found = recovered.iter().find(|r| r.id.index == orig.id.index);
            assert!(found.is_some(), "Shred {} mevcut olmalı", orig.id.index);
            assert_eq!(
                found.unwrap().payload, orig.payload,
                "Shred {} verileri eşleşmeli", orig.id.index
            );
        }
    }

    /// Kayıp ile kurtarma — 4 data shred kaybı (8 code = 8 erasure toleransı)
    #[test]
    fn erasure_decode_with_data_loss() {
        let config = ErasureConfig::light_16_8(); // 16 data, 8 code — max 8 erasure
        let encoder = ErasureEncoder::new(config.clone());
        let decoder = ErasureDecoder::new(config.clone());

        let slot = 2000u64;
        let data_shreds = make_data_shreds(slot, 16);
        let code_shreds = encoder.encode(slot, 0, &data_shreds);

        // 4 data shred kaybet (index 0, 3, 7, 12)
        let lost_indices = [0usize, 3, 7, 12];
        let data_options: Vec<Option<Shred>> = data_shreds.iter().enumerate()
            .map(|(i, s)| if lost_indices.contains(&i) { None } else { Some(s.clone()) })
            .collect();

        assert_eq!(
            data_options.iter().filter(|s| s.is_some()).count(),
            12,
            "12 shred hayatta"
        );

        let recovered = decoder.decode(data_options, &code_shreds, slot)
            .expect("4 kayıp RS ile kurtarılabilmeli");

        assert_eq!(recovered.len(), 16, "Tüm 16 shred kurtarılmalı");

        for orig in &data_shreds {
            let found = recovered.iter().find(|r| r.id.index == orig.id.index);
            assert!(found.is_some(), "Shred {} kurtarılmalı", orig.id.index);
        }
    }

    /// Maksimum erasure toleransı — 8/8 code shred, 8 data kayıp
    #[test]
    fn erasure_maximum_tolerance() {
        let config = ErasureConfig::light_16_8();
        let encoder = ErasureEncoder::new(config.clone());
        let decoder = ErasureDecoder::new(config.clone());

        let slot = 3000u64;
        let data_shreds = make_data_shreds(slot, 16);
        let code_shreds = encoder.encode(slot, 0, &data_shreds);

        // İlk 8 data shred kaybet → tam erasure limiti
        let data_options: Vec<Option<Shred>> = data_shreds.iter().enumerate()
            .map(|(i, s)| if i < 8 { None } else { Some(s.clone()) })
            .collect();

        let recovered = decoder.decode(data_options, &code_shreds, slot)
            .expect("8 kayıp = maksimum tolerans, kurtarılmalı");

        assert_eq!(recovered.len(), 16, "16 data shred kurtarılmalı");
    }

    /// Tolere edilemeyen kayıp: 9 kayıp ile 8 code → kurtarılamaz
    #[test]
    fn erasure_beyond_tolerance_fails() {
        let config = ErasureConfig::light_16_8();
        let encoder = ErasureEncoder::new(config.clone());
        let decoder = ErasureDecoder::new(config.clone());

        let slot = 4000u64;
        let data_shreds = make_data_shreds(slot, 16);
        let code_shreds = encoder.encode(slot, 0, &data_shreds);

        // Sadece 7 data shred kalsın (9 kayıp — 8'den fazla)
        let data_options: Vec<Option<Shred>> = data_shreds.iter().enumerate()
            .map(|(i, s)| if i < 9 { None } else { Some(s.clone()) })
            .collect();

        let result = decoder.decode(data_options, &code_shreds, slot);
        assert!(result.is_err(), "9 kayıp ile kurtarma imkansız olmalı");
    }

    /// Parity (code) shredleri sıfır değil — gerçek RS hesabı
    #[test]
    fn erasure_code_shreds_are_non_trivial() {
        let config = ErasureConfig::default_32_32();
        let encoder = ErasureEncoder::new(config.clone());

        let slot = 5000u64;
        let data_shreds = make_data_shreds(slot, 32);

        let code_shreds = encoder.encode(slot, 0, &data_shreds);
        assert_eq!(code_shreds.len(), 32, "32 code shred üretilmeli");

        // Her code shred sıfır olmayan parity verisi içermeli
        for cs in &code_shreds {
            assert!(!cs.parity.iter().all(|&b| b == 0), "Code shred trivial olamaz");
        }

        // Farklı data → farklı code
        let other_shreds = make_data_shreds(slot + 1, 32);
        let other_codes = encoder.encode(slot + 1, 0, &other_shreds);
        assert_ne!(
            code_shreds[0].parity,
            other_codes[0].parity,
            "Farklı data farklı code üretmeli"
        );
    }

    /// FecSet can_recover / is_complete mantığı
    #[test]
    fn fec_set_status_flags() {
        let config = ErasureConfig::light_16_8();
        let encoder = ErasureEncoder::new(config.clone());

        let slot = 6000u64;
        let data_shreds = make_data_shreds(slot, 16);

        // Başlangıçta: data yok, code yok → kurtarılamaz
        let mut fec_set = FecSet {
            slot,
            fec_set_index: 0,
            data_shreds: vec![],
            code_shreds: vec![],
            config: config.clone(),
        };

        assert!(!fec_set.can_recover(), "Shred olmadan kurtarılamaz");
        assert!(!fec_set.is_complete(), "Shred olmadan complete değil");

        // Tüm data + code shredler eklendi
        let code_shreds = encoder.encode(slot, 0, &data_shreds);
        fec_set.data_shreds = data_shreds;
        fec_set.code_shreds = code_shreds;

        assert!(fec_set.is_complete(), "Tüm data shredler mevcut → complete");
        assert!(fec_set.can_recover(), "Yeterli shred mevcut → kurtarılabilir");
    }

    /// Config recovery ratio doğrulaması
    #[test]
    fn erasure_config_recovery_ratio() {
        let c1 = ErasureConfig::light_16_8();
        // 16 data + 8 code → max 8 erasure → ratio = 8/24 ≈ 0.333
        assert!(c1.recovery_ratio() > 0.3 && c1.recovery_ratio() < 0.4);
        assert_eq!(c1.max_erasures(), 8);
        assert_eq!(c1.total(), 24);

        let c2 = ErasureConfig::default_32_32();
        // 32 data + 32 code → ratio = 0.5
        assert!((c2.recovery_ratio() - 0.5).abs() < 0.01);
        assert_eq!(c2.max_erasures(), 32);
    }
}
