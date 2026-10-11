# CoW Protocol — Autopilot Core Analysis
# Agent 11 — run_loop.rs, solvable_orders.rs, winner_selection.rs, domain/fee/mod.rs, domain/fee/policy.rs, domain/auction/mod.rs, domain/settlement/mod.rs, domain/settlement/observer.rs, domain/settlement/trade/math.rs, domain/competition/mod.rs, domain/competition/bid.rs

**DEX Special Protocol: 3x Sensitivity Applied**
**Date:** 2026-07-22
**Source:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/libs/cow-protocol/crates/autopilot/src/`

---

## File Map

| File | Lines | Role |
|---|---|---|
| `run_loop.rs` | ~1100 | Central auction orchestrator — the beating heart |
| `solvable_orders.rs` | ~700 | Order filtering pipeline before each auction |
| `domain/competition/winner_selection.rs` | ~700 | Multi-winner arbitration algorithm |
| `domain/competition/mod.rs` | ~107 | Competition domain types |
| `domain/competition/bid.rs` | ~65 | Type-state bid lifecycle |
| `domain/fee/mod.rs` | ~705 | Protocol fee policy construction + partner fee capping |
| `domain/fee/policy.rs` | ~108 | Per-order-class policy application |
| `domain/auction/mod.rs` | ~97 | Auction + Price domain types |
| `domain/settlement/mod.rs` | ~420 | On-chain settlement decoding + association |
| `domain/settlement/observer.rs` | ~161 | Post-settlement indexing pipeline |
| `domain/settlement/trade/math.rs` | ~553 | Trade math: surplus, fee, custom prices |

---

## 1. `run_loop.rs` — The Auction Orchestrator

### Structs

```rust
pub struct Config {
    pub submission_deadline: u64,          // blocks past current to submit
    pub max_settlement_transaction_wait: Duration,
    pub min_solve_time: Duration,
    pub sync_solve_deadline_to_blockchain: Option<SlotConfig>,
    pub max_run_loop_delay: Duration,
    pub max_winners_per_auction: NonZeroUsize,
    pub max_solutions_per_solver: NonZeroUsize,
    pub enable_leader_lock: bool,
    pub compress_solve_request: bool,
}

pub struct SlotConfig {
    pub slot_length: Duration,         // e.g. 12s for mainnet
    pub tx_propagation_latency: Duration,
}

pub struct RunLoop {
    config: Config,
    eth: infra::Ethereum,
    persistence: infra::Persistence,
    drivers: Vec<Arc<infra::Driver>>,
    solvable_orders_cache: Arc<SolvableOrdersCache>,
    trusted_tokens: AutoUpdatingTokenList,
    probes: Probes,
    maintenance: MaintenanceSync,
    winner_selection: winner_selection::Arbitrator,
    wake_notify: Arc<tokio::sync::Notify>,
}
```

### Main Loop: `run_forever()`

```
while !shutdown {
    leader_lock_tracker.try_acquire().await   // distributed lock (optional)
    wake_notify.notified().await              // blocks until: new block OR new order
    start_block = update_caches().await       // warm up solvable_orders_cache
    if !leader: sleep 200ms; continue        // follower replicas do no auction work
    if let Some(auction) = next_auction() {
        single_run(auction).await
    }
}
```

**Key Design**: `Arc<tokio::sync::Notify>` — two background tasks write to it:
1. `spawn_block_listener` — triggers on every new block
2. `persistence.spawn_order_listener` — triggers on new order DB insert

This achieves ~block-time latency without polling.

### `single_run()` Pipeline

```
1. store_order_events(Ready)           // mark all auction orders as Ready
2. fetch_solutions(&auction).await     // parallel /solve to all drivers
3. if solutions.empty: return
4. winner_selection.arbitrate()        // rank + select winners
5. post_processing().await             // DB: save auction, solutions, competition, fee_policies
6. store_order_events(Executing)       // winning orders
7. store_order_events(Considered)      // non-winning orders
8. for each winner: start_settlement_execution() // spawn background task
```

### `fetch_solutions()` — Multi-Driver Solve

```rust
// 1. Build solve request (may compress with zstd)
let request = solve::Request::new(auction, trusted_tokens, deadline, compress).await;

// 2. Parallel call to ALL drivers simultaneously
let mut bids = join_all(drivers.iter().map(|d| self.solve(d, request.clone()))).await;

// 3. Filter: solution must come from driver's submission_address
bids.retain(|bid| bid.solution().solver() == bid.driver().submission_address);

// 4. Limit per solver
let mut counter = HashMap::new();
bids.retain(|bid| { counter[driver.name]++ <= max_solutions_per_solver });

// 5. Shuffle to randomize tie-breaking
bids.shuffle(&mut rand::rng());
```

**Security**: Solver identity validation at line 617-628. A driver cannot submit solutions attributed to a different solver address — it would be filtered here.

### `try_solve()` — Parallel Solve + Denylist Check

```rust
let (can_participate, response) = tokio::join!(
    eth.contracts().authenticator().isSolver(driver.submission_address),
    driver.solve(request),
);
```

- Both calls fire **simultaneously**
- If `isSolver` returns false → `SolverDenyListed` error
- If RPC call for `isSolver` fails → also treated as denylist (safe-fail)
- Timeout enforced via `tokio::time::timeout(time_until_deadline)`

### `settle()` — Race Between Driver + Block Monitor

```rust
let result = select!(
    wait_for_settlement_transaction() => tx found on-chain,
    driver.settle() => driver returned result,
);
```

`wait_for_settlement_transaction()`: loops block-by-block until deadline, querying DB for a settlement tx matching `(auction_id, solver, solution_uid)`. Returns `SettleError::Timeout` if deadline block is reached.

### `pick_solve_deadline_impl()` — Block-Aligned Timing

```
for delay_in_blocks in 1..10 {
    target = block_time + slot_length * delay - tx_propagation_latency
    if target >= now + min_solve_time:
        return target
}
return now + min_solve_time   // fallback: unconstrained
```

**Purpose**: Align solve deadline to just before the next block is expected to be mined. This maximizes chance that a winning solution gets included in the immediately upcoming block.

### Mathematical Formula — Solve Deadline

```
target(n) = T_block + n * T_slot - T_propagation
where:
  T_block = timestamp of current block
  n = number of blocks to delay (1..9)
  T_slot = block production period (12s mainnet)
  T_propagation = expected tx propagation delay
