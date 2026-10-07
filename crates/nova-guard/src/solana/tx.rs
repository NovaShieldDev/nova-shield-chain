//! Solana işlem yapısı
//!
//! Solana transaction model:
//! - `Message`: header + account_keys + recent_blockhash + instructions
//! - `Instruction`: program_id_index + accounts + data
//! - `Transaction`: signatures + message
//! - SPL Token: transfer, transferChecked

use crate::errors::{GuardError, GuardResult};
use crate::types::{AssetAddress, ChainId, blake3_hash_many};
use serde::{Deserialize, Serialize};

// ─── SolanaPubkey ─────────────────────────────────────────────────────────────

/// Solana public key (32 byte) — Ed25519
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SolanaPubkey(pub [u8; 32]);

impl SolanaPubkey {
    pub const SYSTEM_PROGRAM: SolanaPubkey = SolanaPubkey([0u8; 32]);
    pub const TOKEN_PROGRAM: SolanaPubkey = SolanaPubkey([
        0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93,
        0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79, 0xac,
        0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91,
        0x32, 0x29, 0xa0, 0x3c, 0xbc, 0x51, 0x55, 0x58,
    ]);

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        SolanaPubkey(bytes)
    }

    pub fn to_display(&self) -> String {
        format!("SOL{}", hex::encode(&self.0[..8]))
    }

    pub fn to_asset_address(&self) -> AssetAddress {
        AssetAddress::solana(self.0)
    }
}

// ─── SolanaInstruction ────────────────────────────────────────────────────────

/// Solana instruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaInstruction {
    /// Program ID indeksi (message.account_keys içindeki)
    pub program_id_index: u8,
    /// Hesap indeksleri
    pub accounts: Vec<u8>,
    /// Instruction verisi
    pub data: Vec<u8>,
}

impl SolanaInstruction {
    /// Sistem programı SOL transferi: transfer(from, to, lamports)
    /// SystemInstruction::Transfer discriminant: [2,0,0,0]
    pub fn system_transfer(from_idx: u8, to_idx: u8, lamports: u64) -> Self {
        let mut data = vec![2u8, 0, 0, 0]; // Transfer discriminant
        data.extend_from_slice(&lamports.to_le_bytes());
        Self {
            program_id_index: 0, // system program always at 0
            accounts: vec![from_idx, to_idx],
            data,
        }
    }

    /// SPL Token transferChecked — token programı
    pub fn spl_transfer_checked(
        source_idx: u8,
        mint_idx: u8,
        dest_idx: u8,
        authority_idx: u8,
        amount: u64,
        decimals: u8,
    ) -> Self {
        let mut data = vec![12u8]; // transferChecked discriminant
        data.extend_from_slice(&amount.to_le_bytes());
        data.push(decimals);
        Self {
            program_id_index: 1, // token program
            accounts: vec![source_idx, mint_idx, dest_idx, authority_idx],
            data,
        }
    }
}

// ─── SolanaMessageHeader ──────────────────────────────────────────────────────

/// Solana mesaj başlığı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaMessageHeader {
    /// İmzalayan sayısı
    pub num_required_signatures: u8,
    /// Salt okunur imzalayanlar
    pub num_readonly_signed_accounts: u8,
    /// Salt okunur imzasız hesaplar
    pub num_readonly_unsigned_accounts: u8,
}

// ─── SolanaMessage ────────────────────────────────────────────────────────────

/// Solana mesajı (işlemin imzalanacak kısmı)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaMessage {
    pub header: SolanaMessageHeader,
    pub account_keys: Vec<SolanaPubkey>,
    /// Recent blockhash (32 byte)
    pub recent_blockhash: [u8; 32],
    pub instructions: Vec<SolanaInstruction>,
}

impl SolanaMessage {
    pub fn new(
        account_keys: Vec<SolanaPubkey>,
        recent_blockhash: [u8; 32],
        instructions: Vec<SolanaInstruction>,
        num_signers: u8,
    ) -> Self {
        Self {
            header: SolanaMessageHeader {
                num_required_signatures: num_signers,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1, // system program
            },
            account_keys,
            recent_blockhash,
            instructions,
        }
    }

    /// Mesaj hash'i (imzalanacak)
    pub fn hash(&self) -> [u8; 32] {
        let key_bytes: Vec<u8> = self.account_keys.iter()
            .flat_map(|k| k.0.iter().copied())
            .collect();
        blake3_hash_many(&[
            b"solana-message-v1",
            &self.recent_blockhash,
            &key_bytes,
            &[self.header.num_required_signatures],
        ])
    }
}

// ─── SolanaTx ─────────────────────────────────────────────────────────────────

/// Solana işlemi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaTx {
    pub message: SolanaMessage,
    /// İmzalar (her imzalayan için 64 byte Ed25519)
    pub signatures: Vec<[u8; 32]>,
}

impl SolanaTx {
    pub fn new(message: SolanaMessage) -> Self {
        Self {
            signatures: vec![[0u8; 32]; message.header.num_required_signatures as usize],
            message,
        }
    }

    /// MPC imza ekle (mock)
    pub fn add_mpc_signature(&mut self, signer_idx: usize, mpc_key: &[u8; 32]) -> GuardResult<()> {
        if signer_idx >= self.signatures.len() {
            return Err(GuardError::InvalidTransaction {
                reason: format!("geçersiz imzalayan indeksi: {}", signer_idx),
            });
        }
        let msg_hash = self.message.hash();
        let sig = blake3_hash_many(&[b"solana-sign", &msg_hash, mpc_key, &[signer_idx as u8]]);
        // Mock: 32 byte (gerçekte 64 byte Ed25519)
        self.signatures[signer_idx] = sig;
        Ok(())
    }

