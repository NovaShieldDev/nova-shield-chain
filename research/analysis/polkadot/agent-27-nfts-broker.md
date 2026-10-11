# Agent-27: pallet-nfts + pallet-uniques + pallet-broker Analizi

## Kapsanan Dosyalar

### pallet-nfts
- `substrate/frame/nfts/src/lib.rs` — Config, Storage, Events, Errors, Extrinsics (1100+ satir)
- `substrate/frame/nfts/src/types.rs` — Tum type tanimlari (623 satir)
- `substrate/frame/nfts/src/features/create_delete_collection.rs` — Collection olusturma/silme
- `substrate/frame/nfts/src/features/create_delete_item.rs` — Item mint/burn
- `substrate/frame/nfts/src/features/transfer.rs` — Transfer ve ownership degisimi
- `substrate/frame/nfts/src/features/approvals.rs` — Transfer approval mekanizmasi
- `substrate/frame/nfts/src/features/attributes.rs` — Attribute set/clear/pre-signed
- `substrate/frame/nfts/src/features/metadata.rs` — Metadata set/clear
- `substrate/frame/nfts/src/features/atomic_swap.rs` — Atomic swap
- `substrate/frame/nfts/src/features/buy_sell.rs` — Price set / buy
- `substrate/frame/nfts/src/features/lock.rs` — Lock mekanizmalari
- `substrate/frame/nfts/src/features/roles.rs` — Rol yonetimi
- `substrate/frame/nfts/src/features/settings.rs` — Collection/item ayarlari
- `substrate/frame/nfts/src/common_functions.rs` — Yardimci fonksiyonlar
- `substrate/frame/nfts/runtime-api/src/lib.rs` — Runtime API

### pallet-uniques
- `substrate/frame/uniques/src/lib.rs` — Eski NFT pallet (200+ satir incelendi)
- `substrate/frame/uniques/src/types.rs` — Eski type tanimlari

### pallet-broker
- `substrate/frame/broker/src/lib.rs` — Config, Storage, Events, Errors, Extrinsics
- `substrate/frame/broker/src/types.rs` — Tum type tanimlari
- `substrate/frame/broker/src/dispatchable_impls.rs` — Tum extrinsic implementasyonlari
- `substrate/frame/broker/src/tick_impls.rs` — On-chain tick mantigi
- `substrate/frame/broker/src/adapt_price.rs` — Fiyat adaptasyon algoritmalari
- `substrate/frame/broker/src/utility_impls.rs` — Yardimci fonksiyonlar

---

## 1. pallet-nfts: Mimari

### Genel Yapi
pallet-nfts, Polkadot SDK'nin modern NFT pallet'idir. StorageVersion::new(1). `I: 'static = ()` generic parametresiyle coklu instance destekler.

### Config Trait (lib.rs:127-249)
```rust
type CollectionId: Member + Parameter + MaxEncodedLen + Copy + Incrementable;
type ItemId: Member + Parameter + MaxEncodedLen + Copy;
type Currency: ReservableCurrency<Self::AccountId>;
type ForceOrigin: EnsureOrigin<Self::RuntimeOrigin>;
type CreateOrigin: EnsureOriginWithArg<Self::RuntimeOrigin, Self::CollectionId, Success = Self::AccountId>;
type Locker: Locker<Self::CollectionId, Self::ItemId>;
```

Sabit degerler:
- `CollectionDeposit`: collection olusturmak icin gereken rezerv
- `ItemDeposit`: item mint icin gereken rezerv
- `MetadataDepositBase`: metadata deposit baz degeri
- `AttributeDepositBase`: attribute deposit baz degeri
- `DepositPerByte`: her byte icin ek deposit
- `StringLimit`: on-chain veri maksimum uzunlugu
- `KeyLimit`: attribute anahtari maksimum uzunlugu
- `ValueLimit`: attribute degeri maksimum uzunlugu
- `ApprovalsLimit`: item basi maksimum approval sayisi
- `ItemAttributesApprovalsLimit`: item attributes approval maksimumu
- `MaxTips`: tip gonderme maksimumu
- `MaxDeadlineDuration`: maksimum deadline suresi (block cinsinden)
- `MaxAttributesPerCall`: call basi maksimum attribute sayisi

Off-chain imza destegi:
- `type OffchainSignature: Verify<Signer = Self::OffchainPublic> + Parameter`
- `type OffchainPublic: IdentifyAccount<AccountId = Self::AccountId>`
- Hem raw hem `<Bytes>...</Bytes>` wrapped formati desteklenir (common_functions.rs:50-62)

### PalletFeatures Sistemi (types.rs:530-556)
Pallet-level feature flag'leri — `u64` bitflag olarak saklanir, `Config::Features` ile konfigure edilir:
```rust
pub enum PalletFeature {
    Trading,    // buy/sell islemleri
    Attributes, // attribute set/clear
    Approvals,  // transfer approval
    Swaps,      // atomic swap
}
```
Her feature ON_INITIALIZE'de check edilmez; extrinsic cagrildiginda `is_pallet_feature_enabled()` kontrolu yapilir (settings.rs:170-173).

