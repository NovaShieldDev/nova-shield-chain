# Penumbra Analiz — Agent 03
## Kapsam: core/transaction (24 dosya)
## Analiz Tarihi: 2026-07-22

---

## GENEL BAKIS

Penumbra'nin `core/transaction` crate'i, shielded (gizlenmis) bir blockchain'in islem lifecycle'ini tam olarak modelleyen bir Rust kutuphane birimidir. Bu crate uc ana katmani tanimlar:

1. **Plan Katmani** (`TransactionPlan`, `ActionPlan`): Islem olusturulmadan once tam bir beyan — hangi aksiyonlar, ne kadar fee, hangi memo. ZK proof henuz uretilmemis; sadece plaintext parametreler.

2. **Transaction Katmani** (`Transaction`, `TransactionBody`): Artik ZK prooflar uretilmis, imzalar alinmis, tamamlanmis islem. Chain'e gonderilmeye hazir.

3. **View Katmani** (`TransactionView`, `TransactionPerspective`): Ayni transaction'i goren kisinin bakis acisina gore sifre cozulmus veriler. "Gondericinin gormesi" ile "alicinin gormesi" farklidir.

Bu uc katman arasindaki akis:
```
ActionList -> TransactionPlan -> (ZK proofs + auth sigs) -> Transaction -> TransactionView
```

Toplam analiz edilen kaynak: ~2200+ satir (24 dosya).

---

## Dosya Analizi

---

## [action.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/action.rs`
**Satir Sayisi:** 398

### Genel Amac
Penumbra'nin bir transactionda gerceklestirilebilecek TUM aksiyon tiplerini tanimlayan ana enum. Bu dosya, blockchain'in "ne yapabilirsin" sorusunun cevabi.

### Struct/Enum/Type Listesi

**`enum Action`** (Satir 18-51):
Penumbra'da bir transaction'da yer alabilecek her aksiyon bir variant:
- `Output(penumbra_sdk_shielded_pool::Output)` — Sifrelenmis not (token transfer, alici gizli)
- `Spend(penumbra_sdk_shielded_pool::Spend)` — Not harcama (borc kaynagi gizli)
- `ValidatorDefinition(...)` — Validator kayit/guncelleme
- `IbcRelay(penumbra_sdk_ibc::IbcRelay)` — IBC protokol mesaji
- `Swap(penumbra_sdk_dex::swap::Swap)` — DEX swap (gizli miktarli)
- `SwapClaim(penumbra_sdk_dex::swap_claim::SwapClaim)` — Swap sonucu talep etme
- `ProposalSubmit/Withdraw/DepositClaim` — Governance oneri yasamdongusü
- `DelegatorVote/ValidatorVote` — Oylama aksiyonlari
- `PositionOpen/Close/Withdraw` — DEX likidite pozisyon yonetimi
- `Delegate/Undelegate/UndelegateClaim` — Stake islemleri
- `Ics20Withdrawal` — ICS-20 cross-chain cekis
- `CommunityPoolSpend/Output/Deposit` — Topluluk havuzu islemleri
- `ActionDutchAuctionSchedule/End/Withdraw` — Dutch auction yasamdongusü
- `ActionLiquidityTournamentVote` — Likidite turnuvasi oylamasi

**`#[allow(clippy::large_enum_variant)]`** annotation: Bu enum cok buyuk bir variant icerebilir. Clippy uyarisi bastiriliyor — bu bir tasarim karari. Boks kacinilmak isteniyor, fakat enum boyutu buyuyebilir.

### Trait Implementasyonlari

**`impl EffectingData for Action`** (Satir 53-83):
`effect_hash()` — Her aksiyon varyantini ilgili subtype'in `effect_hash()`'ine yonlendirir. ValidatorDefinition icin de effect hash var. Bu, transaction imzalanmadan once "ne etkisi olacak?" sorusuna kriptografik yanit verir.

**`impl Action`** (Satir 85-167):
- `create_span(&self, idx: usize) -> tracing::Span` (Satir 89-136): Her aksiyon icin `tracing` span olusturur. IbcRelay icin nested span olusturma dikkat cekici — IBC mesajinin icini de takip etmek icin.
- `variant_index(&self) -> usize` (Satir 139-167): Canonical protobuf siralama. Bu index, serialization standardini zorlar:
  - Spend=1, Output=2, Swap=3, SwapClaim=4 (cekirdek)
  - ValidatorDefinition=16, IbcRelay=17 (protokol)
  - Proposal aksiyonlari=18-22
  - Position aksiyonlari=30-32
  - Stake aksiyonlari=40-42
  - CommunityPool=50-55
  - ICS20=200

**`impl IsAction for Action`** (Satir 170-232):
`balance_commitment()` ve `view_from_perspective()` trait gereklilikleri. ValidatorDefinition icin `balance::Commitment::default()` donuluyor — validator tanimlamanin deger dengesi etkilememesi gerekiyor.

**`impl DomainType for Action`** (Satir 234-236):
Protobuf turu: `pb::Action`. "DomainType" Penumbra'nin domain → proto donusum sistemi.

**`impl From<Action> for pb::Action`** (Satir 238-320):
Domain tipi → Protobuf. Her variant `pb::action::Action::*` ile sarilir. IbcRelay -> `pb::action::Action::IbcRelayAction` (ad farki dikkat cekici).

**`impl TryFrom<pb::Action> for Action`** (Satir 322-397):
Protobuf → Domain. `PositionRewardClaim` variant'i karsilandiginda:
```rust
Err(anyhow!("PositionRewardClaim is deprecated and unsupported"))
```
Bu, protobuf backward compat korurken Rust tarafinda deprecated variant'i silmis olmak anlamina geliyor.

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **Large enum izni**: `large_enum_variant` clippy uyarisi bastiriliyor. Bu kasitli; boks overhead'inden kaciniyor. DEX Swaplari buyuk veri icerdigindan enum boyutu buyuyebilir, ancak tasinma maliyeti boks overhead'i ile karsilastirilarak tercih edilmis.
2. **`variant_index` ile canonical ordering**: Aksiyonlari protobuf field numaralari ile siralar. Bu, farkli client'larin ayni transaction icin ayni bit-dizisi uretmesini garantiler — determinizm ve effect hash tutarliligini saglar.
3. **Deprecated variant yonetimi**: `PositionRewardClaim` protobuf'ta hala vardir, ancak Rust tarafinda `Err` donuluyor. Eski mesajlarin degisken hata vermesini saglar.

### Nova Shield Chain Icin Dersler
- `variant_index()` mekanizmasi Nova Shield'in kendi aksiyon tiplerini siralamasinda kullanilabilir. Protobuf field numaralari ile Rust enum siralama uyumlu olmali.
- `#[allow(clippy::large_enum_variant)]` bilinclice kullanilmali — buyuk variant'lar heap allocation gerektirmeden stack'te yasayabilir fakat stack overflow riskini artirir.
- Deprecated aksiyon tipleri silinmemeli, `Err` donulmelidir. Bu, on-chain mesajlari ile uyumlulugu korur.
- `create_span` pattern'i Nova Shield'in observability sisteminde kullanilabilir; her aksiyon tipi kendi tracing span'ini olusturmali.

---

## [action_list.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/action_list.rs`
**Satir Sayisi:** 245

### Genel Amac
`ActionList`, `TransactionPlan`'a donusmeden once aksiyon listesi uzerine is mantigi yuruten yardimci struct. Ozellikle **fee hesaplama ve ustu para yonetimi** bu sinifta gerceklesir.

### Struct/Enum/Type Listesi

**`struct ActionList`** (Satir 21-29):
```rust
pub struct ActionList {
    actions: Vec<ActionPlan>,
    change_outputs: BTreeMap<asset::Id, OutputPlan>,  // Ustu para notlari
    fee: Fee,
}
```
`change_outputs`, kullanici tarafindan belirtilmez; sistem tarafindan hesaplanir. `BTreeMap` kullanilmasi determinizm icin (her token turu icin tek change output).

### Fonksiyonlar

**`fn requires_memo(&self) -> bool`** (Satir 48-56):
Memo gerekip gerekmedigini belirler:
```rust
let has_change_outputs = !self.change_outputs.is_empty();
let has_other_outputs = self.actions.iter().any(|action| matches!(action, ActionPlan::Output(_)));
has_change_outputs || has_other_outputs
```
Output varsa memo gerekli — aksi takdirde alici change notunu bulamaz.

**`fn into_plan<R: RngCore + CryptoRng>(...) -> Result<TransactionPlan>`** (Satir 59-85):
`ActionList` -> `TransactionPlan` donusumu. Adimlar:
1. Fee'yi `transaction_parameters`'a yaz
2. Aksiyonlari + change_outputs'lari birlestir
3. `populate_detection_data` ile FMD clue'larini ekle
4. `sort_actions()` ile canonical siralama

**`fn push<A: Into<ActionPlan>>(&mut self, action: A)`** (Satir 88-115):
**KRITIK OZEL KOD**: SwapClaim durumu:
```rust
if let ActionPlan::SwapClaim(claim) = &plan {
    let claim_fee = claim.swap_plaintext.claim_fee;
    if self.fee.amount() == Amount::zero() {
        self.fee = claim_fee; // Fee token'i swap claim'e gore ayarla
    } else if self.fee.asset_matches(&claim_fee) {
        self.fee.0.amount += claim_fee.amount(); // Birikimli fee
    }
    // Farkli token: change'e gider
}
```
Bu, swap claim'in on odedigi fee'yi transaction fee'si olarak kullanma mantigi. Baska bir fee token onceden belirlenmisse, swap claim fee'si change'e gider.

**`fn gas_cost(&self) -> Gas`** (Satir 121-134):
Toplam gas hesaplama: Tum aksiyonlarin + change output'larin gas maliyetini toplar. `gas = gas + action.gas_cost()` — AddAssign eksik oldugu icin bu yontem kullanilmis (TODO yorumu var).

**`fn compute_fee_estimate(&self, gas_prices: &GasPrices, fee_tier: &FeeTier) -> Fee`** (Satir 142-145):
```rust
let base_fee = gas_prices.fee(&self.gas_cost());
base_fee.apply_tier(*fee_tier)
```
Gas maliyeti kesin, ancak base fee sadece tahmini (on-chain fiyat degisebilir).

**`fn refresh_fee_and_change<R>(...)`** (Satir 152-178):
Fee yenileme dongusu:
1. `refresh_change` — mevcut bakiyeye gore ustu notlarini yenile
2. `compute_fee_estimate` — yeni fee tahmini hesapla
3. Farkli token ise overwrite, ayni token ise max al
4. `adjust_change_for_imbalance` — change notlarini fee degisimine gore ayarla

