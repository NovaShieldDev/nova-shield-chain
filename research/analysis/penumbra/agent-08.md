# Penumbra Shielded Pool Analizi — Agent 08

**Analiz Tarihi:** 2026-07-22
**Kapsam:** `crates/core/component/shielded-pool/src/` — 41 dosya
**Toplam Satır:** ~3500+ satır

---

## Genel Bakis

Penumbra'nin `shielded-pool` componenti, UTXO-benzeri bir note modeli uzerine kurulu tam gizli bir odeme katmanidir. Her not (note), ZK proof (Groth16/BLS12-377) ile harcanir veya olusturulur. Temel kriptografik primitifler: Poseidon hash, decaf377 eliptik egri, ChaCha20Poly1305 simetrik sifreleme. ICS-20 ile IBC entegrasyonu yapilmistir. FMD (Fuzzy Message Detection) ile kullanici gizliligi note tarama maliyeti ile denge arasinda tutulur.

---

## Dosya Analizi

---

## [backref.rs]

**Dosya:** `.../shielded-pool/src/backref.rs`
**Satir Sayisi:** 217

### Genel Amac

"Backref" (geri referans), bir harcanmis notun (spend) hangi note commitment'a atifta bulundugunu sifrelenmis sekilde saklar. Bu, FVK (Full Viewing Key) sahibinin kendi harcama gecmisini yeniden izlemesine olanak tanir — ama di?aridan kimse bu baglanti?yi goremez.

### Struct/Enum/Type Listesi

- **`ENCRYPTED_BACKREF_LEN: usize = 48`** — ChaCha20Poly1305 sifrelenmis backref'in boyutu. Plaintext 32 byte (note commitment), + 16 byte AEAD tag = 48 byte.
- **`Backref`** — `note_commitment: tct::StateCommitment` iceren yapi. Sifrelenmemis form.
- **`EncryptedBackref`** — `bytes: Vec<u8>` iceren yapi. 0 veya 48 byte olabilir.

### Trait Implementasyonlari

- `TryFrom<[u8; 32]> for Backref` — 32 byte'tan note commitment olusturur.
- `TryFrom<[u8; ENCRYPTED_BACKREF_LEN]> for EncryptedBackref` — 48 byte'tan olusturur.
- `TryFrom<[u8; 0]> for EncryptedBackref` — Bos (dummy) backref icin.
- `From<EncryptedBackref> for Vec<u8>` — Bytes'a donusturur.

### Fonksiyonlar

#### `Backref::new(note_commitment: tct::StateCommitment) -> Self`
- Satir 25–27: Basit constructor.

#### `Backref::encrypt(&self, brk: &BackreferenceKey, nullifier: &Nullifier) -> EncryptedBackref`
- Satir 29–43:
  1. `ChaCha20Poly1305::new(&brk.0)` — BackreferenceKey'den cipher olusturur.
  2. Nonce: `nullifier.to_bytes()[..12]` — Nullifier'in ilk 12 byte'i nonce olarak kullanilir. Bu tasarim karari: ayni (brk, nullifier) ciftinin tek bir backref sifrelemesine karsilik gelmesi garanti eder.
  3. Plaintext: `note_commitment.0.to_bytes()` — 32 byte.
  4. `cipher.encrypt(nonce, plaintext)` — 48 byte ciphertext (32 + 16 AEAD tag).

#### `EncryptedBackref::is_empty(&self) -> bool` / `len(&self) -> usize`
- Satir 47–53: Boyluk kontrolleri.

#### `EncryptedBackref::dummy() -> Self`
- Satir 55–57: Sifir uzunluklu bos backref. Dummy spend'lerde kullanilir.

#### `EncryptedBackref::decrypt(&self, brk, nullifier) -> Result<Option<Backref>>`
- Satir 61–89:
  1. Uzunluk 0 ise `Ok(None)` doner — gecerli, backref yok anlami.
  2. Ayni nonce ile `cipher.decrypt(...)`.
  3. 32 byte plaintext'i `Backref::try_from` ile parse eder.
  4. Hata durumlarinda genel `"decryption error"` mesaji (yan kanal korumasina dikkat).

### ZK/Kriptografik Formuller

- **ChaCha20Poly1305:** AEAD, 256-bit anahtar (`brk.0`), 96-bit nonce (nullifier basta 12 byte).
- **Nonce secimi:** Nullifier, harcanan nota bagli deterministik bir deger oldugu icin, ayni nota iki kez harcanamaz — dolayisiyla nonce tekrari imkansizdir.

### Test'ler

**`encrypted_backref_zero_length` (proptest):** Satir 141–177
- Dummy backref'i olustur, `is_empty()` ve `len() == 0` kontrol et.
- `decrypt()` -> `None` gelmeli.

**`encrypted_backref_round_trip` (proptest):** Satir 180–216
- Rastgele `seed_phrase`, `amount`, `rseed` ile note olustur.
- `nullifier` derive et, backref sifrele ve coz.
- `Some(backref) == decrypted_backref` kontrol et.

### Tasarim Kararlari

1. **Nonce = nullifier basi:** Deterministik, tekrarsiz, protokol seviyesinde garanti.
2. **0 ya da 48 byte:** Dummy spend'ler backref tasimaz — gizlilik ortogonal tutulur.
3. **Genel hata mesaji:** `"decryption error"` — timing/oracle saldirilarini engeller.

### Nova Shield Chain Icin Dersler

- ChaCha20Poly1305 + pozisyon-bagli nonce pattern'i Nova Shield'in kendi spend mekanizmasinda kullanilabilir.
- Dummy spend konsepti (sifir degerli, sahte not) MEV korumasinda noise ekleme stratejisi olarak degerlendirilmelidir.
- Sifir uzunluklu backref yerine sabit uzunluklu (dummy olarak rastgele byte) kullanmak metadata sizdirmayi engelleyebilir — bu bir gelistirme firsatidir.

---

## [component.rs]

**Dosya:** `.../shielded-pool/src/component.rs`
**Satir Sayisi:** 21

### Genel Amac

Component modulunun kok dosyasi. Alt modulleri declare eder ve public API'yi re-export eder.

### Struct/Enum/Type Listesi

Dogrudan struct tanimlamaz; sadece re-export yapar.

### Public Re-exportlar

- `register_metrics` — metrics modulunden.
- `AssetRegistry`, `AssetRegistryRead` — assets modulunden.
- `ClueManager` — fmd modulunden.
- `Ics20WithdrawalWithHandler` — ics20_withdrawal_with_handler modulunden.
- `NoteManager` — note_manager modulunden.
- `ShieldedPool`, `StateReadExt`, `StateWriteExt` — shielded_pool modulunden.
- `Ics20Transfer` — transfer modulunden.
- `rpc` — public mod.

### Tasarim Kararlari

- Her component alt modulu kendi sorumluluklarini izole eder.
- `rpc` modulu public; diger modüller crate-internal.

### Nova Shield Chain Icin Dersler

- Bu module organizasyonu (action_handler, assets, fmd, note_manager, rpc, shielded_pool, transfer) Nova Shield'in kendi component'lari icin referans alinabilir.

---

## [component/action_handler.rs]

**Dosya:** `.../component/action_handler.rs`
**Satir Sayisi:** 3

### Genel Amac

Alt action handler modullerini bildirir: `ics20_withdrawal`, `output`, `spend`. Her biri `ActionHandler` traitini implemente eder.

---

## [component/action_handler/ics20_withdrawal.rs]

**Dosya:** `.../component/action_handler/ics20_withdrawal.rs`
**Satir Sayisi:** 33

### Genel Amac

ICS-20 cekme (withdrawal) eyleminin `check_stateless`, `check_historical`, `check_and_execute` adimlarini implemente eder.

### Trait Implementasyonlari

`Ics20WithdrawalWithHandler<HI>` icin:

#### `check_stateless(&self, _context: ()) -> Result<()>`
- Satir 11–13: `self.action().validate()` cagirir — timeout'un sifir olmamasi ve dakikaya yuvarlanmis olmasi gibi stateless kurallar.

#### `check_historical<S: StateRead>(&self, state) -> Result<()>`
- Satir 15–24: IBC parametrelerinden `outbound_ics20_transfers_enabled` kontrol eder. Devre disi ise islem reddedilir.

#### `check_and_execute<S: StateWrite>(&self, state) -> Result<()>`
- Satir 26–32:
  1. Mevcut blok zamanini al.
  2. `withdrawal_check(action, block_time)` ile paketin gonderilebilirligini kontrol et.
  3. `withdrawal_execute(action)` ile ICS-20 paket gonderimini gerceklestir.

### Tasarim Kararlari

- `HostInterface` generic parametresi: zincir spesifik blok zamani alma islevi icin soyutlama.
- Check/execute ayrimi: zorunlu guvenlik pattern'i. Execute, check'in basarili oldugunu varsayar.

### Nova Shield Chain Icin Dersler

- IBC entegrasyonunda `check_stateless` / `check_historical` / `check_and_execute` uc-asamali pattern'i zorunlu; Nova Shield'in kendi action handler'lari bu yapiya uygun olmalidir.
- `outbound_ics20_transfers_enabled` gibi governance parametresi kullanimi: runtime'da IBC'yi kapatma imkani verir — Nova Shield icin kritik.

---

## [component/action_handler/output.rs]

**Dosya:** `.../component/action_handler/output.rs`
**Satir Sayisi:** 46

### Genel Amac

`Output` action'inin stateless ve stateful validation/execution adimlarini implemente eder. Bir output, yeni sifrelenmis not (note) olusturur.

### Trait Implementasyonlari

`ActionHandler for Output`:

#### `check_stateless(&self, _context: ()) -> Result<()>`
- Satir 14–26:
  - `output.proof.verify(&OUTPUT_PROOF_VERIFICATION_KEY, OutputProofPublic { balance_commitment, note_commitment })` cagirir.
  - Eger ZK proof gecersizse islem reddedilir. Stateless: zincir state'ine ihtiyac duymaz.

#### `check_and_execute<S: StateWrite>(&self, state) -> Result<()>`
- Satir 28–45:
  1. `state.get_current_source()` — Transaction context'ini alir.
  2. `state.add_note_payload(note_payload, source)` — Notu SCT'ye ve bekleyen listeye ekler.
  3. `state.record_proto(EventOutput { note_commitment })` — ABCI event kaydeder.

### Tasarim Kararlari

- `check_stateless`'ta sadece ZK proof dogrulama: CPU yogun is zincir state'inden once yapilir.
- `check_historical` yok: output'lar icin tarihsel kontrol gerekmez — her gecerli proof kabul edilir.

### Nova Shield Chain Icin Dersler

- ZK proof dogrulama `check_stateless` asamasina; state degisikligi `check_and_execute` asamasina aittir — bu ayrimi kesinlikle koru.
- `source` context pattern'i: her note'un nasil olusturuldugunu (genesis, ICS-20, transaction, vs.) izleme imkani verir.

---

## [component/action_handler/spend.rs]

**Dosya:** `.../component/action_handler/spend.rs`
**Satir Sayisi:** 61

### Genel Amac

`Spend` action'inin dogrulama ve yurutme adimlarini implemente eder. Bir spend, mevcut bir notu UTXO stilinde harcar.

### Trait Implementasyonlari

`ActionHandler for Spend`:

#### `check_stateless(&self, context: TransactionContext) -> Result<()>`
- Satir 18–39:
  1. **Spend auth imza dogrulama:** `spend.body.rk.verify(context.effect_hash.as_ref(), &spend.auth_sig)` — Harcamayi authorize eden randomize dogrulama anahtari ile imzayi kontrol eder.
  2. **ZK proof dogrulama:** `spend.proof.verify(&SPEND_PROOF_VERIFICATION_KEY, public)` — Public input'lar: anchor (SCT root), balance_commitment, nullifier, rk.

#### `check_and_execute<S: StateWrite>(&self, state) -> Result<()>`
- Satir 42–59:
  1. `state.check_nullifier_unspent(nullifier)` — Cift harcama (double-spend) kontrolu.
  2. `state.nullify(nullifier, source)` — Nullifier'i harcandi olarak isaretler.
  3. `state.record_proto(EventSpend { nullifier })` — ABCI event kaydeder.

### Tasarim Kararlari

- `TransactionContext` ile `CheckStatelessContext = TransactionContext` pattern'i: effect_hash'in spend authentication'a dahil edilmesi, replay saldirilarini engeller.
- Nullifier kontrolu `check_and_execute` icinde: SCT state'e bagli oldugu icin stateful.

### Nova Shield Chain Icin Dersler

- Effect hash'in imzalanmasi (sadece nullifier degil tum transaction etkisinin imzalanmasi) Nova Shield'in spend mekanizmasi icin kritik bir guvenliktir.
- `check_nullifier_unspent` + `nullify` cift harcamaya karsi atomik koruma; Nova Shield'in nullifier registry'si bu pattern'i takip etmelidir.

---

## [component/assets.rs]

**Dosya:** `.../component/assets.rs`
**Satir Sayisi:** 39

### Genel Amac

Asset metadata'sini state'de kayit eden ve okuyan trait'leri tanimlar.

### Trait Implementasyonlari

#### `AssetRegistryRead: StateRead`
- `denom_metadata_by_asset(&self, asset_id) -> Option<Metadata>`:
  - `self.get(&state_key::denom_metadata_by_asset::by_asset_id(asset_id))` cagirir.
  - Blanket impl: `impl<T: StateRead + ?Sized> AssetRegistryRead for T {}`.

#### `AssetRegistry: StateWrite`
- `register_denom(&mut self, denom: &Metadata)`:
  - Satir 26–35: Once `denom_metadata_by_asset` ile kontrol eder. Zaten kayitliysa no-op. Yoksa `self.put(...)` ile yazar.
  - `#[instrument(skip(self))]` ile trace loglama.
  - Blanket impl: `impl<T: StateWrite + ?Sized> AssetRegistry for T {}`.

### Tasarim Kararlari

- **Idempotent kayit:** Ayni asset iki kez kaydedilmeye calisilirsa no-op. Guvenli.
- **Blanket impl:** Her StateRead/StateWrite implementasyonu otomatik bu trait'leri alir.

### Nova Shield Chain Icin Dersler

- Blanket impl pattern'i (her StateRead/StateWrite otomatik olarak yardimci trait'leri alir) Nova Shield'in state katmani icin kullanilmasi gereken standarttir.
- Asset metadata'sinin on-chain kaydi: IBC ve native token yonetimi icin zorunlu.

---

## [component/fmd.rs]

**Dosya:** `.../component/fmd.rs`
**Satir Sayisi:** 89

### Genel Amac

FMD (Fuzzy Message Detection) clue sayaclarini state'de yoneten trait'leri tanimlar. Clue'lar, kullanicilarin gelen notlari taramasini saglayan FMD ipuclaridir.

### Trait Implementasyonlari

#### `ClueWriteExt: StateWrite` (private)
- `put_current_clue_count(count)`: `u64.to_be_bytes()` ile raw state'e yazar.
- `put_previous_clue_count(count)`: Ayni format.

#### `ClueReadExt: StateRead` (private)
- `get_current_clue_count()`: Raw bytes'tan `u64::from_be_bytes`. Yoksa 0 doner.
- `get_previous_clue_count()`: Ayni.

#### `ClueManager: StateRead + StateWrite` (public)
- `record_clue(&mut self, clue: Clue, tx: TransactionId) -> Result<()>`:
  - Satir 62–72: Mevcut sayaci 1 arttirir (`saturating_add`), sonra `EventBroadcastClue` kaydeder.
  - Clue'lar sadece sayilir; kullanici bu sayac ile kac tane mesaj yayinlandigini anlayabilir.

#### `ClueManagerInternal: ClueManager` (crate-internal)
- `flush_clue_count(&mut self) -> Result<(u64, u64)>`:
  - Satir 80–86: Mevcut sayaci `previous`'a yazar, current'i sifirlar.
  - Donen tuple: (onceki deger, mevcut deger) — FMD parametresi guncelleme algoritmasinda kullanilir.

### ZK/Kriptografik Formuller

- FMD clue'larin kendisi `decaf377_fmd::Clue` tipinde; burada sadece sayilip yayinlanir.
- Clue sayisi FMD precision guncelleme icin kullanilan "trafik yogunlugu" gostergesidir.

### Tasarim Kararlari

- `saturating_add`: Sayac tasmasi yerine u64::MAX'ta kalir — kritik guzenleme.
- `flush_clue_count`: Her FMD guncelleme periyodunun sonunda cagirilir; onceki vs. yeni count karsilastirmasi ile degisim hesaplanir.

### Nova Shield Chain Icin Dersler

- FMD clue sayacinin periyodik flush'u (flush_clue_count) Nova Shield'in privacy metrikleri icin ilham verici: kac tane mesaj gonderildigini on-chain sayma, ama icerigini gizleme.

---

## [component/ics20_withdrawal_with_handler.rs]

**Dosya:** `.../component/ics20_withdrawal_with_handler.rs`
**Satir Sayisi:** 31

### Genel Amac

`Ics20Withdrawal` tipini `HostInterface` generic parametresiyle sarmalayan newtype. `ActionHandler` implementasyonunun generic Host Interface alabilmesi icin type-level soyutlama yapar.

### Struct/Enum/Type Listesi

- **`Ics20WithdrawalWithHandler<HI>(Ics20Withdrawal, PhantomData<HI>)`** — Iki alani var: icerik ve phantom tip.

### Fonksiyonlar

- `new(action) -> Self`, `action() -> &Ics20Withdrawal`, `into_inner() -> Ics20Withdrawal`.
- `From<Ics20WithdrawalWithHandler<HI>> for Ics20Withdrawal`: Sarmali kor.
- `Ics20Withdrawal::with_handler<HI>()` — ergonomik constructor.

### Tasarim Kararlari

- `PhantomData<HI>`: Generic tip Rust type system'inde tutulur ama runtime'da yer kaplamaz.
- Bu pattern, farkli host interface'lerin (test vs. prod) ayni Ics20Withdrawal action'i icin kullanilmasina olanak tanir.

### Nova Shield Chain Icin Dersler

- Host interface soyutlamasi: test ortaminda `MockHostInterface`, productionda `TendermintHostInterface` kullanilabilir. Nova Shield IBC entegrasyonu icin bu pattern alinabilir.

---

## [component/metrics.rs]

**Dosya:** `.../component/metrics.rs`
**Satir Sayisi:** 30

### Genel Amac

Shielded pool komponenti icin metrics altyapisi. Simdilik bos (sample code yorumlanmis).

### Tasarim Kararlari

- `pub use metrics::*` ile metrics crate'i monkey-patch edilir. Bu, diger modullerin `use crate::metrics;` yaparak shadowing kullanmasina olanak tanir — baska paket import etmeden metrics fonksiyonlarina ulasilabilir.

### Nova Shield Chain Icin Dersler

- Bu metrics pattern'i (crate-internal shadow) Nova Shield'in kendi metrics entegrasyonu icin standart yapi olabilir. Prometheus/OpenMetrics ile uyumlu.

---

## [component/note_manager.rs]

**Dosya:** `.../component/note_manager.rs`
**Satir Sayisi:** 114

### Genel Amac

Zincirin kendisinin (genesis, ICS-20, vb.) yeni notlar mintlemesini ve kullanici transaction'larindan gelen notlari SCT'ye (State Commitment Tree) eklemesini saglayan trait.

### Trait Implementasyonlari

#### `NoteManager: StateWrite` (public)

##### `mint_note(&mut self, value, address, source) -> Result<()>`
- Satir 24–58:
  1. **Deterministik rseed:** SCT'nin mevcut pozisyonunu al (`self.get_sct().await.position()`).
  2. `blake2b_simd` ile `rseed_bytes` turetiyor: `BLAKE2b(personal=b"PenumbraMint", data=position.to_le_bytes())[0..32]`.
  3. `Note::from_parts(address, value, Rseed(rseed_bytes))` ile notu olustur.
  4. `self.add_note_payload(note.payload(), source)` ile SCT'ye ekle.

  **Neden deterministik rseed?** Public mint notlari gizlilik gerektirmez ama note commitment'in benzersiz olmasi gerekir. SCT pozisyonu her yeni eklemeyle degisir — boylece collision imkansiz.

##### `add_note_payload(&mut self, note_payload, source)`
- Satir 61–78:
  1. `self.add_sct_commitment(note_payload.note_commitment, source)` ile SCT'ye ekle. Pozisyonu al.
  2. `pending_note_payloads` listesine `(position, note_payload, source)` ekle.
  3. `self.object_put(state_key::pending_notes(), payloads)` ile geciici state'e yaz (compact block olusturulana kadar).

##### `add_rolled_up_payload(&mut self, note_commitment, source)`
- Satir 80–101: `add_note_payload` benzeri, ama note payload yerine sadece commitment eklenir (rolled-up/batch commitment). SCT'ye ekler, `pending_rolled_up_payloads` listesine yazar.

##### `pending_note_payloads(&self) -> im::Vector<...>`
- Satir 103–106: Pending notlari object storage'dan al. Yoksa bos vector.

##### `pending_rolled_up_payloads(&self) -> im::Vector<...>`
- Satir 108–111: Ayni pattern, rolled-up payloadlar icin.

### ZK/Kriptografik Formuller

- **Deterministik rseed:** `rseed = BLAKE2b_256(personal="PenumbraMint", SCT_position_le)`
- SCT pozisyonu monoton artar, boylece her mint unique rseed uretir.

### Unsafe Bloklar

Yok.

### Tasarim Kararlari

- **`im::Vector`** (immer crate): Persistent/immutable vector. Thread-safe, snapshot dostu. Rust'taki functional data structure.
- **`object_put`**: State'in geciici (ephemeral) katmanina yazar. Blok sonunda compact block'a donusturulur.
- **TODO yorumu (satir 71-72):** "Neden hata vermiyor? Blok basina commitment butcesi yok (su an implement edilmemis)." — Potansiyel DoS vektoru.

### Nova Shield Chain Icin Dersler

- Deterministik rseed mantigi Nova Shield'in genesis not olusturma ve protokol tarafindan yaratilan not mintleme icin aynen uygulanabilir.
- **TODO'nun gidermesi:** Blok basina maksimum note sayisi siniri konulmasi zorunlu; yoksa blok doldurup DoS yapilabilir.
- `im::Vector` (immer) pattern'i: Nova Shield'in state snapshot mekanizmasi icin incelenmelidir.

---

## [component/rpc.rs]

**Dosya:** `.../component/rpc.rs`
**Satir Sayisi:** 73

