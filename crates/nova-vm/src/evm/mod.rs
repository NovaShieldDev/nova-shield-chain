//! EVM uyumluluk katmanı (§9.1)

pub mod bridge;
pub mod compat;
pub mod precompiles;

pub use bridge::WasmEvmBridge;
pub use compat::{EvmCallResult, EvmCompatLayer};
pub use precompiles::{NovaPrecompile, PrecompileRegistry};
