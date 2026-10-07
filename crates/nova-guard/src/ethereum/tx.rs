//! Ethereum EIP-1559 işlem yapısı
//!
//! go-ethereum `core/types/tx_dynamic_fee.go` pattern:
//! - `DynamicFeeTx`: ChainID, Nonce, GasTipCap, GasFeeCap, Gas, To, Value, Data, V/R/S
//! - `TxData` interface: txType(), chainID(), data(), gas(), value(), nonce(), to()
//! - `NewTx(inner TxData)` → `Transaction`
//! - Signer: London/Prague signer, EIP-155 replay protection
//!
//! Nova Shield mock: gerçek secp256k1/RLP yok, BLAKE3 ile mock imzalama

use crate::errors::{GuardError, GuardResult};
use crate::types::blake3_hash_many;
use serde::{Deserialize, Serialize};

// ─── EthChainId ───────────────────────────────────────────────────────────────

/// Ethereum zincir ID — EIP-155 replay protection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EthChainId(pub u64);

impl EthChainId {
    pub const MAINNET: EthChainId = EthChainId(1);
    pub const GOERLI: EthChainId = EthChainId(5);
    pub const SEPOLIA: EthChainId = EthChainId(11155111);
}

// ─── EthAddress ───────────────────────────────────────────────────────────────

/// Ethereum adresi (20 byte) — go-ethereum `common.Address`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EthAddress(pub [u8; 20]);

impl EthAddress {
    pub fn zero() -> Self {
        EthAddress([0u8; 20])
    }

    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.0))
    }
}

// ─── EthTx (EIP-1559 DynamicFeeTx) ───────────────────────────────────────────

/// EIP-1559 işlemi — go-ethereum `DynamicFeeTx`
/// go-ethereum alanları: ChainID, Nonce, GasTipCap, GasFeeCap, Gas, To, Value, Data, AccessList, V/R/S
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthTx {
    /// Zincir ID (EIP-155 replay protection) — go-ethereum: `ChainID *big.Int`
    pub chain_id: EthChainId,
    /// Nonce — go-ethereum: `Nonce uint64`
    pub nonce: u64,
    /// maxPriorityFeePerGas (tip) — go-ethereum: `GasTipCap *big.Int` (Gwei)
    pub gas_tip_cap: u64,
    /// maxFeePerGas — go-ethereum: `GasFeeCap *big.Int` (Gwei)
    pub gas_fee_cap: u64,
    /// Gas limiti — go-ethereum: `Gas uint64`
    pub gas: u64,
    /// Alıcı (nil = contract creation) — go-ethereum: `To *common.Address`
    pub to: Option<EthAddress>,
    /// Miktar (wei) — go-ethereum: `Value *big.Int`
    pub value: u64,
    /// Calldata — go-ethereum: `Data []byte`
    pub data: Vec<u8>,
    /// İmza V — go-ethereum: `V *big.Int`
    pub v: u8,
    /// İmza R (32 byte) — go-ethereum: `R *big.Int`
    pub r: [u8; 32],
    /// İmza S (32 byte) — go-ethereum: `S *big.Int`
    pub s: [u8; 32],
    /// Gönderen (imza sonrası türetilir)
    pub from: Option<EthAddress>,
}

impl EthTx {
    /// Yeni unsigned EIP-1559 işlem
    pub fn new(
        chain_id: EthChainId,
        nonce: u64,
        gas_tip_cap: u64,
        gas_fee_cap: u64,
        gas: u64,
        to: Option<EthAddress>,
        value: u64,
        data: Vec<u8>,
    ) -> GuardResult<Self> {
        if gas_fee_cap < gas_tip_cap {
            return Err(GuardError::InvalidTransaction {
                reason: "gasFeeCap < gasTipCap".into(),
            });
        }
        Ok(Self {
            chain_id, nonce, gas_tip_cap, gas_fee_cap, gas, to, value, data,
            v: 0, r: [0u8; 32], s: [0u8; 32], from: None,
        })
    }

    /// Transfer işlemi (ETH gönderimi)
    pub fn transfer(chain_id: EthChainId, nonce: u64, to: EthAddress, value_wei: u64) -> GuardResult<Self> {
        Self::new(chain_id, nonce, 1_500_000_000, 20_000_000_000, 21_000, Some(to), value_wei, vec![])
    }

    /// ERC-20 transfer calldata — transfer(address,uint256) selector: 0xa9059cbb
    pub fn erc20_transfer_data(to: EthAddress, amount: u64) -> Vec<u8> {
        let mut data = vec![0xa9u8, 0x05, 0x9c, 0xbb]; // transfer(address,uint256)
        // ABI-encode: address (32 byte, sağa sıfır doldurulmuş)
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(to.as_bytes());
        // Amount (uint256, 32 byte big-endian)
        data.extend_from_slice(&[0u8; 24]);
        data.extend_from_slice(&amount.to_be_bytes());
        data
    }

    /// Sighash — go-ethereum: EIP-712 typed data hash
    /// Mock: BLAKE3(chain_id || nonce || to || value || data)
    pub fn sighash(&self) -> [u8; 32] {
        let chain_bytes = self.chain_id.0.to_le_bytes();
        let nonce_bytes = self.nonce.to_le_bytes();
        let value_bytes = self.value.to_le_bytes();
        let to_bytes = self.to.map(|a| a.0.to_vec()).unwrap_or_default();

        blake3_hash_many(&[
            b"nova-eth-eip1559-sighash",
            &chain_bytes,
            &nonce_bytes,
            &to_bytes,
            &value_bytes,
            &self.data,
        ])
    }

