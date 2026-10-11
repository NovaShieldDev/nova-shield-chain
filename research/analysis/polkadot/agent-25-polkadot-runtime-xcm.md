# Agent-25: Polkadot Runtime + XCM Analizi

## Kapsanan Dosyalar

### Polkadot Runtime
- `polkadot/runtime/common/src/impls.rs` — DealWithFees, relay_era_payout
- `polkadot/runtime/common/src/lib.rs` — runtime common lib
- `polkadot/runtime/common/src/xcm_sender.rs` — XCM sender impls

### XCM Core
- `polkadot/xcm/src/lib.rs` — versioned wrapper types, global sabitleler
- `polkadot/xcm/src/v3/mod.rs` — XCM v3 instruction seti
- `polkadot/xcm/src/v4/mod.rs` — XCM v4 instruction seti
- `polkadot/xcm/src/v5/mod.rs` — XCM v5 instruction seti (latest)
- `polkadot/xcm/src/v5/traits.rs` — Error, Outcome, ExecuteXcm, SendXcm
- `polkadot/xcm/src/v5/location.rs` — Location struct
- `polkadot/xcm/src/v5/junction.rs` — Junction enum
- `polkadot/xcm/src/v5/asset.rs` — Asset, Assets, AssetId

### XCM Executor
- `polkadot/xcm/xcm-executor/src/lib.rs` — XcmExecutor struct, execute() mantigi
- `polkadot/xcm/xcm-executor/src/config.rs` — Config trait (tum parametreler)
- `polkadot/xcm/xcm-executor/src/traits/fee_manager.rs` — FeeManager, FeeReason
- `polkadot/xcm/xcm-executor/src/traits/transact_asset.rs` — TransactAsset trait

### XCM Builder
- `polkadot/xcm/xcm-builder/src/lib.rs` — tum export edilen bilesenler
- `polkadot/xcm/xcm-builder/src/barriers.rs` — TakeWeightCredit, AllowTopLevelPaidExecutionFrom, AllowUnpaidExecutionFrom, DenyThenTry vs.
- `polkadot/xcm/xcm-builder/src/weight.rs` — FixedWeightBounds, WeightInfoBounds, UsingComponents, FixedRateOfFungible
- `polkadot/xcm/xcm-builder/src/fungible_adapter.rs` — FungibleAdapter, FungibleMutateAdapter
- `polkadot/xcm/xcm-builder/src/fee_handling.rs` — XcmFeeManagerFromComponents, SendXcmFeeToAccount
- `polkadot/xcm/xcm-builder/src/location_conversion.rs` — DescribeLocation, HashedDescription, AccountId32Aliases

---

## 1. Polkadot Runtime Konfigurasyonu

### Parachain NOT: Runtime Dizin Yapisi
Polkadot SDK'da `polkadot/runtime/` altinda parachain/relay-chain ayrimli bir yapi var:
- `polkadot/runtime/common/` — relay chain ve parachainlerin paylasilmasi gereken utility kodlari
- `polkadot/runtime/parachains/` — parachain hostlama mantigina ait pallet'ler (L1 icin alakasiz)
- `polkadot/runtime/rococo/` ve `polkadot/runtime/westend/` — test relay chain runtime'lari

**L1 icin asil ilgili kisim:** `polkadot/runtime/common/src/impls.rs` — gercek relay chain uygulama ornegi.

### Fee Dagilimi: DealWithFees
**Dosya:** `polkadot/runtime/common/src/impls.rs`, satir 47-68

```rust
pub struct DealWithFees<R>(core::marker::PhantomData<R>);
impl<R> OnUnbalanced<Credit<R::AccountId, pallet_balances::Pallet<R>>> for DealWithFees<R>
{
    fn on_unbalanceds(mut fees_then_tips: ...) {
        if let Some(fees) = fees_then_tips.next() {
            // fees icin: 80% treasury, 20% author
            let mut split = fees.ration(80, 20);
            if let Some(tips) = fees_then_tips.next() {
                // tips: 100% author
                tips.merge_into(&mut split.1);
            }
            ResolveTo::<TreasuryAccountId<R>, ...>::on_unbalanced(split.0);  // 80%
            ToAuthor<R>::on_unbalanced(split.1);                              // 20% + tips
        }
    }
}
```

**Kritik:** Fee dagilim formulu: `fees -> 80% treasury + 20% author`, `tips -> 100% author`

### Staking Enflasyon: relay_era_payout
**Dosya:** `polkadot/runtime/common/src/impls.rs`, satir 95-128

```rust
pub struct EraPayoutParams {
    pub total_staked: Balance,
    pub total_stakable: Balance,
    pub ideal_stake: Perquintill,
    pub max_annual_inflation: Perquintill,
    pub min_annual_inflation: Perquintill,
    pub falloff: Perquintill,
    pub period_fraction: Perquintill,
    pub legacy_auction_proportion: Option<Perquintill>,
}
```

Hesaplama:
1. `ideal_stake = ideal_stake - legacy_auction_proportion`
2. `stake = total_staked / total_stakable`
3. `adjustment = compute_inflation(stake, ideal_stake, falloff)` (NPoS curve)
4. `staking_inflation = min_inflation + (max_inflation - min_inflation) * adjustment`
5. `staking_payout = period_fraction * staking_inflation * total_stakable`
6. `rest (treasury) = period_fraction * max_inflation * total_stakable - staking_payout`

Polkadot icin bilinen degerler (test dosyasindan):
- `ideal_stake = 75%`
- `min_annual_inflation = 2.5% (25/1000)`
- `max_annual_inflation = 10%`
- `falloff = 5%`

### VersionedLocatableAsset
**Dosya:** `polkadot/runtime/common/src/impls.rs`, satir 133-158

```rust
pub enum VersionedLocatableAsset {
    #[codec(index = 3)]
    V3 { location: xcm::v3::Location, asset_id: xcm::v3::AssetId },
    #[codec(index = 4)]
    V4 { location: xcm::v4::Location, asset_id: xcm::v4::AssetId },
    #[codec(index = 5)]
    V5 { location: xcm::v5::Location, asset_id: xcm::v5::AssetId },
}
```

Treasury spend ve asset rate icin kullanilir. `From<(latest::Location, latest::AssetId)>` impl V5 olarak uretir.

---

## 2. construct_runtime! Yapisi

