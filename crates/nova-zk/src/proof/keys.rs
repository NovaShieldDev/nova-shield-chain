//! CircuitKeys — Groth16 proving ve verifying key yönetimi
//!
//! Her devre tipi için bir kez `circuit_specific_setup` çalıştırılır.
//! Sonuç `OnceLock` ile önbelleğe alınır: iş parçacığı güvenli, tembel başlatma.
//!
//! Production'da: MPC tabanlı trusted setup ceremony sonuçları dosyadan yüklenir.
//! Geliştirme/test'te: deterministik seed'li RNG ile üretilir.
//!
//! UYARI: Geliştirme keylerini production'da KULLANMAYIN.

use ark_bls12_381::Bls12_381;
use ark_groth16::{Groth16, PreparedVerifyingKey, ProvingKey};
use ark_snark::SNARK;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::sync::OnceLock;

use crate::errors::{ZkError, ZkResult};
use crate::proof::params::CircuitType;

// ─── CircuitKeys ──────────────────────────────────────────────────────────────

/// Bir devre tipi için proving + verifying key çifti
pub struct CircuitKeys {
    /// Groth16 proving key (kanıt üretimi)
    pub pk: ProvingKey<Bls12_381>,
    /// Groth16 verifying key (kanıt doğrulama, hazırlanmış)
    pub pvk: PreparedVerifyingKey<Bls12_381>,
}

impl CircuitKeys {
    /// Verilen devre için Groth16 setup çalıştır
    fn generate<C: ark_relations::r1cs::ConstraintSynthesizer<ark_bls12_381::Fr>>(
        circuit: C,
        seed: u64,
    ) -> ZkResult<Self> {
        let mut rng = StdRng::seed_from_u64(seed);
        let (pk, vk) = Groth16::<Bls12_381>::circuit_specific_setup(circuit, &mut rng)
            .map_err(|e| ZkError::ProofGenerationFailed(e.to_string()))?;
        let pvk = Groth16::<Bls12_381>::process_vk(&vk)
            .map_err(|e| ZkError::ProofGenerationFailed(e.to_string()))?;
        Ok(Self { pk, pvk })
    }
}

// ─── Seed sabitleri (her devre için farklı) ────────────────────────────────────

const SEED_MINT:       u64 = 0x4E4F56415F4D494E;
const SEED_SPEND:      u64 = 0x4E4F56415F535045;
const SEED_TRANSFER:   u64 = 0x4E4F56415F545241;
const SEED_BURN:       u64 = 0x4E4F56415F425552;
const SEED_STAKE:      u64 = 0x4E4F56415F535441;
const SEED_UNSTAKE:    u64 = 0x4E4F56415F554E53;
const SEED_GOVERNANCE: u64 = 0x4E4F56415F474F56;
const SEED_NULLIFIER:  u64 = 0x4E4F56415F4E554C;

// ─── Tembel (lazy) global key deposu ─────────────────────────────────────────

struct AllKeys {
    mint:       CircuitKeys,
    spend:      CircuitKeys,
    transfer:   CircuitKeys,
    burn:       CircuitKeys,
    stake:      CircuitKeys,
    unstake:    CircuitKeys,
    governance: CircuitKeys,
    nullifier:  CircuitKeys,
}

// OnceLock<Result<...>> — setup başarısız olursa hata String olarak saklanır.
// ZkError'ın Sync olması gerekiyor; String ile sarmak bu sorunu çözer.
static ALL_KEYS: OnceLock<Result<AllKeys, String>> = OnceLock::new();

