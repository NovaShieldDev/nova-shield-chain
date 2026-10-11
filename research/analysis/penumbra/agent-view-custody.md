# Penumbra Analizi — View, Wallet, Custody, Ledger USB, Misc, Cnidarium-Component

**Analiz Tarihi:** 2026-07-22
**Kapsam:** view/ (14 dosya) + wallet/ (3 dosya) + custody/ (10 dosya) + custody-ledger-usb/ (2 dosya) + misc/ (10 dosya) + cnidarium-component/ (3 dosya)
**Toplam:** ~42 dosya, ~6500+ satır Rust kodu

---

## KISIM 1: VIEW CRATE

### [lib.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/lib.rs`
**Satır Sayısı:** 37

### Genel Amaç

View crate'in kök modülü. Tüm public API'yi re-export eder. Global direktif: `#![deny(clippy::unwrap_used)]` — tüm crate'te `unwrap()` yasak, tüm hatalar `anyhow::Result` ile handle edilmek zorunda.

### Public Re-Exports

- `ViewClient` — trait, istemci arayüzü
- `register_metrics` — Prometheus metrik kaydı
- `SpendableNoteRecord` — harcanabilir note kaydı
- `Planner` — transaction planlama motoru
- `ViewServer` — gRPC sunucu implementasyonu
- `StatusStreamResponse` — sync durum akışı
- `Storage` — SQLite kalıcı depolama
- `SwapRecord` — swap kaydı
- `TransactionInfo` — transaction bilgisi

### Tasarım Kararları

- `#![recursion_limit = "512"]` — derin generic/async ağaçları için
- `#[cfg_attr(docsrs, feature(doc_auto_cfg))]` — docs.rs için özellik etiketleme

### Nova Shield Chain İçin Dersler

- View-only mode ile tam node arasında net separation: ViewClient trait'i bu sınırı çizer
- `#![deny(clippy::unwrap_used)]` kendi crate'lerimizde de zorunlu yapılmalı

---

### [client.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/client.rs`
**Satır Sayısı:** 1100

### Genel Amaç

`ViewClient` trait tanımı ve `ViewServiceClient<T>` üzerindeki implementasyonu. Client-server ayrımını açıkça ifade eden asenkron trait: her metot `Pin<Box<dyn Future<...>>>` döndürür.

### Trait Tanımı: `ViewClient`

```rust
pub trait ViewClient {
    fn auctions(...) -> Pin<Box<dyn Future<...>>>;
    fn status(...) -> Pin<Box<dyn Future<...>>>;
    fn status_stream(...) -> Pin<Box<dyn Future<...>>>;
    fn app_params(...) -> Pin<Box<dyn Future<...>>>;
    fn gas_prices(...) -> Pin<Box<dyn Future<...>>>;
    fn fmd_parameters(...) -> Pin<Box<dyn Future<...>>>;
    fn notes(request: pb::NotesRequest) -> Pin<Box<dyn Future<...>>>;
    fn notes_for_voting(...) -> Pin<Box<dyn Future<...>>>;
    fn balances(address_index, asset_id) -> Pin<Box<dyn Future<...>>>;
    fn note_by_commitment(...) -> Pin<Box<dyn Future<...>>>;
    fn swap_by_commitment(...) -> Pin<Box<dyn Future<...>>>;
    fn nullifier_status(...) -> Pin<Box<dyn Future<...>>>;
    fn await_nullifier(...) -> Pin<Box<dyn Future<...>>>;
    fn await_note_by_commitment(...) -> Pin<Box<dyn Future<...>>>;
    fn witness(plan: &TransactionPlan) -> Pin<Box<dyn Future<...>>>;
    fn witness_and_build(plan, auth_data) -> Pin<Box<dyn Future<...>>>;
    fn assets(...) -> Pin<Box<dyn Future<...>>>;
    fn owned_position_ids(...) -> Pin<Box<dyn Future<...>>>;
    fn transaction_info_by_hash(...) -> Pin<Box<dyn Future<...>>>;
    fn transaction_info(start_height, end_height) -> Pin<Box<dyn Future<...>>>;
    fn broadcast_transaction(...) -> BroadcastStatusStream;
    fn address_by_index(...) -> Pin<Box<dyn Future<...>>>;
    fn index_by_address(...) -> Pin<Box<dyn Future<...>>>;
    fn unclaimed_swaps(...) -> Pin<Box<dyn Future<...>>>;
    fn lqt_voting_notes(epoch, filter) -> Pin<Box<dyn Future<...>>>;
}
```

### Default Metodlar (trait'te implement edilmiş)

#### `fn unspent_notes_by_address_and_asset(&mut self)` (Satır 212–246)
- `notes(include_spent: false)` çağrısı + `BTreeMap<AddressIndex, BTreeMap<asset::Id, Vec<SpendableNoteRecord>>>` olarak gruplama
- Key: AddressIndex (tam, randomizer dahil)

#### `fn unspent_notes_by_account_and_asset(&mut self)` (Satır 251–284)
- Aynı ama key: `u32` (account number) — ephemeral addressleri ana hesapla gruplayarak not kaybını önler
- `record.address_index.account` ile gruplama — önemli fark

#### `fn unspent_notes_by_asset_and_address(&mut self)` (Satır 288–324)
- Asset-first gruplama: `BTreeMap<asset::Id, BTreeMap<AddressIndex, Vec<SpendableNoteRecord>>>`

### ViewServiceClient Implementasyonu (Satır 357–1100)

Tüm trait metodları `ViewServiceClient<T>` için implement edilmiş. Ortak pattern:
1. `let mut self2 = self.clone()` — Clone required
2. `async move { ... }.boxed()` — owned future
3. gRPC protobuf → domain type dönüşümü

Kritik metodlar:

#### `fn witness_and_build` (Satır 917–959)
- `witness_and_build` gRPC stream döndürür
- Stream mesajlarını bekler: `BuildProgress` (ilerleme) veya `Complete` (bitti)
- Progress updates TODO olarak işaretlenmiş — henüz UI'a yansıtılmıyor

#### `fn auctions` (Satır 989–1070)
- `(AuctionId, SpendableNoteRecord, u64, Option<Any>, Vec<Position>)` tuple dönüşümü
- `.filter_map(|res| res.ok())` — hata toleransı: başarısız auction parse'ları sessizce atlanıyor. TODO yorumu var: "scrap this later"

#### `fn lqt_voting_notes` (Satır 1072–1099)
- LQT = Liquidity Tournament (likidite yarışması voting sistemi)
- `epoch_index: u64` — hangi epoch için oy kullanılacak
- `account_filter: Option<AddressIndex>`

### Tip Tanımları

```rust
pub(crate) type BroadcastStatusStream = Pin<
    Box<dyn Future<Output = Result<Streaming<BroadcastTransactionResponse>, anyhow::Error>> + Send>,
>;
```

### Unsafe Bloklar

Yok.

### Test'ler

Yok (integration testler ayrı).

### Nova Shield Chain İçin Dersler

- View protocol pattern: FVK yeterli — spend key gerektirmez. View-only mode için bu ayrım kritik
- `account` vs `address_index` farkını koddan öğren: ephemeral adresler farklı index ama aynı account
- Auctions response'da `filter_map(|res| res.ok())` ile hata yumuşatma: upstream bug varsa istemciyi kilitlemez ama kayıp da olabilir
- gRPC + domain types pattern: her proto metodu etrafına type-safe wrapper, `try_into()` ile

---

### [note_record.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/note_record.rs`
**Satır Sayısı:** 117

### Genel Amaç

`SpendableNoteRecord` — zincirden taranan, harcanabilir durumda olan bir note'un tam kaydı.

### Struct Tanımı

```rust
pub struct SpendableNoteRecord {
    pub note_commitment: note::StateCommitment,  // note'un commitment hash'i
    pub note: Note,                              // plaintext note (adres + miktar + asset)
    pub address_index: AddressIndex,             // hangi adres indeksine ait
    pub nullifier: Nullifier,                    // harcandığında reveal edilecek nullifier
    pub height_created: u64,                     // oluşturulduğu blok
    pub height_spent: Option<u64>,               // harcandığı blok (None = henüz harcanmamış)
    pub position: Position,                      // TCT'deki konumu (blok/epoch indeksi)
    pub source: CommitmentSource,                // nereden geldi (tx hash veya chain genesis)
    pub return_address: Option<AddressView>,     // gönderenin iade adresi (varsa)
}
```

### Trait Implementasyonları

- `DomainType` → Proto: `pb::SpendableNoteRecord`
- `From<SpendableNoteRecord>` for `pb::SpendableNoteRecord`
- `TryFrom<pb::SpendableNoteRecord>` for `SpendableNoteRecord`
- `TryFrom<&Row<'_>>` for `SpendableNoteRecord` — SQLite satırından doğrudan dönüşüm

### SQLite Dönüşümü (Satır 82–117)

`TryFrom<&Row<'_>>` implementasyonu çok önemli:

```rust
note: Note::from_parts(
    row.get::<_, Vec<u8>>("address")?[..].try_into()?,
    Value {
        amount: u128::from_be_bytes(row.get::<_, [u8; 16]>("amount")?).into(),
        asset_id: row.get::<_, Vec<u8>>("asset_id")?[..].try_into()?,
    },
    Rseed(row.get::<_, [u8; 32]>("rseed")?),
)?,
```

- `amount`: 16 byte big-endian u128 olarak saklanıyor
- `rseed`: 32 byte, note'un blinding faktörü
- `return_address`: binary olarak saklanıyor, Address::try_from ile çözümleniyor

### Güvenlik Notu

`height_spent = 0` proto'da "harcanmamış" anlamına gelir. Kodda:
```rust
height_spent: if v.height_spent > 0 { Some(v.height_spent) } else { None }
```
Proto'daki 0 değeri "None" olarak kodlanıyor — dikkatli: height 0 gerçek bir blok yüksekliği olabilir mi? Ama genesis'te note harcanamaz, o yüzden bu tamam.

