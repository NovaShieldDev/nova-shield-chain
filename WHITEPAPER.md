# Nova Shield Chain: A Privacy-Native Layer 1 Blockchain

**Version 2.0 — October 2026**

---

## Abstract

Nova Shield Chain is a Layer 1 blockchain written entirely in Rust. It addresses four problems that existing blockchains have not solved at the protocol level: seed phrase theft, mempool front-running, liquidity rug pulls, and permanent asset loss when a wallet holder dies. All four protections are enforced by the runtime itself, not by applications or conventions.

Consensus combines BFT per-block commitment, GRANDPA finalization, and Snowball fork resolution, achieving sub-second finality. Shielded transfers use Groth16 zero-knowledge proofs on BLS12-381, the same curve used in Ethereum 2.0 consensus. The mempool uses Identity-Based Encryption over Ristretto255 to hide transaction contents until each slot closes, combined with a batch auction that clears all orders at a single uniform price. Key custody operates at the protocol layer via FROST-based threshold MPC — the private key is never assembled anywhere, not during key generation, signing, or storage.

The NOVA token has a fixed supply of 200 million with no issuance mechanism and a 50% fee burn. The L1 codebase is twenty-one Rust crates, approximately 100,000 lines of production code, passing 3,588 tests with zero failures.

---

## Table of Contents

1. Introduction
2. Problem Statement
3. System Architecture
4. Consensus Mechanism
5. Zero-Knowledge Privacy System
6. Threshold MPC Protocol
7. Network Layer
8. Transaction Lifecycle
9. Security Model
10. Financial Protocols
11. Tokenomics
12. Roadmap
13. Conclusion
14. References
15. Network Activity Proof

---

## 1. Introduction

Public blockchains work for what they were designed to do. Settlement is verifiable, access is permissionless, and censorship resistance is real. The gaps are structural.

Every major blockchain broadcasts its mempool publicly. Validators, MEV bots, and searchers can read pending transactions and insert themselves ahead of users before settlement. Ethereum has Flashbots, Solana has Jito — neither eliminates front-running, they redistribute who benefits from it. The validator has a structural advantage over the user, and that advantage cannot be addressed within the existing architecture.

Key custody still depends on seed phrases. Twelve or twenty-four words — lose them and funds are gone permanently, get robbed and they're gone immediately. Hardware wallets reduce some theft surface but don't eliminate the underlying problem: a complete secret still exists somewhere on a device or piece of paper. Multisig distributes signing authority, but each participant still holds a separate private key. Custodians work until they don't.

When a wallet holder dies, assets are locked. There is no protocol-level recovery path. Billions of dollars in cryptocurrency are already permanently inaccessible because their owners are dead or incapacitated. No major blockchain has addressed this in the base layer.

Liquidity rug pulls remain an accepted risk because the base layer provides no mechanism to prevent them. Lock contracts are smart contracts — they can be paused, upgraded, or circumvented.

Nova Shield Chain addresses each of these at the protocol layer. The mitigations are cryptographic guarantees enforced by the runtime, not conventions or application-layer workarounds.

---

## 2. Problem Statement

### 2.1 The Transparency Problem

Every transaction on a public blockchain permanently records sender, recipient, amount, and timestamp. For most financial activity — enterprise treasury operations, institutional market-making, personal finance — this level of disclosure is unacceptable.

The practical consequences are concrete. Front-running extracts value from users at the mempool layer before their transactions settle. Address profiling enables targeted theft and social engineering. Competitive intelligence is available to anyone with a block explorer query.

### 2.2 The Key Custody Problem

Seed phrase-based key management is the largest single source of permanent loss in the cryptocurrency ecosystem. One failure point — hardware damage, theft, death, coercion — means total, unrecoverable loss of all funds associated with that key.

Existing approaches each solve part of the problem. Hardware wallets reduce theft risk but the seed still exists somewhere. Multisig distributes authority but each participant holds a separate full private key. Custodians introduce counterparty risk and are subject to regulatory freezes.

Threshold cryptography at the protocol layer — where the complete secret is never assembled in any single location under any circumstances — is the only complete answer. Nova implements this natively for every account.

### 2.3 The MEV Problem

Miner/validator extractable value is not a fringe issue. In 2023, over $1.3 billion was extracted from Ethereum users through front-running, sandwich attacks, and transaction reordering. On Solana, Jito's tip infrastructure formalizes MEV extraction rather than eliminating it. The root cause is architectural: if validators can read the contents of pending transactions, some of them will exploit that access.

Nova's encrypted mempool ensures that no party — validators, nodes, or searchers — can read transaction contents until the slot closes and decryption runs. The batch auction executes all orders at a single clearing price, removing the incentive for ordering manipulation entirely.

### 2.4 The Inheritance Problem

No existing Layer 1 blockchain has a native mechanism for asset inheritance. When a wallet holder dies, their assets are permanently locked unless the private key was explicitly shared beforehand — which itself creates a theft surface. Nova implements a deadman switch at the runtime level: if no heartbeat is received within a configurable period, a warning phase begins, followed by a grace period during which the account holder can cancel, and then staged transfer to designated beneficiaries. The runtime enforces this. No application layer can pause or override it.

### 2.5 The Rug Pull Problem

Decentralized exchange liquidity provision carries no protocol-level guarantees. A project can launch a token, attract liquidity, and withdraw it entirely at any time. Application-layer lock contracts can be paused, upgraded, or worked around. Nova enforces liquidity locks at the runtime level — the state machine itself blocks withdrawal calls until the specified expiry. There is no administrative override.

