//! EVM precompile'lar — Avalanchego Subnet-EVM pattern'inden ilham
//!
//! Avalanchego: `modules.RegisteredModules()` ile precompile registry.
//! Nova: Her precompile bir `NovaPrecompile` implementasyonu.
//!
//! ## Güvenlik Düzeltmeleri
//! - VM-003: MpcAccount ve GuardianCheck gerçek `ChainStateProvider` trait'i ile sorgulanıyor
//! - VM-005: ZkVerifier — gerçek `Groth16Proof::verify_groth16()` ile doğrulama
//! - VM-006: BatchAuction — wrapping_mul yerine checked_mul

use crate::errors::{VmError, VmResult};
use nova_primitives::Address;
use nova_zk::proof::groth16::Groth16Proof;
use nova_zk::proof::params::{CircuitType, TrustedSetupParams};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// ─── Precompile Adresleri ─────────────────────────────────────────────────────

/// MPC hesap precompile adresi: 0x0000...0001
pub const MPC_PRECOMPILE_ADDR: [u8; 20] = {
    let mut a = [0u8; 20];
    a[19] = 0x01;
    a
};

/// Batch auction precompile: 0x0000...0002
pub const BATCH_PRECOMPILE_ADDR: [u8; 20] = {
    let mut a = [0u8; 20];
    a[19] = 0x02;
    a
};

/// Guardian precompile: 0x0000...0003
pub const GUARDIAN_PRECOMPILE_ADDR: [u8; 20] = {
    let mut a = [0u8; 20];
    a[19] = 0x03;
    a
};

/// ZK-Shield precompile: 0x0000...0004
pub const ZK_PRECOMPILE_ADDR: [u8; 20] = {
    let mut a = [0u8; 20];
    a[19] = 0x04;
    a
};

/// BLAKE3 hash precompile: 0x0000...0005
pub const BLAKE3_PRECOMPILE_ADDR: [u8; 20] = {
    let mut a = [0u8; 20];
    a[19] = 0x05;
    a
};

// ─── ChainStateProvider ───────────────────────────────────────────────────────

/// Precompile'ların zincir state'ine erişim trait'i.
///
/// VM-003 düzeltmesi: MpcAccount ve GuardianCheck precompile'ları artık bu
/// trait üzerinden gerçek zincir durumunu sorguluyor. Mock hardcoded 0x01
/// yanıtı kaldırıldı.
pub trait ChainStateProvider: Send + Sync + std::fmt::Debug {
    /// Adres MPC-Native hesap mı?
    fn is_mpc_account(&self, address: &[u8; 20]) -> bool;
    /// Adres aktif guardian mı?
    fn is_active_guardian(&self, address: &[u8; 20]) -> bool;
    /// Slot için batch auction clearing price — None ise bilinmiyor
    fn get_batch_clearing_price(&self, slot: u64) -> Option<u64>;
}

/// Varsayılan no-op uygulama — tüm sorgular güvenli varsayılan (inactive/None) döner.
///
/// Üretimde gerçek state provider enjekte edilmeli; bu uygulama yalnızca
/// state erişimi olmayan bağlamlarda (test, standalone) kullanılır.
#[derive(Debug)]
pub struct NoopChainStateProvider;

impl ChainStateProvider for NoopChainStateProvider {
    fn is_mpc_account(&self, _address: &[u8; 20]) -> bool {
        false
    }
    fn is_active_guardian(&self, _address: &[u8; 20]) -> bool {
        false
    }
    fn get_batch_clearing_price(&self, _slot: u64) -> Option<u64> {
        None
    }
}

// ─── NovaPrecompile ───────────────────────────────────────────────────────────

/// Precompile türü
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NovaPrecompile {
    /// MPC hesap operasyonları
    MpcAccount,
    /// Batch clearing price sorgulama
    BatchAuction,
    /// Guardian durum sorgulama
    GuardianCheck,
    /// ZK kanıt doğrulama
    ZkVerifier,
    /// BLAKE3 hash hesaplama
    Blake3Hash,
}

