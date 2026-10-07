//! state — Zincir state yönetimi

pub mod account;
pub mod cache;
pub mod locks;
pub mod merkle;

pub use account::{AccountState, GenesisAccount};
pub use cache::AccountStateCache;
pub use locks::{AccountLockGuard, AccountLocks, MAX_ACCOUNT_LOCKS};
pub use merkle::{account_leaf_hash, compute_accounts_root, internal_node_hash, merkle_root, EMPTY_ROOT};
