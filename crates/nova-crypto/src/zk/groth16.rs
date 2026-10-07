//! Groth16 ZK-SNARK (arkworks, BN254 eğrisi).
//!
//! §5.7.3 — ZK-Shield UTXO transfer kanıtları.
//! BN254 (bn128): Ethereum precompile uyumlu → on-chain doğrulama.
//!
//! ## Parametre Seçimi
//! - Eğri: BN254 (128-bit güvenlik, Ethereum uyumlu)
//! - Kanıt boyutu: 192 byte (sabit, r1cs.rs'ye göre)
//! - Doğrulama süresi: ~2ms (pairing tabanlı)
//!
//! ## Referans
//! - penumbra: arkworks + Groth16 (05-penumbra.md)
//! - Ethereum EIP-196, EIP-197 (BN254 precompile)

use ark_bn254::{Bn254, Fr};
use ark_groth16::{Groth16, PreparedVerifyingKey, Proof, ProvingKey, VerifyingKey};
use ark_relations::r1cs::ConstraintSynthesizer;
use ark_snark::SNARK;
use ark_std::rand::SeedableRng;

use crate::errors::{CryptoError, CryptoResult};

/// Groth16 kanıtı — BN254.
pub type NovaGroth16Proof = Proof<Bn254>;

/// Proving key (büyük, offline üretilir).
pub type NovaProvingKey = ProvingKey<Bn254>;

/// Verifying key (küçük, on-chain saklanır).
pub type NovaVerifyingKey = VerifyingKey<Bn254>;

/// Hazır verifying key (doğrulama için optimize edilmiş).
pub type NovaPreparedVk = PreparedVerifyingKey<Bn254>;

/// Groth16 setup — proving/verifying key çifti üret.
///
/// `circuit`: Boş devre (sadece yapı, değer yok)
pub fn groth16_setup<C: ConstraintSynthesizer<Fr>>(
    circuit: C,
) -> CryptoResult<(NovaProvingKey, NovaVerifyingKey)> {
    let mut rng = rand::rngs::StdRng::from_entropy();
    Groth16::<Bn254>::circuit_specific_setup(circuit, &mut rng)
        .map_err(|e| CryptoError::ZkProve(format!("Groth16 setup hatası: {e:?}")))
}

/// Groth16 kanıt oluştur.
///
/// `pk`: Proving key
/// `circuit`: Witness dahil devre
pub fn groth16_prove<C: ConstraintSynthesizer<Fr>>(
    pk: &NovaProvingKey,
    circuit: C,
) -> CryptoResult<NovaGroth16Proof> {
    let mut rng = rand::rngs::StdRng::from_entropy();
    Groth16::<Bn254>::prove(pk, circuit, &mut rng)
        .map_err(|e| CryptoError::ZkProve(format!("Groth16 prove hatası: {e:?}")))
}

/// Groth16 kanıt doğrula.
///
/// `vk`: Verifying key
/// `proof`: Kanıt
/// `public_inputs`: Kamuya açık girdiler
pub fn groth16_verify(
    vk: &NovaPreparedVk,
    proof: &NovaGroth16Proof,
    public_inputs: &[Fr],
) -> CryptoResult<()> {
    let valid = Groth16::<Bn254>::verify_with_processed_vk(vk, public_inputs, proof)
        .map_err(|e| CryptoError::ZkVerify(format!("Groth16 verify hatası: {e:?}")))?;

    if valid {
        Ok(())
    } else {
        Err(CryptoError::ZkVerify("Groth16 kanıt geçersiz".into()))
    }
}

/// Verifying key'i "prepared" forma dönüştür (hızlı doğrulama için).
pub fn prepare_verifying_key(vk: &NovaVerifyingKey) -> NovaPreparedVk {
    ark_groth16::prepare_verifying_key(vk)
}