```

First `n` where `target(n) >= now + min_solve_time` is chosen.

### `post_processing()` — Competition Data Persistence

Saves to DB in parallel (`try_join!`):
1. `save_auction(auction, block_deadline)` — raw auction data
2. `save_solutions(auction_id, ranking.all())` — all solutions including filtered-out
3. `save_competition(competition)` — competition table with scores, rankings
4. `store_fee_policies(auction_id, fee_policies)` — per-order fee policies

The `Competition` struct:
```rust
Competition {
    auction_id,
    reference_scores: HashMap<Address, Score>,  // per-solver counterfactual score
    participants: HashSet<Address>,
    prices: HashMap<Address, U256>,
    block_deadline: u64,
    competition_simulation_block: u64,
    competition_table: SolverCompetitionDB,     // legacy API compatibility
}
```

Solutions are stored in **reverse order** (worst to best) for backward compatibility with the v1 solver competition API.

### Metrics (Prometheus)

| Metric | Type | Description |
|---|---|---|
| `runloop_auction` | Gauge | Last auction ID |
| `runloop_auction_winners` | Histogram | Number of winners (0-10) |
| `runloop_solve` | HistogramVec | Solve latency by driver+result |
| `runloop_settle` | HistogramVec | Settle latency (0-48s) |
| `runloop_solve_request_body_size` | Histogram | Solve request bytes |
| `runloop_service_maintenance_time` | Histogram | Block indexing overhead |
| `runloop_single_run_time` | Histogram | Full auction cycle time |

### DEX Implementation Note — For Nova Shield

The `RunLoop` architecture is directly transferable:

1. **Block-driven wakeup**: Use `tokio::sync::Notify` triggered by a block subscription. Avoids polling overhead entirely.
2. **Leader lock pattern**: `LeaderLockTracker` enables multi-replica deployment with one active auctioneer. Critical for HA in production.
3. **Parallel solver calls**: `join_all()` minimizes wall-clock time when calling multiple solver APIs.
4. **Block-aligned deadlines**: For Nova Shield's ~2s block time, `T_propagation` would be ~200ms, `T_slot` = 2000ms. The formula produces tight deadlines that maximize on-chain inclusion.
5. **Denylist check parallel to solve**: Zero extra latency for solver authentication.

---

## 2. `solvable_orders.rs` — Order Filtering Pipeline

### Struct: `SolvableOrdersCache`

```rust
pub struct SolvableOrdersCache {
    min_order_validity_period: Duration,   // skip orders expiring too soon
    persistence: infra::Persistence,
    banned_users: banned::Users,
    balance_fetcher: Arc<dyn BalanceFetching>,
    deny_listed_tokens: DenyListedTokens,
    cache: Mutex<Option<Inner>>,           // current cached auction
    native_price_estimator: Arc<NativePriceUpdater>,
    weth: Address,
    protocol_fees: domain::ProtocolFees,
    cow_amm_registry: cow_amm::Registry,
    native_price_timeout: Duration,
    settlement_contract: Address,
    disable_order_balance_filter: bool,
    wrapper_cache: app_data::WrapperCache, // LRU size 20,000
}
```

### `update()` — Full Filter Pipeline

```
tokio::join!(
    fetch_balances(queries),              // parallel: token balances per order
    filter_invalid_orders(orders),        // parallel: banned/presign/unsupported-token
    cow_amm_registry.amms(),              // parallel: CoW AMM registry
    fetch_in_flight_orders(block),        // parallel: orders already being settled
)
↓
filter_out_in_flight_orders              // remove in-flight (being settled)
↓
orders_with_balance                      // remove insufficient balance
↓
filter_dust_orders                       // remove 0-amount partial fills
↓
get_orders_with_native_prices            // remove orders with no price data
↓
prices.insert(weth, to_normalized_price(1.0))  // WETH price = 1.0 always
↓
protocol_fees.apply(order, quote, ...)   // attach fee policies to each order
↓
cache.lock() = Some(Inner { auction, solvable_orders })
```

**All 4 fetches run in parallel** — single `tokio::join!`.

### Incremental Cache Update

```rust
match &*lock {
    Some(cache) => persistence.solvable_orders_after(
        cache.solvable_orders.orders.clone(),
        cache.solvable_orders.quotes.clone(),
        cache.solvable_orders.fetched_from_db,   // checkpoint
        cache.solvable_orders.latest_settlement_block,
        min_valid_to,
    ),
    None => persistence.all_solvable_orders(min_valid_to),
}
```

- First call: full DB fetch
- Subsequent calls: incremental since last checkpoint
- Checkpoint rolled back 60 seconds: `orders.fetched_from_db -= 60s` to avoid race conditions at boundary

### Balance Filter Logic (`orders_with_balance`)

Priority:
1. **EIP-1271 orders**: always pass (pre-interactions may provide balance)
2. **Orders with wrappers** (`filter_bypass_orders`): pass — wrapper provides balance at settle time
3. **Settlement contract as receiver**: pass — flashloan orders self-fund
4. **Partially fillable + balance >= 1**: pass (any balance works)
5. Otherwise: `balance >= sell_amount + fee_amount`

Sort by `creation_date DESC` before filtering → newer orders preferred when balance is shared.

### CoW AMM Filter

```rust
// Skip AMM if any order for this AMM is already in-flight
if in_flight_owners.contains(cow_amm.address()) {
    return false;
}
// Skip AMM if price for any traded token is missing
cow_amm.traded_tokens().iter().all(|token| prices.contains_key(token))
```

Reason: CoW AMM rebalancing orders are state-specific. Multiple in-flight orders for the same AMM would revert.

### Filter Reasons Tracked

```rust
enum OrderFilterReason {
    BannedUser,
    DustOrder,
    InFlight,
    InsufficientBalance,
    InvalidSignature,     // presignature pending
    MissingNativePrice,
    UnsupportedToken,
}
```

All reasons are persisted to DB as `OrderEventLabel::Filtered` or `OrderEventLabel::Invalid`.

### DEX Implementation Note — For Nova Shield

1. **Parallel fetch pattern**: The `tokio::join!` of 4 async operations is the key to fast cache updates. Nova Shield should mirror this: balance fetch + AMM registry + price fetch + in-flight orders all parallel.
2. **Balance filter sophistication**: EIP-1271 bypass, wrapper bypass, flashloan bypass are all necessary for a production DEX. Nova Shield must handle pre-interactions that fund orders.
3. **60-second rollback**: Prevents missed orders at DB consistency boundary — subtle but important.
4. **WETH hardcoded at 1.0**: Native token price anchor eliminates circular pricing. Nova Shield's wrapped native token should also be hardcoded.
5. **Dust order filter**: Critical for partial fills — 0-amount fills from scaling would waste gas.

---

## 3. `domain/competition/winner_selection.rs` — Multi-Winner Arbitration

### Architecture

```
Arbitrator(winsel::Arbitrator)  // wraps external winner_selection crate
```

The `winner_selection` crate is a separate library crate that implements the core algorithm. The autopilot wraps it with domain types.

### `Arbitrator::arbitrate()` — 3-Phase Process

```
Phase 1: filter_unfair_solutions
  - For each order in each solution, find the best single-solution price for that order
  - If any order in a batch gets a worse price than its best individual price → filter entire batch