### Nova Shield Chain İçin Dersler

- Note kaydı = commitment + note plaintext + nullifier + position + meta
- Amount'un u128 big-endian binary olarak saklanması yaygın pattern
- `height_spent: Option<u64>` — `None` = unspent guard, kritik sentinel değer
- `return_address` opt-in: IBC çekimlerinde gönderici iade adresini açıklıyor olabilir

---

### [swap_record.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/swap_record.rs`
**Satır Sayısı:** 85

### Genel Amaç

`SwapRecord` — kullanıcının gerçekleştirdiği batch swap işleminin kaydı. ZK swap iddiasını daha sonra claim etmek için gerekli tüm bilgiyi içerir.

### Struct Tanımı

```rust
pub struct SwapRecord {
    pub swap_commitment: tct::StateCommitment,  // swap'ın TCT commitment hash'i
    pub swap: SwapPlaintext,                    // swap'ın plaintext içeriği (trading pair, delta1/2, claim fee, address)
    pub position: tct::Position,                // TCT pozisyonu
    pub nullifier: Nullifier,                   // swap null etmek için gerekli
    pub output_data: BatchSwapOutputData,       // batch swap sonuçları (fiyat, çıktı miktarı)
    pub height_claimed: Option<u64>,            // claim edildiği blok (None = henüz claim edilmedi)
    pub source: CommitmentSource,               // kaynak işlem
}
```

### SQLite Dönüşümü

```rust
output_data: BatchSwapOutputData::decode(&row.get::<_, Vec<u8>>("output_data")?[..])?,
swap: SwapPlaintext::decode(&row.get::<_, Vec<u8>>("swap")?[..])?,
```
Bu alanlar protobuf binary olarak saklanıyor — `output_data` ve `swap` alanları için schema'da `BLOB` sütunları.

### Nova Shield Chain İçin Dersler (DEX Uygulama Notu)

- Swap kaydı: user tarafında batch swap'ın tam durumu
- `height_claimed: None` = swap claim edilmeyi bekliyor → wallet sweep işlemi bu kontrolle çalışıyor
- `BatchSwapOutputData` içinde gerçek execution fiyatı var: bu veriyi sonradan slippage analizi için kullanabiliriz
- Swap + Claim iki ayrı işlem: swap commit aşaması, ardından `SwapClaim` action ile output note'ları claim et

---

### [status.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/status.rs`
**Satır Sayısı:** 34

### Genel Amaç

Sync durumu için stream response tipi.

### Struct Tanımı

```rust
#[derive(Clone, Copy, Debug)]
pub struct StatusStreamResponse {
    pub latest_known_block_height: u64,  // fullnode'un bildiği en son blok
    pub full_sync_height: u64,           // tam tarama tamamlanan son blok
    pub partial_sync_height: u64,        // kısmi tarama (şu an full == partial)
}
```

### Nova Shield Chain İçin Dersler

- 3 farklı "height" kavramı: chain tip, full scan, partial scan
- `partial_sync_height = full_sync_height` şimdilik — ileride partial scanning (kısmi tarama) eklenebilir
- Status stream: istemci UI'ı bu stream'i dinler ve sync progress gösterir

---

### [metrics.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/metrics.rs`
**Satır Sayısı:** 31

### Genel Amaç

Crate genelinde metrik kayıt sistemi. Şu anda placeholder — aktif metrik yok.

### Tasarım Notu

`metrics` crate'ini shadow ederek kullanım: `use crate::metrics;` ile crate'in kendi metrik tanımları `metrics::` namespace'inde görünür. Gelecekte prometheus counter/gauge eklemek için scaffold.

---

### [transaction_info.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/transaction_info.rs`
**Satır Sayısı:** 19

### Struct Tanımı

```rust
pub struct TransactionInfo {
    pub height: u64,                          // bloktaki yüksekliği
    pub id: TransactionId,                    // tx hash
    pub transaction: Transaction,             // ham tx verisi
    pub perspective: TransactionPerspective,  // FVK ile görülen şifreli alanlar
    pub view: TransactionView,                // perspective'den hesaplanan kullanıcı görünümü
    pub summary: TransactionSummary,          // kısa bakiye özeti (giren/çıkan)
}
```

### Tasarım Notu

`perspective` + `view` ayrımı kritik: `perspective` ham decrypted data, `view` ise bundan türetilen UI-friendly representation. FVK olmayan bir client `view`'i doğrudan render edebilir.

---

### [sync.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/sync.rs`
**Satır Sayısı:** 241

### Genel Amaç

`scan_block` — tek bir compact block'u FVK ile tararken hem note'ları şifresini çözen hem de TCT'yi güncelleyen çekirdek fonksiyon.

### Struct Tanımı

```rust
pub struct FilteredBlock {
    pub new_notes: BTreeMap<StateCommitment, SpendableNoteRecord>,   // bu blokta bulunan bizim note'larımız
    pub new_swaps: BTreeMap<StateCommitment, SwapRecord>,            // bu blokta bulunan bizim swap'larımız
    pub spent_nullifiers: Vec<Nullifier>,                            // bu blokta harcanan nullifier'larımız
    pub height: u64,
    pub fmd_parameters: Option<fmd::Parameters>,
    pub app_parameters_updated: bool,
    pub gas_prices: Option<GasPrices>,
}
```

### `scan_block` Fonksiyonu (Satır 27–241)

**İmza:**
```rust
pub async fn scan_block(
    fvk: &FullViewingKey,
    state_commitment_tree: &mut tct::Tree,
    compact_block: CompactBlock,
    storage: &Storage,
) -> anyhow::Result<FilteredBlock>
```

**Algoritma (Satır 46–241):**

1. **Paralel Trial Decrypt (Satır 47–65):**
   ```rust
   let trial_decrypt_note = |note_payload: NotePayload| -> JoinHandle<Option<Note>> {
       let fvk2 = fvk.clone();
       tokio::spawn(async move { note_payload.trial_decrypt(&fvk2) }.instrument(...))
   };
   ```
   Her note payload için ayrı tokio task spawn eder. Bu kritik performans optimizasyonu: tüm note'ları paralel decrypt et.

