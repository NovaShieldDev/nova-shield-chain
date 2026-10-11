# Nova Shield DEX: A Protocol-Native Decentralized Exchange

**Version 1.0 — October 2026**

---

## Abstract

Nova Shield DEX is a decentralized exchange built natively on Nova Shield Chain. It does not have a separate token. All fees are paid and distributed in NOVA. The protocol is not EVM-compatible by design — the ecosystem is intentionally isolated.

Three problems exist on every other decentralized exchange: liquidity providers can rug pull at any time, validators and bots can front-run transactions, and newly launched tokens can be sniped within seconds of pool creation. Nova Shield DEX solves all three at the protocol level. Liquidity locks are enforced by the chain's state machine. Front-running is blocked by a mandatory commit-reveal scheme using slot-based timing. Launch period limits restrict how much any single address can buy in the first five minutes after pool creation.

The AMM uses the constant product formula (x×y=k). The swap fee is 0.30%, flat, no tiers. High-volume addresses pay 0.15% after 100 swaps per day. Pool creation costs $17.50 in NOVA, calculated at the current oracle price — not user-provided. Every mutating operation requires an Ed25519 signature; the server derives the caller's address from the public key and verifies it matches the request body.

---

## Table of Contents

1. Introduction
2. Problem Statement
3. System Architecture
4. AMM Design
5. Pool Creation
6. Liquidity Lock System
7. Fee Model
8. Fee Distribution & Creator Payouts
9. MEV Protection — Commit-Reveal Scheme
10. Launch Period Protection
11. Authentication Model
12. Persistence & Snapshots
13. API Reference
14. Security Audit Findings
15. Conclusion

---

## 1. Introduction

Most decentralized exchanges share the same failure modes. Liquidity can be pulled without warning. Transaction ordering is visible to validators before settlement. New token launches are immediately drained by automated bots with no restrictions. The typical response to each of these is an application-layer workaround: external lock contracts, MEV relays, or anti-bot fees. None of these address the root cause.

Nova Shield DEX takes a different approach. The underlying chain — Nova Shield Chain — provides the guarantees directly. Liquidity locks are enforced by the runtime state machine, not by a smart contract that can be paused or upgraded. MEV protection comes from the chain's slot model: a commit-reveal mechanism that uses 400ms block timing to ensure transaction contents cannot be front-run. Launch period limits are a core protocol rule, not an optional configuration.

The DEX itself is a Rust service that runs alongside the chain. It stores pool state, processes swaps, distributes fees, and enforces all the rules described above. The chain provides the cryptographic settlement layer.

---

## 2. Problem Statement

### 2.1 Rug Pulls

On Uniswap, Raydium, and PancakeSwap, the pool creator can withdraw all liquidity at any time. External lock services exist but are optional and can be circumvented — the contracts can be paused, upgraded, or the locked tokens can be transferred around. Every day, pools are drained and token holders are left with worthless assets.

Nova Shield DEX requires a lock duration at pool creation. The selected duration is written to chain state at creation time. The state machine rejects `remove_liquidity` calls before the lock expires. There is no administrative override, no governance bypass, and no upgrade path for this rule.

### 2.2 MEV — Front-Running and Sandwich Attacks

A front-run attack works because the attacker can see a pending transaction before it settles and insert their own transaction ahead of it. A sandwich attack adds a second transaction after the victim's as well, profiting from the price movement in both directions.

Both require reading the transaction contents before it executes. Nova Shield DEX prevents this with a mandatory commit-reveal scheme. Every swap requires two steps: first, a commitment hash is submitted; then, after at least one 400ms slot has passed, the full intent is revealed and the swap executes. The commitment hash binds the sender address, pool, direction, amount, and a replay nonce — an attacker who sees the hash cannot derive the intent or front-run the swap.

### 2.3 Launch Sniping

When a new token pool is created, automated bots monitor the chain and attempt to buy as large a position as possible in the first block. By the time any real user interacts with the pool, the price has already been pushed up significantly.

Nova Shield DEX enforces a launch period for the first five minutes (300,000ms) after pool creation. During this window, each address — including the pool creator — can buy at most 5% of the token reserve. This is tracked per address and enforced cumulatively: multiple small buys that total more than 5% are also rejected.

---

## 3. System Architecture

Nova Shield DEX consists of two crates:

| Crate | Responsibility |
|-------|---------------|
| `nova-dex` | Core library: AMM logic, pool registry, MEV mempool, authentication, snapshots |
| `nova-dex-server` | HTTP REST server (axum): exposes API endpoints, handles routing, CORS, TLS |