### Storage Items (lib.rs:252-415)
| Storage | Tip | Anahtar | Deger |
|---------|-----|---------|-------|
| `Collection` | StorageMap | Blake2_128Concat(CollectionId) | CollectionDetails |
| `OwnershipAcceptance` | StorageMap | Blake2_128Concat(AccountId) | CollectionId |
| `Account` | StorageNMap | (AccountId, CollectionId, ItemId) | () |
| `CollectionAccount` | StorageDoubleMap | (AccountId, CollectionId) | () |
| `CollectionRoleOf` | StorageDoubleMap | (CollectionId, AccountId) | CollectionRoles |
| `Item` | StorageDoubleMap | (CollectionId, ItemId) | ItemDetails |
| `CollectionMetadataOf` | StorageMap | Blake2_128Concat(CollectionId) | CollectionMetadata |
| `ItemMetadataOf` | StorageDoubleMap | (CollectionId, ItemId) | ItemMetadata |
| `Attribute` | StorageNMap | (CollectionId, Option<ItemId>, Namespace, Key) | (Value, AttributeDeposit) |
| `ItemPriceOf` | StorageDoubleMap | (CollectionId, ItemId) | (Price, Option<AccountId>) |
| `ItemAttributesApprovalsOf` | StorageDoubleMap | (CollectionId, ItemId) | BoundedBTreeSet |
| `NextCollectionId` | StorageValue | - | Option<CollectionId> |
| `PendingSwapOf` | StorageDoubleMap | (CollectionId, ItemId) | PendingSwap |
| `CollectionConfigOf` | StorageMap | Blake2_128Concat(CollectionId) | CollectionConfig |
| `ItemConfigOf` | StorageDoubleMap | (CollectionId, ItemId) | ItemConfig |

---

## 2. pallet-nfts: Collection ve Item Yonetimi

### CollectionSettings — Bit Flags (types.rs:273-307)
```rust
pub enum CollectionSetting {
    TransferableItems,    // item'lar transfer edilebilir
    UnlockedMetadata,     // metadata degistirilebilir
    UnlockedAttributes,   // attribute'lar degistirilebilir
    UnlockedMaxSupply,    // max supply degistirilebilir
    DepositRequired,      // item'lar icin deposit gerekli
}
```
**KRITIK:** `CollectionSettings` inverted flag sistemidir. Flag bit SET oldugunda o ozellik DISABLE'dir. `is_disabled(setting)` = bit VARSA disabled. `all_enabled()` = `BitFlags::EMPTY` (hic bit set degil). Bu sezgisel olmayan bir tasarim; yanlislikla tersine anlamak kolaydir.

- `create()` extrinsic'inde `DepositRequired`'i disable etmek YASAK (sadece `force_create` ile yapilabilir) — lib.rs:724-728
- Bir ayar bir kez disable edilince geri ENABLE EDILEMEZ — lock.rs:31 comment

### ItemSettings — Bit Flags (types.rs:454-488)
```rust
pub enum ItemSetting {
    Transferable,       // item transfer edilebilir
    UnlockedMetadata,   // item metadata'si degistirilebilir
    UnlockedAttributes, // item attribute'lari degistirilebilir
}
```
Ayni inverted mantik gecerlidir.

### CollectionConfig (types.rs:417-451)
```rust
pub struct CollectionConfig<Price, BlockNumber, CollectionId> {
    pub settings: CollectionSettings,
    pub max_supply: Option<u32>,
    pub mint_settings: MintSettings<Price, BlockNumber, CollectionId>,
}
```

### MintSettings (types.rs:337-373)
```rust
pub enum MintType<CollectionId> {
    Issuer,               // sadece Issuer rol sahibi mint edebilir
    Public,               // herkes mint edebilir
    HolderOf(CollectionId), // o collection'dan item sahibi olanlar
}

pub struct MintSettings<Price, BlockNumber, CollectionId> {
    pub mint_type: MintType<CollectionId>,
    pub price: Option<Price>,       // mint basina odeme
    pub start_block: Option<BlockNumber>,
    pub end_block: Option<BlockNumber>,
    pub default_item_settings: ItemSettings, // yeni item'larin varsayilan ayarlari
}
```

### Collection Olusturma (create_delete_collection.rs:36-77)
`do_create_collection()` akisi:
1. `CollectionIdInUse` kontrolu
2. `T::Currency::reserve(&owner, deposit)` — deposit rezerv
3. `Collection::insert()` — {owner, owner_deposit, items:0, item_metadatas:0, item_configs:0, attributes:0}
4. `CollectionRoleOf::insert()` — admin hesabina Admin+Freezer+Issuer rolleri
5. `CollectionConfigOf::insert()` — config kaydet
6. `CollectionAccount::insert()` — owner indexi
7. Event: `Created` veya `ForceCreated`
8. Eger max_supply tanimlanmissa `CollectionMaxSupplySet` ek eventi

`create` vs `force_create`:
- `create`: `CreateOrigin` check, `CollectionDeposit` reserve, `DepositRequired` disable edilemez
- `force_create`: `ForceOrigin` check, deposit = zero, herhangi config kabul edilir

### Collection Silme (create_delete_collection.rs:100-152)
`do_destroy_collection()`:
1. Collection bos olmali (items == 0)
2. Witness data dogru olmali (attributes, item_metadatas, item_configs eslesmeli)
3. `ItemMetadataOf::drain_prefix` — metadata deposit'leri iade et
4. `Attribute::drain_prefix` — attribute deposit'leri iade et
5. Roller temizle (`clear_roles`)
6. `owner_deposit` iade et
7. `ItemConfigOf::clear_prefix`

### MaxSupply Enforcement (create_delete_item.rs:66-68)
```rust
if let Some(max_supply) = collection_config.max_supply {
    ensure!(collection_details.items < max_supply, Error::MaxSupplyReached);
}
```
Max supply sadece mint sirasinda kontrol edilir. `set_collection_max_supply()` ise mevcut item sayisinin altina dusuremez (settings.rs:74).

### CollectionRole Sistemi (types.rs:558-590)
```rust
pub enum CollectionRole {
    Issuer,  // item mint edebilir
    Freezer, // item transfer'ini lock/unlock edebilir
    Admin,   // thaw, force transfer, burn, metadata/attribute yonetimi
}
```
CollectionRoles `u8` bitflag'dir. Bir hesap birden fazla role sahip olabilir. Rolleri atayan: `do_set_team()`. Sadece collection owner rolleri degistirebilir (root hari hic rol yoksa onceden o rol olmali — roles.rs:58-68).

