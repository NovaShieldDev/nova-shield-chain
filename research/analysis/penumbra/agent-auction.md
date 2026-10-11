# Penumbra Auction Bileşeni — Tam Analiz

**Analiz Tarihi:** 2026-07-22
**Kapsam:** `crates/core/component/auction/src/` — Tüm 27 dosya
**Toplam Satır:** ~900 satır Rust kodu
**Analist:** Agent-auction

---

## GENEL BAKIS

Penumbra'nın auction bileşeni, kullanıcıların **Dutch Auction** (Hollanda Açık Artırması) mekanizmasıyla token satışı yapmasını sağlar. Sistem şu temel prensipler üzerine inşa edilmiştir:

1. **NFT tabanlı sahiplik:** Her auction bir NFT tarafından temsil edilir. Auction'ın lifecycle'ı NFT'nin sequence numarasıyla takip edilir.
2. **DEX entegrasyonu:** Auction her adımda bir LP (Liquidity Position) açar. Zaman ilerledikçe LP kapatılır, fiyat düşürülür, yeni LP açılır.
3. **Value Circuit Breaker (VCB):** Auction component'inin tüm giriş/çıkış değerlerini takip eden bir hesap denetim mekanizması.
4. **Trigger sistemi:** Auction execution'ları belirli blok yüksekliklerinde tetiklenir (end_block içinde).
5. **close_on_fill:** LP pozisyonu doldurulduğunda otomatik kapanır — round-trip saldırısını engeller.

---

## DOSYA ANALİZLERİ

---

## [lib.rs]
**Dosya:** `/crates/core/component/auction/src/lib.rs`
**Satır Sayısı:** 15

### Genel Amaç
Crate'in kök modülü. Ana modülleri expose eder: `auction`, `event`, `genesis`, `params`, `state_key`. `component` modülü sadece `"component"` feature flag aktifken derlenir.

### Önemli Özellikler
- `#![cfg_attr(docsrs, feature(doc_auto_cfg))]` — docs.rs desteği
- `#![deny(clippy::unwrap_used)]` — Tüm `.unwrap()` kullanımları derleme hatası verir. Bu, production kalitesini zorlar.
- `StateReadExt` ve `StateWriteExt` sadece component feature ile re-export edilir.

### Tasarım Kararları
- Feature-gated component modülü: client-side kütüphaneler (wallet, view server) component'i derlemeden auction tiplerini kullanabilir. Bu, binary boyutunu küçük tutar.

### Nova Shield Chain İçin Dersler
- `#![deny(clippy::unwrap_used)]` kuralı Nova Shield'e de uygulanmalıdır. Production kodunda `.unwrap()` yasak olmalı.
- Feature flag ile component/non-component ayrımı, light client ile full node'un aynı type crate'ini paylaşmasını sağlar.

---

## [auction.rs]
**Dosya:** `/crates/core/component/auction/src/auction.rs`
**Satır Sayısı:** 7

### Genel Amaç
`auction` modülünün index dosyası. `dutch`, `id`, `nft` alt modüllerini declare eder. `AuctionId` ve `AuctionNft` tipler re-export edilir.

### Tasarım Kararları
- Modül yapısı minimalist: her auction tipi kendi sub-modülünde. Gelecekte `english`, `sealed_bid` gibi yeni auction tipleri eklenebilir.

---

## [auction/id.rs]
**Dosya:** `/crates/core/component/auction/src/auction/id.rs`
**Satır Sayısı:** 70

### Genel Amaç
`AuctionId` tipini tanımlar: 32-byte benzersiz auction kimliği.

### Struct/Enum/Type Listesi

#### `AuctionId([u8; 32])`
- `PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Serialize, Deserialize`
- Newtype wrapper: iç değer `pub [u8; 32]`
- Bech32m encoding ile string gösterimi: prefix `"pauctid1"`

### Fonksiyonlar

#### `FromStr for AuctionId` (Satır 14–22)
- Parametreler: `s: &str`
- Bech32m ile decode eder, ardından protobuf'a dönüştürüp `try_into()` yapar.
- Yorum: `// IMPORTANT: changing this is state-breaking.` — state key değişikliğinin anlamını belgelemiş.

#### `Display for AuctionId` (Satır 30–38)
- Bech32m ile encode eder.
- Yorum: `// IMPORTANT: changing this is state-breaking.`

#### `TryFrom<pb::AuctionId>` (Satır 54–68)
- Boş `inner` alanını reddeder: `bail!("AuctionId proto message is empty")`
- 32 byte uzunluğunu doğrular: `"raw AuctionId must be 32 bytes"`

### Matematiksel Formüller
Yok (sadece encoding/decoding).

### Unsafe Bloklar
Yok.

### Tasarım Kararları
- `Ord` implement edilmiş: AuctionId'ler sıralanabilir, BTreeMap key olarak kullanılabilir.
- Bech32m (Bitcoin Taproot ile aynı encoding) — hata tespiti güçlü.
- "state-breaking" yorumlar: encoding değişikliği chain state'i bozar, bu yorum kritik bir uyarıdır.

### Nova Shield Chain İçin Dersler
- Nova Shield'in ID tipleri de Bech32m encoding kullanmalı. Human-readable, hata-düzeltici.
- "state-breaking" yorumları eklemek iyi bir pratik. Kod review'larında bu yorumlar refactoring'i önler.
- 32-byte ID, BLAKE2b hash çıktısıyla uyumlu. Nova Shield kendi ID'lerini aynı şekilde yapılandırabilir.

---

## [auction/nft.rs]
**Dosya:** `/crates/core/component/auction/src/auction/nft.rs`
**Satır Sayısı:** 93

### Genel Amaç
`AuctionNft` tipini tanımlar. Auction'ın sahipliğini ve durumunu temsil eden NFT. Denom formatı: `auctionnft_{seq}_{auction_id}`.

### Struct/Enum/Type Listesi

#### `AuctionNft`
- `id: AuctionId` — auction'ın benzersiz kimliği
- `seq: u64` — auction'ın sequence numarası (state)
- `metadata: asset::Metadata` — NFT'nin asset metadata'sı

### Fonksiyonlar

#### `AuctionNft::new(id: AuctionId, seq: u64) -> AuctionNft` (Satır 20–25)
- Denom formatı: `"auctionnft_{seq}_{id}"` — örnek: `auctionnft_0_pauctid1abc...`
- `asset::REGISTRY.parse_denom(...)` ile global registry'den metadata alır.
- `.expect("auction nft denom is valid")` kullanılmış — bu bir invariant çünkü format her zaman geçerli olacak şekilde tasarlanmış.

#### `AuctionNft::asset_id(&self) -> asset::Id` (Satır 27–29)
- Metadata'dan asset ID döndürür.

#### `TryFrom<Metadata> for AuctionNft` (Satır 59–92)
- Regex ile denom parse eder: `^auctionnft_(?P<seq_num>[0-9]+)_(?P<auction_id>pauctid1[a-zA-HJ-NP-Z0-9]+)$`
- Base58 Bech32m alfabe kullanılır: `[a-zA-HJ-NP-Z0-9]` (I ve O harfleri eksik — karışıklık önleme)
- `seq_num` u64'e parse edilir, `auction_id` Bech32m ile decode edilir.

### Matematiksel Formüller
Yok.

### Unsafe Bloklar
Yok.

### Tasarım Kararları
1. **NFT sequence = auction state:** Her durum geçişi (opened → closed → withdrawn) farklı bir NFT yaratır. Sequence 0 = opened, 1 = closed, 2+ = withdrawn. Bu, sahiplik değişikliklerini atomik ve takip edilebilir yapar.
2. **Regex parsing:** Denom format doğrulaması regex ile yapılır. Production kodu için regex derleme maliyeti `Regex::new(...)` her çağrıda gerçekleşir — `once_cell::sync::Lazy` veya `std::sync::OnceLock` ile lazy init daha verimli olurdu.
3. **Global REGISTRY:** Penumbra tüm asset metadata'sını global bir registry'de tutar.

### Nova Shield Chain İçin Dersler
- NFT-tabanlı auction sahipliği çok güçlü bir pattern. Nova Shield DEX'te position NFT'leri için aynı pattern kullanılabilir.
- Regex'i `OnceLock` ile lazy initialize etmek daha iyi performans verir.
- `auctionnft_{seq}_{id}` formatı basit ama güçlü. Nova Shield için `nsdexpos_{seq}_{id}` benzeri bir format kullanılabilir.

---

## [auction/dutch.rs]
**Dosya:** `/crates/core/component/auction/src/auction/dutch.rs`
**Satır Sayısı:** 215

### Genel Amaç
Dutch auction'ın iki ana veri yapısını tanımlar:
1. `DutchAuction` — tam auction durumu (description + state)
2. `DutchAuctionDescription` — değişmez auction parametreleri
3. `DutchAuctionState` — değişen auction durumu

### Struct/Enum/Type Listesi

#### `DutchAuction` (Satır 21–24)
```rust
pub struct DutchAuction {
    pub description: DutchAuctionDescription,
    pub state: DutchAuctionState,
}
```
- `Clone, Serialize, Deserialize, PartialEq, Eq, Debug`
- Protobuf: `pb::DutchAuction`

#### `DutchAuctionDescription` (Satır 64–73)
```rust
pub struct DutchAuctionDescription {
    pub input: Value,          // satılacak token ve miktar
    pub output_id: asset::Id,  // alınmak istenen token
    pub max_output: Amount,    // başlangıç fiyatı (max beklenti)
    pub min_output: Amount,    // bitiş fiyatı (min beklenti)
    pub start_height: u64,     // auction başlangıç bloğu
    pub end_height: u64,       // auction bitiş bloğu
    pub step_count: u64,       // kaç adımda tamamlanacak (2..=255)
    pub nonce: [u8; 32],       // benzersizlik için rastgele değer
}
```

#### `DutchAuctionState` (Satır 170–176)
```rust
pub struct DutchAuctionState {
    pub sequence: u64,                        // 0=opened, 1=closed, 2+=withdrawn
    pub current_position: Option<position::Id>, // aktif LP pozisyonu
    pub next_trigger: Option<NonZeroU64>,     // sonraki tetiklenme yüksekliği
    pub input_reserves: Amount,               // kalan input token miktarı
    pub output_reserves: Amount,              // biriken output token miktarı
}
```

