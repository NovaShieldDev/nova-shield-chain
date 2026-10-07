//! Nova Shield Chain — Protokol Sabitleri
//!
//! WHITEPAPER.md'den alınan tüm numerik sabitler burada toplanmıştır.
//! Bir sabit değiştirilmek istenirse yalnızca bu dosya güncellenir.

// ─── ZAMAN SABİTLERİ ──────────────────────────────────────────────────────────

/// Tek bir slot süresi (milisaniye). §5.10
/// 400ms = 2.5 slot/saniye → saniyede 2.5 blok üretme kapasitesi.
/// Referans: Avalanche Simplex BFT MaxNetworkDelay=5s, bizde 400ms slot.
pub const SLOT_DURATION_MS: u64 = 400;

/// Bir epoch'taki slot sayısı. §5.10
/// 216_000 slot × 400ms = 86_400_000ms = 24 saat (1 epoch = 1 gün).
pub const EPOCH_DURATION_SLOTS: u64 = 216_000;

/// Epoch süresi milisaniye cinsinden.
pub const EPOCH_DURATION_MS: u64 = EPOCH_DURATION_SLOTS * SLOT_DURATION_MS;

/// Şifreli mempool commit penceresi (ms). §5.2
/// Bir slot'un ilk 300ms'sinde TX commitment'lar toplanır.
pub const MEMPOOL_COMMIT_WINDOW_MS: u64 = 300;

/// Threshold decryption reveal penceresi (ms). §5.2
/// Son 100ms'de toplu açma (reveal) yapılır.
pub const MEMPOOL_REVEAL_WINDOW_MS: u64 = 100;

/// TX iletiminde maksimum rastgele gecikme (ms). §6.4.1
/// Timing analizi saldırılarını engeller.
pub const MAX_RANDOM_DELAY_MS: u64 = 200;

// ─── VALİDATÖR / STAKE SABİTLERİ ─────────────────────────────────────────────

/// Maksimum aktif validator sayısı (hedef). §5.10
pub const MAX_VALIDATORS: u64 = 1_000;

/// Başlangıç validator sayısı (genesis). §5.10
pub const INITIAL_VALIDATORS: u64 = 200;

/// Minimum validator stake miktarı (NOVA cinsinden, 10^18 baz). §5.10
/// 100_000 NOVA × 10^18
pub const MIN_VALIDATOR_STAKE: u128 = 100_000 * NOVA_DECIMALS_FACTOR;

/// NOVA token'ının ondalık hassasiyeti (18 hane, Ethereum uyumlu).
pub const NOVA_DECIMALS: u8 = 18;

/// 1 NOVA = 10^18 baz birim.
pub const NOVA_DECIMALS_FACTOR: u128 = 1_000_000_000_000_000_000u128;

/// Toplam NOVA arzı. §5.11
/// 200_000_000 NOVA × 10^18 (deflasyonlu sabit arz, %50 fee burn)
pub const TOTAL_NOVA_SUPPLY: u128 = 200_000_000 * NOVA_DECIMALS_FACTOR;

/// Unbonding süresi (epoch sayısı). §5.11
/// 21 epoch = 21 gün. Stake çekilmesi 21 gün sonra tamamlanır.
pub const UNBONDING_PERIOD_EPOCHS: u64 = 21;

// ─── TOKENOMİKS DAĞILIM SABİTLERİ ────────────────────────────────────────────

/// TGE'de dolaşıma giren NOVA miktarı. §10.2
/// Public Sale + Liquidity = 14M + 14M = 28M NOVA
pub const TGE_CIRCULATING_SUPPLY: u128 = 28_000_000 * NOVA_DECIMALS_FACTOR;

/// Ecosystem & Community dağılımı (%22 = 44M NOVA). §10.2
/// 48 ay linear vesting, cliff yok.
pub const ALLOC_ECOSYSTEM: u128 = 44_000_000 * NOVA_DECIMALS_FACTOR;

/// Team & Founders dağılımı (%19 = 38M NOVA). §10.2
/// 12 ay cliff + 48 ay linear vesting.
pub const ALLOC_TEAM: u128 = 38_000_000 * NOVA_DECIMALS_FACTOR;

/// Investors dağılımı (%17 = 34M NOVA). §10.2
/// 12 ay cliff + 24 ay linear vesting. Tüm turlarda max %17 cap.
pub const ALLOC_INVESTORS: u128 = 34_000_000 * NOVA_DECIMALS_FACTOR;

/// Staking Rewards havuzu (%16 = 32M NOVA). §10.3
/// Yarılanma takvimi: 10 yılda dağıtılır.
pub const ALLOC_STAKING_REWARDS: u128 = 32_000_000 * NOVA_DECIMALS_FACTOR;