**`fn balance_without_fee(&self) -> Balance`** (Satir 181-192):
Aksiyonlarin + change_outputs'larin bakiyesi, fee hesaba katilmadan.

**`fn balance_with_fee(&self) -> Balance`** (Satir 196-198):
```rust
self.balance_without_fee() - self.fee.0
```

**`fn refresh_change<R>(...)`** (Satir 202-211):
Tum change_outputs'u sifirla. `balance_with_fee().provided()` — artan bakiye icin her token icin yeni change OutputPlan olustur.

**`fn adjust_change_for_imbalance(&mut self)`** (Satir 213-244):
- Artan bakiyeyi mevcut change notuna ekle (`and_modify`)
- Eksik bakiyeyi change notundan cikar (`saturating_sub` — yeterli olmayabilir)
- Sifir degerli change notlarini sil

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **SwapClaim fee birikimi**: Swap claim'in on odedigi fee, transaction fee'sine eklenebilir. Bu, swap isleminin otomatik finanse ettigi transaction modeli.
2. **BTreeMap change_outputs**: Her token turu icin tek change output garanti edilir. HashMap yerine BTree kullanmak, determinizm ve sirali iterasyon saglar.
3. **`saturating_sub`**: Change not miktarinin imkansiz hale gelmesini onler; ancak bu durumda islem dengesiz kalabilir. Bu bir "best effort" dir — planner son kontrolu yapacak.
4. **Fee tahmini vs kesin deger**: Gas maliyeti statik ve kesin, ancak fee token fiyati degisebilir. Bu "overestimate" mantigi uygulanmis.

### Nova Shield Chain Icin Dersler
- `ActionList` benzeri bir builder pattern Nova Shield'in transaction planner'i icin ideal — kullanici duzeyinde API, arkada fee/change yonetimi.
- SwapClaim/fee entegrasyonu Nova DEX icin direkt kullanilabilir: swap oncesi on-odeme, swap sonrasi claim fee'si transaction fee'sine uygulanabilir.
- `BTreeMap<asset::Id, OutputPlan>` ile ustu para yonetimi: her token icin tek change note yeterli ve deterministik.
- Gas maliyeti + fee_tier ayirimi Nova Shield'in fee modelinde kullanilmali: base fee (gas * price) + tier multiplier.

---

## [auth_data.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/auth_data.rs`
**Satir Sayisi:** 65

### Genel Amac
`AuthorizationData`, bir `TransactionPlan`'i onaylayan imzalari tasir. Bu, hardware wallet veya farkli bir imzalama cihazinin plan'i alip imzalayip geri vermesi senaryosunu destekler (air-gapped signing).

### Struct/Enum/Type Listesi

**`struct AuthorizationData`** (Satir 9-21):
```rust
pub struct AuthorizationData {
    pub effect_hash: Option<EffectHash>,          // Onaylanan islemin etkisi
    pub spend_auths: Vec<Signature<SpendAuth>>,   // Spend imzalari (SpendPlan sayisi kadar)
    pub delegator_vote_auths: Vec<Signature<SpendAuth>>, // Delegator oy imzalari
    pub lqt_vote_auths: Vec<Signature<SpendAuth>>, // Likidite turnuvasi oy imzalari
}
```
`Signature<SpendAuth>` — `decaf377_rdsa` kurumphanesi. SpendAuth domain tag'i imzanin hangi amacla kullanildigini belirler (replay protection).

`effect_hash: Option<EffectHash>` — Onaylanan islemin etkisi. Option olmasinin nedeni: hardware wallet imzalamadan once effect_hash'i hesaplamayabilir ya da bilmeyebilir.

### Trait Implementasyonlari

**`impl DomainType for AuthorizationData`** (Satir 23-25):
Proto: `pb::AuthorizationData`.

**`impl From<AuthorizationData> for pb::AuthorizationData`** (Satir 27-40):
Direkt alan haritalama. Her signature vec protobuf'a donusturerek encode edilir.

**`impl TryFrom<pb::AuthorizationData> for AuthorizationData`** (Satir 42-64):
`.collect::<Result<_, _>>()?` — hata durumunda ilk imza hatasi yayilir.

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **Imza tiplerinin ayrilmasi**: Spend, DelegatorVote ve LiquidityTournamentVote imzalari ayri Vec'lere koyulmus. Bu, farkli tip imzalar icin randomizer (randomize spending key) kullanilmasini saglar — her imza icin farkli randomizer gereklidir.
2. **Paralel imzalama destegi**: `build_unauth` metodunda dummy imzalar konulup sonradan gercek imzalar slot'lanabilir. Bu, ZK proof uretimi ile imzalamayi paralellestirmeye imkan verir.
3. **Air-gapped signing**: AuthorizationData'nin bağimsiz bir mesaj olarak iletilmesi, islemin olusturulmasi ile imzalanmasinin farkli cihazlarda olmasi demek.

### Nova Shield Chain Icin Dersler
- `AuthorizationData` benzeri bir yapi, Nova Shield'in hardware wallet destegi icin kopyalanabilir.
- Imza tiplerini domain tag ile ayirma (SpendAuth) replay saldirilarini onler.
- Dummy imzalar → gercek imzalar pattern'i (build_unauth + apply_auth_data) ZK proof ile imzalamayi asenkron yapmayi saglar — onemli performans kazanimi.

---

## [detection_data.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/detection_data.rs`
**Satir Sayisi:** 45

### Genel Amac
Fuzzy Message Detection (FMD) altyapisinin transaction'daki temsili. `DetectionData`, bir transaction'in ilgilendigi olasi alicilara ipucu verir — kesin bilgi vermeden.

### Struct/Enum/Type Listesi

**`struct DetectionData`** (Satir 11-13):
```rust
pub struct DetectionData {
    pub fmd_clues: Vec<Clue>,
}
```
`Clue` tipi `decaf377_fmd` crate'inden gelir. Her output icin bir clue eklenir — alici kendi clue key'ini kullanarak bu clue'larin kendisiyle eslip eslestigini kontrol edebilir.

### Trait Implementasyonlari

**`impl EffectingData for DetectionData`** (Satir 15-19):
```rust
fn effect_hash(&self) -> EffectHash {
    EffectHash::from_proto_effecting_data(&self.to_proto())
}
```
FMD clue'lari transaction'in effect hash'ine dahil edilir. Bu, detection data'nin transaction imzasiyla baglanmasini saglar — sahte clue eklenemez.

**`impl DomainType for DetectionData`** (Satir 21-23)
**`impl TryFrom<pbt::DetectionData> for DetectionData`** (Satir 25-36)
**`impl From<DetectionData> for pbt::DetectionData`** (Satir 38-44)

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **FMD sistemi**: Alicilar tum blok'u indirmek zorunda kalmaz; detection server clue'lari tarar. Yanliz alici belli olasiliklarla "yanlis pozitif" alabilir (fuzzy = tam degil).
2. **Effect hash'e dahil edilmesi**: Detection data transaction'in "effecting data"si. Bu kasitli — detection data degistirilirse imza gecersiz olur.
3. **Dummy clue'lar**: `TransactionPlan::populate_detection_data` her output icin gercek clue + kalan sayida dummy clue ekler. Boyut farkindan alicinilabilecek cikantimlarin onune gecilir.

### Nova Shield Chain Icin Dersler
- Nova Shield gizli islem destegi icin FMD benzeri bir sistem uygulanabilir.
- Detection server'lar tam node yerine "hafif" node gibi davranabilir.
- Dummy clue'lari ekleme yontemi set boyutunu sabitleme fikri — trafik analizine karsi koruma.

---

## [error.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/error.rs`
**Satir Sayisi:** 9

### Genel Amac
Transaction modulu hata tipleri. Minimal ve net.

### Struct/Enum/Type Listesi

**`enum Error`** (Satir 1-9):
```rust
pub enum Error {
    #[error("Chain ID not set")]
    NoChainID,
    #[error("Fee not set")]
    FeeNotSet,
    #[error("Value balance of this transaction is not zero")]
    NonZeroValueBalance,
}
```
`thiserror` ile tureyen `Error`. `PartialEq + Eq` deriving, testlerde hata karsilastirmasina izin verir.

`NonZeroValueBalance`: Transaction'in deger dengesinin sifir olmamasi, gecersiz transaction anlamina gelir. Deger korumasi bu hata ile korunur.

### Tasarim Kararlari
- Minimal hata turlerini tasimak, hata yonetimini kolaylastirir. Sadece crate-ozgu durumlar burada; alt katman hatalari `anyhow` ile aktarilir.
- `PartialEq + Eq`: Unit test yaziminda `assert_eq!` kullanimi icin kritik.

### Nova Shield Chain Icin Dersler
- Her crate kendi hata enum'unu `thiserror` ile tanimlamali.
- `NonZeroValueBalance` analog, Nova Shield'in value conservation invariant'ini zorlamanin acisindan onemli.

---

## [gas.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/gas.rs`
**Satir Sayisi:** 724

### Genel Amac
Penumbra'nin cok boyutlu gas sistemi. Her aksiyon turu icin dort boyutlu gas maliyeti hesaplanir. Bu dosya sistemin en kritik ekonomik bilesenlerinden biridir.

### Struct/Enum/Type Listesi

**`trait GasCost`** (Satir 40-42):
```rust
pub trait GasCost {
    fn gas_cost(&self) -> Gas;
}
```
`Gas` struct'i (baska crate'de tanimlanmis) dort alan icerir:
- `block_space: u64` — Byte olarak blok boyutundaki yer
- `compact_block_space: u64` — Kompakt blok'taki yer (hafif client'lar icin)
- `verification: u64` — Dogrulama maliyeti
- `execution: u64` — Yurutme maliyeti

### Sabitler (Satir 30-35):
```rust
const NULLIFIER_SIZE: u64 = 2 + 32;          // 34 bytes
const NOTEPAYLOAD_SIZE: u64 = 32 + 32 + 176; // 240 bytes
const SWAPPAYLOAD_SIZE: u64 = 32 + 272;      // 304 bytes
const ZKPROOF_SIZE: u64 = 192;               // Groth16 proof boyutu
const BSOD_SIZE: u64 = 16 + 16 + 0 + 4 + 64 + 4; // 104 bytes (degisken, yaklasim)
```

### Fonksiyonlar (Gas Maliyet Hesaplama)

**`fn spend_gas_cost() -> Gas`** (Satir 47-65):
```
block_space: 160 + 192 = 352 bytes
compact_block_space: 34 bytes (nullifier)
verification: 1000 (ZK-SNARK)
execution: 10
```
Aciklama: Spend aksiyonu bir ZK proof icerdigi icin verification=1000. Nullifier compact blok'a gider.