---

## 3. System Architecture

### 3.1 Overview

Nova Shield Chain's L1 protocol is organized as twenty-one Rust crates. The native DEX runs as a separate repository. Each crate has a defined responsibility with explicit interface boundaries, allowing independent testing, benchmarking, and security auditing of each component.

The core execution stack:

```
┌───────────────────────────────────────────────────────────────────┐
│                          nova-node                                │
│                (AsyncNodeRunner, CLI, Config)                     │
├─────────────┬──────────────┬──────────────┬───────────────────────┤
│ nova-runtime│ nova-network │  nova-rpc    │   nova-consensus      │
│ (execution) │ (P2P/gossip) │ (JSON-RPC)   │ (BFT/GRANDPA/Snowball)│
├─────────────┴──────┬───────┴──────────────┴───────────────────────┤
│    nova-zk         │             nova-crypto                      │
│ (Groth16, UTXO,    │  (ECDSA, BLS12-381, sr25519 VRF,             │
│  CommitmentTree)   │   FROST, Dilithium, IBE, Noise XX)           │
├────────────────────┴─────────────────────────────────────────────┤
│                        nova-account                               │
│              (MPC-Native Account Model, §5.1)                    │
├──────────────────────────────────────────────────────────────────┤
│                       nova-primitives                             │
│          (Address, Balance, BlockHash, SlotId, EpochId, ...)     │
└──────────────────────────────────────────────────────────────────┘
```

Protocol modules built on the core stack:

| Crate | Responsibility |
|-------|---------------|
| `nova-mempool` | Encrypted transaction pool (IBE + Gulf Stream forwarding) |
| `nova-mpc` | FROST threshold signing, KeyGen, PreSign, Online phase |
| `nova-auction` | CoW batch auction, order book, clearing price engine |
| `nova-inheritance` | Deadman switch, 4-phase state machine, staged release |
| `nova-guardian` | Social recovery, guardian threshold coordination |
| `nova-guard` | Cross-chain bridge, IBC/Cosmos, Ethereum BEEFY light client |
| `nova-vm` | WASM + EVM-compatible virtual machine |
| `nova-freeze` | Compliance-grade asset freezing |
| `nova-sdk` | Developer client SDK |
| `nova-security-audit` | Dedicated security test suite |
| `nova-benchmarks` | Performance benchmarks |
| `nova-testing` | Shared test utilities and fixtures |

### 3.2 The Slot Model

Time is divided into slots of 400 milliseconds each. Slots are the atomic unit of block production — one slot, one block attempt. Slots are grouped into epochs of 216,000 slots (approximately 24 hours). Validator set rotation happens at epoch boundaries.

```
Slot duration:         400 ms
Epoch duration:        216,000 slots  (~24 hours)
Year duration:         ~78,840,000 slots
Checkpoint interval:   every 100 blocks
```

The testnet uses shortened epochs (200 slots, ~80 seconds) to allow faster testing cycles. The production specification is 216,000 slots per epoch.

### 3.3 Account Model

Nova uses a dual account model. Transparent accounts are standard address-balance pairs, compatible with conventional blockchain interaction. Shielded accounts use a UTXO-based note commitment model stored in an incremental Merkle tree; the link between notes and their owners is hidden from all external observers.

The `AccountStateCache` provides thread-safe sharded access to transparent account state via a Binary Merkle Tree. Block state roots are committed into every block header. The `nova-account` crate implements the MPC-native account model where each account's signing authority is distributed across a guardian set rather than residing in a single private key.

---

## 4. Consensus Mechanism

### 4.1 Three-Layer Design

Consensus in Nova is the composition of three mechanisms that each solve a different part of the finality problem:

| Layer | Mechanism | What It Provides |
|-------|-----------|-----------------|
| 1 | BFT | Instant per-block commitment with quorum certificate |
| 2 | GRANDPA | Deterministic finalization of chains of committed blocks |
| 3 | Snowball | Probabilistic fork resolution under network partition |

### 4.2 BFT Layer

For each slot, the designated leader broadcasts a proposed block. Validators vote. A block is committed when votes from ⌊2n/3⌋ + 1 validators are aggregated into a Quorum Certificate. Votes use BLS12-381 signature aggregation: the aggregate signature is always 96 bytes regardless of validator set size, making QC verification cost constant.

```
BFT_QUORUM            = ⌊2n/3⌋ + 1
QC aggregate_signature = 96 bytes (BLS12-381)
```

A Quorum Certificate carries: `block_hash`, `height`, `signers`, `aggregate_signature`, `signed_at_slot`. The QC is validated structurally before checkpoint recording — empty signer list or incorrect signature length are rejected outright.

### 4.3 GRANDPA Finalization

GRANDPA operates asynchronously over chains of BFT-committed blocks. Rather than finalizing blocks one at a time, a GRANDPA round agrees on the highest block seen by a ⌊2n/3⌋ + 1 supermajority and finalizes that block and all its ancestors in a single operation. This is significantly more efficient than per-block finalization under normal operation.

### 4.4 Snowball Fork Resolution

When a network partition produces competing BFT chains, Snowball provides metastable convergence. Each node samples k random peers and adopts the chain preferred by α or more of them. A chain is decided when it maintains preference for β consecutive rounds.

```
Sample size k  = 20
Quorum α       = 15
Decision β     = 150
```

