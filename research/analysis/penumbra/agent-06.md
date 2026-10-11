# Penumbra Analizi — Agent 06

**Analiz Tarihi:** 2026-07-22
**Kapsam:** component/fee (11 dosya) + component/distributions (8 dosya) + component/compact-block (10 dosya) + component/sct (15 dosya)
**Toplam:** 44 dosya, ~2.200 satır kaynak kodu

---

## Genel Bakis

Bu agent dört kritik Penumbra bileşenini kapsar:

1. **Fee Component** — Çok boyutlu gas fiyatlandırma, tier sistemi, proposer tip mekanizması
2. **Distributions Component** — Staking token ihracat bütçesi, Liquidity Tournament (LQT) ödül sistemi
3. **Compact Block Component** — Privacy-korumalı senkronizasyon protokolü; istemcilere note/swap/nullifier akışı
4. **SCT (State Commitment Tree) Component** — Poseidon tabanlı nullifier türetimi, Tiered Commitment Tree yönetimi, epoch ve saat (clock) altyapısı

Bu bileşenler birbirine derinlemesine bağlıdır: SCT, epoch zaman verisi sağlar; compact block, SCT kökleri ile gas fiyatlarını istemciye iletir; fee bileşeni gas tüketimini tahsil eder; distributions, token ihraç zamanlaması için SCT saatini kullanır.

---

## BÖLÜM 1: component/fee (11 Dosya)

---

## [fee.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/fee.rs`
**Satır Sayısı:** 206

### Genel Amaç

Penumbra'nın ücret modelini tanımlar. `Fee` bir `Value` newtype'idir (asset_id + amount). Herhangi bir desteklenen token ile ödeme yapılabilir. `FeeTier` üç seviyeli çarpan sistemi sunar.

### Struct/Enum/Type Listesi

**`Fee(pub Value)`** (satır 16)
- Newtype wrapper: `Value { amount: Amount, asset_id: asset::Id }`
- `Copy + Clone + Debug + PartialEq + Eq`
- Default: staking token, sıfır miktar (satır 18-22)

**Sabitler** (satır 11-13):
```rust
pub static FEE_TIER_LOW_MULTIPLIER: u32 = 105;    // %105 → %5 artış
pub static FEE_TIER_MEDIUM_MULTIPLIER: u32 = 130; // %130 → %30 artış
pub static FEE_TIER_HIGH_MULTIPLIER: u32 = 200;   // %200 → %100 artış
```
Her çarpanın zımni paydası 100'dür. Çarpan / 100 = gerçek katsayı.

**`FeeTier`** (satır 139-143)
```rust
pub enum FeeTier { Low, Medium, High }
```
Default: `Low` (satır 146)

### Trait Implementasyonları

- `DomainType for Fee` → Proto: `pb::Fee`
- `From<Fee> for pb::Fee`: Staking token ise `asset_id: None` ile gönderir (bant genişliği optimizasyonu)
- `TryFrom<pb::Fee> for Fee`: `asset_id: None` → STAKING_TOKEN_ASSET_ID olarak yorumlar
- `Display for FeeTier`: "low" / "medium" / "high" string'i
- `FromStr for FeeTier`: Aynı string'lerden parse, bilinmeyen → `bail!`

### Fonksiyonlar

**`Fee::from_staking_token_amount(amount: Amount) -> Self`** (satır 25-30)
- Staking tokenı için kolaylık kurucu
- `Value { amount, asset_id: *STAKING_TOKEN_ASSET_ID }`

**`Fee::amount(&self) -> Amount`** (satır 32-34)
- Dahili `Value.amount` alanını döndürür

**`Fee::asset_id(&self) -> asset::Id`** (satır 36-38)
- Dahili `Value.asset_id` döndürür

**`Fee::asset_matches(&self, other: &Fee) -> bool`** (satır 40-42)
- İki ücretin aynı token cinsinden olup olmadığını doğrular
- Alternatif token ücret desteği için kritik kontrol

**`Fee::balance(&self) -> balance::Balance`** (satır 44-46)
- `-Balance::from(self.0)` — negatif bakiye döndürür
- Semantik: Fee ödenince bakiye düşer

**`Fee::commit(&self, blinding: Fr) -> balance::Commitment`** (satır 48-50)
- ZK devre içinde fee commitment hesaplar
- Blinding faktörü `Fr` (decaf377 skalar alanı)

**`Fee::apply_tier(self, fee_tier: FeeTier) -> Self`** (satır 56-82)
- **Kritik algoritma:**
  ```
  yeni_miktar = (base_miktar * çarpan) / 100
  ```
  - Low: `amount * 105 / 100`
  - Medium: `amount * 130 / 100`
  - High: `amount * 200 / 100`
- TODO yorumu (satır 57-58): Fee public olduğu için parmak izi riski var. Önerilen düzeltme: örnekleme dağılımı kullanmak. Issue #3153 referansı.
- Tasarım notu: Bölme sonrası kalan (reminder) sessizce atılır — tam sayı bölmesi.

**`Fee::value(&self) -> Value`** (satır 133-135)
- `self.0` döndürür

### Unsafe Bloklar

Yok.

### Testler

Yok (bu dosyada).

### Tasarım Kararları

1. **Çok tokenlu ödeme desteği**: `Fee` herhangi bir asset'i tutabilir. Alternatif token desteği `fee_pay.rs` ve gas parametrelerinde yönetilir.
2. **Negatif bakiye semantiği**: `balance()` negatif döndürür — işlem doğrulama sırasında fee ZK devrelerinde balance koruması için kullanılır.
3. **Public fee parmak izi riski**: Tier seçimi gözlemlenebilir; yorum açıkça belirtmiş, issue açılmış ama henüz çözülmemiş.
4. **Protobuf optimizasyonu**: Staking token gönderiminde `asset_id` alanını atlayarak bant genişliği tasarrufu yapılır.

### Nova Shield Chain İçin Dersler

- Fee tierleri %5/%30/%100 artışlarla gelir — ilk değerlerin seçimi işlem havuzunda sıralama stratejisini etkiler; Nova Shield için test etmeye değer.
- `Fee::commit()` ile ZK devresinde ücret gizliliği sağlanabiliyor — Nova Shield MEV korumasıyla mükemmel uyum.
- `balance()` negatif döndürme tekniği; balance korunumu kanıtlarında şık yaklaşım — Nova Shield DEX işlemlerinde aynen kullanılmalı.
- Tier çarpanlarını `static` yerine governance parametresi yapmak daha esnektir; Penumbra'nın bu değerleri hard-code etmesi bir kısıtlamadır.

---

## [gas.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/gas.rs`
**Satır Sayısı:** 176

### Genel Amaç

Çok boyutlu (multidimensional) gas modelini tanımlar. Penumbra, gas'ı tek bir sayı yerine dört ayrı boyutta ölçer. Bu her boyut için bağımsız fiyat ayarı imkânı tanır.

### Struct/Enum/Type Listesi

**`Gas`** (satır 17-24)
```rust
pub struct Gas {
    pub block_space: u64,
    pub compact_block_space: u64,
    pub verification: u64,
    pub execution: u64,
}
```
- `Copy + Clone + Debug + Eq + PartialEq + Serialize + Deserialize`
- Dört boyut:
  1. `block_space` — transaction'ın L1 blokta kapladığı bayt sayısı
  2. `compact_block_space` — compact block'ta kapladığı bayt (privacy client sync yükü)
  3. `verification` — ZK ispat doğrulama maliyeti
  4. `execution` — on-chain yürütme maliyeti

**`GasPrices`** (satır 95-103)
```rust
pub struct GasPrices {
    pub asset_id: asset::Id,
    pub block_space_price: u64,
    pub compact_block_space_price: u64,
    pub verification_price: u64,
    pub execution_price: u64,
}
```
- Default: Tümü sıfır (ücretsiz), staking token asset_id (satır 105-115)

### Trait Implementasyonları

**`Add for Gas`** (satır 65-76)
- Her boyutu bağımsız olarak toplar; bileşen toplamı

**`AddAssign for Gas`** (satır 78-82)
- `*self = *self + rhs` — `Add`'ı kullanır

**`Sum for Gas`** (satır 84-88)
- `iter.fold(Gas::zero(), |acc, x| acc + x)` — iterator toplamı
- `Gas::zero()` identity element olarak kullanılır

**`From<GasPrices> for pb::GasPrices`** (satır 142-157)
- Staking token asset_id ise `None` gönderir (optimizasyon)

**`TryFrom<pb::GasPrices> for GasPrices`** (satır 159-175)
- `asset_id: None` → staking token olarak yorumlar

### Fonksiyonlar

**`Gas::zero() -> Self`** (satır 55-63)
- Tüm boyutlar sıfır; identity element

**`GasPrices::zero() -> Self`** (satır 118-120)
- `Default` delegate eder; tüm fiyatlar sıfır

**`GasPrices::fee(&self, gas: &Gas) -> Fee`** (satır 122-135) — **KRİTİK FORMÜL**
```rust
let amount = Amount::from(
    (self.block_space_price * gas.block_space) / 1_000
        + (self.compact_block_space_price * gas.compact_block_space) / 1_000
        + (self.verification_price * gas.verification) / 1_000
        + (self.execution_price * gas.execution) / 1_000,
);
```
**Gas Fee Formülü:**
```
fee = Σ (price_i * gas_i) / 1000
```
- Her boyut için: `fiyat * kullanım / 1000`
- Toplam: dört boyutun ücret toplamı
- Zımni pay: 1000 (staking token'ın base biriminin 1/1000'i)
- Sonuç staking token (ya da belirtilen alt token) cinsinden döner

### Unsafe Bloklar

Yok.

### Testler

Yok.

### Tasarım Kararları

1. **Çok boyutlu gas**: Tek skaler yerine dört boyut — farklı işlem tiplerini gerçek maliyetleriyle fiyatlandırma imkânı. ZK ispat doğrulama (verification) ayrı boyut olarak modellenir, çünkü doğrulama maliyeti yürütme maliyetinden bağımsızdır.
2. **1000 paydası**: `u64` tabanlı kesirli aritmetik; 1 birim gas'ın 1/1000 staking token olması mümkün. Bu hassas küçük işlemler için önemli.
3. **Taşma riski**: `block_space_price * gas.block_space` u64 çarpması; teorik taşma riski mevcut ancak pratikte aşılmaz (u64::MAX / 1000 çok büyük bir değer).
4. **compact_block_space ayrımı**: Privacy blockchain özelliği — compact block istemci senkronizasyon yükünü ayrı bir gas boyutu olarak modeller. Ağır senkronizasyon oluşturan işlemleri ayrıca fiyatlandırır.

### Nova Shield Chain İçin Dersler

- **Çok boyutlu gas modeli Nova Shield için mükemmel uyum**: MEV koruması için ZK ispatları kullanılıyorsa `verification` boyutu kritik. Normal hesaplama ile ZK ispatının ayrı fiyatlandırılması kullanıcıya şeffaflık sağlar.
- `GasPrices::fee()` formülü `/ 1_000` ile kesirli aritmetik yapıyor — taşma koruması için checked_mul/checked_div düşünülebilir.
- `Sum for Gas` implementasyonu — işlem batch'lerinin toplam gas'ı hesaplanırken kullanılabilir.
- Nova Shield DEX'inde swap işlemleri, batch swap ZK ispatları için ek verification gas gerektirebilir; bu boyutu ayırmak swap kullanıcılarına dürüst fiyatlama sağlar.

