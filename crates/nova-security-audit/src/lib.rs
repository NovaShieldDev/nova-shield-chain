//! # nova-security-audit
//!
//! Nova Shield Chain güvenlik audit & PoC test suite — ETH/SOL/AAVE/DOT seviyesinde.
//!
//! ## Kapsam
//!
//! | Modül                    | Kategori           | Tehdit                              |
//! |--------------------------|--------------------|-------------------------------------|
//! | `rpc_security`           | Network / DoS      | RPC rate limit, flood, brute-force  |
//! | `consensus_attacks`      | Konsensüs          | Double-sign, quorum, fork choice    |
//! | `tx_security`            | Transaction        | Replay, imza bypass, nonce          |
//! | `economic_attacks`       | Ekonomi            | Slashing yeterliliği, validator     |
//! | `crypto_validation`      | Kriptografi        | BLS curve, ECDSA recovery, entropy  |
//! | `p2p_security`           | P2P Ağ             | Flood, Sybil, eclipse               |
//! | `invariants`             | Sistem Geneli      | State tutarlılığı, overflow         |
//! | `governance_attacks`     | Governance         | Whale, quorum bypass, veto, drain   |
//! | `zk_security`            | ZK Devreleri       | Proof soundness, nullifier, double-spend |
//! | `mpc_security`           | MPC/Threshold      | Shamir, Feldman VSS, P2P ECDH       |
//! | `vm_security`            | VM/WASM Sandbox    | Gas limit, infinite loop, bytecode  |
//! | `bridge_security`        | Cross-chain Bridge | IBC escrow, voucher, denom trace    |
//!
//! ## Nasıl Çalıştırılır
//!
//! ```bash
//! cargo test -p nova-security-audit -- --nocapture
//! cargo test -p nova-security-audit -- --test-threads=1  # sıralı (port çakışması yok)
//! ```

pub mod bridge_security;
pub mod consensus_attacks;
pub mod cross_module_attacks;
pub mod crypto_validation;
pub mod economic_attacks;
pub mod governance_attacks;
pub mod invariants;
pub mod mainnet_security;
pub mod mpc_security;
pub mod p2p_security;
pub mod rpc_security;
pub mod tx_security;
pub mod vm_security;
pub mod zk_security;