/// Treasury (%12 = 24M NOVA). §10.2
/// 6 ay cliff + governance vote ile unlock.
pub const ALLOC_TREASURY: u128 = 24_000_000 * NOVA_DECIMALS_FACTOR;

/// Public Sale (%7 = 14M NOVA). §10.2
/// TGE'de tamamen açık — TGE dolaşımının yarısı.
pub const ALLOC_PUBLIC_SALE: u128 = 14_000_000 * NOVA_DECIMALS_FACTOR;

/// Liquidity (%7 = 14M NOVA). §10.2
/// TGE'de tamamen açık — DEX/CEX likidite sağlama.
pub const ALLOC_LIQUIDITY: u128 = 14_000_000 * NOVA_DECIMALS_FACTOR;

/// Fee burn oranı (BPS). §10.4
/// Her işlem ücretinin %50'si kalıcı olarak yakılır → deflasyonlu model.
pub const FEE_BURN_BPS: u32 = 5_000;

/// Staking yıl 1-2 APY (BPS). §10.3
pub const STAKING_APY_YEAR_1_2_BPS: u32 = 800;

/// Staking yıl 3-4 APY (BPS). §10.3
pub const STAKING_APY_YEAR_3_4_BPS: u32 = 400;

/// Staking yıl 5-6 APY (BPS). §10.3
pub const STAKING_APY_YEAR_5_6_BPS: u32 = 200;

/// Staking yıl 7-10 APY (BPS). §10.3
pub const STAKING_APY_YEAR_7_10_BPS: u32 = 100;

// ─── BFT KONSENSÜS SABİTLERİ ─────────────────────────────────────────────────

/// BFT güvenlik eşiği pay: 2/3 quorum. §5.10
/// Sisteme dayanıklı olduğu maksimum faulty node oranı: f < n/3.
pub const BFT_QUORUM_NUMERATOR: u64 = 2;

/// BFT güvenlik eşiği payda: 2/3 quorum. §5.10
pub const BFT_QUORUM_DENOMINATOR: u64 = 3;

/// Simplex BFT tur sayısı: 2 tur (propose + vote). §5.10
/// Referans: Avalanche snow/consensus/simplex, avalanchego analizi.
pub const SIMPLEX_BFT_ROUNDS: u8 = 2;

/// Maksimum ağ gecikmesi toleransı (ms). §5.10
/// Simplex BFT'nin güvenlik garantisi için MaxNetworkDelay varsayımı.
pub const MAX_NETWORK_DELAY_MS: u64 = 5_000;

// ─── MPC / THRESHOLD SABİTLERİ ───────────────────────────────────────────────

/// Varsayılan hesap threshold'u (T): 3/5. §5.1.3
/// Bir TX imzalamak için 5 guardian'dan en az 3'ünün onayı gerekir.
pub const DEFAULT_THRESHOLD_T: u8 = 3;

/// Varsayılan hesap guardian sayısı (N): 3/5. §5.1.3
pub const DEFAULT_THRESHOLD_N: u8 = 5;

/// Minimum threshold değeri (T). §5.1.3
pub const MIN_THRESHOLD_T: u8 = 2;

/// Minimum guardian sayısı (N). §5.1.3
pub const MIN_THRESHOLD_N: u8 = 3;

/// Maksimum guardian sayısı per hesap. §5.1.3
pub const MAX_GUARDIANS_PER_ACCOUNT: u8 = 15;

/// Maksimum threshold değeri (T): 10/15. §5.1.3
pub const MAX_THRESHOLD_T: u8 = 10;

/// Maksimum guardian sayısı (N): 10/15. §5.1.3
pub const MAX_THRESHOLD_N: u8 = 15;

/// CGGMP21 pre-sign oturumu başına maksimum pre-imza sayısı. §5.9.2
pub const MAX_PRESIGNATURES_PER_SESSION: u32 = 100;

// ─── GUARDIAN DEĞİŞİKLİK PENCERELERİ ─────────────────────────────────────────

/// Guardian atama/değiştirme için bekleme süresi (saniye). §5.1.5
/// 7 gün = 604_800 saniye. Ani guardian değişikliği saldırısını engeller.
pub const GUARDIAN_CHANGE_WINDOW_SECS: u64 = 7 * 24 * 3600;

/// Kurtarma onay penceresi (saniye). §5.1.4
/// 48 saat içinde hesap sahibi itiraz edebilir.
pub const RECOVERY_WINDOW_SECS: u64 = 48 * 3600;

// ─── MİRAS (DEADMAN SWITCH) SABİTLERİ ────────────────────────────────────────