**State machine diyagramı (koddan — Satır 160–167):**
```
┌───┐            ┌───┐             ┌───┐
│ 0 │───Closed──▶│ 1 │──Withdrawn─▶│ 2 │
└───┘            └───┘             └───┘
  ▲
  │
Opened
```

**State semantiği:**
- `sequence = 0`: Auction açık (running)
- `sequence = 1`: Auction kapatılmış (closed — user tarafından veya otomatik expire/fill)
- `sequence >= 2`: Auction çekilmiş (withdrawn) — her withdraw sequence'ı bir artırır

**Neden untyped u64?** Satır 152–156'da belgelenmiş: "This futureproof support for auction types that have a richer state machine e.g. allows claiming a withdrawn auction multiple times, burning and minting a new withdrawn auction with an incremented sequence number."

### Fonksiyonlar

#### `DutchAuctionDescription::id(&self) -> AuctionId` (Satır 77–94)
```rust
pub fn id(&self) -> AuctionId {
    let mut state = blake2b_simd::Params::default()
        .personal(DUTCH_AUCTION_DOMAIN_SEP)  // b"penumbra_DA_nft"
        .to_state();

    state.update(&self.nonce);
    state.update(&self.input.asset_id.to_bytes());
    state.update(&self.input.amount.to_le_bytes());
    state.update(&self.max_output.to_le_bytes());
    state.update(&self.start_height.to_le_bytes());
    state.update(&self.end_height.to_le_bytes());
    state.update(&self.step_count.to_le_bytes());

    let hash = state.finalize();
    let mut bytes = [0; 32];
    bytes[0..32].copy_from_slice(&hash.as_bytes()[0..32]);
    AuctionId(bytes)
}
```

**Not:** `min_output` hash'e dahil edilmemiş! Bu kasıtlı bir tasarım kararı mı yoksa bir hata mı belirsiz. `output_id` de hash'e dahil edilmemiş.

### Matematiksel Formüller

**AuctionId hash:**
```
AuctionId = BLAKE2b(
    personal = "penumbra_DA_nft",
    nonce || input.asset_id || input.amount || max_output ||
    start_height || end_height || step_count
)
```

**Dikkat:** `min_output` ve `output_id` hash'e dahil değil.

### Unsafe Bloklar
Yok.

### Tasarım Kararları
1. **Değişmez description + değişken state ayrımı:** Description hash'lenir ve ID'ye dönüştürülür. State değişir ama description değişmez. Bu, integrity kontrolü yapmayı kolaylaştırır.
2. **NonZeroU64 için next_trigger:** `next_trigger = None` durumu "trigger yok" anlamına gelir. `NonZeroU64` ile protobuf'ta `0` değeri `None` olarak yorumlanır — clever encoding.
3. **`output_id` hash'te yok:** Bu potansiyel bir güvenlik açığı veya kasıtlı tasarım. Farklı output token'larına sahip iki auction aynı ID'ye sahip olabilir (nonce + diğer parametreler aynıysa).

### Nova Shield Chain İçin Dersler
- Description/State ayrımı çok temiz bir pattern. Nova Shield DEX pozisyonları için: `PositionDescription` (değişmez) + `PositionState` (değişken).
- `Option<NonZeroU64>` ile sentinel value encoding: protobuf'ta 0 = None, >0 = Some. Bu, proto optional field olmadan opsiyonel değer temsil etmenin akıllıca yolu.
- **UYARI:** AuctionId hash'inde `min_output` ve `output_id` eksik. Nova Shield ID hesaplamalarında TÜM parametreler hash'e dahil edilmeli.

---

## [auction/dutch/actions/mod.rs]
**Dosya:** `/crates/core/component/auction/src/auction/dutch/actions/mod.rs`
**Satır Sayısı:** 13

### Genel Amaç
Actions modülünün index'i. 4 action tipini declare ve re-export eder:
- `ActionDutchAuctionSchedule` — auction başlatma
- `ActionDutchAuctionEnd` — auction kapatma (user)
- `ActionDutchAuctionWithdraw` — auction çekme
- `ActionDutchAuctionWithdrawPlan` — withdraw planı (planlama aşaması)
- `view` sub-modülü (re-export yok, sadece declare)

---

## [auction/dutch/actions/schedule.rs]
**Dosya:** `/crates/core/component/auction/src/auction/dutch/actions/schedule.rs`
**Satır Sayısı:** 79

### Genel Amaç
`ActionDutchAuctionSchedule` action'ını tanımlar. Bir auction başlatma transaction action'ı.

### Struct/Enum/Type Listesi

#### `MAX_AUCTION_AMOUNT_RESERVES: u128 = (1 << 52) - 1` (Satır 10)
**Kritik sabit:** `4_503_599_627_370_495` — ~4.5 quadrilyon.

**Neden 52 bit?** Satır 9: "52 bits gives us enough headroom to do infallible price interpolation." Fiyat interpolasyonu sırasında çarpma işlemlerinin taşmaması için bu limit konulmuştur.

#### `ActionDutchAuctionSchedule` (Satır 17–19)
```rust
pub struct ActionDutchAuctionSchedule {
    pub description: DutchAuctionDescription,
}
```

### Fonksiyonlar

#### `balance(&self) -> Balance` (Satır 31–42)
Auction başlatma işleminin değer dengesi:
```
┌────────────────────┬──────────────────────┐
│      Burn (-)      │       Mint (+)       │
├────────────────────┼──────────────────────┤
│    input value     │  opened auction nft  │
└────────────────────┴──────────────────────┘
```
- Kullanıcı `input` token'ını yakar (auction component'e gönderir)
- Karşılığında `seq=0` auction NFT'si alır
- NFT amount: `1u128`

#### `effect_hash(&self) -> EffectHash` (Satır 46–49)
- Protobuf encoding üzerinden hash hesaplar.

### Matematiksel Formüller

**Value balance:**
```
balance = +AuctionNft(id, seq=0) - input_value
```

### Unsafe Bloklar
Yok.

### Tasarım Kararları
- **52-bit limit:** Çarpma işlemlerinde u128 overflow riskini önlemek için. `input * step_count` gibi işlemler 52+8=60 bit'ten küçük kalır (step_count max 255 = 8 bit).
- **NFT mint = balance proof:** Transaction doğrulayıcı, balance sıfır olmak zorunda (NFT mint + input burn = net zero). Bu, ZK proof ile doğrulanabilir.

### Nova Shield Chain İçin Dersler
- 52-bit limit gibi overflow korumaları Nova Shield matematiksel işlemlerinde de uygulanmalı.
- NFT tabanlı sahiplik kanıtı: transaction action'ı value balance hesaplar, ZK proof doğrular. Bu pattern Nova Shield auction veya liquidity position sahipliği için kullanılabilir.

---

## [auction/dutch/actions/end.rs]
**Dosya:** `/crates/core/component/auction/src/auction/dutch/actions/end.rs`
**Satır Sayısı:** 74

### Genel Amaç
`ActionDutchAuctionEnd` action'ını tanımlar. Kullanıcının bir açık auction'ı (seq=0 veya seq=1) kapatması.

### Struct/Enum/Type Listesi

#### `ActionDutchAuctionEnd` (Satır 14–16)
```rust
pub struct ActionDutchAuctionEnd {
    pub auction_id: AuctionId,
}
```

### Fonksiyonlar

#### `balance(&self) -> Balance` (Satır 28–40)
```
┌────────────────────┬──────────────────────┐
│      Burn (-)      │       Mint (+)       │
├────────────────────┼──────────────────────┤
│ opened auction nft │  closed auction nft  │
└────────────────────┴──────────────────────┘
```
- seq=0 NFT yakılır
- seq=1 NFT mint edilir
- Value akışı: `Balance::from(end_auction) - Balance::from(start_auction)`

### Matematiksel Formüller

**Value balance:**
```
balance = +AuctionNft(id, seq=1) - AuctionNft(id, seq=0)
```

### Tasarım Kararları
- Sadece `auction_id` taşır. Minimum bilgi: chain state'den gerisini çeker.
- NFT swap ile atomik state geçişi: seq=0 yakılır, seq=1 mint edilir. Bu işlem atomik olduğu için double-spend imkansız.

### Nova Shield Chain İçin Dersler
- Action payload'larının minimal olması iyi bir pratik. Gereksiz veri taşımak transaction boyutunu artırır ve ZK proof maliyetini yükseltir.

---

## [auction/dutch/actions/withdraw.rs]
**Dosya:** `/crates/core/component/auction/src/auction/dutch/actions/withdraw.rs`
**Satır Sayısı:** 101

### Genel Amaç
`ActionDutchAuctionWithdraw` action'ını tanımlar. Kullanıcı kapalı (seq>=1) bir auction'ın rezervlerini çeker.

### Struct/Enum/Type Listesi

#### `ActionDutchAuctionWithdraw` (Satır 15–19)
```rust
pub struct ActionDutchAuctionWithdraw {
    pub auction_id: AuctionId,
    pub seq: u64,                          // beklenen yeni sequence numarası
    pub reserves_commitment: balance::Commitment,  // ZK commitment
}
```

### Fonksiyonlar

#### `balance_commitment(&self) -> balance::Commitment` (Satır 39–56)
**Kritik fonksiyon:** ZK balance commitment hesaplar.

```rust
pub fn balance_commitment(&self) -> balance::Commitment {
    let prev_auction_nft = Balance::from(Value {
        amount: 1u128.into(),
        asset_id: AuctionNft::new(self.auction_id, self.seq.saturating_sub(1)).asset_id(),
    })
    .commit(Fr::zero());  // blinding factor = 0 (açık commitment)

    let next_auction_nft = Balance::from(Value {
        amount: 1u128.into(),
        asset_id: AuctionNft::new(self.auction_id, self.seq).asset_id(),
    })
    .commit(Fr::zero());

    self.reserves_commitment + next_auction_nft - prev_auction_nft
}
```

**Matematiksel formül:**
```
balance_commitment = reserves_commitment + Commit(AuctionNft(id, seq)) - Commit(AuctionNft(id, seq-1))
```

**Blinding factor = Fr::zero():** NFT değerleri açık bilgidir, gizlenmelerine gerek yoktur. Sadece `reserves_commitment` gizlidir.

### Matematiksel Formüller

**Withdraw balance commitment:**
```
C_total = C_reserves + [+AuctionNft(id, seq)] + [-AuctionNft(id, seq-1)]
```