Phase 2: mark_winners
  - Select set of non-overlapping solutions that maximizes total score
  - Solutions sharing an order are incompatible (can't both win)
  - Up to max_winners solutions can win simultaneously if their order sets are disjoint

Phase 3: compute_reference_scores
  - For each winning solver S:
    reference_score(S) = total score of ALL winning solutions EXCLUDING S's solutions
  - This measures: how much did S contribute to overall surplus?
  - Used for solver rewards: reward = score(S) - reference_score(S)
```

### Key Invariant: Directional Clearing Prices

```
// From module comment:
// All orders selling the same token must get the same clearing price for that token.
// But orders BUYING the same token may have different (per-solution) prices.
// = "uniform directional clearing prices" (not "uniform clearing prices")
//
// This is more permissive than classic uniform clearing price (one price per token)
// but still maintains fairness within a batch.
```

### Score Type

```rust
pub struct Score(eth::Ether);
// Score = surplus + protocol_fees, denominated in native token (ETH/WETH)
// Score implements: Add, Sub, Saturating, CheckedSub, Ord, PartialOrd
```

### `Ranking` Struct

```rust
pub struct Ranking {
    filtered_out: Vec<Bid<Ranked>>,    // unfair solutions
    ranked: Vec<Bid<Ranked>>,          // winners first, then non-winners
    reference_scores: HashMap<Address, Score>,
}

impl Ranking {
    fn winners() -> impl Iterator<Item = &Bid<Ranked>>    // is_winner() == true
    fn non_winners() -> impl Iterator<Item = &Bid<Ranked>> // ranked but not winner
    fn all() -> impl Iterator<Item = &Bid<Ranked>>        // winners + non-winners + filtered_out
    fn enumerated() -> impl Iterator<Item = (usize, &Bid<Ranked>)>  // index = solution UID
}
```

### `From<&domain::Auction> for winsel::AuctionContext`

```rust
AuctionContext {
    fee_policies: HashMap<OrderUid, Vec<FeePolicy>>,     // per-order fee policies
    surplus_capturing_jit_order_owners: Vec<Address>,    // CoW AMM addresses
    native_prices: HashMap<Address, U256>,               // token → wei per 10^18 tokens
}
```

### Test Cases (Mathematical Examples)

**Test: `single_bid`**
- 1 solution, score = 200 (buy_amount - sell_amount in normalized units)
- Winner = Solution 1
- Reference score = 0 (no competition)

**Test: `compatible_bids`**
- "Best batch" covers Order 1 + Order 2, score = 200
- "Compatible batch" covers Order 3, score = 100
- Both win (disjoint order sets)
- Reference score of "Best batch solver" = 100 (what Compatible would win if Best weren't there)
- Reference score of "Compatible batch solver" = 200 (what Best would win if Compatible weren't there)

**Test: `incompatible_bids`**
- "Best batch" covers Order 1 + 2, score = 200
- "Compatible batch" covers only Order 1, score = 100
- Only "Best batch" wins (they share Order 1; Best wins because higher score)
- Reference score of "Best batch solver" = 100 (Compatible's score, which would win if Best absent)

**Test: `fairness_filtering`**
- "Unfair batch" covers Order 1 + 2, gives Order 1 → 1100 buy
- "Filtering batch" covers only Order 1, gives Order 1 → 1150 buy
- "Unfair batch" is FILTERED because if only Order 1 is considered, "Filtering batch" beats it
  → batching Order 2 alongside Order 1 must not harm Order 1's execution
- Only "Filtering batch" wins

### DEX Implementation Note — For Nova Shield

**This is the most important algorithmic finding for Nova Shield's DEX.**

1. **Multi-winner model**: Nova Shield should support multiple simultaneous "winners" when their order sets are disjoint. This increases throughput and capital efficiency.

2. **Directional clearing prices** (not uniform): Instead of enforcing one price per token, enforce one price per (token, direction). This allows more solutions to coexist.

3. **Reference score = counterfactual score**: The payment to each solver is `score - reference_score`. This is the Vickrey mechanism adapted for batch auctions — each solver is paid their marginal contribution. Solvers cannot game this by colluding.

4. **Fairness guarantee algorithm**:
```
for each solution B:
    for each order O in B:
        best_individual = max score from all solutions containing only O
        if O gets worse execution in B than best_individual:
            filter B
```

5. **Implementation complexity**: The `winner_selection` crate is separate — Nova Shield can study it in detail in `crates/winner-selection/`. The key data structure is likely a greedy or dynamic programming set cover with the fairness constraint.

---

## 4. `domain/competition/mod.rs` + `bid.rs` — Competition Domain Types

### `Solution` Struct (Autopilot Domain)

```rust
pub struct Solution {
    id: SolutionId,           // u64, provided by solver
    solver: Address,          // which solver
    orders: HashMap<OrderUid, TradedOrder>,
}

pub struct TradedOrder {
    pub side: order::Side,
    pub sell: eth::Asset,        // sell token + limit sell amount
    pub buy: eth::Asset,         // buy token + limit buy amount
    pub executed_sell: eth::TokenAmount,  // actual sell including fees
    pub executed_buy: eth::TokenAmount,   // actual buy received
}
```

**Note**: This is the autopilot-side `Solution` (simplified), distinct from the driver-side `Solution` (full calldata + interactions). The autopilot only needs order execution amounts to compute scores.

### `Score` Type

```rust
pub struct Score(eth::Ether);
// Implements Add, Sub, Saturating, CheckedSub, Ord
// Saturating to avoid panics during accumulation
// CheckedSub for reference score computation (can go negative conceptually)
```

### `Bid<State>` — Type-State Pattern

```rust
pub struct Bid<State = Ranked> {
    solution: Solution,
    driver: Arc<infra::Driver>,
    state: State,
}
// State can be: Unscored | Scored | Ranked
```

Transitions:
```
Bid<Unscored> → (score) → Bid<Scored> → (rank) → Bid<Ranked>
```

The `HasState` trait from `winner_selection::state` enables this:
```rust
impl<State> state::HasState for Bid<State> {
    type Next<NewState> = Bid<NewState>;
    fn with_state<NewState>(self, state: NewState) -> Bid<NewState>
}
```

**Compile-time safety**: Cannot call `is_winner()` on `Bid<Unscored>`. Cannot call score-related methods before scoring phase.

### DEX Implementation Note — For Nova Shield

The type-state pattern for bids should be copied directly. It prevents:
- Accessing a score before it's computed
- Marking winners before all scores are computed
- Processing solutions in wrong order

---

## 5. `domain/fee/mod.rs` — Protocol Fee Construction

### `ProtocolFees` Struct

```rust
pub struct ProtocolFees {
    fee_policies: Vec<ProtocolFee>,            // active policies
    max_partner_fee: FeeFactor,                // global cap on partner fees
    upcoming_fee_policies: Option<UpcomingProtocolFees>,  // scheduled policy change
    volume_fee_policy: VolumeFeePolicy,        // token-bucket overrides
}
```

### `ProtocolFee` — Per-Order Filtering

```rust
pub struct ProtocolFee {
    policy: policy::Policy,       // Surplus | PriceImprovement | Volume
    order_class: OrderClass,      // Market | Limit | Any
}
```

### `apply()` — Order → domain::Order with Fees

```rust
pub fn apply(&self, order, quote, surplus_capturing_jit_order_owners) -> domain::Order {
    let reference_quote = quote.unwrap_or(Quote { buy_amount: U256::ZERO, ... });
    let partner_fee = Self::get_partner_fee(order, &reference_quote, max_partner_fee);

    if surplus_capturing_jit_order_owners.contains(&order.metadata.owner) {
        // CoW AMM orders: only partner fees, no protocol fees
        return boundary::order::to_domain(order, partner_fee, quote);
    }

    self.apply_policies(order, reference_quote, partner_fee)
}
```

**CoW AMM exception**: JIT orders from CoW AMMs skip protocol fees. Only partner fees apply.

### `apply_policies()` — Time-Gated Policy Selection

```rust
let fee_policies = self.upcoming_fee_policies
    .as_ref()
    .filter(|upcoming| upcoming.effective_from_timestamp <= Utc::now())
    .map(|upcoming| &upcoming.fee_policies)
    .unwrap_or(&self.fee_policies);
```

Upcoming policies activate at a specific UTC timestamp. Zero-downtime fee schedule changes.

### `protocol_fee_into_policy()` — Order Class Gating

```rust
match (outside_market_price, &protocol_fee.order_class) {
    (_, OrderClass::Any)    => Some(&protocol_fee.policy),   // always apply
    (true, OrderClass::Limit)   => Some(&protocol_fee.policy),   // limit orders at worse price
    (false, OrderClass::Market) => Some(&protocol_fee.policy),   // market orders at good price
    _                      => None,   // not applicable
}
```

`outside_market_price`: order's limit price is worse than current market. Computed via `boundary::is_order_outside_market_price()`.

### Partner Fee: `get_partner_fee()` — Compounding Cap Algorithm

**The most complex fee calculation in the codebase.**

Partner fees are specified in `app_data.partnerFee` array in basis points (bps). Multiple fees compound multiplicatively.

**Cap Formula (Compounding)**:
```
Let fees = [f1, f2, f3, ...]
Let cap = max_partner_fee (e.g. 0.30)

accumulated = 0
for each fee fi:
    remaining = (1 + cap) / (1 + accumulated) - 1
    effective_fi = min(fi, remaining)
    accumulated += effective_fi
    apply effective_fi
```

**Why compounding**: Fees are applied sequentially. After fee f1 takes 10%, the remaining value is 90%. Fee f2 of 20% applies to 90% → effective 18%. Combined: `(1+0.1)*(1+0.2)-1 = 0.32`.

**Test case from code**:
- f1 = 10% (1000 bps), f2 = 25% (2500 bps), cap = 30%
- f1 = 10% → accumulated = 10%
- remaining for f2 = (1.30)/(1.10) - 1 = 18.18...%
- f2 capped at 18.18...% (instead of 25%)
- Total = (1.10)(1.1818) - 1 = 30% exactly

**bps → FeeFactor conversion**:
```rust
fn fee_factor_from_bps(bps: u64) -> FeeFactor {
    let bps = min(bps, MAX_BPS - 1)  // cap at 9999/10000 = 99.99%
    factor = bps as f64 / 10000.0
}
```

### Mathematical Formula — Partner Fee Cap

```
For n fees with basis points b1, b2, ..., bn and cap C:
  f_i = b_i / 10000
  effective_f_i = min(f_i, (1+C)/(1 + Σ effective_f_j for j<i) - 1)
  Σ applied = (1+effective_f_1)(1+effective_f_2)...(1+effective_f_n) - 1 ≤ C
```

### DEX Implementation Note — For Nova Shield

1. **Time-gated fee schedules**: `UpcomingProtocolFees` with an `effective_from_timestamp` enables zero-downtime fee changes without redeployment. Critical for production operations.

2. **Order class gating**: Market orders vs. Limit orders have different fee policies. Nova Shield should implement this distinction:
   - Market orders: fee when order is "at market" (competitive price)
   - Limit orders: fee when order is "out of market" (user got a better deal than expected)

3. **Partner fee compounding cap**: The compounding formula is essential for integrations (DEX aggregators, frontends). If a frontend charges 5% and another charges 25%, the global cap of 30% must account for their compounding interaction.

4. **CoW AMM fee exemption**: Protocol-owned liquidity positions should not pay protocol fees — they ARE the protocol.

---

## 6. `domain/fee/policy.rs` — Per-Order-Class Policy Application

```rust
impl Surplus {
    fn apply(&self, order: &boundary::Order) -> Option<domain::fee::Policy> {
        match order.metadata.class {
            Market | Liquidity => None,    // no surplus fee for market/liquidity orders
            Limit => Some(domain::fee::Policy::Surplus { factor, max_volume_factor }),
        }
    }
}

impl PriceImprovement {
    fn apply(&self, order, quote) -> Option<domain::fee::Policy> {
        match order.metadata.class {
            Market | Liquidity => None,
            Limit => Some(PriceImprovement { factor, max_volume_factor, quote }),
        }
    }
}

impl Volume {
    fn apply(&self, order, volume_fee_policy) -> Option<domain::fee::Policy> {
        match order.metadata.class {
            Market | Liquidity => None,
            Limit => {
                let factor = volume_fee_policy.get_applicable_volume_fee_factor(
                    order.data.buy_token,
                    order.data.sell_token,
                    Some(self.factor),
                )?;
                Some(Volume { factor })
            }
        }
    }
}
```

**Key**: `VolumeFeePolicy.get_applicable_volume_fee_factor()` handles token-bucket overrides. For specific token pairs (e.g., stable-to-stable), a different volume fee rate may apply.

---

## 7. `domain/auction/mod.rs` — Auction Domain Types

### `RawAuctionData` vs `Auction`

```rust
// Without ID — used before persistence assigns one
pub struct RawAuctionData {
    pub block: u64,
    pub orders: Vec<Order>,
    pub prices: Prices,
    pub surplus_capturing_jit_order_owners: Vec<Address>,
}

// With ID — used after persistence assigns one
pub struct Auction {
    pub id: Id,   // i64
    pub block: u64,
    pub orders: Vec<Order>,
    pub prices: Prices,
    pub surplus_capturing_jit_order_owners: Vec<Address>,
}
```

`Auction::PartialEq` excludes `id` — two auctions with same orders/prices/block are equal regardless of ID. This enables the run loop to skip re-running when auction content hasn't changed.

### `Price` — Token→ETH Conversion

```rust
pub struct Price(eth::Ether);

impl Price {
    const BASE: u128 = 10_u128.pow(18);  // 1 ETH in wei

    pub fn try_new(value: eth::Ether) -> Result<Self, InvalidPrice> {
        if value.0.is_zero() { Err(InvalidPrice) } else { Ok(Self(value)) }
    }

    pub fn in_eth(self, amount: eth::TokenAmount) -> eth::Ether {
        (amount.0 * self.0.0 / U256::from(Self::BASE)).into()
    }
}
```

### Mathematical Formula — `Price::in_eth()`

```
ETH_value = (token_amount * price_in_wei_per_1e18_tokens) / 10^18

Example: price = 0.001 ETH = 10^15 wei (per 10^18 tokens)
         amount = 10^18 tokens (1 full token)
         ETH_value = 10^18 * 10^15 / 10^18 = 10^15 wei = 0.001 ETH ✓
```

**Zero-price guard**: `try_new()` rejects zero prices. Division by zero would otherwise occur in score computation.

---

## 8. `domain/settlement/mod.rs` — Settlement Domain

### `Settlement` Struct

```rust
pub struct Settlement {
    gas: eth::Gas,
    gas_price: eth::EffectiveGasPrice,
    block: eth::BlockNo,
    solver: eth::Address,         // the authenticated solver (not tx.from)
    solution_uid: i64,            // winning solution UID from competition DB
    auction: Auction,
    trades: Vec<Trade>,
}
```

### `Settlement::new()` — Construction from On-Chain Transaction

```rust
pub async fn new(settled: Transaction, persistence, chain) -> Result<Self, Error> {
    let (auction, solver_winning_solutions) = tokio::try_join!(
        persistence.get_auction(settled.auction_id, &settled.trades),
        persistence.get_solver_winning_solutions(settled.auction_id, settled.solver),
    )?;

    // Environment check: reject settlements referencing very old auctions (~6h)
    if settled.block > auction.block + max_settlement_age(chain) {
        return Err(Error::WrongEnvironment);
    }

    // Match on-chain trades to winning solution using OrderMatchKey
    let solution_uid = find_winning_solution_uid(&solver_winning_solutions, &settled.trades)
        .ok_or(Error::InconsistentData(SolutionNotFound))?;

    ...
}
```

### `max_settlement_age()` — 6-Hour Environment Guard

```rust
fn max_settlement_age(chain: &Chain) -> u64 {
    const TARGET_AGE: u64 = 6 * 60 * 60 * 1000; // 6h in ms
    chain.blocks_in(TARGET_AGE).round() as u64
}
```

Purpose: Detect if a production settlement is being processed in the staging environment (or vice versa). Production lags staging by ~270 days of auction IDs. If the on-chain block is >6h of blocks after the auction's block, it's from the wrong environment.

### `find_winning_solution_uid()` — Trade Matching

```rust
fn trade_to_key(trade: &EncodedTrade) -> OrderMatchKey {
    // Key = (order_uid, token, executed_amount)
    // For non-partial-fill orders: executed = order limit amount (contract overrides it)
    let executed = if trade.partially_fillable { trade.executed.0 } else { order_amount };
    OrderMatchKey { uid: trade.uid, token, executed }
}
```

Matches by computing `HashSet<OrderMatchKey>` for both settled trades and winning solution orders. Equality of sets → found the matching solution.

### `summarize()` — Settlement Analytics

```rust
pub(crate) fn summarize(&self) -> SettlementMetrics<'_> {
    // Aggregates across all trades:
    let surplus = trades.sum(|t| t.surplus_in_ether(&auction.prices).unwrap_or(0))
    let fee = trades.sum(|t| t.fee_in_ether(&auction.prices).unwrap_or(0))
    let fee_breakdown: HashMap<OrderUid, FeeBreakdown>
    let jit_orders: Vec<&Jit>
}
```

Used for post-settlement analytics and circuit breaker monitoring.

### `SettlementEvent` — On-Chain Event

```rust
pub struct SettlementEvent {
    pub block: eth::BlockNo,
    pub log_index: u64,
    pub transaction: eth::TxId,
}
```

### DEX Implementation Note — For Nova Shield

1. **Trade matching by key**: The `OrderMatchKey { uid, token, executed }` approach is robust. It handles partial fills correctly by using the actual `executed` amount rather than the limit amount.

2. **Environment guard**: A 6-hour block-based window is a pragmatic solution to multi-environment deployments. Nova Shield needs something similar for testnet/mainnet cross-contamination.

3. **`InconsistentData` metric**: Track settlement/solution mismatches. Indicates bugs in solver or submission code.

---

## 9. `domain/settlement/observer.rs` — Settlement Post-Processing

### `Observer` — Async Retry Loop

```rust
pub async fn post_process_outstanding_settlement_transactions(&self) {
    const MAX_CONCURRENCY: usize = 10;
    futures::stream::iter(settlements)
        .for_each_concurrent(MAX_CONCURRENCY, |settlement| async move {
            retry_with_sleep(|| self.post_process_settlement(settlement)).await
        })
        .await;
}
```

- Processes up to 10 settlements concurrently
- Each settlement gets `retry_with_sleep()` — exponential backoff on transient failures
- `MissingCalldata` and `MissingAuctionId` → warning only, no retry (permanent failure)
- `Authentication` failure → error + retry (temporary RPC issue)

### `fetch_auction_data_for_transaction()` — Calldata Decoding

```
1. eth.transaction(tx_hash) → raw transaction
2. settlement::Transaction::try_new(tx, domain_separator, settlement_contract, authenticator)
   → decodes calldata, recovers solver identity
   → extracts auction_id from last 8 bytes of calldata