---

## [component.rs] (fee)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/component.rs`
**Satır Sayısı:** 74

### Genel Amaç

`FeeComponent` ABCI lifecycle hook'larını implement eder. `init_chain`'de parametreleri yazar; `end_block`'ta birikmiş ücretleri toplar ve event olarak yayar.

### Struct/Enum/Type Listesi

**`FeeComponent`** (satır 20) — boş struct, lifecycle marker

### Trait Implementasyonları

**`Component for FeeComponent`** (satır 22-73)

**`init_chain`** (satır 27-34)
- `app_state: Some(genesis)` → `state.put_fee_params(genesis.fee_params.clone())`
- `None` → "upgrade specific check" yorumuyla no-op
- Genesis sırasında fee parametrelerini JMT'ye yazar

**`begin_block`** (satır 36-41)
- Tamamen boş — fee bileşeninin blok başında işi yoktur

**`end_block`** (satır 43-67)
- `Arc::get_mut(state).expect("unique ref in end_block")` — tekil referans gerektirir
- `state_ref.accumulated_base_fees_and_tips()` ile birikmiş ücretleri çeker
- Staking token için `(swapped_base, swapped_tip)` extract eder
- `swapped_total = swapped_base + swapped_tip`
- `EventBlockFees` event'ı yayar (toplam, base, tip ayrı ayrı)

**`end_epoch`** (satır 69-72) — `Ok(())` — epoch sonunda fee bileşeni pasif

### Fonksiyonlar

Sadece trait metotları.

### Tasarım Kararları

1. **end_block tekil referans baskısı**: `Arc::get_mut().expect()` — bu, fee bileşeninin blok sonunda tek başına state'e sahip olduğunu garanti eder; diğer bileşenlerle eş zamanlı yazma engellenir.
2. **Sadece staking token event'i**: `end_block`'ta yalnızca staking token ücreti event olarak yayılıyor; alternatif tokenlar şu an event'e dahil değil.
3. **base + tip ayrımı**: Proposer tip, base fee'den ayrı izleniyor — proposer incentive mekanizması için kritik.

### Nova Shield Chain İçin Dersler

- `end_block`'ta `Arc::get_mut()` pattern'i; Penumbra, state'i güvenli şekilde seri erişime zorlar. Nova Shield'de aynı pattern kullanılabilir.
- Proposer tip mekanizması (base fee + tip) Ethereum benzeri öncelik sıralaması için kullanılabilir.

---

## [fee_pay.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/component/fee_pay.rs`
**Satır Sayısı:** 78

### Genel Amaç

`FeePay` trait'i, bir işlem için gas tüketimini doğrular ve ücret ödemesini kaydeder. Alternatif token desteği, proposer tip ayrımı ve event kaydı burada gerçekleşir.

### Trait Implementasyonları

**`FeePay: StateWrite`** (satır 13-77)

**`pay_fee(&mut self, gas_used: Gas, fee: Fee) -> Result<()>`** (satır 16-74)

Algoritma adım adım:
1. **Gas fiyatı seçimi** (satır 17-34):
   - Ücret staking token ise → `get_gas_prices()` çağrısı
   - Alternatif token ise → `get_alt_gas_prices()` listesini lineer tara, asset_id eşleşen fiyatı bul
   - Eşleşme bulunamazsa → `anyhow!("fee token {} not recognized by the chain")`
   - NOT: Lineer tarama kabul edilebilir çünkü alt gas price listesinin küçük olması bekleniyor (yorum: satır 28-29)
2. **Varlık eşleşme doğrulaması** (satır 36-42):
   - `ensure!(current_gas_prices.asset_id == fee.asset_id(), ...)`
   - Double-check; zaten 1. adımda kontrol edildi ama güvenlik için tekrar
3. **Base fee hesabı** (satır 44-45):
   - `base_fee = current_gas_prices.fee(&gas_used)` — `GasPrices::fee()` çağrısı
4. **Minimum fee kontrolü** (satır 47-53):
   - `ensure!(fee.amount() >= base_fee.amount(), ...)`
   - Yeterli değilse hata: "fee must be greater than or equal to the transaction base price"
5. **Tip hesabı** (satır 55-59):
   - `tip = Fee { amount: fee.amount() - base_fee.amount(), asset_id: fee.asset_id() }`
6. **Event kaydı** (satır 61-67):
   - `pb::EventPaidFee { fee, base_fee, gas_used, tip }` yayınlanır
7. **Akümülasyon** (satır 69-71):
   - `self.raw_accumulate_base_fee(base_fee)` — block-end için biriktirir
   - `self.raw_accumulate_tip(tip)` — proposer tip için biriktirir

**`impl<S: StateWrite + ?Sized> FeePay for S {}`** (satır 77)
- Blanket impl — StateWrite implementedenler otomatik FeePay kazanır

### Unsafe Bloklar

Yok.

### Tasarım Kararları

1. **Lineer tarama kabulü**: Alt token listesinin küçük tutulması planlanıyor; ileride DEX'e geçiş öneriliyor (satır 28-29 yorum).
2. **Tip sıfır olabilir**: `fee == base_fee` durumunda tip = 0, hata verilmez.
3. **Double-check güvenlik katmanı**: `asset_id` eşleşmesi hem taramada hem de `ensure!`'de doğrulanır.
4. **Blanket impl**: StateWrite'ı implement eden her tip otomatik FeePay kazanır — Rust trait coherence'ının ergonomik kullanımı.

### Nova Shield Chain İçin Dersler

- Bu `pay_fee` akışı Nova Shield'in transaction yürütme pipeline'ına doğrudan adapte edilebilir.
- Alternatif token fee desteği: DEX integrasyonu ile swap fiyatı üzerinden hesaplama yapılabilir (Penumbra bu yolu gelecek için açık bırakmış).
- `raw_accumulate_base_fee` ve `raw_accumulate_tip` pattern'i, proposer rewards akışını clean bir şekilde modeller.

---

## [view.rs] (fee)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/component/view.rs`
**Satır Sayısı:** 111

### Genel Amaç

Fee state'inin okuma/yazma erişim katmanı. Gas fiyatları, birikmiş ücret akümülatörü ve parametre yönetimi burada tanımlanır.

### Trait Implementasyonları

**`StateReadExt: StateRead`** (satır 13-54)

**`get_fee_params(&self) -> Result<FeeParameters>`** (satır 15-19)
- JMT'den fee parametrelerini çeker
- Eksikse `anyhow!("Missing FeeParameters")`

**`get_gas_prices(&self) -> Result<GasPrices>`** (satır 21-29)
- `get_fee_params()` → `params.fixed_gas_prices` döndürür
- Yorum (satır 22-26): Dinamik gas fiyatlandırma implement edilince burası değişecek; şimdi governance parametrelerinden okunuyor

**`get_alt_gas_prices(&self) -> Result<Vec<GasPrices>>`** (satır 31-39)
- `get_fee_params()` → `params.fixed_alt_gas_prices` döndürür

**`gas_prices_changed(&self) -> bool`** (satır 41-45)
- Object store'da `gas_prices_changed` key'i var mı? → bool
- Compact block'ta gas fiyatı güncellemesi gerekip gerekmediğini bildirir

**`accumulated_base_fees_and_tips(&self) -> im::OrdMap<asset::Id, (Amount, Amount)>`** (satır 47-51)
- Object store'dan akümülatör map'i çeker
- `im::OrdMap` → immutable ordered map (im crate)
- Value: `(base_amount, tip_amount)` per asset_id

---

**`StateWriteExt: StateWrite`** (satır 56-110)

**`put_fee_params(&mut self, params: FeeParameters)`** (satır 59-63)
- JMT'ye fee parametrelerini yazar
- `object_put(state_key::gas_prices_changed(), ())` — değişim flag'i koyar

**`take_accumulated_base_fees_and_tips(&mut self) -> im::OrdMap<...>`** (satır 77-83)
- Akümülatörü okur, sıfırlar, eski değeri döndürür
- "Take" semantiği: Atomic read-and-reset

**`raw_accumulate_base_fee(&mut self, base_fee: Fee)`** (satır 85-95)
- `OrdMap::alter()` ile asset_id bazlı base fee biriktirme:
  - Key varsa: `(base + yeni_miktar, tip)` günceller
  - Key yoksa: `(yeni_miktar, Amount::zero())` ekler
- `im::OrdMap::alter()` fonksiyonel yaklaşım; persistent data structure avantajı

**`raw_accumulate_tip(&mut self, tip_fee: Fee)`** (satır 97-107)
- Aynı pattern, tip için: `(base, tip + yeni_miktar)` günceller

### Tasarım Kararları

1. **im::OrdMap kullanımı**: Immutable veri yapısı; `alter()` ile fonksiyonel güncelleme — yeni map oluşturulur, eski korunur. Snapshot isolation için ideal.
2. **Gas fiyatı değişim flag'i**: Governance param güncellemesi → flag set → compact block'a dahil et zinciri.
3. **Akümülatör multi-asset**: Her asset_id için bağımsız (base, tip) tuple — staking tokenın yanı sıra alt tokenlar da biriktirilebilir.

### Nova Shield Chain İçin Dersler

- `im::OrdMap` pattern'i, Nova Shield'de multi-asset fee akümülatörü için kullanılabilir.
- "Gas prices changed" flag mekanizması, istemcilere parametre güncellemelerini bildirmek için şık bir yaklaşım.

---

## [rpc.rs] (fee)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/component/rpc.rs`
**Satır Sayısı:** 39

### Genel Amaç

Fee query service gRPC server implementasyonu.

### Struct/Enum/Type Listesi

**`Server { storage: Storage }`** (satır 10)

### Trait Implementasyonları

**`QueryService for Server`** (satır 21-38)

**`current_gas_prices`** (satır 24-37)
- `storage.latest_snapshot()` → `get_gas_prices()` çağrısı
- NOT: Yanıtta `alt_gas_prices: Vec::new()` — TODO: henüz alt fiyatlar döndürülmüyor (eksik implementasyon)
- Cevap: `CurrentGasPricesResponse { gas_prices: Some(...), alt_gas_prices: [] }`

### Tasarım Kararları

**Eksik implementasyon**: `alt_gas_prices` alanı boş döndürülüyor — alt gas price sorgusu eksik.

### Nova Shield Chain İçin Dersler

- Alt gas price API'si eksik — Nova Shield'de başından itibaren tam implement edilmeli.

---

## [event.rs] (fee)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/event.rs`
**Satır Sayısı:** 49

### Genel Amaç

Blok sonu fee event'ini tanımlar. Üç ayrı fee alanı: toplam, base, tip.

### Struct/Enum/Type Listesi

**`EventBlockFees`** (satır 6-10)
```rust
pub struct EventBlockFees {
    pub swapped_fee_total: Fee,
    pub swapped_base_fee_total: Fee,
    pub swapped_tip_total: Fee,
}
```

### Tasarım Kararları