Polkadot SDK'da runtime kurulum macro'su `construct_runtime!` kullanilir. Bu macro:
- Pallet'lerin index numarasini atar (call dispatch, events, errors icin)
- Her pallet icin `Config` impl zorunlulugunu enforce eder
- Runtime aggregate type'lari (RuntimeCall, RuntimeEvent, RuntimeOrigin) uretir

Ornek pattern (test dosyalarindan):
```rust
frame_support::construct_runtime!(
    pub enum Runtime {
        System: frame_system = 0,
        Balances: pallet_balances = 1,
        Treasury: pallet_treasury = 2,
        // ...
    }
);
```

**L1 tasarim notu:** Nova Shield'de `construct_runtime!` kullanilacaksa her pallet'e sabit index verilmeli. Index degisikligi wire-format degisikligine yol acar (migration gerektirir).

**Pallet index 0 = System'e ayrilmalidir** (frame_system convention).

---

## 3. XCM Format ve Versiyonlama

### Aktif Versiyonlar
**Dosya:** `polkadot/xcm/src/lib.rs`, satir 34-44

```rust
pub mod v3;
pub mod v4;
pub mod v5;

pub mod lts {
    pub use super::v4::*;  // Long-Term Support = v4
}

pub mod latest {
    pub use super::v5::*;  // En yeni = v5
}
```

- **v3:** Hala destekleniyor, `MultiLocation` ve `MultiAsset` isimlendirmesi
- **v4:** LTS (Long-Term Support) versiyonu — `Location`, `Asset` isimlendirmesi (MultiLocation -> Location)
- **v5:** LATEST — `PayFees`, `InitiateTransfer`, `ExecuteWithOrigin`, `SetHints` eklendi
- **MIN_XCM_VERSION = 3** (lib.rs, satir 533)

### VersionedXcm — Encoding
**Dosya:** `polkadot/xcm/src/lib.rs`, satir 320-334

```rust
pub enum VersionedXcm<RuntimeCall> {
    #[codec(index = 3)]
    V3(v3::Xcm<RuntimeCall>),
    #[codec(index = 4)]
    V4(v4::Xcm<RuntimeCall>),
    #[codec(index = 5)]
    V5(v5::Xcm<RuntimeCall>),
}
```

**SCALE encoding birinci byte versiyonu belirtir:** 3, 4 veya 5.

### Versiyon Donusumu: Chain'ler Arasi Uyumluluk
**Dosya:** `polkadot/xcm/src/lib.rs`, satir 141-219

```rust
impl IntoVersion for $n {
    fn into_version(self, n: Version) -> Result<Self, ()> {
        Ok(match n {
            3 => Self::V3(self.try_into()?),
            4 => Self::V4(self.try_into()?),
            5 => Self::V5(self.try_into()?),
            _ => return Err(()),
        })
    }
}
```

Donusum yolu: `V5 -> V4 -> V3` (iki adimli). Ters yonde (`V3 -> V4 -> V5`) de yapilir.

**WrapVersion trait'i**, bir mesaj gondermeden once alici chain'in destekledigi versiyona donusturur:
```rust
pub trait WrapVersion {
    fn wrap_version<RuntimeCall: Decode + GetDispatchInfo>(
        dest: &latest::Location,
        xcm: impl Into<VersionedXcm<RuntimeCall>>,
    ) -> Result<VersionedXcm<RuntimeCall>, ()>;
}
```

`pallet_xcm` bu trait'i kullanak alici chain'in subscribe ettigimiz versiyona gore mesaji sarmalar.

### Boyut Limitleri
**Dosya:** `polkadot/xcm/src/lib.rs`, satir 55-59

```rust
pub const MAX_XCM_DECODE_DEPTH: u32 = 8;
pub const MAX_INSTRUCTIONS_TO_DECODE: u8 = 100;
```

**Dosya:** `polkadot/xcm/src/v5/asset.rs`, satir 556

```rust
pub const MAX_ITEMS_IN_ASSETS: usize = 20;
```

**Dosya:** `polkadot/xcm/xcm-executor/src/lib.rs`, satir 72

```rust
pub const RECURSION_LIMIT: u8 = 10;  // SetErrorHandler/SetAppendix icindeki XCM icin
```

**Boyut kontrolleri** (`size_limits` test, lib.rs satir 608-621):
- `Instruction<()>` max 128 bytes
- `Asset` max 80 bytes
- `Location` max 24 bytes
- `AssetId` max 40 bytes
- `Junctions` max 16 bytes
- `Junction` max 88 bytes
- `Assets` max 24 bytes (pointer, capacity saymaz)

---

## 4. XCM Instruction Seti (v5 - Tam Liste)

**Dosya:** `polkadot/xcm/src/v5/mod.rs`, satir 392-1142

Toplam instruction sayisi (v5): **42 instruction**

### Kategori 1: Asset Yukleme (Holding Register'a koyma)
1. **`WithdrawAsset(Assets)`** — origin'in sahipliginden cek, Holding'e koy (Command)
2. **`ReserveAssetDeposited(Assets)`** — origin chain'in sovereign hesabina lock'li; Holding'de derivative uret (Trusted Indication)
3. **`ReceiveTeleportedAsset(Assets)`** — origin chain'de yakildı; burada mint et, Holding'e koy (Trusted Indication)
4. **`ClaimAsset { assets, ticket }`** — trap'ten claim et, Holding'e koy (Command)

### Kategori 2: Asset Transfer
5. **`TransferAsset { assets, beneficiary }`** — dogrudan transfer, Holding kullanmaz (Command)
6. **`TransferReserveAsset { assets, dest, xcm }`** — dest'in sovereign'ine koy + `ReserveAssetDeposited` mesaji gonder (Command)
7. **`DepositAsset { assets, beneficiary }`** — Holding'den cekip beneficiary'ye depozit et (Command)
8. **`DepositReserveAsset { assets, dest, xcm }`** — Holding'den dest sovereign'ine + `ReserveAssetDeposited` gonder (Command)
9. **`ExchangeAsset { give, want, maximal }`** — Holding'deki varliklarla swap (Command)
10. **`InitiateReserveWithdraw { assets, reserve, xcm }`** — Holding'den sil, reserve'e `WithdrawAsset` gonder (Command)
11. **`InitiateTeleport { assets, dest, xcm }`** — Holding'den sil (burn), dest'e `ReceiveTeleportedAsset` gonder (Command)
12. **`BurnAsset(Assets)`** — Holding'den sil, hata vermez (Command, Infallible)