    /// Mock MPC imzala (BLAKE3 tabanlı)
    /// Gerçekte: secp256k1 ECDSA
    pub fn sign(&mut self, signer: &EthAddress, mpc_key: &[u8; 32]) -> [u8; 32] {
        let sighash = self.sighash();
        let sig = blake3_hash_many(&[b"nova-eth-sign", &sighash, mpc_key]);
        self.r = sig;
        self.s = blake3_hash_many(&[b"nova-eth-sign-s", &sig]);
        self.v = 27 + (sig[0] & 1); // mock v
        self.from = Some(*signer);
        sig
    }

    /// İşlem hash'i (broadcast sonrası)
    pub fn hash(&self) -> [u8; 32] {
        let sighash = self.sighash();
        blake3_hash_many(&[b"nova-eth-tx-hash", &sighash, &self.r, &self.s, &[self.v]])
    }

    /// Effective gas fiyatı — go-ethereum: effectiveGasPrice(baseFee)
    pub fn effective_gas_price(&self, base_fee: u64) -> u64 {
        let tip = self.gas_fee_cap.saturating_sub(base_fee).min(self.gas_tip_cap);
        base_fee.saturating_add(tip)
    }

    /// Maksimum gas maliyeti (gas * gasFeeCap)
    pub fn max_fee_cost(&self) -> u64 {
        self.gas.saturating_mul(self.gas_fee_cap)
    }

    /// İşlem geçerli mi?
    pub fn validate(&self) -> GuardResult<()> {
        if self.gas_fee_cap < self.gas_tip_cap {
            return Err(GuardError::InvalidTransaction {
                reason: "gasFeeCap < gasTipCap".into(),
            });
        }
        if self.gas == 0 {
            return Err(GuardError::InvalidTransaction {
                reason: "gas = 0".into(),
            });
        }
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> EthAddress {
        EthAddress([n; 20])
    }

    #[test]
    fn test_eth_tx_transfer() {
        let tx = EthTx::transfer(EthChainId::MAINNET, 0, addr(0xAA), 1_000_000_000_000_000_000).unwrap();
        assert_eq!(tx.chain_id, EthChainId::MAINNET);
        assert_eq!(tx.gas, 21_000);
        assert!(tx.to.is_some());
    }

    #[test]
    fn test_eth_tx_validate_ok() {
        let tx = EthTx::transfer(EthChainId::MAINNET, 0, addr(1), 1_000).unwrap();
        assert!(tx.validate().is_ok());
    }

    #[test]
    fn test_eth_tx_validate_fee_cap_too_low() {
        // gas_fee_cap < gas_tip_cap
        let result = EthTx::new(
            EthChainId::MAINNET, 0,
            20_000_000_000, // tip
            10_000_000_000, // fee_cap < tip
            21_000, Some(addr(1)), 0, vec![]
        );
        assert!(matches!(result, Err(GuardError::InvalidTransaction { .. })));
    }

    #[test]
    fn test_eth_tx_sighash_deterministic() {
        let tx = EthTx::transfer(EthChainId::MAINNET, 1, addr(2), 500).unwrap();
        assert_eq!(tx.sighash(), tx.sighash());
    }

    #[test]
    fn test_eth_tx_sign() {
        let mut tx = EthTx::transfer(EthChainId::MAINNET, 0, addr(3), 1_000).unwrap();
        let key = [0x42u8; 32];
        let sig = tx.sign(&addr(0x01), &key);
        assert_ne!(sig, [0u8; 32]);
        assert!(tx.from.is_some());
        assert!(tx.v == 27 || tx.v == 28);
    }

    #[test]
    fn test_eth_tx_hash() {
        let mut tx = EthTx::transfer(EthChainId::MAINNET, 0, addr(4), 2_000).unwrap();
        tx.sign(&addr(0x02), &[0x11u8; 32]);
        let hash = tx.hash();
        assert_ne!(hash, [0u8; 32]);
    }

    #[test]
    fn test_eth_tx_effective_gas_price() {
        let tx = EthTx::new(
            EthChainId::MAINNET, 0,
            1_000_000_000,  // tip: 1 Gwei
            20_000_000_000, // fee cap: 20 Gwei
            21_000, Some(addr(1)), 0, vec![]
        ).unwrap();
        let base_fee = 10_000_000_000u64; // 10 Gwei
        let effective = tx.effective_gas_price(base_fee);
        assert_eq!(effective, 11_000_000_000); // 10 + min(10, 1) = 11 Gwei
    }

    #[test]
    fn test_erc20_transfer_data() {
        let data = EthTx::erc20_transfer_data(addr(0xFF), 1_000);
        assert_eq!(data[0..4], [0xa9, 0x05, 0x9c, 0xbb]);
        assert_eq!(data.len(), 68); // 4 selector + 32 address + 32 amount
    }

    #[test]
    fn test_eth_chain_ids() {
        assert_eq!(EthChainId::MAINNET.0, 1);
        assert_eq!(EthChainId::GOERLI.0, 5);
    }

    #[test]
    fn test_eth_address_to_hex() {
        let addr = EthAddress([0x12u8; 20]);
        assert!(addr.to_hex().starts_with("0x"));
        assert_eq!(addr.to_hex().len(), 42);
    }
}