**`fn output_gas_cost() -> Gas`** (Satir 67-85):
```
block_space: 128 + 240 + 192 = 560 bytes
compact_block_space: 240 bytes (NotePayload)
verification: 1000 (ZK-SNARK)
execution: 10
```
NotePayload compact blok'a gider — hafif client'lar yeni notlari gorebilsin diye.

**`fn delegate_gas_cost(delegate: &Delegate) -> Gas`** (Satir 87-101):
```
block_space: encode_to_vec().len() (dinamik)
compact_block_space: 0 (compact blok etkilenmiyor)
verification: 0 (ZK proof yok)
execution: 10
```
Delegate/Undelegate icin dinamik boyut — protobuf kodlamasina gore degisir.

**`fn undelegate_claim_gas_cost() -> Gas`** (Satir 119-140):
```
block_space: 112 + 192 = 304 bytes
compact_block_space: 0
verification: 1000 (ZK-SNARK)
execution: 10
```

**`fn validator_definition_gas_cost(validator_definition: &ValidatorDefinition) -> Gas`** (Satir 142-155):
```
block_space: encode_to_vec().len() (dinamik)
compact_block_space: 0
verification: 200 (imza dogrulama, SNARK degil)
execution: 10
```
Sadece imza dogrulama gerekiyor, ZK yok. Bu nedenle verification=200 (1000'den az).

**`fn swap_gas_cost() -> Gas`** (Satir 157-182):
```
block_space: 128 + 192 + 304 + 104 = 728 bytes
compact_block_space: 304 + 104 = 408 bytes (SwapPayload + BSOD)
verification: 1000 (ZK-SNARK)
execution: 10
```
Kritik not (Satir 172-175): "Swaplar batched, yani BSOD maliyeti teknik olarak paylasiliyor; ancak eger bir blok'ta yalnizca tek swap varsa tum BSOD maliyeti o swap'a ait."

**`fn swap_claim_gas_cost() -> Gas`** (Satir 184-207):
```
block_space: 328 + 192 + 104 = 624 bytes
compact_block_space: 0 (iliskili Spend/Output bunlari oduyor)
verification: 1000 (ZK-SNARK)
execution: 10
```

**`fn delegator_vote_gas_cost() -> Gas`** (Satir 209-232):
```
block_space: 209 + 192 = 401 bytes
compact_block_space: 0
verification: 1000 (ZK-SNARK)
execution: 10
```

**`fn position_withdraw_gas_cost() -> Gas`** (Satir 234-253):
```
block_space: 72 bytes
compact_block_space: 0
verification: 0 (ZK yok)
execution: 10
```
Yorum: "Dutch Auction gas vektorleri de degistirilmeli" — bu uyari Dutch Auction gas maliyetlerinin PositionWithdraw'a bagimli oldugunu gosterir.

**`fn dutch_auction_schedule_gas_cost(dutch_action_schedule: &ActionDutchAuctionSchedule) -> Gas`** (Satir 255-273):
```
block_space: 168 bytes (sabit)
compact_block_space: 0
verification: 50
execution: 2 * step_count * (10 + 10) = 40 * step_count
```
**KRITIK FORMUL**: Dutch auction'in yurutme maliyeti adim sayisiyla dogru orantili. Her adim bir PositionOpen + PositionClose'a esit: `2 * step_count * (PositionOpen_exec + PositionClose_exec)`.

**`fn liquidity_tournament_vote_gas_cost() -> Gas`** (Satir 297-324):
```
block_space: LIQUIDITY_TOURNAMENT_VOTE_DENOM_MAX_BYTES + 223 + 8 + 48 + 32 + 32 + 64 + 192
compact_block_space: NOTEPAYLOAD_SIZE (odul notu olusturabilir)
verification: 1000
execution: 10
```

### GasCost impl'leri

**`impl GasCost for Transaction`** (Satir 326-330):
```rust
fn gas_cost(&self) -> Gas {
    self.actions().map(GasCost::gas_cost).sum()
}
```
Transaction gas = tum aksiyonlarin gas'larinin toplami.

**`impl GasCost for TransactionPlan`** (Satir 332-336):
Ayni mantik, Plan uzerinde.

**`impl GasCost for ActionPlan`** (Satir 346-380):
KRITIK NOT (Satir 340-345): "Bu implementasyonun sonuclari MUTLAKA ilgili Action varyanti ile eslesmelidir, aksi takdirde planner yanlis gas dahil eder." Bu bir dogruluk garantisi degil, bir sozlesmedir.

**Bireysel tip implementasyonlari** (Satir 422-723):
Her aksiyon tipi icin ayri `impl GasCost`. Bazi tipler (CommunityPoolOutput) sifir maliyetlidir (protokol aksiyonu):
```rust
impl GasCost for CommunityPoolOutput {
    fn gas_cost(&self) -> Gas {
        Gas { block_space: 0, compact_block_space: 0, verification: 0, execution: 0 }
    }
}
```

**`impl GasCost for IbcRelay`** (Satir 671-693):
IBC'nin `RecvPacket` icin: compact_block_space=NOTEPAYLOAD_SIZE (not basiyor) ve verification=1000. Diger IBC varyantlari: sifir.

**`impl GasCost for PositionOpenPlan`** (Satir 539-562):
Encrypted metadata dahil edilmemisini cezalandirmamak icin padding:
```rust
let padding = if self.metadata.is_none() {
    ENCRYPTED_POSITION_METADATA_SIZE_BYTES as u64
} else {
    0
};
block_space: self.position.encode_to_vec().len() as u64 + padding,
```
Bu onemli bir tesviktir: metadata eklenirse block_space daha kucuk gorunur → metadata eklenmemesi daha pahali yapilir.

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **4-boyutlu gas**: `block_space`, `compact_block_space`, `verification`, `execution` ayri fiyatlandirabilir. Bu farkli donanim profilleri (hafif node vs full node) icin fiyatlandirmayi mumkun kilar.
2. **Statik gas maliyetleri**: "Penumbra transactionlarin statik gas maliyetleri vardir" yorum satiri. ZK proof boyutu sabit (Groth16 = 192 byte), bu nedenle maliyetler sabit hesaplanabilir.
3. **Dutch auction adim-orantili maliyet**: `2 * step_count * (10 + 10)` — bu gercekciligi yansitir; her adim 2 position aksiyonu gerektiriyor.
4. **Padding ile tesvikler**: Metadata eklemek gas'i dusurur. Fiyatlandirma mekanizmasi ile davranis yonlendirme.

### Nova Shield Chain Icin Dersler
- **4-boyutlu gas modeli aynen kopyalanabilir**: block_space + compact_block_space + verification + execution. Bu, ZK chain'lerde dogru fiyatlandirma yapar.
- `ZKPROOF_SIZE: u64 = 192` — Groth16 proof boyutu sabiti olarak tanimlanmali.
- Dutch auction'in `execution = 2 * step_count * (op_cost)` formulu, Nova DEX'in Dutch auction implementasyonu icin direkt kullanilabilir.
- PositionOpenPlan padding trick'i: "Metadata eklemek gas'i dusurur" — Nova Shield'de benzer tesvikler kullanilabilir.
- `CommunityPoolOutput` gibi protokol aksiyonlari sifir gas maliyetine sahip olmali.
- `ActionPlan` ile `Action` gas hesaplamasinin eslesmesi zorunlu — birim testleriyle dogrulanmali.

---

## [is_action.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/is_action.rs`
**Satir Sayisi:** 539

### Genel Amac
`IsAction` trait'inin tum aksiyon tipleri icin implementasyonu. `balance_commitment()` ve `view_from_perspective()` methodlarini saglar.

### Trait Tanimlamasi

**`pub trait IsAction`** (Satir 36-39):
```rust
pub trait IsAction {
    fn balance_commitment(&self) -> balance::Commitment;
    fn view_from_perspective(&self, txp: &TransactionPerspective) -> ActionView;
}
```
- `balance_commitment()`: Bu aksiyonun deger dengesine katkisini kriptografik taahhut olarak verir.
- `view_from_perspective()`: `TransactionPerspective`'e gore gorunumlu/opak sekilde donusturur.

### Onemli Implementasyonlar

**`impl IsAction for DelegatorVote`** (Satir 49-71):
```rust
fn balance_commitment(&self) -> balance::Commitment {
    Value {
        amount: self.body.unbonded_amount,
        asset_id: VotingReceiptToken::new(self.body.proposal).id(),
    }
    .commit(Fr::zero())
}
```
Delegator oyu, sanki kullanicidan bir VotingReceiptToken tuketen bir aksiyon gibi modellenir. Bu sifir blinding ile taahhut — acik deger.

**`impl IsAction for Output`** (Satir 113-155):
```rust
fn view_from_perspective(&self, txp: &TransactionPerspective) -> ActionView {
    let note_commitment = self.body.note_payload.note_commitment;
    let epk = self.body.note_payload.ephemeral_key;
    if let Some(payload_key) = txp.payload_keys.get(&note_commitment) {
        // Hem not hem memo key coz
        let decrypted_note = Note::decrypt_with_payload_key(...);
        let decrypted_memo_key = self.body.wrapped_memo_key.decrypt_outgoing(payload_key);
        if let (Ok(decrypted_note), Ok(decrypted_memo_key)) = ... {
            OutputView::Visible { ... }
        } else {
            OutputView::Opaque { ... }
        }
    } else {
        OutputView::Opaque { ... }
    }
}
```
Hem not hem memo key sifrelenmesinin IKISI DE basariliysa Visible, herhangi biri basarisizsa Opaque.

**`impl IsAction for PositionWithdraw`** (Satir 291-331):
`balance_commitment()` kompleks — iki NFT state gecirisini modelliyor:
```rust
let prev_state_nft = if self.sequence == 0 {
    // Closed pozisyon NFT'sini tukeniyor
    Value { amount: 1, asset_id: LpNft::new(pos_id, State::Closed).asset_id() }
} else {
    // Bir onceki Withdrawn sequence NFT'si tukeniyor
    Value { amount: 1, asset_id: LpNft::new(pos_id, State::Withdrawn { sequence: seq-1 }).asset_id() }
}.commit(Fr::zero());

let next_state_nft = Value { amount: 1, asset_id: LpNft::new(pos_id, State::Withdrawn { sequence }).asset_id() }.commit(Fr::zero());

self.reserves_commitment - prev_state_nft + next_state_nft
```
Bu, rezervleri serbest birakip yeni bir Withdrawn NFT olusturuyor ve onceki NFT'yi tukeniyor.

**`impl IsAction for Swap`** (Satir 333-444):
`view_from_perspective` ozellikle karmasik. Iki senaryo:
1. Plaintext mevcut (payload key ile sifre cozuldu): BSOD'u bul, output notlarini hesapla, `SwapView::Visible`.
2. Plaintext yok ama BSOD var: Pro-rata output hesapla (`bsod.pro_rata_outputs`), `SwapView::Opaque` (ama output miktar tahmini gosterilebilir).

**`impl IsAction for SwapClaim`** (Satir 447-478):
Output notlarini `advice_notes`'tan alir (bunlar once hesaplanip TxP'ye eklenmis olmali):
```rust
let output_1 = txp.advice_notes.get(&self.body.output_1_commitment);
let output_2 = txp.advice_notes.get(&self.body.output_2_commitment);
```