The server holds a single `AppState` containing a `PoolRegistry` and a `MevProtectedMempool`. Both are wrapped in `Arc` for concurrent access across async handlers. The `PoolRegistry` uses `parking_lot::RwLock` internally for lock-free reads and exclusive writes.

The server connects to Nova Shield Chain's L1 RPC endpoint for balance verification and settlement. The `NOVA_L1_URL` environment variable is required at startup — the server will not run without it.

```
┌────────────────────────────────────────────────────┐
│                  nova-dex-server                   │
│         (axum REST API, port 3000)                 │
├──────────────────────┬─────────────────────────────┤
│    PoolRegistry      │    MevProtectedMempool       │
│  (pool state, LP     │  (commit-reveal, nonce set,  │
│   positions, locks)  │   slot-based timing)         │
├──────────────────────┴─────────────────────────────┤
│                    nova-dex                        │
│  (AMM core, auth, snapshot, error types)           │
└────────────────────────────────────────────────────┘
                         │
                         │ L1 RPC (NOVA_L1_URL)
                         ▼
          Nova Shield Chain — Chain ID 1984
```

---

## 4. AMM Design

### 4.1 Constant Product Formula

The AMM uses the standard constant product invariant:

```
x × y = k
```

where `x` and `y` are the reserves of the two assets in a pool. Any swap that maintains or increases `k` is valid.

Given `amount_in` tokens of asset A and current reserves `(reserve_a, reserve_b)`:

```
fee              = amount_in × LP_FEE_BPS / 10,000
amount_in_net    = amount_in − fee
amount_out       = reserve_b × amount_in_net / (reserve_a + amount_in_net)
```

Reserves are updated after the swap:

```
reserve_a += amount_in
reserve_b -= amount_out
```

The invariant `reserve_a × reserve_b` is non-decreasing after every swap because the fee portion stays in the pool.

### 4.2 LP Tokens

When a pool is created, initial LP tokens are minted as the geometric mean of the initial reserves:

```
initial_lp = √(reserve_a × reserve_b)
```

When liquidity is added to an existing pool, LP tokens minted are proportional to the minimum of the two deposit ratios:

```
lp_a     = (amount_a × total_lp) / reserve_a
lp_b     = (amount_b × total_lp) / reserve_b
lp_minted = min(lp_a, lp_b)
```

Deposits that are not proportional to the current pool ratio are automatically adjusted. The excess of whichever asset is over-provided is not accepted — the actual amounts used are calculated from the pool's current ratio, and the caller receives the difference back.

When liquidity is removed:

```
amount_a_out = (lp_to_burn × reserve_a) / total_lp
amount_b_out = (lp_to_burn × reserve_b) / total_lp
```

LP positions are tracked per address per pool in a separate map.

### 4.3 Price Impact

Price impact increases with trade size relative to pool depth. A swap of `amount_in` against a pool with reserve `reserve_a` causes a price shift of approximately:

```
impact ≈ amount_in / (reserve_a + amount_in)
```

No price impact cap is enforced at the protocol level. Slippage protection is handled by the `min_amount_out` parameter — a swap is rejected if the computed `amount_out` is below the caller's minimum.

---

## 5. Pool Creation

### 5.1 Creation Fee

Creating a pool costs $17.50, paid in NOVA at the current oracle price:

```
CREATION_FEE_USD_CENTS = 1,750   ($17.50)

required_fee_nNOVA = (1,750 × 10^18) / nova_price_cents
```

The oracle price is set server-side and updated by an authorized process. It cannot be supplied by the user — a user-provided price would allow paying a fraction of the intended fee by submitting a falsely inflated price.

Pool creation is rejected if `creation_fee_paid < required_fee_nNOVA`.

### 5.2 Pool Identifier

The pool ID is derived deterministically:

```
pool_id = BLAKE3("nova-dex-pool-v1" || creator || asset_a || asset_b || created_at_ms)[0..16]
encoded as hex (32 characters)
```

### 5.3 Creation Flow

1. Select token pair
2. Select lock duration
3. Submit initial liquidity amounts
4. Pay creation fee (oracle-calculated)
5. Pool is created; creator receives initial LP tokens

---

## 6. Liquidity Lock System

### 6.1 Lock Options

| Option | Duration |
|--------|----------|
| One Day | 24 hours |
| Three Days | 72 hours |
| One Week | 168 hours |
| One Month | 720 hours |
| One Year | 8,760 hours |
| Permanent | No expiry |

The lock duration is set at pool creation and written to chain state immediately. It cannot be shortened after creation. It can only be extended — including upgraded to Permanent.

### 6.2 Enforcement

The `remove_liquidity` handler checks the lock state before processing any withdrawal:

```
if now_ms < pool.unlock_at_ms:
    reject with LockNotExpired { remaining_ms }
```

Permanent pools have `unlock_at_ms = None`. The check always fails for permanent pools — they can never be withdrawn.

Only the pool creator can extend the lock duration. The extension must set a later expiry than the current one. Any extension to Permanent is irreversible.

### 6.3 Transparency

Every pool's lock state is publicly readable via the API. The response includes:

- Lock duration (selected option)
- `unlock_at_ms` — exact timestamp when withdrawal becomes possible
- `created_at_ms` — pool creation timestamp
- Remaining time (computable from the above)
- Creator address

Permanent liquidity pools are identifiable by the absence of an `unlock_at_ms` value.

---

## 7. Fee Model

### 7.1 Swap Fee

The swap fee is 0.30% on every trade, applied to `amount_in` before the AMM formula:

```
LP_FEE_BPS = 30   (30 basis points = 0.30%)

fee             = amount_in × 30 / 10,000
amount_in_net   = amount_in − fee
```

There is one fee tier. No configuration, no pool-specific overrides.

### 7.2 Volume Discount

Addresses that execute 100 or more swaps within a calendar day (reset at 00:00 UTC) pay a reduced fee of 0.15%:

```
Standard:    LP_FEE_BPS = 30   (0.30%)
High-volume: LP_FEE_BPS = 15   (0.15%)
```

The bot swap counter (`bot_swap_count`) increments per pool, per successful swap. A swap that fails the slippage check does not increment the counter — failed swaps do not count toward the discount threshold.

### 7.3 Competitor Comparison

| DEX | Fee Structure |
|-----|--------------|
| Nova Shield | 0.30% flat, 0.15% after 100 swaps/day |
| Uniswap v3 | 0.01% / 0.05% / 0.30% / 1.00% — four tiers |
| Raydium | Variable by pool type |
| PancakeSwap | 0.01% / 0.05% / 0.25% / 1.00% — four tiers |

---

## 8. Fee Distribution & Creator Payouts

### 8.1 Distribution

Each swap fee is split:

```
Creator share:  20% of fee collected
Treasury share: 80% of fee collected
```

### 8.2 Payout Mechanics

Creator payouts are automated and require no manual claim. The payout engine runs once every 24 hours. It aggregates fees earned across all pools for each creator address and sends the net amount directly to the creator's wallet. The gas cost of the transfer is deducted from the payout amount.

Payouts are initiated exclusively by the `payout_authority` address — a server-side key stored in the `NOVA_DEX_PAYOUT_KEY` environment variable. Any caller that is not the payout authority is rejected outright.

### 8.3 Competitor Comparison

| DEX | Creator Revenue |
|-----|----------------|
| Nova Shield | 20% automatic, daily net transfer |
| Uniswap v3 | Manual claim, requires active position management |
| Raydium | No direct automatic payout |
| PancakeSwap | Manual, varies by pool type |

---

## 9. MEV Protection — Commit-Reveal Scheme

### 9.1 How Front-Running Works

A front-run attack requires knowing the contents of a pending transaction before it is included in a block. On chains with a public mempool, this is trivial — any validator or searcher can read pending transactions and submit a competing transaction with a higher fee.

### 9.2 Commit-Reveal Protocol

Every swap on Nova Shield DEX goes through a two-step process:

**Step 1 — Commit**

The caller computes:

```
intent_hash       = BLAKE3("nova-dex-intent-v1" || sender || pool_id || direction || amount_in || min_amount_out || nonce)
commitment_hash   = BLAKE3("nova-mev-commit-v1" || sender || intent_hash)
```

The `commitment_hash` is submitted to the `/v1/mev/commit` endpoint. The server records it alongside the sender address and current slot ID. The slot ID is returned to the caller.

**Step 2 — Reveal**

After at least one full slot (400ms) has passed, the caller submits the full `SwapIntent` to `/v1/mev/reveal`. The server:

1. Recomputes `commitment_hash` from the submitted intent
2. Looks up the stored commit entry
3. Verifies `commit.sender == intent.sender`
4. Verifies `current_slot > commit.slot_id` (at least one slot has elapsed)
5. Verifies the commit has not expired (`current_slot ≤ commit.slot_id + 10`)
6. Checks the nonce has not been used before
7. Removes the commit entry (single use)
8. Executes the swap

### 9.3 Security Properties

**Sender binding** — The commitment hash includes the sender address:

```
commitment_hash = BLAKE3("nova-mev-commit-v1" || sender || intent_hash)
```

