# Substrate Executive + Küçük Palletler — Agent 08

**Analiz Tarihi:** 2026-07-22
**Kaynak:** `/research/chains/polkadot-sdk/substrate/frame/`
**Kapsanan Palletler:**
- `frame/executive/` — 2 dosya (lib.rs, tests.rs)
- `frame/sudo/` — 6 dosya (lib.rs, extension.rs, benchmarking.rs, mock.rs, tests.rs, weights.rs)
- `frame/timestamp/` — 5 dosya (lib.rs, mock.rs, tests.rs, benchmarking.rs, weights.rs)
- `frame/authorship/` — 1 dosya (lib.rs, tests dahil)
- `frame/utility/` — 4 dosya (lib.rs, tests.rs, benchmarking.rs, weights.rs)

---

## KISIM 1: frame-executive

### Genel Mimari

`frame-executive` bir pallet değil, runtime için **orkestrasyon katmanıdır**. Blok yaşam döngüsünü yönetir, extrinsic'leri sıralı işler, hook'ları tetikler. Diğer tüm palletler `Executive` üzerinden çalışır.

Kaynak yolda iki dosya:
- `src/lib.rs` — 1023 satır, ana implementasyon
- `src/tests.rs` — ~2000+ satır, kapsamlı test suite

### Executive Struct ve Generic Parametreleri

```rust
// lib.rs:224-240
pub struct Executive<
    System,
    Block,
    Context,
    UnsignedValidator,
    AllPalletsWithSystem,
    OnRuntimeUpgrade = (),  // DEPRECATED — September 2026'da kaldırılacak
>(PhantomData<(...)>);
```

Generic parametreler:
- `System`: `frame_system::Config + IsInherent<Block::Extrinsic>` — sistem pallet config
- `Block`: `traits::Block<Header=..., Hash=...>` — blok tipi
- `Context`: `Default` — transaction context (imza doğrulama için)
- `UnsignedValidator`: unsigned transaction doğrulayıcı
- `AllPalletsWithSystem`: tuple — TÜM pallet hook'ları bu üzerinden çağrılır
- `COnRuntimeUpgrade`: `OnRuntimeUpgrade` — DEPRECATED legacy parametre

Kritik durum: `OnRuntimeUpgrade` parametresi deprecated ve Eylül 2026 sonrası kaldırılacak. Artık `frame_system::Config::SingleBlockMigrations` kullanılıyor.

### Blok İşleme Pipeline — execute_block

```rust
// lib.rs:690-723
pub fn execute_block(block: Block::LazyBlock) {
    sp_io::init_tracing();
    sp_tracing::within_span! {
        sp_tracing::info_span!("execute_block", ?block);

        // 1. ADIM: initialize_block — on_runtime_upgrade + on_initialize
        let mode = Self::initialize_block(block.header());

        // 2. ADIM: initial_checks — parent_hash doğrula
        Self::initial_checks(block.header());

        let extrinsics = block.extrinsics();

        // 3. ADIM: apply_extrinsics — tüm extrinsic'leri işle
        if let Err(e) = Self::apply_extrinsics(
            mode,
            extrinsics,
            |uxt, is_inherent| {
                Self::do_apply_extrinsic(uxt, is_inherent, Block::Extrinsic::check)
            }
        ) {
            panic!("{:?}", e)
        }

        // 4. ADIM: inherents_applied (eğer transaction yoksa)
        if !<frame_system::Pallet<System>>::inherents_applied() {
            Self::inherents_applied();
        }

        // 5. ADIM: note_finished_extrinsics + PostTransactions hook
        <frame_system::Pallet<System>>::note_finished_extrinsics();
        <System as frame_system::Config>::PostTransactions::post_transactions();

        let header = block.header();

        // 6. ADIM: on_idle — arta kalan weight varsa çalış
        Self::on_idle_hook(*header.number());

        // 7. ADIM: on_finalize — tüm pallet on_finalize hook'ları
        Self::on_finalize_hook(*header.number());

        // 8. ADIM: pending code upgrade uygula
        <frame_system::Pallet<System>>::maybe_apply_pending_code_upgrade();

        // 9. ADIM: final_checks — state root, extrinsics root, digest doğrula
        Self::final_checks(&header);
    }
}
```

**Mermaid diyagramı (kaynak kodda var):**
```
execute_block → on_runtime_upgrade → System::initialize → on_initialize
→ PreInherents → Apply Inherents → PostInherents
→ MBM ongoing? YES → post_transactions → MBMs::step → on_finalize
→ MBM ongoing? NO → on_poll → Apply Transactions → post_transactions
→ Weight remaining? YES → on_idle → on_finalize
→ Weight remaining? NO → on_finalize
```

### initialize_block

```rust
// lib.rs:596-605
pub fn initialize_block(
    header: &frame_system::pallet_prelude::HeaderFor<System>,
) -> ExtrinsicInclusionMode {
    sp_io::init_tracing();
    sp_tracing::enter_span!(sp_tracing::Level::TRACE, "init_block");
    let digests = Self::extract_pre_digest(header);
    Self::initialize_block_impl(header.number(), header.parent_hash(), &digests);
    Self::extrinsic_mode()
}
```

Döndürdüğü `ExtrinsicInclusionMode`:
- `AllExtrinsics` — normal durum
- `OnlyInherents` — MultiBlockMigration devam ediyorsa

```rust
// lib.rs:607-613
fn extrinsic_mode() -> ExtrinsicInclusionMode {
    if <System as frame_system::Config>::MultiBlockMigrator::ongoing() {
        ExtrinsicInclusionMode::OnlyInherents
    } else {
        ExtrinsicInclusionMode::AllExtrinsics
    }
}
```

### initialize_block_impl — Detaylı Adımlar

```rust
// lib.rs:625-666
fn initialize_block_impl(
    block_number: &BlockNumberFor<System>,
    parent_hash: &System::Hash,
    digest: &Digest,
) {
    // 1. Event'leri sıfırla (runtime upgrade event'leri korunacak)
    <frame_system::Pallet<System>>::reset_events();

    let mut weight = Weight::zero();

    // 2. Runtime upgrade kontrolü
    if Self::runtime_upgraded() {
        weight = weight.saturating_add(Self::execute_on_runtime_upgrade());

        // LastRuntimeUpgrade'i güncelle
        frame_system::LastRuntimeUpgrade::<System>::put(
            frame_system::LastRuntimeUpgradeInfo::from(
                <System::Version as frame_support::traits::Get<_>>::get(),
            ),
        );
    }

    // 3. System initialize
    <frame_system::Pallet<System>>::initialize(block_number, parent_hash, digest);

    // 4. Base block weight + runtime upgrade weight kaydet
    weight = System::BlockWeights::get().base_block.saturating_add(weight);
    <frame_system::Pallet<System>>::register_extra_weight_unchecked(
        weight,
        DispatchClass::Mandatory,
    );

    // 5. Tüm pallet on_initialize hook'ları — HER BİRİ AYRI KAYDEDILIR
    weight = weight.saturating_add(
        <AllPalletsWithSystem as OnInitializeWithWeightRegistration<System>>
            ::on_initialize_with_weight_registration(*block_number)
    );

    // 6. note_finished_initialize
    frame_system::Pallet::<System>::note_finished_initialize();

    // 7. PreInherents hook
    <System as frame_system::Config>::PreInherents::pre_inherents();
}
```

### OnInitializeWithWeightRegistration Trait'i

Bu trait kritik — her pallet'in `on_initialize` weight'i ayrı ayrı `Mandatory` class olarak kaydedilir:

```rust
// lib.rs:529-548
frame_support::impl_for_tuples_attr! {
    #[tuple_types_custom_trait_bound(OnInitialize<...>)]
    impl<T: frame_system::Config> OnInitializeWithWeightRegistration<T> for Tuple {
        fn on_initialize_with_weight_registration(n: BlockNumberFor<T>) -> Weight {
            let mut weight = Weight::zero();
            for_tuples!( #(
                let individual_weight = Tuple::on_initialize(n);

                // Her pallet ayrı ayrı register edilir
                <frame_system::Pallet<T>>::register_extra_weight_unchecked(
                    individual_weight,
                    DispatchClass::Mandatory,
                );

                weight = weight.saturating_add(individual_weight);
            )* );
            weight
        }
    }
}
```

**Nova Shield için kritik:** Her pallet `on_initialize` weight'i `Mandatory` class'a gidiyor — bu block weight hesabını etkiler.