**`impl IsAction for ActionDutchAuctionWithdraw`** (Satir 506-518):
```rust
fn view_from_perspective(&self, _txp: &TransactionPerspective) -> ActionView {
    let view = ActionDutchAuctionWithdrawView {
        action: self.to_owned(),
        reserves: vec![], // BOŞ — asil degerler TxP'den gelecek
    };
    ActionView::ActionDutchAuctionWithdraw(view)
}
```
`reserves: vec![]` — Bu neden bos? TxP bu bilgiyi dogrudan saglamiyor. Bir TODO veya tasarim eksikligi olabilir.

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **Visible/Opaque dualize**: Her "gizli" aksiyon iki goruntume sahip. TxP'de anahtar varsa visible, yoksa opaque. Bu, sahte gorunumlere karsi koruma saglar — sadece bilgisi olana plaintext gosterilir.
2. **`Fr::zero()` blinding**: Acik (sifre korumasiz) aksiyonlar sifir blinding ile taahhut eder. Bu, taahhudun degerini ortaya koyar — gizlilik yoktur.
3. **PositionWithdraw NFT zinciri**: State gecisler NFT tukenim/uretim olarak modellenir. Bu programlanabilir dogruluk saglar — yanlis state gecisler deger dengesini bozar.

### Nova Shield Chain Icin Dersler
- `IsAction` trait'i Nova Shield'in kendi aksiyon tipleri icin kopyalanabilir. `balance_commitment()` zorunlu — aksi takdirde binding signature dogrulanamaz.
- "Visible/Opaque" dualize — Nova Shield view protokolu icin gerekli. Her sifreli aksiyon opaque view olabilen bir yapiyla modellenmeli.
- NFT state gecisini deger dengesi icerisinde modellemek (PositionWithdraw ornegi) — Nova DEX pozisyon lifecycle'i icin direkt kullanilabilir.
- `advice_notes` mekanizmasi — view servisi onceden hesaplanmis verileri sağlar. Nova Shield view katmani icin ayni pattern.

---

## [lib.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/lib.rs`
**Satir Sayisi:** 51

### Genel Amac
Crate kokunu tanimlayan, tum modulleri ve public API'yi ihrac eden dosya.

### Ozellikler
```rust
#![deny(clippy::unwrap_used)]   // .unwrap() yasak — hata yonetimi zorunlu
#![allow(clippy::clone_on_copy)] // Copy tipleri klonlanabilir
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
```
`deny(clippy::unwrap_used)`: Butun crate boyunca `.unwrap()` yasak. Hata yonetimi `?` operatoru veya pattern matching ile yapilmali. Bu, production kalitesi icin kritik.

### Modul Yapisi

Private modueller:
- `auth_data`, `detection_data`, `error`, `is_action`, `parameters`, `transaction`, `witness_data`

Public modueller:
- `action`, `action_list`, `gas`, `memo`, `plan`, `view`

### Ozel Tip

**`pub struct Compat<'a, T>(pub &'a T)`** (Satir 50-51):
```rust
/// Orphan rule workaround: trait implementasyonlari bircok crate'de tekrarlaniyor.
pub struct Compat<'a, T>(pub &'a T);
```
Orphan rule (yabanci tip icin yabanci trait implante edilemez) cozumu. Crate bolunmesi sirasinda gecici olarak kullanilan bir wrapper. Issue #2288 referansi var.

### Tasarim Kararlari
- `#[deny(clippy::unwrap_used)]`: Production kodunda `.unwrap()` tamamen yasaklanmis. Nova Shield de bu politikayi benimsemeli.
- `Compat` wrapper: Rust orphan rule cozumu icin. Buyuk crate'lerde bolunme sirasinda sikca karsilasilan bir pattern.

### Nova Shield Chain Icin Dersler
- `#![deny(clippy::unwrap_used)]` Nova Shield'in tum crate'lerine uygulanmali.
- Orphan rule sorunlari icin `Compat<'a, T>` gibi newtype wrapper pattern kullanilabilir.

---

## [memo.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/memo.rs`
**Satir Sayisi:** 401 (testler dahil)

### Genel Amac
Sifrelenmis islem memosu. Her transaction bir memo icerabilir — gondericinin imzalanan metni. Memo, tum output'lar arasinda paylasilan tek bir sifrelenmis mesajdir.

### Sabitler
```rust
pub const MEMO_CIPHERTEXT_LEN_BYTES: usize = 528;
pub const MEMO_LEN_BYTES: usize = 512;           // 528 - 16 (MAC boyutu)
const MAX_TEXT_LEN: usize = 512 - 80;            // 432 bytes (80 byte adres hic icin)
```
Memo formati: `[return_address: 80 bytes][text: maks 432 bytes]`

### Struct/Enum/Type Listesi

**`struct MemoCiphertext([u8; 528])`** (Satir 36):
Sabit boyutlu sifrelenmis memo. Ayni boyut — sifrelenmis memo'larin boyutuna gore alinicinin tespiti engellenir.

**`struct MemoPlaintext`** (Satir 44-48):
```rust
pub struct MemoPlaintext {
    return_address: Address,  // 80 bytes
    text: String,             // maks 432 bytes
}
```

### Fonksiyonlar

**`fn raw_bytes_to_text(data: &[u8]) -> String`** (Satir 29-33):
Bos (null) byte'lari siler — padding'i temizler:
```rust
String::from_utf8_lossy(data).trim_end_matches(0u8 as char).to_string()
```
`from_utf8_lossy` — gecersiz UTF-8 bozulmus karakterle degistirilebilir. Hata dondurmez.

**`impl MemoPlaintext`:**
- `fn new(return_address, text) -> Result<Self>`: Metin uzunlugu kontrol edilir. MAX_TEXT_LEN'i asarsa hata.
- `fn blank_memo(return_address) -> MemoPlaintext`: Bos metin ile memo.
- `fn to_vec(&self) -> Vec<u8>`: `[adres_bytes || metin_bytes]`

**`impl MemoCiphertext`:**

**`fn encrypt(memo_key: PayloadKey, memo: &MemoPlaintext) -> Result<MemoCiphertext>`** (Satir 116-133):
```
1. memo.to_vec() -> bytes (80 + text_len)
2. m = [0u8; 512] ile padding
3. m[..bytes.len()].copy_from_slice(bytes)
4. memo_key.encrypt(m, PayloadKind::Memo)
5. [u8; 528] kontrolu
```

**`fn decrypt(memo_key, ciphertext) -> Result<MemoPlaintext>`** (Satir 136-147):
```
1. decrypt_bytes -> [u8; 512]
2. return_address = bytes[..80]
3. text = raw_bytes_to_text(bytes[80..])
```

**`fn decrypt_outgoing(...) -> Result<MemoPlaintext>`** (Satir 164-194):
Gonderici, kendi OVK'si (Outgoing Viewing Key) ile memo'yu cozebilir:
```
1. Note::decrypt_key(wrapped_ovk, cm, cv, ovk, epk) -> shared_secret
2. action_key = PayloadKey::derive(shared_secret, epk)
3. memo_key = wrapped_memo_key.decrypt_outgoing(action_key)
4. memo_key.decrypt(ciphertext, PayloadKind::Memo)
```

**`impl TryFrom<&[u8]> for MemoCiphertext`** (Satir 197-209):
Kismi sifrelenmis memo kabul edilir (is_uzun_sayida sifirla doldurulur). `input.len() > MEMO_CIPHERTEXT_LEN_BYTES` kontrolu.

### Test'ler (Satir 262-400)

**`test_memo_encryption_and_decryption`** (Satir 273-309):
- Sender tarafindan: memo sifreleme + wrapped_memo_key olusturma
- Alici tarafindan: epk ile IVK key agreement, memo key cozme, memo cozme
- Esitlik kontrolu

**`test_memo_encryption_and_sender_decryption`** (Satir 312-365):
- Sender tarafindan OVK ile kendi gonderdigi memo'yu cozme
- `decrypt_outgoing` kullaniliyor

**`test_memo_size_limit` (proptest)** (Satir 375-400):
0-10000 karakter arasi rastgele UTF-8 string. `MAX_TEXT_LEN`'i asan stringler hata vermeli. Basarisiz encrypt = onceki kontrol sayesinde.

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **Sabit boyutlu ciphertext**: 528 byte her zaman. Memo boyutu sifirdan 432 byte'a kadar degisse de, sifreli metin her zaman ayni boyut. Trafik analizi engellenir.
2. **return_address memo'ya gomulur**: Alici cevap adresini plaintext'e bakmadan bulabilir. Ilginc gizlilik tradeoff.
3. **PayloadKind::Memo domain ayirimi**: Farkli payload turleri farkli domain tag'i ile sifrelenir. Bir not key'iyle memo cozulemez.
4. **OVK ile sender cozumu**: Gonderici kendi gonderdigi mesajlara erisebilir — bir tur "outgoing view key" ozelligi.

### Nova Shield Chain Icin Dersler
- Sabit boyutlu ciphertext (padding ile) Nova Shield'in memo destegi icin kopyalanabilir.
- `PayloadKind` domain tag sistemi — farkli sifre amaclarini ayirmak icin kritik.
- Sender cozumunu desteklemek (OVK mekanizmasi) geri donus icin onemli.
- `from_utf8_lossy` yerine daha katı `from_utf8` kullanilabilir; bozuk UTF-8 red edilebilir.

---

## [parameters.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/parameters.rs`
**Satir Sayisi:** 49

### Genel Amac
Transaction'in zincire kabul sartlarini (fee, vade yuksekligi, chain ID) tanimlayan minimal struct.

### Struct/Enum/Type Listesi

**`struct TransactionParameters`** (Satir 9-13):
```rust
pub struct TransactionParameters {
    pub expiry_height: u64,  // Bu blok yuksekliginden sonra gecersiz
    pub chain_id: String,    // Replay korumasi
    pub fee: Fee,            // Odenen ucret
}
```

### Trait Implementasyonlari

**`impl EffectingData for TransactionParameters`** (Satir 15-19):
`TransactionParameters` effect hash'e dahil edilir. Bunun anlami: chain_id, fee, expiry_height degistirilirse islem imzasi gecersizlenir — kritik guvenlik ozelligi.