2. **State Payload Sınıflandırması (Satır 75–85):**
   ```rust
   for payload in state_payloads.iter() {
       match payload {
           StatePayload::Note { note, .. } => { note_decryptions.push(trial_decrypt_note(...)); }
           StatePayload::Swap { swap, .. } => { swap_decryptions.push(trial_decrypt_swap(...)); }
           StatePayload::RolledUp { commitment, .. } => { unknown_commitments.push(*commitment); }
       }
   }
   ```
   3 payload tipi: Note, Swap, RolledUp (önceki note'ların sıkıştırılmış versiyonu)

3. **RolledUp Advice Sistemi (Satır 87–95):**
   ```rust
   let mut note_advice = storage.scan_advice(unknown_commitments).await?;
   ```
   RolledUp commitment'lar için storage'dan önceki tarama sonuçlarını sor. Bu "advice" sistemi swap output note'larının tespiti için kritik.

4. **TCT Güncelleme Dallanması (Satır 111–209):**

   **Optimizasyon yolu (boş blok):**
   ```rust
   if note_advice.is_empty() && swap_advice.is_empty() {
       state_commitment_tree.insert_block(block_root)...
   }
   ```
   Bize ait note yoksa tüm bloğu tek seferde TCT'ye ekle (O(1) yerine O(n) değil).

   **Detaylı yol (bizim note'larımız var):**
   ```rust
   for payload in state_payloads.into_iter() {
       match (note_advice.get(payload.commitment()), swap_advice.get(payload.commitment())) {
           (Some(note), None) => {
               let position = state_commitment_tree.insert(tct::Witness::Keep, *payload.commitment())?;
               let nullifier = Nullifier::derive(fvk.nullifier_key(), position, payload.commitment());
               let address_index = fvk.incoming().index_for_diversifier(note.diversifier());
               new_notes.insert(...);
           }
           (None, Some(swap)) => {
               let position = state_commitment_tree.insert(tct::Witness::Keep, *payload.commitment())?;
               // Swap output note'larını önceden storage'a kaydet
               let (output_1, output_2) = swap.output_notes(&output_data);
               storage.give_advice(output_1).await?;
               storage.give_advice(output_2).await?;
               new_swaps.insert(...);
           }
           (None, None) => {
               // Bizim değil — Witness::Forget ile TCT'ye ekle (silinebilir)
               state_commitment_tree.insert(tct::Witness::Forget, *payload.commitment())?;
           }
           (Some(_), Some(_)) => unreachable!("swap and note commitments are distinct"),
       }
   }
   ```

5. **Nullifier Filtresi (Satır 227):**
   ```rust
   let filtered_nullifiers = storage.filter_nullifiers(spent_nullifiers).await?;
   ```
   KRITIK OPTİMİZASYON: `// it results in 100x+ slower sync times if we don't do this!`
   Yalnızca bizim kontrolümüzdeki note'ların nullifier'larını işle.

6. **Epoch Sonu (Satır 212–218):**
   ```rust
   if epoch_root.is_some() {
       state_commitment_tree.end_epoch()?;
   }
   ```

7. **Güvenlik Notu — Passive Failure (Satır 161–169):**
   ```rust
   let Some(output_data) = swap_outputs.get(&swap.trading_pair).cloned() else {
       tracing::warn!("invalid compact block, batch swap output data missing...");
       continue;  // Sessizce devam et!
   };
   ```
   Fullnode bize sahte blok gönderirse (swap output verisi eksik), sessizce devam ediyoruz. Neden? Çünkü hata verseydik, fullnode bize özel veri gönderip timing analizi yapabilirdi.

### Unsafe Bloklar

Yok.

### Nova Shield Chain İçin Dersler (DEX Uygulama Notu)

- Trial decryption = paralel tokio task per note: Bu pattern kendi privacy L1'imizde temel scanning altyapısı
- **Advice sistemi**: Swap output note'ları henüz commit olarak görünmüyor, ama gelecekte geleceklerini biliyoruz. `give_advice` → `scan_advice` döngüsü bu bilgiyi taşıyor
- **Witness::Keep vs Witness::Forget**: TCT'de yalnızca kendi note'larımız için witness proof path tutuyoruz — bu ZK harcama için gerekli
- Nullifier filtresi: Blockchain scanning'de 100x performans farkı yaratıyor — bunu Nova Shield'da da uygula
- Passive failure: Hata sızıntıları (timing attacks) için kasıtlı sessiz failure

---

### [worker.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/worker.rs`
**Satır Sayısı:** 543

### Genel Amaç

`Worker` — arkaplan sync görevi. Compact block stream'i dinler, her bloğu `scan_block` ile işler, sonuçları storage'a yazar.

### Struct Tanımı

```rust
pub struct Worker {
    storage: Storage,
    sct: Arc<RwLock<penumbra_sdk_tct::Tree>>,    // paylaşımlı SCT (State Commitment Tree)
    fvk: FullViewingKey,
    error_slot: Arc<Mutex<Option<anyhow::Error>>>, // hata iletim kanalı
    sync_height_tx: watch::Sender<u64>,            // sync ilerleme kanalı
    channel: Channel,                              // gRPC kanal
}
```

**Sabit:** `const MAX_CB_SIZE_BYTES: usize = 12 * 1024 * 1024` — compact block max 12MB

### `Worker::new` (Satır 61–103)

- FVK'yı storage'dan yükle
- `Arc<RwLock<tct::Tree>>` oluştur — Worker + ViewServer SCT'yi paylaşıyor
- `watch::channel(last_sync_height)` — sync height değişimlerini bildir

### `Worker::sync` (Satır 187–450)

**Algoritma:**

1. **Stream aç:**
   ```rust
   stream = client.compact_block_range(CompactBlockRangeRequest {
       start_height: last_sync_height + 1,
       end_height: 0,        // 0 = sınırsız
       keep_alive: true,     // yeni bloklar gelince bildir
   });
   ```

2. **Buffer trick (Satır 215–222):**
   ```rust
   let (tx, mut buffered_stream) = tokio::sync::mpsc::channel(1000);
   tokio::spawn(async move {
       while let Some(block) = stream.message().await.transpose() {
           if tx.send(block).await.is_err() { break; }
       }
   });
   ```
   Stream tüketimini scanning'den ayırır. 2 amaç: (1) smoothing, (2) timing side-channel azaltma

3. **Height sıra kontrolü (Satır 230–234):**
   ```rust
   if height != expected_height { tracing::warn!("out of order block detected"); continue; }
   expected_height += 1;
   ```

4. **SCT write lock (Satır 237):**
   ```rust
   let mut sct_guard = self.sct.write().await;
   ```
   Her blok işlemi SCT write lock'u gerektiriyor. Read lock ile parallel sorgu yapılabilir.

5. **Boş blok optimizasyonu (Satır 250–264):**
   ```rust
   if !block.requires_scanning() {
       sct_guard.end_block()?;
       self.storage.record_empty_block(height).await?;
       self.sync_height_tx.send(height)?;
   } else { ... }
   ```
   Boş bloklar tek satırda tüketilir — tarihi sync hızı açısından kritik.

6. **LP NFT asset registration (Satır 272–370):**
   Blokta bulunan işlemlerden Position açılış/kapanış/çekim eylemlerini izleyerek:
   - LP NFT her durumu için ayrı asset kaydı (Opened/Closed/Withdrawn{sequence})
   - Auction NFT kaydı
   - `storage.record_position/update_position/record_auction_with_state`

7. **Bilinmeyen asset metadata sorgusu (Satır 374–422):**
   ```rust
   client.asset_metadata_by_id(AssetMetadataByIdRequest { asset_id: ... })
   ```
   Note'ta görülen bilinmeyen asset ID'ler için chain'e sorgu at.

8. **Block commit (Satır 424–434):**
   ```rust
   self.storage.record_block(filtered_block, transactions, &mut sct_guard, channel).await?;
   self.sync_height_tx.send(filtered_block.height)?;
   ```

9. **SCT divergence check (Satır 436–438):**
   ```rust
   #[cfg(feature = "sct-divergence-check")]
   sct_divergence_check(self.channel.clone(), height, sct_guard.root()).await?;
   ```
   Opsiyonel özellik: chain'den expected SCT root'u alıp local hesapla kıyasla.

### `Worker::run` (Satır 452–468)

```rust
pub async fn run(mut self) -> anyhow::Result<()> {
    loop {
        if let Err(e) = self.sync().await {
            tracing::error!(?e, "view worker error");
            self.error_slot.lock()?.replace(e);
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
        *self.error_slot.lock()? = None;
    }
}
```
Hata → 10 saniye bekle → retry. Exponential backoff TODO olarak işaretlenmiş.

### `fetch_transactions` (Satır 470–500)

```rust
let response = match client.transactions_by_height(request.clone()).await {
    Ok(rsp) => rsp,
    Err(e) => {
        tracing::warn!(?e, "failed to fetch block, waiting and retrying once");
        tokio::time::sleep(Duration::from_secs(1)).await;
        client.transactions_by_height(request).await?
    }
};
```
Load-balancer split-brain'e karşı: hata → 1 saniye bekle → tek retry. HACK olarak belgeleniyor.

### Unsafe Bloklar

Yok.

### Test'ler

Yok.

### Nova Shield Chain İçin Dersler

- Worker pattern: arkaplan senkronizasyon görevi, hata slot, watch channel ile ViewServer haberleşmesi
- 1000 kapasiteli mpsc channel buffering: scanning performansı için
- LP NFT state machine: her pozisyon değişikliği yeni NFT → wallet'ın bu state'i izlemesi gerekiyor
- Empty block optimization: tarihsel sync 100x hızlanabilir
- SCT divergence check: debug feature olarak çok değerli, production'da optional

---

### [planner.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/planner.rs`
**Satır Sayısı:** 716

### Genel Amaç

`Planner<R>` — Transaction planlarken gerekli spend'leri ve change output'ları otomatik ekleyen builder. Kullanıcının "swap et" veya "transfer et" demesi yeterli; Planner arka planda not seçimi yapıyor.

### Struct Tanımı

```rust
pub struct Planner<R: RngCore + CryptoRng> {
    rng: R,
    action_list: ActionList,
    fee_tier: FeeTier,
    gas_prices: Option<GasPrices>,
    transaction_parameters: TransactionParameters,
    change_address: Option<Address>,
    memo_text: Option<String>,
    memo_return_address: Option<Address>,
}
```

### Builder Metodları

- `action()` — genel action ekle
- `set_gas_prices()` — gas fiyatı set et
- `set_fee_tier()` — fee tier: LOW, MEDIUM, HIGH
- `expiry_height()` — işlem süre sınırı
- `memo()` — memo metni
- `memo_return_address()` — memo iade adresi
- `change_address()` — para üstü adresi (foreign address olabilir → "send max")
- `spend(note, position)` — belirli note'u harca
- `output(value, address)` — output ekle
- `position_open(position)` — LP pozisyonu aç
- `position_open_with_metadata(position, metadata)` — metadata ile LP aç
- `position_close(position_id)` — LP kapat
- `position_withdraw(position_id, reserves, pair, next_sequence)` — LP çek
- `dutch_auction_schedule(description)` — Dutch açık artırma planla
- `dutch_auction_end(auction_id)` — Açık artırmayı bitir
- `dutch_auction_withdraw(auction)` — Açık artırma rezervlerini çek (gerekirse önce end çağrılır)
- `swap(input_value, into_asset, claim_fee, claim_address)` — swap ekle
- `swap_claim(plan)` — swap claim ekle
- `delegate(epoch, amount, rate_data)` — delegation
- `undelegate(epoch, amount, rate_data)` — undelegation
- `undelegate_claim(claim_plan)` — undelegate claim
- `validator_definition(new_validator)` — validator tanımı güncelle
- `proposal_submit(proposal, deposit)` — governance önerisi sun
- `proposal_withdraw(proposal, reason)` — öneri geri çek
- `proposal_deposit_claim(proposal, amount, outcome)` — mevduat claim
- `community_pool_deposit(value)` — topluluk havuzuna yatır
- `validator_vote(vote)` — validator oyu
- `ics20_withdrawal(withdrawal)` — IBC çekim
- `ibc_action(ibc_action)` — genel IBC eylemi
- `lqt_vote(epoch, incentivized, rewards_recipient, notes)` — Likidite Turnuvası oyu

### `delegator_vote` (Satır 433–494)

```rust
pub async fn delegator_vote<V: ViewClient>(...) -> Result<&mut Self> {
    let voting_notes = view.notes_for_voting(NotesForVotingRequest {
        votable_at_height: start_block_height,
        address_index: Some(source.into()),
    }).await?;

    for (record, ik) in &voting_notes {
        let voting_power_at_vote_start = validator_start_rate_data.unbonded_amount(record.note.amount());
        let plan = DelegatorVotePlan::new(...);
        self.delegator_vote_precise(plan);
    }
    Ok(self)
}
```

### `prioritize_and_filter_spendable_notes` (Satır 545–566)

```rust
filtered.sort_by(|a, b| {
    match (a.address_index.is_ephemeral(), b.address_index.is_ephemeral()) {
        (true, false) => std::cmp::Ordering::Less,   // ephemeral önce
        (false, true) => std::cmp::Ordering::Greater,
        _ => b.note.amount().cmp(&a.note.amount()),  // büyük miktar önce
    }
});
```

**Öncelik sırası:** (1) Ephemeral adres note'ları, (2) Büyük miktarlı note'lar.
Neden ephemeral önce? FMD (Fuzzy Message Detection) gelecekte ephemeral adresleri bulamazsa, bunları harcamak onları detectable hale getirir.

### `plan` (Satır 570–715)

**2 fazlı planlama:**

Faz 1: Builder metodları ile action'lar eklendi.

Faz 2: Bakiye dengeleme döngüsü (Satır 624–659):
```rust
let mut iterations = 0usize;
while let Some(required) = self.action_list.balance_with_fee().required().next() {
    let note = notes_by_asset_id.get_mut(&required.asset_id)?.pop()?;
    self.action_list.push(SpendPlan::new(&mut OsRng, note.note, note.position));
    self.action_list.refresh_fee_and_change(...);

    iterations += 1;
    if iterations > 100 {
        return Err(anyhow!("failed to plan transaction after 100 iterations"));
    }
}
```

Her iterasyonda: (1) Eksik bakiyeyi bul, (2) Not ekle, (3) Fee/change yeniden hesapla.
Max 100 iterasyon: sonsuz döngü koruması.

Faz sonunda Planner state reset: `self.action_list = Default::default()` vb. Builder yeniden kullanılabilir ancak bu pratik değil (TODO yorumu var).

### Unsafe Bloklar

Yok.

### Test'ler

Yok.

### Nova Shield Chain İçin Dersler

- Planner pattern: önce user intents → sonra fee/note/change otomatik ekleme
- Ephemeral note önceliği: privacy-preserving note selection — kendi DEX'imizde de uygulanabilir
- 100 iteration cap: infinite loop koruması
- `change_address` foreign olabilir: "send max" pattern'i için
- `dutch_auction_withdraw` otomatik `end` çağrısı: state machine'i planner katmanında handle et

---

### [storage.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/storage.rs`
**Satır Sayısı:** ~1700

### Genel Amaç

`Storage` — view server'ın SQLite depolama katmanı. r2d2 connection pool, broadcast channel'lar ile note/nullifier/swap tespiti, ve tüm CRUD operasyonları.

### Struct Tanımı

```rust
#[derive(Clone)]
pub struct Storage {
    pool: r2d2::Pool<SqliteConnectionManager>,
    uncommitted_height: Arc<Mutex<Option<NonZeroU64>>>,
    scanned_notes_tx: tokio::sync::broadcast::Sender<SpendableNoteRecord>,
    scanned_nullifiers_tx: tokio::sync::broadcast::Sender<Nullifier>,
    scanned_swaps_tx: tokio::sync::broadcast::Sender<SwapRecord>,
}
```

### Bağlantı Konfigürasyonu (Satır 119–157)

```rust
conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
conn.set_prepared_statement_cache_capacity(32);
// max_size = 1  (deadlock önleme)
```

- WAL mode: yazma yaparken okuma engellenmiyor
- synchronous=NORMAL: durable değil ama consistent (veri chain'den tekrar sync edilebilir)
- Pool max_size=1: SQLite'ın "database is locked" hatasından kaçınmak için

### `load` (Satır 159–198)

Schema hash kontrolü:
```rust
static SCHEMA_HASH: Lazy<String> = Lazy::new(|| hex::encode(Sha256::digest(include_str!("storage/schema.sql"))));
```
Kaynak koddan SQL schema'sını hash'le. Eski versiyonla oluşturulan DB → hata + "pcli view reset" yönlendirmesi.

### `initialize` (Satır 200–260)

- `tx.execute_batch(include_str!("storage/schema.sql"))` — schema oluştur
- FVK, params, schema_hash, client_version → `kv` tablosuna insert
- `INSERT INTO sync_height (height) VALUES (-1)` — genesis sentinel

### `balances` (Satır 292–364)

```sql
SELECT notes.asset_id, notes.amount, spendable_notes.address_index
FROM    notes
JOIN    spendable_notes ON notes.note_commitment = spendable_notes.note_commitment
WHERE   spendable_notes.height_spent IS NULL
```
Harcanmamış note'lardan bakiye topla. Sonra Rust'ta gruplama.

### `note_by_commitment` + Await (Satır 366–440)

Subscribe-then-query pattern:
```rust
let mut rx = self.scanned_notes_tx.subscribe();  // önce subscribe
// sonra DB query
if let Some(record) = spawn_blocking(|| db_query(...)).await?? { return Ok(record); }
if !await_detection { bail!("not found"); }
// yoksa broadcast'i bekle
loop {
    match rx.recv().await {
        Ok(record) if record.note_commitment == note_commitment => return Ok(record),
        ...
    }
}
```
Race condition önleme: subscribe ÖNCE, query SONRA. Aksi takdirde arada yazılan note kaybolabilir.

### `filter_nullifiers` (Satır 1366–1393)

```sql
SELECT nullifier FROM (
    SELECT nullifier FROM spendable_notes
    UNION SELECT nullifier FROM swaps
    UNION SELECT nullifier FROM tx_by_nullifier
) WHERE nullifier IN (x'...', x'...')
```
3 tablodan UNION ile bizim nullifier'larımızı filtrele — 100x sync hızı farkı.

### `record_block` (Satır 1395–son)

En kritik fonksiyon. Tek bir SQLite transaction içinde:
1. App parameters güncelle (değiştiyse)
2. Yeni note'ları kaydet (notes + spendable_notes)
3. Yeni swap'ları kaydet
4. Harcanan nullifier'ları işaretle
5. Transaction kayıtlarını kaydet
6. return_address güncelle
7. FMD, gas_prices, app_params güncelle
8. SCT'yi DB'ye yaz
9. Broadcast: scanned_notes_tx, scanned_nullifiers_tx, scanned_swaps_tx

**Transactional consistency:**
```rust
let mut new_sct = sct.clone();  // Copy-on-Write SCT klonu
// Eğer hata olursa yeni_sct atılır, orijinal sct değişmez
*sct = spawn_blocking(move || {
    // ... DB işlemleri
    Ok(new_sct)
}).await??;
```

### `notes_for_voting` (Satır 995–1056)

Oy kullanılabilir note'lar: `denom LIKE '_delegation\\_%'` — delegation token'ları. Belirtilen yükseklikte `height_created < votable_at_height` ve `height_spent IS NULL OR height_spent > votable_at_height`.

### `scan_advice` + `give_advice` (Satır 1298–1363)

```rust
// give_advice: gelecekte görülecek note'u önceden kaydet
pub async fn give_advice(&self, note: Note) -> anyhow::Result<()>

// scan_advice: RolledUp commitment'lar için kayıtlı note'ları getir
pub async fn scan_advice(&self, note_commitments: Vec<note::StateCommitment>) -> anyhow::Result<BTreeMap<...>>
```

Swap claim senaryosu: `swap.output_notes()` → `give_advice()` → sonraki blokta RolledUp görülünce → `scan_advice()` ile eşleşme.

### Nova Shield Chain İçin Dersler

- SQLite WAL + synchronous=NORMAL: resync edilebilir veri için durable olmanın gerekmediği pragmatik karar
- Pool max_size=1: SQLite single-writer zorunluluğu
- broadcast::channel(128) for notes, (512) for nullifiers: nullifier'lar daha sık
- Schema hash → migration koruması
- subscribe-before-query pattern: await_detection implementasyonu için temel

---

### [storage/sct.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/storage/sct.rs`
**Satır Sayısı:** 330

### Genel Amaç

`TreeStore` — Tidalwave Commitment Tree (TCT) için SQLite Read/Write implementasyonu. TCT trait'lerini SQLite sorgularına köprüleyen adapter.

### Struct Tanımı

```rust
pub struct TreeStore<'a, 'c: 'a>(pub &'a mut Transaction<'c>);
```
SQLite transaction üzerinde çalışır. Tree okuma/yazma işlemleri DB transaction ile atomik.

### Read Implementasyonu

- `position()` → `SELECT position FROM sct_position LIMIT 1`
- `forgotten()` → `SELECT forgotten FROM sct_forgotten LIMIT 1`
- `hash(position, height)` → `SELECT hash FROM sct_hashes WHERE position=? AND height=? LIMIT 1`
- `hashes()` → iterator, `genawaiter` crate ile lazy iteration
- `commitment(position)` → `SELECT commitment FROM sct_commitments WHERE position=? LIMIT 1`
- `commitments()` → iterator

### Write Implementasyonu

- `set_position(position)` → `UPDATE sct_position SET position=?`
- `set_forgotten(forgotten)` → `UPDATE sct_forgotten SET forgotten=?`
- `add_hash(position, height, hash, _essential)` → `INSERT ... ON CONFLICT DO NOTHING`
- `add_commitment(position, commitment)` → `INSERT ... ON CONFLICT DO NOTHING`
- `delete_range(below_height, positions)` → `DELETE FROM sct_hashes WHERE position >= ? AND position < ? AND height < ?`

### Test

```rust
fn tree_store_spot_check() {
    let tree = tct::Tree::new();
    tree.insert(Witness::Keep, commitment_0)?;
    tree.end_block()?;
    tree.insert(Witness::Forget, commitment_1)?;
    tree.end_epoch()?;
    tree.insert(Witness::Keep, commitment_2)?;
    tree.to_writer(&mut store)?;

    let deserialized = tct::Tree::from_reader(&mut store)?;
    assert_eq!(tree, deserialized);
}
```

### Nova Shield Chain İçin Dersler

- genawaiter ile lazy DB iterator: sorgu hazırlandıktan sonra satırları teker teker tüket
- TCT'nin SQL şeması: `sct_position`, `sct_forgotten`, `sct_hashes`, `sct_commitments` — 4 tablo
- `ON CONFLICT DO NOTHING`: idempotent insert, retry safe
- Height = u64 → i64 dönüşümü: SQLite INTEGER signed olduğu için

---

### [service.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/view/src/service.rs`
**Satır Sayısı:** ~1000+

### Genel Amaç

`ViewServer` — gRPC `ViewService` implementasyonu. Worker görevi spawn eder, storage'a proxy görevi görür, Tonic streaming metodlarını handle eder.

### Struct Tanımı

```rust
#[derive(Clone)]
pub struct ViewServer {
    storage: Storage,
    error_slot: Arc<Mutex<Option<anyhow::Error>>>,
    state_commitment_tree: Arc<RwLock<penumbra_sdk_tct::Tree>>,  // Worker ile paylaşılan SCT
    node: Url,
    sync_height_rx: watch::Receiver<u64>,
}
```

### `ViewServer::new` (Satır 126–147)

```rust
let (worker, state_commitment_tree, error_slot, sync_height_rx) =
    Worker::new(storage.clone(), channel).await?;
tokio::spawn(worker.run().instrument(span));
```
Tek scanning worker spawn eder. Clone alınca aynı worker kullanılır.

### `get_pd_channel` (Satır 154–165)

TLS detect: URL `https://` ise `ClientTlsConfig` kullan, yoksa plain text.

### `check_worker` (Satır 172–195)

Her gRPC handler başında çağrılır:
```rust
if let Some(error) = self.error_slot.lock()?.as_ref() {
    return Err(tonic::Status::new(tonic::Code::Internal, format!("Worker failed: {error}")));
}
```

### `broadcast_transaction` (Satır 197–293)

Asenkron try_stream macro:
1. `broadcast_tx_sync` → fullnode'a gönder
2. `BroadcastSuccess` yield et
3. İstenmişse: nullifier'ı bekle (20 saniye timeout)
4. Storage'dan detection height al
5. `Confirmed { detection_height }` yield et

```rust
let nullifier = transaction.actions().filter_map(|action| match action {
    Action::Spend(spend) => Some(spend.body.nullifier),
    _ => None,
}).next();
```
Sadece spend nullifier'larını bekliyor. Swap claim nullifier'ları için bilinen bug: #1749.

### `status` (Satır 344–370)

```rust
let height_diff = latest_known_block_height.checked_sub(full_sync_height)?;
let catching_up = match (node_catching_up, height_diff) {
    (false, 0) => false,
    (false, 1) => false,  // 1 blok geride kabul edilebilir
    (false, _) => true,
    (true, _) => true,
};
```

### ViewService Implementation

18+ gRPC stream tipi tanımlanmış:
- `NotesStream`, `NotesForVotingStream`, `AssetsStream`, `StatusStreamStream`
- `TransactionInfoStream`, `BalancesStream`, `OwnedPositionIdsStream`
- `UnclaimedSwapsStream`, `BroadcastTransactionStream`, `WitnessAndBuildStream`

Her handler: `check_worker()` → storage query → stream/response

### Nova Shield Chain İçin Dersler

- ViewServer clone edince aynı worker: tek scanning task, çok istemci
- try_stream macro: Rust'ta async stream üretmek için clean pattern
- 20s nullifier detection timeout: ağ koşullarına bağlı timeout stratejisi
- `partial_sync_height = full_sync_height` backward compat hack: API versionlamada dikkat

---

## KISIM 2: WALLET CRATE

### [wallet/src/lib.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/wallet/src/lib.rs`
**Satır Sayısı:** 6

```rust
#![deny(clippy::unwrap_used)]
mod build;
pub use build::build_transaction;
pub mod plan;
```

Minimal — iki public export: `build_transaction` ve `plan` modülü.

---

### [wallet/src/build.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/wallet/src/build.rs`
**Satır Sayısı:** 48

### Genel Amaç

`build_transaction` — custody + view koordinasyonuyla transaction inşa eden üst düzey fonksiyon.

### Fonksiyon

```rust
pub async fn build_transaction<V, C>(
    fvk: &FullViewingKey,
    view: &mut V,
    custody: &mut C,
    plan: TransactionPlan,
) -> Result<Transaction>
where
    V: ViewClient,
    C: CustodyClient,
```

**3 adım:**

1. **Custody Authorization:**
   ```rust
   let auth_data: AuthorizationData = custody.authorize(AuthorizeRequest {
       plan: plan.clone(),
       pre_authorizations: Vec::new(),
   }).await?.data.ok_or(...)?.try_into()?;
   ```

2. **Witness Data (TCT Proof):**
   ```rust
   let witness_data = view.witness(&plan).await?;
   ```

3. **Transaction Build:**
   ```rust
   #[cfg(not(feature = "parallel"))]
   let tx = plan.build(fvk, &witness_data, &auth_data)?;

   #[cfg(feature = "parallel")]
   let tx = plan.build_concurrent(fvk, &witness_data, &auth_data).await...;
   ```

### Tasarım Notu

ZK proof oluşturma seri (plan.build) veya paralel (plan.build_concurrent). Paralel versiyon feature flag ile aktif edilir — büyük işlemler için anlamlı hız kazanımı.

### Nova Shield Chain İçin Dersler

- Transaction inşası 3-adım: custody auth → witness data → ZK proof build
- `parallel` feature flag: opsiyonel paralel proof generation — kendi implementasyonumuzda da yapılabilir
- `fvk` burada gerekli: ZK proof'u oluştururken spending key gerekmiyor, FVK yeterli. Bu kritik mimari karar

---

### [wallet/src/plan.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/wallet/src/plan.rs`
**Satır Sayısı:** 145

### Genel Amaç

`sweep` — unclaimed swap'ları claim et + dust note'ları birleştir.

### Sabit

```rust
pub const SWEEP_COUNT: usize = 8;
```
Her sweep işlemi en fazla 8 note'u birleştiriyor.

### `sweep` (Satır 18–33)

```rust
pub async fn sweep<V, R>(view: &mut V, mut rng: R) -> anyhow::Result<Vec<TransactionPlan>> {
    let mut plans = Vec::new();
    plans.extend(claim_unclaimed_swaps(view, &mut rng).await?);
    plans.extend(sweep_notes(view, &mut rng).await?);
    Ok(plans)
}
```

### `claim_unclaimed_swaps` (Satır 36–85)

```rust
for swap in unclaimed_swaps {
    let swap_plaintext = swap.swap;
    let output_data = swap.output_data;

    let action_plan = SwapClaimPlan {
        swap_plaintext,
        position: swap.position,
        output_data,
        epoch_duration,
        proof_blinding_r: Fq::rand(&mut rng),
        proof_blinding_s: Fq::rand(&mut rng),
    };
}
```
Her unclaimed swap için ayrı transaction. Pre-paid claim fee kullanılıyor.

### `sweep_notes` (Satır 87–145)

```rust
// Sort ascending (smallest first), chunk to SWEEP_COUNT
records.sort_by(|a, b| a.note.value().amount.cmp(&b.note.value().amount));
for group in records.chunks_exact(SWEEP_COUNT) {
    let mut planner = Planner::new(&mut rng);
    planner.set_gas_prices(gas_prices);
    for record in group { planner.spend(record.note.clone(), record.position); }
    let plan = planner.plan(view, index).await?;
    plans.push(plan);
}
```
En küçük 8 note'u al, tek bir sweep tx'e dök. Büyük note'lar artıkta kalıyor (`chunks_exact` davranışı).

### Nova Shield Chain İçin Dersler (DEX Uygulama Notu)

- Swap claim bir background process: unclaimed swap'lar için otomatik claim
- `proof_blinding_r, proof_blinding_s`: ZK proof her seferinde yeni random, deterministic değil
- `chunks_exact`: büyük note'lar dokunulmaz — bu kasıtlı bir konservatif strateji
- Sweep önce claim, sonra dust consolidation — sıra önemli (claim fee ödenmesi gerekiyor)

---

## KISIM 3: CUSTODY CRATE

### [custody/src/lib.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/lib.rs`
**Satır Sayısı:** 29

Public exports:
- `CustodyClient` — trait
- `PreAuthorization` — ön yetkilendirme
- `AuthorizeRequest`, `AuthorizeValidatorDefinitionRequest`, `AuthorizeValidatorVoteRequest`

Modüller: `soft_kms`, `threshold`, `encrypted`, `null_kms`, `policy`

---

### [custody/src/request.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/request.rs`
**Satır Sayısı:** 135

### Struct'lar

```rust
pub struct AuthorizeRequest {
    pub plan: TransactionPlan,
    pub pre_authorizations: Vec<PreAuthorization>,
}

pub struct AuthorizeValidatorDefinitionRequest {
    pub validator_definition: Validator,
    pub pre_authorizations: Vec<PreAuthorization>,
}

pub struct AuthorizeValidatorVoteRequest {
    pub validator_vote: ValidatorVoteBody,
    pub pre_authorizations: Vec<PreAuthorization>,
}
```

3 farklı custody request tipi: tx, validator definition, validator vote.

---

### [custody/src/pre_auth.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/pre_auth.rs`
**Satır Sayısı:** 88

### Enum Tanımı

```rust
pub enum PreAuthorization {
    Ed25519(Ed25519),
}

pub struct Ed25519 {
    pub vk: ed25519_consensus::VerificationKey,
    pub sig: ed25519_consensus::Signature,
}

impl Ed25519 {
    pub fn verify(&self, message: impl AsRef<[u8]>) -> anyhow::Result<()> {
        self.vk.verify(&self.sig, &bytes).map_err(Into::into)
    }
}
```

### Nova Shield Chain İçin Dersler

- Pre-authorization: Custody'yi kilitlemeden "delegate" etmenin yolu
- Ed25519 ile imzalı plan → offline signer approval akışı
- Vk bytes → Ed25519 VerificationKey: `as_slice().try_into()?`

---

### [custody/src/policy.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/policy.rs`
**Satır Sayısı:** 276

### Trait Tanımı

```rust
pub trait Policy {
    fn check_transaction(&self, request: &AuthorizeRequest) -> anyhow::Result<()>;
    fn check_validator_definition(&self, _request: &AuthorizeValidatorDefinitionRequest) -> anyhow::Result<()>;
    fn check_validator_vote(&self, _request: &AuthorizeValidatorVoteRequest) -> anyhow::Result<()>;
}
```

### AuthPolicy Enum

```rust
#[serde(tag = "type")]
pub enum AuthPolicy {
    DestinationAllowList {
        allowed_destination_addresses: Vec<Address>,
    },
    OnlyIbcRelay,
    PreAuthorization(PreAuthorizationPolicy),
}
```

### `DestinationAllowList` implementasyonu (Satır 207–221)

```rust
for output in plan.output_plans() {
    if !allowed_destination_addresses.contains(&output.dest_address) {
        anyhow::bail!("output {:?} has dest_address not in allow list", output);
    }
}
for swap in plan.swap_plans() {
    if !allowed_destination_addresses.contains(&swap.swap_plaintext.claim_address) {
        anyhow::bail!("swap {:?} has claim_address not in allow list", swap);
    }
}
```

### `OnlyIbcRelay` (Satır 222–233)

Yalnızca `Spend`, `Output`, `IbcAction` izin verilen:
```rust
match action {
    ActionPlan::Spend | ActionPlan::Output | ActionPlan::IbcAction => {}
    _ => anyhow::bail!("action {:?} not allowed by OnlyRelay policy", action);
}
```

### `PreAuthorizationPolicy` (Satır 74–130)

```rust
pub enum PreAuthorizationPolicy {
    Ed25519 {
        required_signatures: u32,
        allowed_signers: Vec<ed25519_consensus::VerificationKey>,
    },
}
```

Çakışmayan imzalar: `allowed_signers.take(&pre_auth.vk)` — her imzalayanın katkısı bir kez sayılır.
`seen_signers.len() < required_signatures` → hata.

### Nova Shield Chain İçin Dersler

- Policy sistemi: custody'ye farklı izin seviyeleri
- `DestinationAllowList`: relayer güvenliği için kritik — output + swap claim adresi her ikisi de kontrol edilmeli
- `OnlyIbcRelay`: ayrı bir relayer key ile IBC mesajları imzalamak için
- Pre-authorization: m-of-n offline approval mekanizması

---

### [custody/src/client.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/client.rs`
**Satır Sayısı:** 61

### Trait Tanımı

```rust
pub trait CustodyClient {
    fn authorize(&mut self, request: AuthorizeRequest)
        -> Pin<Box<dyn Future<Output = Result<AuthorizeResponse>> + Send + 'static>>;
}
```

`CustodyServiceClient<T>` için implementasyon: gRPC call'u wrap eder.

---

### [custody/src/terminal.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/terminal.rs`
**Satır Sayısı:** 79

### Enum ve Trait

```rust
pub enum SigningRequest {
    TransactionPlan(TransactionPlan),
    ValidatorDefinition(Validator),
    ValidatorVote(ValidatorVoteBody),
}

#[async_trait]
pub trait Terminal: Sync {
    async fn confirm_request(&self, request: &SigningRequest) -> Result<bool>;
    fn explain(&self, msg: &str) -> Result<()>;
    async fn broadcast(&self, data: &str) -> Result<()>;
    async fn next_response<D>(&self) -> Result<D> where D: DomainType, ...;
    async fn read_line_raw(&self) -> Result<String>;
    async fn get_password(&self) -> Result<String>;
}
```

`next_response` default implementasyonu: JSON parse loop, parse hatası halinde retry.

### Nova Shield Chain İçin Dersler

- Terminal trait: custody interaction'ı UI'dan bağımsız kıl (CLI, GUI, hardware wallet hepsi bu trait'i implemente edebilir)
- `broadcast` vs `read_line_raw`: threshold signing'de mesaj akışı terminal üzerinden

---

### [custody/src/soft_kms.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/soft_kms.rs`
**Satır Sayısı:** 194

### Genel Amaç

`SoftKms` — bellek içi yazılımsal anahtar yönetim sistemi. En basit custody implementasyonu.

### Struct Tanımı

```rust
pub struct SoftKms {
    config: Config,  // spend_key + auth_policy
}
```

### `sign` (Satır 39–48)

```rust
pub fn sign(&self, request: &AuthorizeRequest) -> anyhow::Result<AuthorizationData> {
    for policy in &self.config.auth_policy {
        policy.check_transaction(request)?;
    }
    Ok(request.plan.authorize(OsRng, &self.config.spend_key)?)
}
```

Tüm policy kontrollerinden geçince `plan.authorize()` ile `SpendKey` + `OsRng` kullanarak `AuthorizationData` üretilir.

### gRPC Implementasyonu (Satır 95–193)

CustodyService: `authorize`, `authorize_validator_definition`, `authorize_validator_vote`, `export_full_viewing_key`, `confirm_address`

### Nova Shield Chain İçin Dersler

- SoftKms: dev/test için basit single-key custody
- Policy chain: her policy sırayla kontrol edilir, biri başarısız → hata
- `plan.authorize(rng, &spend_key)` → AuthorizationData: SpendKey + randomness = signature set
- Production'da SoftKms kullanma, Threshold veya Ledger tercih et

---

### [custody/src/soft_kms/config.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/soft_kms/config.rs`
**Satır Sayısı:** 87

### Struct Tanımı

```rust
pub struct Config {
    #[serde_as(as = "DisplayFromStr")]
    pub spend_key: SpendKey,
    #[serde(default, skip_serializing_if = "is_default")]
    pub auth_policy: Vec<AuthPolicy>,
}
```

- `spend_key`: TOML/JSON'da bech32 string olarak saklanır
- `auth_policy`: boşsa serialize edilmez (default skip)
- `is_default` helper: mevcut değer default değerle eşitse serialize etme

Test: TOML round-trip (SpendKey + tüm policy türleri)

---

### [custody/src/null_kms.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/null_kms.rs`
**Satır Sayısı:** 60

### Genel Amaç

`NullKms` — view-only mode için dummy custody. Tüm istekler `failed_precondition` hatası döndürür.

```rust
pub struct NullKms {}
// Tüm metodlar: Err(tonic::Status::failed_precondition("Got authorization request in view-only mode"))
```

### Nova Shield Chain İçin Dersler

- Null object pattern: view-only modda signing'i engellemek için
- Aynı CustodyService trait'ini implemente ediyor — tip sistemi güvenli

---

### [custody/src/encrypted.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/encrypted.rs`
**Satır Sayısı:** 279

### Şifreleme Modülü (encryption::)

**Parametreler:**
- `SALT_SIZE = 32`, `TAG_SIZE = 16`, `KEY_SIZE = 32`
- Argon2id ile şifre türetme: `Params::new(1 << 21, 1, 4, Some(KEY_SIZE))` — 2MB bellek, 1 iterasyon, 4 iş parçacığı
- RFC 9106 parametreleri

**Şifreleme şeması:** `TAG(16) || SALT(32) || CIPHERTEXT`
- ChaCha20Poly1305
- Salt AD (associated data) olarak kullanılıyor
- Key-committing değil (TODO: güvenlik notu var)

**İmzalı veriler:**
```
salt → key_derivation → symmetric_key → ChaCha20Poly1305(salt_as_AD, data)
```

### Encrypted Struct

```rust
pub enum InnerConfig {
    SoftKms(soft_kms::Config),
    Threshold(threshold::Config),
}

pub struct Config {
    data: Vec<u8>,  // Argon2id + ChaCha20 ile şifrelenmiş InnerConfig JSON
}

pub struct Encrypted<T> {
    config: Config,
    terminal: T,
    inner: OnceCell<anyhow::Result<Box<dyn pb::custody_service_server::CustodyService>>>,
}
```

`OnceCell`: ilk request'te şifre sorulur, sonra cached.

**Test'ler:**
- Şifreleme → şifre çözme roundtrip
- Farklı şifre ile şifre çözme başarısız olmalı

### Nova Shield Chain İçin Dersler

- Argon2id key derivation: şifre tabanlı şifreleme için doğru seçim
- OnceCell: lazy init + password prompt bir kez
- Key-committing AEAD olmadığı için salt AD — güvenlik notu: pratik ataklar için yeterli

---

### [custody/src/threshold.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/threshold.rs`
**Satır Sayısı:** 668

### Genel Amaç

FROST tabanlı threshold signing implementasyonu. Birden fazla imzalayanın koordinasyonuyla eşik imzası üretir.

### Temel Tipler

```rust
pub enum SigningResponse {
    Transaction(AuthorizationData),
    ValidatorDefinition(decaf377_rdsa::Signature<SpendAuth>),
    ValidatorVote(decaf377_rdsa::Signature<SpendAuth>),
}
```

### `follow` (Satır 51–88) — Follower Protokolü

2 round:
1. Coordinator round1 mesajı al → confirm_request → follower_round1 → broadcast cevap
2. Coordinator round2 mesajı al → follower_round2 → broadcast cevap

### `dkg` (Satır 97–171) — Distributed Key Generation

```rust
pub async fn dkg(t: u16, n: u16, terminal: &impl Terminal) -> Result<Config>
```

2 round DKG:
- Round 1: `dkg::round1` → broadcast → collect n-1 cevap
- Round 2: `dkg::round2` → broadcast → collect n-1 cevap
- Round 3: `dkg::round3` → Config

Duplicate detection: `encode_to_vec()` ile byte karşılaştırma.

### `Threshold<T>` (Satır 179–247)

```rust
pub struct Threshold<T> {
    config: Config,
    terminal: T,
}
```

`authorize` metodunda 3 round:
1. Coordinator round1: kendi nonce'ını üret + followers'a gönder
2. threshold-1 adet follower round1 yanıtı topla
3. Coordinator round2: commitments → followers
4. threshold-1 adet follower round2 yanıtı topla
5. Coordinator round3: FROST aggregate

`required_signatures` = 0 ise (tx'de spend yok) → doğrudan effect_hash döndür.

### Test (Satır 362–666)

Detaylı in-memory test:
- `FollowerTerminal`: mpsc channel'larla simüle edilmiş terminal
- `CoordinatorTerminal`: 1-of-N round-robin alım
- `run_dkg(t, n)` → Vec<Config> (t=3, n=3 test)
- DKG → aynı FVK üretilmeli
- Transaction signing → `verify(effect_hash, sig)` doğrulaması

**Test JSON TX:** Hardcoded bir penumbra-testnet-rhea tx planı ile end-to-end signing testi.

### Nova Shield Chain İçin Dersler

- FROST (Flexible Round-Optimized Schnorr Threshold) — decaf377 üzerinde implementasyon
- t-of-n signing: `threshold = t`, `n` toplam katılımcı
- DKG: dealer'sız key generation — hiçbir taraf private key'in tamamını görmüyor
- Terminal trait abstraction: real terminal / simüle edilmiş terminal aynı kod path'i
- `required_signatures = 0` optimizasyonu: gereksiz round-trip'lerden kaçın

---

### [custody/src/threshold/config.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/threshold/config.rs`
**Satır Sayısı:** 283

### Struct Tanımı

```rust
pub struct Config {
    threshold: u16,                                                  // t (eşik)
    fvk: FullViewingKey,                                            // grubun FVK'si
    spend_key_share: frost::keys::SigningShare,                     // bu katılımcının pay'i
    signing_key: SigningKey,                                         // round mesajlarını imzalamak için Ed25519
    verifying_shares: HashMap<VerificationKey, frost::keys::VerifyingShare>,  // diğer katılımcıların payleri
}
```

### `deal` (Satır 152–208) — Centralized Dealer

```rust
pub fn deal(rng: &mut impl CryptoRngCore, t: u16, n: u16) -> Result<Vec<Self>>
```
`frost::keys::generate_with_dealer` ile tüm payları tek bir makinede üret. Test/geliştirme için; üretimde DKG kullan.

### `from_parts` (Satır 117–150)

DKG tamamlandığında config oluştur:
- `group_public()` → FVK'nın spend_verification_key'i olarak FROST group public key
- `nullifier_key` DKG aracılığıyla secret shared

### Test

```rust
fn test_config_serialization_roundtrip() {
    let config = Config::deal(&mut OsRng, 2, 2)?.pop().unwrap();
    // JSON round-trip
}
```

---

### [custody/src/threshold/sign.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/threshold/sign.rs`
**Satır Sayısı:** 606

### Genel Amaç

FROST threshold signing protokolü mesaj tipleri ve 3-round coordinator/follower fonksiyonları.

### Mesaj Tipleri

```rust
pub struct CoordinatorRound1 { request: SigningRequest }  // Plan broadcast
pub struct CoordinatorRound2 { all_commitments: Vec<BTreeMap<frost::Identifier, frost::round1::SigningCommitments>> }
pub struct FollowerRound1 { commitments: Vec<SigningCommitments>, pk: VerificationKey, sig: Signature }
pub struct FollowerRound2 { shares: Vec<frost::round2::SignatureShare>, pk: VerificationKey, sig: Signature }
```

Her mesaj imzalanmış: `pk.verify(sig, data)` — Man-in-the-middle koruması.

### `required_signatures` (Satır 321–331)

```rust
fn required_signatures(request: &SigningRequest) -> usize {
    match request {
        SigningRequest::TransactionPlan(plan) => {
            plan.spend_plans().count()
                + plan.delegator_vote_plans().count()
                + plan.lqt_vote_plans().count()
        }
        SigningRequest::ValidatorDefinition(_) => 1,
        SigningRequest::ValidatorVote(_) => 1,
    }
}
```

### Coordinator Round 3 (Satır 462–546)

```rust
let spend_auths = plan.spend_plans().map(|x| x.randomizer)
    .chain(plan.delegator_vote_plans().map(|x| x.randomizer))
    .chain(plan.lqt_vote_plans().map(|x| x.randomizer))
    .zip(share_maps.iter())
    .zip(state.signing_packages.iter())
    .map(|((randomizer, share_map), signing_package)| {
        frost::aggregate_randomized(signing_package, share_map, &config.public_key_package(), randomizer)
    })
    .collect()?;
```

`aggregate_randomized`: FROST + rerandomization = her spend için farklı randomizer ile imzalama.

### Nova Shield Chain İçin Dersler

- Ed25519 ile mesaj imzalama: threshold mesajlarında MITM koruması
- `aggregate_randomized`: Penumbra'nın rerandomizable signature scheme'i — her spend farklı görünüyor
- Split'te spend_auths / delegator_vote_auths / lqt_vote_auths ayrımı
- FROST identifier = `Identifier::derive(pk.as_bytes())` — Ed25519 public key'den deterministic

---

### [custody/src/threshold/dkg.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody/src/threshold/dkg.rs`
**Satır Sayısı:** 372

### Genel Amaç

Penumbra'nın FROST DKG implementasyonu. Nullifier key için özel shared commitment sistemi içeriyor.

### `NullifierCommitment` (Satır 22–51)

```rust
struct NullifierCommitment([u8; 32]);

impl NullifierCommitment {
    fn create(share: Fq) -> Self {
        let mut state = blake2b_simd::Params::new().personal(b"dkg-commit").to_state();
        share.serialize_compressed(&mut state)?;
        // 32 byte BLAKE2b hash
    }
}
```

### Round 1 Mesajı

```rust
pub struct Round1 {
    package: frost_dkg::round1::Package,    // FROST DKG paket
    nullifier_commitment: NullifierCommitment, // nullifier payına commit
    epk: EncryptionKey,                     // round2 şifrelemesi için geçici public key
    vk: VerificationKey,                    // kimlik
}
```

### Round 2 Mesajı

```rust
pub struct Round2 {
    encrypted_packages: HashMap<VerificationKey, Vec<u8>>,  // her alıcı için şifreli FROST paketi
    nullifier: Fq,                                          // açık nullifier payı
    vk: VerificationKey,
    sig: Signature,
}
```

### `round3` — Nullifier Key Birleştirme (Satır 317–371)

```rust
let nullifier_key = {
    let mut acc = state.nullifier;
    for message in &messages {
        // Taahhüt açma doğrulaması
        if NullifierCommitment::create(message.nullifier) != state.associated_data[&message.vk].1 {
            anyhow::bail!("opened nullifier did not match commitment");
        }
        acc += message.nullifier;  // Fq toplamı = ortak nullifier key
    }
    acc
};
```

**Kritik:** Nullifier key = tüm payların Fq alanı üzerinde toplamı. Bir katılımcı commit ettiği değerden farklı açarsa → hata.

### DKG Şifreleme (encryption.rs)

decaf377 ECDH ile ephemeral anahtar değişimi:
```
EPK || TAG(16) || CIPHERTEXT(=FROST_package)
```
- `infallible_key_agreement`: key agreement başarısız olursa random secret → şifre çözülemez ama çökmez
- BLAKE2b ile symmetric key türetme: `personal = b"dkg-encryption"`

### Nova Shield Chain İçin Dersler

- Commit-then-reveal: DKG'de nullifier pay açılmadan önce commit edilmeli (binding commitment)
- Nullifier key shared: t-of-n eşiği aşılmadan kimse nullifier key'e erişemez
- `infallible_key_agreement`: invalid public key → garbage secret → mesaj okunamaz (panic yok)
- BLAKE2b personalization string: `b"dkg-commit"`, `b"dkg-encryption"` — domain separation

---

## KISIM 4: CUSTODY-LEDGER-USB CRATE

### [custody-ledger-usb/src/lib.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody-ledger-usb/src/lib.rs`
**Satır Sayısı:** 158

### Genel Amaç

Ledger USB donanım cüzdanı entegrasyonu. `Device` üzerinden APDU protokolü ile Penumbra Ledger uygulamasıyla iletişim.

### Config / Service

```rust
pub struct Config {}  // Şimdilik boş — gelecekte device fingerprint eklenebilir

pub struct Service {
    device: Arc<Mutex<Option<Device>>>,  // lazy init, mutex ile korunan
}
```

### `acquire_device` (Satır 52–59)

```rust
async fn acquire_device(&self) -> anyhow::Result<impl DerefMut<Target = Device> + '_> {
    let mut guard = self.device.lock().await;
    if guard.is_none() {
        *guard = Some(Device::connect_to_first().await?);
    }
    Ok(MutexGuard::map(guard, |x| x.as_mut().expect(...)))
}
```
Lazy init: ilk işlemde cihaz bağlanır.

### Desteklenmeyenler

```rust
async fn authorize_validator_definition(...) {
    unimplemented!("ledger does not support validator operations")
}
```

Ledger yalnızca: `authorize`, `export_full_viewing_key`, `confirm_address`

---

### [custody-ledger-usb/src/device.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/custody-ledger-usb/src/device.rs`
**Satır Sayısı:** 305

### Derivation Path (Sabit — Satır 34–41)

```rust
fn vec_with_fixed_derivation_path(capacity: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + capacity);
    out.extend_from_slice(&u32::to_le_bytes(0x8000_002C));  // purpose = 44' (BIP44)
    out.extend_from_slice(&u32::to_le_bytes(0x8000_1984));  // coin type = 6532 = 0x1984 (Penumbra)
    out.extend_from_slice(&u32::to_le_bytes(0x8000_0000));  // account = 0'
    out
}
```
BIP44: `m/44'/6532'/0'` — Penumbra'nın SLIP-0044 coin type kaydı.

### AddressIndex Kodlaması (Satır 26–32)

```rust
fn address_index_to_weird_bytes(index: AddressIndex) -> [u8; 17] {
    let mut out = [0u8; 17];
    out[..4].copy_from_slice(&index.account.to_le_bytes());
    out[4] = u8::from(index.randomizer != [0u8; 12]);  // has_randomizer flag
    out[5..].copy_from_slice(&index.randomizer);
    out
}
```

### APDU Protokolü (Satır 164–304)

**Hata Kodları (Satır 43–101):** 40+ APDU hata kodu tanımlanmış.

**`get_fvk` (Satır 189–204):** INS=0x03, döner FullViewingKey bytes

**`confirm_addr` (Satır 206–222):** INS=0x01, p1=1 (kullanıcı onayı iste), döner Address bytes

**`authorize` (Satır 224–303):**
```
INS=0x02, p1=0: derivation path gönder (başlangıç)
INS=0x02, p1=1: her 250 byte'lık chunk
INS=0x02, p1=2: son chunk
```
Yanıt: `effect_hash(64) || spend_count(2) || delegator_count(2)`

Sonra her imza için:
```
INS=0x05, p1=i: i. spend imzası
INS=0x06, p1=i: i. delegator vote imzası
```

### Güvenlik

`check_error_code` 0x9000 haricindeki tüm kodları hata olarak işaretliyor. Hata mesajı = `data[..payload_end]` UTF-8 (lossy).

### Nova Shield Chain İçin Dersler

- APDU protokolü: Ledger uygulamasıyla byte-level iletişim
- 250 byte chunk: HID protokol sınırlaması
- Derivation path sabit: BIP44 standart, coin type kayıt gerekiyor
- `unimplemented!` yerine `Status::failed_precondition`: tonic uyumlu hata
- Effect hash Ledger tarafından hesaplanıyor (64 byte) — client ile tutarlı olmalı

---

## KISIM 5: MISC CRATE

### [misc/measure/src/main.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/misc/measure/src/main.rs`
**Satır Sayısı:** 329

### Genel Amaç

`penumbra-measure` — geliştirici performans ölçüm aracı.

### Komutlar

- `StreamBlocks` — compact block akışı hızını ölç (byte sayısı, nullifier, payload türleri)
- `OpenConnections` — N paralel bağlantı aç (load test)
- `OpenConnectionsActive` — N bağlantı + aktif işleme (daha gerçekçi load test)
- `FetchCompactBlock height` — tek blok indir, dosyaya kaydet

**MAX_CB_SIZE_BYTES = 12MB** — worker.rs ile tutarlı.

### Metrikler

```
{cb_count} compact blocks
{nf_count} nullifiers
{sp_note_count} note payloads
{sp_swap_count} swap payloads
{sp_rolled_up_count} rolled up payloads
```

### Nova Shield Chain İçin Dersler

- Compact block stream ölçümü: Nova Shield için aynı araç gerekiyor
- Load test: 100 paralel bağlantı — fullnode kapasitesini test et
- Note/Swap/RolledUp oranı: zincirin doluluk istatistikleri

---

### [misc/tct-visualize/] — 8 dosya

TCT (Tidalwave Commitment Tree) görselleştirme aracı. DOT format graph output + canlı web arayüzü.

### render/dot.rs + render.rs

```rust
pub fn dot<W: Write>(tree: &Tree, writer: &mut W) -> io::Result<()>
pub fn dot_pretty<W: Write>(tree: &Tree, writer: &mut W) -> io::Result<()>
```

TCT'nin iç yapısını (node'lar, hash'ler, position, height) DOT format'ta görselleştirir.

### live/ modülü

- `control.rs` — websocket kontrol kanalı
- `query.rs` — TCT state sorguları
- `view/earliest.rs` — en eski görünür node
- `view/resources.rs` — statik kaynaklar
- `view.rs` — canlı render

### Nova Shield Chain İçin Dersler

- TCT debug visualization: kendi commitment tree implementasyonumuzda benzer araç çok değerli
- DOT format: Graphviz ile hızlı görselleştirme

---

## KISIM 6: CNIDARIUM-COMPONENT CRATE

### [cnidarium-component/src/lib.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/cnidarium-component/src/lib.rs`
**Satır Sayısı:** 48

### Genel Amaç

ABCI uygulaması için temel trait tanımları. Komponent crate'lerin nasıl yapılandırılması gerektiğini belgeler.

**Mimari:**
- `data crate`: action tipler, plan, view — `component` feature olmadan, tokio bağımlılığı yok, WASM uyumlu
- `component module`: feature-gated, async kod, state R/W
- `state_key module`: public API state anahtarları
- `event module`: emitted events

---

### [cnidarium-component/src/action_handler.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/cnidarium-component/src/action_handler.rs`
**Satır Sayısı:** 100

### Trait Tanımı

```rust
#[async_trait]
pub trait ActionHandler {
    type CheckStatelessContext: Clone + Send + Sync + 'static;

    async fn check_stateless(&self, context: Self::CheckStatelessContext) -> Result<()>;

    async fn check_historical<S: StateRead + 'static>(&self, _state: Arc<S>) -> Result<()> {
        Ok(())  // default: no-op
    }

    async fn check_and_execute<S: StateWrite>(&self, state: S) -> Result<()>;
}
```

**3 fazlı validasyon:**

1. `check_stateless` — zincir durumu gerekmez; paralel çalışabilir
2. `check_historical` — geçmiş snapshot üzerinde okuma; paralel ama TOCTOU riski var
3. `check_and_execute` — okuma/yazma erişimi; sıralı

**TOCTOU uyarısı (Satır 53–65):** `check_historical` geçmiş state üzerinde çalışır. Aynı tx'deki başka action bu state'i değiştirmiş olabilir → `check_and_execute`'a bırak.

---

### [cnidarium-component/src/component.rs]

**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/cnidarium-component/src/component.rs`
**Satır Sayısı:** 86

### Trait Tanımı

```rust
#[async_trait]
pub trait Component {
    type AppState;

    async fn init_chain<S: StateWrite>(state: S, app_state: Option<&Self::AppState>);
    async fn begin_block<S: StateWrite + 'static>(state: &mut Arc<S>, begin_block: &abci::request::BeginBlock);
    async fn end_block<S: StateWrite + 'static>(state: &mut Arc<S>, end_block: &abci::request::EndBlock);
    async fn end_epoch<S: StateWrite + 'static>(_state: &mut Arc<S>) -> Result<()> { Ok(()) }
}
```

**`&mut Arc<S>` tasarım notu:** Component alt görevlerle state paylaşmak için Arc'ı clone edebilir (read-only), sonra `.get_mut()` ile exclusive access alabilir. Alt görevler dönmeden önce Arc'ı bırakmalı.

### Nova Shield Chain İçin Dersler

- ActionHandler 3-faz: stateless (paralel) → historical (paralel, dikkatli) → execute (sıralı)
- Component yaşam döngüsü: init_chain → (begin_block → tx actions → end_block)* → end_epoch
- `end_epoch` default no-op: epoch konsepti ABCI'a ekleme — Nova Shield'da epoch gerekecek mi?
- `&mut Arc<S>` pattern: concurrent sub-tasks ile state paylaşımı

---

## GENEL SENTEZ VE NOVA SHIELD CHAIN İÇİN EYLEM PLANI

### 1. View Server Mimarisi (Kesin Alınacak)

Penumbra'nın view server mimarisi mükemmel:
- `Worker` (background) + `ViewServer` (gRPC) + `Storage` (SQLite) üçlüsü
- Shared `Arc<RwLock<TCT>>` ile SCT paylaşımı
- `watch::channel` ile sync height bildirimi
- `broadcast::channel` ile note/nullifier/swap event'leri

Nova Shield için aynı mimariye ihtiyaç var.

### 2. Note Scanning Algoritması (Adapte Et)

`scan_block` içindeki:
- Paralel trial decryption (tokio::spawn per note)
- Witness::Keep vs Witness::Forget ayrımı
- Advice sistemi (RolledUp commitment'lar için)
- Filter nullifiers (100x hız farkı)

Bu algoritma doğrudan alınabilir — privacy L1 için evrensel pattern.

### 3. Custody Mimarisi (Katmanlı Kullan)

Custody backends hiyerarşisi:
```
NullKms (view-only) → SoftKms (dev) → Encrypted → Ledger USB
                                              ↘ Threshold (FROST)
```

Nova Shield:
- Dev/test için SoftKms
- Production için Threshold (FROST) veya Ledger USB
- Policy sistemi: DestinationAllowList, OnlyIbcRelay, PreAuthorization

### 4. Threshold Signing (Değerli Referans)

FROST + decaf377:
- DKG (dealer-free key generation)
- Nullifier key sharing (Blake2b commit-reveal)
- Rerandomizable signatures (aggregate_randomized)
- Terminal trait (UI bağımsız)

Nova Shield için cggmp21 tercih edilmiş olsa da FROST'un bazı mekanizmaları adapte edilebilir.

### 5. Transaction Planner (Kopyalanabilir Pattern)

Planner pattern:
- Builder API: tüm user intent'ler → action_list
- 2 fazlı plan: user actions → fee/note/change otomatik ekleme
- 100 iterasyon limiti
- Note seçim prioritization (ephemeral first, largest first)

### 6. DEX Entegrasyonu (Kritik Dersler)

- `SwapRecord` + `SwapPlaintext` + `BatchSwapOutputData`: swap state'i takip
- `claim_unclaimed_swaps`: background sweep
- LP NFT state machine: her position state değişikliği yeni NFT
- `position_withdraw`: sequence numarası — LP withdrawal sequence tracking
- Dutch auction: schedule → end → withdraw state machine

### 7. Güvenlik Desenleri

- Subscribe-before-query: race condition önleme
- Passive failure on invalid compact block: timing side-channel azaltma
- Argon2id (2MB, 4 threads) + ChaCha20Poly1305: encrypted config
- Ed25519 mesaj imzalama: threshold protokol mesajları MITM'e karşı korumalı
- APDU 250 byte chunks: hardware wallet protokol sınırı

### 8. SQLite Yeterli mi?

Penumbra view server için SQLite yeterli:
- Single writer (pool max_size=1)
- WAL mode + synchronous=NORMAL
- Resync edilebilir veri (durable olmak zorunda değil)

Nova Shield için ilk aşamada SQLite, sonra RocksDB geçişi mantıklı.

### 9. Ledger USB Entegrasyonu (Uzun Vadeli)

APDU spec referansı: `https://github.com/Zondax/ledger-penumbra/blob/.../docs/APDUSPEC.md`
Nova Shield için Ledger uygulaması ilerletilirse:
- BIP44 coin type kayıt (SLIP-0044)
- APDU INS kodu tasarımı
- Effect hash Ledger tarafında hesaplama

---

**Analiz Tamamlandı.**
**Toplam kaynak satır:** ~6500+ (view: ~2500, wallet: ~200, custody: ~2000, ledger: ~465, misc: ~700, cnidarium-component: ~235)