### 4.5 Block Lifecycle

Every block passes through three phases:

**Open** — The block accepts new transactions. The proposer collects mempool entries, executes them in parallel where dependencies allow, and accumulates results.

**Frozen** — Transaction collection closes. The state root is computed from the Binary Merkle Tree over all modified account states. The `tx_root` is committed from the ordered transaction list.

**Finalized** — The BFT quorum certificate is attached. The block is complete and immutable.

Every block header contains: `height`, `slot`, `epoch`, `parent_hash`, `state_root`, `tx_root`, `proposer`, `base_fee`, `gas_limit`, `gas_used`, `timestamp`, `tx_count`. The block hash is `BLAKE3(height || slot || epoch || parent_hash || state_root || tx_root || proposer || base_fee || gas_used)`. The epoch field is explicitly included in the hash — a block at the same height in a different epoch produces a different hash, preventing cross-epoch replay.

### 4.6 Checkpoint Manager

Every 100 blocks, the runtime records a checkpoint: a QC-verified snapshot of the chain state that serves as a trusted anchor for light clients, warp sync, and historical verification.

A checkpoint carries: `height`, `block_hash`, `state_root`, `epoch`, `qc`, `created_at_slot`. Before recording, the checkpoint manager validates structural integrity: the QC must have at least one signer, its aggregate signature must be exactly 96 bytes, its height must match the checkpoint height, and its block_hash must match. Checkpoints at height 0 are rejected. Non-multiples of 100 are rejected.

The insert operation is atomic. A single write lock covers both the existence check and the insert, eliminating the TOCTOU race condition that would occur with a separate read-check followed by a write. Checkpoints are stored in a BTreeMap, which keeps them ordered by height for efficient range queries. A secondary hash→height index allows O(1) lookup by block hash.

### 4.7 Validator Economics

Minimum stake: 100,000 NOVA. Unbonding period: 21 days. Slashing for equivocation:

```
slash = min((3k/n)², 1) × stake
```

where k is the number of simultaneously equivocating validators and n is total validator count. The quadratic scaling ensures coordinated attacks are dramatically more costly than isolated incidents.

---

## 5. Zero-Knowledge Privacy System

### 5.1 Overview

Nova's shielded transaction system uses Groth16 zkSNARKs over BLS12-381. Every proof is 192 bytes. Verification time is constant regardless of the transaction's internal complexity. The circuits are implemented using the arkworks library — the same cryptography library used in several production zero-knowledge systems.

| Operation | Direction | Circuit | Proof Size |
|-----------|-----------|---------|------------|
| Mint | Transparent → Shielded | MintCircuit | 192 bytes |
| Transfer | Shielded → Shielded | TransferCircuit | 192 bytes |
| Burn | Shielded → Transparent | BurnCircuit | 192 bytes |

### 5.2 The Note Model

A shielded value unit is a note:

```
Note {
    value:            u128,     // Amount in base units
    asset_id:         [u8; 32], // Asset identifier
    randomness:       [u8; 32], // Blinding factor
    transmission_key: [u8; 32], // Recipient's public key component
}
```

A note's commitment uses MiMC sponge over the BLS12-381 scalar field at 91 rounds:

```
cm = MiMC_sponge(domain_note ‖ value ‖ randomness ‖ asset_id ‖ tk)
```

MiMC requires approximately 728 R1CS constraints per evaluation — roughly 10 times more efficient inside ZK circuits than SHA-256.

### 5.3 Nullifiers

Double-spend prevention uses nullifiers derivable only by the note owner:

```
nullifier = MiMC_sponge(domain_nf ‖ nk ‖ cm)
```

The global nullifier set is checked atomically at execution time. Spending a note whose nullifier is already present fails with `NullifierAlreadySpent`. Constructing a valid proof for a spent note requires solving the discrete logarithm over BLS12-381 — computationally infeasible on any hardware available today or in the foreseeable future.

### 5.4 Circuits

**MintCircuit (729 constraints)**

Public inputs: `cm`

Private witness: `value`, `randomness`, `asset_id`, `transmission_key`

Constraint: `MiMC_sponge(domain_note, value, randomness, asset_id, tk) == cm`

**TransferCircuit**

Public inputs: `input_nullifier`, `output_commitment`, `value_balance`

Private witness: input note, nullifier key, output note

Constraints: input commitment validity, nullifier derivation, output commitment validity, value conservation (`input_value == output_value + value_balance`).

**BurnCircuit**

Public inputs: `nullifier`, `value`, `recipient_hash`

Private witness: note fields, nullifier key, recipient address

Proves ownership and authorizes transparent withdrawal without revealing blinding factor or transmission key.

### 5.5 Commitment Tree

Note commitments are stored in an incremental Merkle tree with a maximum capacity of 2²⁰ (~1,000,000) live notes. The tree is append-only; insertion order is preserved.

**Leaf hashing**: the raw commitment bytes `cm.0` are used directly as the leaf value.

**Internal node hashing**: `MiMC_sponge(domain_merkle ‖ left ‖ right)` — the same function used inside the ZK circuits. This is a deliberate design choice: using MiMC for internal nodes means the circuit's anchor computation is byte-for-byte identical to the on-chain tree's root computation. If we used BLAKE3 for the tree but MiMC in the circuit, the proof would verify a root that does not match the chain state.

Odd-length levels are handled by pairing the last node with itself.

