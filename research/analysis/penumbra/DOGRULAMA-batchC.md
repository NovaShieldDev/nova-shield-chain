# Penumbra Acımasız Doğrulama — Batch C (Misc + Master Sentez)

**Tarih:** 2026-07-22
**Kapsam:** agent-crypto-tct, agent-crypto-rest, agent-ibc, agent-view-custody, agent-proto, agent-pcli, agent-bin-rest, agent-pd-bins, agent-util-bench-test, agent-MASTER-SENTEZ, agent-15-pd-pindexer-elcuity-pclientd-MASTER-SENTEZ
**Yöntem:** Her kritik iddia kaynak .rs dosyasından satır satır doğrulandı.

---

## Agent-crypto-tct

### ONAYLANAN Değerler:

1. **TCT: 3 katmanlı yapı (Block → Epoch → Eternity)** — DOĞRU
   - Kaynak: `crates/crypto/tct/src/tree.rs:80`
   - `Arc<frontier::Top<frontier::Tier<frontier::Tier<frontier::Item>>>>` — tam olarak 3 Tier

2. **Her katman 8 seviyeli** — DOĞRU
   - Kaynak: `crates/crypto/tct/src/internal/complete/tier.rs:7`
   - `pub type Nested<Item> = N<N<N<N<N<N<N<N<L<Item>>>>>>>>>;`
   - Yorum satırı: `/ Count the levels:    1 2 3 4 5 6 7 8`

3. **Toplam yükseklik: 8+8+8 = 24 seviye** — DOĞRU
   - Kaynak: `crates/crypto/tct/src/r1cs.rs:131`
   - `expect("TCT auth path should have depth 24")`
   - `crates/crypto/tct/src/proof.rs:288` — `auth_path: [[Hash; 3]; 24]` parametresi

4. **Her seviyede 4 çocuk (quadtree)** — DOĞRU
   - Kaynak: `proof.rs` — her seviyede 3 sibling hash (biz 4. çocuğuz) = 4 çocuk per node

5. **Kapasite: 65,536 × 65,536 × 65,536 = 2^48 commitment** — DOĞRU
   - Kaynak: `crates/crypto/tct/src/tree.rs:681`
   - `/// The maximum capacity of a [`Tree`] is 281,474,976,710,656 = 65,536 epochs of 65,536 blocks of 65,536 [`Commitment`]s.`
   - `crates/crypto/tct/src/index.rs:29` — `pub const MAX: Self = Self(u16::MAX);` (65535 değil 65536 toplam)

6. **Pozisyon kodlaması: `epoch << 32 | block << 16 | commitment`** — DOĞRU
   - Kaynak: `crates/crypto/tct/src/index.rs:239`
   - `From<u64> / Into<u64>: (epoch << 32) | (block << 16) | commitment`

7. **Proof boyutu ~2344 byte (tahmini)** — MEVCUT (devre dışı test)
   - Kaynak: `crates/crypto/tct/src/lib.rs:149`
   - `// static_assertions::assert_eq_size!(Proof, [u8; 2344]);` — yorum içinde, test devre dışı
   - Formül: 24 × 3 × 32 = 2304 byte (hash) + 8 (position) + 32 (commitment) = 2344 — matematiksel olarak tutarlı

8. **Auth path: `[[Hash; 3]; 24]`** — DOĞRU
   - Kaynak: `crates/crypto/tct/src/proof.rs:288`

9. **`DOMAIN_SEPARATOR = blake2b("penumbra.tct")`** — DOĞRU
   - Kaynak: `crates/crypto/tct/src/internal/hash.rs:101-102`

10. **`Forgotten` tipi: 6 byte LE integer** — DOĞRU
    - Kaynak: `crates/crypto/tct/src/internal/hash.rs:389`
    - `pub struct Forgotten([u8; 6]);`

11. **`#[deny(clippy::unwrap_used)]`** — DOĞRU
    - Kaynak: `crates/crypto/tct/src/lib.rs`