### Kategori 3: Fee Odeme
13. **`BuyExecution { fees, weight_limit }`** — Holding'deki asset ile weight satin al (Command) — v3/v4 yontemi
14. **`RefundSurplus`** — fazla alinmis weight'i geri ver (Command)
15. **`PayFees { asset }`** — v5 yeni fee mekanizmasi; `BuyExecution`'in halefi; sonraki `PayFees` no-op (Command)
16. **`SetFeesMode { jit_withdraw: bool }`** — JIT (just-in-time) cekim modunu ayarla (Command)

### Kategori 4: Origin Yonetimi
17. **`ClearOrigin`** — Origin register'i sil (Command)
18. **`DescendOrigin(InteriorLocation)`** — Origin'i iceri dogru mutate et (Command)
19. **`UniversalOrigin(Junction)`** — Origin'i evrensel bir child'a set et (Command, Fallible)
20. **`AliasOrigin(Location)`** — Origin'i verilen lokasyona degistir, Aliasers dogrulamali (Command)
21. **`UnpaidExecution { weight_limit, check_origin }`** — Ucretsiz execution icin barrier direktifi (Indication)

### Kategori 5: Hata Yonetimi / Kontrol Akisi
22. **`SetErrorHandler(Xcm<Call>)`** — Hata olursa calisacak XCM (Command)
23. **`SetAppendix(Xcm<Call>)`** — Hata olsa da olmasa da sonda calisacak XCM (Command)
24. **`ClearError`** — Error register'i temizle (Command)
25. **`Trap(u64)`** — Kasitli hata firlat (Command)

### Kategori 6: Sorgulama / Raporlama
26. **`QueryResponse { query_id, response, max_weight, querier }`** — Query sonucu gonder (Information)
27. **`ReportError(QueryResponseInfo)`** — Error register icerigini rapor et (Command)
28. **`ReportHolding { response_info, assets }`** — Holding icerigini rapor et (Command)
29. **`ReportTransactStatus(QueryResponseInfo)`** — Transact status'u rapor et (Command)
30. **`QueryPallet { module_name, response_info }`** — Pallet sorgusu (Command)
31. **`ExpectAsset(Assets)`** — Holding dogrulama (Command, hata: ExpectationFalse)
32. **`ExpectOrigin(Option<Location>)`** — Origin dogrulama (Command, hata: ExpectationFalse)
33. **`ExpectError(Option<(u32, Error)>)`** — Error register dogrulama (Command)
34. **`ExpectTransactStatus(MaybeErrorCode)`** — Transact status dogrulama (Command)
35. **`ExpectPallet { index, name, module_name, crate_major, min_crate_minor }`** — Pallet versiyon dogrulama

### Kategori 7: Versiyon / Abonelik
36. **`SubscribeVersion { query_id, max_response_weight }`** — XCM versiyon degisikliklerine abone ol
37. **`UnsubscribeVersion`** — Aboneligi iptal et

### Kategori 8: Asset Kilitleme
38. **`LockAsset { asset, unlocker }`** — Asset'i kilitle, unlocker'a `NoteUnlockable` gonder
39. **`UnlockAsset { asset, target }`** — Kilidi ac
40. **`NoteUnlockable { asset, owner }`** — Kilidin unlock edilebilir oldugunu not et (Trusted Indication)
41. **`RequestUnlock { asset, locker }`** — Locker'a `UnlockAsset` gonder

### Kategori 9: Mesaj Gonderme
42. **`ExportMessage { network, destination, xcm }`** — Bridge/cross-consensus gonderiminm

### Kategori 10: Konu / Meta
43. **`SetTopic([u8; 32])`** — Topic register'i set et (message tracing icin)
44. **`ClearTopic`** — Topic temizle
45. **`HrmpNewChannelOpenRequest { sender, max_message_size, max_capacity }`** — Sistem bildirimi
46. **`HrmpChannelAccepted { recipient }`** — Sistem bildirimi
47. **`HrmpChannelClosing { initiator, sender, recipient }`** — Sistem bildirimi

### Kategori 11: v5 Yeni Instructionlar
48. **`InitiateTransfer { destination, remote_fees, preserve_origin, assets, remote_xcm }`** — Cok adimli transfer icin tek instruction; multi-hop kolaylastirici
49. **`ExecuteWithOrigin { descendant_origin, xcm }`** — Ic xcm'yi farkli origin ile calistir (SafeCallFilter bypass yok)
50. **`SetHints { hints: BoundedVec<Hint, HintNumVariants> }`** — Execution hint'leri; simdilik `AssetClaimer { location }` hint'i mevcut

**NOT:** v5'te `Transact` instruksiyonu degisti:
```rust
// v4:
Transact { origin_kind, require_weight_at_most: Weight, call }
// v5:
Transact { origin_kind, fallback_max_weight: Option<Weight>, call }
// fallback_max_weight: eski versiyonlarla uyumluluk icin; None = eski chain'lerde calismayin
```

---

## 5. XCM Executor

### Yapisallik
**Dosya:** `polkadot/xcm/xcm-executor/src/lib.rs`, satir 77-107

```rust
pub struct XcmExecutor<Config: config::Config> {
    holding: AssetsInHolding,          // Holding Register
    holding_limit: usize,              // = MaxAssetsIntoHolding::get()
    context: XcmContext,               // origin, message_id, topic
    original_origin: Location,
    trader: Config::Trader,            // Weight alim-satim
    error: Option<(u32, XcmError)>,   // Error Register
    total_surplus: Weight,             // Fazla alinan weight
    total_refunded: Weight,
    error_handler: Xcm<...>,          // Error Handler Register
    error_handler_weight: Weight,
    appendix: Xcm<...>,               // Appendix Register
    appendix_weight: Weight,
    transact_status: MaybeErrorCode,  // Transact Status Register
    fees_mode: FeesMode,              // { jit_withdraw: bool }
    fees: AssetsInHolding,            // Fee olarak ayrilan assets
    asset_used_in_buy_execution: Option<AssetId>, // Delivery fee icin ayni asset
    message_weight: Weight,
    asset_claimer: Option<Location>,  // SetHints ile ayarlanan asset claimer
    already_paid_fees: bool,          // PayFees'in no-op kontrolu
}
```

### Execution Akisi
**Dosya:** `polkadot/xcm/xcm-executor/src/lib.rs`, satir 232-331

