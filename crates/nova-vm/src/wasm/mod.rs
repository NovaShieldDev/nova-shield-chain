//! WASM execution katmanı

pub mod gas;
pub mod host_fns;
pub mod memory;
pub mod runtime;

pub use gas::{ComputeBudget, GasMeter};
pub use host_fns::{HostContext, HostFunctionRegistry, NovaHostFns};
pub use memory::LinearMemory;
pub use runtime::{ExecutionResult, WasmModule, WasmRuntime};
