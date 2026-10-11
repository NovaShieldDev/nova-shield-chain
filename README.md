# Nova Shield Chain

Layer 1 blockchain written in Rust. Mainnet went live in October 2026 with five validators. Testnet has been running since early 2026 with three validators.

Source is closed during this phase. This repository contains the whitepaper and technical documentation. Verified builds and audit results are available on request.

---

## Why this exists

Most chains treat protocol-level problems as UX problems and ship smart contract workarounds. That doesn't fix anything.

**Seed phrases.** Your entire wallet is one piece of paper. Lose it, get robbed, die — everything is gone. We moved key management into the protocol itself: threshold MPC at the account level, signing authority split across guardians, the full private key never assembled anywhere — not during signing, not in storage.

**Mempool front-running.** On every public chain, validators and MEV bots read your pending transaction before it executes. They insert themselves ahead of you and extract value. The only real fix is hiding transaction contents until the slot closes. We use Identity-Based Encryption over Ristretto255 for this.

**Rug pulls.** Pool creators can withdraw liquidity at any time on any existing DEX. Lock contracts can be paused, upgraded, or worked around. We enforce liquidity locks at the runtime level — the state machine blocks withdrawals, there is no override path.

**Asset inheritance.** Nobody has built a real on-chain inheritance system. We have a native four-phase deadman switch: heartbeat monitoring, configurable warning periods, a grace period where you can cancel, and staged transfer to designated heirs. It runs in the runtime, not in a smart contract.

---

## What is built

**Consensus** — BFT for per-block quorum certificates, GRANDPA for deterministic finalization, Snowball for fork resolution under network partition. Block time is 400ms. Epochs are 216,000 slots (~24 hours). Validator set rotates at epoch boundaries.

**ZK Privacy** — Groth16 proofs on BLS12-381 using the arkworks library. Three circuits: MintCircuit (729 constraints, transparent to shielded), TransferCircuit (shielded to shielded), BurnCircuit (shielded to transparent). Every proof is 192 bytes. Note commitments use MiMC sponge, the same hash function used inside the circuits, so the on-chain tree root and the circuit anchor are always identical.

**Threshold MPC** — FROST/CGGMP21 threshold signing. KeyGen in 3 rounds, offline PreSign in 2 rounds, online signing completes in approximately 10ms with no interaction between guardians required. All sensitive types implement ZeroizeOnDrop. Nonces are single-use enforced by AtomicBool.

**Encrypted Mempool** — IBE over Ristretto255. Transactions are encrypted under the epoch public key computed jointly by validators. No single node can decrypt a transaction before the slot closes. Bloom filter deduplication with 2²⁰ bits, reset every 7,500 slots. Gulf Stream pre-routing forwards transactions to the expected future leader with fanout 4.

**Inheritance** — Four phases: Dormant, Triggered, Grace, StagedRelease. Warning thresholds at approximately 30 days and 7 days. Guardians can request extensions but cannot block the account holder's own cancellation. Duress mode: a heartbeat with the duress flag silently records the event without triggering any visible response, allowing signaling under coercion. Up to 15 heirs, 10 conditions, 20 release steps.

**Cross-chain** — IBC (ICS-20) for Cosmos chains. Ethereum bridge via BEEFY light client with MMR root verification. All outbound bridge transactions use MPC threshold signing.

**Batch Auction** — Coincidence of Wants order matching with uniform clearing price. All orders in a slot execute at the same price. Arrival time and fee level do not affect execution quality.

**Native DEX** — Launching as a separate repository. Built on Nova's runtime primitives: commit-reveal MEV protection, runtime-enforced liquidity locks, and launch period safeguards against snipe bots.

---

## Numbers

```
Crates:        21  (L1 protocol)
Lines of Rust: ~100,000
Tests:         3,588 passing, 0 failing
Binary:        19 MB release build
Block time:    400 ms
Mainnet:       Chain ID 1984  — 5 validators — live
Testnet:       Chain ID 42001 — 3 validators — live
```

---

## Token

**NOVA** — 200,000,000 fixed supply. No new issuance mechanism exists in the protocol. 50% of every transaction fee is permanently burned — sustained usage is deflationary by design. 18 decimals.

TGE circulating supply: 28,000,000 NOVA (14%).

Distribution: Ecosystem & Community 22%, Team 19%, Investors 17% (hard cap across all rounds), Staking Rewards 16%, Treasury 12%, Public Sale 7%, Liquidity 7%.

Seed: $1.00/NOVA — $20M raise — FDV $200M
Series A: $2.14/NOVA — $30M raise — FDV $428M

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

Full technical whitepaper: [WHITEPAPER.md](./WHITEPAPER.md)

Covers consensus design, ZK circuit construction, MPC protocol, network architecture, the inheritance state machine, cross-chain bridges, and tokenomics in detail. Version 2.0, October 2026.

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
| Native DEX | Separate repository — Q4 2026 |
| TGE | Q2 2027 |

---

## Contact

**X:** [@NovaShieldChainOfficial](https://x.com/NovaShieldChainOfficial)

For verified builds, audit reports, or partnership inquiries: reach out on X.