"Swapped" ön eki: Fees ile fee swap ayrımı için; isim biraz yanıltıcı. Buradaki "swapped" toplanan/işlenmiş anlamında.

---

## [genesis.rs] (fee)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/genesis.rs`
**Satır Sayısı:** 46

### Genel Amaç

Genesis zinciri başlangıç durumu için fee parametrelerini tanımlar.

### Struct/Enum/Type Listesi

**`Content { pub fee_params: FeeParameters }`** (satır 9-12)
- `Default` → `FeeParameters::default()` delegate eder

---

## [params.rs] (fee)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/params.rs`
**Satır Sayısı:** 46

### Genel Amaç

Fee parametreleri: birincil gas fiyatı ve alternatif token gas fiyatları listesi.

### Struct/Enum/Type Listesi

**`FeeParameters`** (satır 8-13)
```rust
pub struct FeeParameters {
    pub fixed_gas_prices: GasPrices,
    pub fixed_alt_gas_prices: Vec<GasPrices>,
}
```
- `Default`: Tümü sıfır (ücretsiz testnet başlangıcı için mantıklı)
- "Fixed" prefix: dinamik gas fiyatlandırma gelene kadar sabit

### Tasarım Kararları

İki alan: ana staking token fiyatı ve alternatif token listesi. Governance bu değerleri güncelleyebilir.

---

## [state_key.rs] (fee)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/state_key.rs`
**Satır Sayısı:** 16

### Genel Amaç

Fee bileşeni state anahtarlarını merkezi olarak tanımlar.

### Fonksiyonlar

```rust
pub fn fee_params() -> &'static str          // "fee/params"
pub fn gas_prices() -> &'static str          // "fee/gas_prices"
pub fn gas_prices_changed() -> &'static str  // "fee/gas_prices_changed"
pub fn fee_accumulator() -> &'static str     // "fee/accumulator"
```

### Tasarım Kararları

`gas_prices` anahtarı var ama kullanılmıyor — dinamik gas fiyatlandırma için rezerv edilmiş. Şu an parametrelerden okunuyor.

---

## [lib.rs] (fee)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/fee/src/lib.rs`
**Satır Sayısı:** 19

### Genel Amaç

Crate kök modülü. `#![deny(clippy::unwrap_used)]` — tüm crate'te `unwrap()` yasak.

### Tasarım Kararları

- `component` modülü feature-gated: `#[cfg(feature = "component")]` — lib kullanıcıları bileşen logicini dahil etmeden sadece tipleri kullanabilir.

---

## BÖLÜM 2: component/distributions (8 Dosya)

---

## [component.rs] (distributions)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/distributions/src/component.rs`
**Satır Sayısı:** 178

### Genel Amaç

Token dağıtım bileşeni. İki tür token ihracatı yönetir:
1. **Staking token ihracatı**: Her epoch sonu, bloklardaki staking ödülü hesaplanır
2. **LQT (Liquidity Tournament) ödülleri**: Her blok sonunda havuza eklenir, epoch sonunda ödüllendirilir

### Struct/Enum/Type Listesi

**`Distributions`** (satır 21) — boş struct

**`DistributionManager`** (satır 113-175) — private trait, StateWriteExt ile bağlı

### Trait Implementasyonları

**`Component for Distributions`** (satır 23-110)

**`init_chain`** (satır 27-35)
- Genesis parametrelerini yazar; `None` → no-op

**`begin_block`** (satır 37-42)
- Boş

**`end_block`** (satır 44-97)
- `Arc::get_mut()` → tekil referans zorunlu
- `get_current_epoch()` → epoch_index alınır
- `params.liquidity_tournament_incentive_per_block` → `increase: Amount`
- `params.liquidity_tournament_end_block` kontrolü:
  - `current_block >= end_block` ise → LQT ödülü sıfırlanır, event yayılır, return
  - Aksi halde → `increment_lqt_issuance(epoch_index, increase)` çağrısı
- `EventLqtPoolSizeIncrease { epoch_index, increase, new_total }` event kaydı

**`end_epoch`** (satır 99-109)
- `define_staking_budget()` çağrısı — epoch boyunca biriken staking ihracatı hesaplanır

---

**`DistributionManager: StateWriteExt`** (satır 113-177) — private trait

**`compute_new_staking_issuance(&self) -> Result<Amount>`** (satır 115-147)

**Kritik Formül:**
```
yeni_staking_issuance = staking_issuance_per_block * num_blocks_in_epoch
```
```rust
let num_blocks = current_block_height
    .checked_sub(current_epoch.start_height)
    .unwrap_or_else(|| panic!(...));

let staking_issuance_per_block = self.get_distributions_params().await?
    .staking_issuance_per_block as u128;

let new_staking_issuance_for_epoch = staking_issuance_per_block
    .checked_mul(num_blocks as u128)
    .expect("infallible unless issuance is pathological");
```
- `u128` kullanımı: `u64 * u64` taşma önlemi için genişletilir
- `checked_mul` zorunlu güvenlik
- TODO yorumu (satır 124-125): `staking_issuance_per_block`'ı `Amount` tipine dönüştürmek planlı

**`define_staking_budget(&mut self) -> Result<()>`** (satır 149-157)
- `compute_new_staking_issuance()` → `set_staking_token_issuance_for_epoch(amount)`
- Object store'a yazar; diğer bileşenler (staking) buradan okur