### apply_extrinsic — Tek Extrinsic İşleme

```rust
// lib.rs:914-917
pub fn apply_extrinsic(uxt: Block::Extrinsic) -> ApplyExtrinsicResult {
    let is_inherent = System::is_inherent(&uxt);
    Self::do_apply_extrinsic(uxt, is_inherent, Block::Extrinsic::check)
}
```

`do_apply_extrinsic` içinde tam akış:

```rust
// lib.rs:857-908
fn do_apply_extrinsic(
    uxt: Block::Extrinsic,
    is_inherent: bool,
    check: impl FnOnce(Block::Extrinsic, &Context) -> Result<...>,
) -> ApplyExtrinsicResult {
    // 1. Encode — uzunluk hesabı için
    let encoded = uxt.encode();
    let encoded_len = encoded.len();

    // 2. Depth-limited decode (MAX_EXTRINSIC_DEPTH = 8)
    let uxt = <Block::Extrinsic as codec::DecodeLimit>::decode_all_with_depth_limit(
        MAX_EXTRINSIC_DEPTH,
        &mut &encoded[..],
    ).map_err(|_| InvalidTransaction::Call)?;

    // 3. İMZA DOĞRULAMA — check(uxt, &Context::default())
    // Bu TransactionExtension pipeline'ını tetikler
    let xt = check(uxt, &Context::default())?;

    // 4. DispatchInfo al
    let dispatch_info = xt.get_dispatch_info();

    // 5. Eğer non-inherent ve henüz inherents_applied değilse — geçiş yap
    if !is_inherent && !<frame_system::Pallet<System>>::inherents_applied() {
        Self::inherents_applied();
    }

    // 6. note_extrinsic — extrinsic hash'i kaydet
    <frame_system::Pallet<System>>::note_extrinsic(encoded);

    // *** AUDIT NOTU: Bu noktadan sonra panic olmaz ***

    // 7. Applyable::apply — dispatch et
    let r = Applyable::apply::<UnsignedValidator>(xt, &dispatch_info, encoded_len)?;

    // 8. Mandatory extrinsic (inherent) başarısız olamaz
    if r.is_err() && dispatch_info.class == DispatchClass::Mandatory {
        return Err(InvalidTransaction::BadMandatory.into());
    }

    // 9. note_applied_extrinsic — sonucu kaydet
    <frame_system::Pallet<System>>::note_applied_extrinsic(&r, dispatch_info);

    Ok(r.map(|_| ()).map_err(|e| e.error))
}
```

**Kritik AUDIT notu (lib.rs:893):** `// AUDIT: Under no circumstances may this function panic from here onwards.` — İmza doğrulamasından sonra panic yasak. Bu güvenlik gereksinimidir.

### apply_extrinsics — Extrinsic Sıralama Kuralı

```rust
// lib.rs:744-779
fn apply_extrinsics(
    mode: ExtrinsicInclusionMode,
    extrinsics: impl Iterator<Item = Result<Block::Extrinsic, codec::Error>>,
    mut apply_extrinsic: impl FnMut(Block::Extrinsic, bool) -> ApplyExtrinsicResult,
) -> Result<(), ExecutiveError> {
    let mut first_non_inherent_idx = 0;

    for (idx, maybe_uxt) in extrinsics.into_iter().enumerate() {
        let uxt = maybe_uxt.map_err(|_| ExecutiveError::UnableToDecodeExtrinsic)?;
        let is_inherent = System::is_inherent(&uxt);

        if is_inherent {
            // KURAL: inherent'ler ÖNCE gelmeli
            // Eğer araya non-inherent girdi ise → hata
            if first_non_inherent_idx != idx {
                return Err(ExecutiveError::InvalidInherentPosition(idx));
            }
            first_non_inherent_idx += 1;
        } else {
            // MBM devam ediyorsa non-inherent yasak
            if mode == ExtrinsicInclusionMode::OnlyInherents {
                return Err(ExecutiveError::OnlyInherentsAllowed);
            }
        }

        if let Err(e) = apply_extrinsic(uxt, is_inherent) {
            // Hata logla ve dur — blok execute başarısız
            return Err(ExecutiveError::ApplyExtrinsic(e.into()));
        }
    }
    Ok(())
}
```

**Kural:** Inherent'ler blokun BAŞINDA sıralı gelmeli. Araya signed transaction giremez. Bu pozisyonlar katı şekilde korunur.

### finalize_block

```rust
// lib.rs:784-800
pub fn finalize_block() -> frame_system::pallet_prelude::HeaderFor<System> {
    sp_io::init_tracing();
    sp_tracing::enter_span!(sp_tracing::Level::TRACE, "finalize_block");

    // 1. inherents_applied kontrolü (transaction yoksa geçiş)
    if !<frame_system::Pallet<System>>::inherents_applied() {
        Self::inherents_applied();
    }

    // 2. note_finished_extrinsics
    <frame_system::Pallet<System>>::note_finished_extrinsics();

    // 3. PostTransactions hook
    <System as frame_system::Config>::PostTransactions::post_transactions();

    let block_number = <frame_system::Pallet<System>>::block_number();

    // 4. on_idle
    Self::on_idle_hook(block_number);

    // 5. on_finalize
    Self::on_finalize_hook(block_number);

    // 6. Pending code upgrade
    <frame_system::Pallet<System>>::maybe_apply_pending_code_upgrade();

    // 7. finalize() — header döndür
    <frame_system::Pallet<System>>::finalize()
}
```

`finalize_block` sadece **block builder** (node tarafı) tarafından çağrılır — `execute_block`'ta final_checks yapılır ama `finalize_block`'ta yapılmaz.

### inherents_applied — MBM / on_poll Geçişi

```rust
// lib.rs:728-742
pub fn inherents_applied() {
    // 1. note_inherents_applied
    <frame_system::Pallet<System>>::note_inherents_applied();

    // 2. PostInherents hook
    <System as frame_system::Config>::PostInherents::post_inherents();

    if <System as frame_system::Config>::MultiBlockMigrator::ongoing() {
        // MBM devam ediyor → bir step at
        let used_weight = <System as frame_system::Config>::MultiBlockMigrator::step();
        <frame_system::Pallet<System>>::register_extra_weight_unchecked(
            used_weight,
            DispatchClass::Mandatory,
        );
    } else {
        // Normal blok → on_poll çalıştır
        let block_number = <frame_system::Pallet<System>>::block_number();
        Self::on_poll_hook(block_number);
    }
}
```

**MBM (Multi-Block Migration) varken:** normal transaction'lar kabul edilmez, her blokta bir migration step atlanır. `on_poll` çalışmaz.

### on_idle_hook

```rust
// lib.rs:804-823
fn on_idle_hook(block_number: NumberFor<Block>) {
    // MBM devam ediyorsa on_idle hiç çalışmaz
    if <System as frame_system::Config>::MultiBlockMigrator::ongoing() {
        return;
    }

    let weight = <frame_system::Pallet<System>>::block_weight();
    let max_weight = <System::BlockWeights as frame_support::traits::Get<_>>::get().max_block;
    let remaining_weight = max_weight.saturating_sub(weight.total());

    // Arta kalan weight varsa on_idle çalışır
    if remaining_weight.all_gt(Weight::zero()) {
        let used_weight = <AllPalletsWithSystem as OnIdle<BlockNumberFor<System>>>::on_idle(
            block_number,
            remaining_weight,
        );
        <frame_system::Pallet<System>>::register_extra_weight_unchecked(
            used_weight,
            DispatchClass::Mandatory,
        );
    }
}
```

**on_idle çalışma koşulları:**
1. MBM aktif değil
2. `remaining_weight > 0` — block tamamen dolmamış

### on_poll_hook

```rust
// lib.rs:825-846
fn on_poll_hook(block_number: NumberFor<Block>) {
    // defensive_assert: MBM aktif olmamalı
    defensive_assert!(
        !<System as frame_system::Config>::MultiBlockMigrator::ongoing(),
        "on_poll should not be called during migrations"
    );

    let weight = <frame_system::Pallet<System>>::block_weight();
    let max_weight = ...;
    let remaining = max_weight.saturating_sub(weight.total());

    if remaining.all_gt(Weight::zero()) {
        let mut meter = WeightMeter::with_limit(remaining);
        <AllPalletsWithSystem as OnPoll<BlockNumberFor<System>>>::on_poll(
            block_number,
            &mut meter,
        );
        <frame_system::Pallet<System>>::register_extra_weight_unchecked(
            meter.consumed(),
            DispatchClass::Mandatory,
        );
    }
}
```

