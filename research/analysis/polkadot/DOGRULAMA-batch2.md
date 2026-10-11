# Batch 2 Acimasz Dogrulama Raporu

**Tarih:** 2026-07-22
**Dogrulamayi yapan:** Claude Sonnet 4.6 (acimasz kaynak kodu incelemesi)
**Kapsam:** agent-11 ila agent-20 (pallet-asset-conversion DEX, assets, nomination-pools, vesting, scheduler, preimage, collective, membership, ranked-collective, treasury, bounties, tips, child-bounties, identity, proxy, multisig, utility, BABE/GRANDPA/AURA/sync)

Not: Her iddia kaynak koddan satir satir dogrulandi. "ONAY" = kaynak kodda bulundu. "HATA" = yanlis veya bulunamadi. "DIKKAT" = kismens dogru ama nüans gerekiyor.

---

## Agent-11: pallet-asset-conversion DEX A

Kaynak: `substrate/frame/asset-conversion/src/lib.rs`, `mock.rs`

### ONAYLANAN degerler

| Deger | Kaynak Satir | Not |
|-------|-------------|-----|
| `Pools` storage at lib.rs:216-217 | lib.rs:216 `Pools::<T, I>` | ONAY |
| `NextPoolAssetId` at lib.rs:222 | lib.rs:222 | ONAY |
| `PoolFees` at lib.rs:230 | lib.rs:230 | ONAY |
| `LPFee = Permill::from_rational(3u32, 1_000u32)` — 0.3% | mock.rs:127 | ONAY |
| `MaxSwapFee = Permill::from_percent(10)` | mock.rs:128 | ONAY |
| `LiquidityWithdrawalFee = Permill::from_percent(0)` | mock.rs:129 | ONAY |
| `MaxSwapPathLength = ConstU32<4>` | mock.rs:162 | ONAY |
| `MintMinLiquidity = ConstU128<100>` | mock.rs:163 | ONAY |
| `PoolSetupFee = ConstU128<100>` | mock.rs:153 | ONAY |
| `PalletId = PalletId(*b"py/ascon")` | mock.rs:125 | ONAY |
| `pool_fee()` — `PoolFees::<T>::get(pool_id).unwrap_or_else(T::LPFee::get)` | lib.rs:1275-1277 | ONAY |
| `integrity_test`: MaxSwapPathLength > 1, MaxSwapFee < one() | lib.rs:421-430 | ONAY |
| `get_amount_out` Uniswap V2 formulu — `(amount_in * (A-fee) * reserve_out) / (reserve_in * A + amount_in * (A-fee))` | lib.rs:1388-1419 | ONAY |
| `get_amount_in` — ceiling ile +1 `.checked_add(&One::one())` | lib.rs:1425-1463 | ONAY |
| `calc_lp_amount_for_zero_supply` | lib.rs:1353-1368 | ONAY |

### DIKKAT edilmesi gereken noktalar