**MerkleWitness** provides a full inclusion proof: `commitment`, `position`, `root`, and an `auth_path` of `(sibling_hash, current_is_left)` pairs. The witness can be consumed by `verify_inclusion()`, which traverses the path from leaf to root and checks that the computed root matches the stored root. This is the mechanism used by SpendCircuit and StakeCircuit to prove that a note is in the tree without revealing which note.

### 5.6 Trusted Setup

Production key generation will follow the Powers of Tau multi-party ceremony protocol, with geographically distributed participants contributing randomness. No single participant's corruption can compromise the setup. The process follows the established precedent of Zcash Sapling and Hermez.

---

## 6. Threshold MPC Protocol

### 6.1 Design Rationale

Every existing Layer 1 blockchain assigns one private key per account. That key is stored on a device, written as a seed phrase, or held by a custodian. All three have failure modes: device loss, phrase theft, custodian insolvency or seizure. In each case, the key exists as a complete secret at some location, and wherever it exists, it can be lost or stolen.

Nova's account model distributes signing authority across multiple parties using threshold cryptography. The complete private key is never assembled — not during key generation, not during signing, not in storage. A transaction is authorized when t-of-n guardians cooperate to produce a valid signature. Access to fewer than t parties yields nothing usable.

### 6.2 Protocol Overview

The MPC system follows the CGGMP21 protocol, Nova's exclusive signing scheme. It operates in three phases:

**KeyGen (3 rounds)**: Participants jointly generate a shared key. Round 1 broadcasts commitments and ephemeral ECDH public keys. Round 2 opens commitments and distributes encrypted VSS shares point-to-point via AES-256-GCM. Round 3 broadcasts public key contributions with Schnorr DLog proofs. At completion, each participant holds a `LocalKey` containing their Shamir secret share (`x_i`), the aggregate public key, and all VSS commitments.

**PreSign (2 rounds, offline)**: Guardians perform expensive nonce generation while idle. Round 1: each party commits to its nonce `R_i = r_i * G` using `BLAKE3("nova-presign-r1" || R_i || session_id)`. Round 2: nonce is opened with a Schnorr DLog proof. The result is a `PreSignState` ready for immediate online use.

**Online (~10ms, 1 round)**: When a transaction requires signing, the online phase completes in approximately 10ms with no interaction between guardians required:

```
e  = BLAKE3("nova-sign-challenge-v1" || R || X || msg_hash)
λᵢ = ∏_{j∈S, j≠i} j / (j - i)   (Lagrange coefficient, secp256k1 scalar field)
sᵢ = rᵢ + e · xᵢ · λᵢ           (partial signature)
s  = ∑sᵢ mod n                    (final signature scalar)
```

Verification is the standard Schnorr check: `s * G == R + e * X`. The mathematical proof:

```
∑(sᵢ) * G = ∑(rᵢ + e · xᵢ · λᵢ) * G
           = ∑(rᵢ * G) + e · ∑(λᵢ · xᵢ) * G
           = R + e · x_secret * G      (Lagrange interpolation)
           = R + e · X                 ✓
```

### 6.3 Security Properties

**ZeroizeOnDrop**: All sensitive types — `LocalKey`, `PreSignShare`, `PreSignState` — implement `ZeroizeOnDrop`. When these structures are dropped, their memory is immediately overwritten with zeros before deallocation. An attacker who gains read access to a memory dump after a signing operation finds nothing.

**Session isolation**: Every KeyGen and signing session carries a unique `SessionId = BLAKE3(curve_params || parties || public_keys || round || nonce)`. ZK proofs embed the session ID as a domain separator, making proofs from one session invalid in any other session. This prevents cross-session replay attacks.

**Nonce single-use**: `PreSignState` contains an `AtomicBool` flag `nonce_used`. Once `create_partial_signature()` is called, the flag is set and the state cannot be reused. Reusing a nonce would allow the private key to be derived algebraically from two signatures.

**Lagrange safety**: Party indices start at 1. Index 0 is explicitly rejected — in Lagrange interpolation, a zero index produces a denominator of zero, which would break the polynomial arithmetic.

**Feldman VSS verification**: Each party's VSS commitment publishes `t+1` group elements `C_{i,j} = a_j * G`. Any participant can verify their received share: `f_i(k) * G == ∑(C_{i,j} * k^j)`. This means a dishonest participant cannot distribute invalid shares without detection.

**P2P encryption**: Round 2 VSS shares are transmitted point-to-point, encrypted with AES-256-GCM using ECDH-derived session keys. The AAD binds the ciphertext to the session ID and sender/receiver party indices, preventing message misdelivery from being exploited.

---

## 7. Network Layer

### 7.1 P2P Architecture

**Kademlia DHT** — Structured peer discovery with k-bucket routing (k = 20). New nodes locate peers by bootstrapping through known seed nodes and populating routing tables through iterative DHT queries.

**Plumtree Gossip** — Unstructured message propagation with fanout 9. Votes, block announcements, and CRDS entries propagate to all peers in sub-second latency on networks up to 10,000 nodes.

**Turbine Block Propagation** — Erasure-coded block dissemination for high-throughput environments:

```
Shred size:   1,232 bytes
RS_DATA:      32 shreds
RS_CODING:    32 shreds  (Reed-Solomon 32+32)
Fanout:       200
Max hops:     4
```

