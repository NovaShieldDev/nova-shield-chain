//! Mock implementasyonları — ağ, konsensüs, depo

pub mod clock;
pub mod network;
pub mod storage;

pub use clock::MockClock;
pub use network::MockNetwork;
pub use storage::MockStorage;