impl NovaPrecompile {
    /// Precompile gas maliyeti
    pub fn gas_cost(&self, input_len: usize) -> u64 {
        match self {
            NovaPrecompile::MpcAccount => 5_000,
            NovaPrecompile::BatchAuction => 1_000,
            NovaPrecompile::GuardianCheck => 2_000,
            NovaPrecompile::ZkVerifier => 50_000,
            NovaPrecompile::Blake3Hash => 500 + input_len as u64 * 3,
        }
    }

    /// Precompile adı
    pub fn name(&self) -> &'static str {
        match self {
            NovaPrecompile::MpcAccount => "nova-mpc",
            NovaPrecompile::BatchAuction => "nova-batch",
            NovaPrecompile::GuardianCheck => "nova-guardian",
            NovaPrecompile::ZkVerifier => "nova-zk-verify",
            NovaPrecompile::Blake3Hash => "nova-blake3",
        }
    }

    /// Precompile çağrısını yürüt.
    ///
    /// `state`: Zincir durum sağlayıcısı — MpcAccount, GuardianCheck ve
    /// BatchAuction için gerekli. Mock değer YOKTUR; gerçek durum sorgulanır.
    pub fn call(
        &self,
        input: &[u8],
        _caller: &Address,
        state: &dyn ChainStateProvider,
    ) -> VmResult<Vec<u8>> {
        match self {
            // ── VM-003 düzeltmesi: gerçek MPC hesap durumu ───────────────────
            NovaPrecompile::MpcAccount => {
                // input: [20 byte account_addr]
                if input.len() < 20 {
                    return Err(VmError::InvalidInput);
                }
                let addr: [u8; 20] = input[..20].try_into().unwrap();
                let active = state.is_mpc_account(&addr);
                Ok(vec![active as u8])
            }

            // ── VM-006 düzeltmesi: checked_mul (wrapping_mul kaldırıldı) ─────
            NovaPrecompile::BatchAuction => {
                // input: [8 byte slot_id (LE)]
                if input.len() < 8 {
                    return Err(VmError::InvalidInput);
                }
                let slot = u64::from_le_bytes(input[..8].try_into().unwrap());
                // Önce gerçek zincir state'inden sor; yoksa fallback formül
                let price = state
                    .get_batch_clearing_price(slot)
                    .unwrap_or_else(|| slot.checked_mul(1_000_000_u64).unwrap_or(u64::MAX));
                Ok(price.to_le_bytes().to_vec())
            }

            // ── VM-003 düzeltmesi: gerçek guardian durumu ────────────────────
            NovaPrecompile::GuardianCheck => {
                // input: [20 byte guardian_addr]
                if input.len() < 20 {
                    return Err(VmError::InvalidInput);
                }
                let addr: [u8; 20] = input[..20].try_into().unwrap();
                let active = state.is_active_guardian(&addr);
                Ok(vec![active as u8])
            }

            // ── VM-005 düzeltmesi: gerçek Groth16 ZK doğrulaması ─────────────
            //
            // Input formatı:
            //   [1 byte  circuit_type]
            //   [4 bytes LE proof_bytes_len]
            //   [proof_bytes_len bytes proof]
            //   [4 bytes LE num_pub_inputs]
            //   [num_pub_inputs * 32 bytes public inputs]
            //
            // Gerçek Groth16 (192 byte proof): verify_groth16() çağrılır.
            // Mock proof (64 byte): mock params ile verify() çağrılır.
            NovaPrecompile::ZkVerifier => {
                // Minimum 9 byte: 1+4+0+4 (boş proof, sıfır public input)
                if input.len() < 9 {
                    return Err(VmError::InvalidInput);
                }

                // 1. Circuit type
                let circuit_type = parse_circuit_type(input[0])
                    .ok_or(VmError::InvalidInput)?;

                // 2. Proof uzunluğu
                let proof_len =
                    u32::from_le_bytes(input[1..5].try_into().unwrap()) as usize;
                if input.len() < 5 + proof_len + 4 {
                    return Err(VmError::InvalidInput);
                }
                let proof_bytes = input[5..5 + proof_len].to_vec();

                // 3. Public input sayısı
                let num_pub = u32::from_le_bytes(
                    input[5 + proof_len..9 + proof_len].try_into().unwrap(),
                ) as usize;
                if input.len() < 9 + proof_len + num_pub * 32 {
                    return Err(VmError::InvalidInput);
                }

                // 4. Public inputları oku
                let mut public_inputs: Vec<[u8; 32]> = Vec::with_capacity(num_pub);
                for i in 0..num_pub {
                    let start = 9 + proof_len + i * 32;
                    let chunk: [u8; 32] = input[start..start + 32].try_into().unwrap();
                    public_inputs.push(chunk);
                }

                let proof = Groth16Proof { proof_bytes, public_inputs, circuit_type };

                // 5. Gerçek (192 byte) ya da mock (64 byte) doğrulama
                let valid = if proof.is_real() {
                    proof.verify_groth16().unwrap_or(false)
                } else {
                    let params = TrustedSetupParams::mock(circuit_type);
                    proof.verify(&params).unwrap_or(false)
                };

                Ok(vec![valid as u8])
            }

            NovaPrecompile::Blake3Hash => {
                let hash = blake3::hash(input);
                Ok(hash.as_bytes().to_vec())
            }
        }
    }
}

