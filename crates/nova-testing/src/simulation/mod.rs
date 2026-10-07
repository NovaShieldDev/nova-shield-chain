//! Nova Shield Chain Test Simülasyonu.
//!
//! ## Modüller
//!
//! - `network_sim`: Deterministik in-memory ağ (partition, drop, delay)
//! - `bft_sim`: Çok-node BFT konsensüs simülasyonu
//! - `chaos_sim`: Kaos mühendisliği senaryoları

pub mod bft_sim;
pub mod chaos_sim;
pub mod network_sim;

pub use bft_sim::{BftRoundResult, BftSimNode, BftSimulation, SafetyCheckResult, SimVote};
pub use chaos_sim::{ChaosReport, ChaosResult, ChaosSimulator};
pub use network_sim::{PartitionConfig, SimMessage, SimNetwork, SimNetworkStats};