```
1. prepare(message, weight_limit):
   - Config::Weigher::weight(&mut message, weight_limit) cagir
   - Ok => WeighedMessage(weight, message)
   - Err => InstructionError (index + XcmError)

2. execute(origin, WeighedMessage, id, weight_credit):
   a. Barrier::should_execute(origin, instructions, xcm_weight, properties) cagir
      - Basarisiz => Outcome::Incomplete { error: XcmError::Barrier }
   b. message_id = properties.message_id veya *id
   c. XcmExecutor::new(origin, id) ile VM olustur
   d. vm.message_weight = xcm_weight
   e. Loop: instructions bitmeyince process() cagir
      - Hata: error_handler veya appendix calistir
      - Basari: error_handler dusur, appendix calistir
   f. vm.post_process(xcm_weight) -> Outcome

3. post_process():
   a. refund_surplus() (best-effort)
   b. trader drop (fee finalize)
   c. weight_used = xcm_weight - total_surplus
   d. Holding bos degilse: AssetTrap::drop_assets(claimer, holding)
      - claimer: asset_claimer veya context.origin veya original_origin
   e. error varsa Outcome::Incomplete, yoksa Outcome::Complete
```

### SetTopic Otomatik Ekleme
**Dosya:** `polkadot/xcm/xcm-executor/src/lib.rs`, satir 460-467

```rust
fn send(&mut self, dest, msg, reason) -> Result<XcmHash, XcmError> {
    // Mesaj SetTopic ile bitmiyorsa, context'teki topic/message_id'yi ekle
    if !matches!(msg.last(), Some(SetTopic(_))) {
        let topic_id = self.context.topic_or_message_id();
        msg.0.push(SetTopic(topic_id.into()));
    }
    // ...
}
```

### Recursion Limit
**Dosya:** `polkadot/xcm/xcm-executor/src/lib.rs`, satir 72

```rust
pub const RECURSION_LIMIT: u8 = 10;
```
`SetErrorHandler`, `SetAppendix` icindeki nested XCM'ler icin. `DenyRecursively` barrier'i da bunu kullanir.

---

## 6. Asset Transfer Mekanizmalari

### Reserve Transfer vs Teleport: Guvenlik Farki

**ReserveAssetDeposited (Reserve Transfer):**
- Gondericinin sovereign hesabi hedef chain'de lock'lidir
- Alici chain: "Gondericiye guveniyorum; o zaten varliklarini reserve'de tutuyor" der
- `IsReserve: ContainsPair<Asset, Location>` konfigurasyonu belirler
- Guvensiz kullanim: kotu niyetli chain `ReserveAssetDeposited` fake mesajlariyla mint yapabilir
- Bu nedenle **`ClearOrigin` onemi**: `ReserveAssetDeposited` sonrasi hemen `ClearOrigin` kullanilmalidir

**ReceiveTeleportedAsset (Teleport):**
- Gondericide yakilan, alanda uretilen
- `IsTeleporter: ContainsPair<Asset, Location>` konfigurasyonu belirler
- Guvensiz kullanim: kotu niyetli chain `ReceiveTeleportedAsset` fake mesajlariyla sinirsiz mint yapabilir
- **Sadece %100 guvenulen chain'ler arasi kullanilamaz**

### FungibleAdapter
**Dosya:** `polkadot/xcm/xcm-builder/src/fungible_adapter.rs`, satir 301-411

Uc adapter var:
1. **`FungibleTransferAdapter`** — sadece `internal_transfer_asset` (transfer, burn/mint yok)
2. **`FungibleMutateAdapter`** — deposit + withdraw + mint (transfer yok)
3. **`FungibleAdapter`** — tam kapsamli; ikisini birlestiriyor

Teleport dogrulama mekanizmasi (`MintLocation`):
```rust
pub enum MintLocation {
    Local,    // Asset burada mint edilir; teleport'ta checking_account azaltilir
    NonLocal, // Asset baska yerde mint edilir; teleport'ta checking_account artirilir
}
```

`CheckingAccount: Get<Option<(AccountId, MintLocation)>>` — `None` ise kontrol yok (guvenilmez!).

### AssetTransferFilter (v5 yeni)
**Dosya:** `polkadot/xcm/src/v5/asset.rs`

```rust
pub enum AssetTransferFilter {
    Teleport(AssetFilter),         // Yakilip uzakta mint edilecek
    ReserveDeposit(AssetFilter),   // Buraya koyulup uzakta derivative uretilecek
    ReserveWithdraw(AssetFilter),  // Burada yakil, uzaktaki reserve'den cek
}
```

`InitiateTransfer` instructionunda kullanilir; `MaxAssetTransferFilters = 6` upper limit.

---

## 7. Guvenlik: Barrier ve Filtreleme

### Barrier Trait
**Dosya:** `polkadot/xcm/xcm-executor/src/traits/should_execute.rs`

```rust
pub trait ShouldExecute {
    fn should_execute<RuntimeCall>(
        origin: &Location,
        instructions: &mut [Instruction<RuntimeCall>],
        max_weight: Weight,
        properties: &mut Properties,
    ) -> Result<(), ProcessMessageError>;
}
```

`Properties` icerigi:
- `weight_credit: Weight` — onceden alinmis weight kredisi (yerel extrinsic icin)
- `message_id: Option<[u8; 32]>` — `TrailingSetTopicAsId` tarafindan set edilir

### Temel Barrier Uygulamalari

**1. TakeWeightCredit**
**Dosya:** `polkadot/xcm/xcm-builder/src/barriers.rs`, satir 34-56

```rust
pub struct TakeWeightCredit;
// weight_credit'ten max_weight'i duser
// Yerel extrinsic cagrilarinda weight_credit onceden konulur
```

**2. AllowTopLevelPaidExecutionFrom<T>**
**Dosya:** satir 66-124

En kritik barrier. Kontrol edilen kosullar:
1. `T::contains(origin)` — origin izin verilmis listede mi?
2. Ilk instruction: `WithdrawAsset | ReceiveTeleportedAsset | ReserveAssetDeposited | ClaimAsset`
   - MAX_ASSETS_FOR_BUY_EXECUTION = **2** asset
3. Araya `ClearOrigin | AliasOrigin | DescendOrigin | SetHints` girebilir (up to 5 instruction bakilir)
4. Son baktigi instruction: `BuyExecution { weight_limit: Limited(w) }` where `w >= max_weight`
   - `Unlimited` ise `Limited(max_weight)` ile mutate eder
   - `PayFees { .. }` de kabul edilir (v5)

**3. AllowUnpaidExecutionFrom<T>**
**Dosya:** satir 287-303