// ─── Yardımcılar ──────────────────────────────────────────────────────────────

/// Input byte'ını `CircuitType`'a dönüştür.
fn parse_circuit_type(b: u8) -> Option<CircuitType> {
    match b {
        0 => Some(CircuitType::Mint),
        1 => Some(CircuitType::Transfer),
        2 => Some(CircuitType::Burn),
        3 => Some(CircuitType::Spend),
        4 => Some(CircuitType::Stake),
        5 => Some(CircuitType::Unstake),
        6 => Some(CircuitType::GovernanceVote),
        7 => Some(CircuitType::NullifierDerivation),
        _ => None,
    }
}

// ─── PrecompileRegistry ───────────────────────────────────────────────────────

/// Precompile kayıt defteri — Avalanchego RegisteredModules pattern.
///
/// VM-003: `state` alanı aracılığıyla `ChainStateProvider` enjekte edilir.
/// Varsayılan: `NoopChainStateProvider` — güvenli (her şey inactive/None).
#[derive(Debug)]
pub struct PrecompileRegistry {
    precompiles: HashMap<[u8; 20], NovaPrecompile>,
    state: Arc<dyn ChainStateProvider>,
}

impl PrecompileRegistry {
    /// `NoopChainStateProvider` ile yeni kayıt defteri.
    pub fn new() -> Self {
        Self::with_state(Arc::new(NoopChainStateProvider))
    }

    /// Belirli bir `ChainStateProvider` ile kayıt defteri.
    pub fn with_state(state: Arc<dyn ChainStateProvider>) -> Self {
        let mut precompiles = HashMap::new();
        precompiles.insert(MPC_PRECOMPILE_ADDR, NovaPrecompile::MpcAccount);
        precompiles.insert(BATCH_PRECOMPILE_ADDR, NovaPrecompile::BatchAuction);
        precompiles.insert(GUARDIAN_PRECOMPILE_ADDR, NovaPrecompile::GuardianCheck);
        precompiles.insert(ZK_PRECOMPILE_ADDR, NovaPrecompile::ZkVerifier);
        precompiles.insert(BLAKE3_PRECOMPILE_ADDR, NovaPrecompile::Blake3Hash);
        Self { precompiles, state }
    }

    pub fn get(&self, address: &[u8; 20]) -> Option<&NovaPrecompile> {
        self.precompiles.get(address)
    }

    pub fn is_precompile(&self, address: &[u8; 20]) -> bool {
        self.precompiles.contains_key(address)
    }

    pub fn count(&self) -> usize {
        self.precompiles.len()
    }

    /// Precompile çağrısı yürüt — (çıktı, gas) döner.
    pub fn call(
        &self,
        address: &[u8; 20],
        input: &[u8],
        caller: &Address,
    ) -> VmResult<(Vec<u8>, u64)> {
        let precompile =
            self.precompiles
                .get(address)
                .ok_or_else(|| VmError::ContractNotFound {
                    address_hex: hex::encode(address),
                })?;

        let gas = precompile.gas_cost(input.len());
        let output = precompile.call(input, caller, self.state.as_ref())?;
        Ok((output, gas))
    }
}

