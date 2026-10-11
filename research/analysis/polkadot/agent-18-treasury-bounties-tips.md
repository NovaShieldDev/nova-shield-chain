# pallet-treasury + pallet-bounties + pallet-tips + pallet-child-bounties — Tam Analiz
**Agent**: 18 | **Tarih**: 2026-07-22

---

## KAPSANAN DOSYALAR

| Pallet | Dosyalar |
|---|---|
| pallet-treasury | lib.rs, tests.rs, migration.rs, benchmarking.rs, weights.rs |
| pallet-bounties | lib.rs, tests.rs, migrations/mod.rs, migrations/v4.rs, benchmarking.rs, weights.rs |
| pallet-tips | lib.rs, tests.rs, migrations/mod.rs, migrations/v4.rs, migrations/unreserve_deposits.rs, benchmarking.rs, weights.rs |
| pallet-child-bounties | lib.rs, tests.rs, migration.rs, benchmarking.rs, weights.rs |
| pallet-multi-asset-bounties | lib.rs, tests.rs, mock.rs, benchmarking.rs, weights.rs |

---

## BÖLÜM 1: pallet-treasury

### 1.1 Spend Lifecycle

**Eski API (spend_local — DEPRECATED, Mayıs 2025'te kaldırılacak):**
```
[Teklif] spend_local → Approvals kuyruğuna ekle
[on_initialize] SpendPeriod dolunca → spend_funds() → her onayı işle → Awarded event
```
- `spend_local`: call_index 3, lib.rs:528 — sadece SpendOrigin, anında kuyruğa girer, deposit 0
- `remove_approval`: call_index 4, lib.rs:604 — RejectOrigin kaldırır, deposit iade edilmez

**Yeni API (spend — aktif):**
```
spend() → SpendStatus::Pending → payout() → PaymentState::Attempted → check_status() → SpendProcessed
```
- `spend`: call_index 5, lib.rs:651 — SpendOrigin, herhangi AssetKind, valid_from + PayoutPeriod = expire_at
- `payout`: call_index 6, lib.rs:736 — herhangi imzalayan tetikleyebilir, spend.valid_from <= now < expire_at
- `check_status`: call_index 7, lib.rs:780 — Success/Unknown → kaldır (Pays::No); Failure → Failed state; InProgress → Inconclusive hata
- `void_spend`: call_index 8, lib.rs:834 — RejectOrigin iptal eder, sadece Pending veya Failed ise

**on_initialize akışı** (lib.rs:452-489):
1. `pot != deactivated` ise reactivate/deactivate güncelle
2. `LastSpendPeriod` hesapla (yoksa `update_last_spend_period()` çağır)
3. `blocks_since_last_spend_period / SpendPeriod` → kaç dönem geçtiğini bul
4. Eğer `spend_periods_passed > 0` → `spend_funds()` çağır

### 1.2 Burn Formülü

**Kaynak:** lib.rs:964-978

```rust
// Sadece missed_any == false VE Burn != 0 ise yakılır
let one_minus_burn = T::Burn::get().left_from_one();  // 1 - burn_rate
let percent_left = one_minus_burn.saturating_pow(spend_periods_passed.unique_saturated_into());
let new_budget_remaining = percent_left * budget_remaining;
let burn = budget_remaining.saturating_sub(new_budget_remaining);
```

**Formül özeti:**
- `burned = budget_remaining * (1 - (1 - Burn)^N)` — N dönem geçmişse compound yakma
- Tek dönem: `burned = Burn * budget_remaining`
- Birden fazla dönem: üstel azalma (her dönem kalan bakiyenin Burn yüzdesi yakılır)

**KRITIK:** Burn sadece "missed_any == false" durumunda uygulanır. Eğer herhangi bir harcama başarısız olduysa (bütçe yetersiz) yanma olmaz. Bu, küçük treasury balansını koruyan güvenlik mekanizmasıdır.

### 1.3 Sabitler

| Sabit | Trait | Lokasyon |
|---|---|---|
| `SpendPeriod` | `BlockNumberFor` | lib.rs:231 |
| `Burn` | `Permill` | lib.rs:235 |
| `PalletId` | `PalletId` | lib.rs:239 |
| `MaxApprovals` | `u32` | lib.rs:257 — DEPRECATED |
| `PayoutPeriod` | `BlockNumberFor` | lib.rs:287 |

**pot() formülü** (lib.rs:999-1003):
```rust
pub fn pot() -> BalanceOf<T, I> {
    T::Currency::free_balance(&Self::account_id())
        .saturating_sub(T::Currency::minimum_balance())
}
```
Existential deposit treasury bakiyesine dahil değildir.

**account_id()** (lib.rs:856-858):
```rust
T::PalletId::get().into_account_truncating()
```

### 1.4 Finansman Kaynakları

Treasury nasıl finanse edilir:

1. **OnUnbalanced implementasyonu** (lib.rs:1085-1093):
```rust
impl<T: Config<I>, I: 'static> OnUnbalanced<NegativeImbalanceOf<T, I>> for Pallet<T, I> {
    fn on_nonzero_unbalanced(amount: NegativeImbalanceOf<T, I>) {
        let _ = T::Currency::resolve_creating(&Self::account_id(), amount);
        Self::deposit_event(Event::Deposit { value: numeric_amount });
    }
}
```
Bu trait ile:
- Transaction fee'lerinin bir kısmı (ToAuthor + ToTreasury split)
- Slash'ların bir kısmı
- Diğer imbalance kaynakları treasury'e akar

2. **SpendFunds trait** (lib.rs:138-144): External palletlerin (bounties gibi) treasury bütçesini kullanmasını sağlar

3. **TreasuryAccountId** (lib.rs:1097-1106): TypedGet implementasyonu, başka palletlerin treasury hesabına referans etmesini sağlar

### 1.5 Multi-Asset Spend

**Eski yol:** `spend_local` — sadece native currency, pot'tan çekilir, SpendPeriod'da otomatik ödenir

**Yeni yol:** `spend` — herhangi AssetKind, Paymaster trait üzerinden ödenir

**spend() içinde** (lib.rs:658-713):
- `valid_from` + `PayoutPeriod` = `expire_at` — zaman penceresi
- `BalanceConverter::from_asset_balance(amount, asset_kind)` → native eşdeğer hesapla
- SpendOrigin'in maksimum iznini kontrol et
- `SpendContext` ile aynı extrinsic içinde çoklu spend kontrolü

**PaymentState döngüsü:**
```
Pending → Attempted{id} → (check_status) → Success → kaldır
                                          → Failure → Failed → (payout retry)
                                          → InProgress → Inconclusive hata
```

**PaymentInspect / Pay trait:** lib.rs:274 — `Paymaster: Pay<Beneficiary=..., AssetKind=...>`

### 1.6 Migration

**cleanup_proposals migration** (migration.rs:30-70):
- Onaylanmamış proposal'ların bond'larını iade eder
- `spend_local` kaldırıldığında çalışacak

---

## BÖLÜM 2: pallet-bounties

### 2.1 Bounty Lifecycle — Tüm State'ler

**BountyStatus enum** (lib.rs:166-199):
```
Proposed → Approved → Funded → CuratorProposed → Active → PendingPayout
                                                         ↘ (unassign) → Funded
        ↘ ApprovedWithCurator → (spend period) → CuratorProposed
```

**Detaylı Geçişler:**

| Fonksiyon | Kaynak State | Hedef State | Yetki |
|---|---|---|---|
| `propose_bounty` | — | Proposed | Signed |
| `approve_bounty` | Proposed | Approved | SpendOrigin |
| `approve_bounty_with_curator` | Proposed | ApprovedWithCurator | SpendOrigin |
| Treasury SpendPeriod | Approved | Funded | Otomatik |
| Treasury SpendPeriod | ApprovedWithCurator | CuratorProposed | Otomatik |
| `propose_curator` | Funded | CuratorProposed | SpendOrigin |
| `accept_curator` | CuratorProposed | Active | Curator |
| `extend_bounty_expiry` | Active | Active (update_due güncellenir) | Curator |
| `award_bounty` | Active | PendingPayout | Curator |
| `claim_bounty` | PendingPayout | (silindi) | Herhangi |
| `unassign_curator` | Active/CuratorProposed/PendingPayout | Funded | RejectOrigin/curator/herhangi* |
| `close_bounty` | Proposed/Funded/Active | (silindi) | RejectOrigin |

*Herhangi biri sadece curator "inactive" ise (update_due geçmişse) unassign yapabilir.

**Proposer Bond Formülü** (lib.rs:1113-1119):
```rust
fn calculate_bounty_deposit(description: &BoundedVec<u8, T::MaximumReasonLength>) -> BalanceOf<T, I> {
    T::BountyDepositBase::get().saturating_add(
        T::DataDepositPerByte::get().saturating_mul((description.len() as u32).into()),
    )
}
```
`bond = BountyDepositBase + DataDepositPerByte * description_len`

### 2.2 Curator Deposit Formülü

**Kaynak:** lib.rs:1048-1060

```rust
pub fn calculate_curator_deposit(fee: &BalanceOf<T, I>) -> BalanceOf<T, I> {
    let mut deposit = T::CuratorDepositMultiplier::get() * *fee;

    if let Some(max_deposit) = T::CuratorDepositMax::get() {
        deposit = deposit.min(max_deposit)
    }

    if let Some(min_deposit) = T::CuratorDepositMin::get() {
        deposit = deposit.max(min_deposit)
    }

    deposit
}
```

**Formül:** `curator_deposit = clamp(CuratorDepositMultiplier * fee, CuratorDepositMin, CuratorDepositMax)`

- `CuratorDepositMultiplier`: Permill (örn. %50)
- `CuratorDepositMax`: Option — üst sınır
- `CuratorDepositMin`: Option — alt sınır

**accept_curator'da kullanımı** (lib.rs:659-661):
```rust
let deposit = Self::calculate_curator_deposit(&bounty.fee);
T::Currency::reserve(curator, deposit)?;
bounty.curator_deposit = deposit;
```

### 2.3 Sabitler

| Sabit | Tip | Açıklama | Kaynak |
|---|---|---|---|
| `BountyDepositBase` | `BalanceOf` | Bounty önerisi için base deposit | lib.rs:279 |
| `BountyDepositPayoutDelay` | `BlockNumber` | Ödeme öncesi bekleme süresi | lib.rs:283 |
| `BountyUpdatePeriod` | `BlockNumber` | Curator'ın güncelleme yapması gereken süre | lib.rs:292 |
| `CuratorDepositMultiplier` | `Permill` | Curator deposit = bu * curator fee | lib.rs:299 |
| `CuratorDepositMax` | `Option<BalanceOf>` | Curator deposit üst sınırı | lib.rs:303 |
| `CuratorDepositMin` | `Option<BalanceOf>` | Curator deposit alt sınırı | lib.rs:307 |
| `BountyValueMinimum` | `BalanceOf` | Minimum bounty değeri | lib.rs:311 |
| `DataDepositPerByte` | `BalanceOf` | Açıklama başına deposit | lib.rs:315 |
| `MaximumReasonLength` | `u32` | Max açıklama uzunluğu | lib.rs:326 |

### 2.4 Award/Claim Mekanizması

**award_bounty** (lib.rs:692-726):
- Curator çağırır, Active → PendingPayout
- `unlock_at = treasury_block_number + BountyDepositPayoutDelay`
- Aktif child bounty varsa hata: `HasActiveChildBounty`

**claim_bounty** (lib.rs:738-786):
```rust
let balance = T::Currency::free_balance(&bounty_account);
let fee = bounty.fee.min(balance);          // curator ücreti (max=bakiye)
let payout = balance.saturating_sub(fee);   // beneficiary'e gidecek

// children_fee: child bounty curator'larının toplam ücreti
let children_fee = T::ChildBountyManager::children_curator_fees(bounty_id);
debug_assert!(children_fee <= fee);
let final_fee = fee.saturating_sub(children_fee);  // parent curator'a gidecek net ücret

T::Currency::transfer(&bounty_account, &curator, final_fee, AllowDeath);
T::Currency::transfer(&bounty_account, &beneficiary, payout, AllowDeath);
```

**Ödeme dağılımı:**
- Bounty hesabı bakiyesi = curator fee + payout
- Parent curator net ücreti = curator_fee - child_bounties_curator_fees (toplam)
- Beneficiary = bakiye - curator_fee

### 2.5 SpendFunds Entegrasyonu

**lib.rs:1163-1213** — Treasury SpendFunds trait implementasyonu:
- Treasury her spend döneminde `BountyApprovals` listesini işler
- `Approved` → `Funded` (veya `ApprovedWithCurator` → `CuratorProposed`)
- Proposer bond iade edilir
- Bounty hesabına `bounty.value` yatırılır

**bounty_account_id** (lib.rs:1072-1075):
```rust
T::PalletId::get().into_sub_account_truncating(("bt", id))
```

### 2.6 Curator Unassign — Slash Mantığı

**lib.rs:561-631:**
| Durum | Slash |
|---|---|
| RejectOrigin çağırır (Active/PendingPayout) | Curator deposit slash edilir |
| Curator kendisi çağırır (Active) | Deposit iade edilir |
| 3. kişi çağırır ve update_due geçmişse | Curator deposit slash edilir |
| 3. kişi çağırır ve update_due henüz gelmemişse | `Premature` hata |

**TransferAllAssets trait** (lib.rs:214-263): close_bounty'de bounty hesabındaki tüm varlıkları (native + fungibles) treasury'e geri aktarır.

---

## BÖLÜM 3: pallet-tips

### 3.1 Tip Lifecycle

```
report_awesome() → Tips[hash] = OpenTip{finder, deposit, finders_fee:true}
tip_new()        → Tips[hash] = OpenTip{finder=tipper, deposit=0, finders_fee:false}
tip()            → insert_tip_and_check_closing → threshold >= count/2 → closes = now + TipCountdown
close_tip()      → payout_tip() → median öde + finder ücreti
slash_tip()      → finder deposit slash
retract_tip()    → finder tarafından iptal, deposit iade
```

**OpenTip struct** (lib.rs:98-120):
```rust
pub struct OpenTip<AccountId, Balance, BlockNumber, Hash> {
    reason: Hash,
    who: AccountId,
    finder: AccountId,
    deposit: Balance,
    closes: Option<BlockNumber>,    // None = henüz threshold yok
    tips: Vec<(AccountId, Balance)>, // Sorted by AccountId
    finders_fee: bool,
}
```

**Tip Hash hesabı** (lib.rs:271):
```rust
let hash = T::Hashing::hash_of(&(&reason_hash, &who));
```

**report_awesome deposit** (lib.rs:274-275):
```rust
let deposit = T::TipReportDepositBase::get() +
    T::DataDepositPerByte::get() * (reason.len() as u32).into();
```

### 3.2 Median Hesabı

**Kaynak:** lib.rs:572-580

```rust
fn payout_tip(...) -> DispatchResult {
    let mut tips = tip.tips;
    Self::retain_active_tips(&mut tips);  // Eski üyeleri filtrele
    tips.sort_by_key(|i| i.1);           // Miktara göre sırala

    let max_payout = pallet_treasury::Pallet::<T, I>::pot();

    let mut payout = tips
        .get(tips.len() / 2)      // ORTADAKI ELEMAN (medyan)
        .ok_or(Error::<T, I>::NoActiveTippers)?
        .1
        .min(max_payout);         // treasury pot'un üzerinde olamaz
```

**Medyan Formülü:** `tips[tips.len() / 2]` — indexing ile alt-medyan.
- 3 tip: `[a, b, c]` → index 1 = b (orta)
- 4 tip: `[a, b, c, d]` → index 2 = c (üst-orta)
- 2 tip: `[a, b]` → index 1 = b

**Threshold kontrolü** (lib.rs:532-538):
```rust
let threshold = T::Tippers::count().div_ceil(2);  // ceil(N/2)
if tip.tips.len() >= threshold && tip.closes.is_none() {
    tip.closes = Some(frame_system::Pallet::<T>::block_number() + T::TipCountdown::get());
    true
} else {
    false
}
```
Threshold: `ceil(tippers_count / 2)` — yarıdan az çoğunluk yeterli.

### 3.3 Finder Ücreti Hesabı

**Kaynak:** lib.rs:587-594

```rust
if tip.finders_fee && tip.finder != tip.who {
    let finders_fee = T::TipFindersFee::get() * payout;
    payout -= finders_fee;
    let res = T::Currency::transfer(&treasury, &tip.finder, finders_fee, KeepAlive);
}
```
- `finders_fee = TipFindersFee * median_tip`
- `beneficiary_payout = median_tip - finders_fee`
- Finder == who ise ücret ödenmez

### 3.4 Sabitler

| Sabit | Tip | Açıklama | Kaynak |
|---|---|---|---|
| `MaximumReasonLength` | `u32` | Max tip neden uzunluğu | lib.rs:147 |
| `DataDepositPerByte` | `BalanceOf` | Neden başına deposit | lib.rs:151 |
| `TipCountdown` | `BlockNumber` | Threshold sonrası bekleme süresi | lib.rs:155 |
| `TipFindersFee` | `Percent` | Finder'ın aldığı yüzde | lib.rs:159 |
| `TipReportDepositBase` | `BalanceOf` | report_awesome base deposit | lib.rs:163 |
| `MaxTipAmount` | `BalanceOf` | Tek tip için maksimum | lib.rs:167 |

**Integrity test** (lib.rs:482-486):
```rust
assert!(!T::TipReportDepositBase::get().is_zero(), "`TipReportDepositBase` should not be zero");
```

### 3.5 retain_active_tips

**Kaynak:** lib.rs:542-559

Tippers set güncellenmiş olabilir (üyeler değişebilir). Her işlemde aktif olmayan tippers'ın oyları filtrelenir:
```rust
fn retain_active_tips(tips: &mut Vec<(T::AccountId, BalanceOf<T, I>)>) {
    let members = T::Tippers::sorted_members();
    // Linear merge, O(T)
    tips.retain(|(ref a, _)| ...)
}
```

### 3.6 slash_tip

**lib.rs:460-476:**
- RejectOrigin çağırır
- Finder'ın depositi `slash_reserved` ile slash edilir
- `OnSlash::on_unbalanced(imbalance)` — slash edilenler buraya gider

---

## BÖLÜM 4: pallet-child-bounties

### 4.1 Parent-Child İlişkisi

**Struct** (lib.rs:99-111):
```rust
pub struct ChildBounty<AccountId, Balance, BlockNumber> {
    pub parent_bounty: BountyIndex,
    pub value: Balance,
    pub fee: Balance,
    pub curator_deposit: Balance,
    pub status: ChildBountyStatus<AccountId, BlockNumber>,
}
```

**Storage** (lib.rs:218-252):
- `ChildBounties`: DoubleMap `(parent_id, child_id) → ChildBounty`
- `ParentChildBounties`: `parent_id → aktif_count` — award/close ile azalır
- `ParentTotalChildBounties`: `parent_id → toplam_count` (tamamlananlar dahil) — ID üretimi için
- `ChildrenCuratorFees`: `parent_id → toplam_child_curator_fee` — parent claim'de düşülür
- `ChildBountyDescriptionsV1`: `(parent_id, child_id) → description`

**Hesap türetme** (lib.rs:854-862):
```rust
T::PalletId::get().into_sub_account_truncating(("cb", parent_bounty_id, child_bounty_id))
```
Prefix `"cb"` → `"bt"` ile çakışmaz.

**ChildBountyManager trait implementasyonu** (lib.rs:969-996):
```rust
impl<T: Config> pallet_bounties::ChildBountyManager<BalanceOf<T>> for Pallet<T> {
    fn child_bounties_count(bounty_id: BountyIndex) -> BountyIndex {
        ParentChildBounties::<T>::get(bounty_id)
    }

    fn children_curator_fees(bounty_id: BountyIndex) -> BalanceOf<T> {
        let total = ChildrenCuratorFees::<T>::get(bounty_id);
        ChildrenCuratorFees::<T>::remove(bounty_id);  // Claim sonrası temizle
        total
    }

    fn bounty_removed(bounty_id: BountyIndex) {
        // Tüm ilgili storage temizlenir
        ParentChildBounties::<T>::remove(bounty_id);
        ParentTotalChildBounties::<T>::remove(bounty_id);
    }
}
```

### 4.2 Child Bounty Lifecycle

**ChildBountyStatus enum** (lib.rs:115-138):
```
Added → CuratorProposed → Active → PendingPayout → (claim → kaldırıldı)
```

**Detaylı Geçişler:**

| Fonksiyon | Kaynak | Hedef | Yetki |
|---|---|---|---|
| `add_child_bounty` | — | Added | Parent curator |
| `propose_curator` | Added | CuratorProposed | Parent curator |
| `accept_curator` | CuratorProposed | Active | Child curator |
| `award_child_bounty` | Active | PendingPayout | Parent veya child curator |
| `claim_child_bounty` | PendingPayout | (kaldırıldı) | Herhangi |
| `unassign_curator` | Active/CuratorProposed | Added | RejectOrigin/parent curator/curator/herhangi (inactive ise) |
| `close_child_bounty` | Added/CuratorProposed/Active | (kaldırıldı) | Parent curator/RejectOrigin |

### 4.3 Para Akışı

**add_child_bounty'de** (lib.rs:302-320):
```rust
// Parent bounty hesabından child bounty hesabına transfer
let parent_bounty_account = pallet_bounties::Pallet::<T>::bounty_account_id(parent_bounty_id);
let child_bounty_account = Self::child_bounty_account_id(parent_bounty_id, child_bounty_id);

let bounty_balance = T::Currency::free_balance(&parent_bounty_account);
let new_bounty_balance = bounty_balance.checked_sub(&value)
    .ok_or(Error::<T>::InsufficientBountyBalance)?;

T::Currency::transfer(&parent_bounty_account, &child_bounty_account, value, KeepAlive)?;
```

**claim_child_bounty'de** (lib.rs:714-752):
```rust
let balance = T::Currency::free_balance(&child_bounty_account);
let curator_fee = child_bounty.fee.min(balance);
let payout = balance.saturating_sub(curator_fee);

T::Currency::unreserve(curator, child_bounty.curator_deposit);
T::Currency::transfer(&child_bounty_account, curator, curator_fee, AllowDeath);
T::Currency::transfer(&child_bounty_account, beneficiary, payout, AllowDeath);
```

**close_child_bounty'de** (lib.rs:934-946):
```rust
// Child bounty hesabından parent bounty hesabına iade
T::Currency::transfer(&child_bounty_account, &parent_bounty_account, balance, AllowDeath);
```

### 4.4 Child Curator Deposit Hesabı

**lib.rs:840-851:**
```rust
fn calculate_curator_deposit(
    parent_curator: &T::AccountId,
    child_curator: &T::AccountId,
    bounty_fee: &BalanceOf<T>,
) -> BalanceOf<T> {
    if parent_curator == child_curator {
        return Zero::zero();  // Parent curator ise deposit yok!
    }
    pallet_bounties::Pallet::<T>::calculate_curator_deposit(bounty_fee)
}
```
**Parent curator child bounty'yi de yönetirse sıfır ek deposit gerekir.**

### 4.5 Curator Fees Birikimi

Child bounty `propose_curator`'da (lib.rs:389-391):
```rust
ChildrenCuratorFees::<T>::mutate(parent_bounty_id, |value| {
    *value = value.saturating_add(fee)
});
```
Parent `claim_bounty` çağrıldığında bu toplam düşülerek parent curator'ın net ücreti hesaplanır.

---

## BÖLÜM 5: pallet-multi-asset-bounties (BONUS)

Bu pallet, `pallet-bounties` ve `pallet-child-bounties`'in geliştirilmiş versiyonudur. Herhangi asset türünü destekler.

### 5.1 Temel Farklar

| Özellik | pallet-bounties | pallet-multi-asset-bounties |
|---|---|---|
| Asset desteği | Sadece native | Herhangi AssetKind |
| Lifecycle | propose → approve → fund | fund_bounty (tek adım) |
| Curator deposit | reserve() | Consideration trait (hold) |
| Child bounty entegrasyonu | Ayrı pallet | Dahili (tek pallet) |
| Ödeme mekanizması | Currency::transfer | Paymaster (PayWithSource) |
| Durum makinesi | 6 state | FundingAttempted, Funded, Active, PayoutAttempted, RefundAttempted |

### 5.2 Yeni State Makinesi

```
fund_bounty → FundingAttempted → (check_status, payment OK) → Funded → (accept_curator) → Active
Active → award_bounty → PayoutAttempted → (check_status) → (kaldırıldı, başarı)
cancel_bounty → RefundAttempted → (check_status) → (kaldırıldı)
```

### 5.3 Curator Deposit

Consideration trait kullanır (lib.rs:855-856):
```rust
let native_amount = T::BalanceConverter::from_asset_balance(value, asset_kind)?;
let curator_deposit = T::Consideration::new(&curator, native_amount)?;
```

### 5.4 fund_child_bounty

Parent bounty değerinden düşülerek tracking yapılır (lib.rs:695-698):
```rust
let child_bounties_value = ChildBountiesValuePerParent::<T, I>::get(parent_bounty_id);
let remaining_parent_value = parent_value.saturating_sub(child_bounties_value);
ensure!(remaining_parent_value >= value, Error::<T, I>::InsufficientBountyValue);
```

---

## Kritik Bulgular Özeti

| # | Bulgu | Kaynak |
|---|---|---|
| 1 | **Burn compound formülü:** `burned = budget * (1 - (1-Burn)^N)`, N dönem geçince | treasury/lib.rs:967-971 |
| 2 | **Burn sadece missed_any==false ise çalışır** — herhangi harcama başarısız olursa yanma yok | treasury/lib.rs:964 |
| 3 | **pot() = free_balance - ED** — Existential deposit treasury'e dahil değil | treasury/lib.rs:999-1003 |
| 4 | **spend_local ve tüm eski proposal API DEPRECATED** — Mayıs 2025'te kaldırılacak | treasury/lib.rs:524-525 |
| 5 | **Curator deposit formülü:** `clamp(CuratorDepositMultiplier * fee, Min, Max)` | bounties/lib.rs:1048-1060 |
| 6 | **Bounty deposit formülü:** `BountyDepositBase + DataDepositPerByte * desc_len` | bounties/lib.rs:1116-1119 |
| 7 | **claim_bounty'de parent curator net ücreti:** `fee - sum(child_curator_fees)` | bounties/lib.rs:759-762 |
| 8 | **Tip median:** `tips[tips.len() / 2]` (sıralı dizi, alt-medyan) | tips/lib.rs:578 |
| 9 | **Tip threshold:** `ceil(tippers_count / 2)` — `div_ceil(2)` ile hesaplanır | tips/lib.rs:532 |
| 10 | **Finder ücreti:** `TipFindersFee * median_tip`, finder == who ise ödenmez | tips/lib.rs:589-594 |
| 11 | **Child bounty fund transferi:** parent bounty hesabından child hesabına direkt transfer | child-bounties/lib.rs:320 |
| 12 | **Parent curator = child curator ise sıfır ek deposit** | child-bounties/lib.rs:845-847 |
| 13 | **close_child_bounty:** fonlar parent bounty'e iade edilir | child-bounties/lib.rs:940-945 |
| 14 | **PendingPayout state'de close_child_bounty başarısız:** önce unassign gerekir | child-bounties/lib.rs:915-921 |
| 15 | **multi-asset-bounties:** pallet-bounties'in evolved versiyonu, PayWithSource + Consideration ile | multi-asset-bounties/lib.rs:342-370 |

---

## Nova Shield Chain Notları

### Treasury Tasarımı

1. **Spend API:** Yeni `spend` + `Paymaster` API'sini kullan, `spend_local` deprecated. Bu mimari multi-asset treasury'e geçişi kolaylaştırır.

2. **Burn Mekanizması:** Compound burn mantığı (üstel azalma) zekicedir. Nova Shield'da kullanmak istersen: `missed_any` flag'ini dikkate al. Alternatif: sabit yüzde yakma, her dönem.

3. **SpendPeriod:** Polkadot'ta 24 gün (14.400 blok). Nova Shield için çok daha kısa (örn. 7 gün) veya on-demand spend düşünülebilir.

4. **BlockNumberProvider:** Parachain olacaksa relay chain block numarasını kullanmak için bu trait kritik.

5. **Finansman kaynakları için OnUnbalanced:** Transaction fee'lerinin bir kısmını treasury'e yönlendirmek için `ToAuthor + ToTreasury` split pattern kullan.

### Bounty Tasarımı

6. **CuratorDepositMultiplier:** Polkadot'ta %50 (fee'nin yarısı). Bu değer curator'ın ciddiyetini güvence altına alır. Nova Shield için %20-50 arası uygun.