**`increment_lqt_issuance(&mut self, epoch_index: u64, by: Amount) -> Amount`** (satır 159-174)
- Mevcut LQT issuance'ı çek, `checked_add` ile artır, kaydet, yeni değeri döndür
- Nonverifiable storage kullanır (hızlı, Merkle'a dahil değil)

**`impl<T: StateWrite + ?Sized> DistributionManager for T {}`** (satır 177)
- Blanket impl

### Unsafe Bloklar

Yok.

### Tasarım Kararları

1. **LQT end_block parametresi**: `NonZeroU64` olarak tanımlanmış (0 = sonsuz). Bu, `Option<NonZeroU64>` ile `None = no end, Some(n) = ends at block n` semantiği sağlar.
2. **u128 çarpma**: `u64 * u64` taşma riski; Penumbra açıkça u128'e genişletiyor.
3. **checked_mul + expect**: Debug build'de panic, release'de olmaması beklenen taşma. Kritik ekonomik hesapta `expect` kullanımı tartışmalı — `checked_mul(...).ok_or()?` daha güvenli olurdu.
4. **Staking ve LQT ayrımı**: İki farklı token dağıtım mekanizması aynı bileşende; epoch-bazlı (staking) ve blok-bazlı (LQT).

### Nova Shield Chain İçin Dersler

- **LQT mekanizması ilgi çekici**: Blok başına ödül birikimi → epoch sonunda dağıtım; Nova Shield DEX için liquidity incentive mekanizması olarak adapte edilebilir.
- **Staking formülü** (`rate * num_blocks`) doğrudan kullanılabilir; Nova Shield'in kendi staking ihracat hesabı için template.
- `NonZeroU64` ile "bitiş bloğu" mekanizması şık — Nova Shield için zaman sınırlı ödül kampanyaları bu şekilde implement edilebilir.
- `u128` genişletme zorunluluğu göz önünde bulundurulmalı: büyük ekonomik hesaplamalarda `u64 * u64` daima taşabilir.

---

## [params.rs] (distributions)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/distributions/src/params.rs`
**Satır Sayısı:** 55

### Genel Amaç

Dağıtım parametreleri: staking ihracat hızı, LQT ödül hızı, LQT bitiş bloğu.

### Struct/Enum/Type Listesi

**`DistributionsParameters`** (satır 11-15)
```rust
pub struct DistributionsParameters {
    pub staking_issuance_per_block: u64,
    pub liquidity_tournament_incentive_per_block: u64,
    pub liquidity_tournament_end_block: Option<NonZeroU64>,
}
```

**Default değerleri** (satır 47-53):
```rust
staking_issuance_per_block: 1_000_000,
liquidity_tournament_incentive_per_block: 0,
liquidity_tournament_end_block: None,
```

### Protobuf Dönüşümü

`liquidity_tournament_end_block: Option<NonZeroU64>` ↔ Protobuf `u64`:
- `None` → `0` (Protobuf'ta yokluğun temsili)
- `Some(n)` → `n.get()`
- Deserialization: `NonZeroU64::new(msg.liquidity_tournament_end_block)` → `0 → None`, `n>0 → Some(NonZeroU64(n))`

### Tasarım Kararları

`NonZeroU64` kullanımı: Protobuf'ta "yokluk değeri" olarak 0 kullanmak yaygın ancak tip güvenliği açısından zayıf. Rust tarafında `NonZeroU64` ile bu çevirimi net bir şekilde yönetmek iyi pratik.

---

## [component/view.rs] (distributions)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/distributions/src/component/view.rs`
**Satır Sayısı:** 57

### Genel Amaç

Distributions state okuma/yazma erişimi.

### Trait Implementasyonları

**`StateReadExt: StateRead`** (satır 10-34)

- **`get_distributions_params`**: JMT'den `DistributionsParameters` çeker
- **`get_staking_token_issuance_for_epoch`**: Object store'dan `Amount` çeker (blok içi geçici veri)
- **`get_lqt_reward_issuance_for_epoch(epoch_index: u64)`**: Nonverifiable storage'dan `Amount` çeker; hata durumunda `None` döndürür ve tracing::error loglar

**`StateWriteExt: StateWrite + StateReadExt`** (satır 36-56)

- **`set_staking_token_issuance_for_epoch`**: Object store yazar
- **`put_distributions_params`**: JMT'ye yazar
- **`set_lqt_reward_issuance_for_epoch(epoch_index, issuance)`**: Nonverifiable storage'a yazar; key: `state_key::lqt::v1::budget::for_epoch(epoch_index)`

### Tasarım Kararları

LQT issuance verisi nonverifiable storage'da — Merkle proof gerektirmiyor, hız öncelikli.

---

## [component/state_key.rs] (distributions)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/distributions/src/component/state_key.rs`
**Satır Sayısı:** 27

### Genel Amaç

Distributions bileşeni state anahtarları.

### Fonksiyonlar

```rust
pub fn staking_token_issuance_for_epoch() -> &'static str
    // "distributions/staking_token_issuance_for_epoch"

pub mod lqt::v1::budget {
    pub fn prefix(epoch_index: u64) -> String
        // "distributions/lqt/v1/budget/{epoch_index:020}"

    pub fn for_epoch(epoch_index: u64) -> [u8; 48]
        // 48-byte key: prefix string olarak
}

pub fn distributions_parameters() -> &'static str
    // "distributions/parameters"
```

### Tasarım Kararları

1. **`:020` format**: 20 haneli zero-padded epoch index — lexicographic sıralama range query'leri için. Bu, `nonverifiable_range` sorguları için kritik.
2. **`[u8; 48]` fixed-size key**: LQT budget key'i sabit boyut dizisi olarak döndürülüyor — stack allocation, no heap.
3. **Versioned namespace** (`v1`): Gelecekteki LQT protokol güncellemeleri için versiyon izolasyonu.

---

## [component/rpc.rs] (distributions)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/distributions/src/component/rpc.rs`
**Satır Sayısı:** 81

### Genel Amaç

Distributions gRPC query server. İki endpoint: mevcut LQT pool büyüklüğü ve geçmiş epoch LQT issuance sorgusu.

### Trait Implementasyonları

**`DistributionsService for Server`** (satır 21-80)

**`current_lqt_pool_size`** (satır 22-55)

**Algoritma:**
```
pool_size = lqt_block_reward_rate * epoch_length
```
```rust
let epoch_length = current_block_height
    .checked_sub(current_epoch.start_height)
    .unwrap_or_else(|| panic!("epoch start height is greater..."));

let lqt_block_reward_rate = ...params.liquidity_tournament_incentive_per_block as u64;

let current_lqt_pool_size = lqt_block_reward_rate
    .checked_mul(epoch_length as u64)
    .expect("infallible unless issuance is pathological");
```
- Anlık hesaplama: tahmin değeri, nonverifiable'dan okunmuyor
- `checked_mul` + `expect` — yine aynı pattern

**`lqt_pool_size_by_epoch`** (satır 57-79)
- Belirtilen epoch için `get_lqt_reward_issuance_for_epoch(epoch_index)` çağrısı
- Nonverifiable storage'dan gerçek değer; mevcut değilse 404

### Tasarım Kararları

`current_lqt_pool_size` anlık hesaplama yapıyor — depolanan değer değil. Bu tutarsızlık: hem hesaplanan (RPC) hem de depolanan (nonverifiable) değer var. RPC daha "şimdiki" durumu tahmin ediyor, storage daha doğru ancak gecikmeli.

---

## [event.rs] (distributions)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/distributions/src/event.rs`
**Satır Sayısı:** 48

### Genel Amaç

LQT havuzu büyüklük artış event'i.

### Struct/Enum/Type Listesi

**`EventLqtPoolSizeIncrease`** (satır 8-12)
```rust
pub struct EventLqtPoolSizeIncrease {
    pub epoch_index: u64,
    pub increase: Amount,
    pub new_total: Amount,
}
```

`epoch_index` alanı protobuf'ta `epoch` olarak adlandırılmış — isim tutarsızlığı.

---

## [genesis.rs] / [lib.rs] (distributions)

Standart genesis ve crate root. `#![deny(clippy::unwrap_used)]`. Feature-gated `component` modülü.

---

## BÖLÜM 3: component/compact-block (10 Dosya)

---

## [compact_block.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/compact-block/src/compact_block.rs`
**Satır Sayısı:** 172

### Genel Amaç

Penumbra'nın privacy-korumalı light-client senkronizasyon formatı. Full blok yerine istemcinin kendi notlarını taraması için gereken minimum veri seti.

### Struct/Enum/Type Listesi

**`CompactBlock`** (satır 21-49)
```rust
pub struct CompactBlock {
    pub height: u64,
    pub state_payloads: Vec<StatePayload>,     // note/swap şifreli payloadları
    pub nullifiers: Vec<Nullifier>,             // harcanan notlar
    pub block_root: block::Root,               // SCT blok kökü
    pub epoch_root: Option<epoch::Root>,       // Epoch sonu varsa epoch kökü
    pub fmd_parameters: Option<fmd::Parameters>, // Fuzzy Message Detection parametreleri
    pub proposal_started: bool,               // Governance teklifi başladı mı?
    pub swap_outputs: BTreeMap<TradingPair, BatchSwapOutputData>, // DEX swap çıktıları
    pub app_parameters_updated: bool,         // App parametresi değişti mi?
    pub gas_prices: Option<GasPrices>,        // Gas fiyatı değişti ise yeni değer
    pub alt_gas_prices: Vec<GasPrices>,       // Alt token gas fiyatları değişti ise
    pub epoch_index: u64,                     // Mevcut epoch indeksi
}
```

**ÖNEMLI NOT (satır 45-48):** Yeni alan eklenmesi durumunda `requires_scanning` metodu güncellenmeli — bu bir invariant uyarısı.

### Fonksiyonlar

**`CompactBlock::requires_scanning(&self) -> bool`** (satır 72-81)
```rust
pub fn requires_scanning(&self) -> bool {
    !self.state_payloads.is_empty()   // note tarama
        || !self.nullifiers.is_empty() // nullifier toplama
        || self.fmd_parameters.is_some() // FMD param güncelleme
        || self.proposal_started       // governance teklifini işle
        || self.app_parameters_updated // app param yenile
        || self.gas_prices.is_some()   // gas fiyatı kaydet
        || !self.alt_gas_prices.is_empty() // alt gas fiyatı kaydet
}
```
- İstemci bu false döndüğünde bloğu atlayabilir
- `swap_outputs` ve `epoch_root` kontrol edilmiyor — istemci bunları her zaman işliyor mu? Potansiyel atlama bug'ı.

### Protobuf Dönüşümleri

`From<CompactBlock> for pb::CompactBlock` (satır 87-109):
- `block_root`: Boş finalized root ise `None` gönderilir (bant genişliği optimizasyonu)
- `swap_outputs`: `BTreeMap.into_values()` — key (TradingPair) atılır, value içinde trading_pair var

`TryFrom<pb::CompactBlock> for CompactBlock` (satır 111-152):
- `block_root: None` → `block::Finalized::default().root()` (default boş blok kökü)
- `swap_outputs`: Key yeniden türetilir: `(trading_pair, swap_output)` pair'i
- Tüm alanlar exhaustive try_from zinciri

**`From<CompactBlock> for CompactBlockRangeResponse`** (satır 154-159)
**`TryFrom<CompactBlockRangeResponse> for CompactBlock`** (satır 161-170)
- gRPC streaming response wrapper

### Tasarım Kararları

1. **Privacy-first tasarım**: İstemci tüm note payloadlarını dener (FMD ile filtreleme sonra), harcanan notları (nullifiers) kontrol eder. Full blok indirimi yok.
2. **Swap output özel durumu**: DEX swap sonuçları compact block'ta açık yer alır — swap istemciler fiyat bilgisine erişebilir.
3. **epoch_root opsiyonel**: Epoch geçişlerinde epoch kökü; normal bloklarda `None`.
4. **BTreeMap yerine Vec**: Protobuf swap_outputs Vec, Rust tarafı BTreeMap — decode sırasında key yeniden türetilir (ek işlem).
5. **`requires_scanning` optimizasyonu**: Boş blokları atlamak istemci senkronizasyonunu hızlandırır. Ancak swap_outputs ve epoch_root kontrolü eksik — bakılması gereken alan.

### Nova Shield Chain İçin Dersler

- Compact block konsepti Nova Shield için doğrudan kullanılabilir: full block indirimi yerine sadece gerekli veriler.
- `swap_outputs: BTreeMap<TradingPair, BatchSwapOutputData>` — DEX batch swap sonuçlarını compact block'a ekleme tasarımı Nova Shield DEX için şablon.
- `requires_scanning` optimizasyonu — senkronizasyon maliyetini düşürür; Nova Shield istemcileri için benzer mekanizma şart.
- FMD (Fuzzy Message Detection) ileri not: istemcinin hangi notların kendisine ait olduğunu öğrenmesi için istatistiksel yaklaşım — Nova Shield MEV korumasıyla ilgili.

---

## [state_payload.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/compact-block/src/state_payload.rs`
**Satır Sayısı:** 157

### Genel Amaç

Compact block içindeki bireysel payload'ları temsil eden enum. Her payload bir commitment kaynağına (source) sahip.

### Struct/Enum/Type Listesi

**`StatePayload`** (satır 15-28)
```rust
pub enum StatePayload {
    RolledUp {
        source: CommitmentSource,
        commitment: note::StateCommitment, // sadece hash, tam not yok
    },
    Note {
        source: CommitmentSource,
        note: Box<NotePayload>,    // şifreli not
    },
    Swap {
        source: CommitmentSource,
        swap: Box<SwapPayload>,    // şifreli swap
    },
}
```

**`StatePayloadDebugKind<'a>`** (satır 30-40)
- Yalnızca varyant adını yazdırır, içeriği değil — debug output'ta hassas veri sızmasını önler

### Fonksiyonlar

**`commitment(&self) -> &note::StateCommitment`** (satır 43-49)
- Her varyant için commitment referansı döndürür

**`source(&self) -> &CommitmentSource`** (satır 51-57)
- Her varyant için kaynak referansı döndürür

**From implementasyonları** (satır 59-85):
- `From<note::StateCommitment>` → `RolledUp { source: CommitmentSource::transaction() }`
- `From<(NotePayload, CommitmentSource)>` → `Note { note: Box::new(note), source }`
- `From<(SwapPayload, CommitmentSource)>` → `Swap { swap: Box::new(swap), source }`

### Tasarım Kararları

1. **RolledUp varyantı**: Doğrulama için commitment hash'i yeterli olan notlar; tam payload gönderilmez. Bu bant genişliği optimizasyonu — istemci hiçbir zaman bu payload'ı decrypt edemeyeceğini biliyor.
2. **Box kullanımı**: `NotePayload` ve `SwapPayload` büyük struct'lar; Box ile stack boyutu sabitlenir (enum varyantları arasında boyut farkını azaltır).
3. **Debug gizlilik**: `StatePayloadDebugKind` ile log output'ta veri sızmaması için kasıtlı kısıtlama.

### Nova Shield Chain İçin Dersler

- RolledUp pattern: ZK sistemlerde "commitment but no reveal" çok yaygın; Nova Shield'de benzer optimizasyon kullanılabilir.
- `StatePayloadDebugKind` pattern: Debug trait'in ayrı wrapper ile implement edilmesi — Nova Shield'de hassas veri içeren tiplerde standart olmalı.

---

## [component/manager.rs] (compact-block)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/compact-block/src/component/manager.rs`
**Satır Sayısı:** 155

### Genel Amaç

Blok/epoch sonu compact block oluşturma ve nonverifiable storage'a yazma.

### Trait Implementasyonları

**`CompactBlockManager: StateWrite`** (satır 17-29) — public API

- `finish_block(&mut self)` → `finalize_compact_block(false)`
- `finish_epoch(&mut self)` → `finalize_compact_block(true)`

**`Inner: StateWrite`** (satır 32-154) — private implementasyon

**`finalize_compact_block(&mut self, end_epoch: bool) -> Result<()>`** (satır 34-151)

**Adım adım algoritma:**
1. `get_block_height()` → height
2. `param_changes_for_height(height)` → `app_parameters_updated`; height==0 ise zorla true
3. `gas_prices_changed() || height == 0` → gas fiyatlarını compact block'a ekle
4. `height == 0` ise → FMD parametrelerini ekle (sadece genesis)
5. `proposal_started()` → governance flag
6. `end_sct_block(end_epoch)` → `(block_root, epoch_root)` — SCT'yi mühürler
7. Payload toplama:
   - `pending_note_payloads()` → source stripped → `(pos, (note, stripped_source).into())`
   - `pending_rolled_up_payloads()` → `(pos, commitment.into())`
   - `pending_swap_payloads()` → source stripped → `(pos, (swap, stripped_source).into())`
8. Tüm payload'ları `(pos, payload)` ile merge, `sort_by_key(pos)` ile sırala
9. Sıralamadan pozisyonları at, sadece payload listesi al
10. `pending_batch_swap_outputs()` → `swap_outputs`
11. `pending_nullifiers()` → `nullifiers`
12. `get_current_epoch().index` → `epoch_index`
13. `CompactBlock { ... }` oluştur
14. `nonverifiable_put_raw(state_key::compact_block(height), compact_block.encode_to_vec())`

### Tasarım Kararları

1. **Source stripping**: Compact block'a gönderilen payload'larda transaction ID'ler soyulur. İstemci "stripped" kaynak görünce tam bloğu indirmesi gerektiğini anlar.
2. **Pozisyona göre sıralama**: SCT pozisyonu sıralı tutulur — istemci ağaç traversal'ını doğru sırayla yapar.
3. **Nonverifiable storage**: Compact block Merkle ağacına dahil edilmez — hız öncelikli, proof gerektirmez.
4. **Genesis özel durumu**: Height 0'da her zaman FMD parametreleri ve gas fiyatları eklenir.

### Nova Shield Chain İçin Dersler

- **Payload sıralama önemli**: SCT pozisyonuna göre sıralı compact block, istemci tarafında doğru ağaç inşasını garanti eder.
- **Source stripping privacy mekanizması**: İşlem ID'lerinin compact block'tan çıkarılması, hangi notun hangi işleme ait olduğunu gizler.

---

## [component/rpc.rs] (compact-block)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/compact-block/src/component/rpc.rs`
**Satır Sayısı:** 273

### Genel Amaç

Compact block gRPC streaming server. İki önemli endpoint: tekil blok ve range streaming.

### Struct/Enum/Type Listesi

**`Server { storage: Storage }`** (satır 18-26)

**`CompactBlockConnectionCounter`** (satır 238-251)
- RAII guard: `new()` → metrics gauge increment; `drop()` → decrement
- Aktif bağlantı sayacı otomatik yönetimi

**`BlockSender`** (satır 253-272)
```rust
struct BlockSender {
    next_height: u64,
    inner: mpsc::Sender<Result<CompactBlock, tonic::Status>>,
}
```
- `send(&mut self, block: CompactBlock)` → height doğrulama + gönderme + next_height++
- Monoton blok yüksekliği garantisi; sıra dışı blok gönderilmez

### Trait Implementasyonları

**`QueryService for Server`** (satır 29-232)

**`compact_block(request: CompactBlockRequest)`** (satır 34-50)
- Tek blok: `storage.latest_snapshot().compact_block(height)` → gRPC response

**`compact_block_range(request: CompactBlockRangeRequest)`** (satır 52-232)

**İki fazlı akış algoritması:**

**Faz 1: Yakalama (catch-up)** (satır 115-158):
- `latest_snapshot.stream_compact_block(start_height)` → stream
- `while let Some(res) = cb_stream.next().await` → height <= end_height ise gönder
- `keep_alive` false ise biter

**Faz 2: Keep-alive** (satır 160-207):
- `rx_state_snapshot.borrow_and_update()` → son anlık görüntü; cur_height alınır
- `(end_height+1)..=cur_height` range'i için catch-up 2 (Faz 1 ve 2 arası boşluk)
- `loop { rx_state_snapshot.changed().await; ... }` — yeni bloklar gelince gönder

**Hata yönetimi** (satır 208-218):
- Async task içinde hatalar `tx_blocks_err.send(Err(...)).await` ile client'a iletilir

**Bağlantı boyunca metrics:**
- `COMPACT_BLOCK_RANGE_ACTIVE_CONNECTIONS`: aktif streaming bağlantı sayısı (gauge)
- `COMPACT_BLOCK_RANGE_SERVED_TOTAL`: toplam gönderilen blok (counter)

**Timeout yorumu** (satır 138-149): Daha önce 10s timeout vardı, kaldırıldı — bellek sızıntısı riski var ama client yanıt vermezse. Issue #2908/#2932 referansları.

### Tasarım Kararları

1. **İki fazlı streaming**: Catch-up + live streaming arası boşluk (Faz 2 yakalama) — race condition'ı önler.
2. **`BlockSender` wrapper**: Monoton blok sıralaması garantisi, assertion görevi görür.
3. **Channel boyutu 10**: `mpsc::channel(10)` — 10 blok buffer; düşük bellek kullanımı.
4. **RAII counter**: `Drop` trait ile bağlantı counter otomatik yönetimi — kayıpsız metrics.
5. **Chain ID kontrolü devre dışı** (satır 65-73): TODO yorum; güvenlik riski, ileride aktif edilmeli.

### Nova Shield Chain İçin Dersler

- **İki fazlı streaming** pattern'i Nova Shield'de tam replika edilebilir.
- `BlockSender` wrapper: Sıra dışı blok gönderimini önleyen tip güvenliği — Nova Shield'in P2P katmanında benzer yaklaşım.
- Chain ID kontrolünün devre dışı olması önemli güvenlik notu: Nova Shield baştan aktif etmeli.
- RAII gauge: `Drop` ile metrics counter yönetimi standart Rust pattern'i.

---

## [component/view.rs] (compact-block)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/compact-block/src/component/view.rs`
**Satır Sayısı:** 56

### Genel Amaç

Compact block nonverifiable storage okuma katmanı. Stream ve tekil erişim.

### Trait Implementasyonları

**`StateReadExt: StateRead`** (satır 13-55)

**`stream_compact_block(start_height: u64) -> Pin<Box<dyn Stream<...>>>`** (satır 20-37)
- `nonverifiable_range_raw(prefix, start_height..)` → raw bytes stream
- Lazy decode: `CompactBlock::decode(&mut v.as_slice())`
- Protobuf decode — not domain type (daha hızlı: proto doğrudan gRPC'ye gider)
- Return type: `Pin<Box<dyn Stream<...> + Send + 'static>>`

**`compact_block(height: u64) -> Result<Option<CompactBlock>>`** (satır 39-52)
- Tekil fetch: `nonverifiable_get_raw` → `CompactBlock::decode`

### Tasarım Kararları

1. **Proto decode, domain decode değil**: `CompactBlock` proto tipi döndürülüyor çünkü hemen gRPC'ye gönderilecek; domain tipine dönüştürme gereksiz.
2. **Range query**: `nonverifiable_range_raw` ile verimli blok aralığı sorgusu; lexicographic key sıralama sayesinde yükseklik sırası korunur.
3. **`expect` in decode**: Nonverifiable'dan gelen veri kendi serileştirdiği veri; `expect` güvenli kabul ediliyor.

---

## [component/metrics.rs] (compact-block)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/compact-block/src/component/metrics.rs`
**Satır Sayısı:** 38

### Genel Amaç

Metrics kayıt ve sabit isimleri. Monkey-patch trick: `use crate::component::metrics;` ile `metrics` crate'ini shadow'lar.

### Sabitler

```rust
pub const COMPACT_BLOCK_RANGE_ACTIVE_CONNECTIONS: &str =
    "cnidarium_component_compact_block_compact_block_range_active_connections";

pub const COMPACT_BLOCK_RANGE_SERVED_TOTAL: &str =
    "cnidarium_component_compact_block_compact_block_range_served_total";
```

### Fonksiyonlar

**`register_metrics()`** — gauge ve counter tanımlar

### Tasarım Kararları

Metrics sabitlerini string literal yerine const olarak tutmak: yazım hatası önleme + refactoring kolaylığı.

---

## [state_key.rs] (compact-block)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/compact-block/src/state_key.rs`
**Satır Sayısı:** 15

```rust
pub fn compact_block(height: u64) -> String
    // "compactblock/{height:020}"
pub fn prefix() -> &'static str
    // "compactblock/"
pub fn height(height: u64) -> String
    // "{height:020}"  (20-digit zero-padded)
```

**`:020` formatı**: 20 haneli zero-padding — lexicographic sıralamada sayısal sıralama sağlar. `nonverifiable_range_raw` range query'leri için şart.

---

## [event.rs] (compact-block)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/compact-block/src/event.rs`
**Satır Sayısı:** 2

Boş dosya — compact block için event tanımlanmamış.

---

## [lib.rs] (compact-block)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/compact-block/src/lib.rs`
**Satır Sayısı:** 16

```rust
pub use compact_block::CompactBlock;
pub use state_payload::{StatePayload, StatePayloadDebugKind};
```

---

## BÖLÜM 4: component/sct (15 Dosya)

---

## [component/clock.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/component/clock.rs`
**Satır Sayısı:** 129

### Genel Amaç

Blockchain'in mantıksal ve fiziksel saatlerini yönetir: blok yüksekliği, blok zaman damgası, epoch indeksi, erken epoch bitişi.

### Trait Implementasyonları

**`EpochRead: StateRead`** (satır 10-89)

**`get_block_height(&self) -> Result<u64>`** (satır 15-19)
- `get_proto(state_key::block_manager::block_height())` → JMT'den u64

**`get_current_block_timestamp(&self) -> Result<tendermint::Time>`** (satır 21-36)
- JMT'den RFC3339 string → `tendermint::Time::from_str()`

**`get_block_timestamp(&self, height: u64) -> Result<tendermint::Time>`** (satır 38-57)
- Nonverifiable storage'dan tarihi blok zaman damgası
- Historical query için NV storage tercih edilmiş — Merkle proof gereksiz, hız öncelikli

**`get_current_epoch(&self) -> Result<Epoch>`** (satır 59-70)
- `get_block_height()` → `get(state_key::epoch_manager::epoch_by_height(height))`
- Mevcut yüksekliğe göre epoch bulunur

**`get_epoch_by_height(&self, height: u64) -> Result<Epoch>`** (satır 72-80)
- Belirtilen yükseklik için epoch

**`is_epoch_ending_early(&self) -> bool`** (satır 82-87)
- Object store'dan flag okuma; `unwrap_or(false)`

---

**`EpochManager: StateWrite`** (satır 95-128)

**`put_block_timestamp(&mut self, height: u64, timestamp: tendermint::Time)`** (satır 99-109)
- RFC3339 string olarak JMT'ye (current) + NV storage'a (historical) yazar
- İki ayrı yere yazma: JMT için anlık, NV storage için geçmiş sorguları

**`set_end_epoch_flag(&mut self)`** (satır 111-115)
- Object store'a `true` yazar — erken epoch bitişi tetikler

**`put_block_height(&mut self, height: u64)`** (satır 117-121)
- JMT'ye yazılır; consensus tarafından doğrulanabilir

**`put_epoch_by_height(&mut self, height: u64, epoch: Epoch)`** (satır 123-126)
- `epoch_by_height/{height}` key'iyle JMT'ye yazar

### Tasarım Kararları

1. **Dual-write zaman damgası**: Güncel zaman damgası JMT'de (consensus doğrulaması için), tarihsel zaman damgaları NV storage'da (hız için).
2. **Erken epoch bitişi**: `set_end_epoch_flag()` ile herhangi bir bileşen epoch'u erken bitirmeye zorlanabilir (örn: governance).
3. **Blanket impl**: `EpochRead` ve `EpochManager` tüm StateRead/StateWrite implementasyonlarına otomatik eklenir.

### Nova Shield Chain İçin Dersler

- **Epoch-height indexing**: Her yükseklik için epoch saklamak, geçmiş epoch sorgulamayı O(1) yapar — Nova Shield'de ödül geçmişi, staking snapshot gibi özellikler için şart.
- **Dual-write pattern**: JMT = consensus doğrulaması; NV storage = hız. Nova Shield'de hangi verinin hangisinde tutulacağı önemli bir tasarım kararı.

---

## [component/sct.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/component/sct.rs`
**Satır Sayısı:** 104

### Genel Amaç

SCT bileşeni ABCI lifecycle ve SCT parametrelerine okuma/yazma erişimi.

### Trait Implementasyonları

**`Component for Sct`** (satır 18-70)

**`init_chain`** (satır 22-47)
- `put_sct_params(genesis.sct_params)`
- `put_block_height(0)`
- `put_epoch_by_height(0, Epoch { index: 0, start_height: 0 })` — genesis için
- `put_epoch_by_height(1, Epoch { index: 0, start_height: 0 })` — ilk blok için
- Neden blok 1 de? `end_block` height-based epoch set eder; init_chain'den sonra end_block çağrılmaz, bu yüzden blok 1 de set edilmeli.

**`begin_block`** (satır 49-57)
- `put_block_height(height)` — Her blok başında yükseklik güncellenir
- `put_block_timestamp(height, time)` — Zaman damgası kaydedilir

**`end_block`, `end_epoch`** — boş

**`StateReadExt: StateRead`** (satır 73-91)
- `get_sct_params()` → JMT'den `SctParameters`
- `get_epoch_duration_parameter()` → `params.epoch_duration`

**`StateWriteExt: StateWrite`** (satır 96-103)
- `put_sct_params(params)` → JMT'ye yazar

### Tasarım Kararları

Genesis'te hem height 0 hem height 1 için epoch set etmek: `end_block` sonradan epoch indeksini height'a göre ayarlar, ancak init_chain → begin_block arası dönemde blok 1 epoch'u bilinmeli.

---

## [component/tree.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/component/tree.rs`
**Satır Sayısı:** 262

### Genel Amaç

State Commitment Tree (SCT) yönetimi: tree okuma/yazma/cache, commitment ekleme, nullifier kayıtlama, blok/epoch mühürleme.

### Trait Implementasyonları

**`SctRead: StateRead`** (satır 15-57)

**`get_sct(&self) -> tct::Tree`** (satır 18-34)
```rust
// 1. Object store cache'ini kontrol et
if let Some(tree) = self.object_get(state_key::cache::cached_state_commitment_tree()) {
    return tree;
}
// 2. Nonverifiable storage'dan bincode ile deserialize et
// 3. Yoksa yeni boş ağaç oluştur
```
Cache-first okuma stratejisi: object store → NV storage → yeni tree.

**`get_anchor_by_height(&self, height: u64) -> Result<Option<tct::Root>>`** (satır 36-40)
- JMT'den SCT kökü — consensus doğrulaması için JMT'de.

**`spend_info(&self, nullifier: Nullifier) -> Result<Option<NullificationInfo>>`** (satır 42-49)
- `spent_nullifier_lookup(nullifier)` key'iyle JMT'den okuma
- Double-spend önlemek için JMT'de (Merkle kanıtlanabilir)

**`pending_nullifiers(&self) -> im::Vector<Nullifier>`** (satır 51-54)
- Object store'dan mevcut blok nullifier listesi

---

**`SctManager: StateWrite`** (satır 60-221)

**`write_sct(&mut self, height, sct, block_root, epoch_root)`** (satır 67-115)
```rust
// 1. sct.root() → sct_anchor
// 2. JMT'ye: anchor_lookup(sct_anchor) → height (ters indeks)
// 3. JMT'ye: anchor_by_height(height) → sct_anchor (ileri indeks)
// 4. EventAnchor yayınla
// 5. EventBlockRoot yayınla
// 6. epoch_root varsa EventEpochRoot yayınla
// 7. write_sct_cache(sct)
// 8. persist_sct_cache()
```
İki yönlü indeks: `anchor → height` ve `height → anchor` — her iki yönden O(1) sorgulama.

**`add_sct_commitment(&mut self, commitment, source) -> Result<tct::Position>`** (satır 117-133)
```rust
let mut tree = self.get_sct().await;
let position = tree.insert(tct::Witness::Forget, commitment)?;
self.write_sct_cache(tree);
self.record_proto(event::commitment(commitment, position, source));
Ok(position)
```
- `tct::Witness::Forget` — commitment eklenir ama Merkle yolu hatırlanmaz (compact)
- Position döndürülür; nullifier türetme için gerekli

**`nullify(&mut self, nullifier, source)`** (satır 136-159)
```rust
// 1. JMT'ye NullificationInfo { id: source.id(), spend_height } yaz
// 2. pending_nullifiers'a ekle (object store)
```

**`end_sct_block(&mut self, end_epoch: bool) -> Result<(block::Root, Option<epoch::Root>)>`** (satır 165-192)
```rust
let mut tree = self.get_sct().await;
let block_root = tree.end_block().expect("cannot fail");
let epoch_root = if end_epoch {
    Some(tree.end_epoch().expect("cannot fail"))
} else {
    None
};
self.write_sct(height, tree, block_root, epoch_root).await;
Ok((block_root, epoch_root))
```
Blok bitişi: Her blok için blok kökü üretilir. Epoch bitişi ek olarak epoch kökü üretir.

**`write_sct_cache(&mut self, tree)`** (satır 194-198)
- Object store'a yazar — ucuz (in-memory)

**`persist_sct_cache(&mut self)`** (satır 200-218)
- Object store'daki tree var mı? → `bincode::serialize` → nonverifiable_put_raw
- "persist" aslında NV storage'a flush; diske gitmesi cnidarium'un commit aşamasında olur

---

**`VerificationExt: StateRead`** (satır 223-261)

**`check_claimed_anchor(&self, anchor: tct::Root) -> Result<()>`** (satır 225-242)
```rust
if anchor.is_empty() { return Ok(()); }  // Boş anchor her zaman geçerli
if self.get_proto::<u64>(&state_key::tree::anchor_lookup(anchor)).await?.is_some() {
    Ok(()) // Geçerli anchor
} else {
    Err(anyhow!("provided anchor {} is not a valid SCT root", anchor))
}
```

**`check_nullifier_unspent(&self, nullifier: Nullifier) -> Result<()>`** (satır 244-258)
```rust
if let Some(info) = self.get::<NullificationInfo>(...).await? {
    anyhow::bail!("nullifier {} was already spent in {:?}", nullifier, hex::encode(info.id));
}
Ok(())
```

### Tasarım Kararları

1. **Cache-first SCT okuma**: Object store (in-memory) → NV storage (disk). Her blokta ağaç yüzlerce kez okunabilir; cache kritik.
2. **`Witness::Forget`**: Tüm commitmentlar Forget modunda ekleniyor — sadece leaf hash'i saklanır, tam yol değil. İstemci kendi yolunu saklıyor (self-custody).
3. **İki yönlü anchor indeksi**: `anchor → height` (anchor'ın geçerliliği anında kontrol) ve `height → anchor` (belirli yükseklikteki state commitment kanıtı).
4. **`end_block` asla panic vermez**: `tree.end_block().expect("cannot fail")` — bu işlem matematiksel olarak başarısız olamaz (tam ağaç durumu hariç).
5. **Dual commit semantiği**: `write_sct_cache` (object store, ucuz) + `persist_sct_cache` (NV flush, pahalı) — transaction boyunca birden fazla ağaç güncellemesi önce cache'e, blok sonunda persist.

### Nova Shield Chain İçin Dersler

- **Cache-first pattern**: Nova Shield'in SCT benzeri yapısı için in-memory cache şart. Her blokta defalarca okunan kritik veri için bu mimari zorunlu.
- **`Witness::Forget`**: Privacy sistemlerde sunucu tarafı Merkle yolu saklamamak — istemci kendi witness'ını tutar. Nova Shield ZK sistemi için aynı yaklaşım.
- **İki yönlü indeks**: Hem `anchor → height` hem `height → anchor` indeksini tutmak (Anchor kanıtlama + snapshot sorgulama için) iyi pratik.
- **Nullifier JMT'de**: Double-spend kanıtlanabilir olmalı → JMT zorunlu. Bu bir temel kural.

---

## [component/source.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/component/source.rs`
**Satır Sayısı:** 30

### Genel Amaç

İşlem yürütme sırasında ambient transaction ID'yi state'e bağlamak için helper trait.

### Trait Implementasyonları

**`SourceContext: StateWrite`** (satır 9-29)

- `put_current_source(source: Option<TransactionId>)` — Var ise object_put, yok ise object_delete
- `get_current_source() -> Option<TransactionId>` — Object store okuma
- `put_mock_source(counter: u8)` — Test için: `TransactionId([counter; 32])`

### Tasarım Kararları

Ambient source pattern: İşlem yürütülürken tüm alt operasyonlar (note oluşturma, swap vb.) mevcut işlem ID'sine erişebilir. State'e yazılır/silinir — block scope'lu.

---

## [component/rpc.rs] (sct)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/component/rpc.rs`
**Satır Sayısı:** 146

### Genel Amaç

SCT query gRPC server. Dört endpoint: epoch by height, anchor by height, timestamp by height, SCT frontier.

### Trait Implementasyonları

**`QueryService for Server`** (satır 29-145)

**`epoch_by_height`** (satır 31-45): Epoch bilgisini döndürür

**`anchor_by_height`** (satır 47-62): Belirtilen height için SCT anchor (Merkle kökü)

**`timestamp_by_height`** (satır 64-84)
- Blok zaman damgası (NV storage'dan)
- `chrono::DateTime::parse_from_rfc3339` → `Timestamp { seconds, nanos }`

**`sct_frontier`** (satır 86-144) — **EN KARMAŞİK ENDPOINT**
```rust
let frontier = state.get_sct().await;

let (anchor, maybe_proof) = if !with_proof {
    (frontier.root(), None)
} else {
    let anchor_key = state_key::tree::anchor_by_height(current_height).as_bytes().to_vec();
    let (maybe_raw_anchor, proof) = state.get_with_proof(anchor_key).await?;
    // ... anchor parse, proof wrap
};

// Sanity check: frontier.root() == anchor
if anchor != locked_anchor {
    return Err(tonic::Status::internal("anchor mismatch: ..."));
}

let raw_frontier = bincode::serialize(&frontier)?;
SctFrontierResponse { height, anchor, compact_frontier: raw_frontier, proof }
```

İki mod:
- `with_proof = false`: Sadece frontier ve kök
- `with_proof = true`: JMT Merkle inclusion proof ile birlikte

Sanity check: get_sct().root() == JMT'deki anchor — tutarlılık doğrulaması.

### Tasarım Kararları

`sct_frontier` endpoint'i ışık istemcilerin (light clients) SCT'yi bootstrap etmesine olanak tanır — tam senkronizasyon yerine anlık frontier alma.

---

## [nullifier.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/nullifier.rs`
**Satır Sayısı:** 167

### Genel Amaç

Nullifier türetimi ve ZK devre değişkeni. **Penumbra'nın double-spend önleme mekanizmasının kalbi.**

### Struct/Enum/Type Listesi

**`Nullifier(pub Fq)`** (satır 15)
- `Fq` — decaf377 eliptik eğrisinin baz alanı (`BLS12-377` bazlı, 254-bit)
- `Copy + Clone + PartialEq + Eq + Hash + PartialOrd + Ord + Serialize + Deserialize`

**`NullifierVar`** (satır 105-107)
- ZK devre için R1CS değişken tipi
- `inner: FqVar` — arkworks R1CS constraint variable

**Sabit:**
```rust
pub static NULLIFIER_DOMAIN_SEP: Lazy<Fq> = Lazy::new(|| {
    Fq::from_le_bytes_mod_order(blake2b_simd::blake2b(b"penumbra.nullifier").as_bytes())
});
```
- Domain separatör: `blake2b("penumbra.nullifier")` → Fq alanına indirgen
- `Lazy` = once_cell; program boyunca bir kez hesaplanır

### Fonksiyonlar

**`Nullifier::derive(nk, pos, state_commitment) -> Nullifier`** (satır 69-78)

**KRİTİK FORMÜL — Nullifier Türetimi:**
```
nullifier = Poseidon3(NULLIFIER_DOMAIN_SEP, nk, state_commitment, pos)
```
```rust
Nullifier(hash_3(
    &NULLIFIER_DOMAIN_SEP,
    (nk.0, state_commitment.0, (u64::from(pos)).into()),
))
```
Parametreler:
- `nk: &NullifierKey` — spending key'den türetilen özel anahtar
- `pos: tct::Position` — SCT'deki konum (epoch, block, index üçlüsü)
- `state_commitment: &StateCommitment` — notun veya swap'ın commitment'ı
- `NULLIFIER_DOMAIN_SEP` — domain ayrımı

Poseidon377 hash_3: Üç girişli Poseidon hash; ZK-friendly tasarlanmış.

**`NullifierVar::derive(nk, position, state_commitment) -> Result<NullifierVar, SynthesisError>`** (satır 147-165)

**ZK Devre İçin Aynı Formül:**
```rust
let domain_sep = FqVar::new_constant(cs.clone(), *NULLIFIER_DOMAIN_SEP)?;
let nullifier = poseidon377::r1cs::hash_3(
    cs,
    &domain_sep,
    (nk.inner.clone(), state_commitment.inner.clone(), position.position.clone()),
)?;
Ok(NullifierVar { inner: nullifier })
```
Arkworks R1CS üzerinde constraint olarak aynı Poseidon377 hesabı.

**`AllocVar<Nullifier, Fq> for NullifierVar`** (satır 109-126):
- `AllocationMode::Input` destekleniyor (public input)
- `AllocationMode::Constant` ve `Witness` → `unimplemented!()`

**`R1CSVar<Fq> for NullifierVar`** (satır 128-138)
**`EqGadget<Fq> for NullifierVar`** (satır 140-144)

### Unsafe Bloklar

Yok.

### Tasarım Kararları

1. **Poseidon377**: ZK-friendly hash; R1CS constraint sayısı SHA256'ya kıyasla çok az. Hem native hem devre içi aynı fonksiyon.
2. **Üçlü hash girişi (nk, commitment, pos)**: Tüm üç bileşen gerekli — nullifier sadece "bu not bir kez harcanabilir" değil, "bu not bu konumdan bu anahtarla harcanabilir" demek. Aynı notun farklı konumlar gibi gösterilmesi önlenir.
3. **`Lazy<Fq>` domain sep**: Once-cell ile hesaplanır; blake2b → Fq dönüşüm pahalı değil ama determinizm önemli.
4. **unimplemented! için Constant ve Witness**: Nullifier'ın ZK devresinde sadece public input (kanıtlanacak değer) olarak kullanılması tasarım kararı — witness/constant olması anlamsız.

### Nova Shield Chain İçin Dersler

- **Nullifier formülü doğrudan adapte edilebilir**: Nova Shield ZK spend/DEX sistemi için `Poseidon3(domain_sep, spending_key, commitment, position)` aynı güvenlik özelliklerini sağlar.
- **Domain separator zorunlu**: Farklı sistemlerde aynı hash fonksiyonu çakışmasını önler. Nova Shield'de her önemli hash türü için ayrı domain sep tanımlanmalı.
- **Blake2b → Fq indirgen**: Domain sep türetimi için güvenli yöntem; Nova Shield'de aynı pattern.
- **`EqGadget` implement etmek**: ZK devresinde eşitlik karşılaştırması için gerekli. Nova Shield ZK devrelerinde tüm kritik tiplerin `EqGadget` implement etmesi şart.

---

## [epoch.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/epoch.rs`
**Satır Sayısı:** 40

### Genel Amaç

Epoch veri tipi ve zamanlaması.

### Struct/Enum/Type Listesi

**`Epoch`** (satır 7-10)
```rust
pub struct Epoch {
    pub index: u64,
    pub start_height: u64,
}
```
`Debug + Eq + PartialEq + Ord + PartialOrd + Copy + Clone + Serialize + Deserialize`

### Fonksiyonlar

**`Epoch::is_scheduled_epoch_end(&self, current_height: u64, epoch_duration: u64) -> bool`** (satır 36-38)
```rust
current_height - self.start_height >= epoch_duration - 1
```
**Zamanlaması:**
- Epoch başlangıcından bu yana geçen blok sayısı `epoch_duration - 1`'e ulaşınca epoch biter
- Yani: `epoch_duration` blokluk epoch → başlangıç bloğu dahil

### Tasarım Kararları

Epoch bitişi `>= epoch_duration - 1` (son blok dahil). Bu "soft target" — erken bitiş de mümkün (governance, upgrade vb.)

---

## [source.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/source.rs`
**Satır Sayısı:** 150

### Genel Amaç

`CommitmentSource` enum'u — her SCT commitment'ının kaynağı.

### Struct/Enum/Type Listesi

**`CommitmentSource`** (satır 8-39)
```rust
pub enum CommitmentSource {
    Genesis,
    Transaction { id: Option<[u8; 32]> },
    FundingStreamReward { epoch_index: u64 },
    CommunityPoolOutput,
    Ics20Transfer { packet_seq: u64, channel_id: String, sender: String },
    LiquidityTournamentReward { epoch: u64, tx_hash: TransactionId },
}
```

### Fonksiyonlar

**`CommitmentSource::transaction() -> Self`** (satır 47-49)
- `Transaction { id: None }` — stripped version

**`CommitmentSource::stripped(&self) -> Self`** (satır 51-57)
- Transaction → id'yi None'a ayarla; diğer varyantlar clone

**`CommitmentSource::id(&self) -> Option<[u8; 32]>`** (satır 59-64)
- Sadece `Transaction { id: Some(id) }` → `Some(id)`, rest → None

### Tasarım Kararları

1. **6 farklı kaynak tipi**: Genesis, işlem, funding stream, community pool, ICS20 transfer, LQT ödül. Her birinin farklı metadata'sı var.
2. **Optional transaction ID**: `Transaction { id: Option<[u8; 32]> }` — compact block için stripped (id=None), tam blok için hydrated (id=Some).
3. **ICS20 transfer metadatası**: `packet_seq`, `channel_id`, `sender` — cross-chain kaynak izleme.
4. **LQT transaction hash**: LQT ödülü hangi oy transaction'ından kaynaklandığını izler.

### Nova Shield Chain İçin Dersler

- `CommitmentSource` pattern'i Nova Shield'de her note/commitment'ın kaynağını izlemek için kullanılabilir — audit trail ve MEV koruması için değerli.
- ICS20 transfer source, Nova Shield'in IBC entegrasyonu için şablon.

---

## [nullification_info.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/nullification_info.rs`
**Satır Sayısı:** 37

### Genel Amaç

Harcanan nullifier hakkında bilgi: hangi işlemde, hangi yükseklikte harcanmış.

### Struct/Enum/Type Listesi

**`NullificationInfo`** (satır 7-10)
```rust
pub struct NullificationInfo {
    pub id: [u8; 32],       // işlem ID'si
    pub spend_height: u64,  // harcandığı blok yüksekliği
}
```

### Tasarım Kararları

JMT'ye yazılır; Merkle kanıtı sunulabilir. Kullanım: double-spend kanıtlamasında (fraud proof).

---

## [params.rs] (sct)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/params.rs`
**Satır Sayısı:** 46

### Struct/Enum/Type Listesi

**`SctParameters`** (satır 8-13)
```rust
pub struct SctParameters {
    pub epoch_duration: u64,  // Varsayılan: 17280 blok
}
```

**Default değer** (satır 38-44):
```rust
epoch_duration: 17280  // 5s blok süresi varsayımıyla yaklaşık 1 gün
```
- `17280 blok * 5 saniye = 86400 saniye = 1 gün`

---

## [event.rs] (sct)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/event.rs`
**Satır Sayısı:** 119

### Genel Amaç

SCT ile ilgili eventler: anchor, blok kökü, epoch kökü, commitment.

### Struct/Enum/Type Listesi

**Fonksiyon olarak tanımlanan eventler:**

**`anchor(height, anchor, timestamp) -> pb::EventAnchor`** (satır 13-22)
- SCT kökü event'i: her blok sonunda

**`EventBlockRoot`** (satır 24-64)
```rust
pub struct EventBlockRoot {
    pub height: u64,
    pub root: block::Root,
    pub timestamp_seconds: i64,
}
```

**`EventEpochRoot`** (satır 66-106)
```rust
pub struct EventEpochRoot {
    pub index: u64,
    pub root: epoch::Root,
    pub timestamp_seconds: i64,
}
```

**`commitment(commitment, position, source) -> pb::EventCommitment`** (satır 108-118)
- SCT'ye eklenen her commitment için event

### Tasarım Kararları

Her commitment için event kaydediliyor — indexer'lar tam commitment geçmişini takip edebilir. Block/epoch root ayrı event tipleri: farklı cadans (her blok vs her epoch).

---

## [genesis.rs] / [lib.rs] (sct)

Standart genesis (satır sayısı: 38) ve crate root. `#![deny(clippy::unwrap_used)]`. Eksport edilen tipler:
```rust
pub use nullification_info::NullificationInfo;
pub use nullifier::{Nullifier, NullifierVar};
pub use source::CommitmentSource;
```

---

## [state_key.rs] (sct)

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/core/component/sct/src/state_key.rs`
**Satır Sayısı:** 76

### Genel Amaç

SCT bileşeni tüm state anahtarlarının merkezi tanımı.

### Fonksiyonlar ve Modüller

```
config::
  sct_params() → "sct/config/sct_params"

block_manager::
  block_height() → "sct/block_manager/block_height"
  current_block_timestamp() → "sct/block_manager/block_timestamp"
  block_timestamp(height) → "sct/block_manager/historical_block_timestamp/{height}"

epoch_manager::
  epoch_by_height(height) → "sct/epoch_manager/epoch_by_height/{height}"
  epoch_change_at_height(height) → "sct/epoch_manager/pending_epoch_changes/{height}"
  end_epoch_early() → "sct/epoch_manager/end_epoch_early"

nullifier_set::
  spent_nullifier_lookup(nullifier) → "sct/nullifier_set/spent_nullifier_lookup/{nullifier}"
  pending_nullifiers() → "sct/nullifier_set/pending_nullifiers"

tree::
  anchor_by_height(height) → "sct/tree/anchor_by_height/{height}"
  anchor_lookup(anchor) → "sct/tree/anchor_lookup/{anchor}"
  state_commitment_tree() → "sct/tree/state_commitment_tree"
  note_source(commitment) → "sct/tree/note_source/{commitment}"

cache::
  cached_state_commitment_tree() → "sct/cache/cached_state_commitment_tree"

ambient::
  current_source() → "sct/ambient/current_source"
```

### Tasarım Kararları

1. **Hiyerarşik namespace**: `sct/submodule/entity/{key}` yapısı; range query ve debug için okunabilir.
2. **`epoch_change_at_height` tanımlı ama kullanılmıyor**: Planlı ama implement edilmemiş özellik.
3. **`note_source` tanımlı ama kullanılmıyor**: Legacy veya gelecek özellik.
4. **`pending_epoch_changes`**: Gelecekte height-based epoch güncelleme mekanizması için rezerv.

---

## Genel Mimari Analiz

### Bileşen Bağımlılık Grafiği

```
SCT Component
    ├── block height / timestamps  ←── begin_block (her blok)
    ├── epoch indeks              ←── end_epoch (her epoch sonu)
    ├── tct::Tree (penumbra-tct) ←── add_sct_commitment, end_sct_block
    └── Nullifier set            ←── nullify, check_nullifier_unspent

Compact Block Component
    ├── SCT (end_sct_block → block_root/epoch_root)
    ├── Fee (gas_prices_changed?)
    ├── Shielded Pool (pending_note_payloads)
    ├── DEX (pending_batch_swap_outputs, pending_swap_payloads)
    └── Governance (proposal_started, param_changes)

Fee Component
    ├── FeeParameters (governance güncelleyebilir)
    ├── GasPrices (fixed → gelecekte dynamic)
    └── Accumulator (base_fee + tip → end_block event)

Distributions Component
    ├── SCT Clock (get_block_height, get_current_epoch)
    ├── Staking budget (end_epoch → compute_new_staking_issuance)
    └── LQT pool (end_block → increment_lqt_issuance)
```

### Depolama Katmanları

| Veri | Depolama | Neden |
|------|-----------|-------|
| Fee parametreleri | JMT | Governance doğrulaması gerekli |
| Gas prices (değişim flag) | Object store | Block-scope geçici |
| Fee akümülatörü | Object store | Block-scope geçici |
| SCT tree | NV storage + Object cache | Hız kritik, proof gerekmiyor |
| Anchor (height→kök) | JMT | Kanıtlanabilir olmalı |
| Anchor (kök→height) | JMT | Hızlı lookup |
| Nullifiers (spent) | JMT | Double-spend kanıtı için Merkle |
| Nullifiers (pending) | Object store | Block-scope geçici |
| Compact blocks | NV storage | Hız öncelikli, büyük veri |
| LQT issuance | NV storage | Hız öncelikli |
| Block timestamps (current) | JMT | Consensus doğrulama |
| Block timestamps (historical) | NV storage | Sadece sorgu amaçlı |

---

## Kritik Matematiksel Formüller (Özet)

### 1. Gas Fee Formülü (gas.rs:122-134)
```
fee = (block_space_price * block_space) / 1000
    + (compact_block_space_price * compact_block_space) / 1000
    + (verification_price * verification) / 1000
    + (execution_price * execution) / 1000
```
Birim: her price * 1000 = 1 staking token base unit

### 2. Fee Tier Çarpanları (fee.rs:56-82)
```
fee_low    = base_fee * 105 / 100   (+%5)
fee_medium = base_fee * 130 / 100   (+%30)
fee_high   = base_fee * 200 / 100   (+%100)
```

### 3. Staking İhracat Formülü (component.rs:115-147)
```
staking_issuance = staking_issuance_per_block * num_blocks_in_epoch
num_blocks = current_block_height - epoch.start_height
```
u128 ile hesaplanır (taşma koruması)

### 4. LQT Havuz Büyüklüğü (component.rs:44-97)
```
lqt_pool += lqt_incentive_per_block  (her blok sonu)
```
epoch_index bazlı nonverifiable storage'a yazılır

### 5. Nullifier Türetimi (nullifier.rs:69-78)
```
nullifier = Poseidon3(NULLIFIER_DOMAIN_SEP, nk, state_commitment, position)
NULLIFIER_DOMAIN_SEP = Fq::from_le_bytes_mod_order(blake2b("penumbra.nullifier"))
```
ZK-friendly: aynı formül R1CS constraint olarak da çalışır (NullifierVar::derive)

### 6. Epoch Bitiş Koşulu (epoch.rs:36-38)
```
is_epoch_end = (current_height - start_height) >= (epoch_duration - 1)
```
Varsayılan epoch_duration = 17280 blok ≈ 1 gün (5s blok süresi)

---

## Nova Shield Chain İçin Genel Dersler ve Eylem Planı

### 1. Çok Boyutlu Gas Modeli (KRİTİK)
**Ders:** Penumbra 4 boyutlu gas kullanıyor: block_space, compact_block_space, verification, execution.
**Nova Shield için:** ZK ispat doğrulama için ayrı gas boyutu şart. MEV koruması ZK ispatları gerektiriyorsa, standart yürütme ile ayrı fiyatlandırılmalı. Önerilen: `{ block_space, verification, execution }` üç boyut minimum.

### 2. Proposer Tip Mekanizması
**Ders:** `fee = base_fee + tip`. tip = 0 kabul edilir. Block sonu `EventBlockFees { total, base, tip }`.
**Nova Shield için:** İşlem önceliği için tip mekanizması baştan implement edilmeli. Bu MEV protection ile çelişmez — Nova Shield'de tip ancak minimum base karşılandıktan sonra alınır.

### 3. SCT Mimari Kararları
**Ders:** `Witness::Forget` ile commitment ekleme (sunucu yol saklamaz), cache-first okuma, çift yönlü anchor indeksi.
**Nova Shield için:** ZK sistemde sunucu tarafı Merkle yolu saklamamak gizlilik sağlar. Cache-first kritik — her blokta yüzlerce SCT okuma olabilir.

### 4. Nullifier Tasarımı
**Ders:** `Poseidon3(domain_sep, nk, commitment, position)` — üçlü bağlam. ZK-friendly.
**Nova Shield için:** Nova Shield DEX'inde swap commitmentları için aynı pattern. Domain separator zorunlu (cross-protocol çakışmasını önler). `NullifierVar` implementasyonu ile ZK spend devre entegrasyonu.

### 5. Compact Block / Light Client Senkronizasyon
**Ders:** `requires_scanning()` optimizasyonu; payload sıralama; streaming (catch-up + keep-alive); source stripping.
**Nova Shield için:** Light client desteği için compact block şablon olarak kullanılabilir. `BlockSender` wrapper ile monoton blok garantisi önemli. Chain ID kontrolünü baştan aktif et (Penumbra'da devre dışı!).

### 6. Depolama Katmanı Tasarımı
**Ders:** JMT (kanıtlanabilir) vs NV storage (hızlı) vs Object store (block-scope geçici) üç katman.
**Nova Shield için:** Hangi verinin kanıtlanabilir olması gerektiği baştan belirlenecek:
- Nullifier set → JMT (double-spend kanıtı)
- Blok yüksekliği, parametreler → JMT
- SCT tree, compact blocks, historical timestamps → NV storage
- Block-scope geçici data → Object store

### 7. LQT (Liquidity Tournament) Ödül Sistemi
**Ders:** Blok başına ödül birikimi (`incentive_per_block`), epoch bazlı dağıtım, `NonZeroU64` bitiş bloğu.
**Nova Shield için:** DEX likidite teşviki için adapte edilebilir. Nova Shield DEX'inde liquidity provider'ları ödüllendirmek için blok-bazlı birikim, epoch-bazlı dağıtım ideal.

### 8. Ekonomik Hesap Güvenliği
**Ders:** u128 ile `staking_issuance_per_block * num_blocks`; `checked_mul().expect()` pattern.
**Nova Shield için:** Büyük ekonomik hesaplarda daima checked aritmetik + Result propagation (expect yerine ?). u128 genişletme standart pratik.

### 9. Event Tasarımı
**Ders:** Her kritik eylem için typed event struct'ları: `EventBlockFees`, `EventLqtPoolSizeIncrease`, `EventBlockRoot`, `EventEpochRoot`, `EventCommitment`.
**Nova Shield için:** Indexer altyapısı için tüm kritik state değişikliklerine event ekle. Her commitment, nullifier, fee ödemesi event kaydı şart.

### 10. Rust Ergonomi Desenleri
- `#![deny(clippy::unwrap_used)]` — tüm crate'lerde
- Blanket impl: `impl<T: StateWrite + ?Sized> FeePay for T {}` — trait mixin
- `im::OrdMap` — persistent/functional veri yapısı
- RAII counter (`CompactBlockConnectionCounter`) — Drop trait ile otomatik cleanup
- `StatePayloadDebugKind` — hassas veri için özel Debug
- `:020` zero-padding — lexicographic range query
- Cache-first pattern — hot path optimizasyonu

---

## Güvenlik ve Eksik Implementasyon Notları

### Güvenlik Riskleri (Dikkat!)

1. **Chain ID kontrolü devre dışı** (compact_block/rpc.rs:65-73): Replay attack riski. Nova Shield'de baştan aktif edilmeli.
2. **Fee parmak izi** (fee.rs:57-58): Fee tier seçimi gözlemlenebilir → işlem gruplaması analizi mümkün. Issue #3153 referansı, çözümlenmemiş.
3. **`alt_gas_prices` RPC'de boş** (fee/rpc.rs:35): `alt_gas_prices: Vec::new()` — eksik implementasyon; alternatif token gas sorgulama çalışmıyor.
4. **`requires_scanning` eksik alanlar** (compact_block.rs:72-81): `swap_outputs` ve `epoch_root` kontrol edilmiyor. Epoch geçişlerinde veya DEX swap'larında istemci bu blokları atlayabilir.
5. **`unwrap_or_else(|| panic!)` kullanımı** (distributions/rpc.rs:38, distributions/component.rs:122): Hata yönetimi eksik; `checked_sub` + `Result` propagation daha güvenli.

### Eksik / Planlanan Özellikler

1. **Dinamik gas fiyatlandırma** (view.rs:22-29 yorumu): "fixed" fiyatlar şimdilik governance parametrelerinden okunuyor
2. **Alt gas price API** (fee/rpc.rs:35): Her zaman boş döndürülüyor
3. **`gas_prices` state key** (state_key.rs:5-7): Tanımlı ama kullanılmıyor — dinamik fiyatlandırma için rezerv
4. **`epoch_change_at_height`** (sct/state_key.rs): Tanımlı ama kullanılmıyor
5. **`note_source`** (sct/state_key.rs): Tanımlı ama kullanılmıyor

---

*Agent 06 — 44 dosya, ~2.200 satır kaynak kodu — Tamamlandı.*
