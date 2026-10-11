# Penumbra Stake Component Analizi — Agent 10

**Analiz Tarihi:** 2026-07-22
**Kapsam:** `crates/core/component/stake/src/` — 43 dosya, tam analiz
**Analist:** Agent-10

---

## Genel Bakis

Penumbra'nin staking modülü, Cosmos/Tendermint tarzından belirgin şekilde ayrışır. Temel farklar:

1. **Epoch-bazlı delegasyon**: Delegasyonlar ve undelegasyonlar blok bazında değil, epoch sınırında işlenir. İşlemler kuyrukta bekler.
2. **Exchange rate modeli**: Doğruca staking token yerine, delegation token / staking token exchange rate tutulur. Bu Cosmos'daki shares modeline benzer ama daha explicit.
3. **ZK proof ile undelegation claim**: Unbonding token'ları claim etmek için Groth16 tabanlı ZK kanıtı zorunludur.
4. **Çift state machine**: Validator'lar hem `validator::State` (Defined/Inactive/Active/Jailed/Tombstoned/Disabled) hem de `validator::BondingState` (Bonded/Unbonding/Unbonded) state machine'lerine tabidir.
5. **BPS^2 scaling**: Tüm rate verileri `1_0000_0000` (= 10^8) sabit noktalı scale factor ile temsil edilir.

---

## Dosya Analizi

---

## [changes.rs]
**Dosya:** `.../stake/src/changes.rs`
**Satır Sayısı:** 44

### Genel Amaç
Bir epoch boyunca biriken delegasyon ve undelegasyon değişikliklerini tutmak için kullanılan veri yapısı. Bu değişiklikler her blok sonunda kaydedilir, epoch sonunda toplu işlenir.

### Struct/Enum/Type Listesi

- **`DelegationChanges`** (satır 10-13):
  ```rust
  pub struct DelegationChanges {
      pub delegations: Vec<Delegate>,
      pub undelegations: Vec<Undelegate>,
  }
  ```
  - `#[derive(Debug, Clone, Default, Serialize, Deserialize)]`
  - `Default` derive: Boş vektörlerle başlangıç, her blok için delegation değişikliği yoksa null yerine `Default::default()` kullanılır.

### Trait Implementasyonları

- `DomainType for DelegationChanges`: Proto tipi `pb::DelegationChanges`.
- `From<DelegationChanges> for pb::DelegationChanges`: Vec iter + `Into::into()` ile her eleman dönüştürülür.
- `TryFrom<pb::DelegationChanges> for DelegationChanges`: Her eleman `TryInto::try_into()` ile parse edilir; hata varsa erken dönüş.

### Tasarım Kararları

- Delegasyon değişiklikleri tek bir blok için `DelegationChanges` içinde toplanır.
- `Default` derive kritiktir: Her blok için her validator bir delegasyon göndermiyor olabilir; başlangıç değeri boş koleksiyon.
- JMT (Jellyfish Merkle Tree) içinde `by_height` key ile saklanır (bkz. `state_key.rs`).

### Nova Shield Chain İçin Dersler

- Nova Shield için de delegasyon değişikliklerini epoch sınırında toplu işlemek mantıklıdır. Bu hem batching hem de atomicity sağlar.
- `Default` derive, blok başlangıcında sıfır delegasyonu ifade etmek için yeterli. Explicit zero-value yazmaktan daha temiz.
- Proto serializasyon paterni (`DomainType` trait) tüm domain tiplerinde tekrarlanıyor — Nova Shield için bu paterne uymak protobuf etkileşimi kolaylaştırır.

---

## [component/action_handler.rs]
**Dosya:** `.../stake/src/component/action_handler.rs`
**Satır Sayısı:** 7

### Genel Amaç
Action handler modüllerini declare eder. Gerçek implementasyon alt modüllerdedir.

### Modüller
- `delegate`, `undelegate`, `undelegate_claim`, `validator_definition` — her transaction action'ı için ayrı dosya.
- `use cnidarium_component::ActionHandler` — blankname import.

### Tasarım Kararları
- Her action tipi için ayrı dosya: tek-sorumluluk prensibi, yüzlerce satırlık tek dosya yerine küçük odaklanmış modüller.

---

## [component/action_handler/delegate.rs]
**Dosya:** `.../stake/src/component/action_handler/delegate.rs`
**Satır Sayısı:** 138

### Genel Amaç
`Delegate` action'ının stateless ve stateful doğrulamasını ve yürütmesini implement eder.

### Trait Implementasyonları

**`impl ActionHandler for Delegate`** (satır 15):
- `type CheckStatelessContext = ()` — stateless kontrol için context gerekmez.

### Fonksiyonlar