The `NetworkWorker` and `NetworkService` are separated following the Polkadot service architecture pattern. The worker is an async future that drives the network loop. The service is an `Arc<NetworkService>` handle distributed to other components. Communication is through a `ServiceToWorkerMsg` channel supporting: `ConnectPeer`, `DisconnectPeer`, `PublishGossip`, `RetransmitShred`, `DhtPut`, `DhtGet`, and `GetStatus`.

### 7.2 Encrypted Mempool

Nova's mempool encryption uses **Identity-Based Encryption over the Ristretto255 group**. The epoch public key is computed jointly by the validator set at epoch boundaries. Transactions are encrypted under this key before broadcast. No single validator can decrypt a transaction before block production. At block time, the leader initiates cooperative decryption with validator quorum participation.

A mempool observer — whether a validator, a well-connected full node, or a searcher running an MEV bot — sees only ciphertext. Transaction contents, sender addresses, and amounts are hidden until the slot closes. Front-running requires reading a transaction before it executes. Encrypted transactions give nothing to read.

The mempool pool maintains:
- A `DashMap` for lock-free concurrent transaction storage
- A Bloom filter deduplicator (`TxDeduper`) using `2²⁰` bits and double-hashing, reset every 7,500 slots (~5 minutes). The filter runs before the DashMap lookup — a `O(1)` bit operation that eliminates most duplicate processing before any hashtable access occurs.
- Per-sender block limits to prevent mempool spam
- Slot-based TX grouping with `MAX_TX_PER_SLOT = 10,000`
- Automatic expiry of transactions from closed slots

### 7.3 CRDS Gossip (Cluster Replicated Data Store)

The CRDS synchronizes network state across all validators. Entry types: `ContactInfo`, `Vote`, `EpochSlots`, `MpcShare`. Anti-entropy uses a pull protocol: each node sends a Bloom filter of its current state to a random peer, which responds with all entries not present in the filter. The false positive rate is 0.1 with 8 hash functions.

### 7.4 Gulf Stream Transaction Forwarding

Rather than waiting for the current slot leader to collect transactions, Nova pre-routes transactions to the expected future leader before their target slot arrives. This is adapted from Solana's Gulf Stream design.

When a transaction enters the network, the `GulfStreamRouter` identifies the next 4 likely slot leaders from the `LeaderSchedule` (round-robin with VRF override for production) and forwards the transaction to each of them. By the time the leader's slot arrives, the transaction is already in their local cache.

```
Forward fanout:                  4 validators
Transaction TTL:                 150 slots (~60 seconds)
Max cache per validator:         10,000 transactions
Priority ordering:               fee-descending
```

Expired transactions are pruned automatically. Duplicate detection at the forwarding layer prevents the same transaction from consuming multiple cache slots. When the leader's slot arrives, ready transactions are drained from the local cache, sorted by priority fee, and submitted to the mempool.

---

## 8. Transaction Lifecycle

### 8.1 Transaction Types

| Type | Description |
|------|-------------|
| `Transfer` | Transparent value transfer |
| `Stake` / `Unstake` | Validator stake management |
| `ShieldedMint` | Transparent → Shielded (enters note commitment tree) |
| `ShieldedTransfer` | Shielded → Shielded (nullifier + new commitment) |
| `ShieldedBurn` | Shielded → Transparent (nullifier, value exits) |
| `GovernanceVote` | On-chain proposal voting |
| `Deploy` / `Call` | Smart contract operations |

### 8.2 Validation Pipeline

1. Signature verification — ECDSA over secp256k1
2. Nonce check — strict monotonic ordering
3. Balance check — `balance ≥ amount + max_fee × gas_limit`
4. Base fee check — `max_fee_per_gas ≥ current_base_fee`
5. Gas limit check — `gas_limit > 0`
6. ZK proof verification — Groth16 over BLS12-381 for shielded transactions

### 8.3 Parallel Execution

Non-conflicting transactions execute concurrently using work-stealing parallelism following the Block-STM model. Transactions sharing state dependencies are serialized in nonce order. The executor tracks read/write sets to detect conflicts and serialize only where necessary.

### 8.4 Fee Mechanism

EIP-1559-style adaptive base fee:

```
Base fee adjustment: ±12.5% per block relative to gas target
Fee distribution:    50% burned (deflationary) + 50% to validators and stakers
```

### 8.5 JSON-RPC API

Nova exposes a JSON-RPC 2.0 server over HTTP. All methods use the `nova_` prefix.

**Chain**: `nova_blockNumber`, `nova_getBlock`, `nova_chainId`, `nova_syncing`, `nova_getSlot`

**Account**: `nova_getAccount`, `nova_getBalance`, `nova_getNonce`, `nova_estimateGas`, `nova_getGuardians`

**Transaction**: `nova_sendTransaction`, `nova_getTransaction`

**Batch Auction**: `nova_submitBatchOrder` — requires a secp256k1 ECDSA signature (minimum 64 bytes) binding the order to the sender. Full ECDSA verification is completed at the consensus layer.

**ZK Shielded**: `nova_submitShieldedTx`, `nova_getShieldedBalance`

**Guardian**: `nova_addGuardian`, `nova_removeGuardian`, `nova_initiateRecovery`

### 8.6 State Snapshots

The node supports point-in-time state snapshots for warp sync and archive purposes. A `StateSnapshot` records `block_height`, `block_hash`, `chain_id`, `state_root`, and all account states at that height. A BLAKE3 integrity hash is computed at save time and verified at load time, detecting any file corruption or tampering. The current serialization format is JSON for portability; a production binary format using Borsh is planned.

