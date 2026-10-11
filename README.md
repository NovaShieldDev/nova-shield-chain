# Nova Shield Chain

A Layer 1 blockchain written entirely in Rust. Mainnet has been running since late 2026 with five validators. Testnet with three validators has been live since earlier this year.

The source is closed during the pre-mainnet public phase. This repository holds the whitepaper and technical documentation. Verified builds and audit results are available on request.

---

## The problem this solves

Four specific problems that every other chain has either ignored or tried to solve at the wrong layer:

**Seed phrases.** Your entire wallet is one piece of paper. Lose it, get robbed, die — everything is gone. This is not a UX problem, it is a protocol design problem. Nova uses threshold MPC at the account level: your signing authority is split across multiple guardians and the complete private key is never assembled anywhere, not during signing, not during storage.

**Mempool front-running.** On every public chain, validators and MEV bots can read your pending transaction before it executes. They insert themselves before you, extract value, and you receive worse execution. The only real fix is an encrypted mempool where transaction contents are hidden until a slot closes. Nova uses Identity-Based Encryption over Ristretto255 for this.

**Rug pulls.** A pool creator can withdraw all liquidity at any time on any existing DEX. The lock period you see is a suggestion enforced by a smart contract that can be paused, upgraded, or worked around. Nova enforces liquidity lock at the runtime level — the state machine itself blocks withdrawals until the chosen duration expires. There is no override path.

**Asset inheritance.** Nobody has solved what happens to your crypto when you die. Nova has a native four-phase deadman switch: heartbeat monitoring, configurable warning periods, a grace period where you can cancel, and then staged transfer to designated beneficiaries. This runs in the runtime, not in a smart contract.

---

## What is actually built

**Consensus** — Three layers: BFT for per-block quorum certificates, GRANDPA for deterministic chain finalization, Snowball for fork resolution under network partition. Block time is 400ms. Epochs are 216,000 slots (~24 hours). Validator set rotates at epoch boundaries.

**Zero-Knowledge Privacy** — Groth16 proofs on BLS12-381 using the arkworks library. Three circuits: MintCircuit (729 constraints, transparent to shielded), TransferCircuit (shielded to shielded), BurnCircuit (shielded to transparent). Every proof is 192 bytes. Note commitments use MiMC sponge — the same hash function used inside the ZK circuits, so the on-chain tree root and the circuit anchor are always identical.

**Threshold MPC** — FROST/CGGMP21 threshold signing. KeyGen in 3 rounds, offline PreSign in 2 rounds, online signing completes in approximately 10ms with no interaction between guardians required. All sensitive types implement ZeroizeOnDrop. Nonces are single-use enforced by AtomicBool.

**Encrypted Mempool** — IBE over Ristretto255. Transactions are encrypted under the epoch public key computed jointly by validators. No single node can decrypt a transaction before the slot closes. Bloom filter deduplication with 2²⁰ bits, reset every 7,500 slots. Gulf Stream pre-routing forwards transactions to the expected future leader with fanout 4, so by the time a leader's slot arrives the transaction is already in their local cache.

**Network** — Kademlia DHT for peer discovery, Plumtree gossip with fanout 9, Turbine block propagation with Reed-Solomon erasure coding (32+32 shreds, 1,232 bytes each). P2P encryption via Noise XX: X25519 key exchange, HKDF-SHA256, ChaCha20-Poly1305 transport. CRDS gossip synchronizes validator state across the cluster.

**Inheritance** — Four phases: Dormant, Triggered, Grace, StagedRelease. Warning thresholds at approximately 30 days and 7 days. Guardians can request extensions but cannot block the account holder's own cancellation. Duress mode: sending a heartbeat with the duress flag silently records the event without triggering any response, allowing signaling under coercion. Up to 15 heirs, 10 conditions, 20 release steps.

**Cross-chain** — IBC (ICS-20) for Cosmos chains. Ethereum bridge via BEEFY light client with MMR root verification. All outbound bridge transactions use MPC threshold signing.

**Batch Auction** — Coincidence of Wants order matching with uniform clearing price. All orders in a slot execute at the same price. No transaction gets better execution by arriving earlier or paying a higher fee.

---

## Numbers

```
Crates:        23  (21 L1 protocol + 2 DEX)
Lines of Rust: ~100,000
Tests:         3,588 passing, 0 failing
Binary:        19MB release build
Block time:    400ms
Mainnet:       Chain ID 1984  — 5 validators — running
Testnet:       Chain ID 42001 — 3 validators — running
```

---

## Token

**NOVA** — 200,000,000 fixed supply. No new issuance mechanism exists in the protocol. 50% of every transaction fee is permanently burned, making sustained usage deflationary. 18 decimals.

TGE circulating supply: 28,000,000 NOVA (14%).

Distribution: Ecosystem & Community 22%, Team 19%, Investors 17% (hard cap across all rounds), Staking Rewards 16%, Treasury 12%, Public Sale 7%, Liquidity 7%.

Seed round: $1.00/NOVA, $20M raise, FDV $200M.
Series A: $2.14/NOVA, $30M raise, FDV $428M.

---

## Cryptographic primitives

| Purpose | Algorithm |
|---------|-----------|
| Transaction signatures | ECDSA secp256k1 |
| Consensus votes | BLS12-381 aggregate (96-byte QC) |
| Leader election | sr25519 VRF (schnorrkel) |
| ZK proofs | Groth16 / BLS12-381 |
| ZK-compatible hash | MiMC sponge, 91 rounds |
| General hash | BLAKE3 |
| P2P transport | Noise XX — X25519 + ChaCha20-Poly1305 |
| Mempool encryption | IBE over Ristretto255 |
| MPC share encryption | AES-256-GCM |
| Post-quantum signatures | Dilithium (NIST Level 2) |
| Threshold signing | FROST / CGGMP21 |

---

## Documentation

The full technical whitepaper is in this repository: [WHITEPAPER.md](./WHITEPAPER.md)

It covers consensus design, ZK circuit construction, MPC protocol, network architecture, the inheritance state machine, cross-chain bridges, and tokenomics in detail. Version 2.0, October 2026.

---

## Status

| Component | Status |
|-----------|--------|
| Core L1 protocol | Complete |
| ZK circuits (Groth16) | Complete |
| MPC threshold signing | Complete |
| Encrypted mempool (IBE) | Complete |
| Inheritance protocol | Complete |
| IBC + BEEFY bridges | Complete |
| Security audit suite | Complete — 3,588 tests |
| Mainnet (private, 5 validators) | Live |
| Testnet (3 validators) | Live |
| Public testnet | Q4 2026 |
| TGE | Q2 2027 |

---

## Contact

**X:** [@NovaShieldChainOfficial](https://x.com/NovaShieldChainOfficial)

For verified builds, audit reports, or partnership inquiries: contact via X.