`on_poll` inherent'ler uygulandıktan SONRA, transaction'lar uygulanmadan ÖNCE çalışır. `WeightMeter` ile kontrollü.

### SignedExtra / TransactionExtension Validation Pipeline

`do_apply_extrinsic`'te `check(uxt, &Context::default())` çağrısı `Checkable` trait'ini tetikler. Bu `TransactionExtension` pipeline'ıdır.

Doğrulama zinciri (kaynak: sp_runtime traits):
```
UncheckedExtrinsic::check(&Context)
  → signature doğrula (Ed25519/Sr25519/ECDSA)
  → TransactionExtension tuple'ını validate
    → CheckNonZeroSender
    → CheckSpecVersion
    → CheckTxVersion
    → CheckGenesis
    → CheckEra (mortality check)
    → CheckNonce
    → CheckWeight
    → ChargeTransactionPayment
    → ...
```

`validate_transaction` (RPC/pool için):

```rust
// lib.rs:952-994
pub fn validate_transaction(
    source: TransactionSource,
    uxt: Block::Extrinsic,
    block_hash: Block::Hash,
) -> TransactionValidity {
    // Sonraki block numarasıyla initialize
    <frame_system::Pallet<System>>::initialize(
        &(frame_system::Pallet::<System>::block_number() + One::one()),
        &block_hash,
        &Default::default(),
    );

    let encoded = uxt.encode();
    let uxt = decode_with_depth_limit(...)?;

    // check — signature doğrula
    let xt = uxt.check(&Default::default())?;
    let dispatch_info = xt.get_dispatch_info();

    // Mandatory extrinsic pool'a giremez
    if dispatch_info.class == DispatchClass::Mandatory {
        return Err(InvalidTransaction::MandatoryValidation.into());
    }

    // validate — TransactionExtension chain validation
    xt.validate::<UnsignedValidator>(source, &dispatch_info, encoded.len())
}
```

**Kritik kural:** `Mandatory` extrinsic'ler (inherent'ler) transaction pool'a hiç giremez — doğrudan `MandatoryValidation` hatası.

### Runtime Upgrade Detection

```rust
// lib.rs:669-674
fn runtime_upgraded() -> bool {
    let last = frame_system::LastRuntimeUpgrade::<System>::get();
    let current = <System::Version as frame_support::traits::Get<_>>::get();
    last.map(|v| v.was_upgraded(&current)).unwrap_or(true)
}
```

`LastRuntimeUpgrade` storage'daki version ile `System::Version` karşılaştırılır. İlk blokta (genesis) `last = None` → `unwrap_or(true)` → upgrade çalışır.

### execute_on_runtime_upgrade

```rust
// lib.rs:580-593
pub fn execute_on_runtime_upgrade() -> Weight {
    let before_all_weight =
        <AllPalletsWithSystem as BeforeAllRuntimeMigrations>::before_all_runtime_migrations();

    let runtime_upgrade_weight = <(
        COnRuntimeUpgrade,                              // 1. Legacy (deprecated)
        <System as frame_system::Config>::SingleBlockMigrations, // 2. Yeni sistem
        AllPalletsWithSystem,                           // 3. Pallet on_runtime_upgrade'leri
    ) as OnRuntimeUpgrade>::on_runtime_upgrade();

    before_all_weight.saturating_add(runtime_upgrade_weight)
}
```

Sıra önemli: `before_all_runtime_migrations` → `SingleBlockMigrations` → `AllPallets::on_runtime_upgrade`.

### final_checks

```rust
// lib.rs:919-945
fn final_checks(header: &frame_system::pallet_prelude::HeaderFor<System>) {
    // 1. finalize() — geçici storage temizle
    let new_header = <frame_system::Pallet<System>>::finalize();

    // 2. Digest log sayısı eşleşmeli
    assert_eq!(
        header.digest().logs().len(),
        new_header.digest().logs().len(),
        "Number of digest items must match that calculated."
    );

    // 3. Her digest item eşleşmeli
    let items_zip = header.digest().logs().iter().zip(new_header.digest().logs().iter());
    for (header_item, computed_item) in items_zip {
        header_item.check_equal(computed_item);
        assert!(header_item == computed_item, "Digest item must match that calculated.");
    }

    // 4. State root eşleşmeli (Merkle root)
    let storage_root = new_header.state_root();
    header.state_root().check_equal(storage_root);
    assert!(header.state_root() == storage_root, "Storage root must match that calculated.");

    // 5. Extrinsics trie root eşleşmeli
    assert!(
        header.extrinsics_root() == new_header.extrinsics_root(),
        "Transaction trie root must be valid.",
    );
}
```

3 kritik doğrulama: **state root**, **extrinsics root**, **digest**. Biri uyuşmazsa blok panik ile reddedilir.

### offchain_worker

```rust
// lib.rs:997-1022
pub fn offchain_worker(header: &frame_system::pallet_prelude::HeaderFor<System>) {
    let digests = header.digest().clone();

    // Bilinmeyen log'ları deposit et
    let existing_digest = frame_system::Pallet::<System>::digest();
    for digest in digests.logs().iter().filter(|d| !existing_digest.logs.contains(d)) {
        frame_system::Pallet::<System>::deposit_log(digest.clone());
    }

    // Intra-block entropy initialize
    frame_system::Pallet::<System>::initialize_intra_block_entropy(header.parent_hash());

    // Bu bloğun hash'ini ekle (normalde bilinmez ama offchain'de biliniyor)
    frame_system::BlockHash::<System>::insert(header.number(), header.hash());

    <AllPalletsWithSystem as OffchainWorker<BlockNumberFor<System>>>::offchain_worker(
        *header.number(),
    )
}
```

### ExecutiveError Tipleri

```rust
// lib.rs:182-209
pub enum ExecutiveError {
    UnableToDecodeExtrinsic,          // codec hatası
    InvalidInherentPosition(usize),    // inherent yanlış sıraya geldi
    OnlyInherentsAllowed,              // MBM aktifken signed tx geldi
    ApplyExtrinsic(TransactionValidityError),  // dispatch hatası
    Custom(&'static str),              // try-runtime için
}
```

### try-runtime Özellikleri

`try-runtime` feature aktifken ek metodlar:

1. `try_execute_block` — imza ve state root check'leri opsiyonel
2. `try_runtime_upgrade` — migration test
3. `try_runtime_upgrade_with_config` — granüler config ile
4. `log_decode_result` — tüm storage decode edilebilir mi?

```rust
// lib.rs:429-431
pub fn try_runtime_upgrade(checks: UpgradeCheckSelect) -> Result<Weight, TryRuntimeError> {
    Self::try_runtime_upgrade_with_config(TryRuntimeUpgradeConfig::new(checks))
}
```

---

## KISIM 2: pallet-sudo

### Genel Bakış

`pallet-sudo` production için geçici bir araçtır — genesis'te belirlenen tek bir `AccountId`'ye Root origin gücü verir. Mainnet'te kaldırılması gerekir.

Dosyalar: `lib.rs` (365 satır) + `extension.rs` (124 satır) + `benchmarking.rs` (122 satır) + `mock.rs` (144 satır) + `tests.rs` (227 satır)

### Storage

```rust
// lib.rs:332-333
#[pallet::storage]
pub type Key<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;
```

Tek storage: `Key<T>` — sudo hesabı. `OptionQuery` → set edilmemişse `None` döner.

**GenesisConfig:**
```rust
// lib.rs:335-347
#[pallet::genesis_config]
pub struct GenesisConfig<T: Config> {
    pub key: Option<T::AccountId>,
}

#[pallet::genesis_build]
impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
    fn build(&self) {
        Key::<T>::set(self.key.clone());
    }
}
```

### ensure_sudo — Yetki Kontrolü

```rust
// lib.rs:351-364
pub(crate) fn ensure_sudo(origin: OriginFor<T>) -> DispatchResult {
    let sender = ensure_signed_or_root(origin)?;

    if let Some(sender) = sender {
        // Signed origin: Key ile karşılaştır
        if Key::<T>::get().map_or(false, |k| k == sender) {
            Ok(())
        } else {
            Err(Error::<T>::RequireSudo.into())
        }
    } else {
        // Root origin: Her zaman geçerli
        Ok(())
    }
}
```

**Kritik:** `ensure_signed_or_root` hem signed hem de root origin'i kabul eder. Root origin sudo key olmadan da çalışır — bu önemli bir davranış.

### sudo Dispatchable

