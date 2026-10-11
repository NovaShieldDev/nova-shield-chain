# Penumbra Analizi — Agent 07

**Analiz Tarihi:** 2026-07-22
**Kapsam:** component/community-pool (16 dosya) + component/funding (22 dosya)
**Toplam:** 38 dosya

---

## BÖLÜM 1: community-pool

---

## [action.rs]
**Dosya:** `.../community-pool/src/action.rs`
**Satır Sayısı:** 7

### Genel Amaç
Modül re-export dosyası. `community_pool_deposit`, `community_pool_output`, `community_pool_spend` alt modüllerini gizler; dışarıya yalnızca üç public struct'ı açar.

### Struct/Enum/Type Listesi
Yok (sadece re-export).

### Trait Implementasyonları
Yok.

### Fonksiyonlar
Yok.

### Unsafe Bloklar
Yok.

### Testler
Yok.

### Tasarım Kararları
- İç modüller `mod` (gizli), dışarıya yalnızca `pub use` ile seçici açılım — iyi Rust kapsülleme alışkanlığı.

### Nova Shield Chain İçin Dersler
- Her bileşenin kendi `action.rs` barrel dosyası olmalı. Dışarıdan import edilebilecek action tipleri tek bir yerden kontrol edilmeli.

---

## [action/community_pool_deposit.rs]
**Dosya:** `.../community-pool/src/action/community_pool_deposit.rs`
**Satır Sayısı:** 57

### Genel Amaç
Community Pool'a value yatırmayı temsil eden `CommunityPoolDeposit` struct'ını tanımlar. Governance protobuf'larıyla (governance/v1) eşleştirilir.

### Struct/Enum/Type Listesi

#### `CommunityPoolDeposit` (Satır 15–17)
```rust
pub struct CommunityPoolDeposit {
    pub value: Value,
}
```
- `Value`: `{ asset_id: asset::Id, amount: Amount }` — hangi token, ne kadar.
- Derive: `Clone, Debug, Serialize, Deserialize`
- Serde: `try_from = "pb::CommunityPoolDeposit"` / `into = "pb::CommunityPoolDeposit"` — proto round-trip garantisi.

### Trait Implementasyonları