#### `check_stateless` (satır 17-20)
- Hiçbir stateless kontrol yok. Boş OK döner.
- **Tasarım notu**: Delegation'ın geçerliliği tamamen state'e bağlıdır (validator'ın varlığı, epoch index eşleşmesi vb.).

#### `check_and_execute` (satır 22-137)
Parametreler: `&self` (Delegate), `mut state: S` (StateWrite)

Adımlar:
1. **Epoch index kontrolü** (satır 31-37): Delegation hangi epoch için hazırlandıysa o epoch'un gerçek epoch ile eşleşmesi gerekir:
   ```
   ensure!(d.epoch_index == current_epoch.index, ...)
   ```
2. **Delegation amount doğrulaması** (satır 39-67): Validator'ın mevcut exchange rate'inden `delegation_amount` hesaplanır:
   ```
   expected = validator_rate.delegation_amount(d.unbonded_amount)
   ensure!(expected == d.delegation_amount, ...)
   ```
   - Rounding yönü: Kullanıcı `unbonded_amount`'dan başlar, `delegation_amount`'ı rate'ten hesaplar.
   - Tersine (delegation -> unbonded) hesap yapılmaz çünkü yuvarlama farkı oluşabilir.
3. **Validator enabled check** (satır 81-85): `validator.enabled == true` zorunlu.
4. **Validator state check** (satır 87-92): Sadece `Defined | Inactive | Active` durumlarına delegasyon izni var.
5. **Minimum stake kontrolü** (satır 110-130): Eğer validator `Defined` durumundaysa ve pool boşsa, `unbonded_delegation >= min_validator_stake` zorunlu.
6. **Queue'ya ekleme** (satır 133-135): `state.push_delegation(self.clone())` ile epoch sonuna kadar beklemek üzere kuyruğa alınır.

### Matematiksel Formüller

- **Delegation miktarı**: `delegation_amount = floor(unbonded_amount / validator_exchange_rate)` (rate.rs'den)
- Rounding: her zaman `floor` (aşağıya yuvarla).

### Tasarım Kararları

- **Çift taraflı rounding güvencesi**: Hem delegation hem undelegation işlemlerinde tek yön hesap yapılır. Hangi yönden hesap yapılacağı, kullanıcının başlangıç değerine göre belirlenir.
- **Epoch sonu işlemi**: Delegasyon hemen etki etmez; epoch sonunda toplu işlenir. Bu, bir epoch içindeki tüm delegasyonların tutarlı bir rate ile işlenmesini sağlar.
- **Defined validator minimum stake**: Issue #3853 — ilk delegasyon en az `min_validator_stake` olmalı, aksi halde validator asla `Inactive` durumuna geçemez.

### Nova Shield Chain İçin Dersler

- "Delegation amount, rate'ten türetilir" modeli doğru. Kullanıcının kendi hesapladığı miktarı kabul etmek yerine on-chain hesaplamayı doğrulayın.
- Minimum self-delegation gereksinimi, "dust validator" sorununu engeller: staking token deposu çok küçük validator'lar consensus set'e giremez.
- Epoch bazlı delegasyon işleme için `push_to_queue` + `process_at_epoch_end` pattern'i kullanılabilir.

---

## [component/action_handler/undelegate.rs]
**Dosya:** `.../stake/src/component/action_handler/undelegate.rs`
**Satır Sayısı:** 92

### Genel Amaç
`Undelegate` action'ının doğrulama ve yürütme logic'ini implement eder.

### Fonksiyonlar

#### `check_stateless`
Boş OK döner. Hiç stateless check yok.

#### `check_and_execute`
Adımlar:
1. **Epoch kontrolü** (satır 30-43): `u.from_epoch == current_epoch`. Eğer eşleşmezse kullanıcı başka bir epoch için hazırlamış demektir (epoch boundary geçilmiş olabilir).
2. **Unbonded amount doğrulaması** (satır 45-78): Delegate'in tersine, burada `delegation_amount` başlangıç noktasıdır:
   ```
   expected_unbonded = rate_data.unbonded_amount(u.delegation_amount)
   ensure!(u.unbonded_amount == expected_unbonded, ...)
   ```
3. **Denom registrasyonu** (satır 83): Unbonding token'ının denom'u asset registry'ye kaydedilir.
4. **Kuyruğa ekleme** (satır 86): `state.push_undelegation(self.clone())`.

### Matematiksel Formüller

- **Unbonded amount**: `unbonded_amount = floor(delegation_amount * validator_exchange_rate)`
- Yuvarlama: `floor` (aşağıya).

### Tasarım Kararları

- Unbonding token denom'unun daha bu aşamada kaydedilmesi, istemcilerin daha sonra bu denom'u sorgulayabilmesi için gereklidir.
- Kullanıcı delegation token'dan başlar; çünkü elinde bu token vardır ve almak istediği unbonded miktarı rate'ten hesaplar.

### Nova Shield Chain İçin Dersler

- Asset denom kaydı, token kullanılabilir hale gelmeden önce yapılmalıdır. Nova Shield'in herhangi bir "synthetic token" (unbonding token, LP token gibi) oluşturduğu durumlarda denom registry'ye erken kayıt yapılmalı.
- Epoch kontrolü hem kullanıcı için hata mesajı hem de güvenlik önlemi: eski epoch'ta hazırlanmış bir işlem geçersiz rate kullanmış olabilir.

---

## [component/action_handler/undelegate_claim.rs]
**Dosya:** `.../stake/src/component/action_handler/undelegate_claim.rs`
**Satır Sayısı:** 98

### Genel Amaç
Unbonding token'larını staking token'larına dönüştürme (claim) işleminin doğrulama mantığı. ZK kanıt doğrulaması da burada gerçekleşir.

### Fonksiyonlar

#### `check_stateless` (satır 16-33)
Adımlar:
1. Unbonding token'ın asset ID'si oluşturulur: `UnbondingToken::new(validator_identity, unbonding_start_height).id()`
2. ZK kanıtı doğrulanır:
   ```rust
   self.proof.verify(&CONVERT_PROOF_VERIFICATION_KEY, UndelegateClaimProofPublic {
       balance_commitment: self.body.balance_commitment,
       unbonding_id,
       penalty: self.body.penalty,
   })?;
   ```
- **Stateless ZK doğrulama**: Proof herhangi bir state erişimi gerektirmeden doğrulanabilir — bu önemli bir scalability avantajı.

#### `check_and_execute` (satır 35-97)
Adımlar:
1. **Yükseklik kontrolü** (satır 42-45): `current_height >= unbonding_start_height`
2. **Unbonding height hesabı** (satır 49-52): `compute_unbonding_height(validator_identity, unbonding_start_height)` — validator'ın mevcut bonding state'ine bakarak ne zaman unbond edilebileceğini hesaplar.
3. **Bekleyen blok sayısı** (satır 54): `wait_blocks = allowed_unbonding_height.saturating_sub(current_height)`
4. **Unbonding bitme kontrolü** (satır 56-62): `current_height >= allowed_unbonding_height`
5. **Epoch aralığı hesabı** (satır 64-67): `unbonding_epoch_start` ve `unbonding_epoch_end` elde edilir.
6. **Penalty doğrulama** (satır 78-91): `compounded_penalty_over_range(start, end)` ile beklenen penalty hesaplanır ve `self.body.penalty` ile karşılaştırılır.
7. **State değişikliği yok**: Bu action sadece token dönüşümü sağlar; tüm etki ZK kanıt ve balance commitment üzerinden olur.

### Tasarım Kararları

- **ZK kanıtı stateless aşamada doğrulanır**: En pahalı işlem (`verify()`) state erişimi olmaksızın yapılabilir. Bu, mempool'da erken eleme sağlar.
- **Hiç state değişikliği**: Action, balance commitment üzerinden etki eder. "Bu kadar unbonding token yakıldı, bu kadar staking token üretildi" görüntüsü ZK kanıtıyla ortaya konur.
- **Penalty aralığı sorgulama**: `compounded_penalty_over_range` birden fazla epoch'ta uygulanan slashing penalty'leri birleştirir.

### Nova Shield Chain İçin Dersler

- ZK kanıt doğrulamasını stateless aşamaya taşımak, state bottleneck'ini azaltır. Nova Shield'in ZK tabanlı işlemlerinde bu ayrımı uygulayın.
- Unbonding claim için time-lock mekanizması kritik: doğru height check + penalty hesabı atomik yapılmalı.
- Penalty'nin ZK kanıt içinde yer alması (public input olarak), kullanıcının doğru penalty değerini beyan etmesini zorunlu kılar; aksi halde kanıt reddedilir.

---

## [component/action_handler/validator_definition.rs]
**Dosya:** `.../stake/src/component/action_handler/validator_definition.rs`
**Satır Sayısı:** 148

### Genel Amaç
Validator tanımı publish etme veya güncelleme action'ının doğrulama ve yürütme mantığı.

### Fonksiyonlar

#### `check_stateless` (satır 18-67)
Adımlar:
1. **Website uzunluk limiti** (satır 22-24): `> 70` karakter yasak.
2. **Name uzunluk limiti** (satır 26-28): `> 140` karakter yasak.
3. **Description uzunluk limiti** (satır 30-32): `> 280` karakter yasak.
4. **Funding stream sayısı** (satır 34-36): En fazla 8.
5. **sequence_number == u32::MAX && enabled** (satır 43-45): Son sequence numarasına ulaşılmış bir validator aktif olamaz. **Güvenlik tasarımı**: Bir saldırgan validator anahtarını ele geçirse bile, validator'ı sonsuza kadar etkin bırakamaz; çünkü son sequence number ile yalnızca `enabled=false` geçerlidir.
6. **İmza doğrulama** (satır 47-51): Validator'ın identity key'i ile `validator` alanının proto-encoding'i imzalanmış olmalı.
7. **Toplam commission** (satır 53-65): `funding_streams.iter().map(rate_bps).sum() <= 10_000 bps` (= 100%).

#### `check_and_execute` (satır 70-147)
Adımlar:
1. **Sequence number artışı** (satır 82-93): Mevcut validator varsa `new_seq > old_seq` zorunlu. Replay saldırısı engellenir.
2. **Consensus key teklik kontrolü** (satır 96-117): Eğer consensus key zaten başka bir validator tarafından kullanılıyorsa reddedilir. Duplicate consensus key CometBFT'yi kilitler (satır 108-112'de açıklanmış).
3. **Yeni validator** (satır 130-143): `add_validator()` çağrısı:
   - `validator_reward_rate: 0u128.into()`
   - `validator_exchange_rate: 1_0000_0000u128.into()` (= 1.0, bps^2 scaling ile)
4. **Mevcut validator güncelleme** (satır 123-128): `update_validator_definition()`.

### Tasarım Kararları

- **sequence_number u32::MAX guard**: Saldırganın ele geçirdiği validator'ı `enabled=true` ile sonsuza kadar zorla aktif tutmasını engeller. Orijinal operatör `u32::MAX + 1` yapamaz (overflow), ama `enabled=false` ile validator'ı kapatabilir.
- **Initial rate**: Yeni validator `exchange_rate=1.0`, `reward_rate=0` ile başlar. İlk epoch'ta reward kazanamaz; bu consensus seti stabilize eder.

### Nova Shield Chain İçin Dersler

- `sequence_number` guard, key compromise senaryosuna karşı zarif bir çözüm. Nova Shield validator key yönetiminde bunu uygulayın.
- Consensus key teklik kontrolü CometBFT/Tendermint ile doğrudan etkileşimde kritik.
- Initial exchange rate 1.0 ile başlamak, genesis ve post-genesis validator'lar arasında tutarlılık sağlar.

---

## [component/epoch_handler.rs]
**Dosya:** `.../stake/src/component/epoch_handler.rs`
**Satır Sayısı:** 528

### Genel Amaç
Epoch sonu işlemlerini implement eden `EpochHandler` trait'i. Tüm staking hesaplamaları buradadır.

### Trait Implementasyonları

**`pub trait EpochHandler: StateWriteExt + ConsensusIndexRead`** (satır 30)

#### `end_epoch` (satır 33-159) — ANA METOD
Adımlar:
1. **Delegasyon değişikliklerini topla** (satır 34-81): `epoch_to_end.start_height..=end_height` aralığındaki her blok için `get_delegation_changes(height)` çağrılır. `delegations_by_validator` ve `undelegations_by_validator` BTreeMap'lerinde biriktirilir. `saturating_add` overflow koruması.
2. **Base rate hesabı** (satır 84): `process_chain_base_rate()` — issuance budget / total_active_stake.
3. **Validator kümesi birleşimi** (satır 86-110): Delegasyon değişikliği olan validator'lar ∪ consensus set → işlenecek validator kümesi.
4. **Her validator işle** (satır 112-143): `process_validator()` çağrısı, reward kuyruğuna ekleme.
5. **Sanity check** (satır 149-150): Tüm delegasyon değişikliklerinin işlendiğini doğrula (`assert!`).
6. **Staking reward kuyruğu** (satır 154): Funding component'ın tüketmesi için kaydet.
7. **Active/Inactive validator kümesi** (satır 158): `set_active_and_inactive_validators()`.

#### `process_validator` (satır 162-316)
Adımlar:
1. Validator tanımı ve state'ini al.
2. **Önceki rate'e penalty uygula** (satır 191-201):
   ```rust
   let penalty = get_penalty_in_epoch(identity, epoch_to_end.index).unwrap_or(0%);
   let prev_rate_with_penalty = prev_validator_rate.slash(penalty);
   ```
3. **Sonraki rate hesabı** (satır 204-208):
   ```rust
   let next_validator_rate = prev_rate_with_penalty.next_epoch(
       &next_base_rate, validator.funding_streams.as_ref(), &validator_state
   );
   ```
4. **Delegation delta hesabı** (satır 214-215):
   ```rust
   let delegation_delta = (total_delegations.value() as i128) - (total_undelegations.value() as i128);
   ```
   i128 kullanımı: toplam supply << 2^107 olduğu için safe.
5. **Pool büyütme/küçültme** (satır 230-242): `increase_validator_pool_size` veya `decrease_validator_pool_size`.
6. **Voting power hesabı** (satır 250-251):
   ```rust
   let voting_power = next_validator_rate.voting_power(delegation_token_supply);
   ```
7. **Rate ve power güncelleme** (satır 262-263).
8. **Ödül kuyruğu** (satır 267-278): Yalnızca `Active` state validator'lar için.
9. **Precursor transition** (satır 281-288): Minimum stake eşiği kontrol edilerek state geçişi denenebilir.
10. **Pool state işleme** (satır 298-302): Unbonding tamamlandıysa `Unbonded`'a geç.
11. **Consensus set güncelleme** (satır 311-313): `belongs_in_index` false ise consensus set'ten çıkar.

#### `process_chain_base_rate` (satır 319-367)
```rust
base_reward_rate = U128x128::ratio(issuance_budget_for_epoch, total_active_stake_previous_epoch)
base_reward_rate = (base_reward_rate * BPS_SQUARED_SCALING_FACTOR).round_down()
next_base_rate = prev_base_rate.next_epoch(base_reward_rate)
```

**Formül:**
```
base_reward_rate[e] = issuance_budget[e] / total_active_stake[e-1]
base_exchange_rate[e] = base_exchange_rate[e-1] * (1 + base_reward_rate[e])
```

#### `set_active_and_inactive_validators` (satır 371-418)
- Consensus set'teki tüm validator'ların güçlerini topla.
- Zero-power olanları `Inactive` kümesine al.
- Geri kalanları `voting_power` azalan sıraya göre sırala.
- İlk `active_validator_limit` (default 80) validator `Active`, geri kalanlar `Inactive`.

#### `build_cometbft_validator_updates` (satır 425-525)
- `JoinSet` ile concurrent fetch: tüm consensus set validator'larının state, power, consensus_key bilgileri paralel çekilir.
- Non-active validator'ların effective_power = 0.
- Zero-power AND daha önce CometBFT'ye bildirilmemişse hariç tutulur.
- Daha önce bilinen sıfır-power validator'lar silinmek üzere gönderilir.

### Matematiksel Formüller

**Base Reward Rate:**
```
r_e = issuance_budget_e / total_active_stake_{e-1}
(scaled: r_e_bps_sq = r_e * 1_0000_0000)
```

**Base Exchange Rate:**
```
BXR[e] = BXR[e-1] * (1 + r_e)
```

**Validator Reward Rate (Commission ile):**
```
VRR[e] = r_e * (1 - commission_rate)
commission_rate = sum(funding_stream.rate_bps) / 10_000
```

**Validator Exchange Rate:**
```
VXR[e] = VXR[e-1] * (1 + VRR[e])
```

**Voting Power:**
```
voting_power = delegation_pool_size * VXR[e]
(= staking token equivalent of delegation tokens)
```

**Delegation Amount (from unbonded):**
```
delegation_amount = floor(unbonded_amount / VXR[e])
```

**Unbonded Amount (from delegation):**
```
unbonded_amount = floor(delegation_amount * VXR[e])
```

### Tasarım Kararları

- **i128 for delegation_delta**: Delegasyon eksi undelegasyon hesabı negatif olabilir. i128 kullanımı hem işaretli hem de yeterince büyük.
- **saturating_add/saturating_sub**: Overflow'a karşı koruma; overflow gerçekleşirse panic yerine max değerde durur.
- **assert! sonrası temiz delegasyon haritaları**: Epoch sonunda tüm delegasyon değişikliklerinin işlendiği invariant doğrulanır.
- **Concurrent JoinSet ile CometBFT güncellemesi**: Validator sayısı arttıkça sequential fetch yerine tokio::task::JoinSet ile paralel fetch scalability sağlar.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler

- **Epoch-bazlı toplu delegasyon**: Blok-bazlı anlık işlem yerine epoch sonu batch işleme, hem hesap tutarlılığı hem de simplicity sağlar.
- **Base rate formülü**: `issuance / active_stake` oranı basit ve anlaşılır. Nova Shield token issuance'ini benzer şekilde staking durumuna göre dinamik ayarlayabilir.
- **i128 delegation_delta**: Delegasyon ve undelegasyonu birleştirmek için i128 kullanımı net ve güvenli. i64 yeterli ama i128 ile overflow riski sıfır.
- **Concurrent validator güncellemesi**: 100+ validator için tokio::task::JoinSet ile paralel state okuma kritik.
- **MAX_VOTING_POWER = i64::MAX / 8**: CometBFT'nin iç limiti. Nova Shield kendi consensus motoru için benzer bir upper bound belirlemelidir.

---

## [component/metrics.rs]
**Dosya:** `.../stake/src/component/metrics.rs`
**Satır Sayısı:** 26

### Genel Amaç
Staking modülü için Prometheus metrik tanımları.

### Fonksiyonlar

#### `register_metrics()`
```rust
describe_gauge!(MISSED_BLOCKS, Unit::Count, "The number of missed blocks per validator");
```

### Const'lar
- `MISSED_BLOCKS: &str = "penumbra_stake_missed_blocks"` — metrik adı.

### Tasarım Kararları
- Metrics crate'i shadow pattern: `use crate::metrics;` deyimi ile metrics crate monkey-patched. Bu, metrik string'lerini merkezi tutarken crate içi erişimi kolaylaştırır.

### Nova Shield Chain İçin Dersler
- Her modül kendi metrik sabitlerini `const &str` olarak tanımlasın. Prometheus entegrasyonunda label isimleri tutarlı olsun.
- `missed_blocks` gibi validator operasyonel metrikleri zorunlu. Nova Shield'in monitoring altyapısı için bu pattern takip edilmeli.

---

## [component/mod.rs]
**Dosya:** `.../stake/src/component/mod.rs`
**Satır Sayısı:** 17

### Genel Amaç
Staking component'inin public API'sini dışa aktarır.

### Önemli Sabit
```rust
pub const MAX_VOTING_POWER: u128 = 1152921504606846975; // i64::MAX / 8
```
CometBFT'nin üst sınırı. Kaynak: `tendermint/types/validator_set.go:25`.

### Dışa Aktarılanlar
- `Staking` struct (Component trait implementation)
- `ConsensusIndexRead`, `SlashingData`, `StateReadExt`, `StateWriteExt`
- `ValidatorPoolDeposit`

---

## [component/rpc.rs]
**Dosya:** `.../stake/src/component/rpc.rs`
**Satır Sayısı:** 237

### Genel Amaç
gRPC query servisi implementasyonu. `QueryService` trait'ini implement eder.

### Struct'lar

- **`Server`** (satır 23-25): `Storage` bağımlılığı tutar.

### Fonksiyonlar (gRPC endpoint'leri)

| Endpoint | İstek | Yanıt |
|---|---|---|
| `get_validator_info` | `GetValidatorInfoRequest` | `GetValidatorInfoResponse` |
| `validator_info` | `ValidatorInfoRequest` | Stream `ValidatorInfoResponse` |
| `validator_status` | `ValidatorStatusRequest` | `ValidatorStatusResponse` |
| `validator_penalty` | `ValidatorPenaltyRequest` | `ValidatorPenaltyResponse` |
| `current_validator_rate` | `CurrentValidatorRateRequest` | `CurrentValidatorRateResponse` |
| `validator_uptime` | `ValidatorUptimeRequest` | `ValidatorUptimeResponse` |

#### `validator_info` (satır 71-136) — Streaming endpoint
```rust
type ValidatorInfoStream = Pin<Box<dyn Stream<Item=Result<ValidatorInfoResponse, Status>> + Send>>;
```
- `async_stream::try_stream!` makrosu ile streaming response oluşturulur.
- `show_inactive` parametresiyle aktif olmayan validator'lar filtrelenebilir.
- `filter_inactive` closure: `Active` veya `show_inactive=true` ise göster.

#### `validator_penalty` (satır 163-184)
- `compounded_penalty_over_range` çağrısı; [start_epoch, end_epoch) aralığında compounded penalty döner.

### Tasarım Kararları
- **Streaming response**: Validator listesi büyük olabilir, tek response yerine stream tercih edilmiş.
- **`tap` kullanımı**: `TapOptional`, `TapFallible` ile logging side-effects zincire eklenir; kodun akışını bozmaz.
- **latest_snapshot()**: Her request için en güncel state snapshot'ı alınır.

### Nova Shield Chain İçin Dersler
- Validator list endpoint'i için streaming response zorunludur — blok limitlerini aşan veri boyutlarında tek response başarısız olur.
- `TapOptional`/`TapFallible` pattern logging için temiz alternatif; `match` bloğu yazmak yerine `.tap_none(|| log!(...))` yeterli.

---

## [component/stake.rs]
**Dosya:** `.../stake/src/component/stake.rs`
**Satır Sayısı:** 597

### Genel Amaç
`Staking` component'inin `Component` trait implementasyonu, çok sayıda extension trait.

### Struct'lar

- **`Staking {}`** (satır 37-39): Boş struct; state bağımsız, tüm state `StateWrite`/`StateRead` üzerinden erişilir.

### Trait Implementasyonları

#### `impl Component for Staking` (satır 42-178)

**`init_chain`** (satır 49-119):
1. `StakeParameters` kaydedilir.
2. Genesis base rate: `base_reward_rate=0, base_exchange_rate=1_0000_0000`.
3. Genesis allocation'larından her validator'ın delegation token miktarı hesaplanır.
4. Tüm genesis validator'lar `add_genesis_validator` ile eklenir.
5. `CurrentConsensusKeys::default()` ile başlangıç consensus key listesi oluşturulur.
6. Blok başlangıcında boş delegasyon değişikliği kaydedilir.
7. CometBFT validator güncellemesi oluşturulur.

**`begin_block`** (satır 121-140):
1. Byzantine evidence işlenir: `process_evidence(evidence)` — slashing tetiklenir.
2. Uptime takibi: `track_uptime(&last_commit_info)`.

**`end_block`** (satır 142-156):
- Delegasyon değişikliklerini bu blok için JMT'ye yazar: `set_delegation_changes(height, changes)`.

**`end_epoch`** (satır 158-177):
1. `end_epoch(epoch_ending)` — tüm epoch işlemleri.
2. `build_cometbft_validator_updates()` — CometBFT güncellemesi.

#### `StateReadExt` trait (satır 206-270)
- `get_stake_params()`: JMT'den `StakeParameters` okur.
- `signed_blocks_window_len()`, `missed_blocks_maximum()`: Parametrelerden türetilir.
- `get_delegation_changes_tally()`: Object storage'dan blok tally'si.
- `get_current_base_rate()`, `get_previous_base_rate()`: Base rate oku.
- `get_funding_queue()`: Staking reward kuyruğu.
- `get_delegation_changes(height)`: Belirli blok yüksekliğindeki değişiklikler.

#### `StateWriteExt` trait (satır 276-338)
- `put_stake_params()`, `put_delegation_changes()`, `push_delegation()`, `push_undelegation()`: State mutasyonları.
- `queue_staking_rewards()`: Funding component'ın tüketmesi için.
- `register_consensus_key()`: İki dizin günceller:
  - `cometbft_address -> PublicKey`
  - `PublicKey -> IdentityKey`
  - **Silinmez!** Eski kayıtlar korunur; byzantine evidence attribution için kritik.

#### `SlashingData` trait (satır 344-391)
- `get_penalty_in_epoch(id, epoch_index)`: Tek epoch'ta penalty.
- `get_penalty_for_range(id, start, end)`: `[start, end)` aralığı.
- `compute_compounded_penalty(penalties: Vec<Penalty>) -> Penalty`: fold ile `compound` zinciri.
- `compounded_penalty_over_range(id, start, end)`: Tam range penalty.

**Compound formülü:**
```
compound(p1, p2) = p1.kept_rate * p2.kept_rate
// Örnek: 10% + 20% penalty = 1.0 * 0.90 * 0.80 = 0.72 → %28 toplam kayıp
```

#### `InternalStakingData` trait (satır 393-444)
- `total_active_stake()`: Tüm Active validator'ların `unbonded_amount(delegation_token_supply)` toplamı.

#### `ConsensusIndexRead` trait (satır 511-565)
- `consensus_set_stream()`: JMT prefix scan → validator identity key stream.
- `belongs_in_index(id)`: `Active | Inactive` ise true.

#### `ConsensusIndexWrite` trait (satır 570-597)
- `add_consensus_set_index()`, `remove_consensus_set_index()`: Nonverifiable storage'a yaz/sil.

### Tasarım Kararları

- **Consensus key silinmez**: `register_consensus_key` eski kayıtları asla silmez. Byzantine evidence, eski consensus key ile gelse bile validator identity'sine ulaşılabilir.
- **Object vs JMT storage**: Geçici/blok-içi veriler object storage, kalıcı veriler JMT'de. `get_delegation_changes_tally()` object_get kullanır; `get_delegation_changes(height)` JMT'den okur.
- **Blanket impl**: `impl<T: StateRead + ?Sized> SlashingData for T {}` ile her StateRead implementasyonu otomatik SlashingData yetenekleri kazanır.

### Nova Shield Chain İçin Dersler

- **İki dizin pattern**: Consensus key rotation'a karşı her iki yönde lookup yeteneği (address->key, key->identity) ve özellikle eski kayıtların tutulması byzantine evidence için zorunlu.
- **Compound penalty**: Birden fazla slashing event'i tek bir "kept rate" değerine katlamak doğru matematik. Nova Shield slashing sistemi aynı formülü kullanmalı.
- **total_active_stake**: Sadece Active state validator'ların pool'u sayılır; Inactive/Jailed'ın stake'i base rate hesabına dahil edilmez. Bu tasarım reward dilüsyonunu engeller.

---

## [component/stake/address.rs]
**Dosya:** `.../stake/src/component/stake/address.rs`
**Satır Sayısı:** 23

### Genel Amaç
CometBFT validator address'ini consensus public key'den türetme utility'si.

### Tip Tanımları
```rust
pub(crate) type Address = [u8; ADDRESS_LEN];
const ADDRESS_LEN: usize = 20;
```

### Fonksiyonlar

#### `validator_address(ck: &PublicKey) -> Address` (satır 18-23)
```rust
let ck_bytes = ck.to_bytes();
Sha256::digest(ck_bytes).as_slice()[0..20].try_into().expect(...)
```
**Algoritma**: `SHA256(consensus_key_bytes)[0..20]` — ilk 20 byte alınır.

**Not**: Bu Tendermint'in kendi format'ı. "Refined type upstream" notu var ama tendermint-rs'e taşınamadığı için burada implement edilmiş.

### Nova Shield Chain İçin Dersler
- CometBFT ile çalışıyorsanız bu format zorunlu. `SHA256(pubkey)[0:20]` truncation.
- `[u8; 20]` type alias kullanmak, 20 byte adresleri tip seviyesinde ayırt eder.

---

## [component/stake/tests.rs]
**Dosya:** `.../stake/src/component/stake/tests.rs`
**Satır Sayısı:** 105

### Genel Amaç
Consensus key persistency testini doğrular.

### Test

#### `test_persistent_identity_by_ck` (satır 16-105)
**Senaryo**: Validator consensus key rotasyonu sonrasında eski adresten identity key bulunabilmeli.

Adımlar:
1. `old_ck` ve `new_ck` oluştur.
2. `state.register_consensus_key(&id, &old_ck)`.
3. `old_address` üzerinden `old_ck` bulunabilmeli → identity eşleşmeli.
4. `state.register_consensus_key(&id, &new_ck)`.
5. `new_address` üzerinden `new_ck` bulunabilmeli → identity eşleşmeli.
6. **CRUCIAL**: `old_address` üzerinden hâlâ `old_ck` bulunabilmeli → hâlâ identity eşleşmeli.

**Doğrulanan invariant**: Consensus key silme yapılmaz; geçmiş adresler Byzantine evidence attribution için korunur.

### Nova Shield Chain İçin Dersler
- Bu test tam olarak "neden eski kayıtlar silinmez" sorusunu yanıtlar. Byzantine evidence delay'li gelir; o zaman kullanılan anahtarın hâlâ lookup edilebilir olması şart.

---

## [component/validator_handler.rs]
**Dosya:** `.../stake/src/component/validator_handler.rs`
**Satır Sayısı:** 11

### Genel Amaç
Validator handler alt modüllerini re-export eder.

### Dışa Aktarılanlar
- `ValidatorManager` (validator_manager.rs)
- `ValidatorDataRead`, `ValidatorDataWrite` (validator_store.rs)
- `ValidatorPoolTracker` (validator_store.rs)
- `ValidatorUptimeTracker` (uptime_tracker.rs)

---

## [component/validator_handler/uptime_tracker.rs]
**Dosya:** `.../stake/src/component/validator_handler/uptime_tracker.rs`
**Satır Sayısı:** 197

### Genel Amaç
Validator'ların block imzalama uptimesini takip eder. Yeterince çok blok kaçıran validator'lar `Jailed` edilir.

### Tip Tanımları
```rust
type ValidatorInformation = (IdentityKey, tendermint::PublicKey, Uptime);
type LookupResult = anyhow::Result<Option<ValidatorInformation>>;
```

### Trait Implementasyonları

**`pub trait ValidatorUptimeTracker: StateWrite`**

#### `track_uptime` (satır 43-93)
Adımlar:
1. **Vote haritası oluştur** (satır 52-71):
   ```rust
   let did_address_vote: BTreeMap<Address, bool> = last_commit_info.votes
       .iter()
       .map(|vote| (vote.validator.address, vote.sig_info.is_signed()))
       .collect();
   ```
2. **Parallel validator lookup** (satır 75-90): `JoinSet` + `spawn_validator_lookup_fut`.
3. **Her validator için uptime process** (satır 85-90): `process_validator_uptime`.

#### `spawn_validator_lookup_fut` (satır 105-151)
- Validator state, uptime, consensus_key async olarak parallel fetch.
- Sadece `Active` state validator için `Some(...)` döner; diğerleri için `None`.
- Span follows_from pattern ile tracing context bağlanır.

#### `process_validator_uptime` (satır 153-193)
Adımlar:
1. Validator'ın CometBFT address'ini hesapla: `validator_address(&consensus_key)`.
2. Vote haritasında ara: `did_address_vote.get(&addr)`.
3. **Height==1 edge case** (satır 167): Genesis block'ta signer yok; varsayılan olarak hepsi imzalamış sayılır.
4. Metrics güncelle: `gauge!(MISSED_BLOCKS).set(uptime.num_missed_blocks() as f64)`.
5. Missed block event emit et (imzalanmadıysa).
6. `uptime.mark_height_as_signed(height, voted)`.
7. **Slashing check** (satır 185-190):
   ```rust
   if uptime.num_missed_blocks() as u64 >= params.missed_blocks_maximum {
       set_validator_state(Jailed).await?;
   } else {
       set_validator_uptime(uptime);
   }
   ```

### Tasarım Kararları

- **Parallel lookup**: Validator sayısı büyüdükçe sequential lookup yerine JoinSet ile tüm lookuplar parallel yapılır. Determinizm bozulmaz çünkü her validator state değişikliği bağımsız.
- **Genesis block grace**: Height 1'de `last_commit_info` genesis block'a aittir ve signer yoktur; hatalı jail'i önlemek için varsayılan signed=true.
- **Non-determinism uyarısı**: "process validator uptime changes in a random order" (satır 83-84) — ama bağımsız oldukları için sonuç deterministik.

### Nova Shield Chain İçin Dersler

- JoinSet ile parallel validator uptime fetch, yüksek validator sayısında kritik performans optimizasyonu.
- Genesis block edge case'i her blockchain'de var: ilk blokta last_commit_info boş/yetersiz; her staking modülünde bu özel durum handle edilmeli.
- `num_missed_blocks >= missed_blocks_maximum` → jail threshold olarak sliding window kullanımı, anlık blok kaçırma yerine uzun vadeli pattern tespiti yapar.

---

## [component/validator_handler/validator_manager.rs]
**Dosya:** `.../stake/src/component/validator_handler/validator_manager.rs`
**Satır Sayısı:** 693

### Genel Amaç
Validator state machine'ini implement eden `ValidatorManager` trait'i. Penumbra'nın en kritik dosyalarından biri.

### Validator State Machine Diyagramı (satır 54-83'te belgelenmiş)
```
        Defined (başlangıç) <---> Disabled
           |
           v (yeterli stake)
        Inactive <---> Active
           |               |
           v               v
          Jailed <------  (downtime)
           |
           v (byzantine)
        Tombstoned (terminal)
```

Geçerli geçişler:
- `Defined | Disabled | Jailed -> Inactive`: Yeterli stake ile consensus set'e giriş.
- `Inactive | Jailed | Disabled -> Defined`: Min stake altına düşüş.
- `Inactive | Jailed | Defined -> Disabled`: Operatör devre dışı bırakma.
- `Inactive -> Active`: Yüksek oy gücü ile aktif set'e girme.
- `Active -> Inactive | Defined | Disabled`: Şerefli ayrılış (honorable discharge).
- `Active -> Jailed`: Downtime slashing.
- `Any -> Tombstoned`: Byzantine misbehavior.
- `Tombstoned -> Tombstoned`: No-op (idempotent).

### Trait Implementasyonları

**`pub trait ValidatorManager: StateWrite`**

#### `set_validator_state` (satır 103-119)
- Mevcut state'i okur, `set_validator_state_inner`'a delege eder.

#### `set_validator_state_inner` (satır 125-314)
Karmaşık match bloğu — her geçiş için side effect'ler:

| Geçiş | Side Effect |
|---|---|
| `* -> Inactive` | `add_consensus_set_index` |
| `Inactive -> Active` | `set_validator_bonding_state(Bonded)`, `set_validator_uptime(new)` |
| `Active -> Inactive/Defined/Disabled` | `set_validator_bonding_state(Unbonding { unbonds_at_height })` |
| `Active -> Jailed` | `slashing_penalty_downtime` kaydı, `Unbonding` başlat |
| `Any -> Tombstoned` | `slashing_penalty_misbehavior` kaydı, `Unbonded` (hemen!) |
| `Tombstoned -> Tombstoned` | No-op |
| Yasak geçişler | `bail!` |

**Önemli**: `Active -> Jailed` geçişinde `Unbonding` başlar ama hemen bitmez. `Active -> Tombstoned`'da ise pool anında `Unbonded` olur.

**Early epoch end** (satır 145-148):
```rust
if let (Active, Defined | Disabled | Jailed | Tombstoned) = (old_state, new_state) {
    self.set_end_epoch_flag();
}
```
Active validator'ın beklenmedik çıkışı epoch'u erken sonlandırır.

#### `try_precursor_transition` (satır 316-374)
`Defined | Inactive | Active` durumlarında minimum stake eşiği kontrolü:
```rust
let unbonded_pool = next_rate.unbonded_amount(delegation_token_supply);
let has_minimum_stake = unbonded_pool >= min_stake;
let new_state = match (previous_state, has_minimum_stake) {
    (Defined, true) => Inactive,
    (Defined, false) => Defined,
    (Active, true) => Active,
    (Active, false) => Defined,  // Aktif validator stake kaybederse Defined'e düşer!
    (Inactive, true) => Inactive,
    (Inactive, false) => Defined,
};
```

#### `add_genesis_validator` (satır 378-426)
- Rate: genesis base rate ile initialize.
- State: `Active`.
- Bonding: `Bonded`.
- `add_consensus_set_index` ve `set_validator_uptime` ile tamamlanır.

#### `add_validator` (satır 431-443)
- State: `Defined`.
- Bonding: `Unbonded`.
- Voting power: `Amount::zero()`.

#### `add_validator_inner` (satır 453-507)
1. `put(definitions/by_id, validator)` — tanım kaydet.
2. `register_consensus_key` — lookup dizinleri.
3. `register_denom(DelegationToken.denom())` — token registry.
4. `set_validator_rate_data` — initial rate.
5. `set_initial_validator_state` — başlangıç state.
6. `set_validator_power` — initial power.
7. `set_validator_bonding_state` — bonding state.
8. `set_validator_pool_size` — pool.
9. Metrics gauge initialize.

#### `update_validator_definition` (satır 514-616)
- `(Active | Inactive | Jailed | Defined | Disabled, false)` → `Disabled`.
- `(Disabled, true)` → Re-enable: epoch grinding guard check:
  ```rust
  allowed_enabled_height = last_disabled + epoch_duration
  ensure!(current_height >= allowed_enabled_height, ...)
  ```
  Issue #4067: Epoch grinding attack engellenir.
- `(Jailed, true)` → Unjail: minimum stake check; yeterli ise `Inactive`, değilse `Defined`.
- `(Active | Inactive, true)` → No-op (state değişmez).
- `(Tombstoned, _)` → Görmezden gel.

#### `process_validator_pool_state` (satır 619-651)
- Unbonding height'a ulaşıldıysa pool `Unbonded` state'ine geçer.

#### `process_evidence` (satır 661-689)
- CometBFT'nin `Misbehavior` evidence'ını alır.
- CometBFT address → consensus key → identity key (3 adımlı lookup).
- `set_validator_state(Tombstoned)` çağrısı.
- Event emit.

### Unsafe Bloklar
Yok.

### Tasarım Kararları

- **Early epoch end on active validator exit**: Active validator beklenmedik ayrılışı, epoch'u acil sonlandırır. Bu, consensus set'in tutarlı kalmasını sağlar.
- **Epoch grinding guard** (Issue #4067): Validator disable → enable → disable döngüsü ile epoch'ları elle ayarlamak epoch grinding saldırısıdır. `last_disabled + epoch_duration` beklenme süresi bu saldırıyı önler.
- **Tombstone idempotency**: Aynı validator için birden fazla byzantine evidence uygulandığında tekrar ceza verilmez. Çok küçük misconfigürasyon yanlışlıkla validator'ı silebilir; tek ceza yeterli deterrence.
- **3-adımlı evidence attribution**: CometBFT address → consensus key → identity key lookup zinciri, key rotation'a karşı dayanıklı.

### Nova Shield Chain İçin Dersler

- **State machine eksplisit tanımlansın**: Nova Shield validator state machine'i tam olarak bu diyagram gibi açık, yasak geçişlerle birlikte kodlanmalı.
- **Early epoch end trigger**: Consensus security için aktif validator ani çıkışında epoch'u hemen sonlandırmak kritik. Nova Shield için aynı guard gerekli.
- **Epoch grinding koruması**: Validator enable/disable'ı için cooldown süresi şart. En az 1 epoch bekleme zorunlu tutulmalı.
- **Tombstone idempotency**: Byzantine ceza tek seferlik. Kümülatif ceza değil, tek bir maksimum ceza anında uygulanır ve pool hemen `Unbonded` olur.

---

## [component/validator_handler/validator_store.rs]
**Dosya:** `.../stake/src/component/validator_handler/validator_store.rs`
**Satır Sayısı:** 474

### Genel Amaç
Validator state, power, rate, uptime, bonding state okuma/yazma trait'leri. JMT ve non-verifiable storage ile etkileşim.

### Trait Implementasyonları

#### `ValidatorDataRead` (satır 23-236)
- `get_validator_info()`: Üç ayrı get → birleşik Info nesnesi.
- `get_validator_state()`: `DomainFuture` (lazy future, boxing olmadan).
- `get_validator_bonding_state()`: Async, `Option<BondingState>`.
- `get_validator_pool_size()`: Delegation token arzı.
- `get_validator_status()`: state + power + bonding_state birleşik.
- `get_validator_rate()`: `Pin<Box<dyn Future<...>>>` — boxed future (non-object-safe dönüş tipi nedeniyle).
- `get_last_disabled_height()`: Nonverifiable storage, 8 byte big-endian u64.
- `lookup_identity_key_by_consensus_key()`: consensus_key → identity_key.
- `lookup_consensus_key_by_comet_address()`: [u8;20] → PublicKey.

#### `compute_unbonding_height` (satır 179-220) — KRİTİK
```rust
let min_block_delay = stake_params.unbonding_delay;
let upper_bound_height = start_height.saturating_add(min_block_delay);

match val_bonding_state {
    Bonded => Some(upper_bound_height),
    Unbonding { unbonds_at_height } => {
        if unbonds_at_height > start_height {
            Some(unbonds_at_height.min(upper_bound_height))
        } else {
            None  // zaten unbonded
        }
    }
    Unbonded => None,
}
```
**Detay**: `Unbonding` durumunda hem validator'ın belirlediği yükseklik hem de parametreden gelen `start + delay` hesaplanır; ikisinin minimumu alınır. Parametre değişimi (delay azalırsa) lehine çalışır.

#### `ValidatorPoolDeposit` (satır 239-297)
- `deposit_to_validator_pool()`: Checked add + rate ile bonded value hesabı. Hata durumunda None döner (pool overflow, rate yok).

#### `ValidatorDataWrite` (satır 300-411)
- `set_validator_uptime()`: Nonverifiable storage, raw protobuf bytes.
- `set_validator_bonding_state()`: JMT'ye yazar + event emit.
- `set_validator_power()`: MAX_VOTING_POWER guard + JMT + event.
- `set_initial_validator_state()`: Sadece `Active | Defined` geçerli başlangıç.
- `set_validator_rate_data()`: JMT + event.
- `set_prev_validator_rate()`: JMT (previous path).
- `set_last_disabled_height()`: 8 byte BE u64, nonverifiable.

#### `ValidatorPoolTracker` (satır 416-474)
- `set_validator_pool_size()`: Direkt overwrite.
- `increase_validator_pool_size()`: Checked add, `Option<Amount>` döner.
- `decrease_validator_pool_size()`: Checked sub, `Option<Amount>` döner.

### Tasarım Kararları

- **JMT vs nonverifiable storage**: Validator uptime ve last_disabled_height nonverifiable'da; bunlar inclusion proof gerektirmiyor, sadece liveness için kullanılıyor. Validator state, rate, power JMT'de; bunlar Merkle proof gerektiriyor.
- **Checked arithmetic**: `increase_validator_pool_size` ve `decrease_validator_pool_size` checked arithmetic ile None döner; caller panic yerine None'u handle edebilir.
- **`DomainFuture` vs boxed future**: `get_validator_rate` için boxed future kullanılmak zorunda; aksi halde `async fn` trait'teki object-safety bozulur.

### Nova Shield Chain İçin Dersler

- JMT (Merkle proof capable) vs nonverifiable storage ayrımı bilinçli yapılmalı: proof gerektiren her şey JMT'ye, ephemeral/operational data nonverifiable'a.
- `increase/decrease_validator_pool_size` → checked arithmetic ile `Option` dönmesi doğru. `expect()` yerine `Option` hatası caller'a ilet.
- `compute_unbonding_height`'taki `min(unbonds_at_height, upper_bound)` logic, parametre değişimine karşı delegation sahibini korur.

---

## [current_consensus_keys.rs]
**Dosya:** `.../stake/src/current_consensus_keys.rs`
**Satır Sayısı:** 40

### Genel Amaç
CometBFT'nin şu an bildiği consensus key listesini tutar. Duplicate silme işlemlerini önlemek için kullanılır.

### Struct'lar

- **`CurrentConsensusKeys`** (satır 13-15):
  ```rust
  pub struct CurrentConsensusKeys {
      pub consensus_keys: Vec<PublicKey>,
  }
  ```
  `#[derive(Default)]` — başlangıçta boş liste.

### Kullanım Senaryosu
`build_cometbft_validator_updates` içinde mevcut lista okunur; sıfır güçlü ama daha önce bildirilmiş validator'lar hâlâ update'e dahil edilir (silinmek üzere).

---

## [delegate.rs]
**Dosya:** `.../stake/src/delegate.rs`
**Satır Sayısı:** 97

### Genel Amaç
Delegasyon transaction action'ını temsil eden `Delegate` struct'ı.

### Struct'lar

- **`Delegate`** (satır 12-27):
  - `validator_identity: IdentityKey`
  - `epoch_index: u64`
  - `unbonded_amount: Amount` — input: ne kadar staking token veriliyor
  - `delegation_amount: Amount` — output: ne kadar delegation token alınıyor

### Fonksiyonlar

#### `balance()` (satır 38-43)
```rust
pub fn balance(&self) -> Balance {
    delegation - stake  // delegation token üretilir, staking token tüketilir
}
```

#### `delegation_value()` / `unbonded_value()` (satır 46-60)
- `DelegationToken::new(validator_identity).id()` ile delegation token asset ID'si.
- `STAKING_TOKEN_ASSET_ID` ile staking token asset ID'si.

### Trait Implementasyonları
- `EffectingData`: `EffectHash::from_proto_effecting_data(&self.to_proto())` — tüm action etki hash'i hesabına dahil.
- `DomainType for Delegate`: Proto tipi `pb::Delegate`.

### Nova Shield Chain İçin Dersler

- Balance = üretilen - tüketilen modeli temiz ve composable. Her action için bu şekilde balance contribution hesaplanabilir.
- `EffectingData` trait implement etmek, action'ın transaction hash'ine dahil edilmesini sağlar.

---

## [delegation_token.rs]
**Dosya:** `.../stake/src/delegation_token.rs`
**Satır Sayısı:** 146

### Genel Amaç
Her validator için ayrı delegation token tip yönetimi. Denom formatı: `udelegation_{validator_identity}`.

### Struct'lar

- **`DelegationToken`** (satır 10-13):
  ```rust
  pub struct DelegationToken {
      validator_identity: IdentityKey,
      base_denom: asset::Metadata,
  }
  ```

### Fonksiyonlar

#### `DelegationToken::new(validator_identity: IdentityKey)` (satır 29-38)
```rust
let base_denom = asset::REGISTRY.parse_denom(
    &format!("udelegation_{validator_identity}")
).expect("base denom format is valid");
```

#### `TryFrom<asset::Metadata>` (satır 61-86)
Regex: `^udelegation_(?P<data>penumbravalid1[a-zA-HJ-NP-Z0-9]+)$`
- Bech32m formatındaki validator identity key parse edilir.
- `penumbravalid1` prefix zorunlu.

### Testler

#### `delegation_token_denomination_round_trip` (satır 131-145)
- `DelegationToken::new(ik)` → `to_string()` → `from_str()` → `to_string()` — round trip test.

### Nova Shield Chain İçin Dersler

- Delegation token denom'unu `udelegation_{validator_id}` gibi deterministic bir formattan üret. Bu asset registry'de eşsiz tanımlama sağlar.
- Regex tabanlı parse + round-trip test kombinasyonu, denom formatının her zaman tutarlı olduğunu garantiler.
- "u" prefix: micro-denomination (= 10^-6). Nova Shield'in staking tokenları için benzer denomination yapısı kullanılabilir.

---

## [event.rs]
**Dosya:** `.../stake/src/event.rs`
**Satır Sayısı:** 449

### Genel Amaç
Staking modülünün tüm event tiplerini tanımlar. Her event proto-serializasyon ile zincire emit edilir.

### Event Tipleri

| Event Struct | İçerik |
|---|---|
| `EventValidatorStateChange` | `identity_key`, `state` |
| `EventValidatorVotingPowerChange` | `identity_key`, `voting_power` |
| `EventValidatorBondingStateChange` | `identity_key`, `bonding_state` |
| `EventRateDataChange` | `identity_key`, `rate_data` |
| `EventValidatorDefinitionUpload` | `validator` |
| `EventValidatorMissedBlock` | `identity_key` |
| `EventDelegate` | `identity_key`, `amount` |
| `EventUndelegate` | `identity_key`, `amount` |
| `EventTombstoneValidator` | `evidence_height`, `current_height`, `identity_key`, `address`, `voting_power` |
| `EventSlashingPenaltyApplied` | `identity_key`, `epoch_index`, `new_penalty` |

### Tasarım Kararları

- Her event ayrı struct — type-safe event handling.
- `From<&Delegate>`, `From<&Undelegate>` gibi conversion'lar action → event kolay dönüşüm sağlar.
- `EventTombstoneValidator::from_evidence(height, ik, misbehavior)`: Tek akıllı constructor; raw evidence field'larını event'e çevirir.

### Nova Shield Chain İçin Dersler

- Her state değişikliği için ayrı event struct kullanın. Indexing, monitoring ve client notification için kritik.
- Event'leri proto'ya serialize ederek zincire kaydetmek, dışarıdan sorgulama için standart yöntem.
- `EventSlashingPenaltyApplied` içinde `new_penalty` kaydedilir: hem önceki hem sonraki değer event'te olsaydı daha iyi olurdu, ama mevcut tasarımda sadece yeni değer var.

---

## [funding_stream.rs]
**Dosya:** `.../stake/src/funding_stream.rs`
**Satır Sayısı:** 243

### Genel Amaç
Validator commission paylaşım mekanizması. Staking ödülleri belirli adreslere veya community pool'a yönlendirilebilir.

### Enum'lar

- **`FundingStream`** (satır 11-25):
  - `ToAddress { address: Address, rate_bps: u16 }`
  - `ToCommunityPool { rate_bps: u16 }`

- **`Recipient`** (satır 29-32):
  - `Address(Address)`
  - `CommunityPool`

### Struct'lar

- **`FundingStreams`** (satır 177-180): `Vec<FundingStream>` wrapper, max 8 stream, toplam <= 10000 bps.

### Fonksiyonlar

#### `FundingStream::reward_amount(base_reward_rate, validator_exchange_rate, total_delegation_tokens) -> Amount` (satır 54-105)

**Matematiksel Formül:**
```
R_{v,e}(stream) = y_v * c_{stream,e} * r_e * psi_v(e)

Adımlar:
1. commission_rate = rate_bps / 10_000
2. prev_validator_exchange_rate = validator_exchange_rate / BPS_SQUARED_SCALING_FACTOR
3. prev_base_reward_rate = base_reward_rate / BPS_SQUARED_SCALING_FACTOR
4. staking_tokens = total_delegation_tokens * prev_validator_exchange_rate
5. total_reward_amount = staking_tokens * prev_base_reward_rate
6. stream_reward_amount = total_reward_amount * commission_rate
7. result = floor(stream_reward_amount)
```

**Notasyon:**
- `y_v` = toplam delegation token arzı
- `c_{stream,e}` = bu funding stream'in komisyon oranı (bps / 10000)
- `r_e` = base reward rate (epoch e için)
- `psi_v(e)` = validator exchange rate (epoch e'de)

### Tasarım Kararları

- `FundingStreams::try_from(Vec<FundingStream>)`: Sadece bu constructor ile oluşturulabilir; invariant'lar (max 8, toplam <= 100%) enforce edilir.
- `AsRef<[FundingStream]>` impl: `as_ref()` ile slice erişimi, owned->borrowed dönüşüm.
- `#[allow(clippy::large_enum_variant)]`: `Address` variant büyük; boxing yerine allow tercih edilmiş.

### Nova Shield Chain İçin Dersler

- Funding stream modeli validator commission'ı esnek ayarlar: hem adres hem de community pool'a yönlendirme.
- Max 8 stream sınırı, state space'i kontrol altında tutar.
- `reward_amount` formülü tam olarak uygulanmalı: scaling factor kaldırılırken dikkat, yuvarlama her zaman floor.

---

## [genesis.rs]
**Dosya:** `.../stake/src/genesis.rs`
**Satır Sayısı:** 45

### Genel Amaç
Genesis state için `Content` struct'ı.

### Struct'lar

- **`Content`** (satır 9-14):
  - `stake_params: StakeParameters`
  - `validators: Vec<pb::Validator>` — Not: domain type değil proto type! Genesis'te proto doğrudan kullanılır.

### Tasarım Kararları

- Genesis validators'ın proto formatında tutulması, genesis dosyasının doğrudan protobuf'tan okunmasını kolaylaştırır.

---

## [governance_key.rs]
**Dosya:** `.../stake/src/governance_key.rs`
**Satır Sayısı:** 67

### Genel Amaç
Validator governance key — yönetişim oylamasında kullanılan ayrı anahtar.

### Struct'lar

- **`GovernanceKey(VerificationKey<SpendAuth>)`** (satır 22): Newtype wrapper.
  - `SpendAuth` VerificationKey kullanılır — cüzdan yönetimi ile aynı key tipi.
  - Bech32m formatında encode: `penumbravalcons` (governance) prefix.
  - Cold storage'daki fund custody key ile aynı cryptographic altyapı.

### Nova Shield Chain İçin Dersler

- Governance key = identity key'den ayrı. Bu, operator validator'ı cold storage ile korurken governance'a farklı bir hot key ile katılabilir.
- Nova Shield governance mekanizması için bu ayrımı düşünün.

---

## [identity_key.rs]
**Dosya:** `.../stake/src/identity_key.rs`
**Satır Sayısı:** 98

### Genel Amaç
Validator'ın kalıcı kimliği.

### Struct'lar

- **`IdentityKey(VerificationKeyBytes<SpendAuth>)`** (satır 27): 32 byte SpendAuth verification key.
  - Bech32m encode: `penumbravalid1` prefix.
  - State-breaking implementation: `Display` ve `FromStr` implementasyonları değiştirilemez (comment ile belgelenmiş).

### Sabitler
- `IDENTITY_KEY_LEN_BYTES: usize = 32`

### Trait Implementasyonları
- `From<IdentityKey> for CurrentValidatorRateRequest`: Doğrudan gRPC request'e çevrilebilir.

### Tasarım Kararları

- **"IMPORTANT: Changing this implementation is state-breaking"** notu: State key'lerin bağımlılığı. String format değişirse tüm state geçersiz olur.

### Nova Shield Chain İçin Dersler

- State key formatlarını kesinlikle belgeleyin ve "state-breaking" olarak işaretleyin. Nova Shield'in key format değişikliği migration gerektirir.
- 32 byte ED25519/SpendAuth key standarttır; yeterli güvenlik sağlar.

---

## [lib.rs]
**Dosya:** `.../stake/src/lib.rs`
**Satır Sayısı:** 52

### Genel Amaç
Crate root — modül ve re-export yönetimi.

### Önemli Sabit
```rust
pub static BPS_SQUARED_SCALING_FACTOR: once_cell::sync::Lazy<U128x128> =
    once_cell::sync::Lazy::new(|| 1_0000_0000u128.into());
```
- Tüm rate hesaplamalarında kullanılan global scaling factor: 10^8.
- `Lazy` initialization: İlk erişimde oluşturulur.

### Özellikler (Attributes)
- `#![deny(clippy::unwrap_used)]` — `.unwrap()` kullanımı derleme hatası verir.
- `#![allow(clippy::clone_on_copy)]` — Copy tipler üzerinde `.clone()` call'larına izin.

### Nova Shield Chain İçin Dersler

- `#![deny(clippy::unwrap_used)]` zorunlu olarak uygulayın. Panic'leri Result/Option üzerinden handle etmek güvenilirliği artırır.
- Global `BPS_SQUARED_SCALING_FACTOR` gibi sabitleri `Lazy<U128x128>` ile thread-safe yönetin.

---

## [params.rs]
**Dosya:** `.../stake/src/params.rs`
**Satır Sayısı:** 83

### Genel Amaç
Staking parametrelerini tutan `StakeParameters` struct'ı.

### Struct'lar

**`StakeParameters`** (satır 8-23):

| Alan | Tip | Açıklama |
|---|---|---|
| `unbonding_delay` | `u64` | Unbonding için beklenmesi gereken blok sayısı |
| `active_validator_limit` | `u64` | Maksimum aktif validator sayısı |
| `slashing_penalty_misbehavior` | `u64` | Byzantine ceza (bps^2) |
| `slashing_penalty_downtime` | `u64` | Downtime ceza (bps^2) |
| `signed_blocks_window_len` | `u64` | Uptime penceresi (blok sayısı) |
| `missed_blocks_maximum` | `u64` | Bu penceredeki maksimum kaçırılabilir blok |
| `min_validator_stake` | `Amount` | Min delegation pool büyüklüğü |

### Varsayılan Değerler (`Default`)

```rust
unbonding_delay: 120960,          // ~1 hafta (7 gün * 24 saat * 60 dk * 12 blok/dk)
active_validator_limit: 80,       // Cosmos Hub benzeri
signed_blocks_window_len: 10000,  // Cosmos Hub'dan kopyalanmış
missed_blocks_maximum: 9500,      // 9500/10000 = %95 missed = jailed
slashing_penalty_misbehavior: 1000_0000,  // 1000 bps^2 = 10%
slashing_penalty_downtime: 1_0000,        // 1 bps^2 = 0.01%
min_validator_stake: 1_000_000u128.into(),  // 1 penumbra (= 10^6 micro-penumbra)
```

### Tasarım Kararları

- **bps^2 ceza**: Slashing penalty'ler bps^2 formatında. `1_0000_0000 bps^2 = 100%`.
- **9500/10000 downtime threshold**: %5'ten az uptime variance → jail. Bu Cosmos Hub parametrelerinden alınmış.
- Deprecated `unbonding_epochs` ve `base_reward_rate` alanları protobuf'ta var ama Rust struct'ında yok (`From` impl'de `0` yazılır).

### Nova Shield Chain İçin Dersler

- `unbonding_delay` blok bazlı (epoch bazlı değil). Bu, kesin zaman hesabı için consensus finality time tahminini gerektirir.
- Default parametreleri Cosmos Hub'dan almak iyi başlangıç noktası, ama Nova Shield'in kendi threat modelini analiz edin.
- `min_validator_stake = 1 penumbra` düşük eşik; Sybil saldırısına karşı ek mekanizmalar gerektirebilir.

---

## [penalty.rs]
**Dosya:** `.../stake/src/penalty.rs`
**Satır Sayısı:** 136

### Genel Amaç
Slashing penalty'leri temsil eden `Penalty` tipi.

### Struct'lar

- **`Penalty(U128x128)`** (satır 18): U128x128 sabit nokta, 0-1 arasında "kept rate" temsil eder.
  - **Temsil**: `1.0 = %0 penalty`, `0.9 = %10 penalty`, `0.0 = %100 penalty`.
  - `PartialOrd, Ord`: Büyük kept_rate → küçük penalty.

### Fonksiyonlar

#### `from_percent(percent: u64)` (satır 24-26)
```rust
Penalty::from_bps(percent.saturating_mul(100))
```

#### `from_bps(bps: u64)` (satır 31-33)
```rust
Penalty::from_bps_squared(bps.saturating_mul(10000))
```

#### `from_bps_squared(bps_squared: u64)` (satır 36-43)
```rust
assert!(bps_squared <= 1_0000_0000);
Self(U128x128::ratio(bps_squared, 1_0000_0000)).one_minus_this()
```
- `ratio(bps_squared, 1e8)` → penalty fraction (0-1).
- `one_minus_this()` → kept rate (1 - penalty).

#### `compound(&self, other: Penalty) -> Penalty` (satır 60-62)
```rust
Self((self.0 * other.0).expect("compounding penalties will not overflow"))
```
**Formül**: `compound(p1, p2) = kept_rate_1 * kept_rate_2`
**Örnek**: 10% + 20% = `0.9 * 0.8 = 0.72` → %28 toplam kayıp.

#### `apply_to_amount(amount: Amount) -> Amount` (satır 65-69)
```rust
self.0.apply_to_amount(&amount).expect("should not overflow, because penalty is <= 1")
```

#### `balance_for_claim(unbonding_id, unbonding_amount) -> Balance` (satır 83-95)
```rust
Balance::zero()
    - Value { amount: unbonding_amount, asset_id: unbonding_id }  // unbonding token yak
    + Value { amount: self.apply_to_amount(unbonding_amount), asset_id: STAKING_TOKEN_ASSET_ID }  // staking token üret
```
Penalty uygulanmış miktarı staking token'a çevirir.

### Trait Implementasyonları

- `ToConstraintField<Fq>`: ZK circuit'a giriş — penalty ZK kanıtı içinde public input.
- `From<Penalty> for [u8; 32]`: U128x128 → 32 byte serialization.
- `TryFrom<&[u8]>`: 32 byte → U128x128 → Penalty.

### Nova Shield Chain İçin Dersler

- **"Kept rate" temsili**: Penalty'yi `1 - penalty_fraction` olarak saklamak, compound işlemini basit çarpma haline getirir.
- **Compound formülü doğru**: Bağımsız penalty olayları `kept_rate` çarpımıyla birleşir. Additive formülden kaçının.
- **ZK circuit entegrasyonu**: `ToConstraintField` implement etmek, penalty'yi ZK kanıt devresine sokmaya olanak tanır. Nova Shield'in ZK tabanlı işlemleri için bu pattern gerekli.

---

## [rate.rs]
**Dosya:** `.../stake/src/rate.rs`
**Satır Sayısı:** 416

### Genel Amaç
Exchange rate ve reward rate hesaplamaları. Penumbra staking matematiğinin kalbi.

### Struct'lar

#### `RateData` (satır 16-23)
```rust
pub struct RateData {
    pub identity_key: IdentityKey,
    pub validator_reward_rate: Amount,  // BPS^2 scaled
    pub validator_exchange_rate: Amount,  // BPS^2 scaled, başlangıç 1_0000_0000
}
```

#### `BaseRateData` (satır 257-266)
```rust
pub struct BaseRateData {
    pub epoch_index: u64,
    pub base_reward_rate: Amount,     // BPS^2 scaled
    pub base_exchange_rate: Amount,   // BPS^2 scaled, başlangıç 1_0000_0000
}
```

### Fonksiyonlar

#### `RateData::next_epoch(next_base_rate, funding_streams, validator_state) -> RateData` (satır 32-127)

Sadece `State::Active` validator için hesaplanır:

**Adımlar:**
1. Toplam commission bps hesapla.
2. Scaling factor kaldır:
   ```rust
   validator_commission = commission_bps / 10_000
   next_base_reward_rate_fp = next_base_reward_rate / BPS_SQUARED_SCALING_FACTOR
   previous_validator_exchange_rate_fp = previous_exchange_rate / BPS_SQUARED_SCALING_FACTOR
   ```
3. Validator reward rate:
   ```rust
   commission_factor = 1 - validator_commission
   next_validator_reward_rate_fp = next_base_reward_rate_fp * commission_factor
   ```
4. Validator exchange rate:
   ```rust
   reward_growth_factor = 1 + next_validator_reward_rate_fp
   next_validator_exchange_rate_fp = previous_validator_exchange_rate_fp * reward_growth_factor
   ```
5. Rescale ve floor:
   ```rust
   next_validator_reward_rate = floor(next_validator_reward_rate_fp * BPS_SQUARED_SCALING_FACTOR)
   next_validator_exchange_rate = floor(next_validator_exchange_rate_fp * BPS_SQUARED_SCALING_FACTOR)
   ```

**Non-Active validator**: Rate sabit kalır (freeze).

**Formüller özet:**
```
VRR[e] = base_reward_rate[e] * (1 - commission_rate)
VXR[e] = VXR[e-1] * (1 + VRR[e])
```

#### `RateData::delegation_amount(unbonded_amount) -> Amount` (satır 142-167)
```rust
delegation_amount = floor(unbonded_amount / VXR)
```
- `VXR == 0` edge case: delegation_amount = 0 döner.

#### `RateData::slash(penalty: Penalty) -> Self` (satır 169-179)
```rust
penalized_exchange_rate = floor(penalty.apply_to(self.validator_exchange_rate))
```

#### `RateData::unbonded_amount(delegation_amount) -> Amount` (satır 195-210)
```rust
unbonded_amount = floor(delegation_amount * VXR)
```

#### `RateData::voting_power(delegation_pool_size) -> Amount` (satır 212-231)
```rust
voting_power = floor(delegation_pool_size * VXR)
```
Delegation token → staking token equivalent.

#### `RateData::build_delegate(epoch, unbonded_amount) -> Delegate` (satır 233-243)
Helper: Rate ile delegation action oluştur.

#### `RateData::build_undelegate(start_epoch, delegation_amount) -> Undelegate` (satır 246-254)
Helper: Rate ile undelegation action oluştur.

#### `BaseRateData::next_epoch(next_base_reward_rate: Amount) -> BaseRateData` (satır 270-305)
```rust
reward_growth_factor = 1 + next_base_reward_rate_fp
next_base_exchange_rate_fp = prev_base_exchange_rate_fp * reward_growth_factor
```

### Testler

#### `slash_rate_by_penalty` (satır 401-415)
```rust
// exchange_rate = 2_0000_0000 (= 2.0)
// penalty = 10%
// expected slashed: 2.0 * 0.9 = 1.8 → 1_8000_0000
assert_eq!(slashed.validator_exchange_rate, 1_8000_0000u128.into());
```

### Tasarım Kararları

- **BPS^2 scaling (10^8)**: 1 bps = 10^-4, 1 bps^2 = 10^-8. Exchange rate başlangıcı 1.0 = 1_0000_0000. Bu oran zaman içinde büyür (reward kazanıldıkça).
- **Floor rounding**: Her hesaplamada floor. Bu hem delegation hem unbonding'de hafif kayıp olmasına neden olur ama system toplam tutarlılığını korur.
- **Non-Active için freeze**: Jailed/Disabled validator rate değişmez — ne ceza ne ödül. Ceza epoch sonunda `slash()` ile uygulanır.

### Nova Shield Chain İçin Dersler

- **Exchange rate modeli**: `VXR[e] = VXR[e-1] * (1 + reward_rate)` basit ve doğru. Cosmos shares modeline benzer.
- **BPS^2 scaling**: 10^8 integer ile floating point hassasiyeti yakalanır. Nova Shield için aynı ya da daha büyük (10^18) bir scaling factor düşünülebilir.
- **Rounding yönü tutarlılığı**: delegation: floor(unbonded/rate), unbonded: floor(delegation*rate). Her iki yönde de floor kullanılır. Bu staking token'ların hafif "kaymasına" yol açar ama overflow'dan korur.
- **Non-active freeze**: Jail/disable durumundaki validator ödül kazanmaz. Nova Shield için zorunlu invariant.
- **Slash formülü**: `exchange_rate *= (1 - penalty_fraction)`. Penalty history epoch boyunca birikmez — sadece epoch sonunda tek bir penalty uygulanır (compounded).

---

## [state_key.rs]
**Dosya:** `.../stake/src/state_key.rs`
**Satır Sayısı:** 202

### Genel Amaç
JMT ve nonverifiable storage key string'lerini üreten fonksiyon kütüphanesi.

### Key Yapısı

```
staking/parameters
staking/validators/consensus_set_index/{identity_key}
staking/validators/lookup_by/consensus_key/{hex(pub_key)}
staking/validators/lookup_by/cometbft_address/{hex(address)}
staking/validators/definitions/{identity_key}
staking/validators/data/state/{identity_key}
staking/validators/data/rate/current/{identity_key}
staking/validators/data/rate/previous/{identity_key}
staking/validators/data/power/{identity_key}
staking/validators/data/pool/balance/{identity_key}
staking/validators/data/pool/bonding_state/{identity_key}
staking/validators/data/uptime/{identity_key}
staking/validators/data/last_disabled/{identity_key}
staking/validators/staking_rewards
staking/chain/base_rate/current
staking/chain/base_rate/previous
staking/chain/total_bonded
staking/delegation_changes/{height}
staking/penalty/{identity_key}/{epoch_index:010}  // 10 haneli sıfır doldurma
staking/cometbft_data/consensus_keys
```

### Kritik Detay

**Penalty key padding** (satır 137):
```rust
format!("{}{epoch_index:010}", prefix(id))
```
- `{epoch_index:010}`: 10 haneli sıfır doldurma.
- **Neden**: Lexicographic sıralama == numerik sıralama için gerekli. `BTreeMap::range()` string sıralaması kullanır.

### Testler

#### `penalty_in_epoch_padding` (satır 166-175)
```rust
assert_eq!(
    penalty::for_id_in_epoch(&ik, 791),
    format!("staking/penalty/{ik}/0000000791")
);
```

#### `penalty_in_epoch_sorting` (satır 177-201)
BTreeSet içinde 7, 79, 791, 792, 793 → range(k791..=k793) sadece 791, 792, 793 döndürmeli. Padding olmadan 7 < 791 olmaz lexicographic sıralamada.

### Nova Shield Chain İçin Dersler

- **Key padding**: Epoch index veya blok yüksekliği içeren key'lerde sıfır doldurma zorunludur; aksi halde lexicographic range query bozulur.
- **Hiyerarşik key yapısı**: `module/entity_type/sub_type/{id}` yapısı prefix scan'ı kolaylaştırır.
- State key'leri merkezi bir modülde toplayın ve test edin. Yanlış format → yanlış lookup.

---

## [unbonding_token.rs]
**Dosya:** `.../stake/src/unbonding_token.rs`
**Satır Sayısı:** 161

### Genel Amaç
Unbonding token temsili. Staking token'a dönüştürülmeden önce unbonding süresini geçirecek token.

### Struct'lar

- **`UnbondingToken`** (satır 14-18):
  - `validator_identity: IdentityKey`
  - `unbonding_start_height: u64`
  - `base_denom: asset::Metadata`

### Denom Format

```
uunbonding_start_at_{unbonding_start_height}_{validator_identity}
```

Örnek: `uunbonding_start_at_782_penumbravalid1...`

### Regex (satır 71)
```
^uunbonding_(?P<data>start_at_(?P<start>[0-9]+)_(?P<validator>penumbravalid1[a-zA-HJ-NP-Z0-9]+))$
```

### Testler

#### `unbonding_token_denomination_round_trip` (satır 144-160)
- `new(ik, 782)` → `to_string()` → `from_str()` → `to_string()` eşitliği.

### Nova Shield Chain İçin Dersler

- Unbonding token'ın denom'una başlangıç yüksekliği encoding'i, aynı validator için farklı unbonding dönemlerini ayırt eder.
- Token ID = hash(denom) pattern: denom'u değiştirmek yeni bir asset tipi yaratır.

---

## [undelegate.rs]
**Dosya:** `.../stake/src/undelegate.rs`
**Satır Sayısı:** 116

### Genel Amaç
Undelegasyon transaction action'ı.

### Struct'lar

- **`Undelegate`** (satır 13-27):
  - `validator_identity: IdentityKey`
  - `from_epoch: Epoch`
  - `unbonded_amount: Amount` — alınacak unbonding token miktarı
  - `delegation_amount: Amount` — verilecek delegation token miktarı

### Fonksiyonlar

#### `balance()` (satır 38-43)
```rust
undelegation - delegation
// unbonding token üretilir, delegation token tüketilir
```

#### `unbonding_token()` (satır 46-53)
```rust
UnbondingToken::new(validator_identity, self.from_epoch.start_height)
```
Unbonding token `from_epoch`'un başlangıç yüksekliğini kullanır.

### Nova Shield Chain İçin Dersler

- Undelegation anında staking token vermez; unbonding token verir. Bu token'ın claim edilebilmesi için unbonding süresi beklenmeli ve penalty hesaplanmalı.

---

## [undelegate_claim/action.rs]
**Dosya:** `.../stake/src/undelegate_claim/action.rs`
**Satır Sayısı:** 103

### Genel Amaç
Unbonding token claim action'ı — ZK kanıtı ile birlikte.

### Struct'lar

#### `UndelegateClaimBody` (satır 10-19)
- `validator_identity: IdentityKey`
- `penalty: Penalty` — uygulanan toplam penalty
- `balance_commitment: balance::Commitment` — ZK commitment
- `unbonding_start_height: u64`

#### `UndelegateClaim` (satır 22-26)
- `body: UndelegateClaimBody`
- `proof: UndelegateClaimProof`

### Trait Implementasyonları
- `EffectingData for UndelegateClaimBody`: Effect hash sadece body'den hesaplanır (proof değil).
- `EffectingData for UndelegateClaim`: Body'ye delegate eder.

---

## [undelegate_claim/mod.rs]
**Dosya:** `.../stake/src/undelegate_claim/mod.rs`
**Satır Sayısı:** 7

Alt modülleri re-export eder.

---

## [undelegate_claim/plan.rs]
**Dosya:** `.../stake/src/undelegate_claim/plan.rs`
**Satır Sayısı:** 151

### Genel Amaç
İstemci tarafında ZK kanıtı üretmek için kullanılan planlama veri yapısı.

### Struct'lar

**`UndelegateClaimPlan`** (satır 18-34):
- `validator_identity: IdentityKey`
- `penalty: Penalty`
- `unbonding_amount: Amount`
- `balance_blinding: Fr` — balance commitment blind faktörü
- `proof_blinding_r: Fq` — ZK kanıt blind faktörü 1
- `proof_blinding_s: Fq` — ZK kanıt blind faktörü 2
- `unbonding_start_height: u64`

### Fonksiyonlar

#### `undelegate_claim_proof()` (satır 55-72)
```rust
UndelegateClaimProof::prove(
    self.proof_blinding_r,
    self.proof_blinding_s,
    &CONVERT_PROOF_PROVING_KEY,
    UndelegateClaimProofPublic { balance_commitment, unbonding_id, penalty },
    UndelegateClaimProofPrivate { unbonding_amount, balance_blinding }
)
```

#### `balance()` (satır 86-89)
```rust
self.penalty.balance_for_claim(self.unbonding_id(), self.unbonding_amount)
```

### Tasarım Kararları

- `balance_blinding` ve `proof_blinding_r/s` ayrı parametreler: balance commitment ve ZK kanıt için farklı randomness kullanılır.
- Plan yapısı client-side'da saklanır; chain'e gönderilmez.

### Nova Shield Chain İçin Dersler

- Client-side "Plan" ayrımı temiz: kullanıcı önce plan oluşturur (randomness ile), sonra action üretir. Bu wallet implementasyonu için doğal akış.
- `blinding_r + blinding_s`: Groth16 proof için iki ayrı randomness parametresi gerekli.

---

## [undelegate_claim/proof.rs]
**Dosya:** `.../stake/src/undelegate_claim/proof.rs`
**Satır Sayısı:** 156

### Genel Amaç
Groth16 tabanlı ZK kanıt wrapper'ı.

### Struct'lar

#### `UndelegateClaimProofPublic` (satır 17-21)
- `balance_commitment: balance::Commitment`
- `unbonding_id: asset::Id`
- `penalty: Penalty`

#### `UndelegateClaimProofPrivate` (satır 35-38)
- `unbonding_amount: Amount`
- `balance_blinding: Fr`

#### `UndelegateClaimProof(ConvertProof)` (satır 51)
Wrapper: `ConvertProof` (shielded_pool crate'inden).

`From<UndelegateClaimProofPublic> for ConvertProofPublic` (satır 23-31):
```rust
ConvertProofPublic {
    from: value.unbonding_id,
    to: *STAKING_TOKEN_ASSET_ID,
    rate: value.penalty.kept_rate(),
    balance_commitment: value.balance_commitment,
}
```

Kanıt = "unbonding_id token'ını penalty.kept_rate() oranında staking token'a dönüştürdüm" ifadesi.

### Fonksiyonlar

#### `prove(blinding_r, blinding_s, pk, public, private) -> Result<Self>`
`ConvertProof::prove()` çağrısı — Groth16 prover.

#### `verify(vk, public) -> Result<()>`
`ConvertProof::verify()` çağrısı — Groth16 verifier.

### Testler

#### `undelegate_claim_proof_happy_path` (proptest, satır 119-155)
- `ProptestConfig::with_cases(2)` — sadece 2 case, ZK kanıt yavaş.
- Random validator, random amount, random penalty (0-100%).
- Prove + Verify round-trip test.
- `generate_prepared_test_parameters` ile test proving/verification key'leri üretilir.

### Tasarım Kararları

- **ConvertProof reuse**: UndelegateClaimProof, shielded_pool'daki genel `ConvertProof`'u wrap eder. Proof circuit paylaşımı kod tekrarını azaltır.
- **Public inputs**: balance_commitment, unbonding_id, penalty.kept_rate() — bunlar chain'in doğrulayabildiği değerler.
- **Private inputs**: unbonding_amount, balance_blinding — sadece prover bilir.

### Nova Shield Chain İçin Dersler

- ZK kanıt wrapper pattern: domain-specific public/private struct'ları genel bir kanıt sistemine map et.
- `kept_rate()` = `1 - penalty` → ZK devre içinde "ne kadar staking token aldın?" sorusunun yanıtı.
- Proptest ile ZK kanıt testi: sadece 2 case çünkü Groth16 prove yavaş. Happy path + edge case yeterli.

---

## [uptime.rs]
**Dosya:** `.../stake/src/uptime.rs`
**Satır Sayısı:** 244

### Genel Amaç
Validator block imzalama uptimesini halka tampon (ring buffer) ile takip eder.

### Struct'lar

- **`Uptime`** (satır 20-29):
  ```rust
  pub struct Uptime {
      as_of_block_height: u64,
      signatures: BitVec<u8, Lsb0>,  // halka tampon olarak kullanılır
  }
  ```

### Fonksiyonlar

#### `Uptime::new(initial_block_height: u64, signed_blocks_window_len: usize)` (satır 43-50)
```rust
let signatures = bitvec![u8, Lsb0; 1; signed_blocks_window_len];
// Başlangıçta tüm bitler 1 (imzalanmış)
```
Grace period: İlk `window_len` blok için imzalanmış varsayılır.

#### `mark_height_as_signed(height: u64, signed: bool)` (satır 59-74)
```rust
if height != self.as_of_block_height + 1 { bail!(...) }
let index = (height as usize) % self.signatures.len();
self.signatures.set(index, signed);
self.as_of_block_height = height;
```
**Ring buffer**: `height % window_len` — her yeni blok N blok öncesinin verisini üzerine yazar.

#### `num_missed_blocks()` (satır 77-79)
```rust
self.signatures.iter_zeros().len()
```

#### `missed_blocks()` (satır 82-99)
Height aralığından missed height'ları döndürür:
```rust
let earliest_height = current_height.saturating_sub(window_len as u64 - 1);
all_heights.filter_map(|height| {
    let index = (height as usize) % window_len;
    let signed = self.signatures[index];
    Some(height).filter(|_| !signed)
})
```

### Testler

#### `counts_missed_blocks`
- 128 window, her 4. blok missed → 32 missed.
- Sonraki 128 blok hepsi signed → 0 missed.

#### `enumerate_missed_first_block`
- İlk blok (1) missed → `missed_blocks()` = [1].

#### `enumerate_uptime_simulates_bounded_queue` (proptest)
- `VecDeque` referans implementasyonu ile `Uptime` ring buffer'ı karşılaştırır.
- Random window_len (1-16) ve signed_blocks → her ikisinde de aynı missed block listesi.

#### `proto_round_trip`
- 113-size window, 300 blok → encode → decode → eşitlik.

### Tasarım Kararları

- **BitVec ring buffer**: 10000-block window için 10000 bit = 1250 byte. Çok verimli.
- **Başlangıç grace period**: Tüm 1'ler ile initialize — yeni validator hemen jail'e girmiyor.
- **Strict monoton height**: `height != as_of + 1` → hata. Block atlama tespiti.
- **Proto serialize**: unused bits `set_uninitialized(true)` ile kanonize edilir; round-trip tutarlılığı sağlanır.

### Nova Shield Chain İçin Dersler

- `BitVec` ring buffer, N-block sliding window için en verimli veri yapısı. Nova Shield downtime tracking için aynısını kullanın.
- Grace period ile yeni validator initialize: ilk N blokta imzalanmış varsay, yoksa hatalı jail tetiklenir.
- `proptest` + referans implementasyon pattern: ring buffer gibi karmaşık veri yapıları için simulation-based testing vazgeçilmez.

---

## [validator.rs]
**Dosya:** `.../stake/src/validator.rs`
**Satır Sayısı:** 290

### Genel Amaç
Validator yapılandırma veri yapısı.

### Struct'lar

#### `Validator` (satır 29-70)
| Alan | Tip | Kısıt |
|---|---|---|
| `identity_key` | `IdentityKey` | — |
| `governance_key` | `GovernanceKey` | — |
| `consensus_key` | `tendermint::PublicKey` | ED25519 |
| `name` | `String` | <= 140 char |
| `website` | `String` | <= 70 char |
| `description` | `String` | <= 280 char |
| `enabled` | `bool` | — |
| `funding_streams` | `FundingStreams` | max 8, toplam <= 10000 bps |
| `sequence_number` | `u32` | monoton artan |

#### `ValidatorToml` (satır 79-120)
TOML formatı için human-readable struct. `identity_key` ve `governance_key` `DisplayFromStr` ile string olarak yazılır.

#### `FundingStreamToml` (satır 182-193)
TOML için untagged enum:
- `Address { address, rate_bps }`
- `CommunityPool { recipient: CommunityPool, rate_bps }`

#### `CommunityPool` (satır 196-197)
Unit struct — sadece TOML'da `recipient = "CommunityPool"` yazısını desteklemek için.

### Tasarım Kararları

- **İki format**: `Validator` (domain) ve `ValidatorToml` (human-readable). Kullanıcılar TOML config dosyası yazabilir, bu otomatik `Validator`'a dönüşür.
- **Uzunluk limitleri hem `TryFrom<pb::Validator>` hem `TryFrom<ValidatorToml>`'da**: İki parsing yolunda da aynı validasyon, bir yol kısa devre yaparsa sorun çıkmaz.

### Nova Shield Chain İçin Dersler

- Validator tanımı için insan-okunabilir TOML/JSON format ayrı tutun; domain type ile senkronize edin.
- `FundingStreamToml::CommunityPool` gibi unit struct pattern, TOML serialization'ı anlamlı hale getirir.

---

## [validator/bonding.rs]
**Dosya:** `.../stake/src/validator/bonding.rs`
**Satır Sayısı:** 82

### Genel Amaç
Validator delegation pool'unun bonding state machine'i.

### Enum'lar

**`State`** (satır 6-23):
```rust
pub enum State {
    Bonded,           // aktif sette, unbonding delay var
    Unbonded,         // serbest, anında çekilebilir
    Unbonding { unbonds_at_height: u64 },  // belirli yükseklikte serbest olacak
}
```

### TryFrom<pb::BondingState>
- `Unbonding` state için `unbonds_at_height > 0` zorunlu.
- `Unspecified` → hata.

### Tasarım Kararları

- **Üçlü bonding state**: Bonded → Unbonding → Unbonded. `Unbonding` içinde hedef yükseklik saklanır.
- Eski proto'da `unbonds_at_epoch` vardı (deprecated); şimdi `unbonds_at_height`.

### Nova Shield Chain İçin Dersler

- Bonding state'i validator state'inden ayırın. Bir validator `Inactive` ama `Bonded` olabilir (yeni devre dışı bırakıldıysa unbonding sürer).
- `unbonds_at_height` block-bazlı takip, epoch-bazlıdan daha granüler.

---

## [validator/definition.rs]
**Dosya:** `.../stake/src/validator/definition.rs`
**Satır Sayısı:** 48

### Genel Amaç
Validator tanım güncelleme action'ı — imza ile.

### Struct'lar

**`Definition`** (satır 11-14):
```rust
pub struct Definition {
    pub validator: Validator,
    pub auth_sig: Signature<SpendAuth>,  // validator.identity_key ile imzalanmış
}
```

### Trait Implementasyonları
- `EffectingData`: Effect hash tüm proto encoding'den hesaplanır (validator + imza).

---

## [validator/info.rs]
**Dosya:** `.../stake/src/validator/info.rs`
**Satır Sayısı:** 69

### Genel Amaç
Validator hakkında birleşik bilgi.

### Struct'lar

**`Info`** (satır 12-16):
```rust
pub struct Info {
    pub validator: Validator,
    pub status: Status,
    pub rate_data: RateData,
}
```

RPC yanıtları için `ValidatorInfoResponse`'a dönüşüm sağlanır.

---

## [validator/state.rs]
**Dosya:** `.../stake/src/validator/state.rs`
**Satır Sayısı:** 78

### Genel Amaç
Validator state machine'inin durumlarını tanımlar.

### Enum'lar

**`State`** (satır 8-26):
```rust
pub enum State {
    Defined,     // tanımlı ama stake altında minimum
    Inactive,    // yeterli stake var, consensus set adayı
    Active,      // consensus set üyesi
    Jailed,      // downtime slashing
    Tombstoned,  // byzantine misbehavior (kalıcı)
    Disabled,    // operatör devre dışı bıraktı
}
```

### TryFrom<pb::ValidatorState>
`Unspecified` → hata.

---

## [validator/status.rs]
**Dosya:** `.../stake/src/validator/status.rs`
**Satır Sayısı:** 59

### Genel Amaç
Validator'ın anlık durumunu temsil eden birleşik struct.

### Struct'lar

**`Status`** (satır 11-20):
```rust
pub struct Status {
    pub identity_key: IdentityKey,
    pub voting_power: Amount,
    pub state: State,           // validator state machine durumu
    pub bonding_state: BondingState,  // delegation pool bonding durumu
}
```

---

## Genel Matematiksel Formüller Özeti

### Exchange Rate Modeli (BPS^2 Scaled)

```
Başlangıç:
  VXR[0] = 1.0 = 1_0000_0000

Her epoch için (sadece Active validator):
  commission = sum(funding_stream.rate_bps) / 10_000
  VRR[e] = base_reward_rate[e] * (1 - commission)
  VXR[e] = VXR[e-1] * (1 + VRR[e])

Penalty uygulandığında:
  VXR' = VXR[e] * penalty.kept_rate()

Delegation dönüşümleri:
  delegation_tokens = floor(unbonded_amount / VXR[e])
  unbonded_amount   = floor(delegation_tokens * VXR[e])
  voting_power      = floor(delegation_tokens * VXR[e])
```

### Base Rate Formülü

```
base_reward_rate[e] = issuance_budget[e] / total_active_stake[e-1]
base_exchange_rate[e] = base_exchange_rate[e-1] * (1 + base_reward_rate[e])
```

### Compound Penalty

```
compound(p1, p2) = p1.kept_rate * p2.kept_rate
// kept_rate = 1 - penalty_fraction
```

### Funding Stream Reward

```
R_{stream} = y_v * c_{stream} * r_e * psi_v(e)
  y_v = delegation_token_supply
  c_{stream} = stream.rate_bps / 10_000
  r_e = base_reward_rate[e] (descaled)
  psi_v(e) = validator_exchange_rate[e] (descaled)
```

---

## Unsafe Bloklar

Tüm 43 dosyada sıfır `unsafe` blok. `#![deny(clippy::unwrap_used)]` ile `.unwrap()` de yasak.

---

## Genel Tasarım Kararları Özeti

1. **Epoch-bazlı işlem**: Delegasyon/undelegasyon hemen etki etmez, epoch sınırında batch halinde işlenir.
2. **Exchange rate modeli**: Delegation token / staking token oranı merkezi. Ödüller exchange rate artışıyla temsil edilir.
3. **BPS^2 scaling (10^8)**: Integer aritmetiği ile floating point hassasiyeti.
4. **ZK kanıt ile claim**: Unbonding claim ZK proof gerektirir; privacy korunur ve stateless doğrulama mümkün olur.
5. **Dual state machine**: `validator::State` (operasyonel) + `validator::BondingState` (delegation pool) — bağımsız ama ilgili.
6. **Key non-deletion**: Consensus key lookup hiçbir zaman silinmez — byzantine evidence attribution.
7. **Checked arithmetic her yerde**: `checked_add`, `checked_sub`, `saturating_add` — overflow koruması.
8. **Ring buffer uptime**: `BitVec` ile N-block sliding window.
9. **Epoch grinding guard**: Validator disable/enable için cooldown süresi.
10. **Tombstone idempotency**: Byzantine ceza tek seferlik, tekrar uygulanmaz.

---

## Nova Shield Chain İçin Genel Dersler

### Zorunlu Benimsenmesi Gereken Tasarımlar

1. **BPS^2 Scaling Factor**: Tüm reward hesaplamalarında 10^8 scaling kullanın. Nova Shield için 10^18 (Ethereum tarzı) düşünülebilir.

2. **Exchange Rate Modeli**: Delegation shares yerine explicit exchange rate yönetimi daha şeffaf.
   ```rust
   delegation_tokens = floor(staking_tokens / exchange_rate)
   staking_tokens    = floor(delegation_tokens * exchange_rate)
   ```

3. **Compound Penalty (kept_rate)**: Slashing matematik için `1 - penalty` kept_rate kullanımı ve compounding ile çarpma.

4. **Validator State Machine Dokumentasyonu**: ASCII art diyagram ile tüm geçişler ve yan etkileri belgeleyin.

5. **Key Padding in State Keys**: Epoch/height index içeren state key'lerde sıfır doldurma: `{epoch:010}`.

6. **Epoch Grinding Guard**: Disable → enable için `last_disabled + epoch_duration` cooldown.

7. **Concurrent Validator Lookups**: Büyük validator setleri için `tokio::task::JoinSet` ile parallel fetch.

8. **Grace Period for New Validators**: BitVec ring buffer'ı tüm 1'lerle başlatın.

9. **ZK Proof Stateless Check**: ZK kanıt doğrulamasını stateless aşamaya taşıyın — mempool'da erken eleme.

10. **Non-Verifiable vs JMT Ayrımı**: Inclusion proof gereken veriler JMT'ye, operational veriler nonverifiable storage'a.

### Potansiyel İyileştirmeler

1. `EventSlashingPenaltyApplied` içinde sadece yeni penalty var; önceki değer de eklenebilir.
2. `ValidatorDataRead::get_validator_rate` için boxed future yerine `DomainFuture` kullanılabilirse object-safety yeniden düşünülebilir.
3. `process_validator` içindeki sequential validator döngüsü (issue #3874'te belirtilmiş) concurrent stream ile optimize edilebilir.
4. `genesis.rs`'de validators alanı `pb::Validator` yerine `validator::Validator` olabilir.

---

*Agent 10 — Penumbra Stake Component Analizi Tamamlandı*
*43 dosya, ~4500+ satır kaynak kod analiz edildi.*