    /// İşlem ID (signature[0] veya hash)
    pub fn tx_id(&self) -> [u8; 32] {
        if !self.signatures.is_empty() && self.signatures[0] != [0u8; 32] {
            self.signatures[0]
        } else {
            self.message.hash()
        }
    }

    /// İşlem geçerli mi?
    pub fn is_signed(&self) -> bool {
        self.signatures.iter().all(|s| *s != [0u8; 32])
    }
}

// ─── SolanaTxBuilder ──────────────────────────────────────────────────────────

/// Solana işlem inşaatçısı
pub struct SolanaTxBuilder {
    recent_blockhash: [u8; 32],
}

impl SolanaTxBuilder {
    pub fn new(recent_blockhash: [u8; 32]) -> Self {
        Self { recent_blockhash }
    }

    /// SOL transferi
    pub fn sol_transfer(
        &self,
        from: SolanaPubkey,
        to: SolanaPubkey,
        lamports: u64,
    ) -> SolanaTx {
        let system_program = SolanaPubkey::SYSTEM_PROGRAM;
        let account_keys = vec![from, to, system_program];
        let instruction = SolanaInstruction::system_transfer(0, 1, lamports);
        let message = SolanaMessage::new(account_keys, self.recent_blockhash, vec![instruction], 1);
        SolanaTx::new(message)
    }

    /// SPL token transferi
    pub fn spl_transfer(
        &self,
        source: SolanaPubkey,
        mint: SolanaPubkey,
        destination: SolanaPubkey,
        authority: SolanaPubkey,
        amount: u64,
        decimals: u8,
    ) -> SolanaTx {
        let token_program = SolanaPubkey::TOKEN_PROGRAM;
        let account_keys = vec![source, mint, destination, authority, token_program];
        let instruction = SolanaInstruction::spl_transfer_checked(0, 1, 2, 3, amount, decimals);
        let message = SolanaMessage::new(account_keys, self.recent_blockhash, vec![instruction], 1);
        SolanaTx::new(message)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn pk(n: u8) -> SolanaPubkey {
        SolanaPubkey([n; 32])
    }

    fn blockhash() -> [u8; 32] {
        [0xBBu8; 32]
    }

    #[test]
    fn test_solana_pubkey_display() {
        let pk = SolanaPubkey([0xABu8; 32]);
        assert!(pk.to_display().starts_with("SOL"));
    }

    #[test]
    fn test_system_program_zero() {
        assert_eq!(SolanaPubkey::SYSTEM_PROGRAM.0, [0u8; 32]);
    }

    #[test]
    fn test_sol_transfer_instruction() {
        let instr = SolanaInstruction::system_transfer(0, 1, 1_000_000_000);
        assert_eq!(instr.program_id_index, 0);
        assert_eq!(instr.accounts, vec![0, 1]);
        assert_eq!(&instr.data[..4], &[2, 0, 0, 0]); // Transfer discriminant
    }

    #[test]
    fn test_spl_transfer_instruction() {
        let instr = SolanaInstruction::spl_transfer_checked(0, 1, 2, 3, 1_000, 9);
        assert_eq!(instr.data[0], 12); // transferChecked
        assert_eq!(instr.data.last(), Some(&9)); // decimals
    }

    #[test]
    fn test_solana_message_hash() {
        let builder = SolanaTxBuilder::new(blockhash());
        let tx = builder.sol_transfer(pk(1), pk(2), 500_000_000);
        let h = tx.message.hash();
        assert_ne!(h, [0u8; 32]);
        assert_eq!(h, tx.message.hash()); // deterministic
    }

    #[test]
    fn test_sol_transfer_builder() {
        let builder = SolanaTxBuilder::new(blockhash());
        let tx = builder.sol_transfer(pk(1), pk(2), 1_000_000_000);
        assert!(!tx.message.instructions.is_empty());
        assert_eq!(tx.message.account_keys.len(), 3);
    }

    #[test]
    fn test_spl_transfer_builder() {
        let builder = SolanaTxBuilder::new(blockhash());
        let tx = builder.spl_transfer(pk(1), pk(2), pk(3), pk(4), 1_000, 6);
        assert_eq!(tx.message.instructions[0].data[0], 12);
    }

    #[test]
    fn test_solana_tx_add_mpc_signature() {
        let builder = SolanaTxBuilder::new(blockhash());
        let mut tx = builder.sol_transfer(pk(1), pk(2), 100_000);
        let key = [0x42u8; 32];
        tx.add_mpc_signature(0, &key).unwrap();
        assert!(tx.is_signed());
    }

    #[test]
    fn test_solana_tx_invalid_signer_idx() {
        let builder = SolanaTxBuilder::new(blockhash());
        let mut tx = builder.sol_transfer(pk(1), pk(2), 100_000);
        let result = tx.add_mpc_signature(5, &[0u8; 32]);
        assert!(matches!(result, Err(GuardError::InvalidTransaction { .. })));
    }

    #[test]
    fn test_solana_tx_id_after_signing() {
        let builder = SolanaTxBuilder::new(blockhash());
        let mut tx = builder.sol_transfer(pk(1), pk(2), 100_000);
        tx.add_mpc_signature(0, &[0x99u8; 32]).unwrap();
        let txid = tx.tx_id();
        assert_ne!(txid, [0u8; 32]);
    }
}