**Neden commitment?** Withdraw action'ı ZK-aware. Kullanıcı rezervlerin miktarını gizleyebilir. `reserves_commitment` Pedersen commitment'ı.

### Unsafe Bloklar
Yok.

### Tasarım Kararları
- `saturating_sub(1)`: seq=0 underflow güvenliği. Ama zaten check_stateless seq>=1 gerektirir.
- `Fr::zero()` blinding factor: NFT'ler açık, gizleme gerekmez.
- `reserves_commitment`: Kullanıcı rezervlerin tam miktarını açıklamak zorunda değil — ZK proof ile doğrulama yapılır.

### Nova Shield Chain İçin Dersler
- ZK-aware balance tracking: gizli miktarlar için Pedersen commitment, açık miktarlar için blinding_factor=0. Nova Shield ZK işlemlerinde aynı pattern uygulanabilir.
- `saturating_sub` kullanımı: underflow güvenliği için iyi pratik, özellikle sequence numarası manipülasyonlarında.

---

## [auction/dutch/actions/plan.rs]
**Dosya:** `/crates/core/component/auction/src/auction/dutch/actions/plan.rs`
**Satır Sayısı:** 100

### Genel Amaç
`ActionDutchAuctionWithdrawPlan` tipini tanımlar. Client-side planlama aşaması: on-chain commitment gönderilmeden önce client rezervleri hesaplar.

### Struct/Enum/Type Listesi

#### `ActionDutchAuctionWithdrawPlan` (Satır 14–19)
```rust
pub struct ActionDutchAuctionWithdrawPlan {
    pub auction_id: AuctionId,
    pub seq: u64,
    pub reserves_input: Value,   // gerçek miktar (istemci bilir)
    pub reserves_output: Value,  // gerçek miktar (istemci bilir)
}
```

### Fonksiyonlar

#### `to_action(&self) -> ActionDutchAuctionWithdraw` (Satır 22–28)
- Plan'dan gerçek on-chain action'ı oluşturur.
- `reserves_commitment()` hesaplanır ve action'a eklenir.

#### `reserves_balance(&self) -> Balance` (Satır 30–32)
```rust
pub fn reserves_balance(&self) -> Balance {
    Balance::from(self.reserves_input) + Balance::from(self.reserves_output)
}
```

#### `reserves_commitment(&self) -> balance::Commitment` (Satır 34–36)
```rust
pub fn reserves_commitment(&self) -> balance::Commitment {
    self.reserves_balance().commit(Fr::zero())
}
```

#### `balance(&self) -> Balance` (Satır 38–51)
```
balance = reserves_input + reserves_output + AuctionNft(seq) - AuctionNft(seq-1)
```

### Matematiksel Formüller

**Planlama aşaması balance:**
```
balance = reserves_input + reserves_output + AuctionNft(id, seq) - AuctionNft(id, seq-1)
```

**Bu, on-chain commitment'la eşleşmeli:**
```
reserves_commitment = commit(reserves_input + reserves_output, blinding=0)
```

### Tasarım Kararları
- Plan tipi, action tipinden ayrıdır: client-side hesaplama ile on-chain commitment'ı ayrıştırır. Plan detayları chain'e gitmez, sadece commitment gider.
- `blinding_factor = Fr::zero()`: Gizlilik yok, sadece integrity. Gerçek ZK gizliliği için `Fr::random()` kullanılırdı.

### Nova Shield Chain İçin Dersler
- Plan/Action ayrımı: client hesaplama ve on-chain veri ayrı tutulur. Nova Shield transaction building için aynı pattern: `XxxPlan` → `XxxAction`.
- Blinding factor seçimi açıkça belgelenmeli: Ne zaman sıfır, ne zaman rastgele.

---

## [auction/dutch/actions/view.rs]
**Dosya:** `/crates/core/component/auction/src/auction/dutch/actions/view.rs`
**Satır Sayısı:** 132

### Genel Amaç
Wallet/view server için zenginleştirilmiş action görünümleri tanımlar. Chain'e gönderilmez, sadece UI katmanında kullanılır.

### Struct/Enum/Type Listesi

#### `ActionDutchAuctionScheduleView` (Satır 19–24)
```rust
pub struct ActionDutchAuctionScheduleView {
    pub action: ActionDutchAuctionSchedule,
    pub auction_id: AuctionId,
    pub input_metadata: Option<Metadata>,   // token bilgisi (opsiyonel)
    pub output_metadata: Option<Metadata>,  // token bilgisi (opsiyonel)
}
```

#### `ActionDutchAuctionWithdrawView` (Satır 26–35)
```rust
pub struct ActionDutchAuctionWithdrawView {
    pub action: ActionDutchAuctionWithdraw,
    pub reserves: Vec<ValueView>,  // commitment'ı oluşturan değerlerin listesi
}
```

### Fonksiyonlar

#### `From<ActionDutchAuctionScheduleView> for ActionDutchAuctionSchedule` (Satır 40–43)
View → Action dönüşümü. `value.action` döndürür.

#### `From<ActionDutchAuctionWithdrawView> for ActionDutchAuctionWithdraw` (Satır 46–49)
Aynı şekilde.

### Tasarım Kararları
- View tipleri, raw action'ları extend eder. İstemci bu tipleri UI gösterimi için kullanır.
- `Option<Metadata>`: Token metadata bulunamazsa None. Bilinmeyen asset desteği.
- `Vec<ValueView>`: Commitment'ı oluşturan değerlerin listesi — ZK'da commitment = sum of values.

### Nova Shield Chain İçin Dersler
- View tipi pattern: on-chain minimal data + off-chain enriched view. Nova Shield wallet client'ı için kullanılabilir.
- `Option<Metadata>` ile graceful degradation: bilinmeyen asset ID'leri hata vermez, None döner.

---

## [params.rs]
**Dosya:** `/crates/core/component/auction/src/params.rs`
**Satır Sayısı:** 31

### Genel Amaç
`AuctionParameters` struct'ını tanımlar. Şu anda boş — gelecekte parametreler eklenecek.

### Struct/Enum/Type Listesi

#### `AuctionParameters` (Satır 7)
```rust
pub struct AuctionParameters {}
```
- Boş struct. `Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default`
- Protobuf: `pb::AuctionParameters` (de boş)

### Tasarım Kararları
- Boş parametre struct'ı: "Şu an parametre yok ama gelecekte ekleyeceğiz" mesajı. Protobuf backwards compatibility için iyi pratik.
- Gelecekte eklenebilecek parametreler: max_step_count, max_auction_duration, fee_bps, vb.

### Nova Shield Chain İçin Dersler
- Bileşen parametrelerinizi baştan struct'a alın, boş bırakın. Governance parametrelerini eklemek için zemin hazır olur.

---

## [genesis.rs]
**Dosya:** `/crates/core/component/auction/src/genesis.rs`
**Satır Sayısı:** 37

### Genel Amaç
Auction bileşeninin genesis (başlangıç) durumunu tanımlar.

### Struct/Enum/Type Listesi

#### `Content` (Satır 9–12)
```rust
pub struct Content {
    pub auction_params: AuctionParameters,
}
```
- `Deserialize, Serialize, Debug, Clone, Default`
- Protobuf: `pb::GenesisContent`
- Default: boş params ile

### Tasarım Kararları
- Genesis content sadece params içerir. Başlangıç auction'ları yoktur — mantıklı.

### Nova Shield Chain İçin Dersler
- Genesis config ayrı struct'ta tutulmalı. Nova Shield genesis.json yapısı için model.

---

## [state_key.rs]
**Dosya:** `/crates/core/component/auction/src/state_key.rs`
**Satır Sayısı:** 56

### Genel Amaç
Chain state'teki tüm key path'lerini tanımlar. Bu, state'e erişimin tek noktası.

### Modül/Fonksiyon Listesi

#### `parameters::key() -> &'static str` (Satır 2–4)
```
"auction/parameters"
```

#### `parameters::updated_flag() -> &'static str` (Satır 6–8)
```
"auction/parameters/updated"
```

#### `value_balance::prefix() -> &'static str` (Satır 15–17)
```
"auction/value_breaker/"
```

#### `value_balance::for_asset(asset_id: &asset::Id) -> String` (Satır 19–22)
```
"auction/value_breaker/{asset_id}"
```

#### `auction_store::prefix() -> &'static str` (Satır 28–30)
```
"auction/auction_store/"
```

#### `auction_store::by_id(auction_id: AuctionId) -> String` (Satır 32–34)
```
"auction/auction_store/{auction_id}"
```

#### `dutch::trigger::prefix() -> &'static str` (Satır 41–43)
```
"auction/dutch/trigger/"
```

#### `dutch::trigger::by_height(trigger_height: u64) -> String` (Satır 45–47)
```
"auction/dutch/trigger/{height:020}/"
```
**Önemli:** `{trigger_height:020}` — 20 karakter, sıfır dolgulu. Bu, lexicographic sıralama = numerik sıralama garantisi verir.

#### `dutch::trigger::auction_at_height(auction_id: AuctionId, trigger_height: u64) -> String` (Satır 49–51)
```
"auction/dutch/trigger/{height:020}/{auction_id}"
```

### Tasarım Kararları
1. **Namespace hierarchy:** `auction/dutch/trigger/` → prefix query ile aynı height'taki tüm auction'ları bulabilir.
2. **Zero-padded height:** `:020` ile lexicographic scan doğru sıralama yapar. RocksDB/JMT için kritik.
3. **`#[allow(dead_code)]` yorumları:** `value_balance` modülündeki fonksiyonlar dead_code uyarısı veriyor — kullanılmıyor olabilir veya başka bir crate'ten kullanılıyor.
4. **State key merkezi:** Tüm key'ler tek dosyada. Refactoring ve debugging kolaylaşır.

### Nova Shield Chain İçin Dersler
- **Zero-padded numeric keys** pattern: Nova Shield'in tüm height-based key'leri zero-padded olmalı. Aksi halde `"9/" > "10/"` olur (lexicographic hata).
- State key'lerini tek modülde toplamak iyi pratik. Nova Shield'in `state_key.rs` modülü bu yapıyı takip etmeli.
- Prefix query için hierarchical key design: `component/subtype/index/` formatı.

---

## [event.rs]
**Dosya:** `/crates/core/component/auction/src/event.rs`
**Satır Sayısı:** 100

### Genel Amaç
Auction bileşeninin emit ettiği tüm event'leri tanımlar. 7 event türü.

