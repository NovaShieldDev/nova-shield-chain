//! Bitcoin modülü — MPC tabanlı Bitcoin varlık koruması

pub mod address;
pub mod tx;

pub use address::{derive_mpc_btc_address, build_p2wsh_script, build_multisig_redeem_script};
pub use tx::{BitcoinUtxo, BitcoinTxInput, BitcoinTxOutput, BitcoinTx, BitcoinTxBuilder};
