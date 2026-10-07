//! Binary Merkle Tree — AccountStateCache için state root hesabı (§5.9)
//!
//! ## Tasarım
//!
//! Sıralı anahtar-değer çiftlerinden deterministik bir Merkle kökü üretir.
//! Ethereum Patricia Trie'nin basitleştirilmiş versiyonu:
//! - Adresler sıralanır (deterministizm garantisi)
//! - Her hesap için yaprak hash'i: `BLAKE3("nova-leaf:" || addr || nonce || balance)`
//! - Dahili düğümler: `BLAKE3("nova-node:" || sol || sağ)`
//! - Tek kalan öğe: son hash çiftlenir (standard Bitcoin/Ethereum pattern)
//! - Boş ağaç: `[0u8; 32]`
//!
//! ## Güvenlik özellikleri
//!
//! Domain separator prefix ("nova-leaf:", "nova-node:") → ikinci önceden görüntü
//! saldırılarını önler (aynı 32-byte veri hem yaprak hem iç düğüm gibi hash'lenemesin).
//!
//! ## Araştırma kaynakları
//!
//! - Ethereum Yellow Paper §4.1 — Modified Merkle Patricia Trie
//! - Bitcoin merkle tree — `ComputeMerkleRoot()` (src/consensus/merkle.cpp)
//! - Cosmos `iavl` — iAVL+ tree immutable hash
//! - Polkadot `sp-trie` — Substrate state trie

use nova_primitives::Address;

use crate::state::account::AccountState;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Boş Merkle kökü (sıfır hesap)
pub const EMPTY_ROOT: [u8; 32] = [0u8; 32];

/// Yaprak domain separator
const LEAF_PREFIX: &[u8] = b"nova-merkle-leaf:";

/// Dahili düğüm domain separator
const NODE_PREFIX: &[u8] = b"nova-merkle-node:";

// ─── Hesap yaprağı ────────────────────────────────────────────────────────────

/// Bir hesap için Merkle yaprak hash'i üret.
///
/// ```text
/// BLAKE3("nova-merkle-leaf:" || address[20] || nonce[8] || balance[16]
///        || code_hash_flag[1] || [code_hash[32]]?
///        || storage_root_flag[1] || [storage_root[32]]?)
/// ```
///
/// İkinci önceden görüntü saldırısına karşı domain separator zorunludur.
pub fn account_leaf_hash(addr: &Address, state: &AccountState) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(LEAF_PREFIX);
    hasher.update(addr.as_bytes());
    hasher.update(&state.nonce.to_le_bytes());
    hasher.update(&state.balance.to_le_bytes());
    // code_hash opsiyonel (EOA = 0x00, kontrat = 0x01 + hash)
    if let Some(h) = &state.code_hash {
        hasher.update(&[0x01]);
        hasher.update(h);
    } else {
        hasher.update(&[0x00]);
    }
    // storage_root opsiyonel
    if let Some(r) = &state.storage_root {
        hasher.update(&[0x01]);
        hasher.update(r);
    } else {
        hasher.update(&[0x00]);
    }
    *hasher.finalize().as_bytes()
}

// ─── İkili Merkle ağacı ───────────────────────────────────────────────────────

/// İki hash'ten iç düğüm üret.
///
/// `BLAKE3("nova-merkle-node:" || left[32] || right[32])`
pub fn internal_node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(NODE_PREFIX);
    hasher.update(left);
    hasher.update(right);
    *hasher.finalize().as_bytes()
}

/// Yaprak hash listesinden Merkle kökü hesapla.
///
/// # Algoritma
/// 1. Yaprak sayısı 0 → `EMPTY_ROOT`
/// 2. Yaprak sayısı 1 → tek yaprak
/// 3. Her seviyede çiftler halinde iç düğüm hash'i hesapla
/// 4. Tek kalan yaprak → kendisiyle çiftleştirilir (Bitcoin pattern)
/// 5. Tek kök kalana dek tekrarla
///
/// Zaman: O(n log n) &nbsp; Alan: O(n)
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return EMPTY_ROOT;
    }

    let mut level: Vec<[u8; 32]> = leaves.to_vec();

    while level.len() > 1 {
        let mut next: Vec<[u8; 32]> = Vec::with_capacity((level.len() + 1) / 2);
        let mut i = 0;
        while i < level.len() {
            let left = &level[i];
            let right = if i + 1 < level.len() {
                &level[i + 1]
            } else {
                // Tek öğe: kendisiyle çiftleştir (Bitcoin/Ethereum pattern)
                &level[i]
            };
            next.push(internal_node_hash(left, right));
            i += 2;
        }
        level = next;
    }

    level[0]
}

