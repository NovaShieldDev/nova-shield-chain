//! Cross-crate entegrasyon testleri — FAZ 3 GÖREV 1
//!
//! ## Araştırma Kaynakları
//! - CoW Protocol (agent-25-MASTER-SENTEZ): Batch auction + CoW netting
//! - Solana Turbine (agent-09.md): Reed-Solomon FEC, shred pipeline
//! - Penumbra (03-PENUMBRA-NOVA-ARASTIRMA.md): ZK shielded tx, UTXO
//! - go-ethereum (06-go-ethereum.md): EVM execution, ABI, gas
//! - CGGMP21 (05-MPC-NOVA-ARASTIRMA.md): MPC threshold, key gen
//!
//! ## Test Kapsamı
//! Her test dosyası en az 2 farklı crate'i birlikte test eder.

pub mod consensus_bft;
pub mod full_pipeline;
pub mod guardian_freeze;
pub mod mempool_auction;
pub mod network_erasure;
pub mod runtime_vm;
pub mod stress_tests;
pub mod zk_shielded;