### Tasarim Kararlari
1. **`chain_id: String`**: Replay saldirisi korumasi. Bir chain'de gecerli imza baska chain'de gecersiz.
2. **`expiry_height: u64`**: Islem vade yuksekligi. Uzun sure mempool'da bekleyen islemler otomatik gecersizlesir.
3. **Fee effect hash'e dahil**: Kullanici belirli fee'yi onayladi. Fee degistirilirse imza bozulur.

### Nova Shield Chain Icin Dersler
- `chain_id` replay korumasinda zorunlu. Genesis'te ayarlanmali.
- `expiry_height` mempool yonetimi icin gerekli. Eski islemler temizlenir.
- `EffectingData` implementasyonu — sadece fee/chain_id degil, TUM transaction parametreleri hash'e dahil edilmeli.

---

## [plan.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/plan.rs`
**Satir Sayisi:** 603 (testler dahil)

### Genel Amac
`TransactionPlan` — bir transaction'in tam bildirimi, ZK proof'lar olmadan. Plan asamasinda her sey plaintext; buradan `Transaction`'a gecis ZK proof uretimini gerektirir.

### Struct/Enum/Type Listesi

**`struct TransactionPlan`** (Satir 47-52):
```rust
pub struct TransactionPlan {
    pub actions: Vec<ActionPlan>,
    pub transaction_parameters: TransactionParameters,
    pub detection_data: Option<DetectionDataPlan>,
    pub memo: Option<MemoPlan>,
}
```

### Fonksiyonlar

**`fn sort_actions(&mut self)`** (Satir 56-59):
```rust
self.actions.sort_by_key(|action: &ActionPlan| action.variant_index());
```
Canonical siralama. Protobuf field numaralarina gore siralar. Bu, farkli client implementasyonlarinin ayni effect hash uretmesini saglar.

**`fn effect_hash(&self, fvk: &FullViewingKey) -> Result<EffectHash>`** (Satir 70-115):
**KRITIK METOD** — Transaction'in imzalanacagi hash'i hesaplar (ZK proof gerektirmeden):
```
state = blake2b_simd ile "PenumbraEfHs" personalize hash
state.update(parameters_hash)
state.update(memo_hash)  // None ise all-zero hash
state.update(detection_data_hash)  // None ise all-zero hash
state.update(num_actions.to_le_bytes())
for action_plan:
    state.update(action_plan.effect_hash(fvk, &memo_key))
return EffectHash(state.finalize())
```
Not: Memo yoksa sifir hash kullanilir — bu "memo yoktu" bilgisini hash'e dahil eder, memo sonradan eklenmesini engeller.

**Tip-spesifik iterator metodlari** (Satir 117-337):
`spend_plans()`, `output_plans()`, `delegations()`, vb. Hepsi ayni pattern: `filter_map` ile ilgili `ActionPlan` varyantini filtreler.

**`fn num_proofs(&self) -> usize`** (Satir 357-370):
ZK proof gerektiren aksiyonlari sayar:
```rust
ActionPlan::Spend(_) => 1,
ActionPlan::Output(_) => 1,
ActionPlan::Swap(_) => 1,
ActionPlan::SwapClaim(_) => 1,
ActionPlan::UndelegateClaim(_) => 1,
ActionPlan::DelegatorVote(_) => 1,
_ => 0,
```

**`fn populate_detection_data<R>(&mut self, rng, precision)`** (Satir 373-396):
FMD clue stratejisi:
1. Her dest_address icin gercek CluePlan olustur
2. `num_dummy_clues = num_outputs - clue_plans.len()` adet dummy clue ekle
3. Clue sayisi == output sayisi olana kadar dummy doldur

Bu, output sayisindan clue sayisi farki analizi ile alinicinin tespitini engeller.

**`fn memo_key(&self) -> Option<PayloadKey>`** (Satir 411-413):
```rust
self.memo.as_ref().map(|memo_plan| memo_plan.key.clone())
```

### Test (Satir 450-603)

**`plan_effect_hash_matches_transaction_effect_hash`** (Satir 477-601):
- 2 SpendPlan + 1 OutputPlan + 1 SwapPlan iceren TransactionPlan olustur
- `sort_actions()` uygula
- `plan.effect_hash(fvk)` hesapla
- `plan.authorize(rng, sk)` ile imzala
- `plan.build(fvk, witness_data, auth_data)` ile transaction olustur
- `transaction.effect_hash()` hesapla
- Iki hash esit olmali: `assert_eq!(plan_effect_hash, transaction_effect_hash)`

Bu test, plan ve transaction'in effect_hash'inin uyumlu oldugunu dogrular — bu kritik bir duzgunluk kontrouldur.

Ayrica: `transaction.decrypt_memo(fvk)` == `memo_plaintext` kontrolu.

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **"PenumbraEfHs" domain separator**: blake2b personalization ile hash domain'i ayrilir. Farkli hashlar birbiriyle karisitirilmaz.
2. **Absent veri = sifir hash**: Memo veya detection data yoksa all-zero hash. Bu, "veri eklendi" ile "veri hep vardı" arasindaki farki hash'e yansitiyor.
3. **`sort_actions` ile determinizm**: Kullanici siralamasina bagli olmayan canonical hash. Client implementasyon farklarini ortadan kaldirir.
4. **Plan-Transaction EffectHash uyumu**: Plan duzeyinde hesaplanabilir effect hash — ZK proof uretimi GEREKMEZ. Bu hardware wallet entegrasyonu icin kritik.

### Nova Shield Chain Icin Dersler
- `effect_hash` hesaplamada blake2b + domain separator ("NovaTxEfHs" gibi) zorunlu.
- "None = sifir hash" pattern'i tutarlilik icin kullanilmali.
- `sort_actions` kopyalanmali — canonical siralama olmadan farkli client'lar farkli hash uretir.
- `num_proofs()` — ZK proof gerektiren aksiyonlari ayirt etmek icin ayni pattern.
- Plan-level effect hash, hardware wallet guvenligi icin kritik — ZK olmadan imzalanabilir.

---

## [plan/action.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/plan/action.rs`
**Satir Sayisi:** 666

### Genel Amac
`ActionPlan` enum'u — her aksiyon icin plaintext plan versiyonu. Plan, ZK prooftan once tum bilgileri icerir.

### Struct/Enum/Type Listesi

**`enum ActionPlan`** (Satir 42-88):
`Action` enum'unun plan versiyonu. Onemli farklar:
- `Spend(SpendPlan)` — Note + TCT pozisyonu (proof uretmek icin gerekli)
- `Output(OutputPlan)` — Alici adresi + deger (sifreleme icin gerekli)
- `PositionOpen(PositionOpenPlan)` — Metadata sifreleme icin plan gerekli
- `PositionWithdraw(PositionWithdrawPlan)` — Rezerv bakiyesi icin plan gerekli

Bazi aksiyonlar ayni (plan = action): `Delegate`, `Undelegate`, `ValidatorDefinition`, `IbcRelay`, `ProposalSubmit`, vb.

### Fonksiyonlar

**`fn build_unauth(action_plan, fvk, witness_data, memo_key) -> Result<Action>`** (Satir 99-184):
Her ActionPlan varyantini `Action`'a donusturur (sifir imzalarla):
- Spend: `spend_plan.spend(fvk, [0;64].into(), auth_path, anchor)` — sifir auth_sig
- Output: `output_plan.output(fvk.outgoing(), memo_key.unwrap_or(&dummy_key))`
- SwapClaim: TCT auth_path gerekli
- DelegatorVote: TCT auth_path gerekli
- LQT vote: TCT auth_path gerekli
- Diger aksiyonlar: `.clone()` ile direkt kopyalanir

**`fn variant_index(&self) -> usize`** (Satir 187-215):
`Action::variant_index()` ile birebir ayni sayilar. Bu uyum zorunlu — siralama tutarliligini saglar.

**`fn balance(&self) -> Balance`** (Satir 217-249):
Her ActionPlan varyantinin deger dengesini hesaplar:
- `IbcAction`, `ValidatorDefinition`, `ValidatorVote`, `ActionLiquidityTournamentVote` icin `Balance::default()` (sifir)

**`fn value_blinding(&self) -> Fr`** (Satir 251-280):
Sadece `Spend` ve `Output` gercek blinding faktor kullanir. Swap icin `fee_blinding`. Diger tum aksiyonlar `Fr::zero()`.

**`fn effect_hash(&self, fvk, memo_key) -> EffectHash`** (Satir 283-314):
Her plan'in uretecegi action body'nin effect hash'i — ZK proof uretmeden. Bu fonksiyon plan-level transaction hash'i hesaplamaya izin verir.

### Convenience From impl'leri (Satir 319-467):
Her spesifik tip'ten `ActionPlan`'a donusum. `impl From<SpendPlan> for ActionPlan`, `impl From<OutputPlan> for ActionPlan`, vb.

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **Sifir imzalarla build_unauth**: Once aksiyonu sifir imzalarla olustur, sonra gercek imzalari ekle. Bu, ZK proof uretimi (paralel yapilabilir) ile imzalamayi (senkron olmali) ayirir.
2. **`ActionPlan` ile `Action` variant_index uyumu**: Iki enum ayni sayilara sahip — bu uyum test edilmeli.
3. **`value_blinding` tek noktada**: Sadece Spend/Output/Swap fee'si blinding kullanir. Bu, binding signature'in dogru hesaplanmasi icin kritik.

### Nova Shield Chain Icin Dersler
- `build_unauth` pattern'i Nova Shield'de aynen uygulanmali. Sifir imza + sonra gercek imza, paralel ZK proof uretimini saglar.
- `value_blinding()` metodunu her aksiyon tipi icin dogru tanimlamak zorunlu — binding signature buna dayanir.
- `ActionPlan::variant_index()` == `Action::variant_index()` garantisi test edilmeli.

---

## [plan/auth.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/plan/auth.rs`
**Satir Sayisi:** 47

### Genel Amac
`TransactionPlan`'i `SpendKey` ile onaylama. `AuthorizationData` uretir.

### Fonksiyonlar

**`fn authorize<R>(mut rng, sk) -> Result<AuthorizationData>`** (Satir 12-46):
Algoritma:
1. `effect_hash = self.effect_hash(sk.full_viewing_key())?` — transaction effect hash'i hesapla
2. Her Spend icin: `rsk = sk.spend_auth_key().randomize(&plan.randomizer)` — randomized spend key
3. `auth_sig = rsk.sign(&mut rng, effect_hash.as_ref())` — effect hash imzala
4. Her DelegatorVote icin ayni randomizer + imzalama
5. Her LQT vote icin ayni randomizer + imzalama
6. `AuthorizationData { effect_hash: Some(effect_hash), spend_auths, ... }` dondur