### Genel Amac

gRPC `QueryService` implementasyonu. Asset metadata sorgulama.

### Struct/Enum/Type Listesi

- **`Server { storage: Storage }`** — gRPC server yapisi.

### Trait Implementasyonlari

`QueryService for Server`:

#### `asset_metadata_by_id` (Satir 36–65)
- Request'ten `asset::Id` parse eder.
- `state.denom_metadata_by_asset(&id)` ile state'den sorgular.
- Bulunursa `AssetMetadataByIdResponse { denom_metadata }` doner; bulunamazsa bos response.

#### `asset_metadata_by_ids` (Satir 67–73)
- `unimplemented!()` — Henuz implement edilmemis.

### Tasarim Kararlari

- `latest_snapshot()`: Her request icin en guncel snapshot alinir; eski snapshot'lara karsi race condition yok.
- Stream tipi `Pin<Box<dyn Stream<...> + Send>>`: Async streaming response icin standart Rust pattern'i.

### Nova Shield Chain Icin Dersler

- `latest_snapshot()` pattern'i: Her RPC cagrisi en guncel state'i gorur; Nova Shield'in RPC katmani ayni sekilde snapshot-bazli calismalıdir.

---

## [component/rpc/bank_query.rs]

**Dosya:** `.../component/rpc/bank_query.rs`
**Satir Sayisi:** 226

### Genel Amac

Cosmos IBC `BankQuery` interface'ini implemente eder. Toplam arz sorgulama, denom metadata, bakiye sorgulama. Penumbra shielded chain oldugu icin bakiye sorgulari `unimplemented!` doner.

### Trait Implementasyonlari

`BankQuery for Server`:

#### `total_supply` (Satir 39–132)
- Iki kaynagi birlestiriyor:
  1. `state_key::denom_metadata_by_asset::prefix()` ile tum kayitli asset'leri tarar. (Hardcoded 0 arz donuyor — TODO).
  2. `ibc_state_key::ics20_value_balance::prefix()` ile IBC escrow bakiyelerini tarar.
- IBC bakiyeleri: asset_id'yi anahtar path'inden parse edip asset'in metadata'sini bulur, toplama ekler.
- NFT asset'ler (metadata'siz) sessizce atlaniyor.
- Doner: `supply: Vec<Coin>` — denom string ve amount.

#### Implemente Edilmemis Metotlar
- `balance`, `all_balances`, `spendable_balances`, `spendable_balance_by_denom` — Hepsi "penumbra is a shielded chain" mesajiyla `Err(unimplemented)`.
- `supply_of`, `denom_metadata`, `denoms_metadata`, `denom_owners`, `send_enabled`, `denom_metadata_by_query_string`, `denom_owners_by_query` — Hepsi unimplemented.

### Tasarim Kararlari

- `total_supply` icin hardcoded 0 arz (`Amount::from(0u32)`) — sadece IBC tokenlar icin gercek arz gosteriliyor. Native token arzi ayriyeten implement edilmeli.
- NFT atlamasi: `metadata.is_none()` ise `continue` — bu ince bir konvansiyondur.

### Nova Shield Chain Icin Dersler

- IBC uyumluluk icin Cosmos BankQuery interface'ini implement etmek gerekebilir. Penumbra'nin yaklasimiyla, shielded chain'de bakiye sorgulari anlamsiz; sadece IBC escrow bilgileri anlamlidir.

---

## [component/rpc/transfer_query.rs]

**Dosya:** `.../component/rpc/transfer_query.rs`
**Satir Sayisi:** 87

### Genel Amac

IBC Transfer sorgu interface'ini implemente eder (`TransferQuery for Server`).

### Trait Implementasyonlari

`TransferQuery for Server`:
- `denom_traces` (satir 55–86): State'deki tum metadata'yi tarar, `best_effort_ibc_transfer_parse()` ile IBC denom trace formatina cevirmis olanlari filtreler. Pagination yok.
- Diger metotlar (`total_escrow_for_denom`, `escrow_address`, `denom_hash`, `params`, `denom_trace`) — hepsi `unimplemented`.

### Tasarim Kararlari

- `best_effort_ibc_transfer_parse()`: IBC denom'larini best-effort parse et; IBC olmayanlari `None` ile atla.
- Pagination disabled: TODO olarak not edilmis.

### Nova Shield Chain Icin Dersler

- Denom trace sorgulari IBC relayer uyumlulugu icin gerekli. Nova Shield IBC saglayacaksa bu endpoint'leri implement etmek zorunlu.

---

## [component/shielded_pool.rs]

**Dosya:** `.../component/shielded_pool.rs`
**Satir Sayisi:** 164

### Genel Amac

`ShieldedPool` componenti: Genesis baslatma, blok baslangic/bitis ve epoch sonu hook'larini implemente eder. FMD parametre guncellemesini yonetir.

### Struct/Enum/Type Listesi

- **`ShieldedPool {}`** — Bos struct; tum davranis trait implementasyonlarinda.

### Trait Implementasyonlari

#### `Component for ShieldedPool`

##### `init_chain<S: StateWrite>(state, app_state)`
- Satir 27–57:
  - `app_state` yoksa (checkpoint): no-op.
  - Varsa:
    1. `state.put_shielded_pool_params(genesis.shielded_pool_params)` — Parametreler.
    2. `state.put_current_fmd_parameters(fmd::Parameters::default())` — FMD baslatma.
    3. `state.put_previous_fmd_parameters(fmd::Parameters::default())`.
    4. Her `allocation` icin: `raw_amount != 0` assert (sifir not yasakli), `register_denom`, `mint_note(..., CommitmentSource::Genesis)`.

##### `begin_block` (Satir 60–65)
- No-op. Shielded pool blok baslarinda bir sey yapmiyor.

##### `end_block<S: StateWrite>(state, end_block)` (Satir 67–101)
- FMD parametre guncelleme:
  1. `height` al.
  2. `meta_params` al (`fmd_meta_params`).
  3. `should_update_fmd_params(grace_period_blocks, height)` kontrol et: `height % grace_period_blocks == 0` ise guncelle.
  4. Eski FMD params, clue count delta, algorithm state al.
  5. `meta_params.updated_fmd_params(old, algorithm_state, height, clue_count_delta)` hesapla.
  6. Onceki params guncelle, yeni params yaz, algoritma state yaz.

##### `end_epoch` (Satir 103–105)
- No-op.

#### `StateReadExt: StateRead` (Satir 108–136)
- `get_current_fmd_parameters()`, `get_previous_fmd_parameters()`, `get_shielded_pool_params()`, `get_fmd_algorithm_state()`.
- Blanket impl.

#### `StateWriteExt: StateWrite + StateReadExt` (Satir 139–163)
- `put_shielded_pool_params()`, `put_current_fmd_parameters()`, `put_previous_fmd_parameters()`, `put_fmd_algorithm_state()`.
- Blanket impl.

### Tasarim Kararlari

- **FMD guncelleme periyodu:** `height % fmd_grace_period_blocks == 0` — modulo ile deterministik.
- **Assert raw_amount != 0:** Genesis'te sifir not olusturulamaz — bu kritik bir invariant.
- **Arc::get_mut:** `end_block`'ta `Arc<S>` exclusive erisim almayi bekliyor; bu Rust'in tek-sahip mutation modelini goruntular.

### Nova Shield Chain Icin Dersler

- FMD otomatik guncelleme (end_block periyodik hook) Nova Shield'in privacy parametrelerini on-chain governance ile guncellemek icin model.
- Genesis allocation dogrulama (`assert_ne!(raw_amount, 0)`) Nova Shield'in genesis init'inde aynen kullanilmali.

---

## [component/transfer.rs]

**Dosya:** `.../component/transfer.rs`
**Satir Sayisi:** 671

### Genel Amac

ICS-20 Fungible Token Transfer protokolunun tam implementasyonu. Paket gonderme/alma, timeout, acknowledgement, channel yonetimi.

### Struct/Enum/Type Listesi

- **`Ics20Transfer {}`** — Bos struct; tum davranis trait implementasyonlarinda.

### Fonksiyonlar

#### `is_source(source_port, source_channel, denom, is_refund) -> bool` (Satir 55–67)
- Denom string'inin `"{port}/{channel}/"` prefix'iyle basladigini kontrol eder.
- `is_refund` true ise mantik tersine cevirilir (prefixsiz = source).
- **ICS-20 kaynak belirleme logigi:** Token Penumbra'dan mi kokenli, yoksa baska bir zincirden mi koprulenip geldi?

### Trait Implementasyonlari

#### `Ics20TransferReadExt: StateRead`
- `withdrawal_check(withdrawal, current_block_time)`: Paketi `IBCPacket::send_packet_check(...)` ile dogrular.

#### `Ics20TransferWriteExt: StateWrite`
##### `withdrawal_execute(&mut self, withdrawal) -> Result<()>` (Satir 94–200)
- Iki senaryo:
  1. **Kaynak zincir (native token):** `!denom.starts_with(prefix)` — token Penumbra'dan kokenli. Escrow bakiyesine ekle (`checked_add`, overflow kontrol). Paket gonder.
  2. **Hedef zincir (koprolenip gelen):** `denom.starts_with(prefix)` — koprulenip geldi, artik buradan gidiyor. Escrow bakiyesinden cikart (`checked_sub`, underflow kontrol). Bakiye yetersizse bail.
- Her iki durumda `EventOutboundFungibleTokenTransfer` kaydeder.

#### `AppHandlerCheck for Ics20Transfer` (Satir 207–301)
- Channel negotiate kontrolu: her zaman Unordered, "ics20-1" versiyonu.
- `chan_close_init_check`: Her zaman bail — ICS-20 kanal kapatmaya izin vermez.
- `recv_packet_check`: Sadece execute'da yapilir (no-op here).
- `timeout_packet_check`: IBC escrow bakiyesi yeterli mi kontrol eder.

#### `recv_transfer_packet_inner` (Satir 304–476) — private
- Gelen paketi isle:
  1. `FungibleTokenPacketData` JSON parse.
  2. `is_source(port_on_a, chan_on_a, denom, false)` — kaynak zincir mi?
     - Evet: Prefix'i sil, `mint_note(...)` ile Penumbra not olustur, escrow bakiyesini dusur.
     - Hayir: Yeni prefixed denom olustur, register et, `mint_note(...)` ile Penumbra not olustur, escrow bakiyesini artir.
  3. Her iki durumda `EventInboundFungibleTokenTransfer` kaydeder.

#### `refund_tokens` (Satir 479–602) — private
- Timeout veya error ack durumunda token iade:
  1. `is_source(packet.port_on_a, packet.chan_on_a, denom, true)` — kaynak biz miydik?
     - Evet: Escrow bakiyesinden dusur, `mint_note` ile iade.
     - Hayir: `mint_note` ile voucher iade, escrow bakiyesini artir.

#### `AppHandlerExecute for Ics20Transfer` (Satir 605–671)
- Channel lifecycle: hepsi no-op.
- `recv_packet_execute`: `recv_transfer_packet_inner` cagirir. Hata olursa `TokenTransferAcknowledgement::Error(msg)` kaydeder — transaction basarisiz olmaz, sadece error ack yazilir.
- `timeout_packet_execute`: `refund_tokens(..., Timeout)`.
- `acknowledge_packet_execute`: `ack.is_successful()` false ise `refund_tokens(..., Error)`.