3. Settlement::new(transaction, persistence, chain)
   → fetches matching auction from DB
   → creates Settlement domain object
```

Error classification:
- `MissingCalldata`: settlement tx has no calldata → permanent error
- `MissingAuctionId`: calldata too short for auction ID → permanent error
- `Decoding/SignatureRecover/OrderUidRecover` → warning only
- `Authentication` → retryable (RPC state issue)

---

## 10. `domain/settlement/trade/math.rs` — Autopilot-Side Trade Math

### `Trade` Struct (Settlement Context)

```rust
pub struct Trade {
    pub uid: domain::OrderUid,
    pub sell: eth::Asset,      // sell token + limit sell amount
    pub buy: eth::Asset,       // buy token + limit buy amount
    pub side: order::Side,
    pub executed: order::TargetAmount,
    pub prices: Prices,        // { uniform: ClearingPrices, custom: ClearingPrices }
}

pub struct Prices {
    pub uniform: ClearingPrices,   // before fees (raw solver prices)
    pub custom: ClearingPrices,    // after fees (what user actually paid/received)
}

pub struct ClearingPrices {
    pub sell: U256,   // sell token price (how much buy per sell)
    pub buy: U256,    // buy token price (how much sell per buy)
}
```

**Note**: `ClearingPrices` here is the "inverted" format from the driver: `{sell: received_buy_amount, buy: paid_sell_amount}`. This is the settlement contract's native format.

### `surplus_over()` — Core Surplus Formula

**Sell order surplus** (denominated in buy token):
```
limit_buy = executed_sell * (limit_buy_amount / limit_sell_amount)  [ceil div]
bought = executed_sell * (prices.sell / prices.buy)                  [ceil div]
surplus = bought - limit_buy
```

**Buy order surplus** (denominated in sell token):
```
limit_sell = executed_buy * (limit_sell_amount / limit_buy_amount)
sold = executed_buy * (prices.buy / prices.sell)
surplus = limit_sell - sold
```

### `fee()` — Total Fee Calculation

```
fee = surplus_before_fee - surplus_after_fee
    = surplus_over(uniform_prices) - surplus_over(custom_prices)