7. **BountyUpdatePeriod:** Curator'ın düzenli güncelleme yapmasını zorunlu kılmak için kritik. Aksi takdirde pasif curator sorunu oluşur.

8. **pallet-multi-asset-bounties:** Eğer multi-asset treasury yapacaksan, eski `pallet-bounties` yerine bu paleti incele. PayWithSource trait daha esnek.

9. **Child Bounty değer tracking:** `ChildrenCuratorFees` storage item ile child curator ücretleri parent'tan düşülür — bu mantık Nova Shield DEX bounty programı için kullanılabilir.

10. **TransferAllAssets trait:** Bounty kapatılırken tüm asset türlerinin treasury'e dönmesini garanti eder. Multi-asset bounty için gerekli.

### Tips Tasarımı

11. **Tips sistemi kullanım senaryosu:** Küçük, anlık ödüller için uygundur. Validator ödülleri, geliştirici katkıları gibi.

12. **Tippers set:** `SortedMembers + ContainsLengthBound` gerektiriyor. Bu kolektif genellikle Council üyeleridir. Nova Shield'da bir komite olabilir.

13. **MaxTipAmount:** Sınır koyulması önemli — aksi takdirde tek tipper çok büyük ödüller verebilir.

14. **Finder deposit:** `TipReportDepositBase > 0` zorunlu (integrity test). Bu spam korumasıdır.

15. **retain_active_tips:** Tippers set değiştiğinde eski oylar otomatik düşer. Bu dinamik yönetim için iyi bir pattern.

---

*Analiz tamamlandı. Tüm kaynak satır numaraları doğrulanmıştır.*