### ZK/Kriptografik Formuller

Yok (IBC transfer kriptografi kullanmaz).

### Tasarim Kararlari

- **JSON paket verisi:** IBC spec proto demesine ragmen ibc-go JSON kullanir. Penumbra bu gercegi yorumda belirtmis (`serde_json::from_slice`).
- **`recv_packet` hata toleransi:** Hata olursa transaction basarisiz olmaz; error ack yazilir. Bu ICS-20 spec geregi.
- **overflow/underflow korumalari:** Her bakiye isleminde `checked_add/sub` kullaniliyor. Saturating yerine check — dogru karar, cunku bakiye tutarsizligi kabul edilemez.
- **`saturating_add` bir yerde (satir 456):** Yeni token mintlerken escrow bakiyesi artiriliyor; saturation mantikli (bakiye kaybi olmamali, yukariya gidebilir).

### Nova Shield Chain Icin Dersler

- ICS-20 tam implementasyonu karmasik; `is_source` mantigi ozellikle dikkat ister. Nova Shield IBC implementasyonunda bu kodu referans al.
- `recv_packet` hata toleransi (transaction basarisiz olmaz, sadece error ack): IBC standardina uymak zorundayiz.
- Escrow bakiye yonetimi (checked_add/sub): Nova Shield'in IBC modulu icin aynen kullanilmali.

---

## [convert.rs]

**Dosya:** `.../shielded-pool/src/convert.rs`
**Satir Sayisi:** 335

### Genel Amac

Asset donusumu icin ZK proof sistemi. Bir miktari bir asset'ten digeri icin belli bir kurla donustururken miktari gizli tutar.

### Struct/Enum/Type Listesi

- **`ConvertProofPublic { from: asset::Id, to: asset::Id, rate: U128x128, balance_commitment: balance::Commitment }`**
- **`ConvertProofPrivate { amount: Amount, balance_blinding: Fr }`**
- **`ConvertCircuit { amount, balance_blinding, from, to, rate, balance_commitment }`**
- **`ConvertProof([u8; GROTH16_PROOF_LENGTH_BYTES])`**

### Trait Implementasyonlari

#### `ConstraintSynthesizer<Fq> for ConvertCircuit` (Satir 124–161)
- **Witnesses:** `amount_var`, `balance_blinding_var`
- **Public inputs:** `from`, `to`, `rate`, `balance_commitment`
- **Kisit:**
  ```
  taken    = -amount[from]
  produced = rate * amount[to]
  expected_balance = taken + produced
  expected_commitment = expected_balance.commit(balance_blinding)
  expected_commitment == balance_commitment
  ```
- `rate.apply_to_amount(amount_var)` — U128x128 sabit nokta carpimi devrede.

### ZK/Kriptografik Formuller

**Devre (Circuit) Kisiti:**
```
COMMIT(-amount[from] + (rate * amount)[to], balance_blinding) == balance_commitment
```
- `amount` gizli.
- `from`, `to`, `rate`, `balance_commitment` public.
- Prover: belirli bir miktari belirli bir kurla donusturdugunü balance commitment uzerinden ispatlayabilir, ama miktari aciklamadan.

**`ConvertProof::prove` (Satir 198–211):**
- `Groth16::<Bls12_377, LibsnarkReduction>::create_proof_with_reduction(circuit, pk, blinding_r, blinding_s)`
- `blinding_r`, `blinding_s`: Fq alani uzerinde rastgele proof blinding.

**`ConvertProof::verify` (Satir 215–260):**
- Public inputs sirasi: `from`, `to`, `rate`, `balance_commitment`.
- `Groth16::verify_with_processed_vk(vk, public_inputs, proof)`.

### Test'ler

- **`convert_proof_happy_path`** (proptest): gecerli statement -> check_satisfaction OK, check_circuit_satisfaction OK.
- **`convert_proof_invalid_convert_statement_wrong_rate`** (proptest): yanlis kur -> circuit unsatisfiable.

### Tasarim Kararlari

- `U128x128` sabit nokta: 128.128 bit sabit nokta sayisi; kesirik donusum kurlarini hatasiz temsil eder.
- `DummyWitness for ConvertCircuit`: Proving key setup icin dummy circuit. `amount=1`, `rate=1`, ayni asset.

### Nova Shield Chain Icin Dersler

- ZK convert proof, shielded staking reward dagitimi veya token bridge icin kullanilabilir: miktari gizli tut, kurla donusum dogru yapildigini ispat et.
- `U128x128` sabit nokta kutuphanesi Nova Shield'in precision-sensitive hesaplari icin gerekli.

---

## [event.rs]

**Dosya:** `.../shielded-pool/src/event.rs`
**Satir Sayisi:** 274

### Genel Amac

Shielded pool ile ilgili ABCI event'lerini tanimlar: spend, output, ICS-20 transferler.

### Struct/Enum/Type Listesi

- **`EventSpend { nullifier: Nullifier }`**
- **`EventOutput { note_commitment: StateCommitment }`**
- **`FungibleTokenTransferPacketMetadata { channel: String, sequence: u64 }`**
- **`EventOutboundFungibleTokenTransfer { value: Value, sender: Address, receiver: String, meta: FungibleTokenTransferPacketMetadata }`**
- **`FungibleTokenRefundReason`** enum: `Unspecified = 0, Timeout = 1, Error = 2`
- **`EventOutboundFungibleTokenRefund { value, sender, receiver, reason, meta }`**
- **`EventInboundFungibleTokenTransfer { value, sender: String, receiver: Address, meta }`**

### Trait Implementasyonlari

Her struct icin `TryFrom<pb::*>` ve `From<*> for pb::*` ve `DomainType`. Proto dontusumu icin standart Penumbra pattern.

### Tasarim Kararlari

- Spend event'i sadece nullifier icerir (not commitment degil). Gizlilik: nullifier'dan nota ulasilamaz.
- Output event'i sadece note commitment icerir. Not icerigi sifrelenmis.
- `#[repr(i32)]` ile `FungibleTokenRefundReason`: Proto int ile uyumlu.

### Nova Shield Chain Icin Dersler

- Event tipleri indexing icin gerekli. Kritik: event'lerde minimum veri (sadece nullifier/commitment) — gizliligi korur.
- ICS-20 event'lerinde `channel + sequence` metadata: paket takibi icin gerekli.

---

## [fmd.rs]

**Dosya:** `.../shielded-pool/src/fmd.rs`
**Satir Sayisi:** 310

### Genel Amac

FMD (Fuzzy Message Detection) parametrelerini, meta parametrelerini ve guncelleme algoritmalarini tanimlar.

### Struct/Enum/Type Listesi

- **`FMD_GRACE_PERIOD_BLOCKS_DEFAULT: u64 = 1 << 4 = 16`**
- **`Parameters { precision: Precision, as_of_block_height: u64 }`**
- **`SlidingWindow { window: u32, targeted_detections_per_window: u32 }`**
- **`MetaParametersAlgorithm { Fixed(Precision), SlidingWindow(SlidingWindow) }`**
- **`MetaParameters { fmd_grace_period_blocks: u64, algorithm: MetaParametersAlgorithm }`**
- **`MetaParametersAlgorithmState { Nothing, Fixed, SlidingWindow { approximate_clue_count: u32 } }`**

### Fonksiyonlar

#### `should_update_fmd_params(fmd_grace_period_blocks, height) -> bool` (Satir 14–16)
- `height % fmd_grace_period_blocks == 0`.