### Fonksiyonlar

#### `dutch_auction_schedule_event(id, description) -> pb::EventDutchAuctionScheduled` (Satır 8–16)
Auction başlatıldığında emit edilir.

#### `dutch_auction_updated(id, state) -> pb::EventDutchAuctionUpdated` (Satır 18–27)
Her execution round sonunda (auction devam ediyorsa) emit edilir.

#### `dutch_auction_closed_by_user(id, state) -> pb::EventDutchAuctionEnded` (Satır 29–38)
Kullanıcı end_auction action'ı gönderdiğinde emit edilir.
- `reason: Reason::ClosedByOwner`

#### `dutch_auction_expired(id, state) -> pb::EventDutchAuctionEnded` (Satır 41–50)
Auction step_count'u tükettiğinde emit edilir.
- `reason: Reason::Expired`

#### `dutch_auction_exhausted(id, state) -> pb::EventDutchAuctionEnded` (Satır 52–63)
Input rezervler sıfırlandığında (tam dolum) emit edilir.
- `reason: Reason::Filled`

#### `dutch_auction_withdrawn(id, state) -> pb::EventDutchAuctionWithdrawn` (Satır 65–74)
Kullanıcı withdraw action'ı gönderdiğinde emit edilir.

#### `auction_vcb_credit(asset_id, previous_balance, new_balance) -> pb::EventValueCircuitBreakerCredit` (Satır 77–86)
Auction component'e değer girişinde emit edilir.

#### `auction_vcb_debit(asset_id, previous_balance, new_balance) -> pb::EventValueCircuitBreakerDebit` (Satır 89–99)
Auction component'ten değer çıkışında emit edilir.

### Tasarım Kararları
1. **Reason enum:** Auction bitişinin nedeni event'te taşınır. Monitoring sistemleri için kritik: expire mi, fill mi, kullanıcı mı?
2. **VCB events:** Her credit/debit event'i `previous_balance` ve `new_balance` içerir. Bu, external indexer'ların VCB'yi bağımsız izlemesine olanak tanır.
3. **Event fonksiyonları:** Constructor pattern — struct literal yerine fonksiyon çağrısı. Protobuf oluşturma mantığı merkezi.

### Nova Shield Chain İçin Dersler
- Auction bitişi için `reason` field'ı: Expired/Filled/Cancelled ayrımı analytics için kritik.
- VCB events: Nova Shield'in her değer akışında previous/new balance emit etmesi gerekir. Off-chain monitoring kolaylaşır.
- Event fonksiyonları constructor pattern: Nova Shield event modülü aynı yapıyı kullanmalı.

---

## [component/mod.rs]
**Dosya:** `/crates/core/component/auction/src/component/mod.rs`
**Satır Sayısı:** 14

### Genel Amaç
Component modülünü organize eder. Public/private visibility yönetimi.

### Export Listesi
- `pub` (dışarıya): `Auction`, `StateReadExt`, `StateWriteExt`, `AuctionStoreRead`, `metrics`, `rpc`, `action_handler`
- `pub(crate)`: `AuctionCircuitBreaker`, `DutchAuctionManager`
- Private: `auction`, `auction_store`, `dutch_auction`, `trigger_data`

### Tasarım Kararları
- `DutchAuctionManager` ve `AuctionCircuitBreaker` crate içi: Dışarıdan doğrudan erişilmez. Sadece action handler ve component aracılığıyla.
- Trait visibility kontrolü: kritik state mutation trait'leri crate-private.

---

## [component/auction.rs]
**Dosya:** `/crates/core/component/auction/src/component/auction.rs`
**Satır Sayısı:** 204

### Genel Amaç
Auction bileşeninin ana component implementasyonu. `cnidarium_component::Component` trait'i implemente eder. Aynı zamanda:
- `StateReadExt` — auction parametrelerini okur, VCB bakiyesini sorgular
- `StateWriteExt` — auction parametrelerini yazar
- `AuctionCircuitBreaker` — değer akışını takip eder (VCB mekanizması)

### Struct/Enum/Type Listesi

#### `Auction` (Satır 19)
```rust
pub struct Auction {}
```
Boş struct, Component trait'i implemente eder.

### Fonksiyonlar

#### `Component for Auction` (Satır 22–55)

##### `init_chain` (Satır 26–33)
- `app_state = None` → upgrade modu (yorum: perform upgrade specific check)
- `app_state = Some(content)` → `put_auction_params` çağrılır

##### `begin_block` (Satır 35–39)
- **Boş.** Auction bileşeni begin_block'ta hiçbir şey yapmaz.

##### `end_block` (Satır 42–49)
**KRITIK:** Auction execution burada gerçekleşir.
```rust
async fn end_block<S: StateWrite + 'static>(
    state: &mut Arc<S>,
    end_block: &abci::request::EndBlock,
) {
    let state: &mut S = Arc::get_mut(state).expect("state should be unique");
    let _ = state.process_triggers(end_block.height as u64).await;
}
```
- `end_block.height` ile trigger'lar işlenir.
- Hata yoksayılır: `let _ = ...` — bu bir tasarım seçimi veya TODO.

##### `end_epoch` (Satır 51–54)
- **Boş.** Epoch sonunda hiçbir şey yapmaz.

#### `StateReadExt::get_auction_params(&self) -> Result<AuctionParameters>` (Satır 60–65)
- State key: `"auction/parameters"`

#### `StateReadExt::auction_params_updated(&self) -> bool` (Satır 67–70)
- Object cache'de updated flag'i kontrol eder.

#### `StateReadExt::get_auction_value_balance_for(&self, asset_id: &asset::Id) -> Amount` (Satır 72–81)
- State key: `"auction/value_breaker/{asset_id}"`
- Yoksa `Amount::zero()` döner.

#### `StateWriteExt::put_auction_params(&mut self, params: AuctionParameters)` (Satır 90–93)
- Updated flag set eder.
- Params'ı yazar.

#### `AuctionCircuitBreaker::auction_vcb_credit(&mut self, value: Value) -> Result<()>` (Satır 137–165)
**VCB credit mekanizması:**
```rust
async fn auction_vcb_credit(&mut self, value: Value) -> Result<()> {
    if value.amount == Amount::zero() {
        return Ok(());  // zero-value: erken çıkış
    }
    let prev_balance = self.get_auction_value_balance_for(&value.asset_id).await;
    let new_balance = prev_balance.checked_add(&value.amount).ok_or_else(|| {
        anyhow::anyhow!("overflowed balance while crediting auction circuit breaker ...")
    })?;
    self.put(state_key::value_balance::for_asset(&value.asset_id), new_balance);
    self.record_proto(event::auction_vcb_credit(value.asset_id, prev_balance, new_balance));
    Ok(())
}
```

#### `AuctionCircuitBreaker::auction_vcb_debit(&mut self, value: Value) -> Result<()>` (Satır 170–198)
**VCB debit mekanizması:**
```rust
let new_balance = prev_balance.checked_sub(&value.amount).ok_or_else(|| {
    anyhow::anyhow!("underflowed balance while debiting auction circuit breaker ...")
})?;
```
- `checked_sub` ile underflow koruması: bileşen asla negatif bakiye veremez.

### Matematiksel Formüller

**VCB invariant:**
```
VCB[asset] = sum(all credits for asset) - sum(all debits for asset) >= 0
```

**Circuit breaker mantığı:** VCB her zaman >= 0 olmalı. `checked_sub` ile underflow → hata. Bu, auction component'inin dışarıya verdiği değerin içeri aldığından fazla olması imkansız.

### Unsafe Bloklar
Yok.

### Tasarım Kararları
1. **end_block'ta execution:** Auction tetiklemeleri end_block'ta gerçekleşir. Bu, transaction işlendikten sonra ve state commit olmadan önce çalışır.
2. **Hata yoksayma (`let _ = ...`):** `process_triggers` hatası yoksayılır. Bu, bir auction'ın execution hatası tüm blok'u durdurmaz. Trade-off: robustness vs. error visibility.
3. **VCB per-asset:** Her asset için ayrı bakiye takip edilir. Bu, cross-asset circuit breaker etkisini önler.
4. **Zero-value short-circuit:** Zero amount credit/debit işlemi atlanır. Bu, boş LP pozisyonları gibi degenerate durumları verimli işler.
5. **ASCII art diagram (Satır 101–133):** VCB değer akışı diyagramı. Kodun documentation değeri yüksek.

### Nova Shield Chain İçin Dersler
- **VCB (Value Circuit Breaker) pattern:** Nova Shield'in her bileşeni kendi VCB'sine sahip olmalı. Per-asset bakiye takibi, overflow/underflow koruması, her işlemde event emit etme.
- **end_block execution:** Periyodik işlemler (auction ticks, epoch transitions) end_block'ta çalıştırılmalı.
- **Hata isolation:** `let _ = process_triggers(...).await` pattern'i — bir sub-task hatası tüm blok'u durdurmamalı. Ama bu hatalar loglanmalı.
- **checked_add/checked_sub:** Tüm bakiye işlemlerinde overflow/underflow koruması zorunlu.

---

## [component/auction_store.rs]
**Dosya:** `/crates/core/component/auction/src/component/auction_store.rs`
**Satır Sayısı:** 58

### Genel Amaç
`AuctionStoreRead` trait'ini tanımlar. Auction verilerini state'ten okur.

### Trait: `AuctionStoreRead: StateRead`

#### `auction_id_exists(&self, auction_id: AuctionId) -> bool` (Satır 19–21)
- `get_raw_auction` çağrısı yapar, Some → true, None → false.

#### `get_dutch_auction_by_id(&self, auction_id: AuctionId) -> Result<Option<DutchAuction>>` (Satır 29–43)
- Raw `Any` proto mesajını çeker.
- `type_url` kontrolü: beklenen `pb::DutchAuction::type_url()` ile karşılaştırılır.
- Yanlış tip → hata: `"expected type to be {}, but got: {}"`
- `DutchAuction::decode(any_auction.value.as_ref())` ile deserialize.

#### `get_raw_auction(&self, auction_id: AuctionId) -> Option<Any>` (Satır 48–52)
- State key: `auction_store::by_id(auction_id)`
- Proto `Any` olarak döner.