#### `impl CommunityPoolDeposit` (Satır 19–24)
- `fn balance(&self) -> Balance`: Negatif balance döner (`-Balance::from(self.value)`).
  - Anlamı: Kullanıcı bu değeri harcıyor (wallet'tan çıkıyor). Community Pool'a giriyor.

#### `impl EffectingData for CommunityPoolDeposit` (Satır 26–30)
- `fn effect_hash(&self) -> EffectHash`: `EffectHash::from_proto_effecting_data(&self.to_proto())` — proto serialize → hash. Transaction effect hash hesabına katkı sağlar.

#### `impl DomainType for CommunityPoolDeposit` (Satır 32–34)
- `type Proto = pb::CommunityPoolDeposit` — canonical proto tipi.

#### `impl From<CommunityPoolDeposit> for pb::CommunityPoolDeposit` (Satır 36–42)
- `value: Some(msg.value.into())` — option wrapping zorunlu (proto3 optional).

#### `impl TryFrom<pb::CommunityPoolDeposit> for CommunityPoolDeposit` (Satır 44–56)
- `proto.value.ok_or_else(...)?` → `.try_into()` `.context("malformed value")?`
- Hata zinciri: eksik alan → `anyhow::anyhow!`, parse hatası → `context`.

### Fonksiyonlar
| fn | Parametreler | Dönüş | Algoritma |
|----|-------------|-------|-----------|
| `balance` | `&self` | `Balance` | `-Balance::from(self.value)` — negatif, value tüketilir |
| `effect_hash` | `&self` | `EffectHash` | Proto serialize → hash |

### Unsafe Bloklar
Yok.

### Testler
Yok.

### Tasarım Kararları
1. **Governance proto'su kullanımı:** CommunityPool için ayrı proto paketi olmasına rağmen `governance/v1`'den import ediliyor. Kod içinde `// TODO: why are the CommunityPool actions not in the Community Pool protos?` yorumu bunu şüpheli buluyor — teknik borç.
2. **Negatif balance:** Penumbra'da balance, girdi-çıktı dengesi olarak modellenir. Deposit negatif balance üretir çünkü kullanıcının transaction'ı bu değeri tüketmelidir.

### Nova Shield Chain İçin Dersler
- `balance()` → negatif/pozitif dönüş mantığı Nova Shield'in transaction value balancing sisteminde aynen kullanılabilir.
- Proto ve domain type ayrımı (DomainType trait) kesinlikle uygulanmalı — wire format değişikliği iç modeli etkilemez.
- `EffectingData` pattern'i: her action'ın canonical hash'i olmalı, TX imzası bu hash üzerinden yapılmalı.

---

## [action/community_pool_output.rs]
**Dosya:** `.../community-pool/src/action/community_pool_output.rs`
**Satır Sayısı:** 61

### Genel Amaç
Community Pool'dan belirli bir adrese değer göndermeyi temsil eder. `CommunityPoolSpend` ile farkı: Spend sadece value azaltır, Output hem value azaltır hem de belirli bir alıcı adres içerir ve shielded nota çevirir.

### Struct/Enum/Type Listesi

#### `CommunityPoolOutput` (Satır 12–15)
```rust
pub struct CommunityPoolOutput {
    pub value: Value,
    pub address: Address,
}
```
- `Address`: Penumbra shielded adresi (bech32m encode, ~80 byte).

### Trait Implementasyonları

#### `impl CommunityPoolOutput` (Satır 17–22)
- `fn balance(&self) -> Balance`: `-Balance::from(self.value)` — negatif. Pool değeri harcıyor.
  - NOT: CommunityPoolDeposit ile aynı işaret. Hem deposit hem output "kullanıcı tarafında" negatiftir ama mantıksal olarak farklı: deposit kullanıcıdan pool'a, output pool'dan kullanıcıya gider. Output'taki negatif balance "pool balance azalıyor" anlamına gelir.

#### `impl EffectingData`, `DomainType`, `From`, `TryFrom`
- Deposit ile yapısal olarak aynı; fark sadece `address` alanının proto dönüşümünde eklenmesi.

### Fonksiyonlar
`balance()` ve `effect_hash()` — deposit ile aynı imzalar, aynı mantık.

### Unsafe Bloklar
Yok.

### Testler
Yok.

### Tasarım Kararları
1. **Spend vs Output ayrımı:** Spend governance proposal tarafından tetiklenir (community pool bakiyesini azaltır), Output ise shielded nota mint eder. İki farklı action tipi ayrı tutulmuş — sorumluluk ayrımı nettir.
2. **Transparent output:** Action handler kodu "it's just a transparent output" diye yorumlanmış — governance kontekstinde hangi adrese gideceği zaten açıktır.

### Nova Shield Chain İçin Dersler
- Treasury/community fund'dan ödeme yaparken iki aşama düşünülmeli: (1) fund balance azaltma, (2) alıcıya mint. Bu iki işlemi ayrı action türleri ile modellemek daha güvenli.

---

## [action/community_pool_spend.rs]
**Dosya:** `.../community-pool/src/action/community_pool_spend.rs`
**Satır Sayısı:** 53

### Genel Amaç
Community Pool'un bakiyesini azaltan harcama action'ı. `Output`'tan farkı: hedef adres yok, sadece value. Governance önerisi ile tetiklenir; `Output` ise direkt alıcıya nota mint eder.

### Struct/Enum/Type Listesi

#### `CommunityPoolSpend` (Satır 11–13)
```rust
pub struct CommunityPoolSpend {
    pub value: Value,
}
```

### Trait Implementasyonları

#### `impl CommunityPoolSpend` (Satır 21–26)
- `fn balance(&self) -> Balance`: `Balance::from(self.value)` — POZİTİF.
  - Kritik fark: Deposit ve Output'ta negatif, Spend'de pozitif!
  - Sebep: Spend, pool'un kendi balance'ını azaltır ama TX perspektifinden "değer üretir" — başka bir output (CommunityPoolOutput) bu değeri tüketecek. Balance denkliği bu şekilde sağlanır.

#### `impl EffectingData`, `DomainType`, `From`, `TryFrom`
- Standart pattern — deposit ile aynı yapı.

### Tasarım Kararları
- **Balance işareti kritik:** Spend +balance, Output -balance üretir. İkisi çiftlenirse TX balance dengede olur. Bu şekilde governance önerisi `Spend + Output` ikilisi olarak atomik çalışır.

### Nova Shield Chain İçin Dersler
- Treasury harcamaları çift-girdi modeli: "treasury spend" (pozitif balance üretir) + "recipient output" (negatif balance tüketir). Bu denge zorunluluğu güvenlik garantisi sağlar — treasury'den fazla harcama imkânsız.

---

## [component.rs]
**Dosya:** `.../community-pool/src/component.rs`
**Satır Sayısı:** 57

### Genel Amaç
`CommunityPool` bileşeninin ABCI lifecycle implementasyonu. `cnidarium_component::Component` trait'ini implement eder.

### Struct/Enum/Type Listesi

#### `pub struct CommunityPool {}` (Satır 22)
- Boş struct — state tutmaz, sadece lifecycle hook'ları implement eder.

### Trait Implementasyonları

#### `impl Component for CommunityPool` (Satır 25–57)

```
type AppState = genesis::Content
```

| Hook | Parametreler | İşlem |
|------|-------------|-------|
| `init_chain` | `state: S, app_state: Option<&Content>` | Genesis params yazar + initial balance deposit eder |
| `begin_block` | `_state, _begin_block` | No-op |
| `end_block` | `_state, _end_block` | No-op |
| `end_epoch` | `_state` | No-op |

**`init_chain` detayı (Satır 29–37):**
```rust
match app_state {
    Some(genesis) => {
        state.put_community_pool_params(genesis.community_pool_params.clone());
        state.community_pool_deposit(genesis.initial_balance).await;
    }
    None => {}
}
```
- `None` branch: upgrade senaryosu — genesis state yoksa hiçbir şey yapılmaz.
- `community_pool_deposit` async, direkt await — genesis sırasında.

### Tasarım Kararları
1. **Boş struct:** Component'in state'i yok. Tüm state `StateWrite`/`StateRead` üzerinden JMT'den okunur. Rust zero-cost abstraction.
2. **No-op hooks:** Community Pool blok başı/sonu veya epoch sonunda özel işlem gerektirmiyor. Logic, action handler'larda.
3. **Upgrade compatibility:** `None` app_state branch, chain upgrade senaryosunda genesis yeniden çalıştırılmaması için.

### Nova Shield Chain İçin Dersler
- Lifecycle pattern: `init_chain`, `begin_block`, `end_block`, `end_epoch` — Nova Shield her bileşen için bu dört hook'u implement etmeli. İhtiyaç yoksa no-op.
- Genesis state opsiyonel (`Option<&AppState>`) tutmak upgrade uyumluluğu için kritik.

---

## [component/action_handler.rs]
**Dosya:** `.../community-pool/src/component/action_handler.rs`
**Satır Sayısı:** 3

### Genel Amaç
Barrel dosyası — üç action handler alt modülünü dahil eder. Public değil (mod), dışarı açmaz.

---

## [component/action_handler/community_pool_deposit.rs]
**Dosya:** `.../community-pool/src/component/action_handler/community_pool_deposit.rs`
**Satır Sayısı:** 19

### Genel Amaç
`CommunityPoolDeposit` action'ının stateless ve stateful doğrulama + yürütme mantığı.

### Trait Implementasyonları

#### `impl ActionHandler for CommunityPoolDeposit`

```
type CheckStatelessContext = ()
```

| Metod | Mantık |
|-------|--------|
| `check_stateless` | Her zaman `Ok(())` — herhangi bir deposit geçerli |
| `check_and_execute` | `state.community_pool_deposit(self.value).await` |

**`check_and_execute` detayı:**
- State write: `StateWriteExt::community_pool_deposit` çağrısı.
- `Ok(state.community_pool_deposit(...).await)` — unit return; community_pool_deposit void.

### Tasarım Kararları
- Stateless check yok çünkü deposit her zaman geçerli (herhangi biri pool'a para yatırabilir).
- Stateful overdraft kontrolü gerekmez (deposit, yalnızca ekler).

### Nova Shield Chain İçin Dersler
- Bazı action'lar için `check_stateless` no-op olabilir. Bunu açıkça `Ok(())` ile belirtmek ve yorum eklemek okunabilirlik açısından önemli.

---

## [component/action_handler/community_pool_output.rs]
**Dosya:** `.../community-pool/src/component/action_handler/community_pool_output.rs`
**Satır Sayısı:** 28

### Genel Amaç
`CommunityPoolOutput` action'ını execute eder: pool'dan değer alıp alıcıya shielded nota mint eder.

### Trait Implementasyonları

#### `impl ActionHandler for CommunityPoolOutput`

**`check_stateless`:** `Ok(())` — yorum: "Any output from the Community Pool is valid (it's just a transparent output)."

**`check_and_execute` (Satır 18–27):**
```rust
state.mint_note(
    self.value,
    &self.address,
    CommitmentSource::CommunityPoolOutput,
).await
```
- `NoteManager::mint_note`: shielded pool'a yeni nota ekler.
- `CommitmentSource::CommunityPoolOutput`: notanın kaynağını etiketler — audit trail için.

### Tasarım Kararları
- Balance check burada yok — balance dengesi TX seviyesinde (Spend + Output çifti) kontrol edilir.
- `CommitmentSource` enum'u notanın nereden geldiğini kayıt altına alır.

### Nova Shield Chain İçin Dersler
- Her mint işlemi için kaynak etiketleme (`CommitmentSource`) şart. Neyin neden mint edildiği iz bırakmalı.

---

## [component/action_handler/community_pool_spend.rs]
**Dosya:** `.../community-pool/src/component/action_handler/community_pool_spend.rs`
**Satır Sayısı:** 21

### Genel Amaç
`CommunityPoolSpend` action'ını execute eder. Pool bakiyesini azaltır.

### Trait Implementasyonları

#### `impl ActionHandler for CommunityPoolSpend`

**`check_stateless`:** `Ok(())` — "We can't statelessly check that the Community Pool has enough funds."

**`check_and_execute` (Satır 17–20):**
```rust
state.community_pool_withdraw(self.value).await
```
- `community_pool_withdraw`: overdraft kontrolü yapar; yetersiz bakiye → hata.
- Bu, stateful check'in `execute` içinde olduğu nadir örneklerden biri.

### Tasarım Kararları
- Stateless'ta bakiye kontrol edilemiyor (state bilinmiyor). Stateful kontrol execute'da yapılıyor. Bu doğru yaklaşım.

---

## [component/rpc.rs]
**Dosya:** `.../community-pool/src/component/rpc.rs`
**Satır Sayısı:** 85

### Genel Amaç
Community Pool bakiyelerini sorgulayan gRPC servisi. `QueryService` trait'ini implement eden `Server` struct'ı.

### Struct/Enum/Type Listesi

#### `pub struct Server` (Satır 20–22)
```rust
pub struct Server {
    storage: Storage,
}
```
- `Storage`: `cnidarium`'dan, JMT'ye erişim için.

### Trait Implementasyonları

#### `impl QueryService for Server` (Satır 31–84)

**`community_pool_asset_balances` (Satır 42–84):**
- Parametre: `CommunityPoolAssetBalancesRequest { asset_ids: Vec<pb::AssetId> }`
- Dönüş: `Stream<Item = CommunityPoolAssetBalancesResponse>`
- Algoritma:
  1. `storage.latest_snapshot()` — anlık görüntü.
  2. `request.asset_ids` → `Vec<asset::Id>` dönüşümü. Boşsa filtre uygulanmaz.
  3. `state.community_pool_balance().await` — tüm bakiyeleri çek (`BTreeMap<asset::Id, Amount>`).
  4. `try_stream!` macro ile async stream oluştur: her `(asset_id, amount)` çifti için filtre uygula, `pb::Value` olarak yield et.
  5. `.map_ok(|value| CommunityPoolAssetBalancesResponse { balance: Some(value) })` — proto wrap.

### Tasarım Kararları
1. **Streaming response:** Tek bir protobuf message yerine stream — çok sayıda asset için bellek dostu.
2. **Optional filter:** `asset_ids` boşsa tüm bakiyeler döner. Kliyent tarafında filtreleme yerine server-side filtering destekleniyor.
3. **`try_stream!` macro:** `async_stream` crate'inden — async generator benzeri syntax.

### Nova Shield Chain İçin Dersler
- Treasury bakiyesi sorguları stream olarak tasarlanmalı.
- `latest_snapshot()` pattern: her RPC handler fresh snapshot alır, stale state riski yok.
- Optional filter pattern (boş liste = tüm kayıtlar) kullanıcı dostu API tasarımı.

---

## [component/state_key.rs]
**Dosya:** `.../community-pool/src/component/state_key.rs`
**Satır Sayısı:** 14

### Genel Amaç
JMT (Jellyfish Merkle Tree) anahtar şemalarını tanımlar.

### Fonksiyonlar

| fn | Dönüş | Değer |
|----|-------|-------|
| `community_pool_params()` | `&'static str` | `"community_pool/params"` |
| `balance_for_asset(asset_id)` | `String` | `"community_pool/asset/{asset_id}"` |
| `all_assets_balance()` | `&'static str` | `"community_pool/asset/"` |

### Tasarım Kararları
- `all_assets_balance()` `balance_for_asset` için prefix olması zorunluluğu açıkça belirtilmiş (`// note: this must be the prefix of the above`). Prefix scan ile tüm bakiyeler çekilebilir.
- String key formatı: `/` ile hiyerarşik. Prefix scan güvenli çünkü `"community_pool/asset/"` sadece asset balance key'lerini kapsar.

### Nova Shield Chain İçin Dersler
- State key fonksiyonları ayrı dosyaya çekilmeli. String literal yerine `&'static str` veya `format!` kullanımı standartlaştırılmalı.
- Prefix/key ilişkisi belgelenmeli — özellikle scan operasyonları için.

---

## [component/view.rs]
**Dosya:** `.../community-pool/src/component/view.rs`
**Satır Sayısı:** 85

### Genel Amaç
Community Pool state okuma/yazma extension trait'leri. `StateRead` ve `StateWrite` üzerine inşa edilir.

### Trait Implementasyonları

#### `pub trait StateReadExt: StateRead` (Satır 17–44)

| Metod | Dönüş | Algoritma |
|-------|-------|-----------|
| `get_community_pool_params` | `Result<CommunityPoolParameters>` | JMT'den proto deserialize; yoksa hata |
| `community_pool_asset_balance(asset_id)` | `Result<Amount>` | Tek asset balance; yoksa `Amount::from(0u64)` |
| `community_pool_balance()` | `Result<BTreeMap<asset::Id, Amount>>` | Prefix scan + key parse + try_collect |

**`community_pool_balance` detayı (Satır 32–43):**
```rust
let prefix = state_key::all_assets_balance();
self.prefix(prefix)
    .map(|result| {
        let (key, amount) = result?;
        let asset_id = key.rsplit('/').next().expect("key is well-formed");
        let asset_id = asset::Id::from_str(asset_id)?;
        Ok((asset_id, amount))
    })
    .try_collect()
    .await
```
- `rsplit('/')` ile key'in son segmentini alır (asset_id string).
- `try_collect()` → `BTreeMap` — sıralı map, deterministik iterasyon.

**Blanket impl (Satır 46):**
```rust
impl<T> StateReadExt for T where T: StateRead + ?Sized {}
```

#### `pub trait StateWriteExt: StateWrite` (Satır 48–85)

| Metod | Dönüş | Algoritma |
|-------|-------|-----------|
| `put_community_pool_params` | `()` | JMT'ye proto serialize + put |
| `community_pool_deposit(value)` | `()` (async) | Mevcut bakiye + value.amount |
| `community_pool_withdraw(value)` | `Result<()>` | checked_sub; yetersizse bail! |

**`community_pool_withdraw` detayı (Satır 65–82):**
```rust
let current = self.get(&key).await?.unwrap_or_else(|| Amount::from(0u64));
if let Some(remaining) = u128::from(current).checked_sub(u128::from(value.amount)) {
    if remaining > 0 {
        self.put(key, Amount::from(remaining));
    } else {
        self.delete(key);   // sıfır bakiyeleri silme — temiz state
    }
} else {
    anyhow::bail!("insufficient balance...");
}
```
- `u128` cast: `Amount` → `u128` → checked_sub. Overflow güvenceli.
- `remaining == 0` → key silinir. Storage'da sıfır değer tutulmaz.

### Tasarım Kararları
1. **checked_sub ile overdraft koruması:** `u128::checked_sub` → None → bail! Underflow imkânsız.
2. **Sıfır bakiye silme:** `remaining > 0` kontrolü — state temizliği. Prefix scan'de gereksiz sıfırlar dönmez.
3. **`unwrap_or_else(|| Amount::from(0u64))`:** Mevcut olmayan bakiyeler sıfır kabul edilir.

### Nova Shield Chain İçin Dersler
- Treasury withdraw her zaman `checked_sub` kullanmalı — overdraft kesinlikle `bail!` ile reddedilmeli.
- Sıfır değerleri storage'da tutma — `delete` ile temizle. Prefix scan'leri kirletir.
- Extension trait pattern (`StateReadExt`, `StateWriteExt`) Nova Shield'de her bileşen için uygulanmalı.

---

## [event.rs]
**Dosya:** `.../community-pool/src/event.rs`
**Satır Sayısı:** 5

### Genel Amaç
Placeholder dosya. İçerik yalnızca TODO yorumu:
```
// TODO: events for
// - Community Pool spend
// - Community Pool deposit
// ?
```

### Tasarım Kararları
- Community Pool için event sistemi henüz implement edilmemiş — teknik borç. Spend ve deposit event'leri eklenmelidir.

### Nova Shield Chain İçin Dersler
- Her state değiştiren action için event emit edilmeli. Bu bir TODO değil, zorunluluk olmalı. Indexer ve explorer desteği için kritik.

---

## [genesis.rs]
**Dosya:** `.../community-pool/src/genesis.rs`
**Satır Sayısı:** 57

### Genel Amaç
Genesis state yapısı. Chain başlatıldığında Community Pool'un ilk parametreleri ve bakiyesini tanımlar.

### Struct/Enum/Type Listesi

#### `pub struct Content` (Satır 10–15)
```rust
pub struct Content {
    pub community_pool_params: CommunityPoolParameters,
    pub initial_balance: Value,
}
```

### Trait Implementasyonları

#### `impl Default for Content` (Satır 47–57)
```rust
Content {
    community_pool_params: CommunityPoolParameters::default(),
    initial_balance: Value {
        amount: 0u128.into(),
        asset_id: *STAKING_TOKEN_ASSET_ID,
    },
}
```
- Default: sıfır başlangıç bakiyesi, staking token cinsinden.

### Tasarım Kararları
- `STAKING_TOKEN_ASSET_ID` static referans (`*` deref). Genesis'te pool boş başlar.
- `TryFrom<pb::GenesisContent>` her alanı ayrı `.context()` ile doğrular — hata lokalizasyonu.

### Nova Shield Chain İçin Dersler
- Genesis struct'ı her bileşen için zorunlu. Default değerler mantıklı olmalı (boş pool, temel parametreler).

---

## [lib.rs] (community-pool)
**Dosya:** `.../community-pool/src/lib.rs`
**Satır Sayısı:** 15

### Genel Amaç
Crate kök barrel. Feature flag ile `component` modülü koşullu derlenir.

```rust
#![deny(clippy::unwrap_used)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#[cfg(feature = "component")]
pub mod component;
```

### Tasarım Kararları
- `#![deny(clippy::unwrap_used)]`: Crate genelinde `.unwrap()` yasak — `expect()` veya `?` kullanımı zorunlu. Güvenlik için.
- `feature = "component"`: Component kodu opsiyonel — yalnızca full-node binary'sinde derlenir, light client'ta değil.

### Nova Shield Chain İçin Dersler
- `deny(clippy::unwrap_used)` tüm Nova Shield crate'lerinde zorunlu olmalı.
- Feature flag ile light/full ayrımı: consensus logic ile node logic ayrı feature'larda.

---

## [params.rs] (community-pool)
**Dosya:** `.../community-pool/src/params.rs`
**Satır Sayısı:** 43

### Genel Amaç
Community Pool runtime parametreleri.

### Struct/Enum/Type Listesi

#### `pub struct CommunityPoolParameters` (Satır 10–13)
```rust
pub struct CommunityPoolParameters {
    pub community_pool_spend_proposals_enabled: bool,
}
```
- Tek parametre: spend önerilerinin aktif/pasif olması.

#### `impl Default for CommunityPoolParameters` (Satır 37–43)
```rust
Self {
    community_pool_spend_proposals_enabled: true,
}
```
- Default: spend önerileri aktif.

### Tasarım Kararları
- Parametre governance ile değiştirilebilir (governance proposal → param update). Runtime switchable.

### Nova Shield Chain İçin Dersler
- Treasury harcama mekanizması governance ile enable/disable edilebilir olmalı. Acil durdurma için kritik.

---

## BÖLÜM 2: funding

---

## [action_handler/mod.rs] (funding)
**Dosya:** `.../funding/src/action_handler/mod.rs`
**Satır Sayısı:** 1

```rust
pub mod liquidity_tournament;
```
Sadece alt modül bildirimi.

---

## [action_handler/liquidity_tournament/mod.rs]
**Dosya:** `.../funding/src/action_handler/liquidity_tournament/mod.rs`
**Satır Sayısı:** 191

### Genel Amaç
`ActionLiquidityTournamentVote` action'ının stateless ve stateful doğrulama + yürütme mantığı. Liquidity tournament oy kullanma mekanizmasının kalbi.

### Fonksiyonlar

#### `fn is_valid_denom(denom: &Denom) -> anyhow::Result<()>` (Satır 27–40)
- Parametre: denom string referansı
- Doğrulama 1: `denom.denom.len() <= LIQUIDITY_TOURNAMENT_VOTE_DENOM_MAX_BYTES` (256 byte)
- Doğrulama 2: `denom.denom.starts_with("transfer/")` — sadece IBC transfer asset'leri oylanabilir
- IBC dışı native asset'ler (örn. staking token) oy alamaz — tasarım kararı.

#### `async fn start_position_good_for_epoch(current: Epoch, start: Position) -> anyhow::Result<()>` (Satır 43–56)
- Not'un kullanılan position'ının mevcut epoch'tan ÖNCE olmasını doğrular.
- Mantık:
  ```
  (e, _, _) if e < current.index => true       // kesinlikle eski epoch
  (e, 0, 0) if e == current.index => true      // tam epoch başı (block=0, commitment=0)
  _ => false                                    // aksi halde geçersiz
  ```
- ZK proof not'un position'dan ÖNCE olduğunu kanıtlar, bu yüzden epoch başı pozisyonu güvenli.
- Bu epoch'ta yaratılan delegation token'ları bu epoch'ta oylamada kullanılamaz — sybil önlemi.

#### `fn voting_power(amount: Amount) -> u64` (Satır 59–64)
- `amount.value().try_into().expect(...)` — `u128` → `u64` cast.
- `u64::MAX` üzeri amount sahibi olmak "birinin böyle bir miktarı edinmesi" olarak yorum etiketlenmiş (imkânsız sayılıyor).

### Trait Implementasyonları

#### `impl ActionHandler for ActionLiquidityTournamentVote` (Satır 67–191)

**`check_stateless` (Satır 70–108):**
1. `is_valid_denom(incentivized)?` — denom kontrolü
2. `rk.verify(context.effect_hash.as_ref(), auth_sig)` — spend auth imzası doğrulama
3. ZK proof doğrulama:
   ```rust
   let public = LiquidityTournamentVoteProofPublic {
       anchor: context.anchor,
       value,
       nullifier,
       rk,
       start_position,
   };
   proof.verify(&DELEGATOR_VOTE_PROOF_VERIFICATION_KEY, public)
   ```
   - `DELEGATOR_VOTE_PROOF_VERIFICATION_KEY`: Groth16 verifying key (static, compile-time)

**`check_and_execute` (Satır 110–189):**
1. `start_position_good_for_epoch(current_epoch, self.body.start_position)` — pozisyon geçerliliği
2. Nullifier duplicate check:
   ```rust
   let nullifier_exists = state.get_lqt_spent_nullifier(nullifier).await.is_some();
   anyhow::ensure!(!nullifier_exists, "nullifier {} already voted...");
   ```
3. Nullifier kaydet: `state.put_lqt_spent_nullifier(epoch, nullifier, tx_id)`
4. Validator lookup: `state.validator_by_delegation_asset(self.body.value.asset_id)`
5. Validator state check: `Defined | Tombstoned` → red
6. Validator rate kontrolü:
   ```rust
   let validator_rate = state.get_validator_rate(&validator).await?...;
   let unbonded_amount = validator_rate.unbonded_amount(self.body.value.amount);
   ensure!(unbonded_amount > Amount::zero(), "unbonded amount is zero");
   ```
7. Voting power hesabı: `let power = voting_power(unbonded_amount)`
8. Event emit: `EventLqtVote { epoch_index, voting_power, incentivized_asset_id, ... }`
9. Tally: `state.tally(epoch, incentivized_asset_id, power, validator, &rewards_recipient).await`

### Tasarım Kararları
1. **IBC-only denom kısıtlaması:** Sadece `transfer/` prefix'li asset'ler oy alabilir. Bu, staking token'ının kendi kendini incentivize etmesini önler.
2. **Unbonded amount kullanımı:** Delegation token miktarı direkt voting power değil — validator exchange rate üzerinden `unbonded_amount`'a çevriliyor. Bu, farklı validator exchange rate'leri arasında adil karşılaştırma sağlar.
3. **Nullifier per epoch:** Nullifier epoch index ile birlikte key'lenir. Aynı nullifier farklı epoch'larda kullanılabilir (her epoch için bir oy).
4. **Validator state filtresi:** `Defined` (yeni, aktif değil) ve `Tombstoned` (cezalandırılmış) validatörler için oy geçersiz.
5. **ZK proof stateless:** Proof, state bilgisi gerektirmeyen anchor ve nullifier'ı içerir. Stateless doğrulama hızlı.

### Nova Shield Chain İçin Dersler
- **Liquidity incentive sistemi için tam şablon:** IBC token → oy → LP + delegator ödül dağıtımı. Nova Shield DEX için aynen adapte edilebilir.
- Voting power için raw token miktarı değil, unbonded/normalized değer kullanmak adillik sağlar.
- Nullifier tabanlı double-vote önleme: ZK-based sistemlerde tek güvenilir yöntem.
- Validator sağlığı kontrolü (Tombstoned reject) oy ağırlıklandırmasında önemli.

---

## [component.rs] (funding)
**Dosya:** `.../funding/src/component.rs`
**Satır Sayısı:** 187

### Genel Amaç
`Funding` bileşeninin ABCI lifecycle implementasyonu. `end_epoch` içinde iki büyük işlem: LQT ödül dağıtımı + staking funding stream işleme.

### Struct/Enum/Type Listesi

#### `pub struct Funding {}` (Satır 26)
- Boş struct, state tutmaz.

### Trait Implementasyonları

#### `impl Component for Funding` (Satır 29–187)

**`init_chain` (Satır 33–40):**
- `state.put_funding_params(genesis.funding_params.clone())`

**`end_epoch` (Satır 57–186) — KRİTİK FONKSİYON:**

Adım 1: LQT ödüllerini dağıt
```rust
if let Err(e) = liquidity_tournament::distribute_rewards(&mut *state).await {
    tracing::error!("while processing LQT rewards: {}", e);
}
```
- Hata durumunda panic yok, `tracing::error!` ile logla + devam et.

Adım 2: Funding queue al
```rust
let funding_queue = state.get_funding_queue().unwrap_or_default();
```
- Staking component'in hazırladığı queue: `Vec<(ValidatorIdentity, Vec<FundingStream>, delegation_token_supply)>`

Adım 3: Base rate al
```rust
let Some(base_rate) = state.get_previous_base_rate() else {
    tracing::error!("ending epoch base rate not found");
    return Ok(());
};
```
- Eksik base rate → erken çıkış (panic değil).

Adım 4: Staking issuance budget kontrolü
```rust
let staking_issuance_budget = state
    .get_staking_token_issuance_for_epoch()
    .expect("staking token issuance MUST be set");
```
- Bu `expect` intentional — issuance budget yoksa devam etmek anlamsız.

Adım 5: Her validator + funding stream için ödül hesapla
```rust
for (validator_identity, funding_streams, delegation_token_supply) in funding_queue {
    for stream in funding_streams {
        let reward_amount_for_stream = stream.reward_amount(
            base_rate.base_reward_rate,
            validator_rate.validator_exchange_rate,
            delegation_token_supply,
        );
        total_staking_rewards_for_epoch =
            total_staking_rewards_for_epoch.saturating_add(reward_amount_for_stream.value());
        match stream.recipient() {
            Recipient::Address(address) => {
                state.mint_note(Value { amount: ..., asset_id: *STAKING_TOKEN_ASSET_ID }, &address, CommitmentSource::FundingStreamReward { epoch_index }).await?;
            }
            Recipient::CommunityPool => {
                state.community_pool_deposit(Value { ... }).await;
            }
        }
    }
}
```

**Reward formülü** (`stream.reward_amount` — staking crate'te):
```
reward = base_reward_rate * validator_exchange_rate * delegation_token_supply
```

Adım 6: Budget kontrolü ve log
```rust
let rewards_difference = (total_staking_rewards_for_epoch as i128
    - staking_issuance_budget.value() as i128) as f64;
gauge!(metrics::VALIDATOR_FUNDING_VS_BUDGET_DIFFERENCE).set(rewards_difference);

if total_staking_rewards_for_epoch > staking_issuance_budget.value() {
    tracing::warn!("total staking rewards exceeded staking token issuance budget target");
}
```
- Budget aşımı WARN ile loglanır, panic değil — soft limit.

### Tasarım Kararları
1. **LQT hata toleransı:** LQT dağıtım hatası bloku durdurmaz. `tracing::error!` + devam. Staking ödülleri daha kritik sayılıyor.
2. **Soft budget limit:** Staking ödülleri budget'ı aşarsa sadece warn — chain durmuyor. Hard limit yerine monitoring.
3. **İki farklı alıcı tipi:** `Recipient::Address` → shielded nota mint. `Recipient::CommunityPool` → pool deposit. Tek bir match arm ile her iki case handle.
4. **`saturating_add`:** Toplam ödül hesabında taşma güvencesi.

### Nova Shield Chain İçin Dersler
- **Epoch sonu işlem sırası önemli:** LQT → Staking. Bu sıra kasıtlı. LQT hata olsa bile staking devam etmeli.
- Budget monitoring gauge metric olarak tutulmalı — alarm sistemi için.
- `CommitmentSource::FundingStreamReward { epoch_index }`: Epoch bazlı kaynak etiketleme — validator ödülleri için iz bırakır.
- `saturating_add` toplam ödül toplamında zorunlu — overflow'dan korur.

---

## [component/liquidity_tournament/bank.rs]
**Dosya:** `.../funding/src/component/liquidity_tournament/bank.rs`
**Satır Sayısı:** 89

### Genel Amaç
Liquidity tournament ödül ihracını kontrol eden `Bank` trait'i. Voter'lara (delegatörler) ve LP pozisyonlarına ödül dağıtır. İki farklı hedef için farklı mekanizma.

### Trait Implementasyonları

#### `pub trait Bank: StateWrite + Sized` (Satır 27–89)

**`async fn reward_to_voter(...)` (Satır 30–75):**
```rust
async fn reward_to_voter(
    &mut self,
    unbonded_reward: Amount,     // staking token cinsinden ödül
    validator: IdentityKey,      // hangi validatör pool'una yatırılacak
    voter: &Address,             // ödülü alacak adres
    tx_hash: TransactionId,      // kayıt için
    incentivized_asset_id: asset::Id,
) -> anyhow::Result<()>
```
- Sıfır ödül erken çıkış: `if unbonded_reward == Amount::zero() { return Ok(()); }`
- `deposit_to_validator_pool(&validator, unbonded_reward)` → `bonded_reward` (delegation token)
  - Staking token ödül → validator pool'una yatır → delegation token al
  - Bu, voter'ın validatör stake'ini artırır!
- `mint_note(bonded_reward, voter, CommitmentSource::LiquidityTournamentReward { epoch, tx_hash })`
  - Delegation token nota olarak voter'a mint edilir.

**`async fn reward_to_position(...)` (Satır 78–86):**
```rust
async fn reward_to_position(&mut self, reward: Amount, lp: position::Id) -> anyhow::Result<()>
```
- Sıfır ödül erken çıkış: `if reward == Amount::default() { return Ok(()); }`
- `self.reward_position(lp, reward).await?` — DEX PositionManager'a delege eder; LP rezervlerini artırır.

**Blanket impl (Satır 89):**
```rust
impl<T: StateWrite + Sized> Bank for T {}
```

### Tasarım Kararları
1. **Voter → delegation token:** Voter staking token değil, delegation token alır. Bu otomatik re-stake mekanizması — compound büyüme. LQT oylamak validator'a stake artırımı anlamına gelir.
2. **LP → rezerv artışı:** LP pozisyonu nota almaz, doğrudan rezervi artar. LP için ödül likit değil, pozisyon içinde kalır.
3. **`deposit_to_validator_pool`:** `Option<Value>` döner — validator pool yoksa None → bail!
4. **Event kaydı:** `EventLqtDelegatorReward` hem unbonded (staking) hem bonded (delegation) miktarı içerir.

### Nova Shield Chain İçin Dersler
- **LP incentive tasarımı kritik karar:** Nova Shield DEX LP'leri ödül olarak doğrudan pozisyon rezervi artışı mı alacak, yoksa yeni token mi? Penumbra rezerv artışını seçmiş — LP'yi pozisyonda tutmaya zorlar.
- Voter'a delegation token vermek "stake et ve oy kullan" döngüsünü güçlendirir.
- `CommitmentSource::LiquidityTournamentReward` ile kaynak etiketleme — her minted nota neden mint edildiğini bilir.

---

## [component/liquidity_tournament/mod.rs]
**Dosya:** `.../funding/src/component/liquidity_tournament/mod.rs`
**Satır Sayısı:** 6

```rust
mod bank;
pub mod nullifier;
mod rewards;
pub mod votes;
pub(crate) use rewards::distribute_rewards;
```
- `rewards::distribute_rewards` crate içi erişime açık.
- `bank` ve `rewards` dışarıya açılmamış.

---

## [component/liquidity_tournament/nullifier/mod.rs]
**Dosya:** `.../funding/src/component/liquidity_tournament/nullifier/mod.rs`
**Satır Sayısı:** 72

### Genel Amaç
LQT oy nullifier'larını okuma/yazma. Her epoch'ta her nullifier yalnızca bir kez kullanılabilir (double-vote önleme).

### Trait Implementasyonları

#### `pub trait NullifierRead: StateRead` (Satır 10–53)

**`async fn get_lqt_spent_nullifier(nullifier) -> Option<TransactionId>` (Satır 13–32):**
- Ambient epoch index alır.
- `state_key::lqt::v1::nullifier::key(epoch_index, &nullifier)` → NV storage'dan oku.
- NV (non-verifiable) storage: JMT'ye girmez, Merkle proof üretmez. Hız için.

**`async fn get_lqt_spent_nullifier_by_epoch(nullifier, epoch_index) -> Option<TransactionId>` (Satır 36–52):**
- Belirli bir epoch için nullifier sorgulama.
- RPC tarafından çağrılır (`lqt_check_nullifier`).

#### `pub trait NullifierWrite: StateWrite` (Satır 57–70)

**`fn put_lqt_spent_nullifier(epoch_index, nullifier, tx_id)`:**
- `state_key::lqt::v1::nullifier::key(epoch_index, &nullifier)` → NV storage'a yaz.
- Key format: `"funding/lqt/v1/nullifier/{epoch_index:020}/lookup/{nullifier}"`
- `epoch_index:020` — 20 haneli sıfır doldurmalı sayı. Lexicographic sıralama için.

### Tasarım Kararları
1. **NV storage kullanımı:** Nullifier'lar konsensüs state'e girmez (JMT değil). Double-vote önleme için hız yeterli; Merkle proof gerekmez.
2. **Epoch-scoped nullifier:** `{epoch}/{nullifier}` key yapısı — aynı nullifier farklı epoch'larda yeniden kullanılabilir.
3. **`format!("{:020}")` :** Lexicographic prefix scan için zorunlu zero-padding.

### Nova Shield Chain İçin Dersler
- ZK-based voting'de nullifier double-spend koruması NV storage'da tutulabilir. JMT gereksiz — Merkle proof istenmiyorsa NV daha hızlı.
- Epoch-scoped key: `{epoch:020}/{nullifier}` formatı.

---

## [component/liquidity_tournament/rewards/mod.rs]
**Dosya:** `.../funding/src/component/liquidity_tournament/rewards/mod.rs`
**Satır Sayısı:** 233

### Genel Amaç
LQT ödül dağıtım algoritmasının tamamı. `distribute_rewards` fonksiyonu epoch sonunda çalışır.

### Fonksiyonlar

#### `fn create_share(portion, total) -> U128x128` (Satır 24–27)
```rust
U128x128::ratio(portion.into(), total.into().max(U128x128::from(1u64)))
    .expect("max(x, 1) cannot be 0")
```
- `max(total, 1)`: Sıfıra bölme önleme. Total 0 olsa bile 0/1 = 0 döner.
- `U128x128`: 128-bit fixed-point, 128 bit tamsayı + 128 bit kesirli. Hassas oran hesabı.

#### `async fn relevant_votes_for_asset(state, params, epoch, asset) -> anyhow::Result<Amount>` (Satır 29–43)
- `state.ranked_voters(epoch, asset)` stream'ini `max_delegators` ile limitler.
- Sadece en fazla `max_delegators` voter'ın oyları sayılır. Kuyruk kesilir.

#### `async fn asset_totals(state, params, epoch) -> anyhow::Result<Vec<(asset::Id, Amount)>>` (Satır 52–72)
- `state.ranked_assets(epoch)` stream'ini iterate eder (azalan power sırasında).
- Her asset için `relevant_votes_for_asset` çağrısı.
- Eşik kontrolü:
  ```rust
  if create_share(votes, total_votes) < U128x128::from(params.gauge_threshold) {
      break;  // assets ranked descending; ilk eşik altı = sonrakiler de eşik altı
  }
  ```
- Eşiği geçen asset'ler `Vec` olarak döner.

#### `fn voter_shares_of_asset(state, params, epoch, asset, total) -> impl Stream<...>` (Satır 74–88)
- `ranked_voters` stream + `.take(max_delegators)` limit.
- Her `((votes, voter, validator), tx)` için `create_share(votes, total)` hesaplar.
- `Stream<Item = (Address, U128x128, IdentityKey, TransactionId)>` döner.

#### `async fn relevant_positions_total_volume(state, params, epoch, asset) -> anyhow::Result<Amount>` (Satır 90–104)
- `state.positions_by_volume_stream(epoch, asset)?` → `.take(max_positions)`.
- LP'lerin toplam hacmini sum eder.

#### `fn position_shares(state, params, epoch, asset, total_volume) -> impl Stream<...>` (Satır 106–118)
- `positions_by_volume_stream` + `.take(max_positions)`.
- `(lp_id, volume, create_share(volume, total_volume))` üçlüsü.

#### `pub async fn distribute_rewards(mut state: impl StateWrite) -> anyhow::Result<()>` (Satır 121–233)

**KRITIK ALGORİTMA — TAM DETAY:**

**Adım 1: Budget al ve community pool'dan çek**
```rust
let initial_budget = state.get_lqt_reward_issuance_for_epoch(current_epoch).await.unwrap_or_default();
if initial_budget <= 0u64.into() {
    return Ok(());  // bütçe yoksa erken çıkış
}
state.community_pool_withdraw(Value { asset_id: *STAKING_TOKEN_ASSET_ID, amount: initial_budget }).await?;
let mut current_budget = initial_budget;
```

**Adım 2: Asset listesini hesapla**
```rust
let asset_totals = asset_totals(&state, &params.liquidity_tournament, current_epoch).await?;
let total_votes: Amount = asset_totals.iter().map(|(_, v)| *v).sum();
```

**Adım 3: Her asset için ödül dağıt**
```rust
for (incentivized_asset, asset_votes) in asset_totals {
    let asset_share = create_share(asset_votes, total_votes);
    // --- Delegatör ödülleri ---
    let mut voter_stream = voter_shares_of_asset(...);
    while let Some((voter, voter_share, identity_key, tx)) = voter_stream.try_next().await? {
        let unbonded_reward_amount =
            ((U128x128::from(params.liquidity_tournament.delegator_share) * asset_share)?
                * voter_share)?
            .apply_to_amount(&initial_budget)?;
        state.reward_to_voter(unbonded_reward_amount, identity_key, &voter, tx, incentivized_asset).await?;
        current_budget = current_budget.checked_sub(&unbonded_reward_amount)
            .ok_or(anyhow!("LQT rewards exceeded budget"))?;
    }
    // --- LP ödülleri ---
    let total_volume = relevant_positions_total_volume(...).await?;
    let mut lp_stream = position_shares(...);
    while let Some((lp, lp_volume, lp_share)) = lp_stream.try_next().await? {
        let lp_reward_share = U128x128::from(params.liquidity_tournament.delegator_share.complement());
        let reward = ((lp_reward_share * asset_share)? * lp_share)?.apply_to_amount(&initial_budget)?;
        state.reward_to_position(reward, lp).await?;
        current_budget = current_budget.checked_sub(&reward)
            .ok_or(anyhow!("LQT rewards exceeded budget"))?;
    }
}
```

**Adım 4: Kalan budget'ı geri yatır + sonraki epoch'a taşı**
```rust
state.community_pool_deposit(Value { asset_id: *STAKING_TOKEN_ASSET_ID, amount: current_budget }).await;
state.set_lqt_reward_issuance_for_epoch(current_epoch + 1, current_budget);
```

**LQT Ödül Formülleri (matematiksel):**

Delegatör ödülü:
```
delegator_reward(voter) =
    initial_budget
    × delegator_share
    × (asset_votes / total_votes)
    × (voter_votes / asset_total_votes_top_N)
```

LP ödülü:
```
lp_reward(position) =
    initial_budget
    × (1 - delegator_share)
    × (asset_votes / total_votes)
    × (position_volume / total_volume_top_M)
```

Burada:
- `delegator_share`: `Percentage` (0–100%)
- `(1 - delegator_share)` = `delegator_share.complement()`
- `total_votes` = tüm threshold'u geçen asset'lerin top-N voter toplamı
- `initial_budget` sabit (döngü içinde değişmez), `current_budget` azalır (overflow kontrolü)

### Tasarım Kararları
1. **`initial_budget` sabit, `current_budget` değişken:** Share hesapları `initial_budget`'a göre yapılır. `current_budget` sadece overdraft kontrolü için.
2. **Kalan budget bir sonraki epoch'a taşınır:** Rounding artıkları kaybolmaz — kompound birikir.
3. **Gauge threshold:** Asset'in toplam vote'un belirli bir yüzdesini alması gerekir. Küçük asset'ler ödül alamaz.
4. **max_delegators / max_positions limiti:** Hesaplama maliyeti sınırlandırılır — top-N voter, top-M LP.
5. **`checked_sub` ile budget koruması:** Ödül toplamı budget'ı asla aşamaz. Overflow → `anyhow!` ile hata.

### Nova Shield Chain İçin Dersler
- **LQT sistemi direkt kopyalanabilir:** Nova Shield DEX için incentive mekanizması olarak.
- `U128x128` fixed-point ratio hesabı: precision kaybı olmadan oran hesaplamak için zorunlu.
- `.complement()` pattern: `(1 - share)` hesabı için `Percentage::complement()` metodu — floating point riski yok.
- Budget carry-forward: Epoch sonunda kalan her şeyi bir sonraki epoch'a yolla, yakma.
- Gauge threshold: Küçük/spam asset'leri filtreler. Nova Shield DEX'te minimum oy oranı şartı.

---

## [component/liquidity_tournament/votes/mod.rs]
**Dosya:** `.../funding/src/component/liquidity_tournament/votes/mod.rs`
**Satır Sayısı:** 215

### Genel Amaç
LQT oy tally sistemi. NV storage'da asset ve voter ranking'lerini tutar. Oy verildiğinde çift indeks güncellenir.

### Fonksiyonlar (Private, NV Storage)

| fn | Açıklama |
|----|---------|
| `get_total(state, epoch)` | Epoch'taki toplam oyu al |
| `put_total(state, epoch, total)` | Toplam oyu yaz |
| `get_asset_total(state, epoch, asset)` | Asset bazında oy toplamı |
| `put_asset_total(state, epoch, asset, total)` | Asset oy toplamı yaz |
| `put_ranked_asset(state, epoch, power, asset)` | Ranked set'e asset ekle |
| `delete_ranked_asset(state, epoch, power, asset)` | Eski güçteki asset'i sil |
| `get_voter_total(state, epoch, asset, voter, validator)` | Voter'ın mevcut oy gücü |
| `put_voter_total(state, epoch, asset, voter, total, validator)` | Voter oy gücünü yaz |
| `put_ranked_voter(state, epoch, asset, power, voter, validator, tx)` | Voter'ı ranked set'e ekle |
| `delete_ranked_voter(state, epoch, asset, power, voter, validator)` | Eski güçteki voter'ı sil |

### Trait Implementasyonları

#### `pub trait StateReadExt: StateRead + Sized` (Satır 132–174)

**`async fn total_votes(&self, epoch) -> u64`**

**`fn ranked_assets(&self, epoch) -> Pin<Box<dyn Stream<Item = (u64, asset::Id)>>>`**
- `nonverifiable_prefix_raw` ile prefix scan.
- Key parse: `state_key::lqt::v1::votes::by_asset::ranked::parse_key(&k)`.
- Stream: azalan power sırasında asset'ler (key inverted `!power`).

**`fn ranked_voters(&self, epoch, asset) -> Pin<Box<dyn Stream<...>>>`**
- `nonverifiable_prefix_proto` ile prefix scan.
- Key parse: `(power, Address, IdentityKey)` + value: `TransactionId`.
- Stream: azalan power sırasında voter'lar.

#### `fn add_power(total: u64, power: u64) -> u64` (Satır 176–178)
```rust
total.checked_add(power).expect("LQT voting power overflow")
```
- Taşma → panic (development fail-fast, production'da imkânsız sayılıyor).

#### `pub trait StateWriteExt: StateWrite + Sized` (Satır 181–215)

**`async fn tally(&mut self, epoch, asset, power, validator, voter)` (Satır 182–212):**

Algoritma — 3 adım:
```
1. Toplam güncelle:
   total += power

2. Asset index güncelle:
   old_asset_power = get_asset_total(epoch, asset)
   delete_ranked_asset(epoch, old_asset_power, asset)  // eski rank sil
   new_asset_power = old + power
   put_asset_total(epoch, asset, new_asset_power)
   put_ranked_asset(epoch, new_asset_power, asset)     // yeni rank ekle

3. Voter index güncelle:
   old_voter_power = get_voter_total(epoch, asset, voter, validator)
   delete_ranked_voter(epoch, asset, old_voter_power, voter, validator)  // eski rank sil
   new_voter_power = old + power
   put_voter_total(epoch, asset, voter, new_voter_power, validator)
   put_ranked_voter(epoch, asset, new_voter_power, voter, validator, tx)  // yeni rank ekle
```

### State Key İnversiyonu (KRİTİK)
State key'lerde `!power` (bit-invert) kullanılır:
```rust
bytes.extend_from_slice(&((!power).to_be_bytes()));
```
Parse'da tersine çevrilir:
```rust
let power = !u64::from_be_bytes(bytes_power.try_into()?);
```
**Neden:** NV storage lexicographic sıralar. En yüksek power = en düşük key (invert sonucu). Prefix scan direkt azalan sırada verir — `ranked_assets` ve `ranked_voters` stream'leri sort gerektirmez.

### Tasarım Kararları
1. **Çift indeks:** Hem `by_asset` hem `by_voter` indeksleri ayrı tutulur. Her oy 2 index günceller (O(1) per operation, O(N) storage).
2. **Bit-invert ranking:** Lexicographic sort ile descending order — sort algoritması gerekmez, O(log N) prefix scan.
3. **Delete-then-insert:** Ranking güncelleme atomik: önce eski rank sil, sonra yeni rank ekle. Yarı-güncelleme riski yok (aynı transaction içinde).

### Nova Shield Chain İçin Dersler
- **Bit-invert key trick:** Descending ranking için classic NV storage pattern. Nova Shield'in her sıralama gerektiren index'inde kullanılmalı.
- Çift indeks (by_asset + by_voter): Her iki perspektiften verimli sorgu imkânı.
- Ranked set update: Delete-then-insert pattern (eski rank sil, yeni rank ekle) — partial update riski yok.

---

## [component/metrics.rs] (funding)
**Dosya:** `.../funding/src/component/metrics.rs`
**Satır Sayısı:** 49

### Genel Amaç
Funding bileşeni metrik tanımları. `metrics` crate'ini re-export eder.

### Konstanlar

| Sabit | Değer | Açıklama |
|-------|-------|---------|
| `TOTAL_VALIDATOR_REWARDS` | `"penumbra_funding_total_validator_rewards_staking_token"` | Gauge |
| `VALIDATOR_FUNDING_VS_BUDGET_DIFFERENCE` | `"penumbra_funding_validator_vs_budget_difference_staking_token"` | Gauge |
| `FETCH_FUNDING_QUEUE_LATENCY` | `"penumbra_funding_fetch_funding_queue_latency_milliseconds"` | Histogram |
| `TOTAL_FUNDING_STREAMS_PROCESSING_TIME` | `"penumbra_funding_streams_total_processing_time_milliseconds"` | Histogram |

### Fonksiyonlar

**`pub fn register_metrics()`:**
- `describe_gauge!` ve `describe_histogram!` macro'ları ile her metriği kayıt eder.
- `Unit::Count`, `Unit::Milliseconds` ile birim bildirir.

### Tasarım Kararları
- Metrik adları `penumbra_funding_` prefix'i ile başlar — collision önleme.
- Hem gauge (anlık değer) hem histogram (dağılım) kullanımı — farklı analiz ihtiyaçları.

### Nova Shield Chain İçin Dersler
- Her bileşen için `metrics.rs` pattern. Prefix standardı: `nova_shield_{component}_{metric}_{unit}`.
- `register_metrics()` fonksiyonu uygulama başlangıcında çağrılmalı.
- Budget farkı gauge metric olarak tutulması: alerting için kullanılabilir (budget aşımında alarm).

---

## [component/rpc.rs] (funding)
**Dosya:** `.../funding/src/component/rpc.rs`
**Satır Sayısı:** 70

### Genel Amaç
Funding gRPC servisi. Tek endpoint: `lqt_check_nullifier` — kullanıcı bu epoch'ta oy kullanmış mı?

### Struct/Enum/Type Listesi

#### `pub struct Server` (Satır 9–11)
```rust
pub struct Server { storage: Storage }
```

### Trait Implementasyonları

#### `impl FundingService for Server` (Satır 20–70)

**`async fn lqt_check_nullifier(request) -> Result<LqtCheckNullifierResponse, Status>` (Satır 21–69):**
- `req_inner.epoch_index == 0` → current epoch kullan (default protobuf zero = "current")
- `state.get_lqt_spent_nullifier_by_epoch(nullifier, epoch_index).await`
- Response: `{ transaction: Option<TransactionId>, already_voted: bool, epoch_index: u64 }`

### Tasarım Kararları
- `epoch_index == 0` → current epoch trick: Proto3'te default sıfır, bu sıfırı "current epoch" olarak yorumla. Basit UX.

### Nova Shield Chain İçin Dersler
- Kullanıcı "bu oy kullanılmış mı?" diye sorabilmeli. Double-vote önleme yalnızca konsensüs seviyesinde değil, wallet'ta da kontrol edilmeli.
- Protobuf'taki sıfır default değerini anlamlı bir "current" olarak yorumlama: ergonomik API tasarımı.

---

## [component/staking_rewards.rs] (funding)
**Dosya:** `.../funding/src/component/staking_rewards.rs`
**Satır Sayısı:** 0

### Genel Amaç
Boş dosya — placeholder. Staking rewards logic henüz bu dosyaya taşınmamış; `component.rs` içinde inline duruyor. Gelecekte refactor için ayrılmış yer.

### Nova Shield Chain İçin Dersler
- Boş placeholder dosya oluşturmak modüler tasarım için iyi: gelecekte kodu buraya taşımak kolaylaşır.

---

## [component/state_key.rs] (funding)
**Dosya:** `.../funding/src/component/state_key.rs`
**Satır Sayısı:** 249

### Genel Amaç
Funding bileşeninin tüm JMT ve NV storage key şemalarını tanımlar. Hiyerarşik namespace.

### Fonksiyonlar

**`pub fn funding_parameters() -> &'static str`** → `"funding/parameters"`

**`pub mod lqt::v1::nullifier`:**
- `fn key(epoch_index, nullifier) -> String` → `"funding/lqt/v1/nullifier/{epoch:020}/lookup/{nullifier}"`

**`pub mod lqt::v1::votes::total`:**
- `fn key(epoch_index) -> Vec<u8>` → `PART0 + epoch20 + "/total/"`

**`pub mod lqt::v1::votes::by_asset::total`:**
- `fn key(epoch, asset) -> Vec<u8>` → `PART0 + epoch20 + "/by_asset/" + "total/" + asset_bytes`

**`pub mod lqt::v1::votes::by_asset::ranked`:**
- `fn prefix(epoch) -> [u8; EPOCH_PREFIX_LEN]` — sabit boyutlu array (stack allocated!)
- `fn key(epoch, power, asset) -> Vec<u8>` → prefix + `(!power).to_be_bytes()` + asset_bytes
- `fn parse_key(key) -> (u64, asset::Id)` → power = `!u64::from_be_bytes(...)`, asset parse

**`pub mod lqt::v1::votes::by_voter::total`:**
- `fn key(epoch, asset, addr, validator) -> Vec<u8>`

**`pub mod lqt::v1::votes::by_voter::ranked`:**
- `fn prefix(epoch, asset) -> [u8; EPOCH_ASSET_PREFIX_LEN]`
- `fn key(epoch, asset, power, addr, validator) -> Vec<u8>` → prefix + `(!power)` + addr_bytes + ik_bytes
- `fn parse_key(key) -> (u64, Address, IdentityKey)` → power, addr, ik parse

### Sabitler (State Key Boyutları)
```
EPOCH_LEN = 20       // 20 haneli sıfır dolgu epoch
ASSET_LEN = 32       // asset::Id byte boyutu
POWER_LEN = 8        // u64 big-endian = 8 byte
ADDRESS_LEN_BYTES    // Penumbra address boyutu
IDENTITY_KEY_LEN_BYTES // validator identity key boyutu
```

### Tasarım Kararları
1. **Stack-allocated prefix:** `[u8; N]` array ile heap tahsisatı yok. Sadece key'e append edilir.
2. **Compile-time KEY_LEN:** `const KEY_LEN: usize = ...` — runtime boyut hesabı yok, yanlış boyut compile'da yakalanır.
3. **`!power` inversiyonu:** By-asset ve by-voter ranked key'lerde aynı teknik. Descending lexicographic scan.
4. **`v1` namespace:** Key şemasında versiyon — `v2` ile breaking change mümkün.

### Nova Shield Chain İçin Dersler
- State key boyutlarını compile-time sabit olarak belirle: yanlış anahtar boyutu compile'da yakalanır.
- Stack-allocated prefix (`[u8; N]`) inner-loop key oluşturmada heap baskısını azaltır.
- Versiyon namespace (`lqt/v1/...`): State migration için kritik. Key şeması değişirse `v2` branch açılır.
- Parse/unparse fonksiyonları key ile birlikte tutulmalı — ayrı dosyada kaybolmaz.

---

## [component/view.rs] (funding)
**Dosya:** `.../funding/src/component/view.rs`
**Satır Sayısı:** 26

### Genel Amaç
Funding parametrelerini okuma/yazma extension trait'leri.

### Trait Implementasyonları

#### `pub trait StateReadExt: StateRead`
- `async fn get_funding_params(&self) -> Result<FundingParameters>`: JMT'den `funding_parameters()` key ile oku.

#### `pub trait StateWriteExt: StateWrite + StateReadExt`
- `fn put_funding_params(&mut self, params: FundingParameters)`: JMT'ye yaz.

### Tasarım Kararları
- `StateWriteExt: StateWrite + StateReadExt` — yazma trait'i okuma trait'ini miras alır. Yazarken okuyabilirsin.

---

## [event.rs] (funding)
**Dosya:** `.../funding/src/event.rs`
**Satır Sayısı:** 241

### Genel Amaç
Funding bileşeninin tüm event tiplerini tanımlar. 4 event struct.

### Struct/Enum/Type Listesi

#### `pub struct EventFundingStreamReward` (Satır 13–17)
```rust
pub struct EventFundingStreamReward {
    pub recipient: String,      // adres veya "community-pool"
    pub epoch_index: u64,
    pub reward_amount: Amount,
}
```

#### `pub struct EventLqtDelegatorReward` (Satır 53–65)
```rust
pub struct EventLqtDelegatorReward {
    pub epoch_index: u64,
    pub reward_amount: Amount,            // staking token cinsinden
    pub delegation_tokens: Value,         // mint edilen delegation token
    pub address: Address,                 // alıcı
    pub incentivized_asset_id: asset::Id, // hangi asset için oy verdi
}
```

#### `pub struct EventLqtPositionReward` (Satır 112–126)
```rust
pub struct EventLqtPositionReward {
    pub epoch_index: u64,
    pub reward_amount: Amount,
    pub position_id: position::Id,
    pub incentivized_asset_id: asset::Id,
    pub tournament_volume: Amount,   // toplam turnuva hacmi
    pub position_volume: Amount,     // bu LP'nin hacmi
}
```

#### `pub struct EventLqtVote` (Satır 178–191)
```rust
pub struct EventLqtVote {
    pub epoch_index: u64,
    pub voting_power: Amount,
    pub incentivized_asset_id: asset::Id,
    pub incentivized: Denom,
    pub rewards_recipient: Address,
    pub tx_id: TransactionId,
}
```

### Tasarım Kararları
- Her event için `TryFrom<pb::...>` ve `From<...> for pb::...` — bidirectional proto conversion.
- `EventLqtPositionReward` hem `tournament_volume` hem `position_volume` içerir — off-chain indexer LP'nin payını hesaplayabilir.

### Nova Shield Chain İçin Dersler
- LQT event'leri için **4 ayrı event tipi** yeterli. Nova Shield DEX incentive sistemi için aynen kullanılabilir.
- Event'lere `tournament_volume` ve `position_volume` eklemek off-chain hesaplama imkânı verir.

---

## [genesis.rs] (funding)
**Dosya:** `.../funding/src/genesis.rs`
**Satır Sayısı:** 45

### Struct/Enum/Type Listesi

#### `pub struct Content` (Satır 9–12)
```rust
pub struct Content {
    pub funding_params: FundingParameters,
}
```

#### `impl Default for Content` (Satır 39–45)
```rust
Self { funding_params: FundingParameters::default() }
```

---

## [lib.rs] (funding)
**Dosya:** `.../funding/src/lib.rs`
**Satır Sayısı:** 13

```rust
#![deny(clippy::unwrap_used)]
#[cfg(feature = "component")]
pub mod action_handler;
#[cfg(feature = "component")]
pub mod component;
pub mod event;
pub mod genesis;
pub mod liquidity_tournament;
pub mod params;
pub use params::FundingParameters;
```
- `action_handler` ve `component` feature-gated.
- `event`, `liquidity_tournament`, `params` her zaman mevcut (protocol level).

---

## [liquidity_tournament/action/mod.rs]
**Dosya:** `.../funding/src/liquidity_tournament/action/mod.rs`
**Satır Sayısı:** 168

### Genel Amaç
LQT oy action'ının domain model tipleri. `LiquidityTournamentVoteBody` ve `ActionLiquidityTournamentVote` struct'ları.

### Struct/Enum/Type Listesi

#### `pub struct LiquidityTournamentVoteBody` (Satır 19–38)
```rust
pub struct LiquidityTournamentVoteBody {
    pub incentivized: Denom,          // hangi asset (IBC denom)
    pub rewards_recipient: Address,   // ödül adresi
    pub start_position: Position,     // epoch başı pozisyonu (stateless check için)
    pub value: Value,                 // oy ağırlığı (delegation token)
    pub nullifier: Nullifier,         // double-vote önleme
    pub rk: VerificationKey<SpendAuth>, // spend auth için randomized key
}
```

**`fn incentivized_id(&self) -> Option<asset::Id>` (Satır 44–48):**
```rust
REGISTRY.parse_denom(&self.incentivized.denom).map(|x| x.id())
```
- Denom string → asset::Id dönüşümü. Bilinmeyen denom → None.

**`impl EffectingData`:** `effect_hash` = proto serialize → hash.

#### `pub struct ActionLiquidityTournamentVote` (Satır 110–117)
```rust
pub struct ActionLiquidityTournamentVote {
    pub body: LiquidityTournamentVoteBody,
    pub auth_sig: Signature<SpendAuth>,
    pub proof: LiquidityTournamentVoteProof,
}
```

**`fn balance_commitment(&self) -> balance::Commitment` (Satır 163–166):**
- `balance::Commitment::default()` — sıfır commitment.
- LQT oy action'ı değer üretmez veya tüketmez (sadece mevcut nota'yı "harcıyor").

### Sabitler

**`pub const LIQUIDITY_TOURNAMENT_VOTE_DENOM_MAX_BYTES: usize = 256`** (liquidity_tournament/mod.rs'de tanımlı)

### Tasarım Kararları
1. **Body + sig + proof:** Penumbra'nın standart action yapısı. Body = commit edilenler, sig = yetkilendirme, proof = ZK kanıtı.
2. **Sıfır balance commitment:** LQT oy, TX balance'ını etkilemez. Ayrı bir "spend" olarak modellenmiş.

---

## [liquidity_tournament/mod.rs]
**Dosya:** `.../funding/src/liquidity_tournament/mod.rs`
**Satır Sayısı:** 11

```rust
mod action;
mod plan;
mod view;
pub mod proof;
pub use action::{ActionLiquidityTournamentVote, LiquidityTournamentVoteBody};
pub use plan::ActionLiquidityTournamentVotePlan;
pub use view::ActionLiquidityTournamentVoteView;
pub const LIQUIDITY_TOURNAMENT_VOTE_DENOM_MAX_BYTES: usize = 256;
```

---

## [liquidity_tournament/plan/mod.rs]
**Dosya:** `.../funding/src/liquidity_tournament/plan/mod.rs`
**Satır Sayısı:** 204

### Genel Amaç
LQT oyu planlamak için `ActionLiquidityTournamentVotePlan`. ZK proof oluşturmak için gerekli private girdileri ve blinding faktörlerini içerir.

### Struct/Enum/Type Listesi

#### `pub struct ActionLiquidityTournamentVotePlan` (Satır 32–49)
```rust
pub struct ActionLiquidityTournamentVotePlan {
    pub incentivized: Denom,
    pub rewards_recipient: Address,
    pub staked_note: Note,                    // oy için kullanılan delegation nota
    pub staked_note_position: tct::Position,  // nota'nın TCT pozisyonu
    pub start_position: tct::Position,        // epoch başı pozisyonu
    pub randomizer: Fr,                       // spend auth randomizer
    pub proof_blinding_r: Fq,                 // ZK proof blinding faktörü 1
    pub proof_blinding_s: Fq,                 // ZK proof blinding faktörü 2
}
```

### Fonksiyonlar

**`pub fn new<R: CryptoRng + RngCore>(rng, incentivized, rewards_recipient, staked_note, staked_note_position, start_position) -> Self` (Satır 54–72):**
- `randomizer: Fr::rand(rng)` — decaf377 scalar
- `proof_blinding_r: Fq::rand(rng)` — BLS12-377 field element
- `proof_blinding_s: Fq::rand(rng)` — BLS12-377 field element
- CryptoRng ile güvenli random.

**`pub fn to_body(&self, fvk: &FullViewingKey) -> LiquidityTournamentVoteBody` (Satır 74–91):**
```rust
let commitment = self.staked_note.commit();
let nk = fvk.nullifier_key();
let nullifier = Nullifier::derive(nk, self.staked_note_position, &commitment);
let ak = fvk.spend_verification_key();
let rk = ak.randomize(&self.randomizer);
let value = self.staked_note.value();
```
- Nullifier: `Derive(nk, position, commitment)` — ZK circuit ile tutarlı.
- `rk = ak.randomize(randomizer)` — randomized spend verification key.

**`pub fn to_action(self, fvk, auth_sig, auth_path) -> ActionLiquidityTournamentVote` (Satır 98–140):**
1. commitment, nullifier, rk hesapla.
2. Public ve private ZK input'larını oluştur:
   ```rust
   let public = LiquidityTournamentVoteProofPublic {
       anchor: auth_path.root(),
       value,
       nullifier,
       rk,
       start_position: self.start_position,
   };
   let private = LiquidityTournamentVoteProofPrivate {
       state_commitment_proof: auth_path,
       note: self.staked_note.clone(),
       spend_auth_randomizer: self.randomizer,
       ak: *ak,
       nk: *nk,
   };
   ```
3. ZK proof oluştur:
   ```rust
   let proof = LiquidityTournamentVoteProof::prove(
       self.proof_blinding_r,
       self.proof_blinding_s,
       &DELEGATOR_VOTE_PROOF_PROVING_KEY,
       public,
       private,
   ).expect("can generate ZK LQT voting proof");
   ```

### Tasarım Kararları
1. **Plan pattern:** Action oluşturmak için tüm private veriler önce Plan'da toplanır. Plan → Action dönüşümü sırasında ZK proof üretilir. Wallet bu pattern'i kullanır.
2. **`expect` kullanımı:** ZK proof üretimi başarısız olursa panic — bu bir assertion hatası sayılır, production'da olmamalı.
3. **`DELEGATOR_VOTE_PROOF_PROVING_KEY` static:** Compile-time dahil edilen proving key.

### Nova Shield Chain İçin Dersler
- **Plan pattern:** Wallet-side işlemler için. Plan = "ne yapmak istiyorum", Action = "kanıtlanmış işlem". ZK proof'lar Plan'dan Action'a geçişte üretilir.
- ZK blinding faktörü çiftinin (`proof_blinding_r`, `proof_blinding_s`) ayrı ayrı serializasyonu — her biri `Fq` field element, 32 byte.

---

## [liquidity_tournament/proof/mod.rs]
**Dosya:** `.../funding/src/liquidity_tournament/proof/mod.rs`
**Satır Sayısı:** 123

### Genel Amaç
LQT oy ZK kanıtı. `DelegatorVoteProof` üzerine thin wrapper.

### Struct/Enum/Type Listesi

#### `pub struct LiquidityTournamentVoteProofPublic` (Satır 17–28)
```rust
pub struct LiquidityTournamentVoteProofPublic {
    pub anchor: tct::Root,           // TCT Merkle kökü
    pub value: Value,                // nota değeri
    pub nullifier: Nullifier,        // harcanan nota nullifier'ı
    pub rk: VerificationKey<SpendAuth>,
    pub start_position: tct::Position,
}
```

**`fn to_delegator_vote(self) -> DelegatorVoteProofPublic` (Satır 31–39):**
```rust
DelegatorVoteProofPublic {
    anchor: self.anchor,
    balance_commitment: self.value.commit(Default::default()), // v_blinding = 0
    nullifier: self.nullifier,
    rk: self.rk,
    start_position: self.start_position,
}
```
- `value.commit(Default::default())`: Value commitment, v_blinding = 0 (transparent amount).

#### `pub struct LiquidityTournamentVoteProofPrivate` (Satır 42–54)
```rust
pub struct LiquidityTournamentVoteProofPrivate {
    pub state_commitment_proof: tct::Proof,  // TCT inclusion proof
    pub note: Note,
    pub spend_auth_randomizer: Fr,
    pub ak: VerificationKey<SpendAuth>,
    pub nk: NullifierKey,
}
```

**`fn to_delegator_vote(self) -> DelegatorVoteProofPrivate` (Satır 57–67):**
- `v_blinding: Default::default()` — sıfır blinding (değer transparent).

#### `pub struct LiquidityTournamentVoteProof(DelegatorVoteProof)` (Satır 70)
- Newtype wrapper.

**`pub fn prove(blinding_r, blinding_s, pk, public, private) -> anyhow::Result<Self>` (Satır 76–91):**
- `DelegatorVoteProof::prove(...)` delege edilir.

**`pub fn verify(&self, vk, public) -> anyhow::Result<()>` (Satır 94–101):**
- `self.0.verify(vk, public.to_delegator_vote()?)` delege.

### Tasarım Kararları
1. **Delegator vote proof'u yeniden kullanma:** LQT oy için ayrı ZK circuit yok — governance delegatör oy circuit'i aynen kullanılıyor. Kod tekrarından kaçınılmış.
2. **`v_blinding = 0`:** LQT'de nota miktarı herkese açık (transparent). Governance'da da aynı durum. Gizlilik gerekmediğinde blinding sıfır.
3. **Groth16 üzerinde BLS12-377:** Penumbra'nın standard'ı. Proving key statik.

### Nova Shield Chain İçin Dersler
- Benzer semantik ZK kanıtları için ayrı circuit yerine wrapper pattern kullanılabilir. Kodu azaltır.
- `v_blinding = 0` ile transparent value commitment: ZK kanıtı içinde ama gizlilik gerektirmeyince blinding sıfır.

---

## [liquidity_tournament/view/mod.rs]
**Dosya:** `.../funding/src/liquidity_tournament/view/mod.rs`
**Satır Sayısı:** 92

### Genel Amaç
LQT oy action'ının görünürlük enum'u. Wallet'ın bu action'a bakış açısını temsil eder.

### Struct/Enum/Type Listesi

#### `pub enum ActionLiquidityTournamentVoteView` (Satır 14–22)
```rust
#[allow(clippy::large_enum_variant)]
pub enum ActionLiquidityTournamentVoteView {
    Visible {
        vote: ActionLiquidityTournamentVote,
        note: NoteView,   // kullanıcının görebileceği nota detayları
    },
    Opaque {
        vote: ActionLiquidityTournamentVote,  // sadece action, nota detayı yok
    },
}
```

**`impl From<ActionLiquidityTournamentVoteView> for ActionLiquidityTournamentVote` (Satır 84–91):**
- Her iki variant'tan da action çıkarılabilir.

### Tasarım Kararları
1. **`#[allow(clippy::large_enum_variant)]`:** `Visible` variant `NoteView` içerdiğinden büyük. Box'lama yerine allow — bu noktada kopyalama maliyeti kabul edilmiş.
2. **Visible/Opaque:** Full-node her iki variant'ı da görebilir, light client yalnızca Opaque.

### Nova Shield Chain İçin Dersler
- View enum pattern: Aynı action'ı farklı "gözlem noktaları" için farklı yapılarda sun. Full-node/wallet/explorer farklı view'lar kullanır.

---

## [params.rs] (funding)
**Dosya:** `.../funding/src/params.rs`
**Satır Sayısı:** 92

### Genel Amaç
Funding bileşeni runtime parametreleri. `LiquidityTournamentParameters` ve `FundingParameters` struct'ları.

### Struct/Enum/Type Listesi

#### `pub struct LiquidityTournamentParameters` (Satır 7–16)
```rust
pub struct LiquidityTournamentParameters {
    pub gauge_threshold: Percentage,  // min oy oranı (bu altı ödül alamaz)
    pub max_positions: u64,           // ödül alabilecek max LP sayısı
    pub max_delegators: u64,          // ödül alabilecek max delegatör sayısı
    pub delegator_share: Percentage,  // delegatörlere giden ödül yüzdesi
}
```

#### `impl Default for LiquidityTournamentParameters` (Satır 18–27)
```rust
Self {
    gauge_threshold: Percentage::from_percent(100),  // herkese gerek yok, sıfır ödül
    max_positions: 0,
    max_delegators: 0,
    delegator_share: Percentage::zero(),
}
```
- Default: hiçbir şey dağıtılmaz (LQT devre dışı). Governance ile aktifleştirilmeli.

#### `pub struct FundingParameters` (Satır 53–57)
```rust
pub struct FundingParameters {
    pub liquidity_tournament: LiquidityTournamentParameters,
}
```

**`impl TryFrom<pb::FundingParameters> for FundingParameters` (Satır 63–76):**
```rust
liquidity_tournament: proto
    .liquidity_tournament
    .map(LiquidityTournamentParameters::try_from)
    .transpose()?
    .unwrap_or_default(),  // eksik proto = default (upgrade uyumluluğu)
```
- Upgrade uyumluluğu: Eski chain state'inde `liquidity_tournament` alanı yoksa default parametreler.

### Tasarım Kararları
1. **Default = kapalı:** LQT default kapalı (max=0, threshold=100%). Governance aktifleştirir — güvenli varsayılan.
2. **`Percentage` tipi:** `u64` yerine tip güvenlikli yüzde. `from_percent`, `to_percent`, `complement()` metotları var.
3. **Upgrade-safe deserialization:** `unwrap_or_default()` — yeni alan eski state'te yoksa default.

### Nova Shield Chain İçin Dersler
- **LQT parametrelerini governance ile kontrol edilebilir yap.** Default = kapalı, governance = aktif. Bu güvenlik açısından zorunlu.
- `Percentage` tipi custom bir tip olarak implement et — `u64` veya `f64` yerine.
- Proto'daki eksik alanlar için `unwrap_or_default()` — chain upgrade'lerinde eski state bozulmaz.

---

## GENEL SENTEZ VE NOVA SHIELD CHAIN İÇİN EYLEM PLANI

### 1. Community Pool Mimarisi

**Doğrudan adapte edilebilecek:**
- `CommunityPoolDeposit` / `CommunityPoolSpend` / `CommunityPoolOutput` üçlüsü → Nova Shield Treasury bileşeni için şablon.
- `balance()` işaret mantığı: Deposit = negatif, Spend = pozitif, Output = negatif. TX balance dengesi bu şekilde garanti altına alınır.
- `community_pool_withdraw` → `checked_sub` + overdraft bail! → aynen kopyalanmalı.
- Sıfır bakiye silme pattern: `if remaining > 0 { put } else { delete }`.

**Eksik ve tamamlanması gereken:**
- `event.rs` TODO yorumu: Community Pool için spend/deposit event'leri henüz yok. Nova Shield'de bu TODO bırakılmamalı, event'ler implement edilmeli.

### 2. Liquidity Tournament Sistemi

**Kritik formüller (kaynak koddan doğrulanmış):**

Delegatör ödülü:
```
delegator_reward =
    initial_budget
    × delegator_share_percent / 100
    × (asset_votes_top_N / total_qualifying_votes)
    × (voter_votes / asset_total_top_N_votes)
```

LP ödülü:
```
lp_reward =
    initial_budget
    × (1 - delegator_share_percent / 100)
    × (asset_votes_top_N / total_qualifying_votes)
    × (position_volume_top_M / total_qualifying_volume)
```

**Parametreler:**
- `gauge_threshold`: Asset minimum oy oranı (örn. %5 → toplam oyun %5'ini almayan asset ödülden dışlanır).
- `max_delegators`: Top-N voter kısıtlaması (hesaplama sınırı).
- `max_positions`: Top-M LP kısıtlaması (hesaplama sınırı).
- `delegator_share`: Delegatör vs LP arasında bölüşüm.

**State key tasarımı:**
- `funding/lqt/v1/nullifier/{epoch:020}/lookup/{nullifier}` — NV storage
- `funding/lqt/v1/votes/...` — bit-inverted ranked keys
- `!power` inversion → descending lexicographic sort → O(N) prefix scan

**ZK kanıt sistemi:**
- `LiquidityTournamentVoteProof` = `DelegatorVoteProof` wrapper (Groth16/BLS12-377)
- Public: anchor, value, nullifier, rk, start_position
- Private: TCT inclusion proof, note, spend_auth_randomizer, ak, nk
- `v_blinding = 0` → transparent amount (not hiding)

### 3. Nova Shield DEX İçin Direkt Eylem Planı

1. **LQT sistemi adapt et:** `gauge_threshold` + `max_positions` + `max_delegators` + `delegator_share` parametreleri governance ile kontrol edilebilir hale getir.

2. **Bit-invert ranked key pattern:** Her sıralama ihtiyacında `(!power).to_be_bytes()` → lexicographic descending.

3. **ZK-based vote:** Delegation token ile IBC asset'leri incentivize etmek yerine, Nova Shield'de native DEX pair'lerini incentivize etmek için adapte et. `is_valid_denom` → kendi pair filtren.

4. **Budget carry-forward:** Epoch sonunda kullanılmayan budget bir sonraki epoch'a taşı. Yakma yok.

5. **Bank trait:** `reward_to_voter` (delegation token mint) + `reward_to_position` (rezerv artış) ayrımını koru.

6. **Epoch-scoped nullifier:** Her epoch için ayrı nullifier space. Aynı delegation token her epoch'ta bir oy kullanabilir.

7. **Validator state filtresi:** `Defined | Tombstoned` → oy geçersiz. Nova Shield validatör sistemi için aynı durum.

8. **`unwrap` yasağı:** `#![deny(clippy::unwrap_used)]` tüm crate'lerde.

9. **Feature flags:** `component` feature ile full-node/light-client ayrımı.

10. **Metrics:** Her epoch için `VALIDATOR_FUNDING_VS_BUDGET_DIFFERENCE` gauge, işlem süreleri histogram.