```rust
// lib.rs:194-213
pub fn sudo(
    origin: OriginFor<T>,
    call: Box<<T as Config>::RuntimeCall>,
) -> DispatchResultWithPostInfo {
    Self::ensure_sudo(origin)?;

    // dispatch_bypass_filter: BaseCallFilter bypass et
    let res = call.dispatch_bypass_filter(RawOrigin::Root.into());
    Self::deposit_event(Event::Sudid { sudo_result: res.map(|_| ()).map_err(|e| e.error) });

    // Sudo user hiç fee ödemez
    Ok(Pays::No.into())
}
```

**Önemli noktalar:**
1. `dispatch_bypass_filter` — `BaseCallFilter` atlanır, her call çalışabilir
2. `RawOrigin::Root.into()` — Root origin ile dispatch
3. `Pays::No` — sudo kullanıcısı ücretsiz
4. Event her zaman emit edilir (başarı veya hata)
5. Weight: `T::WeightInfo::sudo() + call.get_dispatch_info().call_weight`

### sudo_unchecked_weight

```rust
// lib.rs:222-235
#[pallet::weight((*weight, call.get_dispatch_info().class))]
pub fn sudo_unchecked_weight(
    origin: OriginFor<T>,
    call: Box<<T as Config>::RuntimeCall>,
    weight: Weight,  // Kullanıcı belirlediği weight
) -> DispatchResultWithPostInfo {
    Self::ensure_sudo(origin)?;
    let _ = weight; // Weight witness kontrolü yapılmaz

    let res = call.dispatch_bypass_filter(RawOrigin::Root.into());
    Self::deposit_event(Event::Sudid { sudo_result: res.map(|_| ()).map_err(|e| e.error) });

    Ok(Pays::No.into())
}
```

**Fark:** Weight extrinsic dispatch info'sundan değil, çağırandan alınır. `let _ = weight` — gerçek weight kontrol edilmez. Bu tehlikelidir ama sudo güvenilir olduğu varsayılır.

### sudo_as

```rust
// lib.rs:267-282
pub fn sudo_as(
    origin: OriginFor<T>,
    who: AccountIdLookupOf<T>,
    call: Box<<T as Config>::RuntimeCall>,
) -> DispatchResultWithPostInfo {
    Self::ensure_sudo(origin)?;

    let who = T::Lookup::lookup(who)?;

    // Signed origin ile dispatch — Root değil!
    let res = call.dispatch_bypass_filter(RawOrigin::Signed(who).into());
    Self::deposit_event(Event::SudoAsDone {
        sudo_result: res.map(|_| ()).map_err(|e| e.error),
    });

    Ok(Pays::No.into())
}
```

**Kritik fark:** `RawOrigin::Signed(who)` — Root değil. Yani `ensure_root` gerektiren çağrılar başarısız olur! Sadece signed origin gerektiren çağrılar çalışır.

Test'te doğrulandı:
```rust
// tests.rs:188-194
fn sudo_as_basics() {
    // A privileged function will not work when passed to `sudo_as`.
    let call = privileged_i32_log { ... }; // ensure_root gerektirir
    assert_ok!(Sudo::sudo_as(RuntimeOrigin::signed(1), 2, call));
    // Çalıştı ama log boş — çünkü ensure_root başarısız oldu!
    assert!(Logger::i32_log().is_empty());
}
```

### set_key ve remove_key

```rust
// lib.rs:240-297
pub fn set_key(origin: OriginFor<T>, new: AccountIdLookupOf<T>) -> DispatchResultWithPostInfo {
    Self::ensure_sudo(origin)?;
    let new = T::Lookup::lookup(new)?;
    Self::deposit_event(Event::KeyChanged { old: Key::<T>::get(), new: new.clone() });
    Key::<T>::put(new);
    Ok(Pays::No.into())
}

pub fn remove_key(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
    Self::ensure_sudo(origin)?;
    Self::deposit_event(Event::KeyRemoved {});
    Key::<T>::kill(); // Storage tamamen sil
    Ok(Pays::No.into())
}
```

`remove_key` geri alınamaz — Key storage tamamen silinir. Belgede **"This cannot be un-done."** uyarısı var.

### Sudo Events

```rust
// lib.rs:302-322
pub enum Event<T: Config> {
    Sudid { sudo_result: DispatchResult },        // sudo, sudo_unchecked_weight
    KeyChanged { old: Option<T::AccountId>, new: T::AccountId },  // set_key
    KeyRemoved,                                    // remove_key
    SudoAsDone { sudo_result: DispatchResult },   // sudo_as
}
```

### CheckOnlySudoAccount — TransactionExtension

`extension.rs` (124 satır) — İlk blok döneminde sadece sudo hesabının transaction göndermesini sağlar:

```rust
// extension.rs:41-43
#[derive(Clone, Eq, PartialEq, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
#[scale_info(skip_type_params(T))]
pub struct CheckOnlySudoAccount<T: Config + Send + Sync>(PhantomData<T>);
```

Validate implementasyonu:

```rust
// extension.rs:90-119
fn validate(
    &self,
    origin: ...,
    call: &...,
    info: &DispatchInfoOf<...>,
    _len: usize,
    ...
) -> Result<(ValidTransaction, Self::Val, ...), TransactionValidityError> {
    // 1. Origin'den signer al
    let who = origin.as_system_origin_signer().ok_or(InvalidTransaction::BadSigner)?;

    // 2. Sudo key al (yoksa CannotLookup)
    let sudo_key: T::AccountId = Key::<T>::get().ok_or(UnknownTransaction::CannotLookup)?;

    // 3. Signer == sudo key olmalı
    ensure!(*who == sudo_key, InvalidTransaction::BadSigner);

    Ok((
        ValidTransaction {
            priority: info.total_weight().ref_time() as TransactionPriority,
            // provides: (signer, call_hash) — replay koruması
            provides: vec![(who, T::Hashing::hash_of(call)).encode()],
            ..Default::default()
        },
        (),
        origin,
    ))
}
```

**Tasarım:** Pool'a giren her transaction sudo key tarafından imzalanmış olmalı. Bu, genesis sonrası dönemde spam koruması sağlar.

### Config Trait

```rust
// lib.rs:171-186
pub trait Config: frame_system::Config {
    type RuntimeEvent: From<Event<Self>> + IsType<...>;
    type RuntimeCall: Parameter
        + UnfilteredDispatchable<RuntimeOrigin = Self::RuntimeOrigin>
        + GetDispatchInfo;
    type WeightInfo: WeightInfo;
}
```

`UnfilteredDispatchable` gereksinimi — `dispatch_bypass_filter` kullanabilmek için.

---

## KISIM 3: pallet-timestamp

### Genel Bakış

Consensus-tabanlı zaman kaynağı. Block author tarafından inherent olarak set edilir. Validator node'ları timestamp'ı doğrular.

### Config Trait

```rust
// lib.rs:169-197
pub trait Config: frame_system::Config {
    type Moment: Parameter
        + Default
        + AtLeast32Bit
        + Scale<BlockNumberFor<Self>, Output = Self::Moment>
        + Copy
        + MaxEncodedLen
        + scale_info::StaticTypeInfo;

    type OnTimestampSet: OnTimestampSet<Self::Moment>;

    #[pallet::constant]
    type MinimumPeriod: Get<Self::Moment>;

    type WeightInfo: WeightInfo;
}
```

`Moment` tipi genellikle `u64` (millisaniye) veya `u32`.

### Storage

```rust
// lib.rs:203-211
/// Şimdiki block timestamp
pub type Now<T: Config> = StorageValue<_, T::Moment, ValueQuery>;

/// Bu blokta timestamp set edildi mi?
pub(super) type DidUpdate<T: Config> = StorageValue<_, bool, ValueQuery>;
```

### Hooks

```rust
// lib.rs:213-231
impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
    // on_initialize: on_finalize için weight rezerve et
    fn on_initialize(_n: BlockNumberFor<T>) -> Weight {
        T::WeightInfo::on_finalize()
    }

    // on_finalize: Timestamp set edilmişse DidUpdate temizle; edilmemişse panic
    fn on_finalize(_n: BlockNumberFor<T>) {
        assert!(DidUpdate::<T>::take(), "Timestamp must be updated once in the block");
    }
}
```

**Önemli:** `on_initialize` `on_finalize` weight'ini döndürür — weight accounting bu şekilde yapılır.

### set — Inherent Call

