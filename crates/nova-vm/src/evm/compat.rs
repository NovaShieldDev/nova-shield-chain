//! EVM uyumluluk katmanı — revm v14 ile gerçek EVM yürütme
//!
//! ## Araştırma Kaynakları
//! - go-ethereum analizi (06-go-ethereum.md): EVM interpreter, opcode maliyetleri
//! - Penumbra analizi (03-PENUMBRA-NOVA-ARASTIRMA.md): precompile pattern
//! - Avalanchego analizi (GAP-ANALIZ.md): Subnet-EVM precompile registry
//!
//! ## Mimari
//! - `revm::Evm`: Gerçek EVM interpreter (London fork uyumlu)
//! - `revm::db::InMemoryDB`: Test ve hafif yürütme için bellek içi state
//! - `EvmCompatLayer`: Nova kontekstinde EVM call'larını yöneten katman
//!
//! ## ERC-20 Arayüzü
//! Standart ERC-20 function selector'ları desteklenir:
//! - transfer(address,uint256)  → 0xa9059cbb
//! - balanceOf(address)         → 0x70a08231
//! - approve(address,uint256)   → 0x095ea7b3
//!
//! ## Güvenlik Notu
//! - Gerçek EVM yürütme; REVERT, gas limit, invalid opcode senaryoları revm tarafından yönetilir
//! - Nova precompile adresleri (0x01..0x05) EVM çağrılarından korunmalıdır

use crate::errors::{VmError, VmResult};
use nova_primitives::Address;
use revm::{
    db::InMemoryDB,
    primitives::{
        AccountInfo, Bytecode, Bytes as RevmBytes, ExecutionResult as RevmResult,
        Output, TransactTo, TxEnv, B256, U256,
    },
    Evm,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// EVM varsayılan chain ID (Nova Mainnet)
pub const NOVA_EVM_CHAIN_ID: u64 = 11823;

/// Minimum calldata uzunluğu (4 byte function selector)
pub const EVM_MIN_CALLDATA_LEN: usize = 4;

// ─── ERC-20 Mock Contract Bytecode ────────────────────────────────────────────
//
// Minimal EVM bytecode — GG20 kaynak analizinden (02-analiz.md) öğrenilen EVM dispatch:
// 1. CALLDATALOAD(0) >> 224 = function selector (4 byte)
// 2. Bilinen selector'larla karşılaştır
// 3. İlgili handler'a JUMPI
//
// Bytecode düzeni:
//   0x00: selector dispatch
//   0x2C: transfer/approve handler (return uint256=1 = true)
//   0x38: balanceOf handler (return uint256=1e18)
pub const MOCK_ERC20_BYTECODE: &[u8] = &[
    // CALLDATALOAD(0) → 32 byte, SHR 224 → 4-byte selector
    0x60, 0x00, 0x35, 0x60, 0xE0, 0x1C,
    // DUP1; PUSH4 transfer(address,uint256)=0xa9059cbb; EQ; PUSH2 0x002C; JUMPI
    0x80, 0x63, 0xA9, 0x05, 0x9C, 0xBB, 0x14, 0x61, 0x00, 0x2C, 0x57,
    // DUP1; PUSH4 balanceOf(address)=0x70a08231; EQ; PUSH2 0x0038; JUMPI
    0x80, 0x63, 0x70, 0xA0, 0x82, 0x31, 0x14, 0x61, 0x00, 0x38, 0x57,
    // DUP1; PUSH4 approve(address,uint256)=0x095ea7b3; EQ; PUSH2 0x002C; JUMPI
    0x80, 0x63, 0x09, 0x5E, 0xA7, 0xB3, 0x14, 0x61, 0x00, 0x2C, 0x57,
    // Unknown selector → REVERT(0, 0)
    0x60, 0x00, 0x60, 0x00, 0xFD,
    // 0x2C: JUMPDEST — transfer/approve handler → return 1 (true) as uint256
    0x5B, 0x50, 0x60, 0x01, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xF3,
    // 0x38: JUMPDEST — balanceOf handler → return 1e18 as uint256
    0x5B, 0x50, 0x67, 0x0D, 0xE0, 0xB6, 0xB3, 0xA7, 0x64, 0x00, 0x00,
    0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xF3,
];

// ─── ABI Codec ────────────────────────────────────────────────────────────────

/// Solidity ABI encoding/decoding yardımcıları
/// Ethereum ABI spesifikasyonu: https://docs.soliditylang.org/en/latest/abi-spec.html
pub struct AbiCodec;

impl AbiCodec {
    /// uint256 encode — 32 byte big-endian (Ethereum ABI standardı)
    pub fn encode_uint256(value: u128) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[16..].copy_from_slice(&value.to_be_bytes());
        buf
    }

    /// uint256 decode
    pub fn decode_uint256(data: &[u8]) -> VmResult<u128> {
        if data.len() < 32 {
            return Err(VmError::AbiDecodeError {
                reason: "uint256 için 32 byte gerekli".into(),
            });
        }
        let bytes: [u8; 16] = data[16..32].try_into().unwrap();
        Ok(u128::from_be_bytes(bytes))
    }

    /// address encode — 32 byte (12 sıfır + 20 byte adres)
    pub fn encode_address(addr: &Address) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[12..].copy_from_slice(addr.as_bytes());
        buf
    }

    /// address decode
    pub fn decode_address(data: &[u8]) -> VmResult<Address> {
        if data.len() < 32 {
            return Err(VmError::AbiDecodeError {
                reason: "address için 32 byte gerekli".into(),
            });
        }
        let addr_bytes: [u8; 20] = data[12..32].try_into().unwrap();
        Ok(Address::from_bytes(addr_bytes))
    }

    /// 4-byte function selector — Ethereum ABI standardı: keccak256(signature)[0..4]
    pub fn function_selector(signature: &str) -> [u8; 4] {
        use sha3::{Digest, Keccak256};
        let hash = Keccak256::digest(signature.as_bytes());
        [hash[0], hash[1], hash[2], hash[3]]
    }
}