**Kritik Guvenlik Notu**: Her spend icin farkli randomizer kullanilir (`spend_plan.randomizer`). Bu, farkli Spend'lerin farkli gozlemlenebilir dogrulama anahtarina sahip olmasini saglar — baglanti kurulamaz.

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **Randomized spend key**: `rsk = sk.spend_auth_key().randomize(randomizer)` — spending key her kullanim icin randomize edilir. Bu, iki Spend'i ayni kullaniciya baglamamayi saglar.
2. **Effect hash imzalama**: imzalanan veri `effect_hash` — sadece transaction'in ETKISI imzalanir, transaction'in kendisi degil. Bu kritik bir soyutlama.

### Nova Shield Chain Icin Dersler
- Randomized spend key Nova Shield'in spend mekanizmasi icin kopyalanmali.
- Effect hash imzalama prensibi: "neyin etkisini onayliyorum" mantigi.

---

## [plan/build.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/plan/build.rs`
**Satir Sayisi:** 229

### Genel Amac
`TransactionPlan`'dan `Transaction`'a build sureci. Bu, tum ZK proof'lari toplayan ve binding signature'i hesaplayan asama.

### Fonksiyonlar

**`fn build_unauth_with_actions(self, actions, witness_data) -> Result<Transaction>`** (Satir 16-42):
Onceden olusturulmus `Action` listesini alip `Transaction` olusturur:
```rust
let transaction_body = TransactionBody { actions, transaction_parameters, detection_data, memo };
Transaction { transaction_body, anchor: witness_data.anchor, binding_sig: [0;64].into() }
```
Binding sig sifir — henuz hesaplanmadi.

**`fn apply_auth_data(&self, auth_data, transaction) -> Result<Transaction>`** (Satir 47-129):
1. Spend sayisi kontrolu: `spend_count != spend_auths.len()` hata
2. Synthetic blinding faktor hesapla:
   ```rust
   let mut synthetic_blinding_factor = Fr::zero();
   for action_plan in &self.actions {
       synthetic_blinding_factor += action_plan.value_blinding();
   }
   ```
3. Gercek imzalari slot'la (Spend, DelegatorVote, LQT vote)
4. Binding signing key: `rdsa::SigningKey::from(synthetic_blinding_factor)`
5. `auth_hash = transaction.transaction_body.auth_hash()`
6. `binding_sig = binding_signing_key.sign_deterministic(auth_hash.as_bytes())`
7. `transaction.binding_sig = binding_sig`

**`fn build(self, full_viewing_key, witness_data, auth_data) -> Result<Transaction>`** (Satir 132-162):
Seri build:
```
1. actions = self.actions.map(|p| ActionPlan::build_unauth(p, fvk, witness_data, memo_key))
2. tx = self.clone().build_unauth_with_actions(actions, witness_data)
3. tx = self.apply_auth_data(auth_data, tx)
```

**`fn build_concurrent(self, fvk, witness_data, auth_data) -> Result<Transaction>`** (Satir 165-207):
Paralel build (`#[cfg(feature = "parallel")]`):
```rust
let action_handles = self.actions.iter()
    .map(|action_plan| {
        let fvk2 = fvk.clone();
        let witness_data2 = witness_data.clone(); // Arc
        tokio::task::spawn_blocking(move || {
            ActionPlan::build_unauth(action_plan, &fvk2, &*witness_data2, memo_key2)
        })
    });
```
`Arc<WitnessData>` ile paylasilancilik. Her aksiyon kendi thread'inda ZK proof uretiyor.

**`fn witness_data(&self, sct) -> Result<WitnessData>`** (Satir 210-228):
SCT'den witness veri uret:
```rust
let anchor = sct.root();
let state_commitment_proofs = self.spend_plans()
    .map(|spend| sct.witness(spend.note.commit()))
    .collect::<Result<_>>()?;
```

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **Synthetic blinding factor = binding key**: Deger taahhutlerinin blinding faktorleri toplandigi zaman, sonuc degeri bir gruba gecis (binding signing key) verir. Bu "commitment scheme" matematigidir: `Commit(v, r) = vG + rH`, toplam `Sum(vG) + Sum(r)H`, ve `Sum(r)` binding signing key.
2. **`sign_deterministic`**: Binding sig deterministik imzalanir — nonce rasgele degil. Bu, ayni transaction'in her seferinde ayni binding sig uretmesini saglar.
3. **Paralel build**: `tokio::task::spawn_blocking` — her ZK proof CPU-bound, thread pool'da paralel uretilir.

### Nova Shield Chain Icin Dersler
- **Binding signature mekanizmasi**: `Sum(value_blindings)` -> signing key -> sign(auth_hash). Bu, shielded transaction dogrulamasinin kalbi. Nova Shield ZK sistemi icin aynen gerekli.
- Paralel ZK proof uretimi (`spawn_blocking + Arc`) Nova Shield'in transaction build performansi icin zorunlu — her proof ~1-2s uretilirse paralel cok onemli.
- `auth_hash` vs `effect_hash` farki: effect_hash "ne etkisi var?", auth_hash "bu transaction body nedir?" ikisi farkli amaclarla kullanilir.

---

## [plan/clue.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/plan/clue.rs`
**Satir Sayisi:** 67

### Genel Amac
FMD clue planlama. Her output icin bir clue olusturulacak.

### Struct/Enum/Type Listesi

**`struct CluePlan`** (Satir 8-12):
```rust
pub struct CluePlan {
    pub address: Address,
    pub precision: Precision,  // FMD hassasiyeti
    pub rseed: [u8; 32],       // Rastgele seed — deterministik clue uretimi
}
```

### Fonksiyonlar

**`fn new<R>(rng, address, precision) -> CluePlan`** (Satir 16-28):
Rastgele rseed ile CluePlan olustur.

**`fn clue(&self) -> Clue`** (Satir 31-37):
```rust
let clue_key = self.address.clue_key();
let expanded_clue_key = clue_key.expand_infallible();
expanded_clue_key.create_clue_deterministic(self.precision, self.rseed)
```
`create_clue_deterministic` — ayni rseed ile ayni clue. Bu, plan serialization/deserializasyon sonrasinda ayni clue'nun uretilmesini saglar.

### Tasarim Kararlari
- `rseed` plana eklenmis — plan kaydedilip yuklendiginde ayni clue uretilir. Deterministik ama gizli.

---

## [plan/detection_data.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/plan/detection_data.rs`
**Satir Sayisi:** 45

### Genel Amac
`DetectionDataPlan` — `DetectionData`'nin plan versiyonu. FMD clue planlari icerir.

### Struct/Enum/Type Listesi

**`struct DetectionDataPlan`** (Satir 9-11):
```rust
pub struct DetectionDataPlan {
    pub clue_plans: Vec<CluePlan>,
}
```

### Fonksiyonlar

**`fn detection_data(&self) -> DetectionData`** (Satir 14-18):
```rust
DetectionData {
    fmd_clues: self.clue_plans.iter().map(|x| x.clue()).collect(),
}
```
Plan → DetectionData donusumu.

---

## [plan/memo.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/plan/memo.rs`
**Satir Sayisi:** 72

### Genel Amac
`MemoPlan` — memo'nun plan versiyonu. Plaintext + sifreleme anahtari icerir.

### Struct/Enum/Type Listesi

**`struct MemoPlan`** (Satir 10-13):
```rust
pub struct MemoPlan {
    pub plaintext: MemoPlaintext,
    pub key: PayloadKey,  // Memo sifreleme anahtari
}
```

### Fonksiyonlar

**`fn new<R>(rng, plaintext) -> MemoPlan`** (Satir 17-20):
Rastgele `PayloadKey` olustur.

**`fn memo(&self) -> Result<MemoCiphertext>`** (Satir 23-25):
```rust
MemoCiphertext::encrypt(self.key.clone(), &self.plaintext)
```

### Tasarim Kararlari
- `PayloadKey` plan'a eklenmis — planin serialization'i memo sifreleme anahtarini da kaydeder. Bu, plan kaydedilip yuklendiginde ayni sifreli memo uretilmesini saglar.

---

## [plan/spend.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/plan/spend.rs`
**Satir Sayisi:** 2 (yorum satiri)

### Genel Amac
Daha onceden tasarlanan Spend plan fonksiyonlari buraya tasindi veya silinmis. Sadece:
```rust
// use penumbra_sdk_tct::Tree;
// use {super::*, penumbra_sdk_proto::DomainType, penumbra_sdk_shielded_pool::SpendPlan};
```
Bu dosya icerik icermez — tartisilan bir refactoring sonucu bos kalmis ya da planlanmis bir modul.

---

## [transaction.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/transaction.rs`
**Satir Sayisi:** 793

### Genel Amac
`Transaction` ve `TransactionBody` — tamamlanmis, imzalanmis ve ZK prooflu islem. Zincire gonderilmeye hazir.

### Struct/Enum/Type Listesi

**`struct TransactionBody`** (Satir 41-46):
```rust
pub struct TransactionBody {
    pub actions: Vec<Action>,
    pub transaction_parameters: TransactionParameters,
    pub detection_data: Option<DetectionData>,
    pub memo: Option<MemoCiphertext>,
}
```

**`struct Transaction`** (Satir 126-130):
```rust
pub struct Transaction {
    pub transaction_body: TransactionBody,
    pub binding_sig: Signature<Binding>,  // Deger korumasi imzasi
    pub anchor: tct::Root,               // TCT root (Spend dogrulamasi icin)
}
```

**`struct TransactionSummary`** (Satir 50-53) + **`struct TransactionEffect`** (Satir 56-60):
```rust
pub struct TransactionSummary { pub effects: Vec<TransactionEffect> }
pub struct TransactionEffect { pub address: AddressView, pub balance: Balance }
```
Transaction'in gorunumlu etkilerini ozet. Cuzdan UI icin.

### Fonksiyonlar

**`impl EffectingData for TransactionBody`** (Satir 62-98):
```
state = blake2b "PenumbraEfHs"
state.update(parameters_hash)
state.update(memo_hash)    // None -> sifir hash
state.update(detection_data_hash) // None -> sifir hash
state.update(num_actions.to_le_bytes())
for action:
    state.update(action.effect_hash())
EffectHash(state.finalize())
```
Bu, `TransactionPlan::effect_hash()` ile ayni mantigi takip eder — plan ve transaction ayni effect hash uretir.

**`impl AuthorizingData for TransactionBody`** (Satir 106-116):
```rust
fn auth_hash(&self) -> AuthHash {
    AuthHash(blake2b_simd::Params::default()
        .hash(&self.encode_to_vec())
        .as_bytes()[0..32]
        .try_into()...)
}
```
Tum transaction body'nin blake2b hash'i. Binding signature bu hash'i imzalar.