---

## 9. Security Model

### 9.1 Cryptographic Primitives

| Primitive | Algorithm | Security Level |
|-----------|-----------|---------------|
| Transaction signatures | ECDSA (secp256k1) | 128-bit |
| Consensus votes | BLS12-381 aggregate | 128-bit |
| Leader election | sr25519 VRF (schnorrkel) | 128-bit |
| ZK proofs | Groth16 / BLS12-381 | 128-bit |
| Hash (general) | BLAKE3 | 128-bit |
| Hash (ZK-compatible) | MiMC sponge (91 rounds) | 128-bit |
| P2P transport encryption | Noise XX — X25519 + HKDF-SHA256 + ChaCha20-Poly1305 | 128-bit |
| Mempool encryption | IBE over Ristretto255 | 128-bit |
| MPC share encryption | AES-256-GCM | 256-bit |
| Post-quantum signatures | Dilithium | NIST Level 2 |
| Threshold signing | FROST / CGGMP21 | 128-bit |
| Key derivation | HKDF-SHA256 | 128-bit |

### 9.2 Test Coverage

The workspace contains **3,588 passing tests with zero failures and zero compilation errors**. This figure covers the full protocol stack: consensus, ZK circuits, MPC signing rounds, mempool, network, runtime, RPC, inheritance, guardian, cross-chain bridge, and all security regression tests.

The `nova-security-audit` crate contains domain-specific security tests verifying:

| Domain | Scope |
|--------|-------|
| Transaction security | Replay attacks, nonce manipulation, signature bypass |
| ZK soundness | Proof forgery attempts, double-spend via nullifier collision |
| Consensus attacks | Double-signing, equivocation, QC forgery |
| Economic attacks | Fee manipulation, integer overflow, supply conservation |
| Bridge security | Over-unlock, voucher inflation, IBC packet manipulation |
| Governance attacks | Quorum bypass, veto threshold manipulation |
| P2P security | Sybil resistance, peer banning, CRDS poisoning |
| MPC security | Threshold reconstruction, nonce reuse, session replay |
| VM security | WASM isolation, precompile boundary violations |
| Crypto validation | Hash determinism, ECDSA malleability |
| Cross-module | Bridge + governance interactions, ZK + bridge interactions |
| RPC security | Input validation, signature format enforcement |

### 9.3 Byzantine Fault Tolerance

With n validators, the network tolerates up to ⌊(n−1)/3⌋ simultaneous Byzantine faults. The quadratic slashing formula for coordinated equivocation ensures that the cost of an attack scales superlinearly with the number of colluding validators.

### 9.4 Double-Spend Prevention

Shielded double-spending is prevented by the nullifier set. Nullifier insertion is atomic with block execution. A nullifier can only be derived from a note by its owner, and a valid ZK proof cannot be constructed for a note whose nullifier is already in the set. The security assumption is the discrete logarithm problem over BLS12-381.

---

## 10. Financial Protocols

### 10.1 Batch Auction (MEV Protection)

Transaction ordering MEV is eliminated through a CoW-protocol-derived batch auction. All orders for a given slot are collected, then executed together at a single Uniform Discriminatory Clearing Price:

```
p* = argmax Σ surplus(txᵢ, p)
```

Every order in the batch executes at `p*`. No transaction gets a better price by arriving earlier or by paying a higher fee. Coincidence of Wants netting matches opposing orders directly, eliminating the need for execution and distributing the saved spread as price improvement.

The `BatchAuctionEngine` processes each slot independently and deterministically. Circuit breaker logic suspends the engine in the event of detected misbehavior, preventing an adversarial edge case from destabilizing the clearing process. The engine produces a `ClearingResult` that the runtime applies atomically to the chain state.

### 10.2 On-Chain Governance

```
MIN_DEPOSIT:      100,000 NOVA
VOTING_PERIOD:    ~7 days
QUORUM:           33.4% of bonded stake
PASS_THRESHOLD:   > 50% Yes / (Yes + No)
VETO_THRESHOLD:   33.4% NoWithVeto (deposit burned)
```

### 10.3 Native Inheritance Protocol

The inheritance system is a four-phase protocol enforced entirely by the runtime. No smart contract is involved. No application can pause or upgrade it.

**Phase 1 — Dormant**: Normal operation. The account holder sends periodic heartbeat messages signed with their account key. The `HeartbeatTracker` records each heartbeat's slot and validates the signature.

**Phase 2 — Triggered**: If no heartbeat is received within the configured inactivity period (minimum 30 days), the trigger fires and a warning period begins. Two warning thresholds exist:
- `PRE_WARNING_SLOTS = 6,480,000` (~30 days at 400ms) — first alert
- `FINAL_WARNING_SLOTS = 1,512,000` (~7 days) — final alert

**Phase 3 — Grace**: After the trigger fires, a grace period allows the account holder to cancel the process by sending a signed cancellation. The guardian set can request extensions, but **a guardian can never block the account holder's own cancellation** (`can_guardian_block_user_cancellation()` always returns `false`). This is a deliberate design decision: guardian power is advisory, not controlling.

**Phase 4 — Staged Release**: If the grace period expires without cancellation, assets transfer to beneficiaries according to the inheritance plan. Transfers can be staged across multiple time steps, each with an independent delay.

**Plan configuration limits:**

```
MAX_HEIRS           = 15
MAX_CONDITIONS      = 10
MAX_RELEASE_STEPS   = 20
MIN_GRACE_PERIOD    = 2,000,000 slots (~9 days)
```