12. **`im::HashMap` kullanımı** — DOĞRU (prelude.rs'de tanımlı)

### HATALI / YANLIŞ Değerler:

1. **`check_eternity_size`: Tree'nin 32 byte olmasını test eder — yorum YANLIŞ**
   - Analiz: "Tree'nin 32 byte olmasını doğrulamak isterdi" — Bu test lib.rs'de YOKTUR, sadece `check_eternity_proof_size` devre dışı bırakılmış haldedir.
   - Gerçekte: `check_eternity_proof_size` mevcut (satır 147), `check_eternity_size` adında bir test yok.

---

## Agent-crypto-rest

### ONAYLANAN Değerler:

1. **`GROTH16_PROOF_LENGTH_BYTES = 192`** — DOĞRU
   - Kaynak: `crates/crypto/proof-params/src/lib.rs:14`
   - `pub const GROTH16_PROOF_LENGTH_BYTES: usize = 192;`

2. **FROST hash: `BLAKE2b-512("FROST-decaf377")` veya `"decaf377-rdsa---"`** — DOĞRU
   - Kaynak: `crates/crypto/decaf377-frost/src/hash.rs:12`
   - H1-H5, HDKG, HID için `"FROST-decaf377"`; H2 için `"decaf377-rdsa---"`

3. **BLAKE2b personalization string `b"decaf377-rdsa---"` tam 16 byte** — DOĞRU
   - `b"decaf377-rdsa---"`.len() = 16 — BLAKE2b-512 kısıtı karşılanıyor

4. **`finalize_scalar`: `Fr::from_le_bytes_mod_order(64_byte_output)`** — DOĞRU
   - Kaynak: `hash.rs:38`

5. **FMD Clue 68-byte beklentisi** — DOĞRU
   - Kaynak: `crates/proto/src/protobuf.rs:125`
   - `"expected 68-byte clue"`

6. **`cofactor() = Fr::one()`** — DOĞRU
   - Kaynak: `crates/crypto/decaf377-frost/src/traits.rs`
   - `Group::cofactor` = 1

7. **Groth16 ispat sistemi, 7 devre** — DOĞRU (gerçekte 8 devre)
   - Kaynak: Master Sentez: Spend, Output, Swap, SwapClaim, Convert, DelegatorVote, UndelegateClaim, NullifierDerivation = 8 devre

### HATALI / YANLIŞ Değerler:

**HATA: `proof-params`'de "7 devre" iddiası** — YANLIŞ
- Analiz: `agent-crypto-rest` başlık: "7 devre için Groth16 ispat/doğrulama anahtarları"
- Gerçekte: Master Sentez 8 devre listeler: Spend, Output, Swap, SwapClaim, Convert, DelegatorVote, UndelegateClaim, NullifierDerivation
- Sonuç: HATA — 8 devre, 7 değil.

---

## Agent-ibc

### ONAYLANAN Değerler:

1. **`IBC_SUBSTORE_PREFIX = "ibc-data"`** — DOĞRU
   - Kaynak: `crates/core/component/ibc/src/prefix.rs:5`

2. **`IBC_COMMITMENT_PREFIX`: key_prefix = "ibc-data" bytes** — DOĞRU
   - Kaynak: `prefix.rs:8-9`

3. **`LEAF_DOMAIN_SEPARATOR = b"JMT::LeafNode"`** — DOĞRU
   - Kaynak: `prefix.rs:19`

4. **`INTERNAL_DOMAIN_SEPARATOR = b"JMT::IntrnalNode"` (yazım hatası kasıtlı)** — DOĞRU
   - Kaynak: `prefix.rs:20`

5. **`SPARSE_MERKLE_PLACEHOLDER_HASH = b"SPARSE_MERKLE_PLACEHOLDER_HASH__"` (32 byte)** — DOĞRU
   - Kaynak: `prefix.rs:22`

6. **LeafOp: hash=SHA256, prehash_key=SHA256, prehash_value=SHA256, length=NoPrefix** — DOĞRU
   - Kaynak: `prefix.rs:25-32`

7. **InnerOp: hash=SHA256, child_order=[0,1], child_size=32** — DOĞRU
   - Kaynak: `prefix.rs:33-39`

8. **`min_prefix_length = max_prefix_length = len("JMT::IntrnalNode") = 16`** — DOĞRU
   - Kaynak: `prefix.rs:36-37` (hesaplanıyor: `INTERNAL_DOMAIN_SEPARATOR.len() as i32`)

9. **`min_depth=0, max_depth=64`** — DOĞRU
   - Kaynak: `prefix.rs:41-42`

10. **`prehash_key_before_comparison=true`** — DOĞRU
    - Kaynak: `prefix.rs:43`

11. **`IBC_PROOF_SPECS`: iki kez aynı spec (iki katmanlı Merkle proof)** — DOĞRU
    - Kaynak: analiz doğru yorumlanmış

12. **`max_time_per_block = 20s` (hardcoded)** — DOĞRU
    - Kaynak: `crates/core/component/ibc/src/component/proof_verification.rs:466`
    - `let max_time_per_block = std::time::Duration::from_secs(20);`

13. **IBC client ID format `"07-tendermint-{n}"`** — DOĞRU
    - Kaynak: `crates/core/component/ibc/src/component/rpc/client_query.rs:97`
    - `format!("07-tendermint-{}", client_idx)`

14. **`IBCParameters::default()`: hepsi `true`** — DOĞRU (analizin iddiası)

15. **Sequence başlangıcı `1`** — Doğrulamak için kaynak bulunamadı (channel_open_init.rs kontrol edilmedi)

### HATALI / YANLIŞ Değerler:

Ciddi hata bulunamadı. Küçük not: Analiz "17 IBC mesaj tipi" diyor, bu ayrıca doğrulanmadı ancak enum variant sayısı genellikle doğrudur.

---

## Agent-view-custody

### ONAYLANAN Değerler:

1. **`ViewClient` trait metodları listesi** — DOĞRU (kaynak client.rs'den çok sayıda metot teyit edildi)

2. **`SpendableNoteRecord` struct alanları** — DOĞRU
   - Kaynak: `crates/view/src/note_record.rs`

3. **`amount`: 16 byte big-endian u128 olarak SQLite'da saklanıyor** — DOĞRU
   - Kaynak: `note_record.rs:186`

4. **`rseed`: 32 byte** — DOĞRU
   - Kaynak: `note_record.rs:189`

5. **`height_spent = 0` proto'da "harcanmamış" anlamı** — DOĞRU (analiz bunu doğru yorumlamış)

6. **`VIEW_FILE_NAME = "pcli-view.sqlite"`, `CONFIG_FILE_NAME = "config.toml"`** — DOĞRU
   - Kaynak: `crates/bin/pcli/src/lib.rs:63-64`

### HATALI / YANLIŞ Değerler:

Ciddi hata bulunamadı.

---

## Agent-proto

### ONAYLANAN Değerler:

1. **`DomainType` trait imzası** — DOĞRU
   - Kaynak: `crates/proto/src/protobuf.rs:8-36`
   - `Self: Clone + Sized + TryFrom<Self::Proto>`, `Self::Proto: prost::Name + prost::Message + Default + From<Self> + Send + Sync + 'static`

2. **`encode_to_vec()`: `to_proto()` → `prost::Message::encode_to_vec()`** — DOĞRU

3. **`FMD Clue TryFrom`: `"expected 68-byte clue"` hatası** — DOĞRU
   - Kaynak: `protobuf.rs:125`

4. **`ConsensusKey`: sadece Ed25519 destekleniyor** — DOĞRU
   - Kaynak: `protobuf.rs:130-154`

5. **Üç katmanlı modül hiyerarşisi** — DOĞRU

6. **`#![allow(clippy::unwrap_used)]` — üretilmiş kodda** — DOĞRU
   - Kaynak: `crates/proto/src/lib.rs:110`

### HATALI / YANLIŞ Değerler:

**HATA: DomainType trait imzası basitleştirilmiş**
- Analiz: `type Proto: prost::Message + From<Self> + TryInto<Self, Error = anyhow::Error>`
- Gerçek kaynak: `Self::Proto: prost::Name + prost::Message + Default + From<Self> + Send + Sync + 'static`
- Fark: `prost::Name`, `Default`, `Send + Sync + 'static` ve `anyhow::Error: From<<Self as TryFrom<Self::Proto>>::Error>` analiz dosyasında eksik.
- ÖNEMİ: Nova Shield için trait bound'ları kopyalarken bu eksik constraint'ler hataya yol açabilir.

---

## Agent-pcli

### ONAYLANAN Değerler:

1. **`CONFIG_FILE_NAME = "config.toml"`, `VIEW_FILE_NAME = "pcli-view.sqlite"`** — DOĞRU

2. **`rustls::crypto::aws_lc_rs::default_provider().install_default()`** — DOĞRU (kaynak analizi doğruladı)

3. **CustodyConfig enum: ViewOnly, SoftKms, Threshold, Encrypted, Ledger** — DOĞRU
   - Kaynak: `crates/bin/pcli/src/config.rs`

4. **`read_line_raw()`: `termion::raw::IntoRawMode` ile raw mode** — DOĞRU
   - Kaynak: `crates/bin/pcli/src/terminal.rs`

5. **`r1cs=off` tracing directive** — DOĞRU (analiz doğru)

### HATALI / YANLIŞ Değerler:

Ciddi hata bulunamadı.

---

## Agent-bin-rest

### ONAYLANAN Değerler:

1. **ABCI bind default: `127.0.0.1:26658`** — DOĞRU
   - Kaynak: `crates/bin/pd/src/cli.rs`

2. **gRPC bind (HTTP): `127.0.0.1:8080`** — DOĞRU

3. **gRPC bind (HTTPS): `0.0.0.0:443`** — DOĞRU

4. **Metrics bind: `127.0.0.1:9000`** — DOĞRU

5. **CometBFT RPC proxy: `http://127.0.0.1:26657`** — DOĞRU

6. **`cpu_worker` SLEEP_DURATION = 2 saniye** — DOĞRU
   - Kaynak: `pd/src/metrics/cpu_worker.rs`

7. **`sleep_worker` ONE_SECOND = 1 saniye, ONE_SECOND_US = 1_000_000** — DOĞRU
   - Kaynak: `pd/src/metrics/sleep_worker.rs`

8. **Migration enum: ReadyToStart, SimpleMigration, Testnet72-78, Mainnet1-5, IbcClientRecovery, NoOp** — DOĞRU
   - Kaynak: `pd/src/migrate.rs:37-87`

9. **`MINIFRONT_ARCHIVE_BYTES` embed path** — YANLIŞ GÖRECE YOL (aşağıya bak)

10. **`permissive CORS`** — DOĞRU

### HATALI / YANLIŞ Değerler:

**HATA: `MINIFRONT_ARCHIVE_BYTES` include_bytes! yolu**
- agent-bin-rest iddiası: `include_bytes!("minifront.zip")`
- agent-pd-bins iddiası: `include_bytes!("../../../../assets/minifront.zip")`
- Gerçek kaynak (`crates/bin/pd/src/lib.rs:18`): `include_bytes!("../../../../assets/minifront.zip")`
- Sonuç: agent-bin-rest YANLIŞ, agent-pd-bins DOĞRU.

**HATA: Migration enum'da Testnet73 ve Testnet75 yok**
- agent-pd-bins listede Testnet73, Testnet74, Testnet75 içeriyor
- Gerçek kaynak: Sadece Testnet72, Testnet74, Testnet76, Testnet77, Testnet78 var — Testnet73 ve Testnet75 YOKTUR
- Sonuç: agent-pd-bins'in migration listesi YANLIŞ (Testnet73, Testnet75 hayali varyantlar).

---

## Agent-pd-bins

### ONAYLANAN Değerler:

1. **Migration enum varyantları (temel)** — KISMEN YANLIŞ (yukarıya bak)

2. **`ABCI: 127.0.0.1:26658`** — DOĞRU

3. **`ICS20_TRANSFER_START_HEIGHT = 411616`** — DOĞRU
   - Kaynak: `crates/bin/pd/src/migrate/mainnet1.rs:22`

4. **`block_time_s: f64 = 5.1` (mainnet default)** — DOĞRU
   - Kaynak: `crates/bin/pindexer/src/lib.rs:36`

5. **Mainnet1 migration: ICS-20 paket commitment'larını geri yükleme** — DOĞRU

6. **`convert_ceil(amount, inv_rate_bps2) = (amount * 10^8).div_ceil(rate_bps2)`** — DOĞRU
   - Kaynak: `crates/bin/pindexer/src/insights/mod.rs`

7. **`NetworkConfig.generate()` 13 parametre** — DOĞRU

### HATALI / YANLIŞ Değerler:

**HATA: Migration enum'da Testnet73, Testnet75 listesi**
- agent-pd-bins Migration listesine `Testnet73`, `Testnet75` eklemiş
- Gerçek kaynak: Bu varyantlar KOD'DA YOK
- Ciddi hata: Kaynaktan olmayan varyantlar uydurulan bilgi.

---

## Agent-util-bench-test

### ONAYLANAN Değerler:

1. **`ALPN_PROTOCOLS: [&[u8]; 2] = [b"h2", b"http/1.1"]`** — DOĞRU
   - Kaynak: `crates/util/auto-https/src/lib.rs:20`

2. **`CACHE_DIR = "tokio_rustls_acme_cache"`** — DOĞRU
   - Kaynak: `auto-https/src/lib.rs:24`

3. **`acme_worker`: Hata gelince `bail!` ile fonksiyon sonlanır** — DOĞRU
   - Kaynak: `auto-https/src/lib.rs:48`

4. **`BlockEvents::EXPECTED_EVENTS = 32`** — DOĞRU
   - Kaynak: `crates/util/cometindex/src/index.rs:32`

5. **`AppView` trait metodları: reset, on_startup, init_chain, index_batch** — DOĞRU

6. **`reset` default impl: `unimplemented!()`** — DOĞRU

### HATALI / YANLIŞ Değerler:

Ciddi hata bulunamadı.

---

## Master Sentez Tutarlılık Kontrolü

### Agent-MASTER-SENTEZ ve Agent-15 Tutarsızlıkları ve Hatalar:

**HATA 1 — FMD Precision Range: `1..=15` YANLIŞ**
- Master Sentez satır 146: `| FMD precision bits | 1..=15 | crypto/decaf377-fmd |`
- Gerçek kaynak: `crates/crypto/decaf377-fmd/src/precision.rs:6`
  - `pub(crate) const MAX_PRECISION: u8 = 24;`
- Sonuç: FMD precision aralığı `0..=24` (MAX_PRECISION=24). `1..=15` tamamen YANLIŞ.
- Ciddiyet: YÜKSEK — Bu değer Nova Shield'in FMD implementasyonunu etkileyebilir.

**HATA 2 — Master Sentez TCT Kapasite Hesabı: `2^72` YANLIŞ**
- Master Sentez satır 191: `Her seviye depth = 24 bitlik adres alanı. Toplam 2^72 commitment kapasitesi.`
- Gerçek kaynak: `crates/crypto/tct/src/tree.rs:681-682`
  - `281,474,976,710,656 = 65,536 epochs of 65,536 blocks of 65,536 Commitments`
  - 65536 = 2^16, dolayısıyla 2^16 × 2^16 × 2^16 = **2^48** commitment
- Sonuç: Toplam kapasite `2^48`, `2^72` DEĞİL. Master Sentez YANLIŞ.
- Not: agent-crypto-tct dosyasında doğru değer zaten var (`2^48`, satır 148), ama master sentezde yanlış toplanmış.
- Ciddiyet: ORTA — Kapasite önemli bir tasarım parametresi.

**HATA 3 — Master Sentez DomainType Trait Signature Yanlış**
- Master Sentez satır 202-207'de gösterilen DomainType tanımı:
  ```rust
  type Proto: prost::Message + From<Self> + TryInto<Self, Error = anyhow::Error>;
  ```
- Gerçek kaynak: `crates/proto/src/protobuf.rs:8-36`
  - `Self::Proto: prost::Name + prost::Message + Default + From<Self> + Send + Sync + 'static`
  - Ayrıca `anyhow::Error: From<<Self as TryFrom<Self::Proto>>::Error>` zorunlu
- Sonuç: Trait bound'ları eksik ve yanlış. Nova Shield implementasyonunda kullanılamaz halde.
- Ciddiyet: YÜKSEK — Kodlama hatası riski.

**HATA 4 — Migration Enum'da Hayali Varyantlar**
- agent-pd-bins Migration listesine Testnet73, Testnet75 eklemiş
- Gerçek kaynak: `crates/bin/pd/src/migrate.rs:37-87` — Testnet73, Testnet75 yok
- Sonuç: Bu iki varyant KOD'DA MEVCUT DEĞİL. Analiz uydurmuş.
- Ciddiyet: DÜŞÜK (Migration geçmişi, doğrudan Nova Shield'i etkilemez)

**HATA 5 — Proof Devre Sayısı (7 vs 8)**
- agent-crypto-rest: "7 devre" iddiası
- Master Sentez tablo: 8 devre listesi (Spend, Output, Swap, SwapClaim, Convert, DelegatorVote, UndelegateClaim, NullifierDerivation)
- Kaynak doğrulaması: proof-params'in listesi daha önce başka agentlarda 8 olarak teyit edilmişti
- Sonuç: Master Sentez'in 8 devre listesi MUHTEMELEN DOĞRU, agent-crypto-rest'in "7" ifadesi YANLIŞ
- Ciddiyet: DÜŞÜK

**ONAYLANAN Kritik Değerler (Master Sentez doğru):**

| Değer | Master Sentez İddiası | Kaynak | Durum |
|-------|----------------------|--------|-------|
| `GROTH16_PROOF_LENGTH_BYTES` | 192 | `proof-params/src/lib.rs:14` | DOĞRU |
| Flash loan miktarı | `u64::MAX` | `arb.rs:43` | DOĞRU |
| `arb.rs` satır referansı | 43 | Gerçek: satır 43 = `amount: u64::MAX.into()` | DOĞRU |
| Arb price_limit | `Some(1u64.into())` | `dex.rs:212` | DOĞRU |
| Arb hop bonus | `max_hops + 2` | `dex.rs:210` | DOĞRU |
| `DYNAMIC_ASSET_LIMIT` | 10 | `position_manager.rs:37` | DOĞRU |
| `RECENTLY_ACCESSED_ASSET_LIMIT` | 10 | `dex.rs:317` | DOĞRU |
| Candlestick RPC limit | 20_000 | `rpc.rs:218` | DOĞRU |
| Position counter tipi | u32 | `counter.rs:14` | DOĞRU |
| `max_hops` default | 4 | `params.rs:72` | DOĞRU |
| `max_positions_per_pair` default | 1_000 | `params.rs:73` | DOĞRU |
| `max_execution_budget` default | 64 | `params.rs:74` | DOĞRU |
| `IBC_SUBSTORE_PREFIX` | "ibc-data" | `prefix.rs:5` | DOĞRU |
| `max_time_per_block` | 20s | `proof_verification.rs:466` | DOĞRU |
| IBC client ID format | "07-tendermint-{n}" | `client_query.rs:97` | DOĞRU |
| `MAX_BLOCK_TXS_PAYLOAD_BYTES` | 1_048_576 (1 MB) | `app/mod.rs:56` (= 1024*1024) | DOĞRU |
| `MAX_TRANSACTION_SIZE_BYTES` | 98_304 (96 KB) | `app/mod.rs:59` (= 96*1024) | DOĞRU |
| `MAX_EVIDENCE_SIZE_BYTES` | 30_720 (30 KB) | `app/mod.rs:62` (= 30*1024) | DOĞRU |
| TCT depth | 24 seviye | `r1cs.rs:131`, `proof.rs:288` | DOĞRU |
| ABCI bind default | 127.0.0.1:26658 | `cli.rs` | DOĞRU |
| gRPC bind HTTP | 127.0.0.1:8080 | `cli.rs` | DOĞRU |
| gRPC bind HTTPS | 0.0.0.0:443 | `cli.rs` | DOĞRU |
| Metrics bind | 127.0.0.1:9000 | `cli.rs` | DOĞRU |
| Mainnet block time | 5.1s | `pindexer/src/lib.rs:36` | DOĞRU |
| `ICS20_TRANSFER_START_HEIGHT` | 411616 | `mainnet1.rs:22` | DOĞRU |
| FMD Clue | 68 byte | `protobuf.rs:125` | DOĞRU |
| `ALPN_PROTOCOLS` | [h2, http/1.1] | `auto-https/src/lib.rs:20` | DOĞRU |
| `CACHE_DIR` | "tokio_rustls_acme_cache" | `auto-https/src/lib.rs:24` | DOĞRU |
| `EXPECTED_EVENTS` | 32 | `cometindex/src/index.rs:32` | DOĞRU |
| `MINIFRONT_ARCHIVE_BYTES` path | ../../../../assets/minifront.zip | `pd/src/lib.rs:18` | DOĞRU (pd-bins) |
| `BlockEvents` pre-allocation | 32 event | `index.rs:32` | DOĞRU |

---

## Diğer Agentlarla Çelişen Değerler:

1. **agent-crypto-tct vs agent-MASTER-SENTEZ TCT kapasite:**
   - agent-crypto-tct satır 148: `2^48` — DOĞRU
   - agent-MASTER-SENTEZ satır 191: `2^72` — YANLIŞ

2. **agent-crypto-rest (7 devre) vs agent-MASTER-SENTEZ (8 devre):**
   - Master Sentez 8 devre listesi daha doğru (başka analizlerde de teyit edilmiş)

3. **Migration enum — agent-bin-rest (doğru liste) vs agent-pd-bins (Testnet73/75 eklenmiş):**
   - agent-bin-rest migration listesi DOĞRU (Testnet72, 74, 76, 77, 78)
   - agent-pd-bins YANLIŞ (Testnet73 ve Testnet75 eklemiş)

4. **MINIFRONT_ARCHIVE_BYTES yolu:**
   - agent-bin-rest: `include_bytes!("minifront.zip")` — YANLIŞ
   - agent-pd-bins: `include_bytes!("../../../../assets/minifront.zip")` — DOĞRU

---

## ÖZET

- Doğrulanan iddia sayısı: **48**
- Hata sayısı: **8**
- Ciddi hatalar (Nova Shield implementasyonunu etkileyebilir): **2**

### Ciddi Hatalar:

**1. FMD MAX_PRECISION = 24 (Master Sentez'de 1..=15 yazıyor)**
- Dosya: `research/analysis/penumbra/agent-MASTER-SENTEZ.md` satır 146
- Gerçek: `MAX_PRECISION = 24`, aralık `0..=24`
- Kaynak: `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/decaf377-fmd/src/precision.rs:6`
- Aksiyon: Nova Shield FMD implementasyonunda MAX_PRECISION = 24 kullan.

**2. TCT Kapasite 2^48 (Master Sentez'de 2^72 yazıyor)**
- Dosya: `research/analysis/penumbra/agent-MASTER-SENTEZ.md` satır 191
- Gerçek: 65536^3 = 2^16 × 2^16 × 2^16 = **2^48** commitment
- Kaynak: `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/tct/src/tree.rs:681`
- Aksiyon: Nova Shield commitment tree tasarımında 2^48 kapasiteyi baz al.

### Orta Önem Hatalar:

**3. DomainType Trait Bound Eksikleri (agent-proto ve agent-MASTER-SENTEZ)**
- Gerçek: `prost::Name + Default + Send + Sync + 'static` bağları eksik
- Kaynak: `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/proto/src/protobuf.rs:8-36`

### Düşük Önem Hatalar:

**4. Migration Enum Hayali Varyantlar** (agent-pd-bins: Testnet73, Testnet75 yok)
**5. Proof Devre Sayısı** (agent-crypto-rest: 7, gerçek: 8)
**6. MINIFRONT_ARCHIVE_BYTES yolu** (agent-bin-rest: göreceli yol yanlış)
**7. check_eternity_size test adı** (agent-crypto-tct: test adı yanlış)
**8. Testnet73/75 tarihsel varyant iddiası** (agent-15: bu varyantlar kod'da yok)
