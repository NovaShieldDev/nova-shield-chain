//! Property-based testler — Nova Shield Chain invariantları (proptest).
//!
//! Her modül kritik bir subsystem'ın matematiksel özelliklerini ispat eder:
//!
//! - `zk_props`: ZK-SNARK commitment ve nullifier özellikleri
//! - `tx_props`: Transaction hash deterministizmi ve EIP-1559 fee modeli
//! - `auction_props`: Batch auction uniform price ve fiyat koruma garantileri
//! - `consensus_props`: BFT quorum eşiği, oy deterministizmi, safety/liveness

pub mod auction_props;
pub mod consensus_props;
pub mod tx_props;
pub mod zk_props;