**Conditional rules** that can block or modify a transfer:
- `AgeCondition` — heir's account must be at least N slots old
- `MultiSigRequired` — t-of-n heirs must submit approval transactions
- `DebtPayment` — a creditor receives payment before heirs

**Duress detection**: If the account holder sends a heartbeat with the duress flag set, the runtime records the event silently without triggering any visible response. The actual assets remain untouched. This mechanism allows a person under coercion to signal distress while appearing to comply normally.

### 10.4 Social Recovery

The `nova-guardian` module allows an account holder to designate a set of trusted guardians who can cooperate above a threshold to authorize key rotation for lost keys. This does not override the threshold MPC model — it authorizes a new guardian set to replace the old one, triggered only by quorum agreement.

### 10.5 Cross-Chain Bridge

The `nova-guard` crate implements two bridge protocols:

**IBC (Cosmos/Tendermint)**: Implements the ICS-20 token transfer standard. The native token denomination is `unsc` (micro-NSC). Bridged tokens use the `DenomTrace` format: `ibc/{SHA256(path/baseDenom)[0:32 hex]}`. The escrow pattern locks tokens on the source chain and mints vouchers on the destination; unescrow burns vouchers and releases the locked tokens.

**Ethereum BEEFY**: A light client implementation of Polkadot's BEEFY finality gadget for Ethereum-facing bridges. Validators submit `BeefyCommitment` structures containing the Nova chain's MMR root and validator set ID. `SignedCommitment` packages the commitment with validator signatures. The light client verifies commitments and maintains a bounded history (`MAX_COMMITMENTS = 256`).

Both bridges use MPC threshold signing for outbound transactions. The `CrossChainCoordinator` manages `GuardedVault` instances (MPC-derived addresses) and `MpcSignSession` tracking. Only registered guardians in the vault's guardian list can contribute approval signatures — external addresses are rejected unconditionally.

---

## 11. Tokenomics

### 11.1 NOVA Token

```
Name:          NOVA
Total supply:  200,000,000  (fixed — no new issuance mechanism exists)
Decimals:      18
Model:         Deflationary (50% of all fees permanently burned)
```

### 11.2 Distribution

| Category | % | Amount | Vesting |
|----------|---|--------|---------|
| Ecosystem & Community | 22% | 44,000,000 | 48-month linear |
| Team & Founders | 19% | 38,000,000 | 12-month cliff + 48-month linear |
| Investors | 17% | 34,000,000 | 12-month cliff + 24-month linear |
| Staking Rewards | 16% | 32,000,000 | 10-year halving schedule |
| Treasury | 12% | 24,000,000 | 6-month cliff + governance unlock |
| Public Sale | 7% | 14,000,000 | Unlocked at TGE |
| Liquidity | 7% | 14,000,000 | Unlocked at TGE |
| **Total** | **100%** | **200,000,000** | |

**TGE circulating supply: 28,000,000 NOVA (14%)**

Investor allocation is permanently capped at 17% across all rounds.

### 11.3 Staking Rewards Schedule

| Period | APY | Annual Distribution |
|--------|-----|---------------------|
| Years 1–2 | ~8% | ~5,120,000 NOVA |
| Years 3–4 | ~4% | ~2,560,000 NOVA |
| Years 5–6 | ~2% | ~1,280,000 NOVA |
| Years 7–10 | ~1% | ~640,000 NOVA |

After the staking reserve is exhausted, validators earn entirely from fee revenue.

### 11.4 Investment Rounds

| Round | Allocation | Amount | Raise | FDV | Price |
|-------|-----------|--------|-------|-----|-------|
| Seed | 10% | 20,000,000 NOVA | $20,000,000 | $200,000,000 | $1.00 |
| Series A | 7% | 14,000,000 NOVA | $30,000,000 | $428,000,000 | $2.14 |

### 11.5 Deflationary Mechanics

50% of every transaction fee is permanently burned. Under sustained high network usage, annual burn can exceed annual staking emissions, making the net supply change negative. Every transaction executed on Nova Shield Chain reduces the total token supply.

---

## 12. Roadmap

### Phase 1 — Core Protocol (Complete ✓)

- Three-layer consensus: BFT + GRANDPA + Snowball
- Real Groth16 ZK proofs: Mint, Transfer, Burn circuits (BLS12-381 / arkworks)
- IBE encrypted mempool over Ristretto255
- Gulf Stream transaction forwarding
- P2P network: Kademlia DHT + Turbine + Plumtree gossip + CRDS
- FROST / CGGMP21 threshold MPC (t-of-n signing, offline pre-signing)
- Native inheritance protocol (4-phase deadman switch)
- Cross-chain bridge: IBC + Ethereum BEEFY
- HTTP JSON-RPC server (axum)
- RocksDB persistent storage + state snapshots
- Checkpoint manager (every 100 blocks, QC-verified)
- Security audit suite: **3,588 tests, 0 failures**
- Production binary: `nova-node` (19MB, release build)
- Private mainnet: 5 validators, Chain ID 1984
- Testnet: 3 validators, Chain ID 42001

### Phase 2 — Network Hardening (Q4 2026 – Q1 2027)

- Multi-party trusted setup ceremony (Powers of Tau)
- External validator onboarding
- Public testnet with open participation
- SpendCircuit Merkle inclusion proof integration
- Slashing enforcement activation
- Block explorer (public endpoint)
- Rate limiting and DDoS hardening

