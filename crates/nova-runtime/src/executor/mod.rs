//! executor — Paralel TX yürütme motoru (Sealevel ilkesi)

pub mod parallel;
pub mod scheduler;

pub use parallel::{ParallelExecutor, TxResult};
pub use scheduler::{build_parallel_groups, derive_account_keys};