**`fn decrypt_memo(&self, fvk) -> Result<MemoPlaintext>`** (Satir 169-212):
OVK ile memo cozme:
1. Ilk output'u al (herhangi biri isler)
2. `Note::decrypt_key(ovk_wrapped_key, ...)` -> shared_secret
3. `PayloadKey::derive(shared_secret, epk)` -> action_key
4. `wrapped_memo_key.decrypt_outgoing(action_key)` -> memo_key
   - Basarisizsa: `wrapped_memo_key.decrypt(epk, fvk.incoming())` -> memo_key (alici)
5. `MemoCiphertext::decrypt(&memo_key, ciphertext)` -> plaintext

**`fn payload_keys(&self, fvk) -> BTreeMap<StateCommitment, PayloadKey>`** (Satir 214-284):
Her Output ve Swap icin payload key hesapla:
- Swap: `PayloadKey::derive_swap(fvk.outgoing(), commitment)`
- Output (OVK ile): `Note::decrypt_key(...)` -> shared_secret -> `PayloadKey::derive(...)`
- Output (IVK ile): `fvk.incoming().key_agreement_with(epk)?` -> shared_secret -> `PayloadKey::derive(...)`

**`fn view_from_perspective(&self, txp) -> TransactionView`** (Satir 286-353):
Her aksiyon icin `view_from_perspective` cagrilir. Output'lar icin memo cozme girisimi yapilir. Sonuclar `TransactionView`'a toplanir.

**`fn id(&self) -> TransactionId`** (Satir 575-583):
```rust
let tx_bytes: Vec<u8> = self.clone().try_into()?;
Sha256::digest(&tx_bytes) -> TransactionId
```
Transaction ID = SHA-256(serialized transaction). Effect hash degil, tx hash.

**`fn binding_verification_key(&self) -> VerificationKey<Binding>`** (Satir 586-607):
```rust
let mut balance_commitments = decaf377::Element::default();
for action in &actions {
    balance_commitments += action.balance_commitment().0;
}
let fee_value_commitment = fee.commit(Fr::zero());
balance_commitments += fee_value_commitment.0;
// balance_commitments -> VerificationKey
```
Binding dogrulama anahtari: Tum deger taahhutlerinin + fee taahhudunun toplami. Binding signature bunu dogrular.

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **EffectHash vs AuthHash vs TransactionId**: Uc farkli hash:
   - `effect_hash`: "Bu transaction ne yapar?" — protobuf'a bagimli deil, semantik
   - `auth_hash`: "Bu transaction body nedir?" — tam serialization hash'i
   - `transaction_id`: SHA-256(serializasyon) — zincir uzerinde benzersiz kimlik
2. **Binding key = sum(balance_commitments + fee)**: Matematiksel deger korumasi. `binding_sig` kullanilarak dogrulama yapilir. Bu, transaction'in deger dengesini sifirlatir.
3. **Memo cozme: OVK sonra IVK**: Oncelikle gonderen (OVK), basarisizsa alici (IVK). Bu, hem gonderici hem alici icin memo erisimine izin verir.

### Nova Shield Chain Icin Dersler
- **Uc hash distinksiyonu kritik**: effect_hash (semantik), auth_hash (sirali), tx_id (global kimlik). Nova Shield de bu ucunu ayirdetmeli.
- **Binding signature mekanizmasi**: `sum(balance_commitments) + fee_commitment = binding_verification_key`. Bu ZK-based value conservation.
- Payload key hesaplama (OVK/IVK dual-key) Nova Shield'in view sistemi icin gerekli.
- `view_from_perspective` memo cozme mantigi kopyalanabilir.

---

## [view.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/view.rs`
**Satir Sayisi:** 887 (testler dahil)

### Genel Amac
`TransactionView`, `TransactionBodyView`, `MemoView` ve `MemoPlaintextView` — transaction'in belirli bir perspektiften (gonder/alici) gorunumu.

### Struct/Enum/Type Listesi

**`struct TransactionView`** (Satir 25-29):
```rust
pub struct TransactionView {
    pub body_view: TransactionBodyView,
    pub binding_sig: Signature<Binding>,
    pub anchor: tct::Root,
}
```

**`struct TransactionBodyView`** (Satir 36-41):
```rust
pub struct TransactionBodyView {
    pub action_views: Vec<ActionView>,
    pub transaction_parameters: TransactionParameters,
    pub detection_data: Option<DetectionData>,
    pub memo_view: Option<MemoView>,
}
```

**`enum MemoView`** (Satir 46-54):
```rust
pub enum MemoView {
    Visible { plaintext: MemoPlaintextView, ciphertext: MemoCiphertext },
    Opaque  { ciphertext: MemoCiphertext },
}
```

**`struct MemoPlaintextView`** (Satir 57-61):
```rust
pub struct MemoPlaintextView {
    pub return_address: AddressView,  // Adres gorunumlu veya opak
    pub text: String,
}
```

### Fonksiyonlar

**`fn transaction(&self) -> Transaction`** (Satir 64-95):
`TransactionView` -> `Transaction` donusumu. Action view'lardan action'lar cikartilir. Memo ciphertext memo_view'dan alinir.

**`fn accumulate_effects(summary: TransactionSummary) -> TransactionSummary`** (Satir 103-115):
```rust
let mut keyed_effects: BTreeMap<AddressView, Balance> = BTreeMap::new();
for effect in summary.effects {
    *keyed_effects.entry(effect.address).or_default() += effect.balance;
}
```
Ayni adresin etkilerini birlestirir. `BTreeMap` ile deterministik siralama.

**`fn summary(&self) -> TransactionSummary`** (Satir 118-225):
Her aksiyonun gozlemlenebilir etkisini hesaplar:
- `Spend::Visible`: `+balance` (kullanicidan geldi)
- `Output::Visible`: `-balance` (kullaniciya gitti)
- `Swap::Visible`: `-delta_1 - delta_2 - fee_value` (tum tukenilenler)
- `SwapClaim::Visible`: `+fee_value` (claim ile alinan fee)
- Diger: Atlaniyor

### Test'ler (Satir 427-886)

Dummy tipler olusturuluyor (SpendProof, OutputProof, SwapProof, Spend, Output, Swap, NoteView).

**`test_internal_transfer_transaction_summary`** (Satir 713-789):
2 Spend + 1 Output iceren plan ile TransactionView olusturulur. `summary().effects.len() == 2` bekleniyor.

**`test_swap_transaction_summary`** (Satir 791-885):
2 Spend + 1 Output + 1 Swap iceren plan. `summary().effects.len() == 3` bekleniyor.

### Unsafe Bloklar
Yok.

### Tasarim Kararlari
1. **Visible/Opaque ikili**: Hem `MemoView` hem `ActionView` visible/opaque olabilir. UI icin esnek.
2. **summary() sadece Visible aksiyonlari isleme**: Opaque aksiyonlardan cikarilanlar yapilmaz. Bu, yalnizca gozlemlenebilir etkilerin ozeti.
3. **BTreeMap ile effect birlesimi**: Ayni adresin etkileri toplanir. UI icin temiz bakiye degisimi gosterimi.

### Nova Shield Chain Icin Dersler
- `TransactionView` pattern'i Nova Shield'in view server/wallet icin dogrudan uygulanabilir.
- `summary()` cuzdan bakiye degisimi hesaplamasinda kullanilabilir.
- Visible/Opaque birimi tum sifrelenmis veri tipleri icin uygulanmali.

---

## [view/action_view.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/view/action_view.rs`
**Satir Sayisi:** 199

### Genel Amac
`ActionView` — her aksiyon tipinin gorunumlu versiyonu. "Gizli" aksiyonlar (Spend, Output, Swap, SwapClaim, DelegatorVote) Visible/Opaque dualize olurken, diger aksiyonlar seffaf.

### Struct/Enum/Type Listesi

**`enum ActionView`** (Satir 33-61):
- Sifrelenmis (Visible/Opaque dualize): `Spend(SpendView)`, `Output(OutputView)`, `Swap(SwapView)`, `SwapClaim(SwapClaimView)`, `DelegatorVote(DelegatorVoteView)`, `PositionOpen(PositionOpenView)`
- Seffaf (tek durum): `ValidatorDefinition`, `IbcRelay`, `ProposalSubmit`, `ValidatorVote`, `Delegate`, `Undelegate`, vs.

### Trait Implementasyonlari

**`impl From<ActionView> for Action`** (Satir 162-198):
`ActionView` -> `Action` donusumu. Her view varyantindan gercek aksiyon cikarilir. `DelegatorVoteView` -> `DelegatorVote` donusumu:
```rust
ActionView::DelegatorVote(x) => Action::DelegatorVote(x.into()),
```

**`impl TryFrom<pb::ActionView> for ActionView`** (Satir 67-119):
Protobuf deserialization. `PositionRewardClaim` kullanildiginda hata doner:
```rust
AV::PositionRewardClaim(_) => return Err(anyhow!("PositionRewardClaim is deprecated"))
```
`PositionOpen` (eski format) -> `PositionOpenView::Opaque` olarak parse edilir:
```rust
AV::PositionOpen(x) => ActionView::PositionOpen(PositionOpenView::Opaque { action: x.try_into()? })
```

### Tasarim Kararlari
- Seffaf aksiyonlar view katmaninda ayni kaliyor — extra enum varyanti gereksiz.
- View -> Action donusumu: UI'dan gercek transaction olusturabilirsin.

---

## [view/transaction_perspective.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/view/transaction_perspective.rs`
**Satir Sayisi:** 306

### Genel Amac
`TransactionPerspective` — bir kullanicinin transaction'i gorebilmesi icin gereken tum veri. "View server" bu veriyi hazirlayip istemciye gonderir.

### Struct/Enum/Type Listesi

**`struct TransactionPerspective`** (Satir 18-64):
```rust
pub struct TransactionPerspective {
    pub payload_keys: BTreeMap<note::StateCommitment, PayloadKey>,
    pub spend_nullifiers: BTreeMap<Nullifier, Note>,
    pub advice_notes: BTreeMap<note::StateCommitment, Note>,
    pub address_views: Vec<AddressView>,
    pub denoms: asset::Cache,
    pub transaction_id: TransactionId,
    pub prices: Vec<EstimatedPrice>,
    pub extended_metadata: BTreeMap<asset::Id, Any>,
    pub creation_transaction_ids_by_nullifier: BTreeMap<Nullifier, TransactionId>,
    pub nullification_transaction_ids_by_commitment: BTreeMap<note::StateCommitment, TransactionId>,
    pub batch_swap_output_data: Vec<BatchSwapOutputData>,
    pub position_metadata_key: Option<PositionMetadataKey>,
}
```