---

## 3. pallet-nfts: Transfer ve Approval

### Transfer Akisi (transfer.rs:46-113)
`do_transfer()` kontroller (siralamayla):
1. `T::Locker::is_locked(collection, item)` — external locker kontrolu
2. `PalletAttributes::TransferDisabled` system attribute kontrolu
3. `CollectionSetting::TransferableItems` kontrolu (collection seviyesi)
4. `ItemSetting::Transferable` kontrolu (item seviyesi)
5. Closure ile ek kontrol (caller'in owner veya delegate olmasi)
6. `Account` storage'ini guncelle (eski owner kaldir, yeni owner ekle)
7. `details.approvals.clear()` — TUM approval'lar silindi (pre-approve attack onleme)
8. `ItemPriceOf::remove` — satis fiyati sifirla
9. `PendingSwapOf::remove` — bekleyen swap'i sifirla

`transfer()` extrinsic (lib.rs:1022-1042):
- Owner ya da delegate transfer edebilir
- Delegate ise deadline kontrolu: `block_number <= deadline` (gecmis deadline = hata)

### Collection Ownership Transfer (transfer.rs:124-161)
`do_transfer_ownership()`:
1. Yeni owner onceden `set_accept_ownership(Some(collection))` cagirilmis olmali
2. `OwnershipAcceptance` storage'ini kontrol et
3. Deposit `repatriate_reserved` ile yeni owner'a tasini
4. `dec_consumers` — yeni owner'in consumer count azalt

### Approval Mekanizmasi (approvals.rs)

`do_approve_transfer()`:
- `PalletFeature::Approvals` aktif olmali
- Collection `TransferableItems` olmali
- Deadline = `current_block + maybe_deadline` (mutlak deger olarak saklanir)
- `details.approvals.try_insert(delegate, deadline)` — map'e ekle (limit: ApprovalsLimit)

`do_cancel_approval()`:
- Deadline gecmisse herkes iptal edebilir (owner olmasina gerek yok)
- Deadline gecmemisse sadece owner iptal edebilir

`do_clear_all_transfer_approvals()`:
- Sadece owner tum approval'lari temizleyebilir

---

## 4. pallet-nfts: Metadata ve Attributes

### Metadata Deposit Formulu (metadata.rs:78-83)
```
deposit = DepositPerByte * data.len() + MetadataDepositBase
```
Eger `CollectionSetting::DepositRequired` disabled VEYA root call ise deposit = 0.

### Item Metadata (metadata.rs:42-111)
- Sadece Admin rol `do_set_item_metadata()` cagirabilir (ya da root)
- `ItemSetting::UnlockedMetadata` disabled ise degistirilemez (root haric)
- Depositor: `maybe_depositor` varsa o, yoksa collection owner
- `ItemMetadataDeposit.account = None` -> depositor collection owner demek
- Burn sirasinda metadata locked degilse otomatik silinir (create_delete_item.rs:240-254)

### Collection Metadata (metadata.rs:179-225)
- Admin rol veya root
- `CollectionSetting::UnlockedMetadata` disabled ise root haric degistirilemez
- Deposit: `owner_deposit`'ten hesaplanir

### Attribute Sistemi (attributes.rs)

#### AttributeNamespace (types.rs:387-396)
```rust
pub enum AttributeNamespace<AccountId> {
    Pallet,           // sistem tarafindan set edilen, kimse manuel set edemez
    CollectionOwner,  // admin rolunun set ettigi
    ItemOwner,        // item sahibinin set ettigi
    Account(AccountId), // onaylanmis bir hesabin set ettigi
}
```

#### Attribute Deposit Formulu (attributes.rs:100-108)
```
deposit = DepositPerByte * (key.len() + value.len()) + AttributeDepositBase
```
`CollectionOwner` namespace ve `DepositRequired` disabled ise deposit = 0.

#### Namespace Yetki Kontrolleri (attributes.rs:464-483)
- `CollectionOwner` namespace: `CollectionRole::Admin` olmali
- `ItemOwner` namespace: item sahibi olmali
- `Account(account_id)` namespace: `account_id == origin` ve `ItemAttributesApprovalsOf`'ta kayitli olmali

#### do_set_attributes_pre_signed (attributes.rs:216-267)
- `PreSignedAttributes` struct: collection, item, attributes, namespace, deadline
- Sadece `CollectionOwner` veya `Account()` namespace desteklenir
- Item sahibi caller olmali, signer da `Account()` icindeki hesap olmali
- Deadline block kontrolu: `deadline >= now`
- MaxAttributesPerCall limiti kontrol edilir

#### PalletAttributes (types.rs:406-414)
```rust
pub enum PalletAttributes<CollectionId> {
    UsedToClaim(CollectionId), // HolderOf claim'de kullanilmis item'i isaretler
    TransferDisabled,           // sistem tarafindan transfer'i bloke
}
```

### Attribute Approvallari
`do_approve_item_attributes()` — item sahibi bir hesaba `Account()` namespace yazma izni verir (ItemAttributesApprovalsOf'a ekler).

`do_cancel_item_attributes_approval()`:
- `witness.account_attributes` kadar attribute'u drain eder
- Tum deposit'leri delegate'e iade eder
- Silinen attribute sayisi witness'i gececek olursa `BadWitness` hatasi

---

## 5. pallet-nfts: Atomic Swap

### Swap Olusturma (atomic_swap.rs:49-103)
`do_create_swap()`:
- `PalletFeature::Swaps` aktif olmali
- `duration <= MaxDeadlineDuration`
- Caller offered item'in sahibi olmali
- `maybe_desired_item`: belirli bir item ya da o collection'dan herhangi biri
- `PendingSwapOf::insert` — deadline = `now + duration`

### Swap Claim (atomic_swap.rs:160-233)
`do_claim_swap()`:
- Caller send item'in sahibi olmali
- `swap.desired_collection == send_collection_id`
- `swap.price == witness_price` (tam eslesme gerekli)
- `desired_item` belirtilmisse o item olmali
- Deadline gecmemis olmali
- Fiyat transferi:
  - `PriceDirection::Send`: receive_item_owner caller'a oder
  - `PriceDirection::Receive`: caller receive_item_owner'a oder
- `do_transfer(send) + do_transfer(receive)` — iki item simetrik transfer edilir (swap'i kaldirir)

### Swap Iptal (atomic_swap.rs:114-140)
- Deadline gecmisse herkes iptal edebilir
- Gecmemisse sadece offered item sahibi

---

## 6. pallet-nfts: Diger Extrinsicler

### Mint (lib.rs:844-938)
`mint()` extrinsic — MintSettings kontrolu yapar:
- `start_block` kontrol: `start_block <= now` (MintNotStarted)
- `end_block` kontrol: `end_block >= now` (MintEnded)
- `MintType::Issuer`: caller'in Issuer rol olmasi gerekir
- `MintType::Public`: herhangi biri mint edebilir
- `MintType::HolderOf(collection_id)`:
  - `witness_data.owned_item` saglanmali
  - Caller o item'in sahibi olmali
  - `PalletAttributes::UsedToClaim(collection)` attribute'u set edilmemis olmali
  - Set edilirse item "kullanilmis" olarak isaretlenir (her item sadece bir kez claim)
- Mint fiyati varsa: `caller -> collection_details.owner` transfer

`force_mint()` — ForceOrigin veya Issuer rol, herhangi item_config ile

### Burn (lib.rs:989-1004)
- ForceOrigin veya item sahibi burn edebilir
- Locker kontrolu: `T::Locker::is_locked` ve `PalletAttributes::TransferDisabled`
- Deposit iade edilir
- Metadata locked degil ise metadata da silinir
- ItemConfig sadece "disabled settings yoksa" kaldirilir (kilitli item config tutulur)

### mint_pre_signed (create_delete_item.rs:124-199)
`do_mint_pre_signed()`:
- `PreSignedMint` struct: collection, item, attributes, metadata, deadline, only_account, mint_price
- `signer` Issuer rol olmali
- `deadline >= now`
- `only_account` belirtilmisse sadece o hesap kullanabilir
- `mint_price`: caller -> collection owner
- Admin hesabina attribute'lar set ettirilir (CollectionOwner namespace)
- Admin hesabina metadata set ettirilir (depositor = caller)

### redeposit (lib.rs:1063-1099)
- Collection owner, item deposit'lerini guncel degere yeniden hesapla
- En iyi caba (best-effort): yetersiz bakiyede item atlanir

### set_collection_max_supply (settings.rs:57-82)
- `CollectionSetting::UnlockedMaxSupply` aktif olmali (locked = degistirilemez)
- `max_supply >= current items`

### Pay Tips (buy_sell.rs:40-56)
- `tips`: BoundedVec<ItemTip> — max: MaxTips
- Dogrulama YOK (valid collection/item olmadan da tip gonderilebilir)
- `sender -> receiver` direct transfer

### Set Price / Buy (buy_sell.rs)
`do_set_price()`:
- Owner `price` ve `whitelisted_buyer` set eder
- `TransferableItems` ve `Transferable` aktif olmali
- `price = None` -> satis fiyatini sifirla

`do_buy_item()`:
- `bid_price >= price_info.0` (tam fiyat veya fazlasi)
- `whitelisted_buyer` varsa sadece o alabilir
- Odeme: `buyer -> old_owner` (tam set price miktari, bid fazlasi IADE EDILMEZ)
- `do_transfer` cagirilir (approvals temizlenir, fiyat sifirlanir)

### Lock Mekanizmalari (lock.rs)
- `do_lock_collection()`: CollectionSettings'i disable et (DepositRequired enable edilemez buradan)
  - Sadece collection owner lock edebilir
- `do_lock_item_transfer()`: ItemSetting::Transferable'i disable et (Freezer rol)
- `do_unlock_item_transfer()`: Freezer rol
- `do_lock_item_properties()`: Admin rol, metadata ve/veya attributes'u lock et (KALICI)

---

## 7. pallet-uniques (Eski Pallet)

### pallet-uniques vs pallet-nfts Farklari

| Ozellik | pallet-uniques | pallet-nfts |
|---------|----------------|-------------|
| Storage Version | 1 | 1 |
| Storage prefix | `Class` (collection'lar icin) | `Collection` |
| CollectionDetails | owner, issuer, admin, freezer (dogrudan hesap), total_deposit, free_holding, is_frozen | owner, owner_deposit, items, item_metadatas, item_configs, attributes |
| ItemDetails | owner, approved (Option<AccountId>), is_frozen, deposit | owner, approvals (BoundedBTreeMap), deposit |
| Metadata | is_frozen bool icinde | CollectionSettings/ItemSettings ile ayri kontrol |
| CollectionMetadata | is_frozen field | ayrı CollectionConfig |
| Roller | CollectionDetails icinde dogrudan 4 alan | CollectionRoleOf StorageDoubleMap |
| Approval | `approved: Option<AccountId>` (TEK) | `approvals: BoundedBTreeMap` (COKLU) |
| Settings | `is_frozen` bool'lar | BitFlag sistemi (CollectionSettings/ItemSettings) |
| Swap | YOK | PendingSwapOf |
| Pre-signed mint | YOK | PreSignedMint |
| Attribute namespace | YOK (basit) | 4 namespace (Pallet/CollectionOwner/ItemOwner/Account) |
| MaxSupply | YOK | CollectionConfig.max_supply |
| MintSettings | YOK | MintType, price, start/end block |
| Price/Buy | YOK | ItemPriceOf |

### uniques Storage (uniques/lib.rs)
Dikkat: Storage prefix'i `Class` ve `ClassAccount` olarak konfigure edilmis:
```rust
#[pallet::storage_prefix = "Class"]
pub type Collection<T, I>...

#[pallet::storage_prefix = "ClassAccount"]
pub type CollectionAccount<T, I>...
```
Bu migration uyumluluğu icin eski isimleri korur.

### Migration Path
pallet-uniques sonraki nesil sistemler icin pallet-nfts'e gecilmesi onerilmektedir. Migration modulleri her iki pallet'te de mevcut. uniques'te `asset_ops` modulu (collection.rs, item.rs) yeni `AssetOps` trait implementasyonu saglar — bu yeni bir abstraction katmani.

---

## 8. pallet-broker (Coretime)

### Genel Mimari
pallet-broker, Polkadot'un Coretime sistemidir — parachains'lerin relay chain uzerinde "core zaman" satimini yonetir. StorageVersion::new(5).

**Temel Kavramlar:**
- **Core**: Relay chain uzerinde bir islem birimi (CoreIndex = u16)
- **Timeslice**: Relay chain bloklarinin gruplandirildigi zaman dilimi (her TimeslicePeriod blok = 1 timeslice)
- **Region**: Belirli bir timeslice araliginda belirli bir CoreMask icin coretime hakki
- **CoreMask**: 80-bit bitmask, bir core'un kac "part"ini kapsadigini gosterir (CORE_MASK_BITS=80)
- **Bulk Coretime**: Donem basi toplu coretime satisi
- **Instantaneous Coretime (InstaPool)**: Anlık/on-demand coretime

### Config Trait (lib.rs:80-137)
```rust
type Currency: Mutate<Self::AccountId> + Balanced<Self::AccountId>;
type AdminOrigin: EnsureOrigin<Self::RuntimeOrigin>;
type OnRevenue: OnUnbalanced<Credit<...>>;
type Coretime: CoretimeInterface;
type PriceAdapter: AdaptPrice<BalanceOf<Self>>;
type ConvertBalance: Convert<BalanceOf<Self>, RelayBalanceOf<Self>> + ConvertBack<...>;
type SovereignAccountOf: MaybeConvert<TaskId, Self::AccountId>;
type PalletId: Get<PalletId>;
type TimeslicePeriod: Get<RelayBlockNumberOf<Self>>;
type MaxLeasedCores: Get<u32>;
type MaxReservedCores: Get<u32>;
type MaxAutoRenewals: Get<u32>;
type MinimumCreditPurchase: Get<BalanceOf<Self>>;
```

### Storage Items (lib.rs:139-213)
| Storage | Deger |
|---------|-------|
| `Configuration` | ConfigRecordOf — pallet konfigurasyonu |
| `Reservations` | BoundedVec<Schedule> — sistem core rezervasyonlari |
| `ForceReservations` | BoundedVec<Schedule> — dinamik force rezervasyonlari |
| `Leases` | BoundedVec<LeaseRecordItem> — eski lease'ler |
| `Status` | StatusRecord — genel sistem durumu |
| `SaleInfo` | SaleInfoRecordOf — mevcut satis bilgisi |
| `PotentialRenewals` | Map<PotentialRenewalId, PotentialRenewalRecordOf> |
| `Regions` | Map<RegionId, RegionRecordOf> |
| `Workplan` | Map<(Timeslice, CoreIndex), Schedule> |
| `Workload` | Map<CoreIndex, Schedule> — current core assignment |
| `InstaPoolContribution` | Map<RegionId, ContributionRecord> |
| `InstaPoolIo` | Map<Timeslice, PoolIoRecord> |
| `InstaPoolHistory` | Map<Timeslice, InstaPoolHistoryRecord> |
| `CoreCountInbox` | Option<CoreIndex> |
| `AutoRenewals` | BoundedVec<AutoRenewalRecord> |
| `RevenueInbox` | Option<OnDemandRevenueRecord> |

### Region Kavramı (types.rs)
```rust
pub struct RegionRecord<AccountId, Balance> {
    pub end: Timeslice,        // bitis timeslice'i
    pub owner: Option<AccountId>, // None = unowned
    pub paid: Option<Balance>,    // None = renewal yok
}
```

`RegionId` (fp_coretime'dan):
```rust
pub struct RegionId {
    pub begin: Timeslice, // baslangic timeslice'i
    pub core: CoreIndex,  // core indeksi
    pub mask: CoreMask,   // 80-bit coretime mask'i
}
```
Region, yani coretime hakki su uc boyutla tanimlaniyor: (zaman araliginin baslangici, core indeksi, kullanilan core part mask'i).

### Extrinsicler (lib.rs:626-800+)

| Call | Indeks | Aciklama |
|------|--------|----------|
| `configure` | 0 | AdminOrigin/Root, ConfigRecord set et |
| `reserve` | 1 | AdminOrigin/Root, sistem core rezervasyonu |
| `unreserve` | 2 | AdminOrigin/Root, rezervasyon iptal |
| `set_lease` | 3 | AdminOrigin/Root, legacy lease set et |
| `start_sales` | 4 | AdminOrigin/Root, Bulk Coretime satisini baslat |
| `purchase` | 5 | Signed, bulk coretime satin al |
| `renew` | 6 | Signed, coretime yenile |
| `transfer` | 7 | Region sahibi, region ownership aktar |
| `partition` | 8 | Region sahibi, region'i zaman boyutunda bol |
| `interlace` | 9 | Region sahibi, region'i mask boyutunda bol |
| `assign` | 10 | Region sahibi, region'i goreve ata |
| `pool` | 11 | Region sahibi, InstaPool'a ekle |
| `claim_revenue` | 12 | Signed, InstaPool gelirleri talep et |
| `purchase_credit` | 13 | Signed, on-demand credit satin al |
| `drop_region` | 14 | Signed, suresi dolmus region'i temizle |
| `drop_contribution` | 15 | Signed, suresi dolmus contribution'i temizle |
| `drop_history` | 16 | Signed, eski history kaydini temizle |
| `drop_renewal` | 17 | Signed, gecmis renewal kaydini temizle |
| `request_core_count` | 18 | AdminOrigin/Root |
| `notify_core_count` | 19 | Coretime arayuzu, core count bildir |
| `notify_revenue` | 20 | Coretime arayuzu, on-demand gelir bildir |
| `swap_leases` | 21 | AdminOrigin/Root, iki task'in lease'ini degistir |
| `enable_auto_renew` | 22 | Signed, auto-renewal etkinlestir |
| `disable_auto_renew` | 23 | Signed, auto-renewal devre disi birak |
| `force_reserve` | 24 | AdminOrigin/Root, anlik etki ile core rezerve et |
| `remove_lease` | 25 | AdminOrigin/Root |
| `remove_potential_renewal` | 26 | AdminOrigin/Root |
| `remove_assignment` | 27 | AdminOrigin/Root, workplan'dan assignment kaldir |

### Purchase Akisi (dispatchable_impls.rs:149-176)

```
1. Status ve SaleInfo var mi?
2. Satis baslamis mi? (now > sale.sale_start)
3. Core var mi? (first_core < core_count, cores_sold < cores_offered)
4. Price hesapla: sale_price(sale, now)
5. price_limit >= price (Overpriced hatasi)
6. purchase_core: charge(who, price) + cores_sold.inc()
7. issue(): Regions'a RegionId -> RegionRecord ekle
8. Purchased event
```

### Sale Fiyat Algoritması (utility_impls.rs:62-66)
```rust
pub fn sale_price(sale, now) -> Balance {
    let num = (now - sale.sale_start).min(sale.leadin_length);
    let through = FixedU64::from_rational(num, sale.leadin_length); // [0,1]
    PriceAdapter::leadin_factor_at(through) * sale.end_price
}
```
`through=0` -> en yuksek fiyat (leadin baslangici)
`through=1` -> `end_price` (leadin sonu, sabit fiyat doneminin baslangici)

### CenterTargetPrice Algoritması (adapt_price.rs:110-136)
```rust
fn leadin_factor_at(when: FixedU64) -> FixedU64 {
    if when <= 0.5 {
        100 - when * 180   // [100x .. 10x] first half
    } else {
        19 - when * 18     // [10x .. 1x] second half
    }
}
// when=0.0 -> factor=100x (max fiyat = 100 * end_price)
// when=0.5 -> factor=10x  (target fiyat = 10 * end_price)
// when=1.0 -> factor=1x   (end_price)

fn adapt_price(performance) -> AdaptedPrices {
    end_price = sellout_price * 0.1   // satilan fiyatin 1/10'u
    target_price = sellout_price       // satilan fiyat ayni kalir
}
```

**Renew Fiyat Kapi (dispatchable_impls.rs:210):**
```rust
let price_cap = cmp::max(record.price + config.renewal_bump * record.price, end_price);
let price = Self::sale_price(&sale, now).min(price_cap);
```
Yenileme fiyati: `min(anlık_satis_fiyati, eski_fiyat * (1 + renewal_bump))`. Her donem fiyat en fazla `renewal_bump` kadar artabilir. Minimum ise mevcut `end_price`.

### Region Partition (dispatchable_impls.rs:255-285)
Region'i zaman boyutunda bol:
- `pivot = region_id.begin + pivot_offset`
- `pivot < region.end` ve `pivot > region_id.begin` olmali
- Orijinal region: `[begin, pivot)` ve yeni region: `[pivot, end)`
- `paid = None` (bolundugunde renewal hakki kalkar)
- Pool'da ise force_unpool_region

### Region Interlace (dispatchable_impls.rs:287-318)
Region'i CoreMask boyutunda bol:
- `pivot`: yeni mask (orijinalin alt kumesi olmali)
- Iki yeni region olusur: `mask=pivot` ve `mask=(original_mask XOR pivot)`
- Her ikisi de ayni zaman araligini kapsar

### Assign (dispatchable_impls.rs:321-382)
Region'i bir TaskId'ye ata:
- `Finality::Final`: region storage'dan kaldirilir, renewal kaydi olusabilir
- `Finality::Provisional`: region storage'da kalir, yeniden atanabilir
- `duration == config.region_length && finality == Final && paid.is_some()` durumunda `PotentialRenewals'a` ekler
- CoreMask tamamen atanmissa `CompletionStatus::Complete`, degilse `Partial`

### Pool (dispatchable_impls.rs:392-416)
Region'i InstaPool'a ekle:
- `InstaPoolIo::mutate(begin, private += size)`, `InstaPoolIo::mutate(end, private -= size)`
- `InstaPoolContribution::insert`

### Revenue Claim (dispatchable_impls.rs:419-469)
```
payout = sum over timeslices of:
    total_payout * contributed_parts / pool_record.private_contributions
```
Pro-rata payment. `contribution_timeout` suresinde talep edilmezse drop edilir.

### on_initialize / do_tick (tick_impls.rs:35-86)
Her blokta:
1. `process_core_count`: CoreCountInbox'tan yeni core count al
2. `process_revenue`: RevenueInbox'tan on-demand geliri isle
3. `next_timeslice_to_commit` varsa:
   - `rotate_sale` (eger sale doneminin baslangicini gecmissek)
   - `process_pool`
   - `process_core_schedule` (her core icin Workplan -> Workload + relay'e bildir)
4. `current_timeslice` guncellendiyse `request_revenue_info_at` + `on_new_timeslice`

### Rotate Sale (tick_impls.rs:156-296)
- Unsold core'lar InstaPool'a atilir (80 bit/core)
- Yeni fiyatlar `PriceAdapter::adapt_price` ile hesaplanir
- Reservations icin core'lar atanir
- ForceReservations uygulanir (region_begin'den once, serbest core'lara)
- Leases kontrol edilir (suresi dolmak uzere olanlar PotentialRenewals'a eklenir)
- Yeni SaleInfo olusturulur:
  - `sale_start = now + interlude_length`
  - `cores_offered = min(limit_cores_offered, max_possible_sales)`
  - `ideal_cores_sold = ideal_bulk_proportion * cores_offered`
- Auto-renewals gerceklestirilir

### ConfigRecord (types.rs:252-293)
```rust
pub struct ConfigRecord<RelayBlockNumber> {
    pub advance_notice: RelayBlockNumber,  // commit oncesi gereken onceden uyari
    pub interlude_length: RelayBlockNumber, // satis oncesi bekleme suresi
    pub leadin_length: RelayBlockNumber,    // fiyat dusus suresi (0 olamaz!)
    pub region_length: Timeslice,          // region uzunlugu
    pub ideal_bulk_proportion: Perbill,    // ideal satis orani
    pub limit_cores_offered: Option<CoreIndex>, // max satilik core sayisi
    pub renewal_bump: Perbill,             // yenileme fiyat artis orani
    pub contribution_timeout: Timeslice,   // instapool gelir talep suresi
}
```
**Kritik:** `leadin_length.is_zero()` ise `validate()` HATA doner (types.rs:287-290).

### Auto-Renewal (dispatchable_impls.rs:559-603)
`do_enable_auto_renew()`:
- `SovereignAccountOf`: TaskId'den sovereign hesap turetir
- Eger core simdi yenileme donemindeyse anlik renew calistirilir
- Yoksa `workload_end_hint` ile ne zaman yenileneceği belirlenir
- `AutoRenewals`: CoreIndex'e gore siralı tutulur (binary_search)

`renew_cores()` tick_impls.rs:358-401:
- rotate_sale sirasinda calisir
- `SovereignAccountOf::maybe_convert(task)` ile payer hesap alınır
- Yetersiz bakiye/hatada `AutoRenewalFailed` event
- `AutoRenewalLimitReached` limiti asılamaz (MaxAutoRenewals)

### InstaPool Revenue Paylasimi
1. Relay chain on-demand geliri `notify_revenue` ile gelir
2. `process_revenue` calisir:
   - `system_contributions` icin sistem hesabindan charge edilir
   - `private_contributions` icin `maybe_payout` set edilir
3. `claim_revenue` ile kullanicı talep eder:
   - Her timeslice icin: `payout += total_payout * parts / private_contributions`

---

## 9. Test Analizi

### pallet-nfts Tests (tests.rs — mevcut ama okunmadı)
Benchmark dosyasindan (benchmarking.rs) cikarim yapilabilir: her extrinsic icin benchmark var. `BenchmarkHelper` trait'i: `collection(i)`, `item(i)`, `signer()`, `sign()`.

### pallet-broker Tests (tests.rs — mevcut)
`test_fungibles.rs` dosyasi — fungible currency mock icin.
`mock.rs` dosyasi — test runtime.
`adapt_price.rs` icerisindeki testler (169-308 satir):
- `linear_no_panic`: fiyat algoritmasinin panik yapmadigi test
- `leadin_price_bound_check`: leadin faktorlerin dogru oldugu test:
  - `when=0.0` -> 100x
  - `when=0.25` -> 55x
  - `when=0.5` -> 10x
  - `when=0.75` -> 5.5x
  - `when=1.0` -> 1x
- `price_stays_stable_on_optimal_sale`: optimal satisda fiyat stabil
- `price_adjusts_correctly_upwards/downwards`
- `price_never_goes_to_zero_and_recovers`: fiyat asla 0 olamaz
- `renewal_price_is_correct_on_no_sale`: satis yapilmamissa target = 10x end_price
- `minimum_price_works`: MinimumPrice adapter test

---

## 10. Kritik Sayısal Degerler ve Deposit Formuller

### pallet-nfts Deposit Formuller
```
collection_deposit = T::CollectionDeposit::get()
item_deposit = T::ItemDeposit::get() (eger DepositRequired aktif)
             = 0 (eger DepositRequired disabled)

metadata_deposit = DepositPerByte * data_len + MetadataDepositBase
attribute_deposit = DepositPerByte * (key_len + value_len) + AttributeDepositBase
```
Kaynak: metadata.rs:78-83, attributes.rs:100-108

### CollectionDetails.owner_deposit
`owner_deposit` su durumlarda artar:
- collection metadata set edildiginde
- CollectionOwner namespace attribute set edildiginde (depositor = collection owner)

Bu bir `saturating_accrue/saturating_reduce` ile tutulur. destroy sırasında tum bakiye iade edilir.

### pallet-nfts Limit Sabitleri
- `ApprovalsLimit`: item basi max approval sayisi
- `ItemAttributesApprovalsLimit`: attribute approval max sayisi
- `MaxTips`: tek cagride max tip sayisi
- `MaxAttributesPerCall`: set_attributes_pre_signed'de max attribute sayisi
- `MaxDeadlineDuration`: swap/approval icin max blok suresi

### Coretime Sayisal Degerler
- `CORE_MASK_BITS = 80` — bir core'un toplam part sayisi
- `PartsOf57600 = u16` — 57600 = 80 * 720 = 80 parts * 720 blok/donem (yaklasik)
- Her CoreMaskBit = 1/80 core per timeslice
- `RegionId.begin` bir timeslice; bir timeslice = `TimeslicePeriod` relay blok
- Leadin fiyat araligı: [100x end_price .. 1x end_price]
- Renewal bump: konfigurasyona gore (default 0 olmali)
- `ideal_bulk_proportion`: Perbill olarak ideal satis orani

### Broker Tick Davranisi
- Her parachain blogu `on_initialize` -> `do_tick` cagrisi
- Timeslice commit: `last_committed_timeslice < latest_ready_to_commit`
- Sale rotation: `commit_timeslice >= sale.region_begin` oldugunda
- InstaPool history: `process_pool(commit_timeslice)` cagrisi ile guncellenir

---

## 11. Nova Shield Chain Icin Cikarsimlar

### pallet-nfts Kullanimi

**Dogrudan Adaptasyon Onerileri:**
1. **Collection/Item tipleri**: `CollectionId: Incrementable` — otomatik artirma saglar. Nova Shield icin `u64` veya ozel bir tip kullanilabilir.
2. **Deposit sistemi**: `DepositRequired` disabled edilirse deposit sifir olur — protocol-owned item'lar icin (stake certificate NFT'leri vb.) uygun.
3. **MintType::HolderOf**: Baska bir collection'a sahip olanlarin mint edebilmesi — Nova Shield'in staking/governance ile NFT entegrasyonu icin kullanilabilir.
4. **Pre-signed mint**: Off-chain olarak yetkilendirilmis mint — validator node'larindan NFT basimi icin; ECDSA veya SR25519 imza ile.
5. **Attribute namespaces**: `Pallet` namespace ile sistem seviyesi metadata (on-chain state kaniti).

**Dikkat Edilmesi Gerekenler:**
- CollectionSettings inverted flag sistemi — Nova Shield uygulamasinda bu kafa karistirici olabilir; belgeleme gerekli.
- `force_create` ile deposit bypass edilebilir — governance paleti ile birlikte dikkatli kullanilmali.
- Approvals transfer sırasında otomatik silinir — tasarım gerektirirse dikkatli davranilmali.
- Atomic swap'ta `witness_price` tam eslesme zorunlu — kullanici deneyimi zor olabilir.
- Swap cancel: deadline gecince herkes iptal edebilir — bu istenen davranis.
- `buy_item`: bid_price fazlasi iade EDILMEZ (tam price odenir) — bu standart davranis.
- `TransferDisabled` attribute ile transfer sistem tarafindan bloke edilebilir — lock contract icin kullanilabilir.

**Royalty Sistemi**: pallet-nfts'te royalty mekanizmasi YOKTUR. Sadece tip (`pay_tips`) mekanizmasi var, o da gönüllü. Nova Shield icin royalty isteniyorsa buy_sell akisina ek mantik eklenmeli veya custom pallet gerekir.

**Onerilen Architecture:**
```
Nova Shield NFT:
  - pallet-nfts fork veya direkt kullanim
  - Custom MintType::HolderOf(staking_collection) ile stake proof NFT
  - Attribute Namespace::Pallet ile ZK-proof hash storage
  - DepositRequired DISABLE — genesis collection icin
  - force_create -> genesis_build icinde
```

### pallet-broker Notlari

Nova Shield bir Layer 1'dir (parachain degil), bu yuzden pallet-broker DOGRUDAN kullanilmaz. Ancak:

1. **Fiyat Adaptasyon Algoritması**: `CenterTargetPrice` tasarimi ilham verici. Nova Shield'in coretime benzeri kapasite satisi varsa (validator slot satisi, network bandwidth satisi) bu algoritma adapte edilebilir:
   - Leadin: 100x'den 1x'e inen fiyat (Dutch auction)
   - Adapt: satis performansina gore bir sonraki donem fiyati
   - MinimumPrice wrapper ile minimum fiyat garantisi

2. **Region Soyutlamasi**: Bir kapasite birimini (begin, core/slot, mask) ile temsil etmek; partition (zaman boyutu) + interlace (kapasite boyutu) split islemleri — paralel/kademeli kapasite satisi icin.

3. **Auto-renewal**: `SovereignAccountOf::maybe_convert(task)` -> hesap turetimi — Nova Shield'de validator'larin otomatik slot yenilemesi icin model.

4. **ConfigRecord.leadin_length sifir olamaz** — piyasa mantigi icin kritik; Dutcg auction minimum leadin gerektirir.

5. **InstaPool modeli**: "spot market" coretime icin. Nova Shield'de varsa anlık islem/blok satisi icin model alinabilir.

### Genel Polkadot SDK Pattern'leri

- **Witness data pattern**: Destroy, cancel_approval gibi islemlerde onceden sayim sagla (on-chain loop'tan kaci). `BadWitness` hatasi ile tutarlilik saglaniyor.
- **BoundedBTreeMap/BoundedBTreeSet**: Limit zorlamak icin standart yaklasim — tum koleksiyonlar icin uygulanmali.
- **Inverted bitflag settings**: Yeni ozellikleri varsayilan olarak AKTIF tutmak icin (bit set = disabled) — sifirdan migration kolayligi.
- **Generic instance (I: 'static = ())**: Ayni pallet'in coklu ornek konfigurasyonu — Nova Shield icerisinde farkli NFT kategorileri icin kullanilabilir.
- **Incrementable CollectionId**: Manuel ID yonetimi yerine otomatik artirma — uygulamasi basit, collision yok.
- **ForceOrigin**: Governance/sudo ile bypass mekanizmasi — test/migration/acil durum icin kritik.