```rust
// lib.rs:254-273
#[pallet::weight((T::WeightInfo::set(), DispatchClass::Mandatory))]
pub fn set(origin: OriginFor<T>, #[pallet::compact] now: T::Moment) -> DispatchResult {
    ensure_none(origin)?;  // Sadece inherent — None origin

    // Çift güncelleme kontrolü
    assert!(!DidUpdate::<T>::exists(), "Timestamp must be updated only once in the block");

    let prev = Now::<T>::get();

    // MinimumPeriod kontrolü (ilk block hariç — prev.is_zero())
    assert!(
        prev.is_zero() || now >= prev + T::MinimumPeriod::get(),
        "Timestamp must increment by at least <MinimumPeriod> between sequential blocks"
    );

    Now::<T>::put(now);
    DidUpdate::<T>::put(true);

    // Subscriber'ları bildir (örn. Aura, Babe)
    <T::OnTimestampSet as OnTimestampSet<_>>::on_timestamp_set(now);

    Ok(())
}
```

**3 panic durumu:**
1. `ensure_none` başarısız → `BadOrigin` (panic değil, hata)
2. `DidUpdate::exists()` → "Timestamp must be updated only once in the block" (panic)
3. `now < prev + MinimumPeriod` → "Timestamp must increment..." (panic)

**Not:** Test'te `MinimumPeriod = ConstU64<5>`. Önceki timestamp 44, 46 ile set etmeye çalışınca (`46 < 44 + 5 = 49`) panic.

### ProvideInherent — Inherent Mekanizması

```rust
// lib.rs:284-331
#[pallet::inherent]
impl<T: Config> ProvideInherent for Pallet<T> {
    type Call = Call<T>;
    type Error = InherentError;
    const INHERENT_IDENTIFIER: InherentIdentifier = INHERENT_IDENTIFIER; // "timstap0"

    // Block builder: inherent call oluştur
    fn create_inherent(data: &InherentData) -> Option<Self::Call> {
        let inherent_data = data
            .get_data::<InherentType>(&INHERENT_IDENTIFIER)
            .expect("Timestamp inherent data not correctly encoded")
            .expect("Timestamp inherent data must be provided");
        let data = (*inherent_data).saturated_into::<T::Moment>();

        // max(node_time, prev + MinimumPeriod) — ileriye doğru garantili
        let next_time = cmp::max(data, Now::<T>::get() + T::MinimumPeriod::get());
        Some(Call::set { now: next_time })
    }

    // Validator: inherent'i doğrula
    fn check_inherent(call: &Self::Call, data: &InherentData) -> result::Result<(), Self::Error> {
        const MAX_TIMESTAMP_DRIFT_MILLIS: sp_timestamp::Timestamp =
            sp_timestamp::Timestamp::new(30 * 1000);  // 30 saniye

        let t: u64 = match call {
            Call::set { ref now } => (*now).saturated_into::<u64>(),
            _ => return Ok(()),
        };

        let data = ...; // node'dan gelen gerçek zaman

        let minimum = (Now::<T>::get() + T::MinimumPeriod::get()).saturated_into::<u64>();

        if t > *(data + MAX_TIMESTAMP_DRIFT_MILLIS) {
            // 30 saniyeden fazla ileriye gidildi
            Err(InherentError::TooFarInFuture)
        } else if t < minimum {
            // MinimumPeriod geçmedi
            Err(InherentError::TooEarly)
        } else {
            Ok(())
        }
    }

    fn is_inherent(call: &Self::Call) -> bool {
        matches!(call, Call::set { .. })
    }
}
```

**create_inherent:** `max(node_time, prev + MinimumPeriod)` — her zaman MinimumPeriod'u garanti eder.

**check_inherent:** 30 saniye drift toleransı. Bu değer hard-coded.

### Time ve UnixTime Trait Implementasyonları

```rust
// lib.rs:352-379
impl<T: Config> Time for Pallet<T> {
    type Moment = T::Moment;
    fn now() -> Self::Moment {
        Now::<T>::get()
    }
}

impl<T: Config> UnixTime for Pallet<T> {
    fn now() -> core::time::Duration {
        let now = Now::<T>::get();
        if now == T::Moment::zero() {
            log::error!(target: "runtime::timestamp",
                "`pallet_timestamp::UnixTime::now` is called at genesis, invalid value returned: 0");
        }
        core::time::Duration::from_millis(now.saturated_into::<u64>())
    }
}
```

**UnixTime** timestamp milisaniye cinsinden saklar ve `Duration::from_millis` döndürür.

### MinimumPeriod Hakkında

```rust
// Belge yorumu (lib.rs:186-193):
// MinimumPeriod, expected block time'dan FARKLIDIR.
// Aura'da varsayılan olarak 2 * MinimumPeriod süre alır.
// Yani MinimumPeriod = 2000ms ise Aura block time = 4000ms
```

**Test mock'undaki değer:** `ConstU64<5>` — 5ms (test amaçlı, gerçekte çok düşük)

---

## KISIM 4: pallet-authorship

### Genel Bakış

Mevcut block'un yazarını (block producer) takip eder. Ödül mekanizmaları için gereklidir.

Tek dosya: `src/lib.rs` (209 satır, tests dahil)

**Önemli not:** Bu sürüm modern sadeleştirilmiş versiyondur. Uncle block desteği **tamamen kaldırılmıştır**. Eski Substrate sürümlerinde uncle reward mekanizması vardı ama artık yok.

### Config Trait

```rust
// lib.rs:42-48
pub trait Config: frame_system::Config {
    /// Block yazarını digest'ten bul
    type FindAuthor: FindAuthor<Self::AccountId>;
    /// Yazarlık event handler (örn. ödül sistemleri)
    type EventHandler: EventHandler<Self::AccountId, BlockNumberFor<Self>>;
}
```

### EventHandler Trait

```rust
// lib.rs:28-34
#[impl_trait_for_tuples::impl_for_tuples(30)]
pub trait EventHandler<Author, BlockNumber> {
    fn note_author(author: Author);
}
```

`impl_for_tuples(30)` — 30'a kadar tuple implementasyonu. `()` için no-op implementasyon var.

### Storage

```rust
// lib.rs:74-77
#[pallet::storage]
#[pallet::whitelist_storage]  // Trie'dan whitelist — hızlı erişim
pub(super) type Author<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;
```

`whitelist_storage` özniteliği bu storage'ı özel listeye ekler — her block'ta erişildiği için cache'de tutulur.

### Hooks

```rust
// lib.rs:53-72
impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
    fn on_initialize(_: BlockNumberFor<T>) -> Weight {
        if let Some(author) = Self::author() {
            T::EventHandler::note_author(author);
        }
        Weight::zero()
    }

    fn on_finalize(_: BlockNumberFor<T>) {
        // Block bitince Author storage'ı temizle — trie'a yazma!
        <Author<T>>::kill();
    }
}
```

**on_initialize:** Yazar bulunduysa EventHandler'a bildir.
**on_finalize:** Author storage'ı sil. Bu `whitelist_storage` ile birlikte block sonu trie'a yazılmaz.

### author() — Lazy Loading

```rust
// lib.rs:85-96
pub fn author() -> Option<T::AccountId> {
    // Önce memoized storage'a bak
    if let Some(author) = <Author<T>>::get() {
        return Some(author);
    }

    // Digest'ten bul
    let digest = <frame_system::Pallet<T>>::digest();
    let pre_runtime_digests = digest.logs.iter().filter_map(|d| d.as_pre_runtime());
    T::FindAuthor::find_author(pre_runtime_digests).inspect(|a| {
        // Bulunduysa memoize et
        <Author<T>>::put(&a);
    })
}
```

**Lazy pattern:** İlk çağrıda digest parse edilir, cache'lenir. Sonraki çağrılar storage'dan okur.

### FindAuthor Trait

`FindAuthor` consensus engine tarafından implement edilir:
- Aura: slot numarasına göre yazar belirler
- BABE: VRF proof'a göre yazar belirler
- Test: Test'te basit bir `AuthorGiven` struct kullanılır

Test:
```rust
// lib.rs:160-173
impl FindAuthor<u64> for AuthorGiven {
    fn find_author<'a, I>(digests: I) -> Option<u64>
    where I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])> {
        for (id, mut data) in digests {
            if id == TEST_ID {
                return u64::decode(&mut data).ok();
            }
        }
        None
    }
}
```

### do_try_state — Invariant Kontrolü

