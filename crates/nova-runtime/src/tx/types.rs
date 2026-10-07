//! Transaction tipleri — nova-runtime (§8.1, §5.9)
//!
//! EIP-1559 dinamik fee modeli (go-ethereum'dan ilham):
//!   - max_fee_per_gas: Kullanıcının ödemek istediği maksimum fee
//!   - priority_fee: Validator tip
//!   - base_fee: Ağ tarafından belirlenir (yakılır)

use nova_primitives::{Address, Balance, SlotId, ValidatorId};
use serde::{Deserialize, Serialize};

// ─── Transaction Tipleri ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionKind {
    /// Token transferi
    Transfer {
        to: Address,
        amount: u128,
    },
    /// Validator stake et
    Stake {
        validator_id: ValidatorId,
        amount: u128,
    },
    /// Stake kaldır (unbonding başlatır)
    Unstake {
        validator_id: ValidatorId,
        amount: u128,
    },
    /// Unbonding tamamlandıktan sonra token çek
    WithdrawUnbonded {
        validator_id: ValidatorId,
    },
    /// Governance oyu
    GovernanceVote {
        proposal_id: u64,
        option: VoteOption,
    },
    /// Governance önerisi gönder
    GovernancePropose {
        title: String,
        description: String,
        payload: ProposalPayload,
        deposit: u128,
    },
    /// Validator kayıt
    RegisterValidator {
        commission_bps: u32,
        min_self_delegation: u128,
    },
    /// Runtime upgrade talebi (governance onaylı)
    RuntimeUpgrade {
        new_version: u32,
        activation_slot: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoteOption {
    Yes,
    No,
    Abstain,
    NoWithVeto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalPayload {
    /// Parametre değişikliği
    ParameterChange { key: String, value: String },
    /// Treasury harcaması
    TreasurySpend { recipient: Address, amount: u128 },
    /// Runtime upgrade
    UpgradeRuntime { new_version: u32 },
    /// Metin önerisi (bağlayıcı değil)
    Text { content: String },
}

// ─── Transaction yapısı ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Gönderenin adresi
    pub sender: Address,
    /// Nonce — replay protection
    pub nonce: u64,
    /// İşlem tipi
    pub kind: TransactionKind,
    /// EIP-1559: Kullanıcının ödemeye razı olduğu max fee per gas
    pub max_fee_per_gas: u128,
    /// EIP-1559: Validator'a gidecek priority fee (tip)
    pub max_priority_fee_per_gas: u128,
    /// Gas limiti
    pub gas_limit: u64,
    /// İmza (serde uyumlu Vec<u8> — production'da gerçek imza)
    pub signature: Vec<u8>,
    /// İşlem slot'u (replay protection için)
    pub slot: SlotId,
}

impl Transaction {
    /// Test için mock transaction oluştur
    pub fn mock_transfer(sender: Address, to: Address, amount: u128, nonce: u64) -> Self {
        Self {
            sender,
            nonce,
            kind: TransactionKind::Transfer { to, amount },
            max_fee_per_gas: 1_000_000_000, // 1 Gwei
            max_priority_fee_per_gas: 100_000_000, // 0.1 Gwei
            gas_limit: 21_000,
            signature: vec![0u8; 64],
            slot: SlotId(0),
        }
    }

    pub fn mock_stake(
        sender: Address,
        validator_id: ValidatorId,
        amount: u128,
        nonce: u64,
    ) -> Self {
        Self {
            sender,
            nonce,
            kind: TransactionKind::Stake { validator_id, amount },
            max_fee_per_gas: 1_000_000_000,
            max_priority_fee_per_gas: 100_000_000,
            gas_limit: 50_000,
            signature: vec![0u8; 64],
            slot: SlotId(0),
        }
    }

    /// Deterministik seed'den gerçek ECDSA imzalı test TX'i oluştur.
    ///
    /// `seed_byte`: [0u8..255] — keypair türetmek için tek byte seed
    /// Sender adresi otomatik türetilir — test state'inde bu adres funded olmalı.
    /// Dönen `sender` değerini state'i fund etmek için kullan.
    ///
    /// Not: Dış test crate'leri için de erişilebilir (nova-security-audit gibi).
    pub fn signed_for_test(
        seed_byte: u8,
        to: Address,
        amount: u128,
        nonce: u64,
    ) -> (Self, Address) {
        use nova_crypto::signatures::ecdsa::{EcdsaKeypair, public_key_to_address};
        use nova_primitives::crypto::Hash;

        let mut seed = [0u8; 32];
        seed[0] = seed_byte;
        let kp = EcdsaKeypair::from_secret_bytes(&seed).expect("geçerli seed");
        let pk = kp.public_key().expect("pubkey");
        let sender = public_key_to_address(&pk);

        let mut tx = Self {
            sender,
            nonce,
            kind: TransactionKind::Transfer { to, amount },
            max_fee_per_gas: 1_000_000_000,
            max_priority_fee_per_gas: 100_000_000,
            gas_limit: 21_000,
            signature: Vec::new(),
            slot: SlotId(0),
        };

        let hash_bytes = compute_tx_hash(&tx);
        let hash = Hash::from_bytes(hash_bytes);
        let sig = kp.sign_hash(&hash).expect("sign");
        tx.signature = sig.as_bytes().to_vec();
        (tx, sender)
    }

    /// İşlemin base fee'si ile gerçek ücretini hesapla
    pub fn effective_fee(&self, base_fee: u128) -> u128 {
        let priority = self.max_priority_fee_per_gas.min(
            self.max_fee_per_gas.saturating_sub(base_fee)
        );
        (base_fee + priority) * self.gas_limit as u128
    }
}

/// SignedTransaction wrapper — imzalı TX
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTransaction {
    pub tx: Transaction,
    pub tx_hash: [u8; 32],
}

impl SignedTransaction {
    pub fn new(tx: Transaction) -> Self {
        let hash = compute_tx_hash(&tx);
        Self { tx, tx_hash: hash }
    }
}

/// TX hash hesabı — signature hariç tüm alanları kapsar.
///
/// BUG-5 düzeltmesi: Eski implementasyon `to` ve `amount` alanlarını
/// dahil etmiyordu; bu sayede farklı hedef/miktar transferleri aynı
/// hash üretebiliyordu. Şimdi tüm içerik alanları hashe dahil.
pub fn compute_tx_hash(tx: &Transaction) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    // Gönderen + replay protection
    hasher.update(tx.sender.as_bytes());
    hasher.update(&tx.nonce.to_le_bytes());
    hasher.update(&tx.slot.0.to_le_bytes());
    // Fee parametreleri
    hasher.update(&tx.gas_limit.to_le_bytes());
    hasher.update(&tx.max_fee_per_gas.to_le_bytes());
    hasher.update(&tx.max_priority_fee_per_gas.to_le_bytes());
    // İşlem tipine göre içerik (kind discriminant + alanlar)
    match &tx.kind {
        TransactionKind::Transfer { to, amount } => {
            hasher.update(&[0u8]);
            hasher.update(to.as_bytes());
            hasher.update(&amount.to_le_bytes());
        }
        TransactionKind::Stake { validator_id, amount } => {
            hasher.update(&[1u8]);
            hasher.update(&validator_id.0.to_le_bytes());
            hasher.update(&amount.to_le_bytes());
        }
        TransactionKind::Unstake { validator_id, amount } => {
            hasher.update(&[2u8]);
            hasher.update(&validator_id.0.to_le_bytes());
            hasher.update(&amount.to_le_bytes());
        }
        TransactionKind::WithdrawUnbonded { validator_id } => {
            hasher.update(&[3u8]);
            hasher.update(&validator_id.0.to_le_bytes());
        }
        TransactionKind::GovernanceVote { proposal_id, option } => {
            hasher.update(&[4u8]);
            hasher.update(&proposal_id.to_le_bytes());
            let opt_byte: u8 = match option {
                VoteOption::Yes => 0,
                VoteOption::No => 1,
                VoteOption::Abstain => 2,
                VoteOption::NoWithVeto => 3,
            };
            hasher.update(&[opt_byte]);
        }
        TransactionKind::GovernancePropose { title, description, payload, deposit } => {
            hasher.update(&[5u8]);
            // Uzunluk prefiksi: title + description karışımını önler
            // "ab"+"cd" ≠ "abcd"+"" — her alan kendi uzunluğuyla prefixlenir
            hasher.update(&(title.len() as u32).to_le_bytes());
            hasher.update(title.as_bytes());
            hasher.update(&(description.len() as u32).to_le_bytes());
            hasher.update(description.as_bytes());
            hasher.update(&deposit.to_le_bytes());
            // Payload içeriğini de dahil et
            match payload {
                ProposalPayload::ParameterChange { key, value } => {
                    hasher.update(&[0u8]);
                    hasher.update(&(key.len() as u32).to_le_bytes());
                    hasher.update(key.as_bytes());
                    hasher.update(&(value.len() as u32).to_le_bytes());
                    hasher.update(value.as_bytes());
                }
                ProposalPayload::TreasurySpend { recipient, amount } => {
                    hasher.update(&[1u8]);
                    hasher.update(recipient.as_bytes());
                    hasher.update(&amount.to_le_bytes());
                }
                ProposalPayload::UpgradeRuntime { new_version } => {
                    hasher.update(&[2u8]);
                    hasher.update(&new_version.to_le_bytes());
                }
                ProposalPayload::Text { content } => {
                    hasher.update(&[3u8]);
                    hasher.update(&(content.len() as u32).to_le_bytes());
                    hasher.update(content.as_bytes());
                }
            }
        }
        TransactionKind::RegisterValidator { commission_bps, min_self_delegation } => {
            hasher.update(&[6u8]);
            hasher.update(&commission_bps.to_le_bytes());
            hasher.update(&min_self_delegation.to_le_bytes());
        }
        TransactionKind::RuntimeUpgrade { new_version, activation_slot } => {
            hasher.update(&[7u8]);
            hasher.update(&new_version.to_le_bytes());
            hasher.update(&activation_slot.to_le_bytes());
        }
    }
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    #[test]
    fn test_mock_transfer() {
        let tx = Transaction::mock_transfer(addr(1), addr(2), 1_000, 0);
        assert_eq!(tx.nonce, 0);
        assert!(matches!(tx.kind, TransactionKind::Transfer { .. }));
    }

    #[test]
    fn test_effective_fee_normal() {
        let tx = Transaction::mock_transfer(addr(1), addr(2), 0, 0);
        // max_fee=1Gwei, base_fee=0.5Gwei → eff = (0.5+0.1)*21000
        let fee = tx.effective_fee(500_000_000);
        assert!(fee > 0);
    }

    #[test]
    fn test_effective_fee_capped_at_max() {
        let mut tx = Transaction::mock_transfer(addr(1), addr(2), 0, 0);
        tx.max_fee_per_gas = 1_000;
        tx.max_priority_fee_per_gas = 9_000; // priority > max_fee - base_fee → capped
        let fee = tx.effective_fee(900);
        // base_fee=900, priority = min(9000, 1000-900=100) = 100
        // effective = (900+100)*21000 = 21_000_000
        assert_eq!(fee, 21_000_000);
    }

    #[test]
    fn test_signed_transaction_hash_deterministic() {
        let tx = Transaction::mock_transfer(addr(1), addr(2), 500, 0);
        let stx1 = SignedTransaction::new(tx.clone());
        let stx2 = SignedTransaction::new(tx);
        assert_eq!(stx1.tx_hash, stx2.tx_hash);
    }

    #[test]
    fn test_different_txs_different_hash() {
        let tx1 = Transaction::mock_transfer(addr(1), addr(2), 500, 0);
        let tx2 = Transaction::mock_transfer(addr(1), addr(2), 500, 1); // nonce farklı
        let h1 = compute_tx_hash(&tx1);
        let h2 = compute_tx_hash(&tx2);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_vote_option_variants() {
        let opts = [VoteOption::Yes, VoteOption::No, VoteOption::Abstain, VoteOption::NoWithVeto];
        assert_eq!(opts.len(), 4);
    }

    #[test]
    fn test_mock_stake() {
        let tx = Transaction::mock_stake(addr(1), ValidatorId(1), 1_000_000, 0);
        assert!(matches!(tx.kind, TransactionKind::Stake { .. }));
    }

    /// TX hash uzunluk karışıklığı testi — GovernancePropose ikinci-preimage saldırısı
    ///
    /// Uzunluk prefiksi olmadan:
    ///   title="ab" + description="cd" hash aynısını üretirdi
    ///   title="abcd" + description="" ile birlikte.
    /// Uzunluk prefiksi ile bu iki TX farklı hash üretmeli.
    #[test]
    fn test_governance_propose_hash_no_length_confusion() {
        fn make_propose(title: &str, description: &str) -> Transaction {
            Transaction {
                sender: addr(1),
                nonce: 0,
                kind: TransactionKind::GovernancePropose {
                    title: title.to_string(),
                    description: description.to_string(),
                    payload: ProposalPayload::Text { content: "x".to_string() },
                    deposit: 1_000,
                },
                max_fee_per_gas: 1_000_000_000,
                max_priority_fee_per_gas: 100_000_000,
                gas_limit: 100_000,
                signature: vec![0u8; 64],
                slot: SlotId(0),
            }
        }

        let tx1 = make_propose("ab", "cd");
        let tx2 = make_propose("abcd", "");
        let tx3 = make_propose("a", "bcd");

        let h1 = compute_tx_hash(&tx1);
        let h2 = compute_tx_hash(&tx2);
        let h3 = compute_tx_hash(&tx3);

        // Tüm hash'ler birbirinden farklı olmalı
        assert_ne!(h1, h2, "title+desc karışıklığı: 'ab'+'cd' ≠ 'abcd'+''");
        assert_ne!(h1, h3, "title+desc karışıklığı: 'ab'+'cd' ≠ 'a'+'bcd'");
        assert_ne!(h2, h3, "title+desc karışıklığı: 'abcd'+'' ≠ 'a'+'bcd'");
    }

    /// ParameterChange payload hash uzunluk karışıklığı testi
    #[test]
    fn test_parameter_change_hash_no_length_confusion() {
        fn make_param_change(key: &str, value: &str) -> Transaction {
            Transaction {
                sender: addr(1),
                nonce: 0,
                kind: TransactionKind::GovernancePropose {
                    title: "T".to_string(),
                    description: "D".to_string(),
                    payload: ProposalPayload::ParameterChange {
                        key: key.to_string(),
                        value: value.to_string(),
                    },
                    deposit: 1_000,
                },
                max_fee_per_gas: 1_000_000_000,
                max_priority_fee_per_gas: 100_000_000,
                gas_limit: 100_000,
                signature: vec![0u8; 64],
                slot: SlotId(0),
            }
        }

        let h1 = compute_tx_hash(&make_param_change("abc", "def"));
        let h2 = compute_tx_hash(&make_param_change("abcdef", ""));
        let h3 = compute_tx_hash(&make_param_change("ab", "cdef"));

        assert_ne!(h1, h2);
        assert_ne!(h1, h3);
        assert_ne!(h2, h3);
    }
}