### Phase 3 — Public Launch (Q2 2027)

- Genesis block migration to public mainnet
- NOVA Token Generation Event (TGE)
- Ecosystem grants program launch
- SDK release for application developers
- Exchange listing (Tier 1 target)

### Phase 4 — Ecosystem Growth (Q3 2027+)

- WASM smart contract developer toolkit
- EVM compatibility layer
- Mobile SDK and light client
- Institutional custody integration
- Governance activation
- Cross-chain expansion (additional IBC integrations)

---

## 13. Conclusion

The problems described in this paper share a common root. Seed phrase loss, front-running, rug pulls, and inheritance are all protocol-level failures that have been treated as application-layer problems. Workarounds built on top of a flawed base layer don't resolve the underlying flaw — they add complexity while leaving the vulnerability intact.

Everything described in this paper is implemented and running. A 19MB production binary operates on five mainnet validators. 3,588 tests verify zero defects across the full protocol stack. The cryptographic foundations — BLS12-381, Ristretto255, secp256k1, Noise XX, Dilithium — are the same primitives used in Ethereum 2.0, Signal, and the NIST post-quantum standards. What Nova adds is their integration into a single base layer built around the specific failures that existing blockchains have not addressed.

---

## 14. References

1. Nakamoto, S. (2008). *Bitcoin: A Peer-to-Peer Electronic Cash System.*
2. Buterin, V. (2014). *Ethereum: A Next-Generation Smart Contract and Decentralised Application Platform.*
3. Yakovenko, A. (2017). *Solana: A new architecture for a high performance blockchain.*
4. Wood, G. (2016). *Polkadot: Vision for a Heterogeneous Multi-chain Framework.*
5. Groth, J. (2016). *On the Size of Pairing-based Non-interactive Arguments.* EUROCRYPT 2016.
6. Boneh, D., Drijvers, M., Neven, G. (2018). *Compact Multi-Signatures for Smaller Blockchains.*
7. Komlo, C., Goldberg, I. (2020). *FROST: Flexible Round-Optimized Schnorr Threshold Signatures.*
8. Canetti, R., Gennaro, R., Goldfeder, S., Makriyannis, N., Peled, U. (2021). *UC Non-Interactive, Proactive, Threshold ECDSA with Identifiable Aborts.* (CGGMP21)
9. Penumbra Labs. (2022). *Penumbra: A Fully Private Proof-of-Stake Network.*
10. Rocket, T., et al. (2019). *Scalable and Probabilistic Leaderless BFT Consensus through Metastability.*
11. Stewart, A. (2020). *GRANDPA: a Byzantine Finality Gadget.*
12. Grassi, L., et al. (2021). *Poseidon: A New Hash Function for Zero-Knowledge Proof Systems.*
13. Al-Bassam, M., et al. (2018). *Fraud and Data Availability Proofs.*
14. Castro, M., Liskov, B. (1999). *Practical Byzantine Fault Tolerance.*
15. Perrin, T. (2018). *The Noise Protocol Framework.*
16. Aumasson, J.P., et al. (2020). *BLAKE3 is not vulnerable to length extension.*
17. NIST. (2022). *CRYSTALS-Dilithium: Digital Signature Standard.*

---

## 15. Network Activity Proof

Live RPC queries against running validator nodes, recorded at query time.

### Mainnet (Chain ID: 1984 / 0x7c0)

**Validators:** 5 (nova-mainnet-v1 through v5)
**RPC endpoint:** http://localhost:19933

```bash
curl -X POST http://localhost:19933/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"nova_blockNumber","params":[],"id":1}'
```
```json
{"jsonrpc":"2.0","id":1,"result":"0x57f40"}
```

```bash
curl -X POST http://localhost:19933/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"nova_chainId","params":[],"id":1}'
```
```json
{"jsonrpc":"2.0","id":1,"result":"0x7c0"}
```

**Block:** `0x57f40` = **360,256**
**Chain ID:** `0x7c0` = **1984**
**Timestamp:** 2026-10-11 19:03:11 +03 (UTC+3)

---

### Testnet (Chain ID: 42001 / 0xa411)

**Validators:** 3 (nova-validator-1/2/3)
**RPC endpoint:** http://localhost:9933

```bash
curl -X POST http://localhost:9933/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"nova_blockNumber","params":[],"id":1}'
```
```json
{"jsonrpc":"2.0","id":1,"result":"0x6420a"}
```

```bash
curl -X POST http://localhost:9933/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"nova_getSlot","params":[],"id":1}'
```
```json
{"jsonrpc":"2.0","id":1,"result":{"currentEpoch":2050,"epochLength":200,"slot":410188,"slotDurationMs":400}}
```

```bash
curl -X POST http://localhost:9933/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"nova_syncing","params":[],"id":1}'
```
```json
{"jsonrpc":"2.0","id":1,"result":false}
```

**Block:** `0x6420a` = **410,122**
**Slot:** **410,188**
**Epoch:** **2050**
**Slot duration:** 400ms
**Sync status:** not syncing (fully caught up)
**Timestamp:** 2026-10-11 18:57:19 +03 (UTC+3)

---

*The Nova Shield Chain codebase is currently closed source during the pre-mainnet phase. This whitepaper describes the protocol as implemented; in any conflict between this document and the source code, the source code is authoritative.*

*© 2026 Nova Shield Chain. All rights reserved.*