```

Where:
- `uniform_prices`: solver's raw prices (no fees extracted)
- `custom_prices`: prices after fees extracted

### `surplus_fee()` — Protocol Fee as Surplus Fraction

```
// Input: surplus = surplus_after_fee, factor = target fraction of surplus_before_fee
// Derivation:
//   fee = surplus_before_fee * factor
//   surplus_after_fee = surplus_before_fee - fee
//   fee/surplus_after_fee = fee/(surplus_before_fee - fee) = factor/(1-factor)
//
fee = surplus_after_fee * factor/(1-factor)
```

Implemented as:
```rust
surplus.0.checked_mul_f64(factor / (1.0 - factor))
```

### `volume_fee()` — Protocol Fee as Volume Fraction

```
// Sell order: fee taken from buy token received
//   fee = traded_buy_before_fee * factor
//   traded_buy_after_fee = traded_buy_before_fee - fee
//   factor' = fee/traded_buy_after_fee = factor/(1-factor)
//   fee = traded_buy_after_fee * factor/(1-factor)

// Buy order: fee added to sell token required
//   fee = traded_sell_before_fee * factor
//   traded_sell_after_fee = traded_sell_before_fee + fee
//   factor' = fee/traded_sell_after_fee = factor/(1+factor)
//   fee = traded_sell_after_fee * factor/(1+factor)
```

Implemented:
```rust
let factor = match self.side {
    Sell => factor / (1.0 - factor),
    Buy  => factor / (1.0 + factor),
};
executed_in_surplus_token * factor
```

### `protocol_fees()` — Layered Fee Computation

```rust
// Iterate policies in REVERSE (last policy first)
for (i, policy) in policies.iter().enumerate().rev() {
    let fee = current_trade.protocol_fee(policy)?;
    total += fee;
    // Update custom prices to reflect cumulative fee for next iteration
    if i != 0 {
        current_trade.prices.custom = self.calculate_custom_prices(total)?;
    }
}
fees.reverse()  // restore original order
```

**Why reverse**: Each policy's fee is computed on the prices BEFORE that policy was applied. By computing in reverse and updating prices as we go backward, we correctly peel off each layer.

### `calculate_custom_prices()` — Price Adjustment for Fee

**Sell order** (fee in buy token):
```
new_custom.sell = buy_amount + protocol_fee   // denominator: user paid sell
new_custom.buy = sell_amount                  // numerator: user received buy + fee
```

**Buy order** (fee in sell token):
```
new_custom.sell = buy_amount                  // denominator: user received buy
new_custom.buy = sell_amount - protocol_fee   // numerator: user paid sell - fee
```

This encodes: "if we remove the protocol fee from the price, what prices did the solver actually offer?"

### `adjust_quote_to_order_limits()` — Quote vs Order Comparison

Handles the case where quote sell/buy amounts differ from order sell/buy amounts.

**Sell order adjustment**:
```
// Remove fee from quote buy amount (fee-adjusted quote)
quote_buy_net = quote.buy - (quote.fee * quote.buy / quote.sell)
// Scale to match order's sell amount
scaled_buy = quote_buy_net * order.sell / quote.sell
// Use the better of: order's limit buy OR scaled quote buy
buy_amount = max(order.buy, scaled_buy)
PriceLimits { sell: order.sell, buy: buy_amount }
```

**Buy order adjustment**:
```
// Add fee to get total sell cost from quote
quote_sell_total = quote.sell + quote.fee
// Scale to match order's buy amount
scaled_sell = quote_sell_total * order.buy / quote.buy
// Use the better of: order's limit sell OR scaled quote sell
sell_amount = min(order.sell, scaled_sell)
PriceLimits { sell: sell_amount, buy: order.buy }
```

**Purpose**: When computing `PriceImprovement` fee, we need to compare execution price against quote price. But the quote might have different absolute amounts than the order. This function creates comparable price limits.

### `surplus_in_ether()` / `fee_in_ether()`

```rust
pub fn surplus_in_ether(&self, prices: &auction::Prices) -> Result<eth::Ether, Error> {
    let surplus_amount = self.surplus_over_limit_price()?;
    let surplus_token = self.surplus_token();  // buy token for sells, sell token for buys
    let price = prices.get(&surplus_token).ok_or(Error::MissingPrice(surplus_token))?;
    Ok(price.in_eth(eth::TokenAmount(surplus_amount.0)))
}
```

Surplus token:
- Sell order → buy token (user receives more buy token than limit)
- Buy order → sell token (user spends less sell token than limit)

### DEX Implementation Note — For Nova Shield

**These formulas are the foundation of the DEX fee system.**

1. **Surplus fee formula** `factor/(1-factor)`: This is NOT a typo. The factor in the config is a fraction of surplus-BEFORE-fee. Since the trade already has fee baked in, you must use this adjusted factor to compute the fee correctly.

2. **Volume fee asymmetry** (`1-factor` for sell, `1+factor` for buy): Sell orders lose buy tokens (denominator decreases), buy orders gain sell token requirement (denominator increases). The asymmetry is mathematically necessary.

3. **Reverse iteration for layered fees**: If fee policy A is applied first in intent, then B, compute B first in code (because B's fee is computed against the price that already has A extracted). Nova Shield must implement this exact pattern for multi-policy fees.

4. **`checked_ceil_div`**: The settlement contract uses ceiling division for buy amounts. All off-chain math MUST match — use `checked_ceil_div` for buy amount computations to be consistent with on-chain behavior.

5. **`surplus_token`**: Always compute surplus in the token the user is trying to accumulate:
   - Sell order user: "give me as much Y as possible" → surplus in Y (buy token)
   - Buy order user: "use as little X as possible" → surplus in X (sell token)

---

## Overall Architecture Summary

### Full Auction Lifecycle

```
Block arrives
    ↓
