# Substrate pallet-transaction-payment — Agent 04

## Kapsam

Analiz edilen dizin: `/substrate/frame/transaction-payment/`
Analiz tarihi: 2026-07-22
Toplam dosya sayısı: 29 Rust dosyası (5 alt crate dahil)

### Analiz Edilen Dosyalar (tam liste)

**Ana pallet (`src/`)**
- `src/lib.rs` — 1096 satır (ana pallet mantığı, ChargeTransactionPayment, TargetedFeeAdjustment)
- `src/types.rs` — 177 satır (InclusionFee, FeeDetails, RuntimeDispatchInfo struct'ları)
- `src/payment.rs` — 224 satır (OnChargeTransaction trait, FungibleAdapter impl)
- `src/tests.rs` — 918 satır (kapsamlı test suite)
- `src/mock.rs` — 152 satır (test mock runtime)
- `src/benchmarking.rs` — 120 satır (benchmark suite)
- `src/weights.rs` — 108 satır (ağırlık sabitleri)

**Alt crate'ler**
- `asset-tx-payment/src/lib.rs` — 445 satır (fungibles ile asst ödemesi)
- `asset-tx-payment/src/payment.rs` — 223 satır (FungiblesAdapter, OnChargeAssetTransaction)
- `asset-conversion-tx-payment/src/lib.rs` — 433 satır (SwapCredit ile asset dönüşümü)
- `asset-conversion-tx-payment/src/payment.rs` — 325 satır (SwapAssetAdapter)
- `skip-feeless-payment/src/lib.rs` — 207 satır (SkipCheckIfFeeless wrapper)
- `pgas-allowance/src/lib.rs` — 385 satır (PGAS token ile ödeme)
- `rpc/src/lib.rs` — 177 satır (RPC endpoint'leri)
- `rpc/runtime-api/src/lib.rs` — 57 satır (runtime API trait tanımları)
- (diğer mock, test, weight dosyaları da okundu)

---

## Fee Hesaplama Formülü

### Temel Formül (lib.rs:33-36)

```
inclusion_fee = base_fee + length_fee + [targeted_fee_adjustment * weight_fee]
final_fee = inclusion_fee + tip
```

Bu formül `doc comment`'ten alınmıştır ve tam olarak kaynak kodda uygulanmaktadır.

### Bileşenlerin Tanımı

| Bileşen | Açıklama | Kaynak |
|---|---|---|
| `base_fee` | Tüm tx için sabit minimum ücret. Blockt weight'ten WeightToFee ile dönüştürülür | lib.rs:680 |
| `len_fee` | Tx boyutu (byte cinsinden) için ücret. LengthToFee ile dönüştürülür | lib.rs:678 |
| `weight_fee` | Tx'in ne kadar hesaplama kullandığına dair ücret. WeightToFee ile dönüştürülür | lib.rs:672 |
| `targeted_fee_adjustment` | Ağ tıkanıklığına göre weight fee'yi çarpan multiplier | lib.rs:673 |
| `tip` | Kullanıcı tarafından eklenen opsiyonel tip. Önceliği artırır | lib.rs:826 |

### Kaynak Kod — `compute_fee_raw` Fonksiyonu (lib.rs:663-688)

```rust
fn compute_fee_raw(
    len: u32,
    weight: Weight,
    tip: BalanceOf<T>,
    pays_fee: Pays,
    class: DispatchClass,
) -> FeeDetails<BalanceOf<T>> {
    if pays_fee == Pays::Yes {
        // ağırlık ücretinin ayarlanabilir kısmı
        let unadjusted_weight_fee = Self::weight_to_fee(weight);   // lib.rs:672
        let multiplier = NextFeeMultiplier::<T>::get();             // lib.rs:673
        // nihai ayarlanmış ağırlık ücreti
        let adjusted_weight_fee = multiplier.saturating_mul_int(unadjusted_weight_fee); // lib.rs:675

        // uzunluk ücreti: LengthToFee üzerinden ayarlanır
        let len_fee = Self::length_to_fee(len);                     // lib.rs:678

        let base_fee = Self::weight_to_fee(T::BlockWeights::get().get(class).base_extrinsic); // lib.rs:680
        FeeDetails {
            inclusion_fee: Some(InclusionFee { base_fee, len_fee, adjusted_weight_fee }),
            tip,
        }
    } else {
        FeeDetails { inclusion_fee: None, tip }
    }
}
```

**KRİTİK NOT:** Multiplier sadece `weight_fee`'yi etkiler, `base_fee` ve `len_fee`'yi etkilemez. Bu, test `compute_fee_works_with_multiplier`'da (tests.rs:429-460) açıkça doğrulanmıştır:
```
// Base fee is unaffected by multiplier
assert_eq!(Pallet::<Runtime>::compute_fee(0, &dispatch_info, 0), 100);
```

### `InclusionFee::inclusion_fee()` Toplamı (types.rs:55-59)

```rust
pub fn inclusion_fee(&self) -> Balance {
    self.base_fee
        .saturating_add(self.len_fee)
        .saturating_add(self.adjusted_weight_fee)
}
```

### `FeeDetails::final_fee()` Toplamı (types.rs:83-89)

```rust
pub fn final_fee(&self) -> Balance {
    self.inclusion_fee
        .as_ref()
        .map(|i| i.inclusion_fee())
        .unwrap_or_else(|| Zero::zero())
        .saturating_add(self.tip)
}
```

### Pays::No Durumu

`pays_fee == Pays::No` olan tx'lerde (Pays::Yes olmayan) `inclusion_fee = None` döner. Ancak `tip` hala ödenir. Bu kritik bir tasarım kararıdır:
- Görev tx'leri (`Mandatory`) genellikle `Pays::No`
- Kullanıcı runtime tarafından da `Pays::No` döndürebilir (post_dispatch'te)

### Gerçek (Actual) vs Tahmini Fee

Tx öncesinde tahmini fee hesaplanır (`compute_fee`), tx çalıştıktan sonra gerçek fee `compute_actual_fee` ile yeniden hesaplanır:

```rust
pub fn compute_actual_fee(
    len: u32,
    info: &DispatchInfoOf<T::RuntimeCall>,
    post_info: &PostDispatchInfoOf<T::RuntimeCall>,
    tip: BalanceOf<T>,
) -> BalanceOf<T> {
    Self::compute_actual_fee_details(len, info, post_info, tip).final_fee()
}
```

`compute_actual_fee_details` şunu kullanır:
```rust
Self::compute_fee_raw(
    len,
    post_info.calc_actual_weight(info),  // gerçek kullanılan weight
    tip,
    post_info.pays_fee(info),
    info.class,
)
```

Bu, tx'in beklenen ağırlıktan daha az kullanması durumunda fazla ödenen fee'nin iade edileceği anlamına gelir.

---

## Multiplier Mekanizması

### Temel Veri Tipi (lib.rs:90)

```rust
pub type Multiplier = FixedU128;
```

`FixedU128` — 18 ondalık basamaklı sabit noktalı sayı. Başlangıç değeri 1.0.

### Depolama (lib.rs:412-415)

```rust
#[pallet::storage]
#[pallet::whitelist_storage]  // her blokta okunacak, okuma performansı için whitelist
pub type NextFeeMultiplier<T: Config> =
    StorageValue<_, Multiplier, ValueQuery, NextFeeMultiplierOnEmpty>;
```

Varsayılan değer:
```rust
const MULTIPLIER_DEFAULT_VALUE: Multiplier = Multiplier::from_u32(1);
```

### Güncelleme Zamanı (lib.rs:458-471)

```rust
fn on_finalize(_: frame_system::pallet_prelude::BlockNumberFor<T>) {
    NextFeeMultiplier::<T>::mutate(|fm| {
        *fm = T::FeeMultiplierUpdate::convert(*fm);
    });
    // Artık TxPaymentCredit varsa logla ve temizle
    TxPaymentCredit::<T>::take().map(|credit| {
        log::error!(target: LOG_TARGET, "The `TxPaymentCredit` was stored between blocks. This is a bug.");
        credit.into_inner()
    });
}
```

Multiplier her bloğun `on_finalize`'ında güncellenir.

### TargetedFeeAdjustment — Multiplier Güncelleme Formülü

**Formal Formül Dokümantasyonu (lib.rs:103-116):**

```
given:
    s  = previous block weight (önceki blok ağırlığı)
    s' = ideal block weight (hedef doluluk oranı)
    m  = maximum block weight (maksimum blok ağırlığı)

    diff = (s - s') / m
    v    = 0.00001  (variability factor / değişkenlik faktörü)
    t1   = (v * diff)
    t2   = (v * diff)^2 / 2

then:
    next_multiplier = prev_multiplier * (1 + t1 + t2)
```

**Kaynak Kod Uygulaması (lib.rs:209-273):**

```rust
fn convert(previous: Multiplier) -> Multiplier {
    let min_multiplier = M::get();
    let max_multiplier = X::get();
    let previous = previous.max(min_multiplier);

    // Sadece Normal class tx'leri için hesaplanır
    let weights = T::BlockWeights::get();
    let normal_max_weight =
        weights.get(DispatchClass::Normal).max_total.unwrap_or(weights.max_block);
    let current_block_weight = frame_system::Pallet::<T>::block_weight();
    let normal_block_weight =
        current_block_weight.get(DispatchClass::Normal).min(normal_max_weight);

    // Çok boyutlu weight: ref_time ve proof_size normalize edilir
    let normalized_ref_time = Perbill::from_rational(
        normal_block_weight.ref_time(),
        normal_max_weight.ref_time().max(1),
    );
    let normalized_proof_size = Perbill::from_rational(
        normal_block_weight.proof_size(),
        normal_max_weight.proof_size().max(1),
    );

    // Kısıtlayıcı boyutu seç (limiting dimension)
    let (normal_limiting_dimension, max_limiting_dimension) =
        if normalized_ref_time < normalized_proof_size {
            (normal_block_weight.proof_size(), normal_max_weight.proof_size())
        } else {
            (normal_block_weight.ref_time(), normal_max_weight.ref_time())
        };

    let target_block_fullness = S::get();   // hedef doluluk oranı (s')
    let adjustment_variable = V::get();     // değişkenlik faktörü (v)

    let target_weight = (target_block_fullness * max_limiting_dimension) as u128;
    let block_weight = normal_limiting_dimension as u128;

    // diff = (s - s') / m işareti belirle
    let positive = block_weight >= target_weight;
    let diff_abs = block_weight.max(target_weight) - block_weight.min(target_weight);

    // diff = |s - s'| / m
    let diff = Multiplier::saturating_from_rational(diff_abs, max_limiting_dimension.max(1));
    let diff_squared = diff.saturating_mul(diff);

    // t2 = v^2 * diff^2 / 2
    let v_squared_2 = adjustment_variable.saturating_mul(adjustment_variable) /
        Multiplier::saturating_from_integer(2);

    // t1 = v * diff
    let first_term = adjustment_variable.saturating_mul(diff);
    // t2 = v^2/2 * diff^2
    let second_term = v_squared_2.saturating_mul(diff_squared);

    if positive {
        // blok hedefin üzerinde → fee artır
        let excess = first_term.saturating_add(second_term).saturating_mul(previous);
        previous.saturating_add(excess).clamp(min_multiplier, max_multiplier)
    } else {
        // blok hedefin altında → fee azalt
        // Defensive: first_term > second_term garantisi var
        let negative = first_term.saturating_sub(second_term).saturating_mul(previous);
        previous.saturating_sub(negative).clamp(min_multiplier, max_multiplier)
    }
}
```

### Limiting Dimension Kavramı

Polkadot'ta Weight iki boyutludur: `ref_time` (hesaplama süresi) ve `proof_size` (Merkle proof büyüklüğü). Multiplier hesaplanırken her ikisi normalize edilir ve **daha kısıtlayıcı olan** kullanılır. Bu, her iki kaynağın da dengelenmesini sağlar.

### TargetedFeeAdjustment Struct'ı (lib.rs:150)

```rust
pub struct TargetedFeeAdjustment<T, S, V, M, X>(core::marker::PhantomData<(T, S, V, M, X)>);
```

Tip parametreleri:
- `T`: Runtime konfigürasyonu
- `S: Get<Perquintill>`: Hedef doluluk oranı (`s'`)
- `V: Get<Multiplier>`: Değişkenlik faktörü (`v`)
- `M: Get<Multiplier>`: Minimum multiplier değeri
- `X: Get<Multiplier>`: Maksimum multiplier değeri

### MultiplierUpdate Trait'i (lib.rs:153-177)

```rust
pub trait MultiplierUpdate: Convert<Multiplier, Multiplier> {
    fn min() -> Multiplier;
    fn max() -> Multiplier;
    fn target() -> Perquintill;
    fn variability() -> Multiplier;
}
```

### Değişim Oranı Analizi (lib.rs:136-147)

Web3 araştırmasına göre, tam kongestion durumunda (28800 blok/gün, v=0.00001, s'=0.1875):
```
p >= 0.00001 * 28800 * 0.8125
p >= 0.234   (~%23 artış/gün)
```

### ConstFeeMultiplier (lib.rs:277-300)

Sabit multiplier implementasyonu. Parametre ne olursa olsun her zaman `M::get()` döner:

```rust
pub struct ConstFeeMultiplier<M: Get<Multiplier>>(core::marker::PhantomData<M>);

impl<M> Convert<Multiplier, Multiplier> for ConstFeeMultiplier<M>
where
    M: Get<Multiplier>,
{
    fn convert(_previous: Multiplier) -> Multiplier {
        Self::min()  // sabit, dinamik ayar yok
    }
}
```

---

## WeightToFee ve LengthToFee Trait'leri

### Config Trait Tanımı (lib.rs:369-374)

```rust
/// Convert a weight value into a deductible fee based on the currency type.
type WeightToFee: WeightToFee<Balance = BalanceOf<Self>>;

/// Convert a length value into a deductible fee based on the currency type.
type LengthToFee: WeightToFee<Balance = BalanceOf<Self>>;
```

**İlginç Tasarım:** `LengthToFee` da `WeightToFee` trait'ini kullanır. Byte sayısı `Weight::from_parts(length as u64, 0)` olarak wrap edilir:

```rust
pub fn length_to_fee(length: u32) -> BalanceOf<T> {
    T::LengthToFee::weight_to_fee(&Weight::from_parts(length as u64, 0))
}
```

### frame_support'daki WeightToFee Trait'i

```rust
pub trait WeightToFee {
    type Balance: Balance;
    fn weight_to_fee(weight: &Weight) -> Self::Balance;
}
```

### Yaygın Implementasyonlar (frame_support'dan)

**IdentityFee** — 1 weight unit = 1 balance unit:
```rust
pub struct IdentityFee<T>(sp_std::marker::PhantomData<T>);
impl<T: Balance> WeightToFee for IdentityFee<T> {
    type Balance = T;
    fn weight_to_fee(weight: &Weight) -> Self::Balance {
        weight.ref_time().saturated_into()
    }
}
```

**ConstantMultiplier** — Her weight unit için sabit çarpan:
```rust
pub struct ConstantMultiplier<T, M>(sp_std::marker::PhantomData<(T, M)>);
impl<T: Balance, M: Get<T>> WeightToFee for ConstantMultiplier<T, M> {
    type Balance = T;
    fn weight_to_fee(weight: &Weight) -> Self::Balance {
        weight.ref_time().saturated_into::<T>().saturating_mul(M::get())
    }
}
```

### Mock Uygulaması (mock.rs:81-97)

```rust
impl WeightToFeeT for WeightToFee {
    type Balance = u64;
    fn weight_to_fee(weight: &Weight) -> Self::Balance {
        Self::Balance::saturated_from(weight.ref_time())
            .saturating_mul(WEIGHT_TO_FEE.with(|v| *v.borrow()))
    }
}

impl WeightToFeeT for TransactionByteFee {
    type Balance = u64;
    fn weight_to_fee(weight: &Weight) -> Self::Balance {
        Self::Balance::saturated_from(weight.ref_time())
            .saturating_mul(TRANSACTION_BYTE_FEE.with(|v| *v.borrow()))
    }
}
```

### weight_to_fee Koruma Mekanizması (lib.rs:697-702)

```rust
pub fn weight_to_fee(weight: Weight) -> BalanceOf<T> {
    // max_block ile kap, overflow'u önle
    let capped_weight = weight.min(T::BlockWeights::get().max_block);
    T::WeightToFee::weight_to_fee(&capped_weight)
}
```

Bu, tx'in aşırı ağırlık iddia etmesi durumunda bile maksimum blok ağırlığı sınırıyla koruma sağlar.

---

## ChargeTransactionPayment — TransactionExtension Akışı

### Struct Tanımı (lib.rs:794-796)

```rust
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
#[scale_info(skip_type_params(T))]
pub struct ChargeTransactionPayment<T: Config>(#[codec(compact)] BalanceOf<T>);
```

İçerdiği tek alan: `tip` miktarı (compact encode edilmiş).

### Val ve Pre Enum'ları (lib.rs:946-990)

```rust
pub enum Val<T: Config> {
    Charge {
        tip: BalanceOf<T>,
        who: T::AccountId,
        fee_with_tip: BalanceOf<T>,
    },
    NoCharge,
}

pub enum Pre<T: Config> {
    Charge {
        tip: BalanceOf<T>,
        who: T::AccountId,
        liquidity_info: <<T as Config>::OnChargeTransaction as OnChargeTransaction<T>>::LiquidityInfo,
    },
    NoCharge {
        refund: Weight,  // iade edilecek weight
    },
}
```

### TransactionExtension Implementasyonu (lib.rs:992-1083)

```rust
impl<T: Config> TransactionExtension<T::RuntimeCall> for ChargeTransactionPayment<T>
where
    T::RuntimeCall: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
{
    const IDENTIFIER: &'static str = "ChargeTransactionPayment";
    type Implicit = ();
    type Val = Val<T>;
    type Pre = Pre<T>;

    fn weight(&self, _: &T::RuntimeCall) -> Weight {
        T::WeightInfo::charge_transaction_payment()
    }
    // ...
}
```

### validate Adımı (lib.rs:1005-1031)

```rust
fn validate(
    &self,
    origin: <T::RuntimeCall as Dispatchable>::RuntimeOrigin,
    call: &T::RuntimeCall,
    info: &DispatchInfoOf<T::RuntimeCall>,
    len: usize,
    _: (),
    _implication: &impl Encode,
    _source: TransactionSource,
) -> Result<
    (ValidTransaction, Self::Val, <T::RuntimeCall as Dispatchable>::RuntimeOrigin),
    TransactionValidityError,
> {
    // Signed origin kontrolü: imzasız tx'lerde ücret yok
    let Ok(who) = frame_system::ensure_signed(origin.clone()) else {
        return Ok((ValidTransaction::default(), Val::NoCharge, origin));
    };

    // Ücretin çekilebileceğini doğrula (DRY-RUN, gerçek çekim yok)
    let fee_with_tip = self.can_withdraw_fee(&who, call, info, len)?;
    let tip = self.0;

    Ok((
        ValidTransaction {
            priority: Self::get_priority(info, len, tip, fee_with_tip),
            ..Default::default()
        },
        Val::Charge { tip: self.0, who, fee_with_tip },
        origin,
    ))
}
```

**Önemli:** `validate` sadece ücretin çekilebileceğini kontrol eder, gerçek çekim yapmaz.

### prepare Adımı (lib.rs:1033-1050)

```rust
fn prepare(
    self,
    val: Self::Val,
    _origin: &<T::RuntimeCall as Dispatchable>::RuntimeOrigin,
    call: &T::RuntimeCall,
    info: &DispatchInfoOf<T::RuntimeCall>,
    _len: usize,
) -> Result<Self::Pre, TransactionValidityError> {
    match val {
        Val::Charge { tip, who, fee_with_tip } => {
            // GERÇEK ücret çekimi burada yapılır
            let (_fee_with_tip, liquidity_info) =
                self.withdraw_fee(&who, call, info, fee_with_tip)?;
            Ok(Pre::Charge { tip, who, liquidity_info })
        },
        Val::NoCharge => Ok(Pre::NoCharge { refund: self.weight(call) }),
    }
}
```

**Önemli:** Gerçek ücret çekimi `prepare`'de yapılır, `validate`'de değil. Bu, tx havuzundaki çakışmalar için önemli bir ayrımdır.

### post_dispatch_details Adımı (lib.rs:1052-1083)

```rust
fn post_dispatch_details(
    pre: Self::Pre,
    info: &DispatchInfoOf<T::RuntimeCall>,
    post_info: &PostDispatchInfoOf<T::RuntimeCall>,
    len: usize,
    _result: &DispatchResult,
) -> Result<Weight, TransactionValidityError> {
    let (tip, who, liquidity_info) = match pre {
        Pre::Charge { tip, who, liquidity_info } => (tip, who, liquidity_info),
        Pre::NoCharge { refund } => {
            return Ok(refund);  // Tüm weight'i iade et
        },
    };

    // Gerçek ücreti hesapla (post_info'daki gerçek weight kullanılarak)
    let actual_fee_with_tip =
        Pallet::<T>::compute_actual_fee(len as u32, info, &post_info, tip);

    // Fazla çekilen ücret iade et, kalan depozite et
    T::OnChargeTransaction::correct_and_deposit_fee(
        &who,
        info,
        &post_info,
        actual_fee_with_tip,
        tip,
        liquidity_info,
    )?;

    // TransactionFeePaid event'i yayınla
    Pallet::<T>::deposit_event(Event::<T>::TransactionFeePaid {
        who,
        actual_fee: actual_fee_with_tip,
        tip,
    });

    Ok(Weight::zero())
}
```

### Üç Adım Özeti

| Adım | Ne Yapılır | Sonuç |
|------|-----------|-------|
| `validate` | Ücret tahmini, çekme kapasitesi kontrolü, priority hesabı | `Val::Charge { fee_with_tip }` |
| `prepare` | Gerçek ücret çekimi, liquidity_info döndürülür | `Pre::Charge { liquidity_info }` |
| `post_dispatch` | Gerçek fee hesabı, fazla iade, event | `Weight::zero()` |

---

## Tip Sistemi

### Tip Nedir?

Tip, kullanıcının isteğe bağlı olarak ödediği ek ücrettir. Blok üreticisi tarafından doğrudan alınır (standart konfigürasyonda). Formüle göre:

```
final_fee = inclusion_fee + tip
```

### Tip ile İşlemler

**withdraw_fee fonksiyonu (lib.rs:813-836):**
```rust
fn withdraw_fee(
    &self,
    who: &T::AccountId,
    call: &T::RuntimeCall,
    info: &DispatchInfoOf<T::RuntimeCall>,
    fee_with_tip: BalanceOf<T>,
) -> Result<
    (BalanceOf<T>, <<T as Config>::OnChargeTransaction as OnChargeTransaction<T>>::LiquidityInfo),
    TransactionValidityError,
> {
    let tip = self.0;  // struct'tan tip değerini al
    <<T as Config>::OnChargeTransaction as OnChargeTransaction<T>>::withdraw_fee(
        who, call, info, fee_with_tip, tip,  // fee_with_tip = fee + tip
    )
    .map(|liquidity_info| (fee_with_tip, liquidity_info))
}
```

### FungibleAdapter'da Tip Ayrımı (payment.rs:141-145)

```rust
// fee_with_tip'i önce tamamen çek
let credit = F::withdraw(
    who,
    fee_with_tip,
    Precision::Exact,
    Preservation::Preserve,
    Fortitude::Polite,
)
.map_err(|_| InvalidTransaction::Payment)?;

// Tip'i ayır ve ayrıca tut
let (tip_credit, inclusion_fee) = credit.split(tip);

// Inclusion fee'yi TxPaymentCredit storage'a koy
<Pallet<T>>::deposit_txfee(inclusion_fee);

// Tip credit'ini döndür (LiquidityInfo olarak)
Ok(Some(tip_credit))
```

### correct_and_deposit_fee'de Tip Dağılımı (payment.rs:165-204)

```rust
fn correct_and_deposit_fee(
    who: &<T>::AccountId,
    _dispatch_info: &DispatchInfoOf<<T>::RuntimeCall>,
    _post_info: &PostDispatchInfoOf<<T>::RuntimeCall>,
    corrected_fee_with_tip: Self::Balance,
    tip: Self::Balance,
    tip_credit: Self::LiquidityInfo,  // LiquidityInfo = tip credit
) -> Result<(), TransactionValidityError> {
    let corrected_fee = corrected_fee_with_tip.saturating_sub(tip);

    // TxPaymentCredit storage'dan inclusion fee'yi al
    let remaining_credit = <TxPaymentCredit<T>>::take()
        .map(|stored_credit| stored_credit.into_inner())
        .unwrap_or_default();

    // Account kill durumunu kontrol et
    let fee_credit = if frame_system::Pallet::<T>::account_exists(who) {
        let (mut fee_credit, refund_credit) = remaining_credit.split(corrected_fee);
        // Fazlayı iade et
        if !refund_credit.peek().is_zero() {
            if let Err(not_refunded) = F::resolve(who, refund_credit) {
                fee_credit.subsume(not_refunded);  // iade başarısız → fee'ye ekle
            }
        }
        fee_credit
    } else {
        remaining_credit  // hesap öldüyse tüm kalan fee alınır
    };

    // Tip ve fee ayrı ayrı OnUnbalanced handler'a gönderilir
    OU::on_unbalanceds(Some(fee_credit).into_iter().chain(tip_credit));
    Ok(())
}
```

**KRİTİK:** Tip ve fee `on_unbalanceds` iterator olarak ayrı gönderilir. Bu, `DealWithFees::on_unbalanceds` implementasyonunda görülür:
- `fees_then_tips.next()` → fee
- `fees_then_tips.next()` → tip

### OnUnbalanced Mock Uygulaması (mock.rs:104-120)

```rust
pub struct DealWithFees;
impl OnUnbalanced<fungible::Credit<...>> for DealWithFees {
    fn on_unbalanceds(
        mut fees_then_tips: impl Iterator<
            Item = fungible::Credit<..>,
        >,
    ) {
        if let Some(fees) = fees_then_tips.next() {
            FeeUnbalancedAmount::mutate(|a| *a += fees.peek());
            if let Some(tips) = fees_then_tips.next() {
                TipUnbalancedAmount::mutate(|a| *a += tips.peek());
            }
        }
    }
}
```

### Tip ve Priority İlişkisi

Tip, öncelik hesaplamasında da kullanılır. Sıfır tip bile 1 artırılarak some priority verilir:
```rust
let tip = tip.saturating_add(One::one());  // lib.rs:904
```

### Tip Validator'a mı Gidiyor?

Bu pallet, tip'in nereye gideceğini doğrudan belirlemez. `OnUnbalanced` trait'i aracılığıyla runtime konfigürasyonuna bırakır. Standart Substrate node'larda:
- Fee: `Treasury` + `Author` arasında paylaşılır
- Tip: Genellikle doğrudan blok üreticisine (author'a) gider

---

## Priority Sistemi

### get_priority Fonksiyonu (lib.rs:871-931)

```rust
pub fn get_priority(
    info: &DispatchInfoOf<T::RuntimeCall>,
    len: usize,
    tip: BalanceOf<T>,
    final_fee_with_tip: BalanceOf<T>,
) -> TransactionPriority {
    let max_block_weight = T::BlockWeights::get().max_block;
    let max_block_length = *T::BlockLength::get().max.get(info.class) as u64;

    // Sınırlandırılmış değerler (0 bölmesini önle)
    let bounded_weight =
        info.total_weight().max(Weight::from_parts(1, 1)).min(max_block_weight);
    let bounded_length = (len as u64).clamp(1, max_block_length);

    // Limiting resource hesabı (weight vs length)
    let max_tx_per_block_weight = max_block_weight
        .checked_div_per_component(&bounded_weight)
        .unwrap_or(1);
    let max_tx_per_block_length = max_block_length / bounded_length;

    let max_tx_per_block = max_tx_per_block_length
        .min(max_tx_per_block_weight)
        .saturated_into::<BalanceOf<T>>();
    let max_reward = |val: BalanceOf<T>| val.saturating_mul(max_tx_per_block);

    // No-tip tx'lerin dağılımı için 1 ekle
    let tip = tip.saturating_add(One::one());
    let scaled_tip = max_reward(tip);

    match info.class {
        DispatchClass::Normal | DispatchClass::Mandatory => scaled_tip,
        DispatchClass::Operational => {
            // Operational tx'ler için "virtual tip" ekle
            let fee_multiplier = T::OperationalFeeMultiplier::get().saturated_into();
            let virtual_tip = final_fee_with_tip.saturating_mul(fee_multiplier);
            let scaled_virtual_tip = max_reward(virtual_tip);
            scaled_tip.saturating_add(scaled_virtual_tip)
        },
    }
    .saturated_into::<TransactionPriority>()
}
```

### OperationalFeeMultiplier (lib.rs:379-401)

```rust
/// A fee multiplier for `Operational` extrinsics to compute "virtual tip" to boost their priority
///
/// For `Normal`:
///     let priority = priority_calc(tip);
///
/// For `Operational`:
///     let virtual_tip = (inclusion_fee + tip) * OperationalFeeMultiplier;
///     let priority = priority_calc(tip + virtual_tip);
#[pallet::constant]
type OperationalFeeMultiplier: Get<u8>;
```

Mock'ta: `pub static OperationalFeeMultiplier: u8 = 5;` (mock.rs:66)

### Priority Test Kanıtı (tests.rs:651-705)

```
Normal tx (tip=5, weight=100, len=10):  priority = 60
Normal tx (tip=10, weight=100, len=10): priority = 110

Operational tx (tip=5, weight=100, len=10):  priority = 5810
Operational tx (tip=10, weight=100, len=10): priority = 6110
```

Operational tx'lerin çok daha yüksek önceliğe sahip olduğu görülür (~97x).

---

## OnChargeTransaction Trait Sistemi

### Trait Tanımı (payment.rs:38-84)

```rust
pub trait OnChargeTransaction<T: Config>: TxCreditHold<T> {
    type Balance: frame_support::traits::tokens::Balance;
    type LiquidityInfo: Default;

    fn withdraw_fee(
        who: &T::AccountId,
        call: &T::RuntimeCall,
        dispatch_info: &DispatchInfoOf<T::RuntimeCall>,
        fee_with_tip: Self::Balance,
        tip: Self::Balance,
    ) -> Result<Self::LiquidityInfo, TransactionValidityError>;

    fn can_withdraw_fee(
        who: &T::AccountId,
        call: &T::RuntimeCall,
        dispatch_info: &DispatchInfoOf<T::RuntimeCall>,
        fee_with_tip: Self::Balance,
        tip: Self::Balance,
    ) -> Result<(), TransactionValidityError>;

    fn correct_and_deposit_fee(
        who: &T::AccountId,
        dispatch_info: &DispatchInfoOf<T::RuntimeCall>,
        post_info: &PostDispatchInfoOf<T::RuntimeCall>,
        corrected_fee_with_tip: Self::Balance,
        tip: Self::Balance,
        liquidity_info: Self::LiquidityInfo,
    ) -> Result<(), TransactionValidityError>;

    // benchmark helpers...
}
```

### TxCreditHold Trait'i (payment.rs:90-101)

```rust
pub trait TxCreditHold<T: Config> {
    type Credit: FullCodec + DecodeWithMemTracking + MaxEncodedLen + TypeInfo + SuppressedDrop;
}
```

Bu trait ayrı tutulmuştur çünkü doğrudan `OnChargeTransaction`'a eklenmesi `trait resolution` döngüsüne yol açar.

### FungibleAdapter (payment.rs:109-223)

Standart implementasyon. `pallet_balances` gibi `fungible::Balanced` implementasyonu olan herhangi bir token ile çalışır:

```rust
pub struct FungibleAdapter<F, OU>(PhantomData<(F, OU)>);
```

- `F: Balanced<T::AccountId>`: Token implementasyonu
- `OU: OnUnbalanced<...>`: Fee'nin nereye gideceğini belirler

### TxPaymentCredit Storage (lib.rs:421-425)

```rust
#[pallet::storage]
#[pallet::whitelist_storage]
pub(crate) type TxPaymentCredit<T: Config> = StorageValue<_, StoredCreditOf<T>>;
```

Bu storage, tx çalışırken diğer pallet'lerin fee credit'ine erişmesine olanak tanır:
- `withdraw_txfee(amount)`: Fee credit'inden miktar çek
- `deposit_txfee(credit)`: Fee credit'ine miktar ekle
- `remaining_txfee()`: Kalan fee miktarını sorgula

---

## Kritik Formüller

### 1. Inclusion Fee

```
inclusion_fee = base_fee + len_fee + adjusted_weight_fee

base_fee          = WeightToFee(base_extrinsic_weight)
len_fee           = LengthToFee(tx_byte_length)
adjusted_weight_fee = NextFeeMultiplier * WeightToFee(actual_weight)
```

### 2. Final Fee

```
final_fee = inclusion_fee + tip
```

### 3. Multiplier Güncelleme (Taylor açılımı)

```
diff     = (block_weight - target_weight) / max_weight
positive = block_weight >= target_weight

if positive:
    next_multiplier = prev * (1 + v*diff + v^2*diff^2/2)
else:
    next_multiplier = prev * (1 - (v*diff - v^2*diff^2/2))

next_multiplier = clamp(next_multiplier, min, max)
```

### 4. Priority (Normal)

```
scaled_tip = (tip + 1) * min(max_block_weight/tx_weight, max_block_length/tx_length)
priority   = scaled_tip
```

### 5. Priority (Operational)

```
virtual_tip         = final_fee * OperationalFeeMultiplier
scaled_virtual_tip  = virtual_tip * min(max_block_weight/tx_weight, max_block_length/tx_length)
priority            = scaled_tip + scaled_virtual_tip
```

### 6. Refund Mekanizması

```
refund = pre_dispatch_fee - actual_post_dispatch_fee
       = (base + len + multiplier*estimated_weight + tip)
         - (base + len + multiplier*actual_weight + tip)
       = multiplier * (estimated_weight - actual_weight)
```

### 7. Asset Fee (FungiblesAdapter)

```
min_converted_fee = if fee == 0 { 0 } else { 1 }
converted_fee     = max(ConversionToAssetBalance(fee, asset_id), min_converted_fee)
```

### 8. SwapAssetAdapter (Dönüşüm + Swap)

```
// Ödeme sırasında:
asset_fee = quote_price_tokens_for_exact_tokens(asset_id, native, fee, include_fee)
// asset_id tipinde ödeme, native tipinde tam fee elde edilir
swap(asset_id → native, amount=asset_fee) → native_fee
```

---

## Pays Fee Durumu (Pays::Yes / Pays::No)

`Pays::Yes` → inclusion_fee hesaplanır ve alınır
`Pays::No`  → inclusion_fee alınmaz, ama tip alınır

Bu iki seviyede belirlenebilir:
1. `DispatchInfo.pays_fee` (önceden bildirilen)
2. `PostDispatchInfo.pays_fee` (tx sonrası override)

Test kanıtı (tests.rs:796-823):
```rust
fn post_info_can_change_pays_fee() {
    // ...
    let post_info = post_info_from_pays(Pays::No);
    // ...
    // Only 5 tip is paid
    assert_eq!(actual_fee, 5);
}
```

---

## Overflow Koruması

Tüm aritmetik işlemler `saturating_*` kullanır. Test (tests.rs:498-517):

```rust
fn compute_fee_does_not_overflow() {
    let dispatch_info = DispatchInfo {
        call_weight: Weight::MAX,
        // ...
    };
    assert_eq!(
        Pallet::<Runtime>::compute_fee(u32::MAX, &dispatch_info, u64::MAX),
        u64::MAX  // saturate eder, panic olmaz
    );
}
```

Weight de capped'dir:
```rust
let capped_weight = weight.min(T::BlockWeights::get().max_block);
```

---

## Alt Crate'ler Detayı

### 1. asset-tx-payment — FungiblesAdapter

Fungible asset'ler ile ödeme. Native token yerine herhangi bir asset kullanılabilir:

```rust
pub struct ChargeAssetTxPayment<T: Config> {
    tip: BalanceOf<T>,
    asset_id: Option<ChargeAssetIdOf<T>>,  // None → native token
}
```

`asset_id = None` ise normal `OnChargeTransaction` yoluna düşer.
`asset_id = Some(id)` ise `OnChargeAssetTransaction::withdraw_fee` çağrılır.

**FungiblesAdapter.withdraw_fee:**
```rust
// Minimum fee garanti (1/asset biriminden daha küçük dönüşümleri önle)
let min_converted_fee = if fee.is_zero() { Zero::zero() } else { One::one() };
let converted_fee = CON::to_asset_balance(fee, asset_id.clone())
    .map_err(|_| TransactionValidityError::from(InvalidTransaction::Payment))?
    .max(min_converted_fee);
```

### 2. asset-conversion-tx-payment — SwapAssetAdapter

En karmaşık alt crate. Asset Conversion pallet'i (AMM) kullanarak ödeme:

```rust
// Quote: ne kadar asset_id gerekecek?
let asset_fee =
    S::quote_price_tokens_for_exact_tokens(asset_id.clone(), A::get(), fee, true)
        .filter(|asset_fee| !asset_fee.is_zero())
        .ok_or(InvalidTransaction::Payment)?;

// Gerçek swap yap
let (fee_credit, change) = match S::swap_tokens_for_exact_tokens(
    vec![asset_id, A::get()],
    asset_fee_credit,
    fee,
) {...}

// Kalan sıfır olmalı (exact quote'dan)
ensure!(change.peek().is_zero(), InvalidTransaction::Payment);
```

Post-dispatch'te refund işlemi:
1. Eğer `asset_id == A::get()` (native) → direkt split ve resolve
2. Aksi halde → native'den asset'e geri swap dene, başarısız olursa refund verilmez

### 3. skip-feeless-payment — SkipCheckIfFeeless

```rust
impl<T, S> TransactionExtension<T::RuntimeCall> for SkipCheckIfFeeless<T, S>
where
    T::RuntimeCall: CheckIfFeeless<Origin = ...>,
{
    fn validate(...) {
        if call.is_feeless(&origin) {
            // Ücretsiz dispatchable → direkt skip
            Ok((Default::default(), Skip(origin.caller().clone()), origin))
        } else {
            // Normal → wrapped extension'a delegate et
            let (x, y, z) = self.0.validate(...)?;
            Ok((x, Apply(y), z))
        }
    }
}
```

`FeeSkipped { origin }` event'i yayınlar.

### 4. pgas-allowance — ChargePGAS

PGAS (Polkadot GAS) adında özel bir asset ile ödeme. Fee burn edilir:

```rust
// Tx çalıştıktan sonra:
let (consumed, fee_refund) = credit.split(actual_fee);
// consumed → DROP → burn (OnDropCredit)
// fee_refund → who'ya iade

let burned = if fee_refund.peek().is_zero() {
    actual_fee
} else {
    match F::resolve(&who, fee_refund) {
        Ok(()) => actual_fee,
        Err(fee_refund) => {
            // iade başarısız → tümü burn
            consumed.merge(fee_refund);
            reserved
        }
    }
};
```

**Önemli:** PGAS maliyeti DROP edilerek yakılır, `OnUnbalanced` handler'a gönderilmez.

### 5. RPC API

**TransactionPaymentApi (v4):**
- `query_info(uxt, len)` → `RuntimeDispatchInfo { weight, class, partial_fee }`
- `query_fee_details(uxt, len)` → `FeeDetails { inclusion_fee, tip }`
- `query_weight_to_fee(weight)` → `Balance`
- `query_length_to_fee(length)` → `Balance`

**TransactionPaymentCallApi (v3):**
- `query_call_info(call, len)` → `RuntimeDispatchInfo`
- `query_call_fee_details(call, len)` → `FeeDetails`
- `query_weight_to_fee(weight)` → `Balance`
- `query_length_to_fee(length)` → `Balance`

---

## Events ve Errors

### Events (lib.rs:450-454)

```rust
pub enum Event<T: Config> {
    /// TransactionFeePaid { who, actual_fee, tip }
    TransactionFeePaid { who: T::AccountId, actual_fee: BalanceOf<T>, tip: BalanceOf<T> },
}
```

### asset-tx-payment Events

```rust
pub enum Event<T: Config> {
    AssetTxFeePaid {
        who: T::AccountId,
        actual_fee: AssetBalanceOf<T>,
        tip: AssetBalanceOf<T>,
        asset_id: Option<ChargeAssetIdOf<T>>,
    },
}
```

### asset-conversion-tx-payment Events

```rust
pub enum Event<T: Config> {
    AssetTxFeePaid {
        who: T::AccountId,
        actual_fee: BalanceOf<T>,
        tip: BalanceOf<T>,
        asset_id: T::AssetId,
    },
    /// Refund swap'ı başarısız olduğunda
    AssetRefundFailed { native_amount_kept: BalanceOf<T> },
}
```

### skip-feeless-payment Events

```rust
pub enum Event<T: Config> {
    FeeSkipped { origin: <T::RuntimeOrigin as OriginTrait>::PalletsOrigin },
}
```

### pgas-allowance Events

```rust
pub enum Event<T: Config> {
    PGASFeePaid { who: T::AccountId, actual_fee: BalanceOf<T> },
}
```

### Errors

Bu pallet explicit olarak `Error` enum'u tanımlamaz. Hatalar `TransactionValidityError` üzerinden döner:

- `InvalidTransaction::Payment` → Ücret ödenemez (yetersiz bakiye vs)
- `InvalidTransaction::Payment` → Asset dönüşümü başarısız

---

## Güvenlik ve Tasarım Kararları

### 1. validate vs prepare Ayrımı

`validate` yalnızca check yapar, `prepare` gerçek işlemi yapar. Bu, tx havuzu güvenliği için kritiktir: tx havuzuna kabul için sadece `validate` çağrılır; gerçek fee çekimi blok üretiminde `prepare`'de yapılır.

### 2. SuppressedDrop

`TxPaymentCredit` için `SuppressedDrop` kullanılır. Normal drop yerine explicit `into_inner()` çağrısı gerektirir. Bu, credit'in kazayla drop edilmesini önler.

### 3. account_exists Kontrolü

```rust
let fee_credit = if frame_system::Pallet::<T>::account_exists(who) {
    // iade yap
} else {
    remaining_credit  // hesap öldüyse iade yapma
};
```

Tx çalışırken hesabın öldürülebileceği durumda (örn. tüm bakiyeleri transfer) iade girişimi yapılmaz. Test (tests.rs:519-557) bunu doğrular.

### 4. Integrity Test

```rust
fn integrity_test() {
    // Multiplier'ın max_block_weight'den büyük olabilmesi gerektiğini doğrular
    assert!(
        <Multiplier as Bounded>::max_value() >=
            Multiplier::checked_from_integer::<u128>(
                T::BlockWeights::get().max_block.ref_time().try_into().unwrap()
            ).unwrap(),
    );

    // Min multiplier'ın makul trafik ile toparlayabildiğini doğrular
    let min_value = T::FeeMultiplierUpdate::min();
    let next = T::FeeMultiplierUpdate::convert(min_value);
    assert!(next > min_value, "Multiplier'ın minimum değeri çok düşük...");
}
```

### 5. Whitelist Storage

`NextFeeMultiplier` ve `TxPaymentCredit` `#[pallet::whitelist_storage]` ile işaretlenmiştir. Bu, her tx okumalarını hızlandırır.

---

## Ağırlık Referansları

### Benchmark Sonuçları (weights.rs:83-91)

```rust
fn charge_transaction_payment() -> Weight {
    // Kitchensink runtime, Intel Xeon @ 2.60GHz
    // Minimum execution time: 35_425_000 picoseconds (35.4 microseconds)
    Weight::from_parts(35_979_000, 3593)
        .saturating_add(T::DbWeight::get().reads(1_u64))  // System::Account okuma
        .saturating_add(T::DbWeight::get().writes(1_u64)) // System::Account yazma
}
```

---

## Test Suite Özeti

### Ana Testler (tests.rs)

| Test | Açıklama |
|------|---------|
| `transaction_extension_transaction_payment_work` | Tip + base + len + weight fee doğru kesilir, refund çalışır |
| `transaction_extension_transaction_payment_multiplied_refund_works` | 3/2 multiplier ile refund doğru hesaplanır |
| `transaction_extension_transaction_payment_is_bounded` | Weight::MAX ile overflow olmaz |
| `transaction_ext_length_fee_is_also_updated_per_congestion` | Multiplier length fee'yi etkilemez (sadece weight fee) |
| `query_info_and_fee_details_works` | RPC benzeri fee sorguları doğru çalışır |
| `compute_fee_works_without_multiplier` | Multiplier=1 durumu |
| `compute_fee_works_with_multiplier` | Multiplier=3/2 durumu |
| `compute_fee_works_with_negative_multiplier` | Multiplier=1/2 durumu (düşük congestion) |
| `compute_fee_does_not_overflow` | Weight::MAX ile u64::MAX saturate eder |
| `refund_does_not_recreate_account` | Öldürülmüş hesap yeniden oluşturulmaz |
| `actual_weight_higher_than_max_refunds_nothing` | Bildirilenden yüksek actual weight → refund yok |
| `zero_transfer_on_free_transaction` | Pays::No → event 0 fee ile yayınlanır |
| `refund_consistent_with_actual_weight` | Refund = multiplier * weight_diff |
| `should_alter_operational_priority` | Operational tx çok daha yüksek priority |
| `no_tip_has_some_priority` | Tip=0 bile priority > 0 |
| `higher_tip_have_higher_priority` | Monoton artan priority |
| `post_info_can_change_pays_fee` | Post dispatch Pays::No → sadece tip ödenir |
| `genesis_config_works` | Genesis multiplier doğru set edilir |
| `no_fee_and_no_weight_for_other_origins` | Root origin → ücret yok, weight iade |
| `fungible_adapter_no_zero_refund_action` | Sıfır iade işlemi event yayımlamaz |

### Kritik Test Doğrulaması

**Multiplier ile test (tests.rs:176-203):**
```rust
// Multiplier = 3/2
// base=5, len=10, call_weight=100, ext_weight=10, tip=5

// Pre-dispatch fee:
// 5 + 10 + (3/2 * (100 + 10)) + 5 = 5 + 10 + 165 + 5 = 185
assert_eq!(Balances::free_balance(2), 200 - 5 - 10 - 165 - 5);

// Actual weight = 50 (100 yerine)
// Refund = 3/2 * (100 - 50) = 75
assert_eq!(Balances::free_balance(2), 200 - 5 - 10 - (165 - 75) - 5);
// = 200 - 5 - 10 - 90 - 5 = 90
```

---

## Nova Shield Uygulama Notu

### 1. Fee Formülü — Birebir Adaptasyon

Nova Shield için öneri: Substrate'in 3 bileşenli fee formülünü benimse:

```rust
inclusion_fee = base_fee + length_fee + (multiplier * weight_fee)
final_fee = inclusion_fee + tip
```

**Avantajlar:**
- `base_fee`: Spam önleme için minimum floor
- `length_fee`: Büyük tx'leri caydırır (state büyümesini dengeler)
- `weight_fee + multiplier`: Gerçek hesaplama maliyetini yansıtır, kongestion ile artar
- `tip`: Kullanıcı tarafından belirlenebilir öncelik mekanizması

### 2. TargetedFeeAdjustment — Kongestion Kontrolü

```rust
// Nova Shield için önerilen parametreler:
// - target: %50 doluluk (Polkadot %25 kullanıyor, daha agresif seçilebilir)
// - v: 0.00001 - 0.0001 arası (değişim hızı)
// - min: Multiplier::from_u32(1) (asla 0'a düşmesin)
// - max: Multiplier::saturating_from_integer(1000) (1000x üst sınır)
```

Bu formülün güzelliği: İkinci dereceli Taylor terimi, hızlı kongestion'da fee'nin üstel büyümesini sağlar. Lineer değil, dolayısıyla spam saldırılarına karşı daha dirençli.

### 3. WeightToFee Seçimi

Nova Shield DEX için:
- `IdentityFee<Balance>`: Basit durumlar için (1 weight = 1 birim fee)
- `ConstantMultiplier<Balance, M>`: Tüm weight'e sabit çarpan
- Özel implementasyon: DEX swap tx'leri için `ref_time + proof_size` ayrı ağırlıklandırma

```rust
// Örnek özel implementasyon
impl WeightToFee for NovaWeightToFee {
    type Balance = u128;
    fn weight_to_fee(weight: &Weight) -> u128 {
        // ref_time ve proof_size'ı farklı katsayılarla ağırlıklandır
        let time_fee = weight.ref_time().saturating_mul(REF_TIME_FEE_PER_UNIT);
        let proof_fee = weight.proof_size().saturating_mul(PROOF_SIZE_FEE_PER_UNIT);
        time_fee.saturating_add(proof_fee).saturated_into()
    }
}
```

### 4. Multi-Asset Ödeme (Asset Payment)

Nova Shield için `asset-tx-payment` veya `asset-conversion-tx-payment` modelini düşün:
- Kullanıcılar NOVA dışı tokenlarla işlem ücreti ödeyebilir
- DEX ile entegrasyon: ödeme asset'i native'e swap edilir
- `SwapAssetAdapter` modeli ideal: kullanıcı her asset ile öder, chain hep native alır

```rust
// Nova Shield DEX entegrasyonu örneği:
// ChargeAssetTxPayment ile kullanıcı istediği asset'i seçer
// SwapAssetAdapter, Nova DEX üzerinden native'e dönüştürür
pub struct NovaSwapAdapter;
impl OnChargeAssetTransaction<T> for NovaSwapAdapter {
    // Nova DEX'i kullanarak swap
}
```

### 5. SkipCheckIfFeeless — Ücretsiz İşlemler

Nova Shield'da belirli işlemler ücretsiz yapılabilir:
- Yeni kullanıcı onboarding tx'leri
- Governance oy tx'leri
- Emergency tx'ler (slashing ihbarları vs)

```rust
// Pallet dispatch'ine eklenecek:
#[pallet::feeless_if(|_origin: &OriginFor<T>, call: &RuntimeCall| {
    matches!(call, RuntimeCall::Governance(GovCall::vote { .. }))
})]
fn vote(origin: OriginFor<T>, ...) { ... }
```

### 6. TxPaymentCredit Mekanizması

Nova DEX için kritik: Pre-swap ve post-swap arasında fee miktar ayarlaması:

```rust
// Nova DEX pallet'inde, swap başarısızsa daha az fee al:
pub fn execute_swap(origin: OriginFor<T>, ...) -> DispatchResult {
    // Tahmini ağır işlem
    let result = do_swap(...);
    if result.is_err() {
        // Swap başarısız → withdraw_txfee ile hesaplama kısmını geri ver
        // Bu mekanizma mevcut: Pallet::<T>::withdraw_txfee(heavy_compute_fee)?
    }
    Ok(())
}
```

### 7. Priority Sistemi — Nova Shield için

Operational tx (validator, staking, governance) vs Normal tx ayrımı:
- Validator reward claim'leri: `DispatchClass::Operational`
- DEX swap'ları: `DispatchClass::Normal`
- Protocol mesajları: `DispatchClass::Mandatory`

`OperationalFeeMultiplier = 5` (Polkadot default) Nova Shield için makul başlangıç.

### 8. Güvenlik Gereksinimleri

Nova Shield için zorunlu kontrol noktaları:

```
a) integrity_test(): Genesis'te ve her önemli parametre değişiminde çalıştır
   - Multiplier'ın min değerinden geri dönebileceğini doğrula
   - MaxBlock weight'inin Multiplier tipi ile temsil edilebileceğini doğrula

b) Whitelist storage: NextFeeMultiplier ve TxPaymentCredit her tx'te okunur
   → mutlaka whitelist_storage olarak işaretle

c) on_finalize sırası: Multiplier güncellenmesi, system pallet weight temizliğinden ÖNCE yapılmalı

d) SuppressedDrop: credit'lerin kazayla drop edilmemesi için zorunlu
```

### 9. PGAS Modeli İncelemesi

`pgas-allowance` alt crate'i, Nova Shield için ilginç bir model sunar:
- PGAS özel bir token (NFT, loyalty token, staking ödülü vb.)
- PGAS ile ödeme yapılınca fee **yakılır** (burn), validator'a gitmiyor
- DEX'te farklı tokenomics modeli için uyarlanabilir

Nova Shield'da örneğin:
- NOVA token'ın belirli yüzdesi yakılır (deflationary pressure)
- Staking rewards'tan gelen NOVA ile fee ödenebilir

### 10. RPC Endpoint'leri

Nova Shield için zorunlu RPC'ler:
```
payment_queryInfo(encoded_xt, at?)       → RuntimeDispatchInfo
payment_queryFeeDetails(encoded_xt, at?) → FeeDetails
query_weight_to_fee(weight)              → Balance
query_length_to_fee(length)              → Balance
```

Bu endpoint'ler istemci uygulamalarının (cüzdanlar, block explorer) fee tahmin yapabilmesi için kritiktir.

---

## Kaynak Kod Referansları

| Satır | Dosya | Konu |
|-------|-------|------|
| lib.rs:33-36 | src/lib.rs | Ana fee formülü (doc comment) |
| lib.rs:90 | src/lib.rs | `type Multiplier = FixedU128` |
| lib.rs:103-116 | src/lib.rs | Multiplier güncelleme formülü (doc) |
| lib.rs:150 | src/lib.rs | `TargetedFeeAdjustment` struct tanımı |
| lib.rs:201-273 | src/lib.rs | `Convert<Multiplier, Multiplier>` impl (tam formül) |
| lib.rs:277-300 | src/lib.rs | `ConstFeeMultiplier` |
| lib.rs:320 | src/lib.rs | `MULTIPLIER_DEFAULT_VALUE = 1` |
| lib.rs:352-405 | src/lib.rs | Config trait'i |
| lib.rs:407-415 | src/lib.rs | `NextFeeMultiplier` storage |
| lib.rs:450-454 | src/lib.rs | `TransactionFeePaid` event |
| lib.rs:458-471 | src/lib.rs | `on_finalize` — multiplier güncelleme |
| lib.rs:604-614 | src/lib.rs | `compute_fee` |
| lib.rs:617-688 | src/lib.rs | `compute_fee_raw` — tam implementasyon |
| lib.rs:691-702 | src/lib.rs | `length_to_fee` ve `weight_to_fee` |
| lib.rs:794-796 | src/lib.rs | `ChargeTransactionPayment` struct |
| lib.rs:871-931 | src/lib.rs | `get_priority` — priority hesabı |
| lib.rs:992-1083 | src/lib.rs | `TransactionExtension` impl |
| lib.rs:1005-1031 | src/lib.rs | `validate` adımı |
| lib.rs:1033-1050 | src/lib.rs | `prepare` adımı |
| lib.rs:1052-1083 | src/lib.rs | `post_dispatch_details` |
| types.rs:34-60 | src/types.rs | `InclusionFee` struct ve `inclusion_fee()` |
| types.rs:66-90 | src/types.rs | `FeeDetails` struct ve `final_fee()` |
| payment.rs:38-101 | src/payment.rs | `OnChargeTransaction` + `TxCreditHold` trait |
| payment.rs:109-223 | src/payment.rs | `FungibleAdapter` implementasyonu |
| tests.rs:176-203 | src/tests.rs | Multiplier refund testi |
| tests.rs:259-278 | src/tests.rs | Length fee kongestion testi |
| tests.rs:429-460 | src/tests.rs | Multiplier fee testi |
| tests.rs:651-705 | src/tests.rs | Operational priority testi |
| mock.rs:63-67 | src/mock.rs | Test parametreleri (OperationalFeeMultiplier=5) |
| mock.rs:81-97 | src/mock.rs | WeightToFee ve LengthToFee mock impl |
| weights.rs:83-91 | src/weights.rs | Benchmark: 35.4 mikrosaniye, 1R+1W |
| asset-tx-payment/payment.rs:142-163 | | `FungiblesAdapter.withdraw_fee` |
| asset-conversion-tx-payment/payment.rs:119-176 | | `SwapAssetAdapter.withdraw_fee` |
| skip-feeless-payment/lib.rs:151-175 | | `SkipCheckIfFeeless.validate` |
| pgas-allowance/lib.rs:344-375 | | `ChargePGAS.post_dispatch_details` (burn) |
| rpc/runtime-api/lib.rs:27-56 | | Runtime API trait v4 tanımları |

---

## Alt Crate Testleri — Derinlemesine Analiz

### asset-tx-payment Test Bulguları

**`transaction_payment_in_asset_possible` (asset-tx-payment/tests.rs:169-240)**

Bu test, native token yerine özel bir asset (örn. asset_id=1) ile fee ödemesinin nasıl çalıştığını doğrular:

```
// Asset oluştur: is_sufficient=true zorunlu
Assets::force_create(RuntimeOrigin::root(), asset_id.into(), 42, true, min_balance);

// Fee hesabı (ConversionToAssetBalance implementasyonu):
// native_fee → asset_fee dönüşümü min_balance / ExistentialDeposit oranıyla
let fee = (base_weight + call_weight + len) * min_balance / ExistentialDeposit::get();

// Native bakiye etkilenmez!
assert_eq!(Balances::free_balance(caller), 10 * balance_factor);

// Block author asset alır (OnUnbalanced handler)
assert_eq!(Assets::balance(asset_id, BLOCK_AUTHOR), fee);
```

**`payment_from_account_with_only_assets` (tests.rs:378-433)**

Native bakiyesi sıfır olan hesapların yalnızca asset bakiyesiyle işlem yapabileceğini kanıtlar. Bu özellik, kullanıcı onboarding senaryoları için kritiktir — yeni kullanıcılar native token olmadan da işlem yapabilir.

**`converted_fee_is_never_zero_if_input_fee_is_not` (tests.rs:480-555)**

Integer bölme sonucu sıfıra yuvarlanan fee için minimum 1 birim garantisi:

```
// min_balance=1, ExistentialDeposit=10
// native_fee = base(1) + weight(1) + len(1) = 3
// naive_asset_fee = 3 * 1 / 10 = 0  ← INTEGER DIVISION → SIFIR!

// FungiblesAdapter çözümü:
let min_converted_fee = if fee.is_zero() { Zero::zero() } else { One::one() };
let converted_fee = to_asset_balance(fee, asset_id).max(min_converted_fee);

// Sonuç: fee=0 değil, fee=1 alınır
assert_eq!(Assets::balance(asset_id, caller), balance - 1);
```

**`payment_only_with_existing_sufficient_asset` (tests.rs:435-478)**

Ödeme için kullanılan asset `is_sufficient=true` olmalıdır. Non-sufficient asset ile ödeme reddedilir:
- Var olmayan asset → `validate_and_prepare` hata
- `is_sufficient=false` asset → hata
- `is_sufficient=true` asset → başarı

**`post_dispatch_fee_is_zero_if_pre_dispatch_fee_is_zero` (tests.rs:557-614)**

Kritik güvenlik garantisi: `Pays::No` ile pre-dispatch → post-dispatch'te `Pays::Yes` bile gelse daha fazla fee alınamaz. `InitialPayment::Nothing` olarak kaydedilen state post-dispatch'te değiştirilemez:

```rust
let Pre::Charge { initial_payment, .. } = &pre else { panic!("Expected Charge"); };
assert!(matches!(initial_payment, InitialPayment::Nothing));
// Pays::Yes post_dispatch versilse de fee alınmaz
assert_eq!(Assets::balance(asset_id, caller), balance);
```

### asset-conversion-tx-payment Test Bulguları

**`setup_lp` AMM Pool Kurulumu (tests.rs:119-153)**

```
Likidite oranı: native:asset = 1000:10000 = 1:10
→ 1 native = 10 asset
→ fee ödemek için: native_fee * 10 kadar asset gerekir (yaklaşık, slippage dahil)
```

Test altyapısı NativeOrWithId enum'u kullanır:
- `NativeOrWithId::Native` → native token
- `NativeOrWithId::WithId(asset_id)` → fungible asset

**`transaction_payment_in_native_possible` (tests.rs:160-)**

Asset payment extension, native ödeme için de çalışır (`asset_id=None`). Bu durumda normal `ChargeTransactionPayment` yoluna düşer.

---

## Weights (Ağırlıklar) Detayı

### charge_transaction_payment Benchmark Sonuçları

**Çalışma ortamı (weights.rs:37-62):**
- CLI version: 32.0.0, Date: 2025-02-21
- Steps: 50, Repeat: 20, CPU: Intel Xeon @ 2.60GHz
- WASM Execution: Compiled, Runtime: Kitchensink

**Sonuç:**
```
ref_time:   35_979_000 picoseconds (35.9 mikrosaniye)
proof_size: 3593 bytes
DB reads:   1 (System::Account okuma)
DB writes:  1 (System::Account yazma)
Minimum execution time: 35_425_000 picoseconds
```

`SubstrateWeight<T>` ve `WeightInfo for ()` aynı değeri döndürür — `()` implementasyonu da üretim için uygun ağırlıklara sahiptir.

### Extension Weight Dahil Edilmesi

`ChargeTransactionPayment` extension'ı kendisi de bir ağırlığa sahiptir ve bu, tx'in toplam ağırlığına eklenir:

```rust
// DispatchInfo.extension_weight alanına eklenir
fn weight(&self, _: &T::RuntimeCall) -> Weight {
    T::WeightInfo::charge_transaction_payment()
    // = Weight::from_parts(35_979_000, 3593) + 1R + 1W
}
```

Test'de bu açıkça gösterilir (tests.rs:144-148):
```rust
let ext = Ext::from(0);
let ext_weight = ext.weight(CALL);
info.extension_weight = ext_weight;
// Toplam fee = base + len + multiplier*(call_weight + ext_weight) + tip
```

---

## Releases ve Storage Migrasyonu

### StorageVersion (lib.rs:304-316)

```rust
#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum Releases {
    V1Ancient,  // Orijinal (FixedI128 multiplier)
    V2,         // FixedU128 multiplier (negatif olamaz)
}
```

V1 → V2 geçişi: `FixedI128` (işaretli) yerine `FixedU128` (işaretsiz) multiplier kullanılmaya başlandı. Bu, multiplier'ın negatif değerlere düşmesini önler — minimum değer `M::get()` ile sınırlanır.

### Genesis Build (lib.rs:440-445)

```rust
impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
    fn build(&self) {
        StorageVersion::<T>::put(Releases::V2);  // Yeni chain = V2
        NextFeeMultiplier::<T>::put(self.multiplier);  // Başlangıç multiplier
    }
}
```

Yeni chain her zaman V2 ile başlar. Eski zincirlerden migration gerekirse V1Ancient → V2 migration kodu ayrıca yazılmalıdır.

---

## EstimateCallFee Trait'i

### Implementasyon (lib.rs:1085-1095)

```rust
impl<T: Config, AnyCall: GetDispatchInfo + Encode> EstimateCallFee<AnyCall, BalanceOf<T>>
    for Pallet<T>
{
    fn estimate_call_fee(call: &AnyCall, post_info: PostDispatchInfo) -> BalanceOf<T> {
        let len = call.encoded_size() as u32;
        let info = call.get_dispatch_info();
        Self::compute_actual_fee(len, &info, &post_info, Zero::zero())  // tip=0
    }
}
```

Bu, diğer pallet'lerin (staking, governance, xcm) bir call'ın ne kadara mal olacağını önceden tahmin etmesine olanak tanır. Tip dahil edilmez.

---

## Çok Boyutlu Weight ve ZK Sistemler İçin Önemi

### ref_time vs proof_size

```
Weight {
    ref_time:   u64,  // nanosaniye cinsinden hesaplama süresi
    proof_size: u64,  // Merkle proof büyüklüğü (byte)
}
```

Her iki boyut normalize edilir ve limiting dimension seçilir:
```
if normalized_ref_time < normalized_proof_size:
    limiting = proof_size   // proof_size daha kısıtlayıcı
else:
    limiting = ref_time     // ref_time daha kısıtlayıcı
```

### Nova Shield ZK İşlemleri İçin Kritik

Nova Shield'da ZK proof üretimi (arkworks, Groth16/PLONK) kullanıldığında:
- `proof_size` önemli ölçüde artabilir (Groth16 proof = ~192 byte, PLONK ~600+ byte)
- Multiplier hesaplamalarında `proof_size` limiting dimension olabilir
- ZK batch tx'lerde `proof_size` kritik dar boğaz haline gelebilir

Önerilen yaklaşım:
```
// ZK tx için weight bileşenleri:
// ref_time: ZK proof doğrulama süresi (SNARK verifier ~ms)
// proof_size: Merkle path + ZK proof boyutu

// Fee formülü ZK bilinciyle:
weight_fee = ref_time * TIME_COEFFICIENT + proof_size * PROOF_COEFFICIENT
// PROOF_COEFFICIENT > TIME_COEFFICIENT seçerek proof_size daha pahalı yapılabilir
```

---

## TransactionExtension Pipeline Detayı

### FRAME Runtime Extension Zinciri Örneği

```
Tx arrival → pool validation:
  validate(CheckNonZeroSender)
  validate(CheckSpecVersion)
  validate(CheckTxVersion)
  validate(CheckGenesis)
  validate(CheckEra)
  validate(CheckNonce)
  validate(CheckWeight)
  validate(ChargeTransactionPayment)  ← sadece bakiye kontrolü

Block production:
  prepare(CheckNonZeroSender)
  ...
  prepare(ChargeTransactionPayment)   ← gerçek fee çekimi

  dispatch(call)

  post_dispatch(ChargeTransactionPayment)  ← refund, event
  ...
```

### validate vs prepare Neden Ayrıdır?

Pool'da tx beklerken bakiye değişebilir:
- `validate` esnasında bakiye = 100, fee = 50 → OK
- Başka tx çalışır, bakiye = 30'a düşer
- `prepare` esnasında bakiye = 30 < 50 → `InvalidTransaction::Payment` hatası

Bu ayrım, tx'lerin pool'a önce hafif bir kontrol ile kabul edilmesini, blok üretiminde ise gerçek çekim yapılmasını sağlar. Sistem tutarlıdır: eğer `prepare` başarısız olursa tx bloğa dahil edilmez.

---

## Refund Mekanizması — Tam Analiz

### Pre-dispatch vs Post-dispatch Farkı

```
Estimated (pre-dispatch):
  fee = base + len + multiplier * estimated_weight

Actual (post-dispatch):
  fee = base + len + multiplier * actual_weight

Refund = estimated_fee - actual_fee
       = multiplier * (estimated_weight - actual_weight)
```

`actual_weight <= estimated_weight` her zaman geçerlidir çünkü:
1. Dispatch, estimated_weight'i aşamaz (CheckWeight extension bunu kontrol eder)
2. Dispatch erken sonlanabilir → actual_weight < estimated_weight

### Multiplier Refund Testi (tests.rs:175-203)

```
Setup: multiplier = 3/2, base=5, len=10
      call_weight=100, ext_weight=10, tip=5

Pre-dispatch fee:
  adjusted_weight_fee = 3/2 * (100 + 10) = 165
  total = 5 + 10 + 165 + 5 = 185
  balance: 200 → 15

Dispatch actual_weight = 50 (call=50, ext=10):
  actual_adjusted = 3/2 * (50 + 10) = 90
  refund = 165 - 90 = 75
  actual_total = 5 + 10 + 90 + 5 = 110
  balance: 200 → 90
```

Kaynak kanıtı (tests.rs:199-201):
```rust
// 75 (3/2 of the returned 50 units of call weight, 0 returned of ext weight) is refunded
assert_eq!(Balances::free_balance(2), 200 - 5 - 10 - (165 - 75) - 5);
```

### Extension Weight de Refund Edilebilir

Extension'ın kendi ağırlığı da gerçek çalışma süresine göre ayarlanabilir. `post_dispatch_details` `Weight::zero()` döndürür (refund yok) ama genel framework'te extension weight farklılıkları da hesaplanır.

---

## Fee Dağılım Örnekleri (Test Verilerinden)

### Örnek 1: Basit Senaryo (Multiplier=1)

```
Parameters:
  balance_factor = 10 → balance = 100
  base_weight = 5, byte_fee = 1, weight_to_fee = 1
  call_weight = 5, ext_weight = 10, len = 10, tip = 0

Hesap:
  base_fee          = WeightToFee(5)  = 5 * 1 = 5
  len_fee           = LengthToFee(10) = 10 * 1 = 10
  unadjusted_weight = (5 + 10) = 15
  adjusted_weight   = 1 * 15 = 15

inclusion_fee = 5 + 10 + 15 = 30
final_fee     = 30 + 0 = 30
```

Test: `assert_eq!(Balances::free_balance(1), 100 - 5 - 5 - 10 - 10)` = 70 (tests.rs:154)

### Örnek 2: Multiplier + Tip + Refund

```
Parameters:
  multiplier = 3/2, base=5, len=10, call_weight=100, ext_weight=10, tip=5

Pre-dispatch:
  adjusted_weight_fee = 3/2 * 110 = 165
  total_pre = 5 + 10 + 165 + 5 = 185

Post-dispatch (actual_weight = 50):
  adjusted_actual = 3/2 * 60 = 90
  total_actual = 5 + 10 + 90 + 5 = 110
  refund = 185 - 110 = 75
```

### Örnek 3: Asset Fee Dönüşümü

```
FungiblesAdapter ConversionToAssetBalance örneği:
  native_fee = 20 (base + weight + len)
  min_balance = 2, ExistentialDeposit = 10

  asset_fee = 20 * 2 / 10 = 4 asset birimi
  → Kullanıcı native bakiyesinden değil, 4 asset öder
```

### Örnek 4: SwapAssetAdapter AMM Dönüşümü

```
AMM Pool: native:asset = 1:10
  native_fee = 100

  quote_price_tokens_for_exact_tokens(asset, native, 100, include_fee=true)
  → Approximately 1000+ asset (AMM price impact + protocol fee dahil)

  swap(asset → native, exact_out=100_native) → fee_credit
  Kalan değişim sıfır olmalı (exact price için)
```

---

## Acımasız İnceleme — Kritik Gözlemler

### 1. Multiplier Yalnızca weight_fee'yi Etkiler (Test Adı Yanıltıcı)

Kaynak: lib.rs:671-680 satırları doğrulanmıştır:
```rust
let adjusted_weight_fee = multiplier.saturating_mul_int(unadjusted_weight_fee);
// ...
let len_fee = Self::length_to_fee(len);    // multiplier YOK
let base_fee = ...base_extrinsic...;       // multiplier YOK
```

`transaction_ext_length_fee_is_also_updated_per_congestion` testi adına rağmen, length fee kongestiondan etkilenmez. Sadece weight fee etkilenir. Bu, Polkadot'un bilinçli bir tasarım kararıdır — base ve length fee sabit kalarak tahmin edilebilirliği artırır.

### 2. Priority Hesabında Sıfır Tip Engeli

```rust
let tip = tip.saturating_add(One::one());  // 0 → 1
let scaled_tip = max_reward(tip);
```

Sıfır-tip tx'ler de minimum 1 birim priority alır. Sıfır-tip tx'ler arasında küçük tx'ler (daha küçük weight/length) daha yüksek priority kazanır. Bu spam korumasına katkıda bulunur.

### 3. Account Kill Durumunda Refund Yapılmaz

```rust
if frame_system::Pallet::<T>::account_exists(who) {
    // refund dene
} else {
    remaining_credit  // hesap öldü → tüm fee alınır
};
```

Tx yürütülürken hesabın tüm bakiyesini transfer etmesi durumunda hesap "öldürülür" (kill). Bu durumda refund yapılmaz — kalan fee validator'a gider.

### 4. TxPaymentCredit Bloklar Arası Sızma Riski

`on_finalize`'da kontrol edilir:
```rust
TxPaymentCredit::<T>::take().map(|credit| {
    log::error!(target: LOG_TARGET,
        "The `TxPaymentCredit` was stored between blocks. This is a bug.");
    credit.into_inner()
});
```

Eğer post_dispatch çağrılmazsa (olağanüstü durum), credit bir sonraki bloğa sızar. `on_finalize` bunu yakalar, loglar ve temizler. Credit drop edilir → burn.

### 5. Multiplier Minimum Sınırı İçin Integrity Test

```rust
fn integrity_test() {
    let min_value = T::FeeMultiplierUpdate::min();
    let next = T::FeeMultiplierUpdate::convert(min_value);
    assert!(
        next > min_value,
        "Min multiplier çok düşük: min'den geri dönemez!"
    );
}
```

Bu test, runtime başlatıldığında multiplier'ın min değerinden toparlanabileceğini doğrular. Eğer `min_value` çok düşük seçilirse, zincir sonsuz ucuz tx döngüsüne girebilir.

### 6. Bare Extrinsic'lerde Sıfır Fee

```rust
let partial_fee = if unchecked_extrinsic.is_bare() {
    0u32.into()
} else {
    Self::compute_fee(len, &dispatch_info, 0u32.into())
};
```

Unsigned extrinsic'ler (bare) fee ödemez. Bu, pallet'lerin `validate_unsigned` mekanizmasıyla güvenilir unsigned tx'leri desteklemesi için gereklidir (örn. heartbeat, oracle report).

### 7. SuppressedDrop Güvenlik Garantisi

`TxPaymentCredit` için `SuppressedDrop<Credit>` kullanılır. Normal drop yerine explicit `into_inner()` çağrısı gerektirir. Bu, credit'in kazayla drop edilmesini önler — accidental burn riski ortadan kalkar.

---

## Config Parametrelerinin Tam Özeti

```rust
pub trait Config: frame_system::Config {
    type RuntimeEvent;
    type OnChargeTransaction: OnChargeTransaction<Self>;  // fee ödeme mantığı
    type WeightToFee: WeightToFee<Balance = BalanceOf<Self>>;
    type LengthToFee: WeightToFee<Balance = BalanceOf<Self>>;
    type FeeMultiplierUpdate: MultiplierUpdate;
    #[pallet::constant]
    type OperationalFeeMultiplier: Get<u8>;
    type WeightInfo: WeightInfo;
}
```

### Önerilen Nova Shield Değerleri

| Parametre | Polkadot Default | Nova Shield Önerisi | Gerekçe |
|-----------|-----------------|---------------------|---------|
| `WeightToFee` | `IdentityFee` | Custom (ref_time + proof_size ayrı) | ZK tx'ler |
| `LengthToFee` | `ConstantMultiplier` | `ConstantMultiplier<Balance, ByteFee>` | Basit ve öngörülebilir |
| `FeeMultiplierUpdate` | `TargetedFeeAdjustment` | `TargetedFeeAdjustment<T,S,V,M,X>` | Dinamik kongestion kontrolü |
| Target (S) | 25% (0.25) | 50% (0.50) | Daha yüksek hedef doluluk |
| Variability (V) | 0.00001 | 0.00005 | Daha hızlı tepki |
| Min Multiplier (M) | 0.1 | 1.0 | Fee hiç sıfırlanmasın |
| Max Multiplier (X) | None | 100x | Sınırsız artışı engelle |
| `OperationalFeeMultiplier` | 5 | 5 | Yeterli operational öncelik |
| `OnChargeTransaction` | `FungibleAdapter` | `FungibleAdapter` + PGAS alternatif | Esnek |

---

## Sonuç ve Değerlendirme

### Güçlü Yönler

1. **Temiz fee bileşen ayrımı**: base, length, weight, tip bağımsız konfigüre edilebilir
2. **Dinamik multiplier (TargetedFeeAdjustment)**: Blok doluluk oranına göre otomatik fee ayarı
3. **Multi-asset desteği**: Native + fungibles + AMM swap üzerinden ödeme
4. **Post-dispatch refund**: Fazla çekilen fee otomatik iade edilir
5. **validate/prepare ayrımı**: Pool güvenliği için doğru tasarım
6. **SkipCheckIfFeeless**: Ücretsiz tx'ler için temiz wrapper mekanizması
7. **PGAS burn modeli**: Deflationary token ekonomisi için güçlü model
8. **Çok boyutlu weight**: ref_time + proof_size ayrımı ZK sistemler için kritik
9. **EstimateCallFee**: Diğer pallet'lerin fee tahmin etmesine olanak tanır
10. **Integrity test**: Genesis'te güvenlik doğrulaması zorunlu kılar

### Zayıf Yönler / Dikkat Edilecekler

1. **Multiplier sadece weight_fee'yi etkiler**: Test adı yanıltıcı, belgelenmesi gerekir
2. **Asset precision riski**: Çok küçük fee dönüşümlerinde min=1 zorunlu
3. **SwapAssetAdapter refund karmaşıklığı**: Geri swap başarısız olursa refund yapılmaz (AssetRefundFailed event)
4. **TxPaymentCredit bloklar arası leak**: Teorik risk, on_finalize'da işleniyor
5. **Operational priority çok yüksek**: OperationalFeeMultiplier=5 ile ~97x avantaj, kötüye kullanım riski var

### Nova Shield İçin Kritik Tasarım Kararları

1. **Multiplier parametrelerini dikkatle seç**: `v` çok yüksek → fee volatil; çok düşük → kongestion sinyali zayıf
2. **ZK tx'leri için proof_size bileşeni ayrıca fiyatlandır**: ref_time yetmez
3. **SwapAssetAdapter modeli tercih et**: DEX entegrasyonu için FungiblesAdapter yerine
4. **PGAS benzeri burn mekanizması**: NOVA token deflasyonu için
5. **whitelist_storage zorunlu**: NextFeeMultiplier her tx'te okunur, performans kritik
6. **on_finalize sırasına dikkat**: Multiplier güncellenmesi, system pallet weight temizliğinden ÖNCE yapılmalı
7. **SuppressedDrop**: Credit'lerin kazayla drop edilmemesi için zorunlu
8. **integrity_test**: Her önemli parametre değişiminde çalıştır

---

## Ek Kaynak Kod Referansları

| Satır | Dosya | Konu |
|-------|-------|------|
| asset-tx-payment/lib.rs:175-179 | | `ChargeAssetTxPayment` struct (tip + asset_id) |
| asset-tx-payment/lib.rs:196-223 | | `withdraw_fee` — native vs asset dağılımı |
| asset-tx-payment/tests.rs:169-240 | | Asset fee tam ödeme testi |
| asset-tx-payment/tests.rs:480-555 | | Min fee=1 garantisi testi |
| asset-tx-payment/tests.rs:557-614 | | Pre=Nothing, post=Pays::Yes güvenlik testi |
| asset-conversion-tx-payment/payment.rs:100-101 | | `SwapAssetAdapter` tip parametreleri |
| asset-conversion-tx-payment/payment.rs:143-146 | | AMM quote fiyatı hesabı |
| asset-conversion-tx-payment/payment.rs:159-174 | | swap_tokens_for_exact_tokens |
| asset-conversion-tx-payment/payment.rs:210-257 | | correct_and_deposit_fee — native path |
| asset-conversion-tx-payment/payment.rs:259-322 | | correct_and_deposit_fee — swap refund path |
| skip-feeless-payment/lib.rs:86-88 | | `SkipCheckIfFeeless` struct |
| skip-feeless-payment/lib.rs:123-126 | | `T::RuntimeCall: CheckIfFeeless` bound |
| pgas-allowance/lib.rs:122-128 | | `ChargePGAS` struct (inner + skip_pgas) |
| pgas-allowance/lib.rs:170-196 | | `Val` ve `Pre` enum'ları |
| pgas-allowance/lib.rs:244-282 | | `validate` — PGAS path vs inner extension |
| pgas-allowance/lib.rs:285-335 | | `prepare` — PGAS fee çekimi |
| pgas-allowance/lib.rs:337-383 | | `post_dispatch_details` — burn mekanizması |
| rpc/src/lib.rs:40-51 | | `TransactionPaymentApi` RPC trait |
| rpc/src/lib.rs:95-126 | | `query_info` RPC implementasyonu |
| rpc/src/lib.rs:128-176 | | `query_fee_details` RPC implementasyonu |
