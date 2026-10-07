//! Turbine blok yayılım protokolü — Solana pattern

pub mod cluster;
pub mod erasure;
pub mod propagator;
pub mod shred;

pub use cluster::{ClusterNodes, WeightedNode, DATA_PLANE_FANOUT, MAX_NUM_TURBINE_HOPS};
pub use erasure::{
    CodeShred, ErasureBatchBuilder, ErasureConfig, ErasureDecoder, ErasureEncoder, ErasureError,
    FecSet, CODE_SHREDS_PER_FEC, DATA_SHREDS_PER_FEC, MIN_SHREDS_FOR_RECOVERY,
};
pub use propagator::{Deduper, RetransmitResult, RetransmitSlotStats, ShredPropagator};
pub use shred::{Shred, ShredBuilder, ShredId, ShredType, SHRED_DATA_PAYLOAD_SIZE};