```rust
// lib.rs:107-122
pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
    if let Some(stored_author) = <Author<T>>::get() {
        let digest = <frame_system::Pallet<T>>::digest();
        let pre_runtime_digests = digest.logs.iter().filter_map(|d| d.as_pre_runtime());
        if let Some(expected_author) = T::FindAuthor::find_author(pre_runtime_digests) {
            ensure!(
                stored_author == expected_author,
                "Stored author does not match the author derived from digest"
            );
        }
    }
    Ok(())
}
```

### Uncle Block Durumu

Uncle block mekanizması bu sürümde **mevcut değildir**. Eski Substrate'da (v3 öncesi):
- `UncleBlockLimit`: uncle block sayısı limiti
- `UncleReward`: uncle yazarlarına ödül
- `Uncles`: uncle block listesi storage'ı

Modern Substrate'da (polkadot-sdk) uncle reward konsepti GRANDPA finality ile anlamsız hale geldiğinden kaldırıldı. Şimdiki `pallet-authorship` sadece mevcut block yazarını takip eder.

---

## KISIM 5: pallet-utility

### Genel Bakış

`pallet-utility` stateless bir multi-call dispatcher. Batch işlemler, pseudonim dispatch, fallback logic sağlar. 644 satır `lib.rs` + kapsamlı test suite.

### Call Limit Hesabı

```rust
// lib.rs:138-153
const CALL_ALIGN: u32 = 1024;

fn batched_calls_limit() -> u32 {
    let allocator_limit = sp_core::MAX_POSSIBLE_ALLOCATION;  // ~4GB
    let call_size = (core::mem::size_of::<<T as Config>::RuntimeCall>() as u32)
        .div_ceil(CALL_ALIGN) * CALL_ALIGN;
    let margin_factor = 3;  // Vec doubling capacity için

    allocator_limit / margin_factor / call_size
}
```

**Integrity test:**
```rust
fn integrity_test() {
    // Call enum 1024 byte'ı aşmamalı — büyük parametreler Box<> ile wrap edilmeli
    assert!(core::mem::size_of::<<T as Config>::RuntimeCall>() as u32 <= CALL_ALIGN, ...);
}
```

### batch vs batch_all vs force_batch — TEMEL FARK

#### batch

```rust
// lib.rs:199-239
pub fn batch(origin: OriginFor<T>, calls: Vec<RuntimeCall>) -> DispatchResultWithPostInfo {
    if ensure_none(origin.clone()).is_ok() {
        return Err(BadOrigin.into());  // None origin yasak
    }

    let is_root = ensure_root(origin.clone()).is_ok();
    ensure!(calls_len <= Self::batched_calls_limit() as usize, Error::<T>::TooManyCalls);

    for (index, call) in calls.into_iter().enumerate() {
        let info = call.get_dispatch_info();
        let result = if is_root {
            call.dispatch_bypass_filter(origin.clone())  // Root: filter bypass
        } else {
            call.dispatch(origin.clone())  // Normal: filter uygulanır
        };
        weight = weight.saturating_add(extract_actual_weight(&result, &info));

        if let Err(e) = result {
            Self::deposit_event(Event::BatchInterrupted { index: index as u32, error: e.error });
            // ERKEN ÇIKIŞ: hatada durur, önceki call'lar revert edilmez
            return Ok(Some(base_weight.saturating_add(weight)).into());
        }
        Self::deposit_event(Event::ItemCompleted);
    }
    Self::deposit_event(Event::BatchCompleted);
    Ok(...)
}
```