/// Tüm devreler için setup — ilk çağrıda çalışır, sonraki çağrılar cache'ten alır.
///
/// UYARI: İlk çağrıda yavaş olabilir (her devre için Groth16 setup).
/// Test ortamında `#[ignore]` attribute'u ile işaretleyin.
///
/// # Production Güvenlik Notu
/// `dev-keys` feature aktif değilse bu fonksiyon `Err(ProductionSetupRequired)` döner.
/// Production'da gerçek MPC ceremony keylerini `load_production_keys()` ile yükleyin.
fn all_keys() -> ZkResult<&'static AllKeys> {
    // Production guard: dev-keys feature'ı kapalıysa deterministik keyler kullanılamaz
    #[cfg(not(feature = "dev-keys"))]
    {
        return Err(ZkError::ProductionSetupRequired);
    }

    #[cfg(feature = "dev-keys")]
    {
        let result = ALL_KEYS.get_or_init(|| {
            init_all_keys().map_err(|e| e.to_string())
        });
        result.as_ref().map_err(|s| ZkError::ProofGenerationFailed(s.clone()))
    }
}

fn init_all_keys() -> ZkResult<AllKeys> {
    use crate::circuits::r1cs::*;

    Ok(AllKeys {
        mint:       CircuitKeys::generate(MintCircuit::setup_dummy(), SEED_MINT)?,
        spend:      CircuitKeys::generate(SpendCircuit::setup_dummy(), SEED_SPEND)?,
        transfer:   CircuitKeys::generate(TransferCircuit::setup_dummy(), SEED_TRANSFER)?,
        burn:       CircuitKeys::generate(BurnCircuit::setup_dummy(), SEED_BURN)?,
        stake:      CircuitKeys::generate(StakeCircuit::setup_dummy(), SEED_STAKE)?,
        unstake:    CircuitKeys::generate(UnstakeCircuit::setup_dummy(), SEED_UNSTAKE)?,
        governance: CircuitKeys::generate(GovernanceVoteCircuit::setup_dummy(), SEED_GOVERNANCE)?,
        nullifier:  CircuitKeys::generate(NullifierDerivationCircuit::setup_dummy(), SEED_NULLIFIER)?,
    })
}

/// Verilen devre tipi için key çiftini al
pub fn get_keys(circuit_type: CircuitType) -> ZkResult<&'static CircuitKeys> {
    let keys = all_keys()?;
    Ok(match circuit_type {
        CircuitType::Mint                 => &keys.mint,
        CircuitType::Spend                => &keys.spend,
        CircuitType::Transfer             => &keys.transfer,
        CircuitType::Burn                 => &keys.burn,
        CircuitType::Stake                => &keys.stake,
        CircuitType::Unstake              => &keys.unstake,
        CircuitType::GovernanceVote       => &keys.governance,
        CircuitType::NullifierDerivation  => &keys.nullifier,
    })
}

/// Verilen devre tipi için key'lerin hazır olup olmadığını test et (setup'ı tetikler)
pub fn warm_up(circuit_type: CircuitType) -> ZkResult<()> {
    get_keys(circuit_type).map(|_| ())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// MintCircuit için key üretimi — yavaş test, CI'da isteğe bağlı
    #[test]
    #[ignore = "Groth16 setup yavaş (~birkaç saniye), sadece manual çalıştır"]
    fn test_mint_keys_generated() {
        let keys = get_keys(CircuitType::Mint).unwrap();
        // ProvingKey boyutu > 0 olmalı
        assert!(!keys.pk.a_query.is_empty(), "Mint PK boş olamaz");
    }

    #[test]
    #[ignore = "Groth16 setup yavaş, sadece manual çalıştır"]
    fn test_nullifier_keys_generated() {
        let keys = get_keys(CircuitType::NullifierDerivation).unwrap();
        assert!(!keys.pk.a_query.is_empty());
    }

    #[test]
    #[ignore = "Tüm devreler için setup — çok yavaş"]
    fn test_all_keys_generated() {
        for ct in [
            CircuitType::Mint,
            CircuitType::Spend,
            CircuitType::Transfer,
            CircuitType::Burn,
            CircuitType::Stake,
            CircuitType::Unstake,
            CircuitType::GovernanceVote,
            CircuitType::NullifierDerivation,
        ] {
            warm_up(ct).unwrap();
            println!("{:?} keys hazır", ct);
        }
    }
}