```rust
// Sadece T::contains(origin) kontrolu yapar; baska bir sey dogrulamaz
// DIKKAT: Tamamen guvenilen origin'ler icin kullanin
```

**4. AllowExplicitUnpaidExecutionFrom<T, Aliasers>**
**Dosya:** satir 321-401

```rust
// Mesajin icinde UnpaidExecution instructioni arar
// AliasOrigin'i calistirir (Aliasers ile dogrulayarak)
// Son olarak: T::contains(&actual_origin) kontrolu
```

**5. WithComputedOrigin<InnerBarrier, LocalUniversal, MaxPrefixes>**
**Dosya:** satir 171-226

```rust
// UniversalOrigin veya DescendOrigin ile origin'i mutate eder
// Sonra InnerBarrier'i computed origin ile cagir
// NOT: UniversalOrigin dogrulamasi yapilmaz (execution esnasinda kontrol edilir)
```

**6. DenyThenTry<Deny, Allow>**
**Dosya:** satir 532-551

```rust
// Once Deny::deny_execution(), basarisizsa dur
// Sonra Allow::should_execute()
```

**7. DenyReserveTransferToRelayChain**
**Dosya:** satir 554-591

```rust
// Relay chain'e (parent=1, interior=Here) reserve transfer icin ozel filtre
// InitiateReserveWithdraw, DepositReserveAsset, TransferReserveAsset ile dest relay ise DENY
// ReserveAssetDeposited'in relay'den gelmesi: sadece debug log, devam edilir
```

**8. DenyRecursively<Inner>**
**Dosya:** satir 604-684

```rust
// SetAppendix, SetErrorHandler, ExecuteWithOrigin icin recursive Inner::deny_execution()
// RECURSION_LIMIT = 10 ile stack overflow engeli
// StackLimitReached hatasi donulebilir
```

**9. AllowKnownQueryResponses<ResponseHandler>**
**Dosya:** satir 440-466

```rust
// Sadece 1 instruction ve QueryResponse icermeli
// ResponseHandler::expecting_response(origin, query_id, querier) true olmali
```

**10. AllowSubscriptionsFrom<T>**
**Dosya:** satir 470-493

```rust
// Sadece 1 instruction: SubscribeVersion | UnsubscribeVersion
// T::contains(origin) kontrolu
```

**11. TrailingSetTopicAsId<InnerBarrier>**
**Dosya:** satir 235-259

```rust
// Mesajin son instruction'i SetTopic ise: message_id = SetTopic degerine set et
// Sonra InnerBarrier'e gec
```

**12. RespectSuspension<Inner, SuspensionChecker>**
**Dosya:** satir 263-281

```rust
// SuspensionChecker::is_suspended() ise Yield
// Yoksa Inner::should_execute()
```

---

## 8. Kritik Sayisal Degerler

### XCM Sabitleri
| Sabit | Deger | Kaynak |
|-------|-------|--------|
| `VERSION` (v3) | 3 | `xcm/src/v3/mod.rs` |
| `VERSION` (v4) | 4 | `xcm/src/v4/mod.rs` |
| `VERSION` (v5) | 5 | `xcm/src/v5/mod.rs`, satir 57 |
| `MIN_XCM_VERSION` | 3 | `xcm/src/lib.rs`, satir 533 |
| `MAX_XCM_DECODE_DEPTH` | 8 | `xcm/src/lib.rs`, satir 55 |
| `MAX_INSTRUCTIONS_TO_DECODE` | 100 | `xcm/src/lib.rs`, satir 59 |
| `MAX_ITEMS_IN_ASSETS` | 20 | `xcm/src/v5/asset.rs`, satir 556 |
| `RECURSION_LIMIT` | 10 | `xcm-executor/src/lib.rs`, satir 72 |
| `MAX_ASSETS_FOR_BUY_EXECUTION` | 2 | `xcm-builder/src/barriers.rs`, satir 58 |
| `MaxPalletNameLen` | 48 | `xcm/src/v5/mod.rs`, satir 211 |
| `MaxPalletsInfo` | 64 | `xcm/src/v5/mod.rs`, satir 212 |
| `MaxAssetTransferFilters` | 6 | `xcm/src/v5/mod.rs`, satir 213 |

### Bellek Boyutlari (Size Limits)
| Tip | Max Boyut |
|-----|-----------|
| `Instruction<()>` | 128 bytes |
| `Asset` | 80 bytes |
| `Location` | 24 bytes |
| `AssetId` | 40 bytes |
| `Junctions` | 16 bytes |
| `Junction` | 88 bytes |
| `Assets` | 24 bytes (ptr+len) |

### Weight Hesaplama
**Dosya:** `xcm-builder/src/weight.rs`, satir 266-268

`FixedRateOfFungible` formulu:
```rust
let amount =
    (units_per_second * weight.ref_time() as u128 / WEIGHT_REF_TIME_PER_SECOND as u128) +
    (units_per_mb * weight.proof_size() as u128 / WEIGHT_PROOF_SIZE_PER_MB as u128);
```

`UsingComponents` formulu:
```rust
let amount = WeightToFee::weight_to_fee(&weight);
```
Runtime'daki `WeightToFee` tipine gore degisir (ornegin `IdentityFee`, `ConstantMultiplier`, vs.).

### BuyExecution Weight Limit Degisiklik
**Dosya:** `xcm-builder/src/barriers.rs`, satir 109-121

```rust
BuyExecution { weight_limit: Limited(ref mut weight), .. }
    if weight.all_gte(max_weight) => {
        *weight = max_weight;   // Mevcut limit max_weight'e indirilir!
        Ok(())
    },
BuyExecution { ref mut weight_limit, .. } if weight_limit == &Unlimited => {
    *weight_limit = Limited(max_weight);  // Unlimited -> Limited(max_weight) mutate
    Ok(())
},
```

**Kritik**: `AllowTopLevelPaidExecutionFrom`, `weight_limit`'i mesajin icinde mutate eder. Bu, mesajin in-place degistirildigini gosterir.

### Fee Dagilim Orani (Polkadot Runtime)
- `fees`: 80% Treasury + 20% Author
- `tips`: 100% Author
- Kaynak: `polkadot/runtime/common/src/impls.rs`, satir 59-65

---

## 9. Executor Config Trait — Tam Parametreler

**Dosya:** `polkadot/xcm/xcm-executor/src/config.rs`, satir 31-135