/// Sıralı `(Address, AccountState)` çiftlerinden state root hesapla.
///
/// # Akış
/// 1. Adreslere göre deterministik sıralama
/// 2. Her hesap için yaprak hash'i hesapla
/// 3. Merkle kökü hesapla
pub fn compute_accounts_root(accounts: &[(Address, AccountState)]) -> [u8; 32] {
    if accounts.is_empty() {
        return EMPTY_ROOT;
    }

    // Deterministik sıralama (adreslere göre)
    let mut sorted = accounts.to_vec();
    sorted.sort_by_key(|(addr, _)| addr.as_bytes().to_vec());

    let leaves: Vec<[u8; 32]> = sorted
        .iter()
        .map(|(addr, state)| account_leaf_hash(addr, state))
        .collect();

    merkle_root(&leaves)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::Address;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn state(balance: u128, nonce: u64) -> AccountState {
        AccountState { balance, nonce, code_hash: None, storage_root: None }
    }

    // ─── account_leaf_hash ────────────────────────────────────────────────

    #[test]
    fn test_leaf_hash_deterministic() {
        let a = addr(1);
        let s = state(1_000, 5);
        assert_eq!(account_leaf_hash(&a, &s), account_leaf_hash(&a, &s));
    }

    #[test]
    fn test_leaf_hash_different_balance() {
        let a = addr(1);
        let h1 = account_leaf_hash(&a, &state(1_000, 0));
        let h2 = account_leaf_hash(&a, &state(2_000, 0));
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_leaf_hash_different_nonce() {
        let a = addr(1);
        let h1 = account_leaf_hash(&a, &state(0, 1));
        let h2 = account_leaf_hash(&a, &state(0, 2));
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_leaf_hash_different_address() {
        let s = state(1_000, 0);
        let h1 = account_leaf_hash(&addr(1), &s);
        let h2 = account_leaf_hash(&addr(2), &s);
        assert_ne!(h1, h2);
    }

    // ─── merkle_root ──────────────────────────────────────────────────────

    #[test]
    fn test_empty_leaves_returns_empty_root() {
        assert_eq!(merkle_root(&[]), EMPTY_ROOT);
    }

    #[test]
    fn test_single_leaf_returns_itself() {
        let leaf = [42u8; 32];
        assert_eq!(merkle_root(&[leaf]), leaf);
    }

    #[test]
    fn test_two_leaves_different_from_each() {
        let l1 = [1u8; 32];
        let l2 = [2u8; 32];
        let root = merkle_root(&[l1, l2]);
        assert_ne!(root, l1);
        assert_ne!(root, l2);
        // Deterministik
        assert_eq!(root, merkle_root(&[l1, l2]));
    }

    #[test]
    fn test_leaf_order_affects_root() {
        let l1 = [1u8; 32];
        let l2 = [2u8; 32];
        let root_12 = merkle_root(&[l1, l2]);
        let root_21 = merkle_root(&[l2, l1]);
        assert_ne!(root_12, root_21, "sıra değişince kök değişmeli");
    }

    #[test]
    fn test_odd_leaf_count_deterministic() {
        let leaves: Vec<[u8; 32]> = (0..5u8).map(|i| [i; 32]).collect();
        let r1 = merkle_root(&leaves);
        let r2 = merkle_root(&leaves);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_adding_leaf_changes_root() {
        let l = [7u8; 32];
        let r1 = merkle_root(&[l]);
        let r2 = merkle_root(&[l, l]);
        assert_ne!(r1, r2, "yaprak eklenmesi kökü değiştirmeli");
    }

    // ─── compute_accounts_root ────────────────────────────────────────────

    #[test]
    fn test_empty_accounts_empty_root() {
        assert_eq!(compute_accounts_root(&[]), EMPTY_ROOT);
    }

    #[test]
    fn test_single_account_deterministic() {
        let accounts = vec![(addr(1), state(1_000, 0))];
        let r1 = compute_accounts_root(&accounts);
        let r2 = compute_accounts_root(&accounts);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_account_order_independent() {
        let a1 = (addr(1), state(1_000, 0));
        let a2 = (addr(2), state(2_000, 1));
        let r1 = compute_accounts_root(&[a1.clone(), a2.clone()]);
        let r2 = compute_accounts_root(&[a2, a1]);
        // Sıralandığından her iki yönde aynı kök
        assert_eq!(r1, r2, "sıra bağımsız kök garantisi");
    }

    #[test]
    fn test_balance_change_changes_root() {
        let r1 = compute_accounts_root(&[(addr(1), state(1_000, 0))]);
        let r2 = compute_accounts_root(&[(addr(1), state(9_999, 0))]);
        assert_ne!(r1, r2);
    }

    #[test]
    fn test_adding_account_changes_root() {
        let a1 = (addr(1), state(1_000, 0));
        let a2 = (addr(2), state(2_000, 0));
        let r1 = compute_accounts_root(&[a1.clone()]);
        let r2 = compute_accounts_root(&[a1, a2]);
        assert_ne!(r1, r2);
    }

    #[test]
    fn test_large_account_set_deterministic() {
        let accounts: Vec<(Address, AccountState)> = (0u8..50)
            .map(|i| (addr(i), state(i as u128 * 1_000, i as u64)))
            .collect();
        let r1 = compute_accounts_root(&accounts);
        let r2 = compute_accounts_root(&accounts);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_domain_separation_leaf_vs_node() {
        // Bir yaprak hash'i ile iç düğüm hash'i asla çakışmamalı
        let leaf = account_leaf_hash(&addr(1), &state(0, 0));
        let node = internal_node_hash(&[0u8; 32], &[0u8; 32]);
        assert_ne!(leaf, node, "domain separator etkili olmalı");
    }
}