/// Varsayılan inaktivite eşiği — tetikleyici başlat (saniye). §5.4.2
/// 365 gün = 1 yıl hareketsizlik → miras süreci başlar.
pub const DEFAULT_INACTIVITY_TRIGGER_SECS: u64 = 365 * 24 * 3600;

/// Miras araştırma penceresi — son inceleme (saniye). §5.4.5
/// 72 saat içinde hesap sahibi itiraz edebilir.
pub const INHERITANCE_INVESTIGATION_WINDOW_SECS: u64 = 72 * 3600;

/// Miras için minimum varis sayısı. §5.4
pub const MIN_HEIRS: u8 = 1;

/// Miras için maksimum varis sayısı. §5.4
pub const MAX_HEIRS: u8 = 20;

// ─── EMERGENCY FREEZE SABİTLERİ ──────────────────────────────────────────────

/// Freeze araştırma penceresi (saniye). §5.5
/// 72 saat. Hesap sahibi bu süre içinde freeze'e itiraz edebilir.
pub const FREEZE_INVESTIGATION_WINDOW_SECS: u64 = 72 * 3600;

/// Anomali tespiti için minimum işlem sayısı. §5.5.2
pub const ANOMALY_DETECTION_MIN_TX: u32 = 10;

/// Coğrafi anomali eşiği: farklı kıtadan işlem % oranı. §5.5.2
pub const GEO_ANOMALY_THRESHOLD_PCT: u8 = 80;

// ─── SLASHING SABİTLERİ ───────────────────────────────────────────────────────

/// Çift imza (double-sign) slashing oranı: %100. §5.10 (§6.5.2)
/// Referans: Polkadot SLASH_DOUBLE_SIGN=100%.
pub const SLASH_DOUBLE_SIGN_BPS: u64 = 10_000; // 10_000 bps = %100

/// Mempool sızıntısı (şifreli TX ifşa) slashing oranı: %50. §5.2.4
pub const SLASH_MEMPOOL_LEAK_BPS: u64 = 5_000; // %50

/// Yanlış clearing price slashing oranı: %25. §5.3.5
pub const SLASH_WRONG_BATCH_BPS: u64 = 2_500; // %25

/// Offline validator günlük slashing oranı: %0.1. §5.10
pub const SLASH_OFFLINE_DAILY_BPS: u64 = 10; // %0.1

/// Slashing değerlerinde baz puan faktörü (1 BPS = %0.01).
pub const BPS_FACTOR: u64 = 10_000;

// ─── GÜVENLİK PADDING ────────────────────────────────────────────────────────

/// TX şifrelemede sabit padding boyutu (byte). §6.4.2
/// Tüm TX'ler şifrelemeden önce bu boyuta doldurulur.
/// Boyut farkından dolaylı bilgi sızmasını engeller.
pub const MAX_ENCRYPTION_PADDING_BYTES: usize = 512;

/// Şifrelenmiş TX maksimum boyutu (byte). §5.2.1
pub const MAX_ENCRYPTED_TX_BYTES: usize = 4096;

// ─── ZK-SHIELD SABİTLERİ ──────────────────────────────────────────────────────

/// ZK-Shield UTXO'larında minimum transfer miktarı (baz birim). §5.7
pub const ZK_MIN_TRANSFER_AMOUNT: u128 = NOVA_DECIMALS_FACTOR / 1000; // 0.001 NOVA

/// ZK proof'un geçerli olduğu maksimum blok sayısı. §5.7.3
pub const ZK_PROOF_VALIDITY_BLOCKS: u64 = 100;

// ─── GUARDIAN MARKETPLACE SABİTLERİ ──────────────────────────────────────────

/// Guardian hizmet ücreti maksimum oranı (BPS). §5.8.4
/// Guardian'lar korunan varlığın %2'sine kadar yıllık ücret alabilir.
pub const GUARDIAN_MAX_FEE_BPS: u64 = 200; // %2

/// Guardian minimum trust score (100 üzerinden). §5.8.3
pub const GUARDIAN_MIN_TRUST_SCORE: u8 = 60;

/// Trust score: çalışma süresi ağırlığı (40/100). §5.8.3
pub const TRUST_WEIGHT_UPTIME: u8 = 40;

/// Trust score: geçmiş performans ağırlığı (30/100). §5.8.3
pub const TRUST_WEIGHT_HISTORY: u8 = 30;

/// Trust score: stake miktarı ağırlığı (20/100). §5.8.3
pub const TRUST_WEIGHT_STAKE: u8 = 20;

/// Trust score: yanıt hızı ağırlığı (10/100). §5.8.3
pub const TRUST_WEIGHT_RESPONSE: u8 = 10;

// ─── AĞ SABİTLERİ ────────────────────────────────────────────────────────────