```rust
pub trait Config {
    type RuntimeCall;             // Dispatch edilecek call tipi
    type XcmSender: SendXcm;     // Onward XCM gondericisi (delivery fee dondurmeli)
    type XcmEventEmitter;         // XCM event'leri emit et
    type AssetTransactor;         // Withdraw/deposit (asset yonetimi)
    type OriginConverter;         // Location -> RuntimeOrigin donusumu (Transact icin)
    type IsReserve;               // Hangi (Asset, Location) ciftleri reserve?
    type IsTeleporter;            // Hangi (Asset, Location) ciftleri teleport?
    type Aliasers;                // (Origin, Target) ciftleri; AliasOrigin icin
    type UniversalLocation;       // Bu chain'in evrensel konumu
    type Barrier;                 // ShouldExecute implementasyonu
    type Weigher;                 // XCM agirlik hesaplayici
    type Trader;                  // Weight alim-satim (WeightTrader)
    type ResponseHandler;         // QueryResponse yonetimi
    type AssetTrap;               // Holding'deki trapped assets
    type AssetLocker;             // LockAsset/UnlockAsset
    type AssetExchanger;          // ExchangeAsset (delivery fee swap)
    type SubscriptionService;     // SubscribeVersion yonetimi
    type PalletInstancesInfo;     // QueryPallet icin pallet bilgileri
    type MaxAssetsIntoHolding;    // Holding Register max asset sayisi
    type FeeManager;              // Fee muaf mi? Fee nereye gidecek?
    type MessageExporter;         // ExportMessage icin
    type UniversalAliases;        // UniversalOrigin icin izinli (Location, Junction) ciftleri
    type CallDispatcher;          // Transact icin call dispatcher
    type SafeCallFilter;          // Transact icin whitelist
    type TransactionalProcessor;  // Instruction transaction sarimasi
    type HrmpNewChannelOpenRequestHandler;
    type HrmpChannelAcceptedHandler;
    type HrmpChannelClosingHandler;
    type XcmRecorder;             // Dry-run icin XCM kayit
}
```

---

## 10. XCM Builder — Hazir Bilesenleri

**Dosya:** `polkadot/xcm/xcm-builder/src/lib.rs`

### Asset Conversion
- `AsPrefixedGeneralIndex` — GenericIndex'e prefix ekle
- `ConvertedConcreteId` — concrete asset ID'yi baska tipe cevir
- `IsConcrete` — somut bir lokasyonla esleyen asset matcher

### Barriers (Tam Liste)
- `TakeWeightCredit`
- `AllowTopLevelPaidExecutionFrom<T>`
- `AllowUnpaidExecutionFrom<T>`
- `AllowExplicitUnpaidExecutionFrom<T, Aliasers>`
- `AllowKnownQueryResponses<ResponseHandler>`
- `AllowSubscriptionsFrom<T>`
- `AllowHrmpNotificationsFromRelayChain`
- `DenyThenTry<Deny, Allow>`
- `DenyReserveTransferToRelayChain`
- `DenyRecursively<Inner>`
- `IsChildSystemParachain<ParaId>`
- `IsSiblingSystemParachain<ParaId, SelfParaId>`
- `IsParentsOnly<Count>`
- `RespectSuspension<Inner, SuspensionChecker>`
- `TrailingSetTopicAsId<InnerBarrier>`
- `WithComputedOrigin<InnerBarrier, LocalUniversal, MaxPrefixes>`

### Asset Adapters
- `FungibleAdapter` — tek fungible token icin tam kapsamli adapter
- `FungibleMutateAdapter` — mint/burn (transfer yok)
- `FungibleTransferAdapter` — sadece transfer
- `FungiblesAdapter` — cok token (Assets pallet gibi)
- `FungiblesMutateAdapter`, `FungiblesTransferAdapter`
- `NonFungibleAdapter`, `NonFungiblesAdapter` vb.

### Weight
- `FixedWeightBounds<T, C, M>` — her instruction icin sabit T weight + Transact icin gercek weight
- `WeightInfoBounds<W, C, M>` — benchmark uretilmis XcmWeightInfo tablosu kullanir
- `FixedRateOfFungible<T, R>` — saniyede sabit token miktari trader
- `UsingComponents<WeightToFee, ...>` — WeightToFee konfi ile trader

### Location Conversion
- `AccountId32Aliases` — AccountId32 junction'i AccountId'ye cevir
- `AccountKey20Aliases` — AccountKey20 (Ethereum-style) icin
- `HashedDescription<AccountId, Describer>` — blake2_256(description) ile sovereign hesap
- `ChildParachainConvertsVia` — child parachain lokasyonunu hesap ID'sine cevir
- `SiblingParachainConvertsVia` — sibling parachain icin
- `ParentIsPreset<AccountId>` — parent lokasyonu onceden ayarlanmis hesap olarak kullan
- `GlobalConsensusConvertsFor` — baska consensus sistemi lokasyonu

### Origin Conversion
- `SovereignSignedViaLocation` — sovereign lokasyondan Signed origin
- `RelayChainAsNative` — relay chain'den Native origin
- `ParentAsSuperuser` — parent chain Superuser
- `SignedAccountId32AsNative` — AccountId32 junction'indan Signed origin
- `ChildParachainAsNative`, `SiblingParachainAsNative`

### Fee Handling
- `XcmFeeManagerFromComponents<WaivedLocations, HandleFee>` — waived listesi + handler
- `SendXcmFeeToAccount<AssetTransactor, ReceiverAccount>` — fee'yi belirli hesaba gonder
- `deposit_or_burn_fee` — fee'yi depozit et veya yak (fallback)

### Routing
- `WithUniqueTopic` — mesajlara unique topic ekle
- `EnsureDecodableXcm` — gondermeden once dekodanabilirlik kontrolu
- `WithComputedOrigin` — computed origin ile barrier calistir

### Transactional
- `FrameTransactionalProcessor` — FRAME'in `with_transaction` kullanarak instruction sarimi

---

## 11. Location Yapisi (v5)

**Dosya:** `polkadot/xcm/src/v5/location.rs`, satir 66-71

```rust
pub struct Location {
    pub parents: u8,           // Kac basamak yukariya cikilacak
    pub interior: Junctions,   // Icerideki junction zinciri
}
pub type InteriorLocation = Junctions;
```

`Junctions` enum'u — 0 ile 8 junction iceren:
```
Here, X1([J;1]), X2([J;2]), ..., X8([J;8])
```