Without sender binding, an attacker could observe a commitment hash submitted by another user and front-run the reveal by racing to submit the same hash from a different address. With sender binding, the same `intent_hash` submitted by a different `sender` produces a completely different `commitment_hash`, so the attacker's commit is a distinct entry that cannot be revealed against the victim's intent.

**Slot gap requirement** — The reveal is rejected if `current_slot <= commit.slot_id`. This enforces a minimum of one 400ms slot between commit and reveal. An attacker who observes the commit in the same slot cannot reveal in the same slot.

**Nonce replay protection** — Each `SwapIntent` carries a 32-byte nonce. Revealed nonces are stored in a hash set. Reusing a nonce causes the reveal to be rejected.

**Automatic cleanup** — Expired commits (older than 50 slots) are cleaned up on each new commit submission. This prevents the commit map from growing indefinitely.

**No direct swap endpoint** — The `/v1/swap` endpoint does not exist. Every swap must go through the commit-reveal flow. There is no bypass.

### 9.4 Slot Parameters

```
SLOT_DURATION_MS        = 400    (one Nova Shield Chain slot)
COMMIT_EXPIRY_SLOTS     = 10     (4 seconds — reveal window after commit)
CLEANUP_THRESHOLD_SLOTS = 50     (20 seconds — auto-cleanup threshold)
```

---

## 10. Launch Period Protection

### 10.1 The Snipe Problem

When a new liquidity pool is created, automated bots observe the creation transaction and immediately submit buy orders in the same block or the next. By the time any retail user sees the pool, a bot has already bought a significant percentage of the supply and driven the price up.

### 10.2 Protocol Rule

For the first five minutes (300,000ms) after pool creation, each address is limited to buying at most 5% of the token reserve:

```
LAUNCH_PERIOD_MS      = 300,000   (5 minutes)
LAUNCH_MAX_BUY_BPS    = 500       (5% of reserve_b)

max_buy = reserve_b × 500 / 10,000
```

The limit is cumulative and tracked per address:

```
if launch_buy_tracker[swapper] + estimated_out > max_buy:
    reject with LaunchPeriodLimitExceeded { max, requested }
```

The pool creator is subject to the same limit. No address is exempt.

Only buy-side swaps are affected (`zero_for_one = true`). Sell-side swaps are unrestricted during the launch period.

After 300,000ms have elapsed from `created_at_ms`, the launch period ends and all swap sizes are unrestricted.

### 10.3 Tracking

The `launch_buy_tracker` is a per-pool map of `address → cumulative_bought`. It is populated only during the launch period. After the period ends, new entries are not added and existing entries have no effect.

---

## 11. Authentication Model

### 11.1 Request Signing

Every mutating operation requires an Ed25519 signature. The request structure:

```
SignedRequest<T> {
    body:       T       // the actual request payload
    public_key: String  // Ed25519 public key, hex-encoded (64 chars)
    signature:  String  // Ed25519 signature, hex-encoded (128 chars)
}
```

The server computes the request hash:

```
request_hash = BLAKE3("nova-dex-auth-v1" || method || body_json_bytes)
```

Then verifies:

```
ed25519::verify(request_hash, signature, public_key)  →  must pass
address_from_pubkey(public_key) == body.address       →  must match
```

If either check fails, the request is rejected with `InvalidSignature` or `Unauthorized`.

### 11.2 Address Derivation

Addresses are derived from Ed25519 public keys:

```
address = BLAKE3(public_key_bytes)[0..20]
```

This is a one-way derivation. The server never trusts a caller-supplied address — it always derives the address from the verified public key and checks it matches what was submitted in the request body.

### 11.3 Why Ed25519

Ed25519 provides 128-bit security, fast verification, small key and signature sizes (32 and 64 bytes respectively), and is not vulnerable to the signature malleability issues that affect some ECDSA implementations.

---

## 12. Persistence & Snapshots

Pool state is held in memory and can be persisted to disk via the `RegistrySnapshot` mechanism.

### 12.1 Snapshot Structure

```
RegistrySnapshot {
    version:        u32   // schema version — currently 1
    snapshot_at_ms: u64   // timestamp of snapshot
    data:           Vec<u8> // serialized pool registry
}
```

### 12.2 Version Check

On restore, the version field is checked against the expected `SNAPSHOT_VERSION`. A snapshot from a different schema version is rejected:

```
if snap.version != SNAPSHOT_VERSION:
    return Err(Internal { "version mismatch" })
```

This prevents silent data corruption when loading a snapshot from an incompatible schema.

### 12.3 Error Handling

All snapshot operations return `DexResult<T>`. Corrupted or malformed snapshot data produces a descriptive error — the server does not panic on invalid input.