### Tasarım Kararları
1. **`prost_types::Any` kullanımı:** Auction state'i type-erased `Any` olarak saklanır. Bu, gelecekte farklı auction tipleri eklenebilmesini sağlar (English auction, sealed bid, vb.).
2. **Type URL kontrolü:** Yanlış auction tipine erişim açık hata verir.
3. **Blanket impl:** `impl<T: StateRead + ?Sized> AuctionStoreRead for T {}` — StateRead implement eden her tip bu trait'i ücretsiz alır.

### Nova Shield Chain İçin Dersler
- `prost_types::Any` ile type-erased storage: gelecekte genişletilebilir. Nova Shield'in auction veya position storage'ı bu pattern'i kullanabilir.
- Blanket impl pattern: trait'i implement edip tüm StateRead tiplerine otomatik ekleme.

---

## [component/trigger_data.rs]
**Dosya:** `/crates/core/component/auction/src/component/trigger_data.rs`
**Satır Sayısı:** 157

### Genel Amaç
`TriggerData` struct'ını tanımlar. Dutch auction'ın zamanlama hesaplamalarını yapar.

### Struct/Enum/Type Listesi

#### `TriggerData` (Satır 5–9)
```rust
pub struct TriggerData {
    pub start_height: u64,
    pub end_height: u64,
    pub step_count: u64,
}
```
Not: Derive yok, serde yok. Sadece hesaplama için kullanılan local struct.

### Fonksiyonlar

#### `compute_step_index(&self, current_height: u64) -> Result<u64>` (Satır 18–48)

**Matematiksel formül:**
```
block_interval = end_height - start_height
step_size = block_interval / step_count  (integer division)
distance_from_start = current_height - start_height  (saturating)
step_index = distance_from_start / step_size
```