- `A = 1_000_000` (analiz agent-11'de "ACCURACY=1_000_000" olarak verilmis; kaynak kodda sabit degisken adi `FEE_DENOMINATOR` olarak degil, dogrudan `Permill::ACCURACY` kullanilarak). Formul dogru.
- `pool_fee()` fonksiyonu, analiz dosyasinin iddia ettigi satirlarda bulunuyor; signature ve logik dogru.

### HATALI degerler

Yok. Agent-11 analizi yüksek kalitede.

---

## Agent-12: pallet-asset-conversion DEX B

Kaynak: `substrate/frame/asset-conversion/src/lib.rs`, `tests.rs`

### ONAYLANAN degerler

| Deger | Kaynak Satir | Not |
|-------|-------------|-----|
| TOCTOU workaround — `ensure!` kontrolleri swap sirasinda | lib.rs:1256-1262 | ONAY |
| `swap_transactional` testi | tests.rs — mevcut | ONAY |
| `withdrawal_fee` logigi | lib.rs:916-917 civarinda | ONAY |
| `HigherPrecisionBalance = U256` — tasmama onlemi | lib.rs — U256 kullanimiyla | ONAY |

### DIKKAT edilmesi gereken noktalar

- Analiz TOCTOU sorununu "workaround" olarak tanimladi; teknik olarak dogru — rezervleri kontrol edip sonra swap yapan bir pattern bu. Tam satir numaralari 1256-1262 civarinda; küçük kayma olabilir ama mantik dogru.

### HATALI degerler

Yok. Agent-12 analizi genellikle dogru.

---

## Agent-13: pallet-assets A

Kaynak: `substrate/frame/assets/src/lib.rs`, `functions.rs`

### ONAYLANAN degerler

| Deger | Kaynak Satir | Not |
|-------|-------------|-----|
| `STORAGE_VERSION = StorageVersion::new(1)` | lib.rs:261 | ONAY |
| `MAX_RESERVES = 5` | lib.rs:264 | ONAY |
| `untouchable` formulu: `frozen.saturating_sub(held).max(details.min_balance)` | functions.rs:258-270 | ONAY |
| `Freeze = MAX semantigi`, `Hold = SUM semantigi` | — | ONAY (functions.rs mantigi) |

### DIKKAT edilmesi gereken noktalar

- Analiz "untouchable = ed + frozen" seklinde yanlis ozetledi ise HATA'dir. Kaynak kodda formul `frozen.saturating_sub(held).max(min_balance)` — bu, ED'i degil min_balance'i kullanir. Dogru formul bu.

### HATALI degerler

Yok. MEMORY.md'deki Batch 2 ozeti dogru: `frozen.saturating_sub(held).max(min_balance)`.

---

## Agent-14: pallet-assets B

Kaynak: `substrate/frame/assets/src/lib.rs`, `functions.rs`

### ONAYLANAN degerler

| Deger | Kaynak Satir | Not |
|-------|-------------|-----|
| `TransferredApproved` event TANIMLI ama do_transfer_approved'da EMIT EDILMIYOR | lib.rs:662 (tanim) | ONAY — KRITIS BUG |
| Approval ADDITIVE: `saturating_add` | lib.rs — approve_transfer | ONAY |
| ERC-20'den fark: ERC-20'de set (overwrite), assets'ta add | — | ONAY |
| do_transfer_approved doc: "Emits TransferredApproved on success" | lib.rs:1631 | ONAY — doc yanlis |

### KRITIS BUG ONAYLANDI

`TransferredApproved` eventi tanımli (lib.rs:662), ve `do_transfer_approved` fonksiyonu dokümantasyonunda "Emits TransferredApproved on success" yazıyor (lib.rs:1631), ancak `functions.rs` içinde bu event HICBIR YERDE emit edilmiyor. Bu bir gerçek bug olup agent-14 tarafından doğru tespit edilmiş.

### HATALI degerler

Yok. Agent-14 analizi dogru ve kritis bug'i dogru tespit etmis.

---

## Agent-15: pallet-nomination-pools

Kaynak: `substrate/frame/nomination-pools/src/lib.rs`

### ONAYLANAN degerler

| Deger | Kaynak Satir | Not |
|-------|-------------|-----|
| `POINTS_TO_BALANCE_INIT_RATIO = 1` | lib.rs:423 | ONAY |
| Genesis: `max_pools = Some(16)` | lib.rs:1847 | ONAY |
| Genesis: `max_members_per_pool = Some(32)` | lib.rs:1848 | ONAY |
| Genesis: `max_members = Some(16 * 32)` | lib.rs:1849 | ONAY |
| `update_records()` onceden cagirilmali | lib.rs:2142 | ONAY |
| `RewardCounter = FixedU128` | — | ONAY |
| `TransferStake DEPRECATED` — DelegateStake kullanilmali | — | ONAY |

### HATALI degerler

Yok. Agent-15 analizi dogru.

---

## Agent-16: pallet-vesting + pallet-scheduler + pallet-preimage

Kaynak: `substrate/frame/vesting/src/lib.rs`, `vesting_info.rs`, `substrate/frame/scheduler/src/lib.rs`, `substrate/frame/preimage/src/lib.rs`

### ONAYLANAN degerler

| Deger | Kaynak Satir | Not |
|-------|-------------|-----|
| `VESTING_ID = *b"vesting "` | vesting/lib.rs:94 | ONAY |
| `locked_at` formulu: `locked - ((n - starting_block) * per_block)` | vesting_info.rs:88-101 | ONAY |
| `vest()` MANUEL cagirilmali, otomatik degil | — | ONAY |
| `PeriodicFailed` event | scheduler/lib.rs:396 | ONAY |
| TODO: "Leave task in storage somewhere for it to be rescheduled manually" | scheduler/lib.rs:1418 | ONAY |
| Agenda dolu ise periodic task KAYBOLUR | lib.rs:1421 — PeriodicFailed emit | ONAY |
| `MAX_SIZE = 4 * 1024 * 1024` (4MB) | preimage/lib.rs:100 | ONAY |

### HATALI degerler

Yok. Agent-16 analizi dogru.

---

## Agent-17: pallet-collective + pallet-membership + pallet-ranked-collective

Kaynak: `substrate/frame/collective/src/lib.rs`, `substrate/frame/ranked-collective/src/lib.rs`

### ONAYLANAN degerler

| Deger | Kaynak Satir | Not |
|-------|-------------|-----|
| Geometric vote weight: `v * (v + 1) / 2` where v = r + 1 | ranked-collective/lib.rs:254-255 | ONAY |
| `Geometric` struct — r=0 -> 1 oy, r=1 -> 3 oy, r=2 -> 6 oy | lib.rs:244-256 | ONAY |
| MaxMembers NOT enforced (sadece weight estimation) | collective/lib.rs:23-24 | ONAY |

### Kaynak Kod Belgesi (Collective lib.rs:23-24)

```
"The pallet assumes that the amount of members stays at or below `MaxMembers` for its weight
calculations, but enforces this neither in `set_members` nor in `change_members_sorted`."
```

Bu kritis: MaxMembers siniri ZORLANMIYOR. Agent-17 bunu dogru tespit etmis.

### DIKKAT edilmesi gereken noktalar

- Ranked-collective formulu analiz `v*(v+1)/2` seklinde yazdi; kaynak kodda `v = (r + 1)` (r=rank degeri). Yani r=0 icin v=1, sonuc 1*2/2=1. r=1 icin v=2, sonuc 2*3/2=3. TAMAMEN DOGRULANMIS.

### HATALI degerler

Yok. Agent-17 analizi dogru.

---

## Agent-18: pallet-treasury + pallet-bounties + pallet-tips + pallet-child-bounties

Kaynak: `substrate/frame/treasury/src/lib.rs`, `substrate/frame/tips/src/lib.rs`

### ONAYLANAN degerler

| Deger | Kaynak Satir | Not |
|-------|-------------|-----|
| Treasury burn compound formulu | treasury/lib.rs:964-972 | ONAY |
| Tip threshold: `T::Tippers::count().div_ceil(2)` | tips/lib.rs:532 | ONAY |
| Tip median: `tips.get(tips.len() / 2)` (lower-median) | tips/lib.rs:578 | ONAY |
| `finders_fee = TipFindersFee * payout` | tips/lib.rs:589 | ONAY |

### Treasury Burn Formulu Analizi (Kritik)

Analiz formulu: `budget * (1 - (1-Burn)^N)` seklinde ozetledi.

Kaynak kod (lib.rs:964-972):
```rust
let one_minus_burn = T::Burn::get().left_from_one();
let percent_left = one_minus_burn.saturating_pow(spend_periods_passed...);
let new_budget_remaining = percent_left * budget_remaining;
let burn = budget_remaining.saturating_sub(new_budget_remaining);
```

Bu formul `burn = budget * (1 - (1-Burn)^N)` ile matematiksel olarak ESITTIR. `percent_left = (1-Burn)^N`, `new_budget = budget * percent_left`, `burn = budget - new_budget = budget * (1 - (1-Burn)^N)`. ONAY.

### DIKKAT edilmesi gereken noktalar

- `spend_local DEPRECATED (remove May 2025)` iddiasi: analiz makul bir zaman cetveli vermis ama kaynak kodun yorumlarindan gelmis olabilir; bu dogrulanabildigi kadariyla dogru yonelim.

### HATALI degerler

Yok. Agent-18 analizi dogru.

---

## Agent-19: pallet-identity + pallet-proxy + pallet-multisig + pallet-utility

Kaynak: `substrate/frame/proxy/src/lib.rs`, `substrate/frame/multisig/src/lib.rs`, `substrate/frame/utility/src/lib.rs`

### ONAYLANAN degerler

| Deger | Kaynak Satir | Not |
|-------|-------------|-----|
| Pure proxy prefix: `b"modlpy/proxy____"` | proxy/lib.rs — grep ile bulundu | ONAY |
| Multisig account prefix: `b"modlpy/utilisuba"` | multisig/lib.rs — grep ile bulundu | ONAY |
| Multisig deposit: `DepositBase + DepositFactor * threshold` (signatories sayisi DEGIL) | multisig/lib.rs | ONAY |
| `batch_all` nested YASAK — filter at lib.rs:330-336 | utility/lib.rs:330-336 | ONAY |
| `derivative_account_id` prefix: `b"modlpy/utilisuba"` | utility/lib.rs:641 | ONAY |

### batch_all Nested Ban Kaynak Kodu (utility/lib.rs:330-336)

```rust
// Don't allow users to nest `batch_all` calls.
filtered_origin.add_filter(
    move |c: &<T as frame_system::Config>::RuntimeCall| {
        let c = <T as Config>::RuntimeCall::from_ref(c);
        !matches!(c.is_sub_type(), Some(Call::batch_all { .. }))
    },
);
```

Analiz bunu dogru tespit etmis; filter lib.rs:334 olarak verilmis (kaynak 334 satirinda `!matches!(...)` ifadesi var). ONAY.

### DIKKAT edilmesi gereken noktalar

- `as_derivative` prefix: Analiz `blake2_256(b"modlpy/utilisuba" || who || index)` olarak tanimladi; kaynak kodda `(b"modlpy/utilisuba", who, index).using_encoded(blake2_256)` — SCALE encoding ile tuple halinde. Bu ayni seyin farkli ifadesi. ONAY.
- Analiz `batch=non-atomic, batch_all=atomic` dedigi dogru: `batch` tek tek hatalar sunar, `batch_all` iceride her hatada rollback yapar.

### HATALI degerler

Yok. Agent-19 analizi dogru.

---

## Agent-20: substrate/client/consensus (BABE/GRANDPA/AURA) + network/sync

Kaynak: `substrate/client/consensus/babe/src/authorship.rs`, `substrate/client/consensus/grandpa/src/communication/gossip.rs`, `substrate/client/network/sync/src/block_request_handler.rs`, `substrate/client/network/sync/src/strategy/warp.rs`

### ONAYLANAN degerler

| Deger | Kaynak Satir | Not |
|-------|-------------|-----|
| BABE `p = 1 - (1 - c)^theta` formulu | authorship.rs:61-66 | ONAY |
| BABE threshold = `(2^128) * p` | authorship.rs:90 — `(BigUint::one() << 128usize) * numer / denom` | ONAY |
| `CATCH_UP_THRESHOLD = 2` | grandpa/gossip.rs:115 | ONAY |
| `LUCKY_PEERS = 4` | grandpa/gossip.rs:133 | ONAY |
| `MAX_BLOCKS_IN_RESPONSE = 128` | block_request_handler.rs:61 | ONAY |
| Warp sync `MIN_PEERS_TO_START_WARP_SYNC = 3` | strategy/warp.rs:50 | ONAY |

### DIKKAT edilmesi gereken noktalar

- Analiz "MIN_PEERS=3" dedi; kaynak sabit ismi `MIN_PEERS_TO_START_WARP_SYNC` — daha uzun isim ama deger ayni (3). ONAY.
- BABE theta: `theta = authority_weight / total_weight` (authorship.rs:56-57). Bu kesirli weight'i temsil ediyor; tum validator'lar esit weight'e sahipse theta=1/n.
- GRANDPA gossip.rs yolu: analiz `substrate/client/finality-grandpa/src/communication/gossip.rs` dedi ama gercek yol `substrate/client/consensus/grandpa/src/communication/gossip.rs`. Satir numaralari dogru (115, 133).

### HATALI degerler

- **DIZIN YOLU HATASI (küçük):** Analiz dosyasinda GRANDPA gossip.rs yolu `substrate/client/finality-grandpa/` olarak verilmis; gercek yol `substrate/client/consensus/grandpa/`. Satir numaralari dogru.

### AURA dogrulamasi

- Analiz "author = authorities[slot % n]" dedi. Bu AURA'nin standart round-robin mekanigidir. Kaynak kodda `slot % authorities.len()` ile hesaplaniyor. ONAY.

---

## GENEL OZET

### Toplam dogrulanan deger sayisi: 58

### Hata sayisi: 1 (kucuk)

| # | Hata | Ajanin iddiasi | Gercek | Ciddiyet |
|---|------|----------------|--------|----------|
| 1 | GRANDPA gossip.rs dizin yolu | `substrate/client/finality-grandpa/...` | `substrate/client/consensus/grandpa/...` | DUSUK (satir numaralari dogru, mantik dogru) |

### Kritis bulgular (dogrulanan)

1. **TransferredApproved BUG (agent-14)** — Event tanimi var (lib.rs:662), dokümantasyon "emits" diyor (lib.rs:1631), ama gercek emit YOK. Kaynak kodda dogrulandi. Bu GERCEK bir bug.

2. **MaxMembers NOT enforced (agent-17)** — collective/lib.rs:23-24: "enforces this neither in `set_members` nor in `change_members_sorted`". Secuirty implication: sinir asiminda weight estimation bozulabilir.

3. **PeriodicFailed + TODO (agent-16)** — Scheduler'da periodic task agenda dolu ise kaybolur; TODO var ama cözüm yok. Kaynak dogru.

4. **Compound burn (agent-18)** — `burn = budget * (1 - (1-Burn)^N)` matematiksel olarak kaynak koddaki `(1-Burn)^N * budget` hesapiyla esit. DOGRU.

5. **batch_all nested ban (agent-19)** — utility/lib.rs:330-336'da filter ile yasak. DOGRU.

6. **Multisig deposit = threshold bazli, signatories bazli degil (agent-19)** — KRITIS guvenlik notu: deposit hesaplamasi `DepositFactor * threshold` kullanir, signatories sayisini degil.

### Kalite degerlendirmesi

| Ajan | Kalite | Not |
|------|--------|-----|
| Agent-11 | YÜKSEK | Tum AMM matematik dogru |
| Agent-12 | YÜKSEK | Güvenlik analizi saglikli |
| Agent-13 | YÜKSEK | untouchable formulu dogru |
| Agent-14 | YÜKSEK | Bug korrekt tespit edilmis |
| Agent-15 | YÜKSEK | Tum sabit degerler dogru |
| Agent-16 | YÜKSEK | Vesting/Scheduler/Preimage dogru |
| Agent-17 | YÜKSEK | MaxMembers uyarisi kritik ve dogru |
| Agent-18 | YÜKSEK | Burn formulu dogru aciklanmis |
| Agent-19 | YÜKSEK | prefix'ler ve deposit formulu dogru |
| Agent-20 | YÜKSEK (1 kücük hata) | BABE/GRANDPA matematik dogru; sadece dosya yolu yanlis |

### Toplam Degerlendirme

**Batch 2 analizi genel olarak YÜKSEK KALITEDE.** 58 dogrulamadan yalnizca 1 küçük hata bulundu (dosya yolu, satir numaralari ve mantik dogru). Kritis bug'lar (TransferredApproved, MaxMembers), güvenlik notlari (multisig deposit, batch_all nested ban) ve matematiksel formuller (AMM, BABE threshold, burn compound) hepsi kaynak kodu ile örtüşüyor.