The `open_db` function similarly returns `DexResult<()>`. An empty path is an error, not a silent fallback.

---

## 13. API Reference

The REST API is served at `NOVA_DEX_ADDR` (default `0.0.0.0:3000`). All endpoints are under `/v1/`. Read operations do not require authentication. Write operations require a `SignedRequest` body.

### Pools

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/pools` | Create a new pool |
| `GET` | `/v1/pools` | List all pools |
| `GET` | `/v1/pools/:id` | Get pool by ID |

### Liquidity

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/pools/:id/liquidity` | Add liquidity |
| `DELETE` | `/v1/pools/:id/liquidity` | Remove liquidity (lock must be expired) |
| `POST` | `/v1/pools/:id/lock/extend` | Extend lock duration (creator only) |
| `GET` | `/v1/pools/:id/lp/:address` | Get LP position for address |

### MEV-Protected Swap

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/mev/commit` | Submit commitment hash |
| `POST` | `/v1/mev/reveal` | Reveal intent and execute swap |

There is no `/v1/swap` endpoint. Every swap must go through the commit-reveal flow.

### Admin

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/payouts/process` | Trigger creator fee payouts (authority only) |
| `GET` | `/health` | Server health check |

### Environment Variables

```
NOVA_DEX_ADDR              Server bind address (default: 0.0.0.0:3000)
NOVA_DEX_ALLOWED_ORIGINS   Comma-separated allowed CORS origins
NOVA_L1_URL                Nova Shield Chain L1 RPC endpoint (required)
NOVA_DEX_PAYOUT_KEY        Hex-encoded payout authority address
NOVA_DEX_TLS_CERT          TLS certificate path (production required)
NOVA_DEX_TLS_KEY           TLS key path (production required)
```

---

## 14. Security Audit Findings

The following findings were identified and resolved:

| Finding | Module | Fix |
|---------|--------|-----|
| `NOVA-RED-DEX-AUTH-001` | `auth.rs` | All mutating handlers require Ed25519 signature verification |
| `NOVA-RED-DEX-POOL-001` | `pool.rs` | Pool creation fee derived from server-side oracle, never from user input |
| `NOVA-RED-DEX-POOL-002` | `pool.rs` | Proportional liquidity deposit — excess returned to caller |
| `NOVA-RED-DEX-MEV-001` | `mev.rs` | Expired commits cleaned up automatically on each new submission |
| `NOVA-RED-DEX-MEV-002` | `mev.rs` | Direct swap endpoint removed — commit-reveal is mandatory |
| `NOVA-RED-DEX-MEV-005` | `mev.rs` | Reveal now executes the swap immediately rather than queuing it |
| `NOVA-RED-DEX-MEV-006` | `mev.rs` | Commitment hash includes sender address — griefing attack closed |
| `NOVA-RED-DEX-SWAP-001` | `pool.rs` | Bot swap counter increments only on successful swaps, not on slippage failures |
| `NOVA-RED-DEX-PAYOUT-001` | `pool.rs` | Payout endpoint restricted to authorized address only |
| `NOVA-RED-DEX-SNAPSHOT-001` | `snapshot.rs` | Schema version verified on restore |
| `NOVA-RED-DEX-DB-001` | `snapshot.rs` | Corrupt data returns error instead of panicking |
| `NOVA-RED-DEX-FEE-001` | `pool.rs` | Single fee implementation — duplicate dead code removed |
| `NOVA-RED-DEX-SERVER-003` | `nova-dex-server` | CORS restricted to configured origin list |
| `NOVA-RED-DEX-SERVER-004` | `nova-dex-server` | Server requires `NOVA_L1_URL` at startup |
| `NOVA-RED-DEX-TLS-001` | deployment | HTTPS required in production; reverse proxy must terminate TLS |
| `NOVA-RED-DEX-HEALTH-001` | `nova-dex-server` | `/health` endpoint does not expose pool count or treasury balance |

---

## 15. Conclusion

Nova Shield DEX is a decentralized exchange where the protocol enforces what other exchanges leave to convention. Liquidity locks are not a suggestion backed by an upgradeable contract — they are a state machine rule. MEV protection is not a relay service sitting between the user and the validator — it is a timing constraint embedded in the swap flow. Launch period limits are not an optional anti-bot fee — they are a hard cap enforced on every address including the pool creator.

The DEX runs on Nova Shield Chain (Chain ID 1984), which provides the settlement layer, cryptographic finality, and the runtime guarantees that make these properties possible.

Source is closed during this phase.

**Chain:** [nova-shield-chain](https://github.com/NovaShieldDev/nova-shield-chain)

*© 2026 Nova Shield Chain. All rights reserved.*