wake_notify.notify_one()
    ↓
RunLoop::update_caches()
    ↓ tokio::join! (parallel):
    ├── fetch_balances()
    ├── filter_invalid_orders()      ← banned/presign/token-denylist
    ├── cow_amm_registry.amms()
    └── fetch_in_flight_orders()
    ↓
filter_out_in_flight_orders
    ↓
orders_with_balance (newest first)
    ↓
filter_dust_orders
    ↓
get_orders_with_native_prices
    ↓
protocol_fees.apply()               ← attach fee policies per order
    ↓
SolvableOrdersCache updated
    ↓
RunLoop::cut_auction()              ← get next auction ID from DB
    ↓
RunLoop::single_run()
    ↓
fetch_solutions()                   ← join_all(drivers.iter().map(solve))
    ↓ parallel:                     ← simultaneous: isSolver() + /solve
    ↓
filter by submission_address
    ↓
limit per solver
    ↓
shuffle (random tie-breaking)
    ↓
winner_selection.arbitrate()
    Phase 1: fairness filtering
    Phase 2: winner selection (max score, disjoint orders)
    Phase 3: reference score computation
    ↓
post_processing()                   ← try_join!(save_auction, save_solutions, save_competition, store_fee_policies)
    ↓
for each winner:
    start_settlement_execution()    ← spawn background task
    ↓ (background):
    settle(driver, auction_id, block_deadline)
    ↓ race:
    ├── driver.settle() → wait for tx on-chain
    └── wait_for_settlement_transaction() → poll DB block-by-block
    ↓