// ─── EvmCallResult ────────────────────────────────────────────────────────────

/// EVM çağrısı sonucu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmCallResult {
    pub success: bool,
    pub return_data: Vec<u8>,
    pub gas_used: u64,
    pub revert_reason: Option<String>,
}

impl EvmCallResult {
    pub fn success(data: Vec<u8>, gas_used: u64) -> Self {
        Self { success: true, return_data: data, gas_used, revert_reason: None }
    }

    pub fn revert(reason: String, gas_used: u64) -> Self {
        Self { success: false, return_data: vec![], gas_used, revert_reason: Some(reason) }
    }
}

// ─── EvmCompatLayer ───────────────────────────────────────────────────────────

/// EVM uyumluluk katmanı
///
/// revm tabanlı gerçek EVM interpreter. InMemoryDB ile state yönetir.
/// Contract deployment için `deploy_contract()` kullanılır.
pub struct EvmCompatLayer {
    db: Arc<parking_lot::RwLock<InMemoryDB>>,
}

impl EvmCompatLayer {
    /// Yeni EVM katmanı — boş state ile başlar
    pub fn new() -> Self {
        Self {
            db: Arc::new(parking_lot::RwLock::new(InMemoryDB::default())),
        }
    }

    /// Nova precompile adresleri ile birlikte başlat
    pub fn with_nova_precompiles() -> Self {
        let layer = Self::new();
        // Gelecekte: precompile bytecode'larını deploy et
        layer
    }

    /// EVM bytecode'unu belirtilen adrese deploy et
    ///
    /// Test ve genesis state kurulumu için kullanılır.
    pub fn deploy_contract(&self, address: Address, bytecode: &[u8]) {
        let revm_addr = nova_to_revm_addr(&address);
        let code = Bytecode::new_raw(RevmBytes::copy_from_slice(bytecode));
        let account = AccountInfo {
            balance: U256::ZERO,
            nonce: 1,
            code_hash: B256::from_slice(&blake3::hash(bytecode).as_bytes()[..]),
            code: Some(code),
        };
        self.db.write().insert_account_info(revm_addr, account);
    }

    /// EVM call yürüt
    ///
    /// revm::Evm ile tam EVM interpreter çalıştırır.
    /// Kısa calldata → Ok(revert), bilinmeyen selector → Ok(revert)
    pub fn evm_call(
        &self,
        caller: Address,
        to: Address,
        input: &[u8],
        value: u128,
        gas_limit: u64,
    ) -> VmResult<EvmCallResult> {
        // Calldata çok kısa kontrolü (4 byte selector zorunlu)
        if input.len() < EVM_MIN_CALLDATA_LEN && !input.is_empty() {
            return Ok(EvmCallResult::revert(
                format!("calldata çok kısa ({} byte, min {EVM_MIN_CALLDATA_LEN} bekleniyor)", input.len()),
                gas_limit / 4,
            ));
        }

        let caller_addr = nova_to_revm_addr(&caller);
        let to_addr = nova_to_revm_addr(&to);

        // revm Evm oluştur ve transaction çalıştır
        let mut db_guard = self.db.write();
        let result = {
            let mut evm = Evm::builder()
                .with_db(&mut *db_guard)
                .modify_tx_env(|tx: &mut TxEnv| {
                    tx.caller = caller_addr;
                    tx.transact_to = TransactTo::Call(to_addr);
                    tx.data = RevmBytes::copy_from_slice(input);
                    tx.value = U256::from(value);
                    tx.gas_limit = gas_limit;
                    // gas_price = 0: Nova üst katmanı ücretlendirmeyi yönetir;
                    // EVM compat layer saf bytecode yürütümü için gas bakiyesi gerektirmez
                    tx.gas_price = U256::ZERO;
                })
                .build();

            evm.transact().map_err(|e| VmError::EvmError {
                reason: format!("revm transact hatası: {e}"),
            })?
        };

        // revm sonucunu EvmCallResult'a çevir
        Ok(revm_result_to_nova(result.result))
    }