/// Nova Shield Chain ID (mainnet). §5.12
pub const MAINNET_CHAIN_ID: u64 = 1984;

/// Nova Shield testnet Chain ID.
pub const TESTNET_CHAIN_ID: u64 = 19840;

/// Nova Shield devnet Chain ID.
pub const DEVNET_CHAIN_ID: u64 = 198400;

/// Gossip protokolü maksimum mesaj boyutu (byte). §5.12
/// Referans: Solana CRDS MAX_CRDS_OBJECT_SIZE=928, bizde daha geniş.
pub const GOSSIP_MAX_MESSAGE_BYTES: usize = 4096;

/// P2P bağlantı başına maksimum eş sayısı.
pub const MAX_PEERS: usize = 50;

/// Minimum bağlı peer sayısı (altına düşerse uyarı).
pub const MIN_PEERS: usize = 3;

// ─── BLOK SABİTLERİ ──────────────────────────────────────────────────────────

/// Blok başlığı maksimum boyutu (byte).
pub const MAX_BLOCK_HEADER_BYTES: usize = 1024;

/// Blok gövdesi maksimum boyutu (byte). §5.10
/// ~4MB — Solana ~1.2MB'a göre daha geniş (şifreli commit overhead).
pub const MAX_BLOCK_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Slot başına maksimum TX sayısı. §5.3
pub const MAX_TX_PER_SLOT: u32 = 10_000;

/// Blok hash boyutu (BLAKE3 = 32 byte).
pub const BLOCK_HASH_SIZE: usize = 32;

/// Adres boyutu (byte). Ethereum uyumlu: 20 byte.
pub const ADDRESS_SIZE: usize = 20;

/// MPC public key boyutu (secp256k1 compressed): 33 byte.
pub const MPC_PUBKEY_SIZE: usize = 33;

/// BLS public key boyutu (G1 compressed, BLS12-381): 48 byte.
pub const BLS_PUBKEY_SIZE: usize = 48;

/// BLS imza boyutu (G2 compressed, BLS12-381): 96 byte.
pub const BLS_SIGNATURE_SIZE: usize = 96;

// ─── EVM UYUMLULUK ────────────────────────────────────────────────────────────

/// EVM magic number (blok yapısında Ethereum uyumluluğu için).
pub const EVM_MAGIC: u64 = 0x4e6f7661; // "Nova" ASCII

/// EVM chain ID (Ethereum uyumlu RPC için). §9.2
pub const EVM_CHAIN_ID: u64 = MAINNET_CHAIN_ID;

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenomics_total_supply_equals_sum_of_allocations() {
        // Tüm kategoriler toplamı TOTAL_NOVA_SUPPLY'ye eşit olmalı
        let sum = ALLOC_ECOSYSTEM
            + ALLOC_TEAM
            + ALLOC_INVESTORS
            + ALLOC_STAKING_REWARDS
            + ALLOC_TREASURY
            + ALLOC_PUBLIC_SALE
            + ALLOC_LIQUIDITY;
        assert_eq!(
            sum, TOTAL_NOVA_SUPPLY,
            "tokenomics dağılım toplamı ({sum}) != toplam arz ({TOTAL_NOVA_SUPPLY})"
        );
    }

    #[test]
    fn test_tge_circulating_equals_public_sale_plus_liquidity() {
        assert_eq!(TGE_CIRCULATING_SUPPLY, ALLOC_PUBLIC_SALE + ALLOC_LIQUIDITY);
    }

    #[test]
    fn test_chain_ids_unique() {
        assert_ne!(MAINNET_CHAIN_ID, TESTNET_CHAIN_ID);
        assert_ne!(MAINNET_CHAIN_ID, DEVNET_CHAIN_ID);
        assert_ne!(TESTNET_CHAIN_ID, DEVNET_CHAIN_ID);
        assert_eq!(MAINNET_CHAIN_ID, 1984);
        assert_eq!(TESTNET_CHAIN_ID, 19840);
        assert_eq!(DEVNET_CHAIN_ID, 198400);
    }

    #[test]
    fn test_min_validator_stake() {
        // 100_000 NOVA (whitepaper §5.10)
        assert_eq!(MIN_VALIDATOR_STAKE, 100_000 * NOVA_DECIMALS_FACTOR);
    }

    #[test]
    fn test_fee_burn_bps_is_50_percent() {
        assert_eq!(FEE_BURN_BPS, 5_000); // %50
    }

    #[test]
    fn test_nova_decimals_factor() {
        assert_eq!(NOVA_DECIMALS_FACTOR, 1_000_000_000_000_000_000u128);
        assert_eq!(NOVA_DECIMALS, 18);
    }
}