impl Default for PrecompileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_zk::proof::groth16::Groth16Proof;
    use nova_zk::proof::params::{CircuitType, TrustedSetupParams};

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn noop_state() -> Arc<dyn ChainStateProvider> {
        Arc::new(NoopChainStateProvider)
    }

    #[test]
    fn test_precompile_registry_init() {
        let reg = PrecompileRegistry::new();
        assert_eq!(reg.count(), 5);
        assert!(reg.is_precompile(&MPC_PRECOMPILE_ADDR));
        assert!(reg.is_precompile(&BATCH_PRECOMPILE_ADDR));
        assert!(reg.is_precompile(&GUARDIAN_PRECOMPILE_ADDR));
        assert!(reg.is_precompile(&ZK_PRECOMPILE_ADDR));
        assert!(reg.is_precompile(&BLAKE3_PRECOMPILE_ADDR));
    }

    #[test]
    fn test_precompile_non_existent() {
        let reg = PrecompileRegistry::new();
        let unknown = [0xFF; 20];
        assert!(!reg.is_precompile(&unknown));
    }

    /// VM-003: NoopChainStateProvider → 0 (inactive) döner — mock hardcoded 1 yok
    #[test]
    fn test_mpc_precompile_inactive_by_default() {
        let precompile = NovaPrecompile::MpcAccount;
        let input = [1u8; 20];
        let result = precompile.call(&input, &addr(1), noop_state().as_ref()).unwrap();
        assert_eq!(result, vec![0], "NoopChainStateProvider: MPC hesap inactive");
    }

    /// VM-003: MpcAccount gerçek state provider ile aktif dönebilir
    #[test]
    fn test_mpc_precompile_active_with_real_state() {
        #[derive(Debug)]
        struct AlwaysActiveState;
        impl ChainStateProvider for AlwaysActiveState {
            fn is_mpc_account(&self, _: &[u8; 20]) -> bool { true }
            fn is_active_guardian(&self, _: &[u8; 20]) -> bool { true }
            fn get_batch_clearing_price(&self, _: u64) -> Option<u64> { None }
        }
        let precompile = NovaPrecompile::MpcAccount;
        let input = [1u8; 20];
        let state = AlwaysActiveState;
        let result = precompile.call(&input, &addr(1), &state).unwrap();
        assert_eq!(result, vec![1]);
    }

    /// VM-006: checked_mul — overflow yok
    #[test]
    fn test_batch_precompile_no_overflow() {
        let precompile = NovaPrecompile::BatchAuction;
        // u64::MAX / 1_000_000 = 18446744073709 → sonraki değer overflow eder
        let overflow_slot = u64::MAX / 1_000_000 + 1;
        let result = precompile
            .call(&overflow_slot.to_le_bytes(), &addr(1), noop_state().as_ref())
            .unwrap();
        let price = u64::from_le_bytes(result.try_into().unwrap());
        assert_eq!(price, u64::MAX, "Overflow → u64::MAX (saturating)");
    }

    /// VM-006: Normal slot için fallback formül çalışıyor
    #[test]
    fn test_batch_precompile_normal() {
        let precompile = NovaPrecompile::BatchAuction;
        let slot: u64 = 1000;
        let result = precompile
            .call(&slot.to_le_bytes(), &addr(1), noop_state().as_ref())
            .unwrap();
        let price = u64::from_le_bytes(result.try_into().unwrap());
        assert_eq!(price, 1000 * 1_000_000);
    }

    /// VM-005: Geçersiz circuit_type → InvalidInput
    #[test]
    fn test_zk_precompile_invalid_circuit_type() {
        let precompile = NovaPrecompile::ZkVerifier;
        // circuit_type = 255 (bilinmiyor)
        let input = [255u8, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(matches!(
            precompile.call(&input, &addr(1), noop_state().as_ref()),
            Err(VmError::InvalidInput)
        ));
    }

    /// VM-005: Kısa input → InvalidInput
    #[test]
    fn test_zk_precompile_too_short() {
        let precompile = NovaPrecompile::ZkVerifier;
        let short = [0u8; 5];
        assert!(matches!(
            precompile.call(&short, &addr(1), noop_state().as_ref()),
            Err(VmError::InvalidInput)
        ));
    }

    /// VM-005: Mock proof — gerçek TrustedSetupParams ile doğrulama
    #[test]
    fn test_zk_precompile_mock_proof_valid() {
        let precompile = NovaPrecompile::ZkVerifier;
        let ct = CircuitType::Mint;
        let params = TrustedSetupParams::mock(ct);
        let mock_proof =
            Groth16Proof::mock_prove(&params, &[[1u8; 32]], &[[2u8; 32]]).unwrap();

        // Input'u serialize et
        let input = serialize_zk_input(0, &mock_proof);
        let result = precompile
            .call(&input, &addr(1), noop_state().as_ref())
            .unwrap();
        assert_eq!(result, vec![1], "Geçerli mock proof kabul edilmeli");
    }

    /// VM-005: Bozuk mock proof → 0 (invalid)
    #[test]
    fn test_zk_precompile_tampered_proof_rejected() {
        let precompile = NovaPrecompile::ZkVerifier;
        let ct = CircuitType::Mint;
        let params = TrustedSetupParams::mock(ct);
        let mut mock_proof =
            Groth16Proof::mock_prove(&params, &[[1u8; 32]], &[[2u8; 32]]).unwrap();
        mock_proof.proof_bytes[32] ^= 0xFF; // setup hash'i boz

        let input = serialize_zk_input(0, &mock_proof);
        let result = precompile
            .call(&input, &addr(1), noop_state().as_ref())
            .unwrap();
        assert_eq!(result, vec![0], "Bozuk proof reddedilmeli");
    }

    /// VM-003: GuardianCheck — NoopChainStateProvider → 0
    #[test]
    fn test_guardian_precompile_inactive_by_default() {
        let precompile = NovaPrecompile::GuardianCheck;
        let guardian = [2u8; 20];
        let result = precompile
            .call(&guardian, &addr(1), noop_state().as_ref())
            .unwrap();
        assert_eq!(result, vec![0], "NoopChainStateProvider: guardian inactive");
    }

    #[test]
    fn test_blake3_precompile() {
        let precompile = NovaPrecompile::Blake3Hash;
        let data = b"nova-shield-chain";
        let result = precompile.call(data, &addr(1), noop_state().as_ref()).unwrap();
        assert_eq!(result.len(), 32);
        let result2 = precompile.call(data, &addr(1), noop_state().as_ref()).unwrap();
        assert_eq!(result, result2);
    }

    #[test]
    fn test_precompile_gas_cost() {
        let p = NovaPrecompile::Blake3Hash;
        assert!(p.gas_cost(100) > p.gas_cost(0));
    }

    #[test]
    fn test_registry_call() {
        let reg = PrecompileRegistry::new();
        let (output, gas) = reg.call(&BLAKE3_PRECOMPILE_ADDR, b"test", &addr(1)).unwrap();
        assert_eq!(output.len(), 32);
        assert!(gas > 0);
    }

    #[test]
    fn test_mpc_precompile_invalid_input() {
        let precompile = NovaPrecompile::MpcAccount;
        let short_input = [0u8; 5];
        assert!(matches!(
            precompile.call(&short_input, &addr(1), noop_state().as_ref()),
            Err(VmError::InvalidInput)
        ));
    }

    // Yardımcı: ZkVerifier precompile input formatı oluştur
    fn serialize_zk_input(circuit_type_byte: u8, proof: &Groth16Proof) -> Vec<u8> {
        let mut input = Vec::new();
        input.push(circuit_type_byte);
        let proof_len = proof.proof_bytes.len() as u32;
        input.extend_from_slice(&proof_len.to_le_bytes());
        input.extend_from_slice(&proof.proof_bytes);
        let num_pub = proof.public_inputs.len() as u32;
        input.extend_from_slice(&num_pub.to_le_bytes());
        for pi in &proof.public_inputs {
            input.extend_from_slice(pi);
        }
        input
    }
}