### Junction Tipleri
**Dosya:** `polkadot/xcm/src/v5/junction.rs`, satir 50-100+

```rust
pub enum Junction {
    Parachain(u32),
    AccountId32 { network: Option<NetworkId>, id: [u8; 32] },
    AccountIndex64 { network: Option<NetworkId>, index: u64 },
    AccountKey20 { network: Option<NetworkId>, key: [u8; 20] },
    PalletInstance(u8),
    GeneralIndex(u128),
    GeneralKey { length: u8, data: [u8; 32] },
    OnlyChild,
    Plurality { id: BodyId, part: BodyPart },
    GlobalConsensus(NetworkId),  // v3'te eklendi, v5'te de mevcut
}
```

### Ornek Lokasyonlar
- `Location::here()` = `{ parents: 0, interior: Here }` — bu chain
- `Location::parent()` = `{ parents: 1, interior: Here }` — relay/parent
- `(1, Parachain(1000)).into()` — sibling parachain 1000
- `(0, AccountId32 { network: None, id: [0u8;32] }).into()` — yerel hesap

---

## 12. Error Kodlari (v5)

**Dosya:** `polkadot/xcm/src/v5/traits.rs`, satir 45-178

XCM spec icindeki 36 standart error kodu (index 0-35) + 5 implementation error:

| Index | Hata | Anlam |
|-------|------|-------|
| 0 | `Overflow` | Aritmetik tasman |
| 1 | `Unimplemented` | Instruction desteklenmiyor |
| 2 | `UntrustedReserveLocation` | Guvenilmeyen reserve origin |
| 3 | `UntrustedTeleportLocation` | Guvenilmeyen teleport origin |
| 8 | `AssetNotFound` | Asset bulunamadi |
| 9 | `FailedToTransactAsset` | Asset islemi hatasi |
| 10 | `NotWithdrawable` | Cekilemez (yeterli bakiye yok) |
| 11 | `LocationCannotHold` | Lokasyon varlik tutamaz |
| 13 | `DestinationUnsupported` | Hedef desteklenmiyor |
| 18 | `MaxWeightInvalid` | Transact weight limiti hatali |
| 19 | `NotHoldingFees` | Holding'de fee yok |
| 20 | `TooExpensive` | Fee yetersiz |
| 21 | `Trap(u64)` | Kasitli Trap hatasi |
| 22 | `ExpectationFalse` | Expect kontrolu basarisiz |
| 26 | `HoldingWouldOverflow` | Holding dolacak |
| 29 | `NoDeal` | Anlasma imkansiz |
| 30 | `FeesNotMet` | Fee yetersiz (genel) |
| 32 | `NoPermission` | Izin yok |
| 35 | `TooManyAssets` | Cok fazla asset eslesti |
| — | `Barrier` | Barrier bloke etti |
| — | `WeightLimitReached(Weight)` | Weight limiti asili |
| — | `ExceedsStackLimit` | Recursion limiti asili |

### Outcome
```rust
pub enum Outcome {
    Complete { used: Weight },
    Incomplete { used: Weight, error: InstructionError },
    Error(InstructionError),  // Execution hic baslamadi
}

pub struct InstructionError {
    pub index: InstructionIndex,  // u8 — hatayi veren instruction index'i
    pub error: Error,
}
```

---

## 13. FeeManager ve FeeReason

**Dosya:** `polkadot/xcm/xcm-executor/src/traits/fee_manager.rs`

```rust
pub enum FeeReason {
    Report,
    TransferReserveAsset,
    DepositReserveAsset,
    InitiateReserveWithdraw,
    InitiateTeleport,
    InitiateTransfer,       // v5 yeni
    QueryPallet,
    Export { network: NetworkId, destination: InteriorLocation },
    ChargeFees,
    LockAsset,
    RequestUnlock,
}
```

`XcmFeeManagerFromComponents<WaivedLocations, FeeHandler>`:
```rust
fn is_waived(origin: Option<&Location>, _: FeeReason) -> bool {
    WaivedLocations::contains(loc)  // FeeReason yok sayilir!
}
```
**Dikkat:** Varsayilan implementasyonda FeeReason, muaf tutmak icin kullanilmaz; sadece lokasyon kontrol edilir.

---

## 14. Nova Shield Chain Icin Cikarsimlar (L1 Perspektifi)

### 14.1. XCM Versiyonu Secimi
- **v5'i hedefleyin ama v4 LTS uyumlulugunu koruyun**
- `AlwaysLts = AlwaysV4` — gonderiminlerde v4'e downgrade icin hazir tip mevcut
- `pallet_xcm` versiyon muzakeresi (version negotiation) otomatik yapabilir
- `SubscribeVersion` ile alici chain'in versiyonunu ogrenin, `WrapVersion` ile uygun versiyona cekin
- `check_is_decodable()` metodunu gondermeden once cagirin

### 14.2. Barrier Tasarimi
**L1 relay chain icin onerilen barrier stack:**
```rust
type Barrier = DenyThenTry<
    DenyReserveTransferToRelayChain,   // Relay'e reserve transfer engelle
    (
        TakeWeightCredit,
        WithComputedOrigin<(
            AllowTopLevelPaidExecutionFrom<Everything>,
            AllowKnownQueryResponses<PolkadotXcm>,
            AllowSubscriptionsFrom<Everything>,
        ), UniversalLocation, ConstU32<8>>,
        TrailingSetTopicAsId<...>,
    )
>;
```