**batch davranışı:**
- Hata durumunda DURUR (remaining call'lar çalışmaz)
- Önceki başarılı call'lar **REVERT EDİLMEZ**
- `BatchInterrupted { index, error }` eventi emit edilir
- Dış `Ok(...)` döner — extrinsic başarılı sayılır

#### batch_all

```rust
// lib.rs:308-353
pub fn batch_all(origin: OriginFor<T>, calls: Vec<RuntimeCall>) -> DispatchResultWithPostInfo {
    ...
    let is_root = ensure_root(origin.clone()).is_ok();

    for (index, call) in calls.into_iter().enumerate() {
        let info = call.get_dispatch_info();
        let result = if is_root {
            call.dispatch_bypass_filter(origin.clone())
        } else {
            // batch_all nested olmasın diye filter ekle
            let mut filtered_origin = origin.clone();
            filtered_origin.add_filter(
                move |c: &<T as frame_system::Config>::RuntimeCall| {
                    let c = <T as Config>::RuntimeCall::from_ref(c);
                    !matches!(c.is_sub_type(), Some(Call::batch_all { .. }))
                },
            );
            call.dispatch(filtered_origin)
        };

        weight = weight.saturating_add(extract_actual_weight(&result, &info));
        result.map_err(|mut err| {
            // Hata durumunda weight ile birlikte hata döndür — ROLLBACK
            let base_weight = T::WeightInfo::batch_all(index.saturating_add(1) as u32);
            err.post_info = Some(base_weight.saturating_add(weight)).into();
            err
        })?;  // ? operatörü — dış Err döner → tüm storage revert

        Self::deposit_event(Event::ItemCompleted);
    }
    Self::deposit_event(Event::BatchCompleted);
    Ok(...)
}
```

**batch_all davranışı:**
- Hata durumunda `?` operatörü ile hata propagate → tüm extrinsic Err döner
- Storage değişiklikleri REVERT EDİLİR (atomik)
- `batch_all` içinde `batch_all` nested olamaz (filter ile korunuyor)
- Test'te doğrulandı: `batch_all_revert` testi balances değişmiyor

#### force_batch

```rust
// lib.rs:404-445
pub fn force_batch(origin: OriginFor<T>, calls: Vec<RuntimeCall>) -> DispatchResultWithPostInfo {
    ...
    let mut has_error: bool = false;

    for call in calls.into_iter() {
        let info = call.get_dispatch_info();
        let result = if is_root {
            call.dispatch_bypass_filter(origin.clone())
        } else {
            call.dispatch(origin.clone())
        };
        weight = weight.saturating_add(extract_actual_weight(&result, &info));

        if let Err(e) = result {
            has_error = true;
            Self::deposit_event(Event::ItemFailed { error: e.error });
            // DEVAM ET — bir sonraki call'a geç
        } else {
            Self::deposit_event(Event::ItemCompleted);
        }
    }

    if has_error {
        Self::deposit_event(Event::BatchCompletedWithErrors);
    } else {
        Self::deposit_event(Event::BatchCompleted);
    }
    Ok(...)
}
```

**force_batch davranışı:**
- Hata olsa da DEVAM EDER
- Başarılı call'lar revert edilmez, başarısız call'lar da revert edilmez
- `ItemFailed { error }` eventi emit edilir
- Sonuçta `BatchCompletedWithErrors` veya `BatchCompleted`
- Dış extrinsic her zaman `Ok` döner

### Karşılaştırma Tablosu

| Özellik | batch | batch_all | force_batch |
|---------|-------|-----------|-------------|
| Hata durumunda duruyor mu? | Evet | Evet (revert ile) | Hayır |
| Atomik (rollback)? | Hayır | Evet | Hayır |
| Dış sonuç hata olabilir mi? | Hayır (Ok döner) | Evet | Hayır (Ok döner) |
| Event (hata) | BatchInterrupted | (hata propagate) | BatchCompletedWithErrors |
| batch_all nesting? | İzin verilir | Yasak | İzin verilir |
| None origin? | Yasak | Yasak | Yasak |

### dispatch_as

```rust
// lib.rs:370-383
pub fn dispatch_as(
    origin: OriginFor<T>,
    as_origin: Box<T::PalletsOrigin>,
    call: Box<<T as Config>::RuntimeCall>,
) -> DispatchResult {
    ensure_root(origin)?;  // SADECE ROOT

    let res = call.dispatch_bypass_filter((*as_origin).into());

    Self::deposit_event(Event::DispatchedAs {
        result: res.map(|_| ()).map_err(|e| e.error),
    });
    Ok(())  // İç call hatası olsa da dış Ok döner
}
```

`dispatch_as` vs `dispatch_as_fallible`:
- `dispatch_as`: iç call hatası dışarı propagate olmaz, event'e yazılır, Ok döner
- `dispatch_as_fallible`: iç call hatası propagate olur, Err döner

### dispatch_as_fallible

```rust
// lib.rs:573-585
pub fn dispatch_as_fallible(
    origin: OriginFor<T>,
    as_origin: Box<T::PalletsOrigin>,
    call: Box<<T as Config>::RuntimeCall>,
) -> DispatchResult {
    ensure_root(origin)?;

    call.dispatch_bypass_filter((*as_origin).into()).map_err(|e| e.error)?;
    // ? ile error propagate — iç hata dışarı çıkar

    Self::deposit_event(Event::DispatchedAs { result: Ok(()) });
    Ok(())
}
```

### with_weight

```rust
// lib.rs:454-465
#[pallet::weight((*weight, call.get_dispatch_info().class))]
pub fn with_weight(origin: OriginFor<T>, call: Box<RuntimeCall>, weight: Weight) -> DispatchResult {
    ensure_root(origin)?;
    let _ = weight; // Weight witness kontrolü yok

    let res = call.dispatch_bypass_filter(frame_system::RawOrigin::Root.into());
    res.map(|_| ()).map_err(|e| e.error)
}
```

`sudo_unchecked_weight` gibi — Root weight'i manüel belirler. `set_code_without_checks` gibi ağır call'ları yönetilebilir weight ile dispatch etmek için.

### if_else

```rust
// lib.rs:501-557
pub fn if_else(
    origin: OriginFor<T>,
    main: Box<RuntimeCall>,
    fallback: Box<RuntimeCall>,
) -> DispatchResultWithPostInfo {
    if ensure_none(origin.clone()).is_ok() { return Err(BadOrigin.into()); }

    let is_root = ensure_root(origin.clone()).is_ok();
    let mut weight = T::WeightInfo::if_else();

    let main_result = if is_root {
        main.dispatch_bypass_filter(origin.clone())
    } else {
        main.dispatch(origin.clone())
    };

    weight = weight.saturating_add(extract_actual_weight(&main_result, &main_info));

    let Err(main_error) = main_result else {
        // Main başarılı → fallback çalışmaz
        Self::deposit_event(Event::IfElseMainSuccess);
        return Ok(Some(weight).into());
    };

    // Main başarısız → fallback dene
    let fallback_result = if is_root {
        fallback.dispatch_bypass_filter(origin.clone())
    } else {
        fallback.dispatch(origin)
    };

    weight = weight.saturating_add(extract_actual_weight(&fallback_result, &fallback_info));

    let Err(fallback_error) = fallback_result else {
        // Fallback başarılı → main_error event'e yaz
        Self::deposit_event(Event::IfElseFallbackCalled { main_error: main_error.error });
        return Ok(Some(weight).into());
    };

    // Her ikisi de başarısız → fallback_error ile Err
    Err(sp_runtime::DispatchErrorWithPostInfo {
        error: fallback_error.error,
        post_info: Some(weight).into(),
    })
}
```

**if_else mantığı:**
- Main başarılı → `IfElseMainSuccess`, fallback çalışmaz
- Main başarısız, fallback başarılı → `IfElseFallbackCalled { main_error }`, Ok döner
- Her ikisi başarısız → fallback hatası ile Err döner

**if_else weights:**
- Hem main hem fallback weight'i önceden declare edilir (worst case)
- Gerçekte hangi path çalıştıysa o weight kullanılır

### as_derivative — Pseudonim Dispatch

```rust
// lib.rs:265-287
pub fn as_derivative(
    origin: OriginFor<T>,
    index: u16,
    call: Box<RuntimeCall>,
) -> DispatchResultWithPostInfo {
    let mut origin = origin;
    let who = ensure_signed(origin.clone())?;

    // Deterministik pseudonim hesapla
    let pseudonym = derivative_account_id(who, index);
    origin.set_caller_from(frame_system::RawOrigin::Signed(pseudonym));

    let info = call.get_dispatch_info();
    let result = call.dispatch(origin);  // Filter uygulanır (bypass yok)
    ...
}
```

### derivative_account_id — Pseudonim Formülü

```rust
// lib.rs:640-644
pub fn derivative_account_id<AccountId: Encode + Decode>(who: AccountId, index: u16) -> AccountId {
    // b2b256("modlpy/utilisuba" ++ who ++ index)
    let entropy = (b"modlpy/utilisuba", who, index).using_encoded(blake2_256);
    Decode::decode(&mut TrailingZeroInput::new(entropy.as_ref()))
        .expect("infinite length input; no invalid inputs for type; qed")
}
```

**Formül:** `blake2_256("modlpy/utilisuba" || who_encoded || index_u16_encoded)`, result TrailingZeroInput ile AccountId'ye decode edilir.

Her `who` için 2^16 (65536) farklı pseudonim. Deterministik — offline hesaplanabilir.

**Uyarı:** `u64` AccountId (test) ile 32 byte hash'den truncation olabilir — collision riski!

### weight_and_dispatch_class

```rust
// lib.rs:589-611
fn weight_and_dispatch_class(calls: &[RuntimeCall]) -> (Weight, DispatchClass) {
    if calls.is_empty() {
        return (Weight::zero(), DispatchClass::Normal);
    }

    let dispatch_infos = calls.iter().map(|call| call.get_dispatch_info());
    let (dispatch_weight, dispatch_class) = dispatch_infos.fold(
        (Weight::zero(), DispatchClass::Operational),
        |(total_weight, dispatch_class), di| {
            (
                total_weight.saturating_add(di.call_weight),
                // Bir tane Normal varsa tüm batch Normal sayılır
                if di.class == DispatchClass::Normal { di.class } else { dispatch_class },
            )
        },
    );

    (dispatch_weight, dispatch_class)
}
```

**Class belirleme mantığı:** Tüm call'lar Operational ise Operational, bir tanesi bile Normal ise Normal. Başlangıç değeri Operational — boş call için Normal (başlangıç farklı ama empty case ayrı handle ediliyor).

---

## Nova Shield Uygulama Notları

### 1. Executive Implementasyonu

Nova Shield kendi L1 zinciri için FRAME kullanmak yerine custom executive yazabilir veya FRAME'i fork edebilir. Kritik tasarım kararları:

**initialize_block sırası:**
```
1. reset_events
2. runtime_upgrade check → execute_on_runtime_upgrade
3. System::initialize(block_number, parent_hash, digest)
4. base_block_weight kaydet
5. on_initialize_with_weight_registration (her pallet ayrı)
6. note_finished_initialize
7. PreInherents hook
```

Bu sıra ZORUNLU — değiştirme.

**Inherent sıralama kuralı:** İnherent'ler ÖNCE gelir. Timestamp, authorship gibi sistem inherent'leri block başında olmalı. Nova Shield MEV koruması için bu sıra kritik — inherent'lerden sonra işlem sıralaması yapılacak.

**MAX_EXTRINSIC_DEPTH = 8:** Nested call depth'i bu ile sınırlı. Nova Shield utility call'ları için aynı limit uygulanabilir.

### 2. Sudo Kaldırma Planı

Production'da sudo kaldırılmalı. Geçiş planı:
1. Genesis: Sudo ile başla (geliştirme döneminde)
2. Testnet: `CheckOnlySudoAccount` extension ile sadece sudo tx
3. Mainnet yaklaşırken: `remove_key()` çağır
4. Mainnet: Governance pallet (democracy, council) ile yönet

**Nova Shield yetki modeli:** Sudo yerine multi-sig veya on-chain governance. `ensure_signed_or_root` pattern'i kopyalanabilir — hem signed (key ile) hem root (governance) origin desteklenebilir.

### 3. Timestamp Implementasyonu

**Nova Shield için MinimumPeriod seçimi:**
- Block time 3000ms ise MinimumPeriod = 1500ms (Aura için: slot = 2 * MinimumPeriod = 3000ms)
- Block time 6000ms ise MinimumPeriod = 3000ms

**MAX_TIMESTAMP_DRIFT_MILLIS = 30s** — Bu tolerans network latency + validator senkronizasyonu için. Nova Shield için değiştirilebilir ama 30s makul.

**OnTimestampSet hook:** Aura/BABE gibi Nova Shield'in kendi consensus motoru buradan notify edilecek.

**DidUpdate flag:** Her blok sonu panic ile zorunlu kılınıyor. Nova Shield inherent sisteminde aynı pattern:

```rust
fn on_finalize(_n: BlockNumberFor<T>) {
    assert!(DidUpdate::<T>::take(), "Timestamp must be updated once in the block");
}
```

### 4. Authorship Takibi

**Nova Shield için author tracking:**
- Consensus engine (libp2p gossipsub üzerinden blok propagation) pre_runtime digest'e author koyar
- `FindAuthor` implementasyonu consensus engine'e göre yazılır
- `EventHandler` → reward distribution, staking updates

Uncle block gereksiz (GRANDPA finality ile deterministic). Kaldırılmış doğru karar.

```rust
// Nova Shield EventHandler örneği:
impl EventHandler<AccountId, BlockNumber> for NovaRewards {
    fn note_author(author: AccountId) {
        // Block reward dağıt
        Rewards::distribute_block_reward(author);
    }
}
```

### 5. Utility Batch Stratejisi

Nova Shield DEX için batch kullanımı:

**DEX trade + fee payment aynı tx'te:**
```rust
batch_all([
    swap_call(token_a, token_b, amount),
    pay_protocol_fee(),
])
// Eğer swap başarısız olursa fee de ödenmez → atomik
```

**Airdrop / toplu transfer:**
```rust
force_batch([
    transfer(user1, amount1),
    transfer(user2, amount2),
    // Başarısız olanlar atlanır, diğerleri devam eder
])
```

**MEV koruması için if_else:**
```rust
if_else(
    protected_swap(min_output),  // Slippage korumalı
    emergency_cancel(),           // Başarısız olursa iptal
)
```

**as_derivative Nova Shield kullanımı:**
- Tek hesaptan multiple DEX position yönetimi
- Her position farklı pseudonim altında — izolasyon sağlanır
- Formül: `blake2_256("modlpy/utilisuba" || owner || index)`

### 6. Weight Accounting

Executive'in `OnInitializeWithWeightRegistration` pattern'i kritik:

```rust
// Her pallet on_initialize weight'i ANINDA kayıt altına alınır
// Bu sayede block_weight her an doğru değeri içerir
register_extra_weight_unchecked(individual_weight, DispatchClass::Mandatory);
```

Nova Shield custom scheduler için bu önemli — on_initialize ağır pallet'ler block'a az transaction sığdırır.

### 7. Runtime Upgrade Güvenliği

```rust
// Runtime upgrade sırası (kritik):
// 1. BeforeAllRuntimeMigrations
// 2. COnRuntimeUpgrade (deprecated)
// 3. SingleBlockMigrations
// 4. AllPalletsWithSystem::on_runtime_upgrade
```

Nova Shield için yeni pallet eklerken veya storage layout değiştirilirken `SingleBlockMigrations` kullanılmalı. MBM (Multi-Block Migration) büyük migration'lar için.

### 8. MBM ile İşlem Kabul Etmeme

```rust
fn extrinsic_mode() -> ExtrinsicInclusionMode {
    if MultiBlockMigrator::ongoing() {
        ExtrinsicInclusionMode::OnlyInherents  // User tx yasak!
    } else {
        ExtrinsicInclusionMode::AllExtrinsics
    }
}
```

Nova Shield storage migration sürecinde DEX işlemleri otomatik durdurulur. Bu kritik güvenlik özelliği — kullanıcı deneyimi açısından önceden duyurulmalı.

---

## Kaynak Kod Referansları

### frame-executive/src/lib.rs

| Satır | İçerik |
|-------|--------|
| 224-240 | Executive struct ve generics |
| 596-605 | `initialize_block` — ExtrinsicInclusionMode döndürür |
| 607-613 | `extrinsic_mode()` — MBM kontrolü |
| 625-666 | `initialize_block_impl` — tam adımlar |
| 669-674 | `runtime_upgraded()` — version karşılaştırma |
| 690-723 | `execute_block` — ana blok işleme |
| 728-742 | `inherents_applied` — MBM/on_poll geçişi |
| 744-779 | `apply_extrinsics` — inherent sıralama kuralı |
| 784-800 | `finalize_block` — block builder için |
| 804-823 | `on_idle_hook` — arta kalan weight |
| 825-846 | `on_poll_hook` — inherent sonrası hook |
| 857-908 | `do_apply_extrinsic` — tek extrinsic işleme |
| 893 | AUDIT notu: panic yasak |
| 914-917 | `apply_extrinsic` — public API |
| 919-945 | `final_checks` — state/extrinsics root |
| 952-994 | `validate_transaction` — pool doğrulama |

### frame/sudo/src/lib.rs

| Satır | İçerik |
|-------|--------|
| 202-213 | `sudo` — Root origin dispatch |
| 221-235 | `sudo_unchecked_weight` — weight bypass |
| 240-253 | `set_key` — sudo key değiştir |
| 259-282 | `sudo_as` — Signed origin dispatch |
| 284-297 | `remove_key` — sudo kaldır (geri alınamaz) |
| 332-333 | `Key<T>` storage |
| 351-364 | `ensure_sudo` — yetki kontrolü |

### frame/sudo/src/extension.rs

| Satır | İçerik |
|-------|--------|
| 41-43 | `CheckOnlySudoAccount` struct |
| 90-119 | `validate` — sadece sudo tx kabul et |
| 107-109 | who == sudo_key kontrolü |
| 113-115 | `provides: (who, call_hash)` — replay koruması |

### frame/timestamp/src/lib.rs

| Satır | İçerik |
|-------|--------|
| 186-193 | MinimumPeriod != expected block time |
| 203-211 | `Now<T>` ve `DidUpdate<T>` storage |
| 217-230 | Hooks: on_initialize (weight reserve), on_finalize (assert) |
| 254-273 | `set` — inherent call, 3 assert |
| 285-331 | `ProvideInherent` — create/check/is_inherent |
| 297 | `next_time = max(data, prev + MinimumPeriod)` |
| 305-306 | `MAX_TIMESTAMP_DRIFT_MILLIS = 30_000ms` |
| 352-379 | `Time` ve `UnixTime` implementasyonları |

### frame/authorship/src/lib.rs

| Satır | İçerik |
|-------|--------|
| 28-34 | `EventHandler` trait |
| 43-48 | Config: FindAuthor, EventHandler |
| 74-77 | `Author<T>` storage + whitelist_storage |
| 53-72 | Hooks: on_initialize (note_author), on_finalize (kill) |
| 85-96 | `author()` — lazy loading + memoize |
| 107-122 | `do_try_state` — invariant kontrolü |

### frame/utility/src/lib.rs

| Satır | İçerik |
|-------|--------|
| 138-153 | `CALL_ALIGN = 1024`, `batched_calls_limit()` |
| 199-239 | `batch` — hata durumunda dur, revert yok |
| 308-353 | `batch_all` — atomik, nested yasak |
| 370-383 | `dispatch_as` — Root only, iç hata event'e |
| 404-445 | `force_batch` — devam et, hata takip et |
| 454-465 | `with_weight` — Root, weight override |
| 501-557 | `if_else` — main/fallback logic |
| 573-585 | `dispatch_as_fallible` — Root, hata propagate |
| 589-611 | `weight_and_dispatch_class` — class belirleme |
| 640-644 | `derivative_account_id` — pseudonim formülü |

---

## Özet: Kritik Tasarım Kararları

1. **Inherent sıralaması katı:** Blok başında, araya transaction giremez. Nova Shield'de timestamp + author inherent'leri her blokta zorunlu.

2. **AUDIT notu (do_apply_extrinsic:893):** İmza doğrulamasından sonra panic olmaz. Nova Shield extrinsic handler'ında aynı garanti gerekli.

3. **MBM sırasında OnlyInherents:** Runtime upgrade / storage migration süresince user transaction'lar otomatik bloklanır. DEX operasyonları bu dönemde durur.

4. **batch vs batch_all:** batch = "try all, stop on error"; batch_all = "atomic, rollback on error". DEX swap atomicity için batch_all, toplu airdrop için force_batch.

5. **Sudo production'da kaldırılmalı:** `remove_key()` ile Key storage silinir. Geri alınamaz. Nova Shield mainnet öncesi bu adım zorunlu.

6. **Timestamp MinimumPeriod != block time:** Aura'da block time = 2 * MinimumPeriod. Nova Shield için doğru değer seçilmeli. 30 saniye drift toleransı makul.

7. **Uncle block yok:** Modern Substrate + GRANDPA finality ile uncle reward anlamsız. Nova Shield uncle mekanizması eklememeli.

8. **weight_and_dispatch_class:** Bir Normal call bile Operational batch'i Normal yapar. Bu önemli — Operational priority avantajı kaybedilir.

9. **CheckOnlySudoAccount extension:** Erken aşama chainlerde spam koruması için kullanılabilir. Nova Shield devnet/testnet'te bu gerekli olabilir.

10. **derivative_account_id:** `blake2_256("modlpy/utilisuba" || who || index)` — deterministik, offline hesaplanabilir. Nova Shield multi-position DEX için kullanılabilir.
