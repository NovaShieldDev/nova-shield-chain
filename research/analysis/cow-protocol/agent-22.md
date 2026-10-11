# CoW Protocol Solana Indexer — Agent 22 Derin Analiz

**Analiz Edilen Dizin:** `/research/libs/cow-protocol/crates/solana-indexer/`
**Toplam Dosya:** 21 Rust dosyası
**Toplam Kaynak Satırı:** ~1.100 satır (tip tanımları + iş mantığı + testler)
**Crate Versiyonu:** 0.1.0 (erken geliştirme — birçok `todo!()` ve `unimplemented!()` stub'ı mevcut)
**Lisans:** GPL-3.0-or-later

---

## Genel Bakış: Bu Crate Ne Yapar?

`solana-indexer`, CoW Protocol'ün Ethereum'dan Solana'ya genişleme stratejisinin veri altyapısıdır. Görevi:
1. Solana blockchain'ini Yellowstone gRPC streaming protokolü üzerinden dinlemek
2. CoW Protocol'ün Solana üzerindeki iki programının (`settlement` ve `SolFlow`) transaction'larını tespit etmek
3. Bu transaction'lardan anlamlı domain event'leri çıkarmak (order oluşturma, settlement finalizasyonu, vb.)
4. Bu event'leri PostgreSQL veritabanına persist etmek

Önemli bağlam: Bu indexer, CoW Protocol'ün EVM tabanlı batch auction mekanizmasını Solana'ya taşıma projesinin bir parçasıdır. `SolFlow` programı, SOL tabanlı kullanıcıların CoW auctions'larına native token (SOL) ile katılmasını sağlayan bir wrapper katmanıdır.

---

## Dosya 1: `src/lib.rs` — 8 satır

### Amaç
Crate'in kök modülü. `missing_docs` uyarısı aktif — tüm public API'ların dokümante edilmesi zorunlu.

### Modül Yapısı
```
pub mod indexer;    -- Decoder, Ingester, FinalizationWorker
pub mod persistence; -- PostgreSQL katmanı
pub mod traits;     -- Dış bağımlılık arayüzleri
pub mod types;      -- Domain tip tanımları
```

### Tasarım Kararı
Crate hem `lib` hem `bin` olarak derleniyor (Cargo.toml: `[lib]` + `[[bin]]`). Bu, hem kütüphane olarak kullanılabilmesini (entegrasyon testleri için) hem de bağımsız binary olarak çalışabilmesini sağlıyor.

---

## Dosya 2: `src/main.rs` — 3 satır

### Amaç
Binary giriş noktası. Şu an sadece bir `println!` stub'ı. Gerçek başlatma mantığı henüz implement edilmemiş.

```rust
fn main() {
    println!("solana-indexer starting");
}
```

### Tasarım Notu
Production entrypoint burada olmayacak muhtemelen — `Ingester::serve()` zaten production wiring'i yapıyor. `main()` muhtemelen config parsing, logging kurulumu ve tokio runtime başlatmayı barındıracak.

---

## Dosya 3: `src/types/mod.rs` — 13 satır

### Amaç
Domain tiplerinin merkezi namespace'i. 8 alt modül re-export eder.

### Önemli Re-export
```rust
pub use solana_sdk::signature::Signature;
```
`Signature` tipi tüm domain kodunda `crate::types::Signature` olarak kullanılıyor — solana_sdk bağımlılığının sızmasını engeller.

### Alt Modüller
- `channel` — Ingester→Decoder kanal mesajları
- `commitment` — Confirmation/finalization takip tipleri
- `errors` — Domain hata enum'ları
- `events` — Settlement ve SolFlow event taxonomy'si
- `order` — OrderUid newtypes
- `recovery` — PDA snapshot ve backfill tipleri
- `slot` — Slot newtypes
- `tx` — Decoder'ın transaction görünümü
- `wire` — yellowstone-grpc-proto re-export'ları

---

## Dosya 4: `src/types/slot.rs` — 11 satır

### Amaç
Solana ledger slot'larını temsil eden newtype wrapper.

### Struct Tanımı (satır 9-11)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, From, Into)]
#[display("Slot({})", _0)]
pub(crate) struct Slot(pub u64);
```

### Tasarım Kararları
- `PartialOrd + Ord`: Slot karşılaştırması için (hangi slot daha eski?)
- `Hash`: HashMap key olarak kullanılabilir
- `From<u64> + Into<u64>`: `derive_more` ile otomatik dönüşüm
- `Display`: Log formatlamasında `Slot(42)` çıktısı
- `pub(crate)`: Sadece crate içinde görünür

---

## Dosya 5: `src/types/order.rs` — 27 satır

### Amaç
CoW Protocol'ün Solana versiyonunda order identifier'ı olan `OrderUid` newtype'ı.

### Struct Tanımı (satır 16-17)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, From, Into)]
pub(crate) struct OrderUid(pub [u8; 32]);
```

### Kritik Detay: `hash(intent)` = UID
```
// Satır 8-15 docstring
/// A 32-byte CoW Protocol order identifier, equal to `hash(intent)`.
/// This is the value used to derive the order PDA seed (["settlement",
/// hash(intent), "order"]).
```

**Bu kritik bir tasarım kararıdır:** OrderUid, order intent'inin hash'idir. Bu sayede:
1. OrderUid deterministik: aynı intent → aynı UID
2. PDA (Program Derived Address) türetmek için seed olarak kullanılıyor: `["settlement", uid_bytes, "order"]`
3. SolFlow custody kayıtları, trade-delta muhasebesi ve settlement program'ın event'leri aynı 32 byte'ı taşıyor

### Display İmplementasyonu (satır 19-27)
```rust
impl fmt::Display for OrderUid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OrderUid(0x")?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        write!(f, ")")
    }
}
```
Hex formatında log output: `OrderUid(0xdeadbeef...)`

### DEX Uygulama Notu
Nova Shield için: On-chain order identifier'larını hash(intent) olarak türetmek, order tracking'i stateless hale getirir. PDA seed olarak kullanmak, order state'ini blockchain'de deterministic adreste saklamayı sağlar. Bu pattern Nova Shield DEX order management için benimsenebilir.

---

## Dosya 6: `src/types/events.rs` — 161 satır

### Amaç
Sistemin tüm domain event taxonomy'si. İki program'ın event'lerinin merkezi tanımı.

### `TradeDelta` Struct (satır 17-31)
```rust
pub(crate) struct TradeDelta {
    pub order_uid: OrderUid,
    pub amount_withdrawn_delta: u64,   // Sell-side: önceki snapshot'tan delta
    pub amount_received_delta: u64,    // Buy-side: önceki snapshot'tan delta
    pub order_fulfilled: bool,         // İnferred: settlement sonrası PDA'dan
}
```

**Kritik tasarım**: `order_fulfilled` alanı on-chain event'den değil, indexer tarafından PDA snapshot karşılaştırmasıyla çıkarılıyor. `amount_withdrawn` ≥ `sell_amount` veya `amount_received` ≥ `buy_amount` koşullarından biri sağlandığında `true` set ediliyor.

### `SettlementEvent` Enum (satır 35-105)

| Variant | Alanlar | Kaynak |
|---|---|---|
| `OrderCreated` | order_uid, owner, created_by | `CreateOrder` instruction |
| `OrderClosed` | order_uid | Lifecycle event |
| `OrderCancelled` | order_uid | Cancel instruction |
| `SettlementFinalized` | auction_id, solver, tx_signature, slot, trades: Vec<TradeDelta> | BeginSettle+FinalizeSettle pair |
| `BufferCreated` | token: Pubkey | CreateBuffer instruction |
| `BufferUsed` | token, tx_signature | Buffer kullanım event'i |
| `ManagerUpdated` | from, to | Yönetim rotasyonu |
| `SolverAdded` | solver | Allow-list event |
| `SolverRemoved` | solver | Allow-list event |
| `SolverInteraction` | tx_signature, ix_index | Generic catch-all |

**`SettlementFinalized` — En Kritik Event:**
- `auction_id: u64` — Hangi auction'dan geldiği (şu an placeholder 0, program henüz wire etmiyor)
- `solver: Pubkey` — Kazanan solver (fee payer = transaction'ın ilk account key'i)
- `trades: Vec<TradeDelta>` — Her settle edilen order'ın sell/buy delta'ları

### `SolFlowEvent` Enum (satır 113-151)

SolFlow, SOL'u wSOL'a sararak CoW auctions'larına native SOL'la katılım sağlayan katman:

| Variant | Alanlar | Anlamı |
|---|---|---|
| `OrderCreated` | custodial_pda, real_owner, order_uid, sol_amount | Yeni SolFlow order |
| `OrderEnabled` | custodial_pda, enabler, order_uid | `SetUpOrder` ix: wSOL settlement'a transfer edildi |
| `OrderRecovered` | custodial_pda, slot | Stuck-state cleanup |

**SolFlow Lifecycle Kritik Detay** (satır 126-143 docstring):
`OrderEnabled` event'i, `SetUpOrder` instruction gözlemlendiğinde emit edilir. Bu noktada wSOL custody'si settlement program'a geçmiş, order auction'a dahil edilmeye hazır. `enabler`, `SetUpOrder`'ı imzalayan unprivileged relayer (gas ödeyici).

### `DecodedEvent` Enum (satır 155-160)
```rust
pub(crate) enum DecodedEvent {
    Settlement(SettlementEvent),
    SolFlow(SolFlowEvent),
}
```
Decoder'dan persistence katmanına taşınan birleşik tip.

### DEX Uygulama Notu
CoW'un event taxonomy'si DEX event modelleme için mükemmel referans:
1. **Delta-based trade accounting**: Mutlak değer yerine önceki snapshot'tan delta — reentrancy ve partial fill durumlarında daha güvenli
2. **Solver tracking**: Her settlement event'i solver address'ini taşıyor → solver performans analizi mümkün
3. **Buffer events**: Liquidity buffer yönetimi için ayrı event tipi — Nova Shield DEX'te flash loan buffer tracking için benzer pattern

---

## Dosya 7: `src/types/tx.rs` — 53 satır

### Amaç
Decoder'ın transaction seviyesindeki çalışma tiplerini tanımlar. Upstream `solana-sdk` tiplerinin decode edilmiş görünümleri.

### `ResolvedInstruction` Struct (satır 20-35)
```rust
pub(crate) struct ResolvedInstruction {
    pub program_id: Pubkey,          // program_id_index çözümlenmiş
    pub data: Bytes,                 // Ham instruction data
    pub accounts: Vec<u8>,           // Account indices (henüz çözümlenmemiş)
    pub instruction_index: u32,      // Top-level instruction pozisyonu
    pub inner_ix_path: Vec<u8>,      // CPI derinlik path
}
```

`inner_ix_path` semantiği (satır 31-34):
- `[]` — Top-level instruction
- `[0]` — Top-level'ın ilk CPI'ı
- `[0, 1]` — O ilk CPI'ın yaptığı ikinci CPI

Bu, nested CPI'ları benzersiz olarak tanımlamak için minimal ama yeterli bir encoding.

### `TxContext` Struct (satır 40-53)
```rust
pub(crate) struct TxContext {
    pub slot: Slot,
    pub signature: Signature,
    pub account_keys: Vec<Pubkey>,       // static + ALT-loaded writable + readonly
    pub post_token_balances: Vec<TokenBalance>,  // SolFlow OrderCreated için wSOL balance
}
```

**Hesaplama notu**: `account_keys` üç bölgeden oluşuyor (Solana V0 transaction formatı):
1. `message.account_keys` (statik)
2. `meta.loaded_writable_addresses` (ALT'tan yüklenen yazılabilir)
3. `meta.loaded_readonly_addresses` (ALT'tan yüklenen salt okunur)

Bu üçünün sıralı birleşimi instruction'lardaki index'lerin çözümleneceği tam liste.

---

## Dosya 8: `src/types/commitment.rs` — 82 satır

### Amaç
Transaction'ların Solana consensus pipeline'ındaki ilerlemesini izleme tipleri.

### `Commitment` Enum (satır 29-37)
```rust
pub(crate) enum Commitment {
    Confirmed,    // Supermajority tarafından oy verildi, hâlâ rollback mümkün
    Finalized,    // Cluster tarafından root'landı, kalıcı
    RolledBack,   // Hiç inmedi veya slot abandon edildi
}
```

**Neden `Processed` yok?** (satır 27-28 docstring):
`processed` commitment, node'un anlık görünümünü yansıtır ve rollback riski taşır. Indexer sadece durable state'leri persist ediyor.

### String Label Metodu (satır 39-48)
```rust
pub fn as_label(self) -> &'static str {
    match self {
        Self::Confirmed => "confirmed",
        Self::Finalized => "finalized",
        Self::RolledBack => "rolled_back",
    }
}
```
PostgreSQL `commitment` kolonunda kullanılacak string değerler.

### `SignatureStatus` Struct (satır 51-57)
```rust
pub(crate) struct SignatureStatus {
    pub slot: Slot,
    pub confirmation_status: Commitment,
}
```
`getSignatureStatuses` RPC sonucunu temsil eder.

### `AccountInfo` Struct (satır 59-68)
```rust
pub(crate) struct AccountInfo {
    pub slot: Slot,
    pub data: Bytes,       // Serileştirilmiş account data
    pub owner: Pubkey,     // Account'ın sahibi program
}
```
Recovery akışında `getAccountInfo` ile alınan PDA snapshot'ı.

### `UnfinalizedRow` Struct (satır 74-82)
```rust
pub(crate) struct UnfinalizedRow {
    pub table: &'static str,  // Hangi solana.* tablosu
    pub signature: Signature,
    pub slot: Slot,
}
```
Finalization worker'ın sweep ettiği row shape'i. `table` alanı `&'static str` — hangi PostgreSQL tablosunun UPDATE edileceğini biliyor.

---

## Dosya 9: `src/types/errors.rs` — 58 satır

### Amaç
Domain hata enum'larının merkezi tanımı. `thiserror` kullanıyor.

### `DecodeError` (satır 8-21)
```rust
pub(crate) enum DecodeError {
    UnknownDiscriminator,   // İlk byte(lar) bilinen instruction'a eşleşmedi
    AltResolutionFailed,    // ALT loaded-address listesi çözümlenemedi
    SchemaMismatch,         // Tanındı ama layout uyuşmadı
}
```

### `PersistenceError` (satır 24-34)
```rust
pub(crate) enum PersistenceError {
    Conflict,       // SQL ON CONFLICT (watermark regression gibi)
    Unavailable,    // Geçici bağlantı hatası, caller retry etmeli
}
```

### `StreamError` (satır 37-58) — En Karmaşık
```rust
pub(crate) enum StreamError {
    Disconnected,
    SendTimeout,
    ReplayWindowExceeded {
        attempted_slot: Slot,
        earliest_replayable_slot: Slot,
    },
}
```

`ReplayWindowExceeded` önemli: Yellowstone gRPC sağlayıcılar sınırlı replay window tutuyor. Eğer indexer çok geride kalırsa, bu hata fırlatılıyor ve caller'ın `from_slot`'u sıfırlaması ve kaybedilen aralığı kaydetmesi gerekiyor.

---

## Dosya 10: `src/types/channel.rs` — 22 satır

### Amaç
Ingester → Decoder arası kanal mesaj tipi.

### `StreamUpdate` Enum (satır 12-22)
```rust
pub(crate) enum StreamUpdate {
    Tx {
        slot: Slot,
        signature: Signature,
        inner: Box<SubscribeUpdateTransactionInfo>,  // Box: büyük struct, heap'e
    },
}
```

Şu an sadece `Tx` variant'ı var. Gelecekte slot/block update'leri için de variant eklenebilir. `Box<SubscribeUpdateTransactionInfo>` — büyük struct'ın stack taşımasını önler, enum variant boyutunu sabit tutar.

---

## Dosya 11: `src/types/wire.rs` — 31 satır

### Amaç
`yellowstone-grpc-proto` tiplerinin curate edilmiş re-export surface'i.

### Re-export Listesi
```rust
pub use yellowstone_grpc_proto::{
    geyser::{
        CommitmentLevel, SlotStatus, SubscribeRequest,
        SubscribeRequestFilterAccounts, SubscribeRequestFilterSlots,
        SubscribeRequestFilterTransactions, SubscribeUpdate,
        SubscribeUpdateAccount, SubscribeUpdateAccountInfo,
        SubscribeUpdatePing, SubscribeUpdateSlot,
        SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo,
        subscribe_update::UpdateOneof,
    },
    solana::storage::confirmed_block::{
        CompiledInstruction, InnerInstruction, InnerInstructions,
        Message, TokenBalance, Transaction, TransactionStatusMeta,
    },
};
```

**Tasarım kararı**: Harici proto tiplerini `wire` modülü arkasına almak, `yellowstone-grpc-proto` major version bump durumunda sadece bu dosyayı güncellemeyi yeterli kılıyor. Domain kodu `wire::SubscribeUpdateTransactionInfo` kullanıyor, proto path'i değil.

---

## Dosya 12: `src/types/recovery.rs` — 35 satır

### Amaç
Recovery akışı için PDA snapshot ve backfill seçenekleri.

### `PdaSnapshot` Struct (satır 10-21)
```rust
pub(crate) struct PdaSnapshot {
    pub order_uid: OrderUid,
    pub amount_withdrawn: u64,      // Kümülatif sell-side çekilme
    pub amount_received: u64,       // Kümülatif buy-side alım
    pub cancelled: bool,
    pub cancellation_timestamp: Option<i64>,  // Unix seconds
}
```

Bu struct `getAccountInfo` ile okunan order PDA'nın mevcut on-chain durumunu temsil ediyor. Recovery'de: eğer indexer bir transaction'ı kaçırdıysa, PDA snapshot ile DB state karşılaştırılarak delta hesaplanabilir.

### `GetSignaturesOpts` Struct (satır 24-35)
```rust
pub(crate) struct GetSignaturesOpts {
    pub from_slot: Option<u64>,
    pub to_slot: Option<u64>,
    pub limit: Option<usize>,
    pub address: Option<Pubkey>,
}
```
`getSignaturesForAddress` RPC çağrısı için parametreler. `Default` implement ediyor — çağırıcı sadece ihtiyaç duyduğu alanları set edebilir.

---

## Dosya 13: `src/persistence.rs` — 105 satır

### Amaç
PostgreSQL persistence katmanının arayüzü. **Tüm metodlar `todo!()`** — gerçek implementasyon "Postgres adapter" PR'ında gelecek.

### `Persistence` Struct (satır 21)
```rust
#[derive(Clone)]
pub(crate) struct Persistence {}
```
"Cheap to clone: wraps a shared pool" notu var ama pool alanı henüz yok. `Clone` implementasyonu, birden fazla component'in (Decoder, Watchdog, FinalizationWorker) aynı persistence instance'ını paylaşmasına izin vermek için.

### Metod İmzaları (Satır Numarasıyla)

**satır 25-31** — `persist_events`:
```rust
pub(crate) async fn persist_events(
    &self,
    events: Vec<DecodedEvent>,
    new_watermark: u64,
) -> Result<(), PersistenceError>
```
Event'leri ve slot watermark'ı atomik olarak kaydeder. Atomiklik kritik — event'ler ve watermark aynı transaction'da yazılmalı, ikisi arasında crash olursa duplicate yoksa kayıp olur.

**satır 34-37** — `write_watermark`:
```rust
pub(crate) async fn write_watermark(&self, slot: u64) -> Result<(), PersistenceError>
```
"Rejects downward writes" — watermark'ı geriye götürmez. SQL `ON CONFLICT` ile implement edilecek.

**satır 40-43** — `read_watermark`:
```rust
pub(crate) async fn read_watermark(&self) -> Result<Option<u64>, PersistenceError>
```
Reconnect sonrası devam etmek için. `None` → cold start.

**satır 45-50** — `record_lost_slot_range`:
```rust
pub(crate) async fn record_lost_slot_range(
    &self,
    range: RangeInclusive<u64>,
) -> Result<(), PersistenceError>
```
Replay window aşıldığında kaybedilen slot aralığını kaydet. v0.1'de write-only (okuma yok).

**satır 52-65** — `get_confirmed_rows`:
```rust
pub(crate) async fn get_confirmed_rows(
    &self,
    max_slot: Slot,
    limit: usize,
) -> Result<Vec<UnfinalizedRow>, PersistenceError>
```
Finalization worker'ın primary pass'i için: `commitment = 'confirmed'` ve `slot <= max_slot` olan row'ları getirir. `limit` DB page size (RPC batch size değil — FinalizationWorker bu row'ları 256'lık batch'lere böler).

**satır 68-75** — `get_aged_rows`:
```rust
pub(crate) async fn get_aged_rows(
    &self,
    retention_horizon_slot: u64,
) -> Result<Vec<UnfinalizedRow>, PersistenceError>
```
Safety-net sweep: ~150 slot'tan eski `confirmed` row'lar `getSignatureStatuses` tarafından artık raporlanmıyor. Bunlar `getTransaction` ile tek tek kontrol edilir.

**satır 79-88** — `update_commitment`:
```rust
pub(crate) async fn update_commitment(
    &self,
    row: &UnfinalizedRow,
    new_commitment: Commitment,
) -> Result<(), PersistenceError>
```
`row.table` alanını kullanarak doğru `solana.*` tablosunda `commitment` kolonunu günceller.

**satır 91-96** — `backfill_event`:
Recovery akışı için — watermark ilerletmeden tek event kaydeder.

**satır 99-104** — `upsert_pda_snapshot`:
On-chain PDA state'ini reconciliation için kaydeder/günceller.

### DEX Uygulama Notu
Bu persistence API tasarımı blockchain indexer'ları için ideal pattern gösteriyor:
1. **Atomik event+watermark yazımı**: event'leri ve ilerlemeyi aynı DB transaction'ında persist et
2. **Watermark regression koruması**: geçmiş slot'a yazma denemelerini reddet
3. **İki aşamalı finalization**: önce confirmed, sonra finalized — rollback'lere karşı güvenli
4. **Kayıp aralık kaydı**: replay window aşımlarını takip et, veri bütünlüğünü belgele

Nova Shield için: UTXO state izleyen indexer'larda aynı pattern'ı kullan. Özellikle `persist_events` + `write_watermark` atomikliği kritik.

---

## Dosya 14: `src/traits/mod.rs` — 3 satır

Sadece `pub mod solana_client;` içeriyor.

---

## Dosya 15: `src/traits/solana_client.rs` — 44 satır

### Amaç
Finalization worker'ın ihtiyaç duyduğu Solana RPC çağrıları için trait arayüzü. Test enjeksiyonu ve mock için.

### `SolanaClient` Trait (satır 19-44)
```rust
#[async_trait]
pub(crate) trait SolanaClient: Send + Sync {
    async fn get_signature_statuses(
        &self,
        signatures: &[Signature],
    ) -> Result<Vec<Option<SignatureStatus>>, ClientError>;

    async fn get_transaction(
        &self,
        signature: &Signature,
    ) -> Result<Option<SubscribeUpdateTransactionInfo>, ClientError>;

    async fn get_signatures_for_address(
        &self,
        address: &Pubkey,
        opts: GetSignaturesOpts,
    ) -> Result<Vec<Signature>, ClientError>;

    async fn get_account_info(
        &self,
        address: &Pubkey,
    ) -> Result<Option<AccountInfo>, ClientError>;
}
```

**Metod analizi:**

1. **`get_signature_statuses`**: Batch RPC — tek çağrıda max 256 signature status'u sorgular. `Vec<Option<...>>` — bilinmeyen signature'lar `None` döner. Finalization promotion pass'in temel silahı.

2. **`get_transaction`**: Tekil, arşivsel lookup. `Ok(None)` = never landed (rolled back). Aged-row sweep'in silahı. `SubscribeUpdateTransactionInfo` döndürüyor — geyser formatı, decoder aynı tipi kullanabilir.

3. **`get_signatures_for_address`**: Backfill için. Bir program address'inin tüm transaction imzalarını listeler. `GetSignaturesOpts` ile slot aralığı ve limit kontrolü.

4. **`get_account_info`**: PDA snapshot okuma. `Ok(None)` = hesap yok (deleted veya initialize edilmemiş). Recovery reconciliation'ı için.

### Tasarım Kararı
`Arc<dyn SolanaClient>` olarak kullanılıyor — trait object, runtime polimorfizmi. Gerçek implementasyon `solana-client` crate'ini kullanacak, test implementasyonları mock enjekte edebilir.

---

## Dosya 16: `src/indexer/mod.rs` — 27 satır

### Amaç
Üç indexer component'ini dışa açar.

### Mimari Açıklama (satır 1-20 docstring)
Sistemin üç ana bileşeninin rolü:

**Ingester**: Yellowstone gRPC stream'ini olabildiğince hızlı drainler, decoder'a iletir. Decoding yapmaz — socket asla yavaş işleme sebebiyle durmamalı. `latest_chain_slot` counter'ının tek yazarı.

**Decoder**: Raw update'leri alır, settlement ve SolFlow transaction'larını ayıklar, tiplendirilmiş event'leri store'a persist eder.

**FinalizationWorker**: Row'lar önce `confirmed` commitment'la yazılır. Bu worker onları chain'e karşı yeniden kontrol ederek `finalized`'a promote eder veya `rolled_back` işaretler.

---

## Dosya 17: `src/indexer/finalization.rs` — 66 satır

### Amaç
Transaction finalization'ını yöneten worker skeleton'ı. `run()` metodu `unimplemented!()`.

### Sabitler (satır 35-42)
```rust
pub const FINALIZATION_WINDOW_SLOTS: u64 = 32;
// ~12.8 saniye @ 400ms/slot
// Promotion pass, bundan daha yeni row'ları atlar (henüz finalize edilemez)

pub const PROMOTION_BATCH_LIMIT: usize = 256;
// getSignatureStatuses batch limiti (Solana RPC hard cap)

pub const SIGNATURE_STATUS_RETENTION_SLOTS: u64 = 150;
// getSignatureStatuses bu kadar slot'tan eskisini raporlamaz
```

**Neden 32 slot?** Solana'da tipik finalization ~32 slot sürer. Degraded consensus durumunda daha uzun sürebilir — aged-row sweep bunu yakalar.

**Neden 150 slot retention?** Solana'nın `getSignatureStatuses` endpoint'i yaklaşık 150 slot'luk bir pencere tutuyor. Bu pencerenin dışına çıkan transaction'lar `getTransaction` ile archival node'dan sorgulanmak zorunda.

### İki Akış Mantığı

**Primary (Promotion Pass):**
- `confirmed` row'lar arasından `slot <= tip - 32` olanları al
- `getSignatureStatuses` ile 256'lık batch'ler halinde sorgula
- `finalized` → `update_commitment(Finalized)`
- Henüz confirm olmamış → bekle

**Secondary (Aged-Row Sweep):**
- `confirmed` row'lar arasından `slot < tip - 150` olanları al
- Her biri için ayrı `getTransaction` çağrısı
- `Some(tx)` → `update_commitment(Finalized)`
- `None` → `update_commitment(RolledBack)`

### `FinalizationWorker` Struct (satır 46-52)
```rust
pub(crate) struct FinalizationWorker {
    pub persistence: Persistence,
    pub rpc: Arc<dyn SolanaClient>,
}
```

### DEX Uygulama Notu
İki seviyeli finalization pattern Nova Shield için kritik önem taşıyor:
- EVM tabanlı DEX'lerde block finality deterministic değil (reorg riski var)
- Solana'da da benzer — `confirmed` ≠ `finalized`
- Nova Shield DEX: settlement event'lerini sadece `finalized` commitment'tan sonra işle
- Aged-row sweep: uzun süreli stuck order'ları otomatik cleanup için

---

## Dosya 18: `src/indexer/ingester.rs` — 361 satır

**En önemli execution dosyası.** Gerçek implementasyonu var.

### Sabit (satır 58)
```rust
pub const INGEST_TO_DECODER_CAPACITY: usize = 1024;
```
Ingester → Decoder mpsc kanalının kapasitesi. 1024 mesaj buffer.

### `Ingester<S>` Generic Struct (satır 69-87)
```rust
pub(crate) struct Ingester<S>
where
    S: Stream<Item = Result<SubscribeUpdate, Status>> + Unpin + Send,
{
    pub stream: S,
    pub tx: Sender<StreamUpdate>,
    pub latest_chain_slot: Arc<AtomicU64>,
}
```

Generic `S` — stream tipi enjekte edilebilir. Production: `GeyserStream`, testler: `stream::iter(...)`.

### `run()` Metodu (satır 114-134) — Ana Drain Loop

```rust
pub async fn run(&mut self) -> Result<(), Error> {
    while let Some(update) = self.stream.next().await {
        match update {
            Ok(update) => {
                if Self::handle_update(&self.tx, &self.latest_chain_slot, update)
                    .await
                    .is_break()
                {
                    tracing::info!("decoder channel closed; ingester stopping");
                    return Ok(());
                }
            }
            Err(status) => {
                tracing::warn!(%status, "yellowstone stream error; ingester stopping");
                return Err(Error::Stream(status));
            }
        }
    }
    Err(Error::StreamEnded)
}
```

**Tasarım kararları:**
- `AutoReconnect` wrapper hataları içerde yönetir, bu loop asla retry görmez
- Decoder channel kapandığında `Ok(())` (clean shutdown)
- Stream terminal hata → `Err(Error::Stream(...))`
- Stream sona erdi (wrapper vazgeçti) → `Err(Error::StreamEnded)`

### `handle_update()` — Associated Function (satır 142-170)

```rust
async fn handle_update(
    tx: &Sender<StreamUpdate>,
    latest_chain_slot: &AtomicU64,
    update: SubscribeUpdate,
) -> ControlFlow<()>
```

**Neden associated function, method değil?** (satır 138-141 comment):
`&self` yerine sadece gerekli alanlar borrow ediliyor. Bu, `run()`'ın future'ını `Send` yapar — `GeyserStream` `Send + !Sync` olduğundan `&self` kullanılsa future `Send` olmayabilirdi.

**Dispatch mantığı:**
```
Transaction → handle_transaction() → forward() → mpsc channel
Slot        → handle_slot() → latest_chain_slot.fetch_max()
Ping/Pong   → Drop (ignore)
Account/Block/BlockMeta/Entry/TransactionStatus → Drop
```

### `handle_transaction()` (satır 175-196)
```rust
#[tracing::instrument(skip_all, fields(slot = tx_msg.slot))]
async fn handle_transaction(
    tx: &Sender<StreamUpdate>,
    tx_msg: SubscribeUpdateTransaction,
) -> ControlFlow<()>
```

İki guard:
1. `transaction` alanı `None` ise → warn ve skip
2. `signature` 64 byte değilse → warn ve skip

### `handle_slot()` (satır 199-206) — Atomik Güncelleme
```rust
async fn handle_slot(
    latest_chain_slot: &AtomicU64,
    slot: SubscribeUpdateSlot,
) -> ControlFlow<()> {
    latest_chain_slot.fetch_max(slot.slot, Ordering::Relaxed);
    ControlFlow::Continue(())
}
```

`fetch_max` + `Ordering::Relaxed` — slot counter sadece monoton artış yapar, `Relaxed` yeterli çünkü diğer thread'lerin bu değeri ne zaman gördüğü critical değil.

### `forward()` — Backpressure Handling (satır 211-224)
```rust
async fn forward(tx: &Sender<StreamUpdate>, update: StreamUpdate) -> ControlFlow<()> {
    match tx.try_send(update) {
        Ok(()) => ControlFlow::Continue(()),
        Err(TrySendError::Full(update)) => {
            tracing::warn!("decoder channel full; ingester blocked on backpressure");
            match tx.send(update).await {
                Ok(()) => ControlFlow::Continue(()),
                Err(_) => ControlFlow::Break(()),
            }
        }
        Err(TrySendError::Closed(_)) => ControlFlow::Break(()),
    }
}
```

**Backpressure stratejisi:**
1. `try_send` — non-blocking, ideal yol
2. Channel dolu → warn + blocking `send().await` — decoder drainleana kadar bekle
3. Channel closed → `Break()` — clean shutdown

TODO notu var: sustained backpressure log flooding'i için rate-limit eklenmeli.

### `serve()` — Production Entrypoint (satır 261-283)
```rust
pub async fn serve(
    mut client: GeyserGrpcClient,
    tx: Sender<StreamUpdate>,
    persistence: Persistence,
    latest_chain_slot: Arc<AtomicU64>,
    settlement_program: Pubkey,
    solflow_program: Pubkey,
) -> Result<(), Error>
```

**Adımlar:**
1. `persistence.read_watermark()` → `from_slot = watermark + 1`
2. `subscribe_request(settlement_program, solflow_program, from_slot)` oluştur
3. `client.subscribe_with_request(Some(request))` → `(sink, stream)` aç
4. `Ingester::new(stream, tx, latest_chain_slot)` oluştur
5. `ingester.run().await`

`_sink` drop ediliyor — runtime resubscription gerekirse gelecekte kullanılabilir.

### `subscribe_request()` — Subscription Filtresi (satır 322-357)
```rust
fn subscribe_request(
    settlement_program: Pubkey,
    solflow_program: Pubkey,
    from_slot: Option<u64>,
) -> SubscribeRequest
```

**Subscription yapısı:**
- `transactions["settlement_txs"]`: `account_include: [settlement_program]`, `vote: false`, `failed: None`
- `transactions["sol_flow_txs"]`: `account_include: [solflow_program]`, `vote: false`, `failed: None`
- `slots["chain_tip"]`: `filter_by_commitment: true`
- `commitment: Confirmed`
- `from_slot: Option<u64>` (cold start'ta `None`)

**`failed: None` neden?** Başarısız transaction'ları da include ediyor. On-chain başarısız order create veya cancel — bu failure kendisi downstream için bir sinyal olabilir.

**`filter_by_commitment: Some(true)`**: Slot filter sadece subscription'ın commitment level'ındaki slot'ları bildirir (Confirmed).

### `assert_serve_future_is_send()` (satır 291-308)
Compile-time kontrol: `Ingester::serve`'ın future'ı `Send` mi? `tokio::spawn` çağrısı gelene kadar bu testi tutuyor.

### DEX Uygulama Notu
**Yellowstone gRPC Pattern Nova Shield için:**
Solana üzerinde DEX event'leri dinlemek için bu exact pattern kullanılabilir:
1. İki program filter (DEX core + wrapper program)
2. `confirmed` commitment'ta subscribe (hız için)
3. `from_slot` ile resume (restart sonrası kayıp yok)
4. Non-blocking `try_send` + backpressure `send().await` pattern
5. `AtomicU64::fetch_max` ile monoton slot tracking

**Kritik tasarım kararı**: Ingester hiç decode yapmıyor. Bu ayrışma sayesinde stream consumer asla decode gecikmesi yüzünden durmuyor. Nova Shield indexer mimarisinde bu pattern zorunlu olmalı.

---

## Dosya 19: `src/indexer/decoder.rs` — 622 satır

**Sistemin kalbi. En karmaşık ve en önemli dosya.**

### `Decoder` Struct (satır 36-48)
```rust
pub(crate) struct Decoder {
    pub persistence: Persistence,
    pub rx: Receiver<StreamUpdate>,
    pub settlement_program: Pubkey,
    pub solflow_program: Pubkey,
}
```

### `run()` Metodu (satır 68-78)
```rust
pub async fn run(&mut self) -> Result<(), PersistenceError> {
    while let Some(update) = self.rx.recv().await {
        let StreamUpdate::Tx { slot, signature, inner } = update;
        self.decode(&inner, slot, signature);
    }
    Ok(())
}
```

Sender drop edildiğinde `Ok(())` — clean shutdown.

### `decode()` — Transaction Dispatch (satır 83-137)

**Adım 1**: `relevant_instructions()` — tx'den sadece settlement/SolFlow instruction'larını çıkar
**Adım 2**: `build_account_keys()` — tam account listesini reconstruct et
**Adım 3**: `TxContext` oluştur
**Adım 4**: Settlement instruction'larını `decode_settlement()` ile decode et
**Adım 5**: SolFlow instruction'ları için `decode_solflow()` (henüz stub)
**Adım 6**: Event'leri log'la (persistence henüz yok, `todo!()`)

### Sabitler (satır 153-171)
```rust
const PLACEHOLDER_AUCTION_ID: u64 = 0;
// BeginSettle wire henüz auction_id taşımıyor

const PLACEHOLDER_AMOUNT_RECEIVED: u64 = 0;
// FinalizeSettle henüz buy-side push amount taşımıyor

const CREATE_ORDER_CREATED_BY: usize = 1;
// CreateOrder account list: [owner(S), created_by(W,S), order_pda(W), system_program(R)]

const BEGIN_SETTLE_FIXED_ACCOUNTS: usize = 3;
// BeginSettle: [instructions_sysvar, state_pda, token_program] + per-order accounts

const CREATE_BUFFER_SHARED_ACCOUNTS: usize = 3;
// CreateBuffer: [payer, system_program, token_program] + (buffer_pda, mint) pairs
```

### `decode_settlement()` — Pure Function (satır 191-244)

```rust
fn decode_settlement(
    instructions: &[ResolvedInstruction],
    ctx: &TxContext,
    resolve_order: impl Fn(&Pubkey) -> Option<ResolvedOrder>,
) -> Vec<SettlementEvent>
```

**Pure tasarım**: Test enjeksiyonu için `resolve_order` closure parametresi. Production'da DB'den order PDA→UID mapping'i çözümler, testlerde canned map inject edilir.

**İki geçiş:**
1. **Standalone instruction geçişi**: `CreateOrder`, `CreateBuffer`, `Initialize` her biri kendi başına decode edilebilir
2. **Pair geçişi**: `BeginSettle` + `FinalizeSettle` birlikte decode edilir → `SettlementFinalized`

**Hata yönetimi**: Başarısız decode → warning log (dead-letter table gelecekte). Sessizce drop edilmiyor.

### `decode_order_created()` (satır 249-265) — Kritik Algoritma

```rust
fn decode_order_created(
    instruction: &ResolvedInstruction,
    body: &[u8],
    account_keys: &[Pubkey],
) -> Result<SettlementEvent, DecodeError>
```

**Algoritma:**
1. `body` → `[u8; EncodedOrderIntent::SIZE]` cast et
2. `EncodedOrderIntent::decode_and_hash(bytes)` → `(intent, uid_hash)`
3. `uid_hash.to_bytes()` → `OrderUid`
4. Account list[1] → `created_by`
5. `SettlementEvent::OrderCreated` oluştur

**Önemli**: `owner` instruction account listinden değil, intent data'sından alınıyor. Intent'in hash'i = UID, intent'in içindeki `owner` field'ı = gerçek owner. Test (satır 304): `pubkey(11)` account listinde var ama `[0x11; 32]` intent'de owner — bunlar kasıtlı farklı.

### `decode_buffers_created()` (satır 267-293) — Buffer Batch

```rust
fn decode_buffers_created(
    instruction: &ResolvedInstruction,
    account_keys: &[Pubkey],
) -> Result<Vec<SettlementEvent>, DecodeError>
```

Body boş. Tüm bilgi account list'te:
- `accounts[3..] ` → `[(buffer_pda_idx, mint_idx), ...]` çiftleri
- Her çift için `BufferCreated { token: account_keys[mint_idx] }`
- `as_chunks::<2>()` ile güvenli pair iterasyonu
- Remainder != 0 → `SchemaMismatch` (tek sayıda account — hatalı)

### `decode_settlements_finalized()` — En Karmaşık Algoritma (satır 302-373)

**BeginSettle + FinalizeSettle pairing algoritması:**

```
İçin her BeginSettle instruction:
  1. BeginSettle body'den: recover_counterpart(body) → finalize_ix_index
  2. Tx'de finalize_ix_index pozisyonunda FinalizeSettle var mı? Yoksa skip.
  3. parse_begin_settle_orders(pull_body) → Vec<BeginSettleOrder>
  4. Her order için: resolve_account(begin, keys, order.order_pda_position) → pda_pubkey
  5. resolve_order(&pda_pubkey) → Option<ResolvedOrder>
  6. TradeDelta oluştur
  7. SettlementFinalized event emit et
```

**Solver tespiti (satır 309-311)**:
```rust
let Some(&solver) = ctx.account_keys.first() else {
    return Vec::new();
};
```
Solana'da transaction fee payer = `account_keys[0]`. Bu solver'ın adresi.

**BeginSettle wire formatı** (satır 319):
```
[finalize_ix_index: u16 BE][n][bump×n][count×n][amount: u64 BE × T]
```
`recover_counterpart` ilk 2 byte'ı (counterpart index) soyar, `parse_begin_settle_orders` geri kalanını işler.

### `parse_begin_settle_orders()` — Wire Format Parser (satır 392-434)

**Format**: `[order_count: u8][bumps: u8×n][counts: u8×n][amounts: u64 BE × T]`

```
n = order_count
T = sum(counts)
```

**Algoritma:**
1. `order_count` oku
2. `bumps[0..n]` skip et (PDA türetme için, indexer ihtiyaç duymuyor)
3. `counts[0..n]` oku
4. `sum(counts) == amounts.len()` doğrula
5. Her order için:
   - counts[i] adet amount topla → `amount_withdrawn_delta`
   - Account position: `BEGIN_SETTLE_FIXED_ACCOUNTS + 2 + count` ilerlet
   - Her order kendi PDA'sı + sell token account + count adet destination tutturuyor

**Amount taşması koruması (satır 421-422)**:
```rust
amount_withdrawn_delta = amount_withdrawn_delta.checked_add(u64::from_be_bytes(*amount))?;
```
Stream untrusted — overflow durumunda `None` return et.

### `relevant_instructions()` — CPI Traverse (satır 540-618)

**En algoritmik fonksiyon.** Tüm transaction'ı (top-level + inner/CPI) traverse ederek sadece settlement/SolFlow instruction'larını toplar.

**Algorithm:**
```
İçin her top-level instruction[index]:
  1. program_id_index → resolve → settlement/solflow? Keep; else Drop
  2. Bu top-level'ın inner instruction group'unu bul (group.index == instruction_index)
  3. İçin her inner instruction in group (DFS, execution order):
     a. stack_height → depth (1-indexed): height - 1
     b. depth > path.len() → path'i extend et (daha derin nest)
     c. depth <= path.len() → path'i truncate et ve son sibling'i artır
     d. program_id resolve → tracked? Keep
```

**Sibling tracking mantığı (satır 593-601)**:
```rust
if depth > path.len() {
    path.resize(depth, 0);
} else {
    path.truncate(depth);
    if let Some(last) = path.last_mut() {
        *last = last.saturating_add(1);
    }
}
```
Drop edilen (untracked) instruction'lar da counter'ı ilerletiyor. Bu sayede kept sibling'lar gerçek pozisyonlarını koruyor.

**MAX_CPI_DEPTH = 4** (satır 531): Solana stack derinlik limiti 5 (top-level = 1) → CPI path max 4 derin.

### `build_account_keys()` — Account List Reconstruction (satır 459-482)

```rust
fn build_account_keys(tx: &SubscribeUpdateTransactionInfo) -> Vec<Pubkey>
```

Üç bölgeyi birleştir:
1. `message.account_keys` (statik)
2. `meta.loaded_writable_addresses` (ALT writable)
3. `meta.loaded_readonly_addresses` (ALT readonly)

Yanlış uzunluktaki key → `Pubkey::default()` (zero pubkey). Bu şekilde index alignment bozulmuyor.

### `to_sdk_pubkey()` — Pubkey Bridge (satır 451-453)

```rust
fn to_sdk_pubkey(pubkey: InterfacePubkey) -> Pubkey {
    Pubkey::new_from_array(pubkey.to_bytes())
}
```

`settlement-interface` ve indexer farklı `solana-pubkey` major versiyonlarına bağlı — byte round-trip dönüşüm.

### DEX Uygulama Notu — Decoder Architecture

**CoW'un decoder'ından Nova Shield DEX için öğrenilecekler:**

1. **Pure decode functions**: `decode_settlement(instructions, ctx, resolve_order)` — side-effect yok, test edilmesi kolay. Nova Shield DEX event decoder'ı aynı şekilde pure olmalı.

2. **Instruction pairing pattern**: `BeginSettle` → `FinalizeSettle` pairing, multi-step settlement'ı atomik event olarak model ediyor. Nova Shield DEX'te "order match" + "settlement confirm" gibi paired instruction'lar için aynı pattern.

3. **CPI traversal**: Settlement genellikle CPI olarak çağrılıyor (direct call değil). Nova Shield DEX de composable yapıda olacaksa, CPI instruction'ları yakalamak için benzer DFS traversal gerekli.

4. **Wire format parsing**: BeginSettle body'si hand-parsed. `as_chunks::<8>()` gibi güvenli slice operasyonları, checked_add ile overflow koruması.

5. **Account resolution lazy**: `accounts: Vec<u8>` (raw indices) instruction'da tutulur, pubkey resolution decode aşamasında yapılır. Bu lazy resolution pattern bellek tasarruflu.

---

## Dosya 20: `src/indexer/decoder/tests.rs` — 441 satır

### Amaç
Decoder'ın tüm kritik code path'lerini kapsamlı birim testleriyle doğrular.

### Test Yardımcıları (satır 41-98)

`pubkey(n: u8) -> Pubkey`: `[n; 32]` bytes
`key_bytes(key: Pubkey) -> Vec<u8>`: Pubkey → byte vec
`compiled(program_id_index, accounts, data)`: CompiledInstruction builder
`inner(program_id_index, accounts, data, stack_height)`: InnerInstruction builder
`tx_info(account_keys, loaded_writable, loaded_readonly, instructions, inner_instructions)`: Full tx fixture builder

### Test 1: `resolves_settlement_and_solflow_across_top_level_and_cpi` (satır 103-149)

**Senaryo**: Settlement CPI olarak çağrılıyor, SolFlow top-level, ALT-loaded accounts.

```
Account list: [router(0), acct_a(1)] ++ [settlement(2)] ++ [solflow(3), acct_b(4)]
Top-level: [router_call, solflow_call]
Inner: settlement CPI under router_call
```

**Doğruladığı şeyler:**
- ALT index resolution (2, 3) doğru çalışıyor
- CPI'dan gelen settlement yakalanıyor
- Top-level SolFlow yakalanıyor
- Router (untracked) drop ediliyor
- Execution order: CPI önce (top-level 0'ın altında), solflow sonra (top-level 1)
- inner_ix_path: settlement `[0]`, solflow `[]`

### Test 2: `unresolvable_programs_dropped_account_indices_carried_through` (satır 155-185)

**Senaryo**: Out-of-range program index, malformed key (5 bytes → zero pubkey).

**Doğruladığı şeyler:**
- Index 9 (out of range) → drop
- 5-byte key → zero pubkey → untracked → drop
- Geçerli settlement instruction: account indices raw olarak carry edilir (bad index drop'a yol açmaz)

### Test 3: `inner_ix_path_tracks_cpi_nesting_depth` (satır 190-228)

**Senaryo**: 2 seviyeli nest, aralarında untracked program.

```
top-level: router (dropped)
  CPI depth 2 (path=[0]): settlement (kept)
    CPI depth 3 (path=[0,0]): other (DROPPED)
    CPI depth 3 (path=[0,1]): settlement (kept - counter advanced by dropped)
  CPI depth 2 (path=[1]): solflow (kept)
```

**Kritik doğrulama**: Drop edilen CPI (`other`, `[0,0]`) sibling counter'ı ilerletiyor, bu yüzden sonraki settlement `[0,1]` oluyor (false pozitif yok).

### Test 4: `corrupt_stack_height_is_clamped` (satır 232-251)

**Senaryo**: stack_height = 10,000 (corrupt data).

**Doğrulama**: `depth.clamp(1, MAX_CPI_DEPTH)` → 4 ile cap. Path `[0, 0, 0, 0]` — 9999 element array allocation yok.

### Test 5: `create_order_decodes_to_order_created` (satır 303-355)

**Senaryo**: Gerçekçi `CreateOrder` instruction, interface'in hash fonksiyonu ile bağımsız doğrulama.

```rust
let intent = OrderIntent {
    owner: InterfacePubkey::new_from_array([0x11; 32]),
    buy_token_account: InterfacePubkey::new_from_array([0x22; 32]),
    sell_token_account: InterfacePubkey::new_from_array([0x33; 32]),
    sell_amount: 1_000,
    buy_amount: 2_000,
    valid_to: 42,
    kind: OrderKind::Sell,
    partially_fillable: false,
    app_data: [0x44; 32],
};
```

**Doğruladığı şeyler:**
- `order_uid = intent.uid()` (decoder ve test aynı hash fonksiyonunu kullanıyor)
- `owner = [0x11; 32]` (intent'ten, account list'ten değil)
- `created_by = pubkey(12)` (account list index 1'den)

### Test 6: `begin_and_finalize_settle_decode_to_settlement_finalized` (satır 361-440)

**Senaryo**: BeginSettle + FinalizeSettle pair, gerçekçi wire format.

**BeginSettle body**:
```
[discriminator][finalize_index: 1u16 BE][order_count: 1][bump: 0xAA][transfer_count: 2][300u64 BE][700u64 BE]
```

**Beklenen TradeDelta**: `amount_withdrawn_delta = 300 + 700 = 1000`

**Doğruladığı şeyler:**
- finalize_ix_index cross-reference doğru çalışıyor
- BeginSettle body parsing: bump skip, count-guided amount summing
- Solver = account_keys[0] (fee payer)
- resolve_order closure ile UID injection
- PLACEHOLDER değerleri (auction_id=0, amount_received=0)

---

## Dosya 21: `src/indexer/ingester/tests.rs` — 219 satır

### Test Yardımcıları (satır 38-104)

`signature(n: u8)`: `[n; 64]` Signature
`tx_update(slot, sig)`: Transaction stream item
`account_update(slot, sig)`: Account stream item
`slot_update(slot)`: Slot stream item
`update_of(update: UpdateOneof)`: Generic wrapper
`ingester(stream)`: Test ingester factory → `(Ingester, Receiver, Arc<AtomicU64>)`

### Test 1: `transaction_update_with_valid_signature_is_forwarded` (satır 106-117)
Geçerli 64-byte signature → `StreamUpdate::Tx` olarak forward edilir.

### Test 2: `account_update_is_ignored` (satır 121-127)
Account update → kanal boş kalır. Subscription filter doğru.

### Test 3: `slot_update_advances_latest_chain_slot` (satır 129-135)
`slot_update(9_001)` → `latest_chain_slot == 9001`. `AtomicU64::fetch_max` doğrulanıyor.

### Test 4: `unrelated_and_empty_updates_are_ignored` (satır 137-159)
Ping, Pong, TransactionStatus, Block, BlockMeta, Entry → tümü ignored.
Son item tx_update(7, 3) → sadece o forward edilir.

### Test 5: `transaction_without_body_or_malformed_signature_is_skipped` (satır 161-191)
- `transaction: None` → skip
- `signature: [1,2,3]` (3 byte) → skip
- Geçerli tx → forward

### Test 6: `terminal_grpc_status_returns_stream_error` (satır 193-202)
`Status::invalid_argument("boom")` → `Err(Error::Stream(status))` döner. Code ve message korunur.

### Test 7: `clean_stream_end_returns_stream_ended` (satır 204-210)
Boş stream → `Err(Error::StreamEnded)`.

### Test 8: `closed_decoder_receiver_stops_cleanly` (satır 212-218)
Receiver drop edilmiş → tx_update gönderilmeye çalışılır → `Ok(())` (break on closed channel).

---

## Bağımlılık Analizi (Cargo.toml)

```toml
settlement-interface = {
  git = "https://github.com/cowprotocol/solana-programs",
  rev = "4e0bd7eaed7c237a327705425be2a4a50a8b4a3a"
}
```

**Pin edilmiş git revision**: `settlement-interface` crate'i CoW Protocol'ün ayrı bir Solana programs reposundan geliyor. Revision pin'i, program değişikliklerinin indexer'ı sessizce kırmasını önlüyor.

Diğer kritik bağımlılıklar:
- `yellowstone-grpc-client`: Yellowstone gRPC streaming (AutoReconnect, subscription management)
- `yellowstone-grpc-proto`: Wire format proto types
- `solana-client`: RPC calls (getSignatureStatuses, getTransaction, vb.)
- `solana-sdk`: Pubkey, Signature core types

---

## Genel Mimari Değerlendirmesi

### Sistem Akışı (End-to-End)

```
Yellowstone gRPC Node
        |
        | (AutoReconnect-backed GeyserStream)
        v
[Ingester::run()]
  - stream.next().await
  - Slot messages → AtomicU64::fetch_max
  - Tx messages → mpsc channel (capacity: 1024)
        |
        | (tokio mpsc)
        v
[Decoder::run()]
  - rx.recv().await
  - relevant_instructions() → filter settlement/solflow
  - decode_settlement() → Vec<SettlementEvent>
  - decode_solflow() → (stub)
  - persist_events() → (stub, todo!())
        |
        | (PostgreSQL)
        v
[Database: solana.* tables]
  - commitment: 'confirmed'

[FinalizationWorker::run()] (timer-based)
  - get_confirmed_rows(tip - 32) → getSignatureStatuses (batch 256)
    - finalized? → update_commitment(Finalized)
  - get_aged_rows(tip - 150) → getTransaction (one per row)
    - Some → Finalized, None → RolledBack
```

### İki Program Mimarisi

**Settlement Program** (on-chain order book):
- `CreateOrder`: Intent hash + owner → PDA
- `BeginSettle` + `FinalizeSettle`: Atomik settlement (pair)
- `CreateBuffer`: Liquidity buffer PDA
- `CancelOrder`, `CloseOrder`: Lifecycle

**SolFlow Program** (SOL wrapper):
- `CreateSolFlowOrder`: Custodial PDA + wSOL deposit
- `SetUpOrder`: wSOL custody'sini settlement'a transfer et
- Recovery instructions

### Eksiklikler ve TODO'lar

1. **persistence adapter**: Tüm `persist_events`, `write_watermark` vb. `todo!()` — v0.1 feature gap
2. **decode_solflow**: SolFlow instruction decode stub
3. **FinalizationWorker::run()**: `unimplemented!()` — PR 11-12'de gelecek
4. **main.rs**: Sadece println — real startup logic yok
5. **PLACEHOLDER_AUCTION_ID**: BeginSettle wire henüz auction_id taşımıyor
6. **PLACEHOLDER_AMOUNT_RECEIVED**: FinalizeSettle henüz buy-side push amount taşımıyor

---

## DEX Uygulama Notu — Nova Shield DEX için Kapsamlı Eylem Planı

### 1. Indexer Mimarisi (Ingester-Decoder Ayrımı)

**Ne**: Ingester decode yapmaz, sadece drain eder; Decoder decode eder, stream görmez.
**Neden önemli**: Decoder yavaşlarsa ingester socket'i bloke etmez — veri kaybı yok.
**Nova Shield DEX için**: DEX event indexer'ını aynı şekilde iki katmana ayır. L1 validator'dan gelen event'ler → ingester buffer (mpsc 1024+) → decoder → PostgreSQL.

### 2. Commitment-Aware Event Processing

**Ne**: Event'ler önce `confirmed`, sonra `finalized` olarak işaretleniyor. İki aşamalı finalization.
**Neden önemli**: Rollback risk. `confirmed` block reorg edilebilir.
**Nova Shield DEX için**: Order fill event'lerini sadece `finalized` commitment'tan sonra execute etmiş say. MEV koruması için kritik — `confirmed` görünen settlement'lar geri alınabilir.

### 3. Delta-Based Trade Accounting

**Ne**: `TradeDelta` absolute değil, snapshot-to-snapshot delta.
**Neden önemli**: Partial fill ve re-entrancy durumlarında tutarlılık.
**Nova Shield DEX için**: Order muhasebesi için `amount_filled_delta` + `amount_received_delta` pattern. Her settlement event'i önceki snapshot'tan delta taşısın.

### 4. Solver Performance Tracking

**Ne**: Her `SettlementFinalized` event'i solver address'ini taşıyor (fee payer = solver).
**Neden önemli**: Solver kalite kontrolü, jailing mekanizması.
**Nova Shield DEX için**: `SettlementEvent` içinde `solver: Address` zorunlu. Solver → settlement sayısı, solver → failed settlement, solver → profitability metrikleri track et.

### 5. Replay Window Yönetimi

**Ne**: `ReplayWindowExceeded` hatası, `record_lost_slot_range()`.
**Neden önemli**: Indexer restart sonrası gap oluşabilir, bu gap kayıt altına alınmalı.
**Nova Shield DEX için**: Indexer reconnect logic'e gap detection ekle. Kaybedilen aralığı `lost_ranges` tablosuna yaz, dashboard'da göster. Backfill endpoint'i ile gap'leri doldurun.

### 6. PDA Determinism — Hash(Intent) = UID

**Ne**: OrderUid = hash(order_intent). PDA seed = `["settlement", uid_bytes, "order"]`.
**Neden önemli**: Stateless order tracking. Order state her zaman aynı adreste.
**Nova Shield DEX için**: Her DEX order'ı için deterministic PDA türet: `pda = hash(order_params)`. Order state değişikliklerini same address üzerinde izle. Off-chain ve on-chain state her zaman sync.

### 7. CPI (Cross-Program Invocation) Awareness

**Ne**: `relevant_instructions()` top-level + CPI instructions'ı traverse ediyor. Settlement çoğunlukla CPI olarak çağrılıyor.
**Neden önemli**: Composable DEX protokolleri settlement'ı CPI ile çağırır. Top-level filter yetersiz.
**Nova Shield DEX için**: DEX indexer'ı hem direct call hem CPI instruction'larını yakalamalı. `inner_instructions` grupları DFS ile traverse et. Max CPI depth sabiti ile stack overflow koruması.

### 8. BeginSettle + FinalizeSettle Pairing — Atomik Settlement

**Ne**: Settlement iki instruction'dan oluşuyor. BeginSettle body'si finalize index taşıyor.
**Neden önemli**: Settlement atomikliği iki separate on-chain instruction ile sağlanıyor.
**Nova Shield DEX için**: Multi-step settlement'ta instruction pairing pattern kullan. Her `BeginXxx` kendi `FinalizeXxx`'ini index ile işaret etsin. Indexer sadece tamamlanan çiftleri event olarak emit etsin.

---

## Özet Tablo: Dosya → Satır Sayısı → İşlev

| Dosya | Satır | İşlev |
|---|---|---|
| `lib.rs` | 8 | Modül kökü |
| `main.rs` | 3 | Binary stub |
| `types/mod.rs` | 13 | Domain tip namespace |
| `types/slot.rs` | 11 | Slot newtype |
| `types/order.rs` | 27 | OrderUid (hash-based) |
| `types/events.rs` | 161 | Settlement + SolFlow event taxonomy |
| `types/tx.rs` | 53 | ResolvedInstruction, TxContext |
| `types/commitment.rs` | 82 | Confirmed/Finalized/RolledBack tracking |
| `types/errors.rs` | 58 | DecodeError, PersistenceError, StreamError |
| `types/channel.rs` | 22 | Ingester→Decoder kanal mesajı |
| `types/wire.rs` | 31 | yellowstone-grpc-proto re-export |
| `types/recovery.rs` | 35 | PdaSnapshot, GetSignaturesOpts |
| `persistence.rs` | 105 | PostgreSQL stub API |
| `traits/mod.rs` | 3 | Trait namespace |
| `traits/solana_client.rs` | 44 | RPC trait (SolanaClient) |
| `indexer/mod.rs` | 27 | Component mimarisi dokümantasyonu |
| `indexer/finalization.rs` | 66 | FinalizationWorker skeleton |
| `indexer/ingester.rs` | 361 | **Yellowstone gRPC drain loop** |
| `indexer/decoder.rs` | 622 | **Instruction decode engine** |
| `indexer/decoder/tests.rs` | 441 | Decoder unit testleri |
| `indexer/ingester/tests.rs` | 219 | Ingester unit testleri |
| **TOPLAM** | **~2.393** | |

---

## Sonuç

`solana-indexer`, CoW Protocol'ün EVM-native batch auction mekanizmasını Solana'ya taşıma stratejisinin on-chain veri okuma altyapısıdır. Crate v0.1 durumunda — Ingester ve Decoder gerçek implementasyona sahip, ancak persistence adapter ve FinalizationWorker henüz stub. Mimari tasarım mükemmel: Ingester-Decoder ayrımı, generic stream parametresi (test edilebilirlik), pure decode fonksiyonları, iki seviyeli finalization, CPI-aware instruction traversal ve delta-based trade accounting. Nova Shield DEX indexer mimarisi için bu crate doğrudan referans modeli olarak alınabilir.