    /// EVM static call (read-only, değer transferi yok)
    pub fn static_call(
        &self,
        caller: Address,
        to: Address,
        input: &[u8],
        gas_limit: u64,
    ) -> VmResult<EvmCallResult> {
        self.evm_call(caller, to, input, 0, gas_limit)
    }
}

impl Default for EvmCompatLayer {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Yardımcı Fonksiyonlar ────────────────────────────────────────────────────

/// Nova Address → revm primitives::Address dönüşümü
fn nova_to_revm_addr(addr: &Address) -> revm::primitives::Address {
    revm::primitives::Address::from(*addr.as_bytes())
}

/// revm ExecutionResult → EvmCallResult dönüşümü
fn revm_result_to_nova(result: RevmResult) -> EvmCallResult {
    match result {
        RevmResult::Success { output, gas_used, .. } => {
            let data = match output {
                Output::Call(bytes) => bytes.to_vec(),
                Output::Create(bytes, _) => bytes.to_vec(),
            };
            EvmCallResult::success(data, gas_used)
        }
        RevmResult::Revert { output, gas_used } => {
            let reason = if output.is_empty() {
                "execution reverted".into()
            } else {
                // ABI decode revert reason (ilk 4 byte Error(string) selector, sonra ABI string)
                format!("revert: 0x{}", hex::encode(&output))
            };
            EvmCallResult::revert(reason, gas_used)
        }
        RevmResult::Halt { reason, gas_used } => {
            EvmCallResult::revert(format!("halt: {reason:?}"), gas_used)
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    /// Test ortamı: addr(2)'ye mock ERC-20 deploy edilmiş EvmCompatLayer
    fn evm_with_mock_erc20() -> EvmCompatLayer {
        let evm = EvmCompatLayer::new();
        evm.deploy_contract(addr(2), MOCK_ERC20_BYTECODE);
        evm
    }

    // ─── ABI Codec Testleri ───────────────────────────────────────────────────

    #[test]
    fn test_abi_encode_decode_uint256() {
        let value: u128 = 12345678901234567890;
        let encoded = AbiCodec::encode_uint256(value);
        let decoded = AbiCodec::decode_uint256(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_abi_encode_decode_address() {
        let a = addr(42);
        let encoded = AbiCodec::encode_address(&a);
        let decoded = AbiCodec::decode_address(&encoded).unwrap();
        assert_eq!(decoded, a);
    }

    #[test]
    fn test_function_selector_deterministic() {
        let s1 = AbiCodec::function_selector("transfer(address,uint256)");
        let s2 = AbiCodec::function_selector("transfer(address,uint256)");
        assert_eq!(s1, s2);
        assert_ne!(s1, [0u8; 4]);
    }

    #[test]
    fn test_function_selector_uses_keccak256() {
        // Ethereum ABI standard — bilinen selector değerleri
        let transfer_sel = AbiCodec::function_selector("transfer(address,uint256)");
        assert_eq!(transfer_sel, [0xa9, 0x05, 0x9c, 0xbb], "transfer selector yanlış");

        let balance_sel = AbiCodec::function_selector("balanceOf(address)");
        assert_eq!(balance_sel, [0x70, 0xa0, 0x82, 0x31], "balanceOf selector yanlış");

        let approve_sel = AbiCodec::function_selector("approve(address,uint256)");
        assert_eq!(approve_sel, [0x09, 0x5e, 0xa7, 0xb3], "approve selector yanlış");
    }

    // ─── EVM Çağrı Testleri ───────────────────────────────────────────────────

    #[test]
    fn test_evm_transfer_call() {
        let evm = evm_with_mock_erc20();
        let caller = addr(1);
        let to = addr(2);

        let sel = AbiCodec::function_selector("transfer(address,uint256)");
        let mut calldata = sel.to_vec();
        calldata.extend_from_slice(&AbiCodec::encode_address(&addr(3)));
        calldata.extend_from_slice(&AbiCodec::encode_uint256(1000));

        let result = evm.evm_call(caller, to, &calldata, 0, 100_000).unwrap();
        assert!(result.success, "transfer başarılı olmalı");
        assert!(result.gas_used > 0);
        // Return data: uint256=1 (true)
        let ret_val = AbiCodec::decode_uint256(&result.return_data).unwrap();
        assert_eq!(ret_val, 1, "transfer true döndürmeli");
    }

    #[test]
    fn test_evm_balance_of_call() {
        let evm = evm_with_mock_erc20();
        let caller = addr(1);
        let to = addr(2);

        let sel = AbiCodec::function_selector("balanceOf(address)");
        let mut calldata = sel.to_vec();
        calldata.extend_from_slice(&AbiCodec::encode_address(&addr(1)));

        let result = evm.static_call(caller, to, &calldata, 50_000).unwrap();
        assert!(result.success, "balanceOf başarılı olmalı");

        let balance = AbiCodec::decode_uint256(&result.return_data).unwrap();
        assert!(balance > 0, "bakiye sıfırdan büyük olmalı");
    }

    #[test]
    fn test_evm_approve_call() {
        let evm = evm_with_mock_erc20();
        let caller = addr(1);
        let to = addr(2);

        let sel = AbiCodec::function_selector("approve(address,uint256)");
        let mut calldata = sel.to_vec();
        calldata.extend_from_slice(&AbiCodec::encode_address(&addr(3)));
        calldata.extend_from_slice(&AbiCodec::encode_uint256(u128::MAX));

        let result = evm.evm_call(caller, to, &calldata, 0, 100_000).unwrap();
        assert!(result.success, "approve başarılı olmalı");
        let ret = AbiCodec::decode_uint256(&result.return_data).unwrap();
        assert_eq!(ret, 1, "approve true döndürmeli");
    }

    #[test]
    fn test_evm_short_input_reverts() {
        let evm = EvmCompatLayer::new();
        let result = evm.evm_call(addr(1), addr(2), &[0x01], 0, 100_000).unwrap();
        assert!(!result.success, "kısa calldata revert olmalı");
        assert!(result.revert_reason.is_some());
    }

    #[test]
    fn test_evm_unknown_selector_reverts() {
        let evm = evm_with_mock_erc20();
        let calldata = [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00];
        let result = evm.evm_call(addr(1), addr(2), &calldata, 0, 100_000).unwrap();
        // Bilinmeyen selector → REVERT (EVM semantiği: revert is Ok, not Err)
        assert!(!result.success, "bilinmeyen selector revert olmalı");
        assert!(result.revert_reason.is_some());
    }

    #[test]
    fn test_evm_call_empty_contract() {
        // Deploy edilmemiş adrese çağrı — boş kod → başarı (EVM: empty account = no-op)
        let evm = EvmCompatLayer::new();
        let sel = AbiCodec::function_selector("transfer(address,uint256)");
        let mut calldata = sel.to_vec();
        calldata.extend_from_slice(&[0u8; 64]);
        let result = evm.evm_call(addr(1), addr(99), &calldata, 0, 100_000).unwrap();
        // Boş hesap = kod yok = başarı, boş return data
        assert!(result.success, "boş hesap çağrısı başarılı olmalı");
        assert!(result.return_data.is_empty());
    }

    #[test]
    fn test_abi_decode_short_data() {
        let short = [0u8; 10];
        assert!(matches!(AbiCodec::decode_uint256(&short), Err(VmError::AbiDecodeError { .. })));
        assert!(matches!(AbiCodec::decode_address(&short), Err(VmError::AbiDecodeError { .. })));
    }

    #[test]
    fn test_evm_deploy_and_call_multiple() {
        let evm = EvmCompatLayer::new();
        // İki farklı adrese aynı kontrat deploy et
        evm.deploy_contract(addr(10), MOCK_ERC20_BYTECODE);
        evm.deploy_contract(addr(11), MOCK_ERC20_BYTECODE);

        let sel = AbiCodec::function_selector("balanceOf(address)");
        let mut calldata = sel.to_vec();
        calldata.extend_from_slice(&AbiCodec::encode_address(&addr(1)));

        let r1 = evm.static_call(addr(1), addr(10), &calldata, 50_000).unwrap();
        let r2 = evm.static_call(addr(1), addr(11), &calldata, 50_000).unwrap();

        assert!(r1.success && r2.success);
        assert_eq!(r1.return_data, r2.return_data, "aynı kontrat aynı sonuç üretmeli");
    }
}