**Örnek** (test'ten):
- start=100, end=200, step_count=5
- block_interval=100, step_size=20
- current_height=100 → distance=0 → step_index=0
- current_height=119 → distance=19 → 19/20=0 (integer) → step_index=0
- current_height=121 → distance=21 → 21/20=1 → step_index=1

**Hata durumları:**
- `end < start` → checked_sub overflow → hata
- `step_count = 0` → checked_div → hata
- `step_size = 0` → checked_div → hata

#### `compute_next_trigger_height(&self, current_height: u64) -> u64` (Satır 51–73)
```
step_size = (end_height - start_height) / step_count
distance_from_start = current_height - start_height  (saturating)
prev_step_index = distance_from_start / step_size
next_step_index = prev_step_index + 1
next_trigger = start_height + step_size * next_step_index
```

**Örnek:**
- start=100, end=200, step_count=5, step_size=20
- current=100 → distance=0 → prev_step=0 → next_step=1 → next_trigger=100+20=120 ✓
- current=119 → distance=19 → prev_step=0 → next_step=1 → next_trigger=120 ✓
- current=121 → distance=21 → prev_step=1 → next_step=2 → next_trigger=100+40=140 ✓

#### `try_next_trigger_height(&self, current_height: u64) -> Option<u64>` (Satır 77–84)
- `compute_next_trigger_height` çağrır.
- Sonuç `> end_height` ise `None` döner.

### Matematiksel Formüller (Özet)

**Step size (blok başına adım):**
```
step_size = (end_height - start_height) / step_count
```

**Mevcut step index:**
```
step_index = (current_height - start_height) / step_size
```

**Sonraki trigger yüksekliği:**
```
next_trigger = start_height + step_size * (step_index + 1)
```

**Geçerlilik:**
```
next_trigger <= end_height → Some(next_trigger)
next_trigger > end_height  → None (auction sona erdi)
```

### Test'ler (Satır 87–155)

| Test | start | end | step | height | beklenen |
|------|-------|-----|------|--------|----------|
| `test_current_height_equals_start_height` | 100 | 200 | 5 | 100 | Some(120) |
| `test_current_height_equals_end_height` | 100 | 200 | 5 | 200 | None |
| `test_current_height_below_start_height` | 100 | 200 | 5 | 90 | Some(120) |
| `test_current_height_above_end_height` | 100 | 200 | 5 | 210 | None |
| `test_current_height_below_boundary` | 100 | 200 | 5 | 119 | Some(120) |
| `test_current_height_above_boundary` | 100 | 200 | 5 | 121 | Some(140) |

**Satırlık doğrulama:** start=100, end=200, step_count=5, step_size=20.
- Trigger heights: 120, 140, 160, 180, 200

### Unsafe Bloklar
Yok.

### Tasarım Kararları
1. **Integer arithmetic:** Float yok. Tam sayı aritmetiği ile deterministik trigger hesaplama.
2. **Saturating sub:** `current_height < start_height` olsa bile (auction henüz başlamamış) doğru davranış — distance=0, yani ilk trigger'a yönlendirir.
3. **Modüler kontrol yok:** `(block_window % step_count) == 0` kontrolü validation'da yapılır (`schedule.rs`), trigger_data'da değil. TriggerData temiz.

### Nova Shield Chain İçin Dersler
- Trigger/schedule ayrımı: TriggerData'nın sadece hesaplama yapması (state yok, IO yok) test edilebilirliği artırır. Nova Shield periyodik scheduler'ı da pure-function hesaplama ve state I/O'yu ayırmalı.
- **Zero-start graceful handling:** Auction başlamamış bile olsa (current < start) saturation ile doğru davranış. Nova Shield validator rotasyonu veya epoch başlangıcı için benzer pattern.

---

## [component/dutch_auction.rs]
**Dosya:** `/crates/core/component/auction/src/component/dutch_auction.rs`
**Satır Sayısı:** 631

### Genel Amaç
Dutch auction'ın tüm state mutation mantığını içerir. En kritik ve uzun dosya. Dört trait tanımlar:
- `DutchAuctionManager` — schedule/execute/end/withdraw
- `HandleDutchTriggers` — trigger processing
- `DutchAuctionData` — state reading (stream)
- `Inner` — LP allocation ve state write (private)

### Fonksiyonlar

#### `DutchAuctionManager::schedule_auction(&mut self, description: DutchAuctionDescription)` (Satır 31–85)

**Adımlar:**
1. `description.id()` ile auction ID hesapla.
2. `TriggerData` oluştur.
3. `try_next_trigger_height(current_height)` ile ilk trigger bul.
4. `DutchAuctionState { sequence: 0, current_position: None, next_trigger: ..., input_reserves: description.input.amount, output_reserves: Amount::zero() }` oluştur.
5. `auction_vcb_credit(description.input)` — input değeri auction bileşenine kredi.
6. `set_trigger_for_dutch_id(auction_id, next_trigger)` — trigger kaydı.
7. `write_dutch_auction_state(dutch_auction)` — state'e yaz.
8. `record_proto(event::dutch_auction_schedule_event(...))` — event emit.

#### `DutchAuctionManager::execute_dutch_auction(&mut self, auction_id, trigger_height)` (Satır 94–270)

**En kritik fonksiyon — tam adım analizi:**

**Adım 1: Mevcut durumu oku.**
```rust
let old_dutch_auction = self.get_dutch_auction_by_id(auction_id)...
```

**Adım 2: Mevcut LP'yi kapat ve çek.**
```rust
let lp_reserves = if let Some(auction_lp_id) = current_position {
    self.close_position_by_id(&auction_lp_id).await...
    self.withdraw_position(auction_lp_id, 0u64).await...
} else {
    Balance::zero()
};
```

**Adım 3: Trigger'ı kaldır.**
```rust
self.unset_trigger_for_dutch_id(auction_id, trigger_height);
```

**Adım 4: State'i sıfırla.**
```rust
new_dutch_auction.state.current_position = None;
new_dutch_auction.state.next_trigger = None;
```

**Adım 5: LP inflow'unu hesapla ve rezervlere ekle.**
```rust
let lp_inflow_input_asset = Value {
    asset_id: auction_input_id,
    amount: lp_reserves.provided()
        .filter(|v| v.asset_id == auction_input_id)
        .map(|v| v.amount)
        .sum::<Amount>(),
};
// ... aynı output için
self.auction_vcb_credit(lp_inflow_input_asset).await...
self.auction_vcb_credit(lp_inflow_output_asset).await...
new_dutch_auction.state.input_reserves += lp_inflow_input_asset.amount;
new_dutch_auction.state.output_reserves += lp_inflow_output_asset.amount;
```

**Adım 6: Step index hesapla.**
```rust
let step_index = auction_trigger.compute_step_index(trigger_height)...
```

**Adım 7: Sonlanma koşullarını kontrol et.**
```rust
let is_auction_expired = step_index >= step_count;
let is_auction_filled = new_dutch_auction.state.input_reserves == Amount::zero();
```

**Adım 8a: Sonlanma → sequence = 1.**
```rust
if is_auction_expired || is_auction_filled {
    new_dutch_auction.state.sequence = 1;
}
```

**Adım 8b: Devam → fiyat hesapla, LP aç.**
```rust
else {
    let next_trigger = auction_trigger.compute_next_trigger_height(trigger_height);
    let price = compute_pq_at_step(&new_dutch_auction.description, step_index);
    let input_reserves = new_dutch_auction.state.input_reserves;
    new_dutch_auction.state.input_reserves = Amount::zero();  // LP'ye transfer
    let pair = DirectedTradingPair::new(auction_input_id, auction_output_id);
    let maybe_id = self.allocate_position(pair, input_reserves, step_index, price, auction_nonce).await;
    new_dutch_auction.state.current_position = maybe_id;
    new_dutch_auction.state.next_trigger = NonZeroU64::new(next_trigger);
    self.set_trigger_for_dutch_id(auction_id, next_trigger);
}
```

**Adım 9: State'i kaydet, event emit et.**
```rust
self.write_dutch_auction_state(new_dutch_auction);
// expired / filled / updated event emit
```

#### `DutchAuctionManager::end_auction(&mut self, auction_to_close: DutchAuction)` (Satır 289–374)

**Kullanıcı tarafından kapatma:**
1. `sequence >= 1` ise short-circuit (zaten kapalı).
2. LP varsa kapat ve çek, inflow'ları hesapla.
3. VCB'ye kredi.
4. Toplam rezervleri hesapla.
5. Trigger'ı kaldır.
6. `sequence = 1` ile yeni state yaz.

#### `DutchAuctionManager::withdraw_auction(&mut self, auction: DutchAuction) -> Result<Balance>` (Satır 392–426)

**Reserve çekme:**
1. `previous_input_reserves` ve `previous_output_reserves` değerlerini al.
2. `auction_vcb_debit` ile her ikisini VCB'den düş.
3. `withdraw_balance = input_reserves + output_reserves`.
4. `sequence += 1` (saturating), tüm alanları sıfırla.
5. Event emit, state yaz.
6. `withdraw_balance` döndür.

#### `HandleDutchTriggers::process_triggers(&mut self, trigger_height: u64)` (Satır 435–448)

```rust
async fn process_triggers(&mut self, trigger_height: u64) -> Result<()> {
    let auction_ids: Vec<AuctionId> = self
        .stream_dutch_ids_by_trigger(trigger_height)
        .await
        .collect()
        .await;

    for auction_id in auction_ids.into_iter() {
        self.execute_dutch_auction(auction_id, trigger_height).await?;
    }
    Ok(())
}
```

**Önemli:** Önce tüm ID'ler collect edilir, sonra execute edilir. Streaming sırasında state mutation yapılmaz.

#### `DutchAuctionData::stream_dutch_ids_by_trigger(&self, trigger_height: u64)` (Satır 455–470)

```rust
let prefix_key = state_key::dutch::trigger::by_height(trigger_height).as_bytes().to_vec();
self.nonverifiable_prefix::<AuctionId>(&prefix_key)
    .map(|res| {
        let (_, auction_id) = res.expect("no deserialization error");
        auction_id
    })
    .boxed()
```
- Prefix scan: `"auction/dutch/trigger/{height:020}/"` ile başlayan tüm key'ler.
- Stream döndürür (lazy evaluation).

#### `Inner::allocate_position(...)` (Satır 476–563)

**DEX entegrasyon noktası — kritik fonksiyon:**

```rust
async fn allocate_position(
    &mut self,
    pair: DirectedTradingPair,
    input_reserves: Amount,
    step_index: u64,
    (p, q): (Amount, Amount),   // fiyat parametreleri
    auction_nonce: [u8; 32],
) -> Option<position::Id>
```

**Adım 1: DEX aktif mi?**
```rust
if !self.get_dex_params().await?.is_enabled {
    return None;
}
```

**Adım 2: Benzersiz nonce bul (retry loop).**
```rust
let mut attempt_counter = 0u64;
loop {
    let full_hash = blake2b_simd::Params::default()
        .personal(b"penum-DA-nonce")
        .to_state()
        .update(&auction_nonce)
        .update(&step_index.to_le_bytes())
        .update(&attempt_counter.to_le_bytes())
        .finalize();
    // tough_nonce = first 32 bytes of hash

    let mut lp = Position::new_with_nonce(tough_nonce, pair, 0u32, p, q, lp_reserves);
    lp.close_on_fill = true;  // KRITIK: tek yönlü LP

    let position_id = lp.id();

    if self.check_position_by_id(&position_id).await {
        attempt_counter += 1;  // çakışma → tekrar dene
        continue;
    } else {
        self.auction_vcb_debit(lp.reserves_1()).await?;
        self.auction_vcb_debit(lp.reserves_2()).await?;
        self.open_position(lp).await?;
        return Some(position_id);
    }
}
```

**LP parametreleri:**
- `pair`: `DirectedTradingPair(input_id → output_id)`
- `fee = 0u32`: Auction LP'si sıfır ücretli
- `p, q`: Fiyat parametreleri (`compute_pq_at_step`'ten)
- `r1 = input_reserves, r2 = 0`: Sadece input token
- `close_on_fill = true`: Dolunca otomatik kapat

**Nonce türetme:**
```
position_nonce = BLAKE2b(
    personal = "penum-DA-nonce",
    auction_nonce || step_index || attempt_counter
)
```

#### `Inner::write_dutch_auction_state(&mut self, new_state: DutchAuction)` (Satır 566–581)

State'e `prost_types::Any` olarak yazar:
```rust
let pb_state: pb::DutchAuction = new_state.into();
let raw_auction = pb_state.encode_to_vec();
let any_auction = prost_types::Any {
    type_url: pb::DutchAuction::type_url(),
    value: raw_auction,
};
self.put_raw(key, any_auction.encode_to_vec());
```

#### `Inner::set_trigger_for_dutch_id` / `unset_trigger_for_dutch_id`
- `nonverifiable_put/delete` ile trigger kaydı.
- Key: `"auction/dutch/trigger/{height:020}/{auction_id}"`

#### `fn compute_pq_at_step(description, step_index) -> (Amount, Amount)` (Satır 606–630)

**KRITIK: Dutch auction fiyat interpolasyon formülü.**

```rust
fn compute_pq_at_step(
    auction_description: &DutchAuctionDescription,
    step_index: u64,
) -> (Amount, Amount) {
    let max_output = auction_description.max_output;
    let min_output = auction_description.min_output;
    let input = auction_description.input;
    let step_index = Amount::from(step_index);
    let step_count = Amount::from(auction_description.step_count);
    let one = Amount::from(1u128);

    // Linearly interpolate between max_output at step_index=0
    //                          and min_output at step_index=step_count-1
    let target_output_scaled =
        (step_count - step_index - one) * max_output + step_index * min_output;
    let input_scaled = (step_count - one) * input.amount;

    let p = target_output_scaled;
    let q = input_scaled;

    (p, q)
}
```

### Matematiksel Formüller — DETAYLI ANALİZ

#### Dutch Auction Fiyat Formülü

**Adım:** `step_index` ∈ [0, step_count-1]

**Hedef output (scaled):**
```
target_output_scaled = (step_count - step_index - 1) * max_output + step_index * min_output
```

**Örnek:** step_count=5, max_output=100, min_output=20, input=10
- step_index=0: `(5-0-1)*100 + 0*20 = 400` → output_scaled=400
- step_index=1: `(5-1-1)*100 + 1*20 = 320` → output_scaled=320
- step_index=2: `(5-2-1)*100 + 2*20 = 240` → output_scaled=240
- step_index=3: `(5-3-1)*100 + 3*20 = 160` → output_scaled=160
- step_index=4: `(5-4-1)*100 + 4*20 = 80` → output_scaled=80

**Input (scaled):**
```
input_scaled = (step_count - 1) * input.amount = 4 * 10 = 40
```

**DEX trading pair fiyatı:**
```
p = target_output_scaled
q = input_scaled
```

**LP exchange rate:** `p/q` = output/input oranı

Gerçek exchange rate:
- step=0: 400/40 = 10 (10 input için 100 output — max fiyat)
- step=1: 320/40 = 8
- step=2: 240/40 = 6
- step=3: 160/40 = 4
- step=4: 80/40 = 2 (10 input için 20 output — min fiyat)

**Lineer interpolasyon doğrulaması:**
```
actual_rate(step) = (step_count - step - 1) * max_output / (step_count - 1) + step * min_output / (step_count - 1)
```
Scaling ile bölme yapılmıyor — `(step_count - 1)` ile çarpmak q'yu ölçekler, böylece:
```
effective_rate = p/q = target_output_scaled / input_scaled
```

**Neden scaling?** `Amount` türü tam sayı. Kesirli işlem yerine payları ve paydaları ayrı tutmak, LP fiyat parametreleri `p` ve `q`'ya doğrudan map olur.

**52-bit neden yeterli?**
- max: `(step_count-1) * max_output` ≤ 254 * (2^52-1) ≈ 2^59.9
- u128 için yeterli headroom: 2^128 >> 2^60

### Unsafe Bloklar
Yok.

### Test'ler
Direkt test yok (`#[cfg(test)]` bloğu yok). Integration testler başka bir crate'te.

### Tasarım Kararları
1. **LP-based execution:** Dutch auction DEX LP pozisyonu açarak çalışır. Merkezi orderbook değil — AMM ile entegrasyon. Bu, DEX likidite havuzundan yararlanır.
2. **close_on_fill:** LP doldurulunca kapanır. Bu, "round-trip" saldırısını engeller: output token → input token swap imkansız.
3. **fee=0 LP:** Auction LP'si ücretsiz. Arbitraj yapanlar fiyat farkından kar eder, fee'den değil.
4. **Nonce retry loop:** Position ID çakışması olursa attempt_counter artar ve yeni nonce denenir. Teorik olarak sonsuz döngü ama pratikte çakışma olasılığı negligible.
5. **Collect then execute:** Stream'i önce collect et, sonra execute et. Streaming sırasında state değişimi ve stream invalidation riski yok.
6. **Direct DEX bypass:** `close_position_by_id` → `withdraw_position` → `allocate_position` doğrudan DEX PositionManager trait'lerini kullanır. Auction, DEX'in LP yöneticisi gibi davranır.

### Nova Shield Chain İçin Dersler
- **Fiyat interpolasyon formülü aynen kullanılabilir.** Nova Shield Dutch Auction uygulaması `compute_pq_at_step` mantığını alabilir.
- **LP-based auction:** Her step'te LP aç/kapat. Bu, DEX ile derin entegrasyon sağlar ve auction likiditeyi DEX'e inject eder.
- **close_on_fill = true:** Single-direction LP. Nova Shield DEX'teki auction pozisyonları için zorunlu.
- **Nonce türetimi:** `BLAKE2b(auction_nonce || step_index || attempt_counter)` — deterministic, collision-resistant, retry-safe.
- **Process triggers pattern:** Önce tüm ID'leri collect, sonra execute. Streaming state mutation'dan kaçın.
- **DEX disabled check:** Auction LP açmadan önce DEX enabled mi kontrol et. Nova Shield'de DEX pause mekanizması olmalı.

---

## [component/action_handler/dutch/schedule.rs]
**Dosya:** `/crates/core/component/auction/src/component/action_handler/dutch/schedule.rs`
**Satır Sayısı:** 132

### Genel Amaç
`ActionDutchAuctionSchedule` için `ActionHandler` implementasyonu. Stateless ve stateful validation + execution.

### Fonksiyonlar

#### `check_stateless(&self, _context: ()) -> Result<()>` (Satır 17–106)

**Validation kuralları (tüm satır numaralarıyla):**

| Satır | Kural |
|-------|-------|
| 30–33 | `input.amount > 0` (pozitif giriş) |
| 35–36 | `step_count > 0` |
| 38–42 | `input.amount <= MAX_AUCTION_AMOUNT_RESERVES` (52-bit) |
| 44–48 | `input.asset_id != output_id` (farklı tokenlar) |
| 50–54 | `max_output > min_output` |
| 56–57 | `max_output > 0` |
| 59–63 | `max_output <= MAX_AUCTION_AMOUNT_RESERVES` |
| 65–66 | `min_output > 0` |
| 68–72 | `min_output <= MAX_AUCTION_AMOUNT_RESERVES` |
| 74–80 | `start_height < end_height` (strict less than) |
| 82–87 | `step_count >= 2` (interpolasyon için min 2 adım) |
| 89–93 | `step_count <= 255` |
| 95–104 | `(end_height - start_height) % step_count == 0` (blok penceresi bölünebilir) |

#### `check_and_execute<S: StateWrite>(&self, mut state: S) -> Result<()>` (Satır 109–131)

**Stateful validation:**
1. `start_height > current_height` (gelecekte başlamalı)
2. `!state.auction_id_exists(id).await` (duplicate ID yok)
3. `state.schedule_auction(description.clone()).await?` (execution)

### Matematiksel Formüller

**Step count kısıtlamaları:**
```
2 <= step_count <= 255
(end_height - start_height) % step_count == 0
start_height < end_height
input.amount <= (1 << 52) - 1
max_output <= (1 << 52) - 1
min_output <= (1 << 52) - 1
min_output < max_output
```

### Tasarım Kararları
1. **step_count <= 255:** 8-bit limit. Yorum yok ama muhtemelen byte-level encoding veya hash space consistency.
2. **step_count >= 2:** Lineer interpolasyon `(step_count - 1)` ile bölme yapar. step_count=1 → division by zero riski.
3. **Bölünebilirlik kontrolü:** `block_window % step_count == 0` — adımlar tam sayı blok içerir, kesirli adım yok.
4. **start_height > current_height:** Geçmiş auction başlatılamaz. Ama `>=` değil `>`: tam blok başlatma yok.

### Nova Shield Chain İçin Dersler
- Validation kuralları Nova Shield Dutch Auction için aynen uygulanabilir. 52-bit limit, step_count 2-255 arası, bölünebilirlik kontrolü kritik.
- `start_height > current_height` (strict): gelecekteki blok. Nova Shield'de aynı.
- Stateless ve stateful validation ayrımı: heavy IO (auction_id_exists) stateless'te yapılmaz, stateful'da yapılır.

---

## [component/action_handler/dutch/end.rs]
**Dosya:** `/crates/core/component/auction/src/component/action_handler/dutch/end.rs`
**Satır Sayısı:** 55

### Genel Amaç
`ActionDutchAuctionEnd` için `ActionHandler`. Kullanıcı tarafından auction kapatma.

### Fonksiyonlar

#### `check_stateless` (Satır 17–19)
Boş. Stateless kontrol yok — yeterli veri yok (sadece auction_id).

#### `check_and_execute` (Satır 22–54)
1. Auction'ı state'ten çek.
2. Sequence kontrolü: `matches!(auction.state.sequence, 0 | 1)` — 0 veya 1 olmalı (açık veya kapalı).
3. `state.end_auction(auction).await?`
4. `ClosedByOwner` event emit.

### Tasarım Kararları
- Sequence `0 | 1` kabul: Zaten kapalı (1) auction'ı tekrar kapatmak idempotent. Bu, transaction replay saldırısını önler ama double-close izin verir. `end_auction` içinde `sequence >= 1` short-circuit var.
- **Withdrawn (sequence >= 2) durumda hata:** Withdraw edilmiş auction kapatılamaz.

### Nova Shield Chain İçin Dersler
- Idempotent end: Zaten kapalı auction'ı kapatmak hata vermemeli. Nova Shield state machine'lerinde idempotent operasyonlar tercih edilmeli.

---

## [component/action_handler/dutch/withdraw.rs]
**Dosya:** `/crates/core/component/auction/src/component/action_handler/dutch/withdraw.rs`
**Satır Sayısı:** 64

### Genel Amaç
`ActionDutchAuctionWithdraw` için `ActionHandler`. Reserve çekme işlemi.

### Fonksiyonlar

#### `check_stateless` (Satır 14–26)
```rust
ensure!(self.seq >= 1, "seq MUST be >= 1");
ensure!(self.seq < u64::MAX, "seq maximum is u64::MAX");
```
seq=0 withdraw imkansız (auction açık). seq=u64::MAX overflow koruması.

#### `check_and_execute` (Satır 29–63)
1. Auction'ı state'ten çek.
2. **Sequence increment kontrolü:**
   ```rust
   ensure!(
       self.seq == auction_state.state.sequence.saturating_add(1),
       "action seq MUST be incremented by one (previous: {}, action: {})",
       self.seq, auction_state.state.sequence
   );
   ```
3. `state.withdraw_auction(auction_state).await?`
4. **Reserve commitment doğrulama:**
   ```rust
   let expected_reserve_commitment = withdrawn_balance.commit(Fr::zero());
   ensure!(
       self.reserves_commitment == expected_reserve_commitment,
       "the reported reserve commitment is incorrect"
   );
   ```

### Matematiksel Formüller

**Commitment doğrulama:**
```
expected_commitment = Pedersen(input_reserves + output_reserves, blinding=0)
claimed_commitment == expected_commitment  → geçerli
```

### Tasarım Kararları
- Sequence increment zorlaması: Her withdraw sequence'ı bir artırır. Bu, replay koruması sağlar.
- Reserve commitment: ZK value binding. Kullanıcı yanlış değer iddia edemez.
- `saturating_add(1)`: u64::MAX overflow koruması.

### Nova Shield Chain İçin Dersler
- **Sequence increment protokolü:** Her withdraw bir sequence artırır. Double-withdraw imkansız. Nova Shield'de aynı.
- **Reserve commitment doğrulama:** Chain kendi hesapladığı commitment ile action'daki commitment'ı karşılaştırır. Nova Shield'de her çekme işlemi bu doğrulamayı yapmalı.

---

## [component/rpc.rs]
**Dosya:** `/crates/core/component/auction/src/component/rpc.rs`
**Satır Sayısı:** 96

### Genel Amaç
gRPC query servisi. `QueryService` trait implementasyonu.

### Struct/Enum/Type Listesi

#### `Server` (Satır 26–28)
```rust
pub struct Server {
    storage: Storage,
}
```
Cnidarium storage'a referans tutar.

### Fonksiyonlar

#### `auction_state_by_id` (Satır 38–83)
1. `auction_id` parse et.
2. `get_raw_auction(id)` ile `Any` proto al.
3. `type_url == DutchAuction::type_url()` kontrolü.
4. `dutch_auction.state.current_position` varsa LP'yi de çek.
5. `AuctionStateByIdResponse { auction: raw_any, positions: [lp] }` döndür.

#### `auction_state_by_ids` (Satır 89–95)
```rust
todo!()
```
Henüz implement edilmemiş.

### Tasarım Kararları
- RPC raw `Any` proto döndürür: type-agnostic response. Client hangi auction tipini işleyeceğini `type_url`'den anlar.
- LP dahil response: Auction durumunu görmek için LP pozisyonuna da bakmak gerekebilir. Convenience.
- `todo!()`: `auction_state_by_ids` eksik. Bu, batch query imkansız — eksik özellik.
- `#![allow(unused)]`: TODO yorumu var, kullanılmayan import'lar var.

### Nova Shield Chain İçin Dersler
- gRPC query server pattern: storage → latest_snapshot → query. Nova Shield'de aynı.
- `type_url` ile type dispatch: Any proto kullanıyorsanız her zaman type_url kontrolü yapın.
- `todo!()` ile eksik RPC'ler: Nova Shield'de eksik RPC'ler compile-time visible olmalı.

---

## [component/metrics.rs]
**Dosya:** `/crates/core/component/auction/src/component/metrics.rs`
**Satır Sayısı:** 25

### Genel Amaç
Metrics altyapısı. Şu anda hiçbir metrik tanımlı değil.

### Fonksiyonlar

#### `register_metrics()` (Satır 17–23)
Boş. Yorum ile örnekler verilmiş:
```rust
// describe_gauge!(SOMETHING_TO_MEASURE, Unit::Count, "...");
```

### Tasarım Kararları
- Placeholder metrics modülü. Gelecekte eklenecek.
- `pub use metrics::*;` ile metrics crate'ini re-export eder.

### Nova Shield Chain İçin Dersler
- Metrics modülünü baştan koy. Nova Shield'in her bileşeninde metrics modülü olmalı. Baştan boş olabilir ama altyapı hazır.
- `describe_gauge!/describe_counter!` pattern: metrics crate standart API'si. Nova Shield'de kullanılmalı.

---

## [component/action_handler.rs]
**Dosya:** `/crates/core/component/auction/src/component/action_handler.rs`
**Satır Sayısı:** 1

```rust
pub mod dutch;
```

Sadece `dutch` sub-modülünü declare eder.

---

## [component/action_handler/dutch/mod.rs]
**Dosya:** `/crates/core/component/auction/src/component/action_handler/dutch/mod.rs`
**Satır Sayısı:** 3

```rust
mod end;
mod schedule;
mod withdraw;
```

Private sub-modüller. Dışarı export yok (ActionHandler trait implementasyonları implicit olarak kullanılır).

---

## DUTCH AUCTION STATE MACHINE — TAM ANALIZ

```
                   schedule_auction()
                          │
                          ▼
                    ┌──────────┐
                    │ seq = 0  │  OPENED
                    │ position │◀─────────────────────────┐
                    │ reserves │                          │
                    └──────────┘                          │
                          │                               │
              ┌───────────┼────────────────┐              │
              │           │                │              │
         end_auction  execute_dutch    execute_dutch      │
         (user)       (expired)        (filled)           │
              │           │                │              │
              └───────────▼────────────────┘              │
                    ┌──────────┐                          │
                    │ seq = 1  │  CLOSED                  │
                    │ no pos.  │                          │
                    │ reserves │                          │
                    └──────────┘                          │
                          │                               │
                   withdraw_auction()                     │
                          │                               │
                          ▼                               │
                    ┌──────────┐                          │
                    │ seq = 2+ │  WITHDRAWN               │
                    │ no pos.  │  (her withdraw +1)       │
                    │ zero res │                          │
                    └──────────┘                          │
                                                          │
    execute_dutch (devam ediyor) → LP aç → seq=0 kalır ──┘
```

**Sequence semantiği:**
- `0` → Açık auction. LP pozisyonu olabilir veya olmayabilir.
- `1` → Kapalı. LP yok. Rezervler var. Kullanıcı withdraw bekliyor.
- `>= 2` → Withdrawn. Her withdraw +1 artırır. Tüm rezervler sıfır.

---

## DUTCH AUCTION DEĞER AKIŞI — TAM ANALIZ

```
Kullanıcı             Auction Component              DEX (LP Manager)
    │                       │                              │
    │ schedule_auction       │                              │
    │──input_token──────────▶│ VCB += input_amount         │
    │◀──AuctionNFT(seq=0)────│                              │
    │                       │                              │
    │                [end_block: trigger]                   │
    │                       │ input_reserves → LP          │
    │                       │──open_position(p,q)──────────▶│
    │                       │ VCB -= input_amount          │
    │                       │                              │
    │                       │    [DEX swap olursa]         │
    │                       │◀─output_token────────────────│
    │                       │                              │
    │                [end_block: next trigger]              │
    │                       │◀─close_position──────────────│
    │                       │◀─withdraw_position───────────│
    │                       │ VCB += (input_remain + output)│
    │                       │ Yeni LP → yeni fiyat         │
    │                       │──open_position(p',q')────────▶│
    │                       │ VCB -= input_remain          │
    │                       │                              │
    │                [expire veya fill]                     │
    │                       │ sequence = 1                 │
    │                       │                              │
    │ end_auction (optional)│                              │
    │──AuctionNFT(seq=0)───▶│ LP kapat+çek                │
    │◀──AuctionNFT(seq=1)───│ sequence = 1                │
    │                       │                              │
    │ withdraw_auction       │                              │
    │──AuctionNFT(seq=1)───▶│ VCB -= (input + output)     │
    │◀──input_remain────────│                              │
    │◀──output_earned───────│                              │
    │◀──AuctionNFT(seq=2)───│                              │
```

---

## KRITIK SAYISAL SABITLER

| Sabit | Değer | Konum | Açıklama |
|-------|-------|-------|----------|
| `MAX_AUCTION_AMOUNT_RESERVES` | `(1 << 52) - 1 = 4_503_599_627_370_495` | schedule.rs:10 | Max input/output miktarı |
| `step_count` min | `2` | schedule.rs:84-87 | Min adım sayısı |
| `step_count` max | `255` | schedule.rs:89-93 | Max adım sayısı |
| `sequence` başlangıç | `0` | dutch.rs:62 | Opened state |
| `sequence` closed | `1` | dutch_auction.rs:228 | Closed state |
| LP fee | `0u32` | dutch_auction.rs:530 | Auction LP ücretsiz |
| LP r2 | `Amount::zero()` | dutch_auction.rs:516-518 | Başlangıçta output sıfır |
| Nonce domain sep | `b"penum-DA-nonce"` | dutch_auction.rs:521 | LP nonce hash |
| Auction domain sep | `b"penumbra_DA_nft"` | dutch.rs:15 | AuctionId hash |
| AuctionId boyutu | `32 bytes` | id.rs:11 | BLAKE2b output |
| Bech32m prefix | `"pauctid1"` | id.rs:19 | Human-readable prefix |
| NFT denom format | `"auctionnft_{seq}_{id}"` | nft.rs:22 | Asset denom |

---

## GENEL TASARIM KARARLARI VE MIMARI

### 1. NFT-Ownership Pattern
Her auction NFT ile temsil edilir. NFT sequence numarası auction durumu. Bu:
- Double-spend imkansız (NFT burn/mint atomik)
- State takibi şeffaf (chain explorer NFT sahipliğini görebilir)
- UTXO-benzeri: her durum geçişi eski NFT'yi yakar, yeni NFT basar

### 2. LP-as-Auction Pattern
Dutch auction, her adımda bir DEX LP pozisyonu açar. Bu:
- DEX likiditenin auctionlardan faydalanmasını sağlar
- Arbitrajcılar auction fiyatını "keşfeder" (DEX routing)
- Merkezi orderbook gerekmez

### 3. Value Circuit Breaker
Her component kendi VCB'sini tutar. Invariant: `VCB >= 0` her zaman.
- Credit: değer girişi
- Debit: değer çıkışı
- checked_add/checked_sub: overflow/underflow koruması

### 4. Trigger Scheduling
Auction'lar blok height ile trigger'lanır. `end_block` callback'te tetiklenir.
- Zero-padded key ile lexicographic scan
- Collect then execute pattern
- Prefix query: aynı yükseklikteki tüm auction'lar

### 5. Type-Erased Storage
Auction state `prost_types::Any` olarak saklanır. Gelecekte yeni auction tipleri eklenebilir.

### 6. Privacy Architecture
- NFT'ler public (denom'dan görünür)
- Reserve commitment: gizlenebilir (blinding_factor = 0 şu an)
- Tam ZK gizliliği için blinding_factor ≠ 0 kullanılabilir

---

## NOVA SHIELD CHAIN ICIN DERS VE UYGULAMA PLANI

### 1. Dutch Auction Implementasyonu (Direkt Kullanım)

**`compute_pq_at_step` formülü aynen alınabilir:**
```rust
fn compute_pq_at_step(input: Amount, max_output: Amount, min_output: Amount, step_count: u64, step_index: u64) -> (Amount, Amount) {
    let step_count = Amount::from(step_count);
    let step_index = Amount::from(step_index);
    let one = Amount::from(1u128);
    let p = (step_count - step_index - one) * max_output + step_index * min_output;
    let q = (step_count - one) * input;
    (p, q)
}
```

**Validation kuralları (aynen uygula):**
- input, max_output, min_output < 2^52
- 2 <= step_count <= 255
- (end - start) % step_count == 0

### 2. Value Circuit Breaker (Zorunlu)
Nova Shield'in her bileşeninde VCB uygulanmalı:
```rust
pub trait ComponentVCB: StateWrite {
    async fn vcb_credit(&mut self, value: Value) -> Result<()>;
    async fn vcb_debit(&mut self, value: Value) -> Result<()>;
}
```
- checked_add/checked_sub zorunlu
- Per-asset tracking
- Her işlemde event emit

### 3. NFT Sequence Pattern (Öneri)
Nova Shield DEX pozisyonları için NFT-tabanlı sahiplik:
```
nsdexpos_{seq}_{position_id}
```
Her position kapatma/withdraw sequence artırır.

### 4. Trigger System Pattern
Periyodik görevler için:
```
nova/component/trigger/{height:020}/{task_id}
```
- Zero-padded height
- Prefix scan ile batch execution

### 5. AuctionId Hash Uyarısı
Penumbra'da `min_output` ve `output_id` hash'e dahil değil. Nova Shield'in ID hash'i **tüm** parametreleri içermeli:
```rust
BLAKE2b(domain_sep || nonce || input_id || input_amount || output_id ||
        max_output || min_output || start_height || end_height || step_count)
```

### 6. LP close_on_fill
Nova Shield DEX'te auction pozisyonları `close_on_fill = true` olmalı. Aksi takdirde round-trip saldırısı mümkün.

### 7. DEX Pause Check
Auction LP açmadan önce:
```rust
if !dex_params.is_enabled { return None; }
```
Nova Shield DEX emergency pause mekanizması auction'ları da etkiler.

### 8. State Key Design
Nova Shield state key'leri zero-padded numeric değerler kullanmalı:
```rust
format!("ns/{component}/{index:020}/")
```

### 9. Eksik Metrikler
Penumbra'nın boş metrics.rs'ini taklit etme. Nova Shield auction bileşeninde ölçülmesi gerekenler:
- `auction_total_scheduled` — toplam başlatılan auction
- `auction_total_filled` — tam dolum ile biten auction
- `auction_total_expired` — expire olan auction
- `auction_input_volume` — auction giriş hacmi (per asset)
- `auction_output_volume` — auction çıkış hacmi (per asset)

### 10. Eksik RPC (auction_state_by_ids)
Penumbra'da `todo!()` olarak bırakılmış batch query. Nova Shield'de baştan implement et.

---

## POTANSIYEL HATALAR VE GUVENLIK ANALIZI

### 1. AuctionId'de Eksik Parametreler (Düşük Risk)
**Bulgu:** `min_output` ve `output_id` hash'e dahil değil (dutch.rs:77-94).
**Risk:** Farklı `min_output` veya `output_id` ile aynı nonce → aynı AuctionId.
**Etki:** Saldırgan aynı ID ile farklı bir auction başlatmaya çalışabilir. Ancak `auction_id_exists` kontrolü bunu engeller — sadece bir auction aynı ID'ye sahip olabilir.
**Sonuç:** Pratik etki düşük ama hash'i incomplete yapıyor.

### 2. end_block'ta Hata Yoksayma (Orta Risk)
**Bulgu:** `let _ = state.process_triggers(...).await` (auction.rs:48)
**Risk:** Auction execution hatası sessizce yoksayılır. Trigger kaldırılmamış olabilir (set ama unset edilmemiştir).
**Etki:** Auction tekrar trigger olmaz, stuck kalabilir.
**Önerim:** Hatayı log'la ama panic etme.

### 3. Regex Her Çağrıda Derleme (Düşük Risk)
**Bulgu:** `Regex::new(...)` her `TryFrom<Metadata>` çağrısında (nft.rs:63).
**Risk:** CPU overhead.
**Öneri:** `once_cell::sync::Lazy<Regex>` veya `std::sync::OnceLock`.

### 4. LP Nonce Retry Sonsuz Döngü (Teorik)
**Bulgu:** `allocate_position`'da retry loop (dutch_auction.rs:512-562).
**Risk:** Çok sayıda çakışma varsa sonsuz döngü.
**Gerçekçilik:** BLAKE2b çıktısı çakışma olasılığı: 1/2^256. Pratikte imkansız.

### 5. auction_state_by_ids TODO (Orta Risk)
**Bulgu:** `todo!()` (rpc.rs:94).
**Risk:** Runtime panic. Eğer client bu RPC'yi çağırırsa crash.
**Öneri:** `unimplemented` yerine `Err(Status::unimplemented(...))`.

---

## DOSYA SAYIMI VE KAPSAM

| Kategori | Dosya Sayısı | Satır Sayısı |
|----------|-------------|-------------|
| Auction data types | 5 | ~400 |
| Component logic | 8 | ~350 |
| Action handlers | 5 | ~150 |
| Supporting files | 9 | ~200 |
| **Toplam** | **27** | **~1100** |

Tüm 27 dosya okundu ve analiz edildi.

---

*Agent-auction — Penumbra Auction Component Analizi — 2026-07-22*