- `payload_keys`: Output/Swap'larin sifre cozme anahtarlari
- `spend_nullifiers`: Harcanan nullifier -> plaintext note
- `advice_notes`: SwapClaim cikarilari gibi hesaplanmis notlar
- `creation_transaction_ids_by_nullifier`: Nullifier -> notu yaratan tx
- `nullification_transaction_ids_by_commitment`: Commitment -> notu harcayan tx
- `batch_swap_output_data`: Swap ciktilari
- `position_metadata_key`: LP pozisyon metadata cozme anahtari

### Fonksiyonlar

**`fn view_value(&self, value: Value) -> ValueView`** (Satir 67-72):
Deger + fiyat + extended metadata ile zenginlestirilmis gorunum.

**`fn view_note(&self, note: Note) -> NoteView`** (Satir 74-80):
```rust
NoteView {
    address: self.view_address(note.address()),
    value: self.view_value(note.value()),
    rseed: note.rseed(),
}
```

**`fn view_address(&self, address: Address) -> AddressView`** (Satir 82-87):
Adres bilinen (kullanicinin adresi) ise Visible, bilinmiyorsa Opaque.

**`fn get_and_view_advice_note(&self, commitment) -> Option<NoteView>`** (Satir 89-94):
Advice note'u gorunumle birlikte dondur.

### Protobuf donusumleri
Cok sayida alan serializasyonu. Dikkat cekici: `advice_notes` key olmadan depolanir (note'un kendi commit'i ile indexlenir):
```rust
for note in msg.advice_notes {
    let note: Note = note.try_into()?;
    advice_notes.insert(note.commit(), note);
}
```

### Tasarim Kararlari
1. **TxP veri zenginligi**: TxP sadece view icin gereken minimum degil, ayni zamanda:
   - Gecmis bagi: `creation_transaction_ids_by_nullifier`
   - Gelecek bagi: `nullification_transaction_ids_by_commitment`
   Bu, transaction'i bir "dedektif hikayesi" gibi okutma imkani saglar.
2. **`position_metadata_key: Option<...>`**: Optional olarak tanimlanmis backward compat icin.
3. **Prices ve extended_metadata**: Deger gosterimi icin fiyat bilgisi ve herhangi ekstra metadata.

### Nova Shield Chain Icin Dersler
- `TransactionPerspective` Nova Shield'in view server protokolu icin temel veri modeli.
- `creation/nullification_transaction_ids` zincirleme: Nova Shield'de "bu note nereye gitti?" ve "bu note nereden geldi?" sorularina yanit.
- `position_metadata_key` optional backward compat pattern'i nova Shield'in versiyonlama politikasinda kullanilabilir.

---

## [witness_data.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/src/witness_data.rs`
**Satir Sayisi:** 57

### Genel Amac
`WitnessData` — ZK proof uretimi icin gereken TCT (Tiered Commitment Tree) witness verileri.

### Struct/Enum/Type Listesi

**`struct WitnessData`** (Satir 10-13):
```rust
pub struct WitnessData {
    pub anchor: tct::Root,
    pub state_commitment_proofs: BTreeMap<note::StateCommitment, tct::Proof>,
}
```
Her harcanan note icin TCT Merkle proof. `anchor` = TCT root = proof'un bagli oldugu root.

### Fonksiyonlar

**`fn add_proof(&mut self, nc, proof)`** (Satir 17-19):
Yeni bir proof ekle.

**Protobuf donusumleri** (Satir 26-56):
`WitnessData` -> proto'da anchor + proofs listesi. Dikkat: from'da `state_commitment_proofs` key olarak `tct_proof.commitment()` kullanilir — key'i proofd'an cikar.

### Tasarim Kararlari
- `BTreeMap` ile deterministik proof sirasi.
- Witness data sadece spend'ler icin gerekli; output'lar icin gerekmez.

### Nova Shield Chain Icin Dersler
- Nova Shield'in shielded spend sistemi icin ayni witness veri yapisi.
- TCT (Tiered Commitment Tree) yerine Nova Shield kendi state commitment tree'sini kullanabilir — ancak ayni `(root, proof)` yapisi gerekli.

---

## [tests/generate_transaction_signing_test_vectors.rs]
**Dosya:** `/research/chains/penumbra/crates/core/transaction/tests/generate_transaction_signing_test_vectors.rs`
**Satir Sayisi:** 759

### Genel Amac
`EffectHash` test vektorleri olusturma ve dogrulama. Proptest ile rastgele `TransactionPlan`'lar uretilip JSON/protobuf dosyasina kaydedilir. Sonraki testler bu vektorleri yukleyip dogrular.

### Onemli Stratejiler

**`fn amount_strategy() -> impl Strategy<Value = Amount>`** (Satir 60-63):
0 ile 10^18 arasi rastgele Amount.

**`fn action_plan_strategy(fvk) -> impl Strategy<Value = ActionPlan>`** (Satir 620-647):
`prop_oneof!` ile 24 farkli aksiyon varyantindan rastgele birini uret. `ActionLiquidityTournamentVote` burada YOK.

**`fn dutch_auction_schedule_gas_cost` test verisi** (Satir 569-595):
`end_height: start_height + 1` — tek adimli auction. `step_count` rastgele.

### Test Fonksiyonlari

**`generate_transaction_signing_test_vectors`** (Satir 679-726):
`#[ignore]` — varsayilan olarak calistirılmaz. 100 TransactionPlan uretir, JSON + proto + effect_hash dosyasi yazar.

**`effect_hash_test_vectors`** (Satir 728-758):
Kayitli vektorleri yukleyip effect_hash'i yeniden hesaplar ve karsilastirir:
```rust
let transaction_plan = TransactionPlan::decode(&transaction_plan_encoded[..])?;
let effect_hash_hex = hex::encode(transaction_plan.effect_hash(fvk)?.0);
assert_eq!(effect_hash_hex, expected_effect_hash);
```
Bu test, serialization/deserialization sonrasi effect hash'in degismedigini dogrular.

### Tasarim Kararlari
1. **Proptest ile test vektoru uretimi**: Rastgele vektorler, uc case'lerin tespitini saglar. Ancak vektorler commit edilmis — deterministik tekrar edilebilirlik.
2. **Hem JSON hem proto**: JSON insan okunabilir test ciktisi, proto machine-readable. Ikisi de tutulmali.

### Nova Shield Chain Icin Dersler
- Nova Shield'in transaction sistemi icin `effect_hash` test vektorleri AYNI sekilde olusturulmali.
- `#[ignore]` flag ile vektor uretici, `cargo test -- --ignored` ile manuel calistirili.
- `proptest` ile effect hash testi: Kod degisikliginde hash degisecek mi? Degerli bir guvenlik neti.

---

## GENEL TASARIM KALIPLARI VE PRENSIPLER

### 1. Lifecycle Katmanlari
```
ActionList -> TransactionPlan -> (ZK + signing) -> Transaction -> TransactionView
```
Her katman daha az bilgi ve daha fazla taahhut icerir. Plan katmaninda her seyi degistirebilirsin; transaction katmaninda hicbir sey degistirilemez.

### 2. Effect Hash Tutarliligi
Plan ve Transaction ayni effect hash'i uretir. Bu, donanim cuzdan/hardware wallet guvenliginin temelidir — ZK proof uretilmeden once islem imzalanabilir.

### 3. Visible/Opaque Dualize
Her sifrelenmis veri tipi iki halde gelir: bilgi varsa Visible, yoksa Opaque. Bu, parcali bilgi aciklamasina izin verir.

### 4. Domain Separationlar
- Blake2b personalize "PenumbraEfHs" — effect hash
- SpendAuth domain tag — spend imzasi
- PayloadKind::Memo — memo sifrelemesi
- PayloadKind::Note — not sifrelemesi

### 5. Cok Boyutlu Gas Modeli
`block_space + compact_block_space + verification + execution` — dort boyut farkli donanim ve islem maliyet profilleri icin fiyatlandirmayi hassaslastirir.

### 6. Deterministik Siralama
`sort_actions()` ile canonical siralama, `BTreeMap` ile deterministik veri yapilari. Farkli implementasyonlar ayni bit dizisi uretir.

---

## GENEL NOVA SHIELD CHAIN ICIN DERSLER

### Mimari
1. **Plan-Transaction-View uc katmani aynen uygulanmali**. Plan katmani hardware wallet destegi icin zorunlu.
2. **`#![deny(clippy::unwrap_used)]`** tum crate'lere uygulanmali — production quality.
3. **`Compat<'a, T>` orphan rule pattern** crate bolunmelerinde kullanilabilir.

### Gas Sistemi
4. **4 boyutlu gas** (block_space, compact_block_space, verification, execution) kopyalanmali.
5. **ZK proof verification gas = 1000 (sabit)** — Groth16 boyutu sabittir.
6. **Dutch auction execution gas = 2 * step_count * 20** — adim-orantili.
7. **Padding tesviki** (PositionOpenPlan) — metadata eklemek gas'i dusurur.

### Guvenlik
8. **Randomized spend key** — farkli spend'ler baglanamaz.
9. **Effect hash imzalama** — "ne etkisi var?" semantigi imzalanir, "hangi byte'lar?" degil.
10. **Binding signature = sum(value_blindings)** — matematiksel deger korumasi.
11. **chain_id + expiry_height** replay ve stale transaction korumasinda zorunlu.
12. **Sabit boyutlu ciphertext** — trafik analizine karsi.

### DEX
13. **PositionWithdraw NFT state transition** — LP pozisyon yasamdongusunu deger dengesi icinde modellemek, Nova DEX icin direkt kullanilabilir.
14. **SwapClaim fee birikimi** — swap on-odemesinin transaction fee'sine uygulanmasi, Nova DEX icin kullanilabilir.
15. **`batch_swap_output_data` pro-rata hesabi** — her swap'in beklenen cikatisini belirler.

### View Sistemi
16. **TransactionPerspective** — view server'in hazirlayip istemciye gonderdigi veri yapisi.
17. **creation/nullification transaction chain** — "bu note nereden geldi/nereye gitti?" sorularini yanit.
18. **`advice_notes`** — onceden hesaplanmis notlar (SwapClaim cikarilari).

### Test
19. **Proptest ile effect_hash test vektorleri** — code degisikliginde hash stability kontrolu.
20. **`#[ignore]` flag** ile vektor uretici ve `cargo test -- --ignored` ile manuel calistirma.
21. **Plan-Transaction effect hash uyumu testi** — `plan.effect_hash == tx.effect_hash` kritik bir duzgunluk kontrouldur.