store_execution_ended()
    ↓
(separately, via observer):
post_process_outstanding_settlement_transactions()
    ↓ for each on-chain settlement:
    decode calldata → extract auction_id
    fetch_auction_data_for_transaction()
    find_winning_solution_uid()
    save_settlement()
    compute surplus/fee/fee_breakdown
```

### Key Mathematical Results

| Formula | Expression | Notes |
|---|---|---|
| Price conversion | `ETH = token_amount * price / 10^18` | price in wei per 10^18 tokens |
| Sell order surplus | `bought - limit_buy` | in buy token, ceil div |
| Buy order surplus | `limit_sell - sold` | in sell token |
| Total fee | `surplus_before - surplus_after` | in surplus token |
| Surplus fee (post) | `surplus_after * f/(1-f)` | f = configured factor |
| Volume fee (sell) | `buy_after * f/(1-f)` | f = configured factor |
| Volume fee (buy) | `sell_after * f/(1+f)` | f = configured factor |
| Partner fee cap | `(1+C)/(1+accumulated) - 1` | compounding cap |
| Solve deadline | `T_block + n*T_slot - T_prop` | first n where ≥ min_time |
| Reference score | `total_score - score_without_S` | counterfactual contribution |

---

## File Count Summary

This analysis covers **11 files** totaling approximately **4,200 lines** of Rust source code from the autopilot crate's core modules.

Key algorithmic discoveries:
1. **Multi-winner auction** with directional uniform clearing prices
2. **Vickrey-style reference scores** for solver rewards
3. **Compounding partner fee cap** using `(1+C)/(1+accumulated)-1`
4. **Reverse iteration** for layered fee computation
5. **Volume fee asymmetry**: `1/(1-f)` for sell, `1/(1+f)` for buy
6. **Block-aligned solve deadlines** for optimal on-chain inclusion
7. **60-second DB checkpoint rollback** for race condition prevention
8. **`isSolver` checked in parallel with `/solve`** — zero auth overhead
9. **In-flight order filtering** prevents double-settlement
10. **Type-state bid lifecycle** prevents logical ordering violations