**Kritik kararlar:**
- `AllowUnpaidExecutionFrom<Nothing>` — Ucretsiz execution tamamen kapatin (relay'de genellikle kapali)
- Sistem parachain'leri icin `IsChildSystemParachain` + `AllowUnpaidExecutionFrom` kombinasyonu
- `DenyRecursively` eklemeyi dusunun: nested XCM icin recursive filtre

### 14.3. Weight Sistemi
**FixedWeightBounds vs WeightInfoBounds:**
- `FixedWeightBounds` — gelistirme/test icin; her instruction ayni sabit weight alir
- `WeightInfoBounds` — uretim icin; benchmark uretilmis agirliklar kullanilir
- `WeightInfoBounds` icin her XCM instruction'i icin benchmark yazmak gerekir (`pallet_xcm_benchmarks`)

**UsingComponents vs FixedRateOfFungible:**
- `UsingComponents` — runtime'in WeightToFee ile entegre; fee multiplier degiskenligine uyar
- `FixedRateOfFungible` — XCM icin sabit deger; kolaydir ama dinamik fee yok

### 14.4. Asset Transfer Guvenlik Kurallari
1. **Reserve asset trust chain olusturun:** Hangi chain'e hangi asset icin guveniyor?
2. **`IsReserve` ve `IsTeleporter` config'ini dikkatli ayarlayin:** Bos birakmayin
3. **`ReserveAssetDeposited` sonrasi her zaman `ClearOrigin` kullanin:** `AllowTopLevelPaidExecutionFrom` bunu enforce eder
4. **CheckingAccount icin guvenilir bir hesap kullanin:** Teleport accounting'in sagliginii saglar
5. **`DenyReserveTransferToRelayChain` kullanin:** Relay chain'e kaza ile reserve transfer olusturmaktan kacanin

### 14.5. Teleport vs Reserve — Ne Zaman Hangisi?
| Senaryo | Yontem |
|---------|--------|
| Relay chain native token (DOT/KSM) -> Parachain | Teleport (relay kaynagi) |
| Parachain token -> Relay chain | Reserve transfer (parachain sovereign account'ta lock) |
| Parachain A -> Parachain B (ortak asset) | Reserve transfer via relay chain |
| Nova Shield kendi tokeni -> diger chain | Reserve (Nova Shield = reserve chain) |
| Nova Shield native token icin diger chain'e izin | IsTeleporter listesine ekle |

### 14.6. construct_runtime! Index Yonetimi
- **Index 0** = `frame_system` (zorunlu)
- **Index'ler sabit olsun:** Sonradan degistirmek call dispatch'i bozar
- **pallet_xcm eklenecekse:** Genellikle 51 veya bos bir index uygundur
- `pallet_xcm_benchmarks` + `pallet-xcm-benchmarks` crate'ini benchmark icin runtime'a ekleyin

### 14.7. FeeManager Tasarimi
```rust
// Onerilen pattern:
pub type FeeManager = XcmFeeManagerFromComponents<
    WaivedLocations,      // fee waived tutulacak lokasyonlar (sistem palletleri vb.)
    (
        SendXcmFeeToAccount<AssetTransactor, TreasuryAccount>,
        // Depozit olmazsa yak
    ),
>;
```

### 14.8. PayFees vs BuyExecution
- **`BuyExecution`** (v3/v4): Holding Register'dan asset ceker, weight satin alir
- **`PayFees`** (v5): Holding'den asset ceker, `fees` register'a koyar; delivery fee'leri de buradan odenebilir; sonraki `PayFees` no-op
- **L1 icin tavsiye:** Her iki instruction'i da destekleyin (geriye donuk uyumluluk)

### 14.9. Kritik Guvenlik: Holding Register Trap
```rust
// post_process() icinde:
let claimer = self.asset_claimer  // SetHints { AssetClaimer } ile ayarlanabilir
    .as_ref()
    .or(self.context.origin.as_ref())
    .unwrap_or(&self.original_origin);
Config::AssetTrap::drop_assets(claimer, self.holding, &self.context);
```
**Tasarim karari:** `AssetTrap` implementasyonunuz varliklarini phaseable bir storage'da saklayip `ClaimAsset` ile geri alinabilir yapin. Aksi takdirde kullanici fondlari kaybolur.

### 14.10. v5 InitiateTransfer — Multi-hop Kolayligi
```rust
InitiateTransfer {
    destination: Location,
    remote_fees: Option<AssetTransferFilter>,  // None = UnpaidExecution ekle
    preserve_origin: bool,
    assets: BoundedVec<AssetTransferFilter, MaxAssetTransferFilters>,
    remote_xcm: Xcm<()>,
}
```
Onceki versiyonlarda multi-hop icin elle `InitiateReserveWithdraw + InitiateTeleport` kombinasyonu gerekiyordu. v5'te tek instruction yeterli. **MaxAssetTransferFilters = 6** limitini asmayin.

### 14.11. Recursion Limiti ve DenyRecursively
- `RECURSION_LIMIT = 10` — nested `SetErrorHandler/SetAppendix/ExecuteWithOrigin` icin
- `DenyRecursively<Inner>` ile belirli instruction'larin nested olarak calismasini engelleyin
- Stack tasmasi `ExceedsStackLimit` hatasini tetikler

### 14.12. MAX_ITEMS_IN_ASSETS = 20 Limiti
Assets tipindeki maksimum item sayisi 20'dir. Decoding sirasinda enforce edilir. Nova Shield'de transfer fonksiyonlariniz bu limiti asmayi denerlerse `check_is_decodable()` ile onceden kontrol edin.

---

## Ozet: En Kritik Bulgular

1. **XCM versiyonu:** v5 latest, v4 LTS; en dusuk desteklenen v3; hepsi VersionedXcm ile runtime'da eslesir
2. **Location yapisi:** `{ parents: u8, interior: Junctions }` — relative path; buyuk parent = relay chain yonunde
3. **Barrier stack sirasi onemli:** `DenyThenTry(Deny, Allow)` - Once deny, sonra allow
4. **AllowTopLevelPaidExecutionFrom:** MAX 2 asset ile `BuyExecution`'i arar; weight_limit mutate eder
5. **Reserve vs Teleport:** IsReserve ve IsTeleporter config'i kritik; yanlis config = mint exploit
6. **Asset Trap:** Holding bos kalmazsa trap; `ClaimAsset` ile geri alinabilir olmali
7. **FeeManager:** Muafiyet sadece lokasyon bazli (FeeReason yok sayilir); `handle_fee` burn veya transfer
8. **PayFees (v5):** BuyExecution'in halefi; Subsequent PayFees no-op; delivery fees ayni asset'ten
9. **DealWithFees:** Polkadot: 80% treasury, 20% author, tips 100% author
10. **RECURSION_LIMIT = 10:** Nested XCM execution limiti; DenyRecursively ile manuel kontrol
11. **MAX_INSTRUCTIONS_TO_DECODE = 100, MAX_XCM_DECODE_DEPTH = 8, MAX_ITEMS_IN_ASSETS = 20**
12. **WeightInfoBounds:** Uretim icin benchmark-generated agirlik tablosu; FixedWeightBounds sadece test'te
13. **CheckingAccount (teleport):** None birakmak = guvensiz; MintLocation dogru ayarlanmali
14. **v5 InitiateTransfer:** Multi-hop transfer artik tek instruction'da; MaxAssetTransferFilters = 6
15. **SetHints + AssetClaimer:** Trapped asset'lerin claimer'ini trap'tan once belirle