#### `SlidingWindow::updated_fmd_params(old, state, height, clue_count_delta) -> (Parameters, State)` (Satir 68–129)
Algoritma adim adim:
1. `window == 0` ise sabit dondur.
2. `new_clues_in_period = clue_count_delta.1.saturating_sub(clue_count_delta.0)`.
3. `projected_clue_count = window * new_clues_in_period` (bu donem gozlemlenen clue'larin pencereye yansitilmasi).
4. Sliding window EMA: `approximate_clue_count = ((window-1) * old + new) / window` (u64 aritmetigi, saturating).
5. `inverse_detection_ratio = approximate_clue_count / targeted_detections_per_window`.
6. Precision: `lg_inverse_ratio = floor(log2(inverse_detection_ratio))`.
   - Sifir ise precision=1 (minimum).
   - Maksimumu asarsa `Precision::MAX`.
   - Aksi halde `Precision::new(lg_inverse_ratio)`.
7. Doner: `(Parameters { precision, as_of_block_height: height }, SlidingWindowState { approximate_clue_count })`.

**FMD precision karar mantigi:**
```
precision = floor(log2(approximate_clue_count / targeted_detections_per_window))
```
Bu, false positive orani 1 / targeted_detections_per_window olacak sekilde precision'u ayarlar. Daha fazla trafik = daha yuksek precision (daha dusuk false positive orani, daha fazla CPU).

#### `MetaParameters::updated_fmd_params(old, state, height, clue_count_delta)` (Satir 282–310)
- Fixed: her zaman ayni precision doner.
- SlidingWindow: `w.updated_fmd_params(...)` delege eder.

### ZK/Kriptografik Formuller

FMD'nin kendisi:
- Her not icin `ClueKey` ile bir `Clue` uretilir.
- Kullanici, clue'lari belirli bir `precision` ile tarayarak kendi notlarini bulabilir.
- Daha yuksek precision = daha az yalanci pozitif, daha fazla hesaplama. Daha dusuk = daha az hesaplama, daha fazla yalanci pozitif.

### Tasarim Kararlari

- **Sliding window EMA:** Kisa vadeli spike'lara karsi direncli.
- **Grace period (16 blok varsayilan):** Kullanicilar parametre degisikligi oncesinde istemci yazilimlarini guncellemeye zaman bulur.
- **`saturating_sub/add`:** Sayac tasmasi/undersow korunmasi.
- `as_of_block_height`: Parametrenin ne zaman gecerli oldugunu istemciler biliyor; yeni parametreden once uretilen clue'lar eski precision ile degerlendirilmeli.

### Nova Shield Chain Icin Dersler

- FMD sistemi, shielded chain'lerde kullanicilarin tum bloklari indirmeden (light client) gelen notlari bulmalarini saglar. Nova Shield gizliligi onceleyecekse FMD veya benzeri bir mekanizma zorunludur.
- Sliding window EMA algoritmasini Nova Shield'in dinamik parametre guncelleme sistemleri icin referans al.

---

## [fmd/state_key.rs]

**Dosya:** `.../shielded-pool/src/fmd/state_key.rs`
**Satir Sayisi:** 26

### Genel Amac

FMD ile ilgili state anahtarlarini sabit string literal'lar olarak tanımlar.

### Anahtarlar

- `shielded_pool/fmd_parameters/current`
- `shielded_pool/fmd_parameters/previous`
- `shielded_pool/fmd_meta_parameters/algorithm_state`
- `shielded_pool/fmd_clue_count/current`
- `shielded_pool/fmd_clue_count/previous`

### Nova Shield Chain Icin Dersler

- State key organizasyonu: hiyerarsik prefix (`shielded_pool/fmd_*/`) ile prefix scan (tum FMD state) mumkun.

---

## [genesis.rs]

**Dosya:** `.../shielded-pool/src/genesis.rs`
**Satir Sayisi:** 94

### Genel Amac

Genesis state: baslangi? token tahsisatlarini ve shielded pool parametrelerini icerir.

### Struct/Enum/Type Listesi

- **`Content { allocations: Vec<Allocation>, shielded_pool_params: ShieldedPoolParameters }`**

### Trait Implementasyonlari

- `DomainType for Content`, `From<Content> for pb::GenesisContent`, `TryFrom<pb::GenesisContent> for Content`.
- `Default for Content`: 4 test tahsisati (1000 penumbra, 100 test_usd, 100 gm, 100 gn).

### Tasarim Kararlari

- Varsayilan genesis test anahtarlarina (`test_keys::ADDRESS_0_STR`, `ADDRESS_1_STR`) tahsisat yapiliyor. Production'da override edilmeli.

### Nova Shield Chain Icin Dersler

- Genesis content Default impl, local test/devnet icin hizli baslangic saglar. Nova Shield'in Genesis modulunde ayni pattern.

---

## [genesis/allocation.rs]

**Dosya:** `.../shielded-pool/src/genesis/allocation.rs`
**Satir Sayisi:** 86

### Genel Amac

Genesis token tahsisatlarini temsil eden `Allocation` struct'i. `raw_denom` + `raw_amount` -> `Value` donusumu.

### Struct/Enum/Type Listesi

- **`Allocation { raw_amount: Amount, raw_denom: String, address: Address }`**

### Fonksiyonlar

- `denom()`: Asset metadata.
- `unit()`: `asset::REGISTRY.parse_unit(&self.raw_denom)`.
- `amount()`: `raw_amount * 10^exponent` — denom birimini baz birime cevirir.
- `value()`: `Value { amount: self.amount(), asset_id: self.unit().id() }`.

### Tasarim Kararlari

- `raw_amount * (10^unit.exponent())`: Orn. `penumbra` exponent=6, `1 penumbra` -> `1_000_000 upenumbra`.
- Debug impl: Address'i to_string() ile yazar (ic bilesenlerini degil).

### Nova Shield Chain Icin Dersler

- Exponent-bazli birim donusumu: Nova Shield'in token decimal yonetiminde ayni pattern. `raw_denom + raw_amount + exponent` uclusu standart.

---

## [ics20_withdrawal.rs]

**Dosya:** `.../shielded-pool/src/ics20_withdrawal.rs`
**Satir Sayisi:** 188

### Genel Amac

ICS-20 cekme (outbound) eyleminin domain tipi. IBC paketi olusturma, validasyon, proto donusum.

### Struct/Enum/Type Listesi

- **`Ics20Withdrawal { amount, denom, destination_chain_address, return_address, timeout_height, timeout_time, source_channel, use_compat_address, ics20_memo, use_transparent_address }`**

### Fonksiyonlar

#### `value(&self) -> Value` / `balance(&self) -> Balance`
- `balance = -Balance::from(value)` — Withdrawal, bakiyeyi azaltir.

#### `packet_data(&self) -> Vec<u8>`
- `FungibleTokenPacketData` -> `serde_json::to_vec`. IBC paketi verisi JSON.

#### `validate(&self) -> Result<()>` (Satir 86–104)
- `timeout_time != 0`: Sifir timeout yasakli.
- `timeout_time % 60_000_000_000 == 0`: Nanosaniye cinsinden, 1 dakikaya yuvarlanmali. **Gizlilik:** clock skew ile kullanici tanimlanmasin diye.

#### `EffectingData::effect_hash(&self) -> EffectHash`
- Proto-encoding hash'i. Transaction authorization icin.

#### `From<Ics20Withdrawal> for pb::FungibleTokenPacketData` (Satir 168–187)
- `use_transparent_address`: Geri adres transparent/bech32-compat olarak encode ediliyor. IBC uyumluluk.
- `use_compat_address`: bech32 (bech32m degil) format.

### Tasarim Kararlari

- **Timeout yuvarlamasi:** Dakikaya yuvarlama gizlilik gerekliligi — zaman damgasi sizi tanimlamamali.
- **`return_address`:** Ephemeral (tek kullanim) adres onerilir — comment'te belirtilmis.
- **`ics20_memo`:** Packet forwarding ve diger protokoller icin.

### Nova Shield Chain Icin Dersler

- Timeout yuvarlamasi (dakikaya) Nova Shield'in IBC modulunde zorunlu.
- `use_compat_address` / `use_transparent_address` secenek: farkli hedef zincirlerle uyumluluk icin kritik.
- Geri adresin ephemeral olmasi onerilmeli (UX katmani).

---

## [lib.rs]

**Dosya:** `.../shielded-pool/src/lib.rs`
**Satir Sayisi:** 40

### Genel Amac

Crate kok dosyasi. Feature flag yonetimi, public API re-exportu.

### Onemli Noktalar

- `#![deny(clippy::unwrap_used)]` — Crate genelinde `unwrap()` yasak! Tum hatalar `?` veya `expect` ile ele alinmali.
- `#[cfg(feature = "component")]` — `component` modulu sadece bu feature aktifken derlenir. Library olarak kullanimda component olmadan kullanilabilir.
- Public re-exportlar: `Note`, `NoteCiphertext`, `NoteView`, `NotePayload`, `Rseed`, `Backref`, `EncryptedBackref`, `ConvertCircuit/Proof`, `NullifierDerivationCircuit/Proof`, `Output/Spend` ve tum alt tipleri.

### Nova Shield Chain Icin Dersler

- `#![deny(clippy::unwrap_used)]` Nova Shield crate'lerinde standart olmali.
- Feature flag ile component/non-component build ayrimi: SDK olarak kullanilabilirlik saglar.

---

## [note.rs]

**Dosya:** `.../shielded-pool/src/note.rs`
**Satir Sayisi:** 681

### Genel Amac

Penumbra'nin temel gizli odeme birimini (note) tanimlar. Icerik, sifreleme, not commitment hesaplama.

### Sabitler

- **`NOTE_LEN_BYTES: usize = 160`** — Plaintext not: 80 (adres) + 16 (amount) + 32 (asset_id) + 32 (rseed) = 160 byte.
- **`NOTE_CIPHERTEXT_BYTES: usize = 176`** — 160 + 16 (AEAD tag) = 176 byte.
- **`NOTECOMMIT_DOMAIN_SEP: Lazy<Fq>`** — `Fq::from_le_bytes_mod_order(blake2b(b"penumbra.notecommit"))`.

### Struct/Enum/Type Listesi

- **`Note { value: Value, rseed: Rseed, address: Address, transmission_key_s: Fq }`**
  - `transmission_key_s`: Adres olusturulurken dogrulanir. EC noktasinin s-bilesenini ayriyeten saklayarak ZK devre icin hazir tutar.
- **`NoteView { value: ValueView, rseed: Rseed, address: AddressView }`** — Gorulebilir (decrypted) not gorunumu.
- **`NoteCiphertext([u8; NOTE_CIPHERTEXT_BYTES])`** — Sifrelenmis not.
- **`Error`** enum: `InvalidNoteCommitment, InvalidTransmissionKey, NoteTypeUnsupported, NoteDeserializationError, InvalidNoteCiphertext, DecryptionError`.

### Fonksiyonlar

#### `Note::controlled_by(&self, fvk) -> bool` (Satir 109–135)
1. `fvk.address_index(&self.address())` ile adres bu FVK'ya ait mi?
2. Transparent adres kontrolu.
3. `transmission_key_matches = *self.transmission_key() == fvk.incoming().diversified_public(...)`.
4. `clue_key` eslesme kontrolu.

#### `Note::from_allocation(allocation) -> Result<Note>` (Satir 141–154)
- `Rseed([0u8; 32])` — Genesis icin deterministik, sifir rseed. (Gizlilik gerektirmiyor.)

#### `Note::from_parts(address, value, rseed) -> Result<Self, Error>` (Satir 156–164)
- `transmission_key_s = Fq::from_bytes_checked(address.transmission_key().0)` — Gecersiz nokta ise `InvalidTransmissionKey`.

#### `Note::encrypt(&self) -> NoteCiphertext` (Satir 236–252)
1. `esk = note.ephemeral_secret_key()` — rseed'den tureten ESK.
2. `epk = esk.diversified_public(diversified_generator)`.
3. `shared_secret = esk.key_agreement_with(transmission_key)` — Diffie-Hellman.
4. `key = PayloadKey::derive(&shared_secret, &epk)`.
5. `key.encrypt(note_plaintext, PayloadKind::Note)`.
6. 176 byte ciphertext.

#### `Note::encrypt_key(&self, ovk, cv) -> OvkWrappedKey` (Satir 255–270)
- Gonderenin kendi notunu tekrar okuyabilmesi icin: `OutgoingCipherKey::derive(ovk, cv, cm, epk)` ile shared secret'i OVK ile sifrele.

#### `Note::decrypt_key` / `Note::decrypt_outgoing` (Satir 273–310)
- OVK ile sifrelenmis shered secret'i ac, sonra notu decrypt et.

#### `Note::decrypt(ciphertext, ivk, epk) -> Result<Note, Error>` (Satir 313–324)
- `shared_secret = ivk.key_agreement_with(epk)` — Alicinin IVK ile DH.
- `key = PayloadKey::derive(&shared_secret, epk)`.
- `decrypt_with_payload_key(...)`.

#### `Note::decrypt_with_payload_key` (Satir 327–349)
- Sifre coz, 160 byte plaintext al.
- **EPK integrity check (ZIP 212 benzeri):** `note.ephemeral_public_key() != *epk` ise hata. Sahte EPK ile saldiri engeli.

#### `Note::commit(&self) -> StateCommitment` (Satir 352–360)
- `commitment(note_blinding, value, diversified_generator, transmission_key_s, clue_key)` cagirir.

#### `commitment(note_blinding, value, div_gen, tk_s, clue_key) -> StateCommitment` (Satir 368–388)
**NOT COMMITMENT FORMULU:**
```rust
commit = poseidon377::hash_6(
    NOTECOMMIT_DOMAIN_SEP,
    (
        note_blinding,          // Fq — rseed'den turetilen
        value.amount.into(),    // Fq
        value.asset_id.0,       // Fq
        div_gen.vartime_compress_to_field(), // Fq — g_d compressed
        transmission_key_s,     // Fq — pk_d.s
        clue_key.0 as Fq        // Fq
    )
)
```
- Poseidon377 6-girdi hash. Domain separator ile alan injection. 6 alan elemani -> 1 alan elemani.

#### `commitment_from_address` (Satir 391–411)
- Ayni formul, adres bilesenlerinden hesaplar.

### Byte Serializasyonu (Satir 481–541)

**`From<&Note> for [u8; NOTE_LEN_BYTES]`:**
```
bytes[0..80]   = address (80 byte)
bytes[80..96]  = value.amount.to_le_bytes() (16 byte = u128)
bytes[96..128] = value.asset_id.0.to_bytes() (32 byte)
bytes[128..160]= rseed.to_bytes() (32 byte)
```

### Test'ler

- **`note_encryption_and_decryption`**: Sifrele, IVK ile coz, eslestigini dogrula. Yanlis IVK ile decrypt -> hata.
- **`note_encryption_and_sender_decryption`**: OVK ile kendi gonderdigini decrypt et.
- **`note_decryption_fails_with_incorrect_epk`**: Yanlis EPK ile decrypt -> hata (ZIP 212 benzeri check).

### ZK/Kriptografik Formuller (Ozet)

1. **Note commitment:** Poseidon377-6 hash (domain sep, blinding, amount, asset_id, g_d, pk_d_s, clue_key).
2. **Note sifreleme:** ESK = prf(rseed, 4), EPK = ESK * g_d, shared = ESK * pk_d, key = KDF(shared, epk), ciphertext = ChaCha20Poly1305(key, plaintext).
3. **OVK wrapped key:** OCK = KDF(OVK, cv, cm, epk), wrapped = OCK.encrypt(shared).

### Tasarim Kararlari

- **transmission_key_s saklama:** ZK devresinde EC nokta sikistrirma pahali; s-bilesenini ayriyeten saklamak devreyi hizlandirir.
- **EPK integrity check:** ZIP 212 benzeri koruma. Sahte EPK ile privacy leak'ini engeller.
- **Domain separator (NOTECOMMIT_DOMAIN_SEP):** Farkli hash kullanim noktalarinin birbirine karismamasini saglar.

### Nova Shield Chain Icin Dersler

- Note commitment formulu (Poseidon377-6) ZK devresi ile uyumlu; ayni hashin devre icinde yeniden hesaplanabilmesi kritik.
- EPK integrity check (ZIP 212): Nova Shield'in note sifreleme sisteminde mutlaka yer almali.
- OVK wrapped key mekanizmasi: gonderenin kendi gonderdigi notlara erisimi. Nova Shield'de sender privacy + sender access birlikte saglanmali.
- Transmission key s-bileseni saklamak devreyi hizlandirir: Nova Shield'in ZK notu icin ayni tasarim.

---

## [note/r1cs.rs]

**Dosya:** `.../shielded-pool/src/note/r1cs.rs`
**Satir Sayisi:** 114

### Genel Amac

`NoteVar` — Not'un ZK devresi icindeki temsili. Poseidon377 devrede not commitment hesaplar.

### Struct/Enum/Type Listesi

- **`NoteVar { value: ValueVar, note_blinding: FqVar, address: AddressVar }`**

### Trait Implementasyonlari

#### `AllocVar<Note, Fq> for NoteVar` (Satir 55–76)
- `note_blinding = FqVar::new_variable(...)`.
- `value = ValueVar::new_variable(...)`.
- `address = AddressVar::new_variable(...)`.
- Mode: Witness veya Public Input olarak tahsis edilebilir.

#### `ToConstraintField<Fq> for Note` (Satir 78–87)
- `[note_blinding, ...value, ...address]` field element listesi.

### Fonksiyonlar

#### `NoteVar::commit(&self) -> Result<StateCommitmentVar, SynthesisError>` (Satir 94–113)
```rust
commitment = poseidon377::r1cs::hash_6(
    cs,
    &domain_sep,
    (
        self.note_blinding.clone(),
        self.value.amount(),
        self.value.asset_id(),
        compressed_g_d,          // address.diversified_generator.compress_to_field()
        pk_d_s,                   // address.transmission_key.compress_to_field()
        self.address.clue_key(),
    )
)
```
Bu, `note.rs`'daki `commitment()` fonksiyonunun devre icindeki karsiligi. Ayni formul, R1CS kisitlari ile.

### Tasarim Kararlari

- `R1CSVar` implemente edilmemis: not'u rseed olmadan reconstruct etmek mumkun degil — `Note` tipine tam donusum yapilamaz.

### Nova Shield Chain Icin Dersler

- ZK devresi icindeki `commit()` ve native kod `commit()` AYNI formulu kullanmali. Nova Shield'in spend/output devrelerinde bu paralellik zorunlu; aksi halde proof sahtekarligi imkani dogabilir.

---

## [note_payload.rs]

**Dosya:** `.../shielded-pool/src/note_payload.rs`
**Satir Sayisi:** 102

### Genel Amac

`NotePayload` — Zincire yazilan nota bilgisi: commitment (public), ephemeral key (public), encrypted note (sifrelenmis).

### Struct/Enum/Type Listesi

- **`NotePayload { note_commitment: StateCommitment, ephemeral_key: ka::Public, encrypted_note: NoteCiphertext }`**

### Fonksiyonlar

#### `NotePayload::trial_decrypt(&self, fvk: &FullViewingKey) -> Option<Note>` (Satir 20–58)
1. `Note::decrypt(&self.encrypted_note, fvk.incoming(), &self.ephemeral_key)` — IVK ile decrypt dene.
2. Basarisiz -> `None` (bu not bize ait degil).
3. `note.amount() == 0` ise `None` + debug log.
4. `!note.controlled_by(fvk)` ise `None` + warn log.
5. `note.commit() != self.note_commitment` ise `None` + warn log.
6. Tum kontroller gecerliyse `Some(note)`.

**Onemli yorum (satir 48–57):** REJECT saldirisi (CVE-2019-16930, ZCash timing analizi). Hatalar panic etmemeli; hatali notlar sessizce dusurulmeli. IP adresi ile "bu notu decrypt edebildim" iliskisi kurulmamali.

### Tasarim Kararlari

- **`Option<Note>` doner, `Result<Note>` degil:** Hatalar ic detay; dis dunyaya sadece "bulundu/bulunamadi" bilgisi gider. Timing/oracle saldiri direnci.
- **Warn vs. debug:** Sifir miktar debug (honest durum olabilir — swap claim, dummy spend). FVK mismatch warn (hic bir honest durumda olmamali).

### Nova Shield Chain Icin Dersler

- `trial_decrypt` pattern'i: Nova Shield'in not tarama (note scanning) servisi icin tam referans. REJECT saldirisi goz onunde bulundurulmali.
- `Option<Note>` donusu: panic veya error propagation yerine sessiz dusurme. View service'in bu patterni kesinlikle benimsemesi gerek.

---

## [nullifier_derivation.rs]

**Dosya:** `.../shielded-pool/src/nullifier_derivation.rs`
**Satir Sayisi:** 322

### Genel Amac

Nullifier turetme icin Groth16 ZK proof sistemi. Musteri tarafi (client-side) kullanim: kullanici bir notun nullifier'ini bildigi nullifier key ile turetip olusturabilir.

### Struct/Enum/Type Listesi

- **`NullifierDerivationProofPublic { position: tct::Position, note_commitment: StateCommitment, nullifier: Nullifier }`**
- **`NullifierDerivationProofPrivate { nk: NullifierKey }`**
- **`NullifierDerivationCircuit { public, private }`**
- **`NullifierDerivationProof([u8; GROTH16_PROOF_LENGTH_BYTES])`**

### Trait Implementasyonlari

#### `ConstraintSynthesizer<Fq> for NullifierDerivationCircuit` (Satir 88–107)
- Witnesses: `nk_var`
- Public inputs: `claimed_nullifier_var`, `note_commitment_var`, `position_var`
- Kisit:
  ```
  nullifier_var = NullifierVar::derive(&nk_var, &position_var, &note_commitment_var)
  nullifier_var == claimed_nullifier_var
  ```

**Guvenlik notu (satir 77–82):** "Bu proof sadece istemci tarafinda kullanilir ve zincir uzerinde degil. Nullifier turetme anahtari, devrede not commitment ile iliskilendirilmemis." — Yani bu proof, `nk`'nin `note_commitment`'in anahtarini bilen kisi oldugunu ispatlayamaz; sadece `nullifier = derive(nk, position, cm)` denkleminin tutarli oldugunu ispatlayabilir.

### ZK/Kriptografik Formuller

**Nullifier turetme (NullifierVar::derive):**
- Penumbra SCT implementasyonunda: `nullifier = PRF(nk, position || note_commitment)` — Poseidon-tabanli PRF.

### Test'ler

- **`nullifier_derivation_proof_happy_path`** (proptest): Gecerli statement -> OK.
- **`nullifier_derivation_proof_unhappy_path`** (proptest): `incorrect_nk = nk + random` -> circuit unsatisfiable.

### Tasarim Kararlari

- Client-side only: Bu proof on-chain kullanilmaz. Kullanicinin kendi notunun nullifier'ini hesapladiginin kaniti — diger kisi ile paylasim icin.

### Nova Shield Chain Icin Dersler

- Nullifier turetme ZK proof'u Nova Shield'in gelecekte nullifier delegation (baskasinin sizin adina harcama yapabilmesi) veya audit mekanizmasi icin kullanilabilir.

---

## [output.rs] / [output/action.rs]

**Dosya:** `.../shielded-pool/src/output.rs` (5 satir, re-export) + `output/action.rs` (122 satir)
**Satir Sayisi:** 127 (toplam)

### Genel Amac

`Output` action: yeni sifrelenmis nota olusturma. Body, proof, OVK wrapped key ve wrapped memo key icerir.

### Struct/Enum/Type Listesi

- **`Output { body: Body, proof: OutputProof }`**
- **`Body { note_payload: NotePayload, balance_commitment: balance::Commitment, ovk_wrapped_key: OvkWrappedKey, wrapped_memo_key: WrappedMemoKey }`**

### Trait Implementasyonlari

- `EffectingData for Body/Output`: `effect_hash` -> proto-encoding hash'i.
- `DomainType, From/TryFrom` proto donusum impls.

### Tasarim Kararlari

- Body'de uc kriptografik nesne: `NotePayload` (alici icin), `ovk_wrapped_key` (gonderenin kendi notuna erisimi), `wrapped_memo_key` (memo sifreleme).

### Nova Shield Chain Icin Dersler

- Output body tasarimi: alici, gonderici ve memo katmanlari ayri sifreleme. Nova Shield'in note sistemi bu uc katmani desteklemeli.

---

## [output/plan.rs]

**Dosya:** `.../shielded-pool/src/output/plan.rs`
**Satir Sayisi:** 218

### Genel Amac

`OutputPlan` — Not gondermeden once istemci tarafinda hazirlik. ZK proof uretimi dahil.

### Struct/Enum/Type Listesi

- **`OutputPlan { value, dest_address, rseed, value_blinding, proof_blinding_r, proof_blinding_s }`**

### Fonksiyonlar

#### `OutputPlan::new(rng, value, dest_address) -> OutputPlan` (Satir 30–45)
- `rseed = Rseed::generate(rng)`.
- `value_blinding = Fr::rand(rng)`.
- `proof_blinding_r, proof_blinding_s = Fq::rand(rng)`.

#### `OutputPlan::dummy(rng) -> OutputPlan` (Satir 48–58)
- Sifir degerli dummy output: MEV onleme / trafigin gizlenmesi icin.

#### `output_proof(&self) -> OutputProof` (Satir 76–94)
- `note = self.output_note()`.
- `balance_commitment = self.balance().commit(self.value_blinding)`.
- `note_commitment = note.commit()`.
- `OutputProof::prove(proof_blinding_r, proof_blinding_s, pk, public, private)`.

#### `output_body(&self, ovk, memo_key) -> Body` (Satir 97–120)
- `ovk_wrapped_key = note.encrypt_key(ovk, balance_commitment)`.
- `wrapped_memo_key = WrappedMemoKey::encrypt(memo_key, esk, transmission_key, div_gen)`.
- Body olustur.

#### `balance(&self) -> Balance` (Satir 127–129)
- `-Balance::from(self.value)` — Output bakiyeyi azaltir.

### Test'ler

- **`check_output_proof_verification`**: Plan olustur, proof uret, verify et.

### Nova Shield Chain Icin Dersler

- Plan/Action ayrimi: istemci planlama (rasgelevelik, proof uretimi) vs. zincir dogrulama. Nova Shield'in transaction builder'i bu ayirimi korumali.
- `proof_blinding_r/s`: Groth16'nin deterministik olmayan proof uretimi icin. Nova Shield'in ZK sisteminde de bu blinding gerekli.

---

## [output/proof.rs]

**Dosya:** `.../shielded-pool/src/output/proof.rs`
**Satir Sayisi:** 405

### Genel Amac

Output ZK proof sistemi (Groth16/BLS12-377). Yeni notun dogru olusturuldugunu ispatlayan devreyi tanimlar.

### Struct/Enum/Type Listesi

- **`OutputProofPublic { balance_commitment: balance::Commitment, note_commitment: StateCommitment }`**
- **`OutputProofPrivate { note: Note, balance_blinding: Fr }`**
- **`OutputCircuit { public, private }`**
- **`OutputProof([u8; GROTH16_PROOF_LENGTH_BYTES])`**

### Trait Implementasyonlari

#### `ConstraintSynthesizer<Fq> for OutputCircuit` (Satir 111–136)
**Witnesses:**
- `note_var = NoteVar` (not icerigi — g_d, pk_d, v, rseed, note_blinding)
- `balance_blinding_vars`

**Public inputs:**
- `claimed_note_commitment`
- `claimed_balance_commitment`

**Kisitlar:**
1. **Note commitment integrity:**
   ```
   note_var.commit() == claimed_note_commitment
   ```
2. **Balance commitment integrity:**
   ```
   BalanceVar::from_negative_value_var(note_var.value()).commit(balance_blinding) == claimed_balance_commitment
   ```

**Yorum (satir 114):** NoteVar tahsisinde diversified base identity kontrolu yapiliyor.

### ZK/Kriptografik Formuller

**Output devresinin ispatladi?i:**
```
ncm = Poseidon377_6(domain_sep, note_blinding, amount, asset_id, g_d_compressed, pk_d_s, clue_key)
vcm = COMMIT(-value, balance_blinding)  =  (-value) * G + balance_blinding * H
ncm == public.note_commitment
vcm == public.balance_commitment
g_d != identity
```

### Test'ler

- **`output_proof_happy_path`**: Gecerli statement -> OK.
- **`output_proof_verification_fails_note_commitment_integrity`**: Yanlis note blinding -> fail.
- **`output_proof_verification_fails_balance_commitment_integrity`**: Yanlis value blinding -> fail.

### Tasarim Kararlari

- **`DummyWitness for OutputCircuit`**: Proving key setup icin deterministik dummy circuit.
- Balance commitment negatif value: Output bakiyeyi dusurur.

### Nova Shield Chain Icin Dersler

- Output devresinin iki kisiti (not commitment + balance commitment) asgari set. Nova Shield bu devreyi aynen kullanabilir.
- `DummyWitness` trait'i: Proving key setup'i kolaylastirir, her circuit turu icin implement edilmeli.

---

## [output/view.rs]

**Dosya:** `.../shielded-pool/src/output/view.rs`
**Satir Sayisi:** 96

### Genel Amac

`OutputView` — Output'un kullanici perspektifinden gorunumu: Visible (decrypt edilebilmis) veya Opaque (decrypt edilemez).

### Struct/Enum/Type Listesi

- **`OutputView`** enum:
  - `Visible { output: Output, note: NoteView, payload_key: PayloadKey }` — Alici veya gonderici gorebilir.
  - `Opaque { output: Output }` — Sifreli, icerik gorulemez.

### Tasarim Kararlari

- `#[allow(clippy::large_enum_variant)]`: Visible varyant buyuk; `Box` kullanmak yerine dogrudan tutmak performans tercihi.
- `From<OutputView> for Output`: Goruntuden ham output alinabilir.

### Nova Shield Chain Icin Dersler

- View/opaque enum pattern'i: kullanici belirli bir FVK ile gorebildigi notlari Visible, goremedigini Opaque alir. Nova Shield'in view service'i ayni pattern'i kullanmali.

---

## [params.rs]

**Dosya:** `.../shielded-pool/src/params.rs`
**Satir Sayisi:** 43

### Genel Amac

`ShieldedPoolParameters` — FMD meta parametrelerini sarar.

### Struct/Enum/Type Listesi

- **`ShieldedPoolParameters { fmd_meta_params: fmd::MetaParameters }`**

### Tasarim Kararlari

- Default impl: `fmd::MetaParameters::default()`.
- `#[allow(deprecated)]` on-chain proto'da eski `fixed_fmd_params` field var, `None` ataniyor.

### Nova Shield Chain Icin Dersler

- Parametreler tek bir struct'ta toplamak: governance ile kolayca guncelleme.

---

## [rseed.rs]

**Dosya:** `.../shielded-pool/src/rseed.rs`
**Satir Sayisi:** 33

### Genel Amac

`Rseed` — Not olusturmada kullanilan 32-byte rastgele deger. ESK ve not blinding factor'un turetimine kaynak.

### Struct/Enum/Type Listesi

- **`Rseed(pub [u8; 32])`** — newtype.

### Fonksiyonlar

#### `Rseed::generate<R>(rng) -> Self` (Satir 12–16)
- 32 byte rastgele doldur.

#### `Rseed::derive_esk(&self) -> ka::Secret` (Satir 19–22)
- `prf::expand(b"Penumbra_DeriEsk", &self.0, &[4u8])` -> `Fr::from_le_bytes_mod_order(hash)`.
- PRF: BLAKE2b-512 ile expand, sonuc Fr alani elemani.

#### `Rseed::derive_note_blinding(&self) -> Fq` (Satir 25–28)
- `prf::expand(b"Penumbra_DeriRcm", &self.0, &[5u8])` -> `Fq::from_le_bytes_mod_order(hash)`.
- Domain ayirma: ESK icin `4u8`, not blinding icin `5u8`.

### ZK/Kriptografik Formuller

**PRF Expand (BLAKE2b-512):**
```
ESK    = Fr( BLAKE2b(key=rseed, data=[4u8]) )
nblind = Fq( BLAKE2b(key=rseed, data=[5u8]) )
```
- Domain ayirma byte'i: farkli derivasyonlarin cakismasini engeller.

### Tasarim Kararlari

- Tek rastgele kaynaktan (rseed) hem ESK hem not blinding turetur. Zcash Sapling'e benzer tasarim.
- `4u8` ve `5u8` domain separatorlari: farkli kullanim noktalari ayri degerleri turetir.

### Nova Shield Chain Icin Dersler

- Rseed patterni Nova Shield'in note sistemi icin directly kullanilabilir.
- Domain separator byte'lari: her turetim noktas icin farkli sabit deger kullan (PRF separation).

---

## [spend.rs] / [spend/action.rs]

**Dosya:** `.../shielded-pool/src/spend.rs` (9 satir, re-export) + `spend/action.rs` (145 satir)
**Satir Sayisi:** 154 (toplam)

### Genel Amac

`Spend` action: mevcut bir notu harcama. Nullifier, randomize verify key, balance commitment ve sifrelenmis backref icerir.

### Struct/Enum/Type Listesi

- **`Spend { body: Body, auth_sig: Signature<SpendAuth>, proof: SpendProof }`**
- **`Body { balance_commitment, nullifier, rk: VerificationKey<SpendAuth>, encrypted_backref: EncryptedBackref }`**

### Trait Implementasyonlari

- `EffectingData for Body/Spend`.
- Proto donusum: Body parse ederken `encrypted_backref` 0 veya 48 byte kontrol edilir. Baska uzunluk -> hata.

### Tasarim Kararlari

- `rk` (randomize verification key): Spend key'den tureyip auth sig dogrular. Randomizer transaction'a ozgu — anahtar tekrar kullanimi engellenir.
- `encrypted_backref`: Opsiyonel (0 byte = yok, 48 byte = var). Gizli gecmis izleme icin.

### Nova Shield Chain Icin Dersler

- Randomize verification key (rk) pattern'i: her transaction icin taze key materyali. Nova Shield spend auth icin bu zorunlu.

---

## [spend/plan.rs]

**Dosya:** `.../shielded-pool/src/spend/plan.rs`
**Satir Sayisi:** 176

### Genel Amac

`SpendPlan` — Harcama isleminin istemci tarafi planlamasi.

### Struct/Enum/Type Listesi

- **`SpendPlan { note, position, randomizer, value_blinding, proof_blinding_r, proof_blinding_s }`**

### Fonksiyonlar

#### `spend_body(&self, fvk) -> Body` (Satir 77–87)
1. `backref = Backref::new(self.note.commit())`.
2. `encrypted_backref = backref.encrypt(&fvk.backref_key(), &self.nullifier(fvk))`.
3. `balance_commitment = self.balance().commit(self.value_blinding)`.
4. `nullifier = self.nullifier(fvk)`.
5. `rk = self.rk(fvk)`.

#### `rk(&self, fvk) -> VerificationKey<SpendAuth>` (Satir 90–92)
- `fvk.spend_verification_key().randomize(&self.randomizer)`.

#### `nullifier(&self, fvk) -> Nullifier` (Satir 95–98)
- `nk = fvk.nullifier_key()`.
- `Nullifier::derive(nk, self.position, &self.note.commit())`.

#### `spend_proof` (Satir 101–129)
- `SpendProof::prove(proof_blinding_r, proof_blinding_s, pk, public, private)`.

### Nova Shield Chain Icin Dersler

- Backref her spend'de otomatik olusturulur. Gizlilik katmani: backref gidip gelmeyi izlememizi saglarken disaridan anlasılamaz.

---

## [spend/proof.rs]

**Dosya:** `.../shielded-pool/src/spend/proof.rs`
**Satir Sayisi:** ~620+ satir

### Genel Amac

Spend ZK proof sistemi. Penumbra'nin en kapsamli ve onemli ZK devresi.

### Struct/Enum/Type Listesi

- **`SpendProofPublic { anchor: tct::Root, balance_commitment, nullifier, rk }`**
- **`SpendProofPrivate { state_commitment_proof, note, v_blinding, spend_auth_randomizer, ak, nk }`**
- **`SpendCircuit { public, private }`**
- **`VerificationError`** enum: `ProofDeserialize, Anchor, BalanceCommitment, Nullifier, DecompressRk, Rk, StartPosition, SynthesisError, InvalidProof`.
- **`SpendProof([u8; GROTH16_PROOF_LENGTH_BYTES])`**

### Trait Implementasyonlari

#### `ConstraintSynthesizer<Fq> for SpendCircuit` (Satir 144–216)

**Witnesses:**
- `note_var` (NoteVar): not icerigi
- `claimed_note_commitment`: SCT proof'undaki commitment
- `position_var`, `merkle_path_var` (tct::r1cs::MerkleAuthPathVar): SCT Merkle auth path
- `v_blinding_vars`
- `spend_auth_randomizer_var`
- `ak_element_var` (AuthorizationKeyVar)
- `nk_var` (NullifierKeyVar)

**Public inputs:**
- `anchor_var` (SCT root)
- `claimed_balance_commitment_var`
- `claimed_nullifier_var`
- `rk_var` (RandomizedVerificationKey)

**Kisitlar (6 adet):**

1. **Note commitment integrity (satir 181–182):**
   ```
   note_var.commit() == claimed_note_commitment
   ```

2. **Nullifier integrity (satir 184–186):**
   ```
   NullifierVar::derive(&nk_var, &position_var, &claimed_note_commitment) == claimed_nullifier_var
   ```

3. **Merkle auth path dogrulama (satir 188–199):**
   ```
   is_not_dummy = (note_var.amount() != 0)
   IF is_not_dummy:
       merkle_path_var.verify(cs, &is_not_dummy, &position_bits, anchor_var, claimed_note_commitment)
   ```
   Dummy spend (sifir miktar) SCT'de yoktur; Merkle dogrulama atlanir.

4. **Randomize verification key integrity (satir 201–203):**
   ```
   ak.randomize(spend_auth_randomizer) == rk_var
   ```

5. **Diversified address integrity (satir 205–209):**
   ```
   ivk = IncomingViewingKeyVar::derive(&nk_var, &ak_element_var)
   ivk.diversified_public(&note_var.diversified_generator()) == note_var.transmission_key()
   ```
   Prover'in `nk` ve `ak` bilesenlerinin notun adresine karsilik geldigini ispatlayamadan harcayamadigini garanti eder.

6. **Balance commitment integrity (satir 211–213):**
   ```
   note_var.value().commit(v_blinding) == claimed_balance_commitment_var
   ```

### ZK/Kriptografik Formuller (Tam)

**Spend devresi ispatladi?i:**
```
ncm        = Poseidon377_6(domain_sep, nblind, amount, asset_id, g_d, pk_d_s, clue_key)
nullifier  = PRF_nf(nk, position || ncm)
SCT_path   = valid Merkle auth path from ncm to anchor     [dummy skip if amount=0]
rk         = ak.randomize(spend_auth_randomizer)
ivk        = derive_ivk(nk, ak)
pk_d       = ivk * g_d                                    [address ownership proof]
vcm        = value * G + v_blinding * H                    [Pedersen commitment]

All: ncm == note_commitment (public)
     nullifier == public.nullifier
     rk == public.rk
     vcm == public.balance_commitment
     g_d != identity (ak != identity)
```

### Test'ler

- **`spend_proof_happy_path`** (proptest): Gecerli -> OK.
- **`spend_proof_verification_merkle_path_integrity_failure`** (proptest): Yanlis anchor -> fail.
- Diversified address, nullifier, rk, balance commitment integrity testleri.

### Tasarim Kararlari

- **Dummy spend shortcircuit:** Sifir degerli notlar SCT'ye eklenmez ama ZK devresi gelistirilir — trafik analizi engellemek icin her transaction dummyla tamamlanir.
- **`tap::Tap`:** Functional debug loglama — ara degerleri etkilemeden logla.
- **`VerificationError` typed:** Hata kategorisi bilgisi tasir; sadece `anyhow` yerine daha zengin hata tipi.

### Nova Shield Chain Icin Dersler

- Spend devresi (6 kisit) Nova Shield'in temel spend proof devresi icin dogrudan referans.
- Dummy spend SCT'ye eklenmez ama proof uretilir — Nova Shield'in privacy stratejisinde dummy spend mutlaka desteklenmeli.
- `VerificationError` typed hata enum'u: proof dogrulama hatalari icin zengin diagnostik bilgisi.
- Diversified address kisiti (ivk * g_d = pk_d): Prover'in sahip olmadigi bir adrese ait notu harcayamamasini saglar.

---

## [spend/view.rs]

**Dosya:** `.../shielded-pool/src/spend/view.rs`
**Satir Sayisi:** 73

### Genel Amac

`SpendView` — Spend'in kullanici perspektifinden gorunumu.

### Struct/Enum/Type Listesi

- **`SpendView`** enum:
  - `Visible { spend: Spend, note: NoteView }` — NoteView ile birlikte; hangi notu harcadigini gorebilir.
  - `Opaque { spend: Spend }` — Gizli; sadece nullifier ve balance commitment gorunur.
- `From<SpendView> for Spend`.

### Nova Shield Chain Icin Dersler

- Spend de Visible/Opaque olarak ayrilabilir. Gonderici FVK'sinin hangi spendleri goruntuledigini bilmek Nova Shield'in audit trail mekanizmasi icin gerekli.

---

## [state_key.rs]

**Dosya:** `.../shielded-pool/src/state_key.rs`
**Satir Sayisi:** 27

### Genel Amac

Shielded pool state anahtarlarini merkezi olarak tanimlar.

### Anahtarlar

- `shielded_pool/assets/{asset_id}/denom` — Asset metadata.
- `shielded_pool/pending_notes` — Bekleyen notlar (ephemeral).
- `shielded_pool/pending_rolled_up_payloads` — Bekleyen rolled-up payloadlar (ephemeral).
- `shielded_pool/params` — Pool parametreleri.

### Tasarim Kararlari

- `denom_metadata_by_asset::prefix()` -> `"shielded_pool/assets/"` — Tum asset'leri prefix scan ile sorgulamak mumkun.
- `by_asset_id` string format: `"shielded_pool/assets/{asset_id}/denom"` — asset_id'nin Display impl'ini kullanir.

### Nova Shield Chain Icin Dersler

- State key organizasyonu: hierarchical namespace (`shielded_pool/*/`). Nova Shield'in state katmani bu hiyerarsiyi kullanmali; prefix scan ile kategorilere gore sorgulama saglar.

---

## Genel Mimari Analiz

### Shielded Pool Sistemi — Buyuk Resim

```
Kullanici -> OutputPlan/SpendPlan -> (ZK proof uret) -> Output/Spend action
                                                              |
                                                    ActionHandler::check_stateless
                                                    (ZK proof dogrula)
                                                              |
                                                    ActionHandler::check_and_execute
                                                    (State guncelle: SCT, nullifier)
                                                              |
                                                    NoteManager::add_note_payload
                                                    (SCT'ye ekle, compact block'a planla)
                                                              |
                                                    end_block -> FMD guncelleme
```

### Kriptografik Stack

| Katman | Kullanilan |
|---|---|
| ZK proof sistemi | Groth16 / BLS12-377 / arkworks |
| Eliptik egri | decaf377 (ristretto benzeri, cofactor-1) |
| Hash | Poseidon377 (ZK uyumlu), BLAKE2b (non-ZK) |
| Not sifreleme | ChaCha20Poly1305 (AEAD) |
| Key agreement | Decaf377 ECDH (ka::Secret, ka::Public) |
| PRF | BLAKE2b-expand (Penumbra_DeriEsk, Penumbra_DeriRcm) |
| Backref sifreleme | ChaCha20Poly1305 (BackreferenceKey, Nullifier nonce) |
| SCT | Tiered Commitment Tree (tct) |

### Note Lifecycle

```
rseed (32 byte random)
  |-> ESK = PRF(rseed, [4]) = BLAKE2b(key=rseed, b"Penumbra_DeriEsk", [4])
  |-> note_blinding = PRF(rseed, [5]) = BLAKE2b(...)
  |
Note = (value, rseed, address, transmission_key_s)
  |-> commit() = Poseidon377_6(domain_sep, blinding, amount, asset_id, g_d, pk_d_s, clue_key)
  |-> encrypt() = ChaCha20Poly1305(PayloadKey(ESK*pk_d, EPK), plaintext)
  |-> NotePayload = (commitment, EPK, ciphertext)
  |-> SCT'ye eklenir
  |
Spend:
  nullifier = PRF_nf(nk, position || commitment)
  SpendProof = Groth16(circuit, witnesses, public_inputs)
  EncryptedBackref = ChaCha20Poly1305(brk, nullifier[..12], commitment)
```

---

## Kritik Guvenlik Notlari

1. **Dummy spend:** Sifir miktarli notlar SCT'ye eklenmez ama ZK proof uretilir. Trafik analizi engeli. `is_not_dummy = amount != 0` devrede shortcircuit.

2. **EPK integrity (ZIP 212):** `note.ephemeral_public_key() != epk` ise decrypt reject. Sahte EPK ile privacy leak engeli.

3. **Timeout yuvarlamasi:** ICS-20 withdrawal timeout'u dakikaya yuvarlanmali. Clock skew ile kullanici tanimlama engeli.

4. **REJECT saldirisi (CVE-2019-16930):** `trial_decrypt` sonucu `Option<Note>` — hata panic veya Result propogation yapmaz. IP adresi ile "decrypt edebildim" iliskisi kurulamaz.

5. **Nullifier cift harcama:** `check_nullifier_unspent` + `nullify` atomik cift harcama onleme.

6. **Effect hash imzalama:** Spend auth imzasi tum transaction etkisini (effect_hash) imzalar; sadece nullifier degil. Replay engellenmiş.

7. **Escrow overflow/underflow:** ICS-20 bakiye islemlerinde `checked_add/sub` kullaniliyor; hata halinde bail (sessiz truncation yok).

8. **`#![deny(clippy::unwrap_used)]`:** Crate genelinde `unwrap()` yasak; tum hatalar explicit ele alinmali.

---

## Toplam Degerlendirme

Penumbra'nin shielded pool kodu:
- Production kalitesi, kapsamli test coverage (proptest ile property-based testing).
- Groth16/BLS12-377 uzerinde iki ana ZK proof (Spend, Output) + iki yardimci (Convert, NullifierDerivation).
- ICS-20 tam implementasyonu; IBC uyumlulugu.
- FMD privacy mekanizmasi: kullanici basina FMD precision; on-chain otomatik guncelleme.
- Nullifier-tabanlı cift harcama onleme; SCT Merkle path dogrulama.

Zayif nokta: Blok basina commitment butcesi yok (TODO yorumu). DoS vektoru.

---

## Nova Shield Chain Icin Genel Dersler

1. **Note commitment formulu:** Poseidon377-6 (blinding, amount, asset_id, g_d, pk_d_s, clue_key) — ZK uyumlu, 6 alan elemani.
2. **Spend ZK devresinin 6 kisiti:** Zorunlu minimum; Nova Shield'in spend devresi bu 6 kisiti icermeli.
3. **Dummy spend:** Sifir miktarli, SCT'ye eklenmeyen, ama proof uretilen spend — MEV ve trafik analizi engeli icin Nova Shield'de destek verilmeli.
4. **EPK integrity check (ZIP 212):** Sifre cozme sirasinda EPK tutarlilik kontrolu mutlaka yapilmali.
5. **OVK wrapped key:** Gonderenin kendi gonderdigi notlara erisimi; sadece alici degil gonderici de sifre cozebilmeli.
6. **Backref mekanizmasi:** ChaCha20Poly1305 + nullifier nonce — gizli harcama gecmisi izleme. Nova Shield audit mekanizmasi icin kullanilabilir.
7. **ICS-20 `is_source` mantigi:** Token kaynagini prefix'ten belirleme; Nova Shield IBC modulunde aynen uygulanmali.
8. **Timeout yuvarlamasi:** Privacy gerekliligi — nanosaniye cinsinden, 1 dakikaya yuvarlama.
9. **trial_decrypt REJECT korunmasi:** Option<Note> donusu, panic yok, Result propagation yok.
10. **Blok commitment butcesi:** Penumbra'nin missing TODO'su — Nova Shield'de blok basina maksimum note sayisi siniri koy.
11. **FMD sliding window:** Otomatik precision guncelleme — Nova Shield'in privacy parametreleri icin dinamik on-chain yonetim modeli.
12. **Blanket impl pattern:** `impl<T: StateRead + ?Sized> AssetRegistryRead for T {}` — Nova Shield state trait'lerinde standart ol.
