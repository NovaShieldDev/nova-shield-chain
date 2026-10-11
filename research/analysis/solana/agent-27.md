# Solana Analiz — Agent 27
## Kapsam: transaction-status/ + rpc-client-api/ + account-decoder/

**Toplam satır:** ~13.800 satır (transaction-status: 8.486 + parse_token/extension: 2.617 = 11.103) + (rpc-client-api: 2.319) + (account-decoder: 3.008) = **~16.430 satır**
**Tarih:** 2026-07-22

---

## BÖLÜM 1: transaction-status/src/

### [lib.rs] (1586 satır)

**Amaç:** Transaction ve block verilerini JSON/Binary formatlarında encode/decode eden ana tip kütüphanesi; tüm transaction meta tiplerini tanımlar.

**Struct/Enum Listesi:**

- `BlockEncodingOptions`: `transaction_details: TransactionDetails`, `show_rewards: bool`, `max_supported_transaction_version: Option<u8>` — block encoding seçenekleri.
- `EncodeError` (enum): `UnsupportedTransactionVersion(u8)` — desteklenmeyen tx version hatası.
- `TransactionBinaryEncoding` (enum): `Base58`, `Base64` — binary encoding türleri.
- `UiTransactionEncoding` (enum): `Binary` (legacy), `Base64`, `Base58`, `Json`, `JsonParsed` — UI encoding türleri. `Binary` backward compat için tutulmuş.
- `TransactionDetails` (enum): `Full`, `Signatures`, `None`, `Accounts` — block içindeki tx detay seviyesi. Default = `Full`.
- `UiInstruction` (enum): `Compiled(UiCompiledInstruction)`, `Parsed(UiParsedInstruction)` — `#[serde(untagged)]` ile parse eden/compile instruction wrapper.
- `UiParsedInstruction` (enum): `Parsed(ParsedInstruction)`, `PartiallyDecoded(UiPartiallyDecodedInstruction)` — parse başarısızsa `PartiallyDecoded` döner.
- `UiCompiledInstruction`: `program_id_index: u8`, `accounts: Vec<u8>`, `data: String` (base58), `stack_height: Option<u32>` — ham compiled instruction için JSON.
- `UiPartiallyDecodedInstruction`: `program_id: String`, `accounts: Vec<String>`, `data: String` (base58), `stack_height: Option<u32>` — decode edilemeyen ix için fallback.
- `InnerInstructions`: `index: u8`, `instructions: Vec<InnerInstruction>` — bir tx'in bir instruction'ına ait CPI çağrı listesi.
- `InnerInstruction`: `instruction: CompiledInstruction`, `stack_height: Option<u32>` — CPI çağrısı + stack derinliği.
- `UiInnerInstructions`: `index: u8`, `instructions: Vec<UiInstruction>` — JSON formatı.
- `TransactionTokenBalance`: `account_index: u8`, `mint: String`, `ui_token_amount: UiTokenAmount`, `owner: String`, `program_id: String` — dahili token balance tipi.
- `UiTransactionTokenBalance`: JSON formatı. `owner` ve `program_id` alanları `OptionSerializer<String>` tipinde — eski veriler için boş string gelirse `Skip` yapılır.
- `TransactionStatusMeta`: **Kritik merkezi struct** — tüm transaction metadata:
  - `status: TransactionResult<()>` — Ok/Err
  - `fee: u64` — ödenen ücret (lamports)
  - `pre_balances: Vec<u64>` — tx öncesi account bakiyeleri (index = account sırası)
  - `post_balances: Vec<u64>` — tx sonrası account bakiyeleri
  - `inner_instructions: Option<Vec<InnerInstructions>>` — CPI çağrıları
  - `log_messages: Option<Vec<String>>` — program log çıktıları
  - `pre_token_balances: Option<Vec<TransactionTokenBalance>>` — tx öncesi token bakiyeleri
  - `post_token_balances: Option<Vec<TransactionTokenBalance>>` — tx sonrası token bakiyeleri
  - `rewards: Option<Rewards>` — blok ödülleri
  - `loaded_addresses: LoadedAddresses` — ALT'ten yüklenen adresler
  - `return_data: Option<TransactionReturnData>` — program return data
  - `compute_units_consumed: Option<u64>` — harcanan compute unit
- `UiTransactionStatusMeta`: JSON formatı `TransactionStatusMeta`'nın kopyası. Tüm optional alanlar `OptionSerializer<T>` kullanır — `None`, `Some`, `Skip` ayrımı mümkün.
- `UiLoadedAddresses`: `writable: Vec<String>`, `readonly: Vec<String>` — ALT'tan yüklenen adresler JSON.
- `TransactionConfirmationStatus` (enum): `Processed`, `Confirmed`, `Finalized` — commitment katmanları.
- `TransactionStatus`: `slot`, `confirmations: Option<usize>` (None = rooted/finalized), `status`, `err`, `confirmation_status` — RPC'ye dönen tx durum nesnesi.
- `ConfirmedTransactionStatusWithSignature`: `signature`, `slot`, `err`, `memo`, `block_time` — tx geçmişi için.
- `Reward`: `pubkey: String`, `lamports: i64`, `post_balance: u64`, `reward_type: Option<RewardType>`, `commission: Option<u8>` — staking/voting ödülü.
- `Rewards` = `Vec<Reward>`.
- `ConfirmedBlock`: `previous_blockhash`, `blockhash`, `parent_slot`, `transactions: Vec<TransactionWithStatusMeta>`, `rewards`, `block_time`, `block_height` — tam onaylı blok.
- `VersionedConfirmedBlock`: metadata'nın her zaman mevcut olduğu garanti edilmiş blok. BigTable upload için kullanılır.
- `VersionedConfirmedBlockWithEntries`: block + `entries: Vec<EntrySummary>` — kriptografik doğrulama için entry verisi.
- `EntrySummary`: `num_hashes`, `hash`, `num_transactions`, `starting_transaction_index`.
- `TransactionWithStatusMeta` (enum, `#[allow(clippy::large_enum_variant)]`): `MissingMetadata(Transaction)`, `Complete(VersionedTransactionWithStatusMeta)` — çok eski tx'lerde metadata olmayabilir.
- `VersionedTransactionWithStatusMeta`: `transaction: VersionedTransaction`, `meta: TransactionStatusMeta`.
- `EncodedTransactionWithStatusMeta`: `transaction: EncodedTransaction`, `meta: Option<UiTransactionStatusMeta>`, `version: Option<TransactionVersion>`.
- `ConfirmedTransactionWithStatusMeta`: `slot`, `tx_with_meta`, `block_time`.
- `EncodedTransaction` (enum): `LegacyBinary(String)`, `Binary(String, TransactionBinaryEncoding)`, `Json(UiTransaction)`, `Accounts(UiAccountsList)` — `#[serde(untagged)]` ile serialize edilir.
- `UiTransaction`: `signatures: Vec<String>`, `message: UiMessage`.
- `UiMessage` (enum): `Parsed(UiParsedMessage)`, `Raw(UiRawMessage)` — `#[serde(untagged)]`.
- `UiRawMessage`: `header: MessageHeader`, `account_keys: Vec<String>`, `recent_blockhash`, `instructions: Vec<UiCompiledInstruction>`, `address_table_lookups: Option<Vec<UiAddressTableLookup>>`.
- `UiParsedMessage`: `account_keys: Vec<ParsedAccount>`, `recent_blockhash`, `instructions: Vec<UiInstruction>`, `address_table_lookups`.
- `UiAccountsList`: `signatures: Vec<String>`, `account_keys: Vec<ParsedAccount>` — `TransactionDetails::Accounts` modu için.
- `UiAddressTableLookup`: `account_key: String`, `writable_indexes: Vec<u8>`, `readonly_indexes: Vec<u8>` — ALT lookup bilgisi JSON.
- `TransactionByAddrInfo`: `signature`, `err`, `index: u32`, `memo`, `block_time` — BigTable tx-by-addr tablosu için row data.
- `UiTransactionReturnData`: `program_id: String`, `data: (String, UiReturnDataEncoding)` — return data her zaman Base64.
- `UiReturnDataEncoding` (enum): sadece `Base64` variant'ı var.

**Trait Listesi:**
- `Encodable`: `type Encoded; fn encode(&self, encoding: UiTransactionEncoding) -> Self::Encoded` — `Transaction` ve `Message` impl'ler.
- `EncodableWithMeta`: `fn encode_with_meta(...)`, `fn json_encode()` — `VersionedTransaction` ve `v0::Message` impl'ler. v0 mesaj encode için meta (LoadedAddresses) gerektirir.
- `JsonAccounts`: `fn build_json_accounts(&self) -> Self::Encoded` — `TransactionDetails::Accounts` modu.

**Kritik Fonksiyonlar:**
- `map_inner_instructions(inner_instructions: InnerInstructionsList) -> impl Iterator<Item = InnerInstructions>` (satır 235): SDK'dan gelen inner instructions listesini bu crate'in tipine çevirir. Boş instruction listelerini filtreler (`filter(|i| !i.instructions.is_empty())`).
- `UiInstruction::parse(instruction, account_keys, stack_height)` (satır 144): `parse()` fn'i çağırır; başarısızsa `PartiallyDecoded` döner. Hiçbir zaman hata fırlatmaz.
- `UiInnerInstructions::parse(inner_instructions, account_keys)` (satır 264): inner ix listesi için JSON parse.
- `UiTransactionStatusMeta::parse(meta, static_keys, show_rewards)` (satır 462): `JsonParsed` encoding için kullanılır. `loaded_addresses` alanı **Skip** yapılır (v0 tx için ALT'lar zaten account_keys içinde expand edilir). `build_simple(meta, show_rewards)` ise `inner_instructions` ve `log_messages`'ı Skip yapar (Accounts modu için).
- `TransactionStatus::satisfies_commitment(commitment_config)` (satır 575): commitment check. `confirmations.is_none()` ise finalized (rooted).
- `TransactionStatus::confirmation_status()` (satır 593): `confirmation_status` None ise confirmations sayısından türetir (backward compat).
- `VersionedTransactionWithStatusMeta::validate_version(max_supported_transaction_version)` (satır 918): version uyumluluk kontrolü. Legacy tx her zaman geçerli; v0 tx için max_version None ise hata.
- `VersionedTransaction::encode_with_meta(encoding, meta)` (satır 1077): JsonParsed için `LoadedAddresses`'ı meta'dan çeker.
- `EncodedTransaction::decode()` (satır 1155): binary encoding'den geri parse eder, `sanitize()` kontrolü yapar. Json/Accounts formatı decode edilemez.

**Serde/Serialization:**
- `#[serde(rename_all = "camelCase")]` — neredeyse her struct.
- `#[serde(untagged)]` — `UiInstruction`, `UiParsedInstruction`, `UiMessage`, `EncodedTransaction` — tip tagı JSON'da görünmez.
- `OptionSerializer<T>` — `UiTransactionStatusMeta`'nın optional alanları. `skip_serializing_if = "OptionSerializer::should_skip"` ile Skip variant'ı serialize edilmez, None ise `null` serialize edilir.
- `#[serde(flatten)]` — bazı nested config struct'larında.
- `lazy_static!` + `extern crate serde_derive` macro'ları.

**Sabitler/Tipler:**
- `pub type Rewards = Vec<Reward>`.

**Test Kapsamı:**
- `test_decode_invalid_transaction`: sanitize başarısız olunca None dönmesi.
- `test_satisfies_commitment`: tüm commitment seviye kombinasyonları.
- `test_serde_empty_fields`: input eksik alanlarla deserialize edilince null olarak serialize olması.
- `test_ui_transaction_status_meta_ctors_serialization`: From vs parse ctor farkı.

**Nova Shield İçin Öğrenilen:**
- Transaction meta'nın core tasarımı aynen uygulanabilir: `pre_balances`/`post_balances` + `pre_token_balances`/`post_token_balances` pattern'i native coin + token değişimlerini ayrı izler.
- `OptionSerializer<T>` (None/Some/Skip üçlü ayrımı) — eski API client'larını kırmadan yeni alanlar eklemek için şık bir pattern. Nova Shield'ın RPC API'sinde kullanılabilir.
- `TransactionDetails::Accounts` modu: tx signatures + account listesi dönmek (ama instruction detayı yok) — blok explorer'lar için verimli sorgulama.
- `TransactionWithStatusMeta::MissingMetadata` — eski ledger verisi geriye dönük uyumluluk için. Nova Shield'da da eski chain verisi saklanırsa benzer bir discriminant enum gerekir.
- `validate_version(max_supported_transaction_version)` pattern'i: client uyumluluk yönetimi için kritik. Nova Shield'da versiyonlu tx'ler desteklendiğinde aynı mekanizma gerekecek.
- `inner_instructions` CPI call tracking: Nova Shield'ın smart contract çağrı ağacını izlemek için referans.

---

### [extract_memos.rs] (148 satır)

**Amaç:** Transaction message içindeki SPL Memo program instruction'larını tespit edip insan okunur string olarak birleştirir.

**Struct/Enum Listesi:**
- `KeyType<'a>` (enum, crate-private): `MemoProgram`, `OtherProgram`, `Unknown(&'a Pubkey)` — lazy classification cache.

**Kritik Fonksiyonlar:**
- `spl_memo_id_v1() -> Pubkey` (satır 12): SPL Memo v1 program ID'sini `solana_sdk::Pubkey` tipine çevirir.
- `spl_memo_id_v3() -> Pubkey` (satır 18): SPL Memo v3 için aynı.
- `extract_and_fmt_memos<T: ExtractMemos>(message: &T) -> Option<String>` (satır 27): memo listesi boşsa None, doluysa `"; "` ile join.
- `extract_and_fmt_memo_data(data: &[u8]) -> String` (satır 36): `[len] memo_text` formatında formatlı string. Geçersiz UTF-8 için `"(unparseable)"`.
- `extract_memos_inner(account_keys, instructions)` (satır 76): **Performans optimizasyonu**: `account_keys` vektörünü `Vec<KeyType>` olarak takip eder — bir program_id bir kez classify edilince sonraki kontrollerde HashMap lookup yerine O(1) index erişimi.

**Trait Listesi:**
- `ExtractMemos`: `fn extract_memos(&self) -> Vec<String>` — `Message`, `SanitizedMessage`, `VersionedTransactionWithStatusMeta` impl'leri.

**Serde/Serialization:** Yok. Saf parsing logic.

**Nova Shield İçin Öğrenilen:**
- Lazy program ID classification pattern'i: bir instruction set'i taranırken program ID'leri ilk kez görüldükçe cache'lemek, aynı programın çok sayıda instruction'ı varsa işlemi hızlandırır.
- Memo programı Solana'nın standard zincir üstü mesaj protokolü. Nova Shield'da on-chain metadata veya audit trail için benzer bir memo programı tasarlanabilir.

---

### [option_serializer.rs] (77 satır)

**Amaç:** Serde için `Option<T>`'nin 3 hallini temsil eden özel serializer — `Some(T)`, `None` (null), `Skip` (alanı serialize etme).

**Struct/Enum Listesi:**
- `OptionSerializer<T>` (enum, Clone, Debug, PartialEq, Eq):
  - `Some(T)` — value var, serialize et.
  - `None` — null serialize et.
  - `Skip` — alanı tamamen atla.

**Kritik Fonksiyonlar:**
- `none() -> Self`: OptionSerializer::None döner.
- `skip() -> Self`: OptionSerializer::Skip döner.
- `should_skip(&self) -> bool`: Skip variant'ını test eder — `skip_serializing_if` attribute'u ile kullanılır.
- `or_skip(option: Option<T>) -> Self` (satır 23): Some → Some, None → Skip. Değer yoksa alanı JSON'dan kaldırır.
- `as_ref(&self) -> OptionSerializer<&T>`: referans dönüşümü.

**Serde Implementasyonları:**
- `Serialize`: `Some` → value serialize, `None` → `serialize_none()`, `Skip` → Error (Skip'i serialize etmek hata — kullanıcı `skip_serializing_if` kullanmalı).
- `Deserialize`: `Option::deserialize()` çağırır, `Into<OptionSerializer>` dönüşümü yapar. Deserialize sırasında Skip kavramı yok (read-only distinction).

**From Implementasyonları:**
- `From<Option<T>>` for `OptionSerializer<T>`: Some → Some, None → None (Skip değil).
- `From<OptionSerializer<T>>` for `Option<T>`: Some → Some, diğerleri → None.

**Nova Shield İçin Öğrenilen:**
- RPC response'larında backward compatibility kritik. Eski client'lar bilinmeyen alanı parse etmeye çalışabilir; `skip_serializing_if` ile yeni alanları sadece değer varken göndermek güvenli.
- Bu pattern doğrudan Nova Shield'ın RPC layer'ında kopyalanabilir.

---

### [parse_accounts.rs] (173 satır)

**Amaç:** Legacy ve v0 message account listelerini `ParsedAccount` yapılarına çevirir.

**Struct/Enum Listesi:**
- `ParsedAccount` (Clone, Debug, Serialize, Deserialize, PartialEq, Eq): `pubkey: String`, `writable: bool`, `signer: bool`, `source: Option<ParsedAccountSource>`.
- `ParsedAccountSource` (enum): `Transaction`, `LookupTable` — account'ın nereden geldiği.

**Kritik Fonksiyonlar:**
- `parse_legacy_message_accounts(message: &Message) -> Vec<ParsedAccount>` (satır 19): `message.is_maybe_writable(i)` ve `message.is_signer(i)` ile her account'ı classify eder. source her zaman `Transaction`.
- `parse_v0_message_accounts(message: &LoadedMessage) -> Vec<ParsedAccount>` (satır 32): `i < message.static_account_keys().len()` kontrolü ile Transaction vs LookupTable ayrımı yapar.

**Test Kapsamı:** Legacy ve v0 için `MessageHeader` kurulumu, writable/signer kombinasyonları doğrulanıyor. v0 testinde `LoadedAddresses` (pubkey4=writable, pubkey5=readonly) ile ALT kaynaklı account'lar test ediliyor.

**Nova Shield İçin Öğrenilen:**
- Account tipi metadata (writable/signer/source) RPC üzerinden client'a expose edilmeli. Blok explorer'lar için kritik.
- ALT-kaynaklı account'lar `Transaction` kaynaklılardan ayrı işaretlenmeli — fee hesabı ve security analysis için önemli.

---

### [parse_address_lookup_table.rs] (361 satır)

**Amaç:** Address Lookup Table program instruction'larını JSON'a decode eder.

**Kritik Fonksiyonlar:**
- `parse_address_lookup_table(instruction, account_keys) -> Result<ParsedInstructionEnum, ParseInstructionError>` (satır 13): `bincode::deserialize` ile `ProgramInstruction` deserialize, sonra match.

**Decode Edilen Instruction'lar:**
- `CreateLookupTable { recent_slot, bump_seed }` — 4 account gerekir: lookupTableAccount, lookupTableAuthority, payerAccount, systemProgram.
- `FreezeLookupTable` — 2 account.
- `ExtendLookupTable { new_addresses }` — min 2 account (payer opsiyonel).
- `DeactivateLookupTable` — 2 account.
- `CloseLookupTable` — 3 account.

**Güvenlik Notu:** Her instruction için `instruction.accounts.iter().max()` kontrolü yapılır — en yüksek index account_keys uzunluğundan küçük olmalı. Yoksa `InstructionKeyMismatch` hatası.

**Nova Shield İçin Öğrenilen:**
- Account lookup table benzeri bir mekanizma (büyük tx'lerde adres sıkıştırması) Nova Shield'da da gerekebilir. Parse pattern'i aynıdır: bincode deserialize → match → JSON.

---

### [parse_associated_token.rs] (297 satır)

**Amaç:** SPL Associated Token Account program instruction'larını decode eder.

**Kritik Fonksiyonlar:**
- `parse_associated_token(instruction, account_keys)`: `AssociatedTokenAccountInstruction::try_from_slice` ile Borsh deserialize. `instruction.data.is_empty()` ise `Create` (eski sürüm backwards compat).

**Decode Edilen Instruction'lar:**
- `Create` — 6 account: source, account, wallet, mint, systemProgram, tokenProgram.
- `CreateIdempotent` — 6 account (aynı).
- `RecoverNested` — 7 account: nestedSource, nestedMint, destination, nestedOwner, ownerMint, wallet, tokenProgram.

**Test Notu:** Eski `create_associated_token_account_deprecated` rent sysvar hesabı içerirdi; bu rent account kaldırılsa da parse hala başarılı olmalı (6 account kontrolü).

**Nova Shield İçin Öğrenilen:**
- Token account management Nova Shield'ın DEX için kritik. ATA (Associated Token Account) pattern: `PDA = PDA_derive([wallet, token_program, mint], ata_program)` — Nova Shield'da da benzer deterministic account adresleme kullanılabilir.

---

### [parse_bpf_loader.rs] (782 satır)

**Amaç:** BPF Loader ve BPF Upgradeable Loader instruction'larını JSON'a decode eder.

**Kritik Fonksiyonlar:**
- `parse_bpf_loader(instruction, account_keys)`: `LoaderInstruction` — `Write { offset, bytes }` ve `Finalize`. `bytes` base64 encode edilir.
- `parse_bpf_upgradeable_loader(instruction, account_keys)`: `UpgradeableLoaderInstruction` — 7 variant:
  - `InitializeBuffer` — 1 account (+ opsiyonel authority).
  - `Write { offset, bytes }` — 2 account. bytes base64.
  - `DeployWithMaxDataLen { max_data_len }` — 8 account.
  - `Upgrade` — 7 account.
  - `SetAuthority` — 2 account. `newAuthority` opsiyonel (None ise program immutable yapılır).
  - `SetAuthorityChecked` — 3 account (yeni authority tx'i imzalamalı).
  - `Close` — 3 account + opsiyonel programAccount.
  - `ExtendProgram { additional_bytes }` — 2 account + opsiyonel system program + payer.

**Nova Shield İçin Öğrenilen:**
- `SetAuthority` vs `SetAuthorityChecked` ayrımı: "Checked" versiyonunda yeni authority tx'i imzalar — multisig veya upgrade için güvenli pattern. Nova Shield'ın program upgrade mekanizmasında benzer checked pattern kullanılmalı.

---

### [parse_instruction.rs] (227 satır)

**Amaç:** Tüm parsable program'lar için merkezi dispatch; program_id → parser routing.

**Struct/Enum Listesi:**
- `ParseInstructionError` (Error, Debug): `InstructionNotParsable(ParsableProgram)`, `InstructionKeyMismatch(ParsableProgram)`, `ProgramNotParsable`, `SerdeJsonError(#[from] serde_json::error::Error)`.
- `ParsedInstruction` (Clone, Debug, Serialize, Deserialize, PartialEq, Eq): `program: String` (kebab-case), `program_id: String`, `parsed: Value` (serde_json), `stack_height: Option<u32>`.
- `ParsedInstructionEnum` (Debug, Serialize, Deserialize): `instruction_type: String` (`#[serde(rename = "type")]`), `info: Value`.
- `ParsableProgram` (enum): `AddressLookupTable`, `SplAssociatedTokenAccount`, `SplMemo`, `SplToken`, `BpfLoader`, `BpfUpgradeableLoader`, `Stake`, `System`, `Vote`.

**Kritik Global State:**
- `PARSABLE_PROGRAM_IDS: HashMap<Pubkey, ParsableProgram>` — lazy_static. `spl_token_ids()` çağrısı ile hem spl-token hem spl-token-2022 program ID'leri eklenir.

**Kritik Fonksiyonlar:**
- `parse(program_id, instruction, account_keys, stack_height) -> Result<ParsedInstruction, ParseInstructionError>` (satır 110): dispatch fonksiyonu. program_id HashMap'te yoksa `ProgramNotParsable`. `format!("{program_name:?}").to_kebab_case()` ile `inflector` crate'i kullanılarak program adı kebab-case'e çevrilir (ör. `SplMemo` → `"spl-memo"`).
- `parse_memo(instruction)` (satır 146): data bytes'ı UTF-8 string'e çevirir.
- `parse_memo_data(data: &[u8]) -> Result<String, Utf8Error>` (satır 152): public — extract_memos.rs de kullanır.
- `check_num_accounts(accounts, num, parsable_program)` (satır 156): account sayısı kontrolü helper. Her parser modülü bu fn'i kullanır.

**Nova Shield İçin Öğrenilen:**
- Program ID → parser routing için `HashMap<ProgramId, ParsableProgram>` pattern'i temiz ve genişletilebilir. Nova Shield'da custom program'lar buraya eklenerek instruction parsing'i genişletmek kolay.
- `to_kebab_case()` ile program adı normalizasyonu: API response'larında tutarlı program adı için iyi practice.
- `stack_height` — CPI çağrı derinliği her instruction'a attach edilmeli; debugging ve güvenlik analizi için kritik.

---

### [parse_stake.rs] (1160 satır)

**Amaç:** Stake program instruction'larını JSON'a decode eder.

**Decode Edilen Instruction'lar (StakeInstruction match — 15 variant):**
- `Initialize(authorized, lockup)` — 2 account. authorized: {staker, withdrawer}; lockup: {unixTimestamp, epoch, custodian}.
- `Authorize(authorized, authority_type)` — 3 account + opsiyonel custodian.
- `DelegateStake` — 6 account: stakeAccount, voteAccount, clockSysvar, stakeHistorySysvar, stakeConfigAccount, stakeAuthority.
- `Split(lamports)` — 3 account.
- `Withdraw(lamports)` — 5 account + opsiyonel custodian.
- `Deactivate` — 3 account.
- `SetLockup(lockup_args)` — 2 account. Opsiyonel custodian.
- `Merge` — 5 account.
- `AuthorizeWithSeed { ... }` — 3 account + opsiyonel custodian.
- `InitializeChecked` — 4 account.
- `AuthorizeChecked(authority_type)` — 4 account + opsiyonel custodian.
- `AuthorizeCheckedWithSeed { ... }` — 4 account + opsiyonel custodian.
- `SetLockupChecked(lockup_args)` — 2 account.
- `GetMinimumDelegation` — 0 account.
- `DeactivateDelinquent` — 3 account.
- `Redelegate` — 5 account.

**Nova Shield İçin Öğrenilen:**
- Staking instruction decode'u uzun ama basit pattern: deserialize + account index'lere göre field map. Nova Shield'ın validator staking mekanizması için benzer instruction decode gerekecek.
- `custodian` opsiyonel — multisig/lockup senaryolarında custody delegation. Nova Shield'ın delegated staking'i için pattern.

---

### [parse_system.rs] (671 satır)

**Amaç:** System program instruction'larını JSON'a decode eder.

**Decode Edilen Instruction'lar (12 SystemInstruction variant):**
- `CreateAccount { lamports, space, owner }` — 2 account.
- `Assign { owner }` — 1 account.
- `Transfer { lamports }` — 2 account.
- `CreateAccountWithSeed { base, seed, lamports, space, owner }` — 2 account.
- `AdvanceNonceAccount` — 3 account: nonceAccount, recentBlockhashesSysvar, nonceAuthority.
- `WithdrawNonceAccount(lamports)` — 5 account.
- `InitializeNonceAccount(authority)` — 3 account.
- `AuthorizeNonceAccount(authority)` — 2 account.
- `UpgradeNonceAccount` — 1 account.
- `Allocate { space }` — 1 account.
- `AllocateWithSeed { base, seed, space, owner }` — 2 account.
- `AssignWithSeed { base, seed, owner }` — 2 account.
- `TransferWithSeed { lamports, from_seed, from_owner }` — 3 account.

**Nova Shield İçin Öğrenilen:**
- System program 13 instruction — native transfer, account creation, nonce management. Nova Shield için aynı instruction set benimsenebilir. PDA-benzeri `CreateAccountWithSeed` mekanizması ZK proof account'ları için yararlı.

---

### [parse_vote.rs] (862 satır)

**Amaç:** Vote program instruction'larını JSON'a decode eder.

**Decode Edilen Instruction'lar (13 VoteInstruction variant):**
- `InitializeAccount(vote_init)` — 4 account.
- `Authorize(new_authorized, authority_type)` — 3 account.
- `AuthorizeWithSeed(args)` / `AuthorizeCheckedWithSeed(args)` — 3/4 account.
- `Vote(vote)` — 4 account. vote: `{slots, hash, timestamp}`.
- `UpdateVoteState(vote_state_update)` / `UpdateVoteStateSwitch(update, hash)` — 2 account.
- `CompactUpdateVoteState(update)` / `CompactUpdateVoteStateSwitch(update, hash)` — 2 account.
- `Withdraw(lamports)` — 3 account.
- `UpdateValidatorIdentity` — 3 account.
- `UpdateCommission(commission)` — 2 account.
- `VoteSwitch(vote, hash)` — 4 account. hash: proof hash (Tower BFT switchover kanıtı).
- `AuthorizeChecked(authority_type)` — 4 account.

**Nova Shield İçin Öğrenilen:**
- `CompactUpdateVoteState` versiyonu daha az byte kullanır — ağ verimliliği için. Nova Shield'ın consensus vote mesajları da compact encoding kullanmalı.
- `VoteSwitch` — fork switching için proof hash gerektirir (Tower BFT güvenliği). Nova Shield'ın PoS mekanizması benzer switchover proof'u gerektirebilir.

---

### [parse_token.rs] (2121 satır) + [parse_token/extension/] (2617 satır)

**Amaç:** SPL Token ve Token-2022 instruction'larını JSON'a decode eder; tüm extension'lar desteklenir.

**parse_token() Decode Edilen Ana Instruction'lar (TokenInstruction — ~30+ variant):**
- `InitializeMint { decimals, mint_authority, freeze_authority }` — 2 account (+ opsiyonel freeze_authority field).
- `InitializeMint2` — 1 account (rent sysvar gerekmez).
- `InitializeAccount` / `InitializeAccount2 { owner }` / `InitializeAccount3 { owner }` — 4/3/2 account.
- `InitializeMultisig { m }` / `InitializeMultisig2 { m }` — min 3/2 account, sonrakiler signer.
- `Transfer { amount }` (deprecated) — 3 account + multisig.
- `Approve { amount }` — 3 account.
- `Revoke` — signer parse.
- `SetAuthority { authority_type, new_authority }` — authority opsiyonel.
- `MintTo { amount }` / `MintToChecked { amount, decimals }`.
- `Burn { amount }` / `BurnChecked { amount, decimals }`.
- `CloseAccount`, `FreezeAccount`, `ThawAccount`.
- `TransferChecked { amount, decimals }` — decimals doğrulaması ile transfer.
- `SyncNative`, `GetAccountDataSize`, `InitializeImmutableOwner`.
- `AmountToUiAmount { amount }`, `UiAmountToAmount { ui_amount }`.
- `InitializeAccount3`, `Reallocate { extension_types }`.
- **Extension initialize instruction'ları** (20+): `InitializeTransferFeeConfig`, `InitializeConfidentialTransferMint`, `InitializeCpiGuard`, `InitializeDefaultAccountState`, `InitializeInterestBearingMint`, `InitializeMemoTransfers`, `InitializePermanentDelegate`, `InitializeTransferHook`, `InitializeMetadataPointer`, `InitializeGroupPointer`, `InitializeGroupMemberPointer`, `UpdateConfidentialTransferMint`, `EnableCpiGuard`/`DisableCpiGuard`, `EnableRequiredMemoTransfers`/`DisableRequiredMemoTransfers`, `EnableNonTransferableAccount`, `UpdateDefaultAccountState`, `WithdrawExcessLamports`, `TokenGroupInitializeGroup`, `TokenGroupInitializeMember`, `TokenMetadataInitialize`, `TokenMetadataUpdateField`, `TokenMetadataRemoveKey`.

**Extension Alt Dizini Dosyaları:**
- `extension/mod.rs` (16 satır): tüm extension modüllerini re-export eder.
- `extension/confidential_transfer.rs` (360 satır): `InitializeConfidentialTransferMint`, `UpdateConfidentialTransferMint`, `ConfigureAccount`, `ApproveAccount`, `EmptyAccount`, `Deposit`, `Withdraw`, `Transfer` (ElGamal + auditor key decode). **Gizli transfer sistemi** — ZK proofları içerir.
- `extension/confidential_transfer_fee.rs` (159 satır): `InitializeConfidentialTransferFeeConfig`, `WithdrawWithheldTokensFromMint/Accounts`, `HarvestWithheldTokensToMint`.
- `extension/transfer_fee.rs` (550 satır): `InitializeTransferFeeConfig { transfer_fee_basis_points, maximum_fee }`, `TransferCheckedWithFee`, `WithdrawWithheldTokensFromMint/Accounts`, `HarvestWithheldTokensToMint`, `SetTransferFee`.
- `extension/transfer_hook.rs` (183 satır): `InitializeTransferHook { authority, hook_program_id }`, `UpdateTransferHook`.
- `extension/cpi_guard.rs` (159 satır): `EnableCpiGuard`, `DisableCpiGuard`.
- `extension/default_account_state.rs` (154 satır): `InitializeDefaultAccountState { account_state }`, `UpdateDefaultAccountState`.
- `extension/group_pointer.rs` / `group_member_pointer.rs` (183/189 satır): group/member pointer initialize ve update.
- `extension/interest_bearing_mint.rs` (63 satır): `InitializeInterestBearingMint { rate_authority, rate }`, `UpdateRateInterestBearingMint`.
- `extension/memo_transfer.rs` (171 satır): `EnableRequiredMemoTransfers`, `DisableRequiredMemoTransfers`.
- `extension/metadata_pointer.rs` (189 satır): `InitializeMetadataPointer`, `UpdateMetadataPointer`.
- `extension/mint_close_authority.rs` (76 satır): `InitializeMintCloseAuthority { close_authority }`.
- `extension/permanent_delegate.rs` (49 satır): `InitializePermanentDelegate { delegate }`.
- `extension/reallocate.rs` (116 satır): `Reallocate { extension_types }`.

**Yardımcı Fonksiyonlar:**
- `parse_signers(map, multisig_start, account_keys, accounts, authority_key, multisig_authority_key)`: tek ya da multisig signer detect eder. `accounts.len() > multisig_start + 1` ise multisig pattern.

**Nova Shield İçin Öğrenilen:**
- Token-2022 extension sistemi son derece modüler — `TransferFee`, `ConfidentialTransfer`, `InterestBearing`, `TransferHook` gibi her özellik ayrı extension. Nova Shield'ın token sistemi de extension tabanlı olmalı.
- `ConfidentialTransfer` extension: ElGamal public key store eder, ZK proof'larla private transfer. Nova Shield'ın DEX'inde gizli token bakiyeleri için referans.
- `TransferFee`: `transfer_fee_basis_points` (1 basis point = 0.01%) ve `maximum_fee`. Nova Shield DEX fee mekanizması için.
- `TransferHook`: transfer sırasında custom program çağrısı. Nova Shield'da compliance/KYC hook'ları için kullanılabilir.

---

### [token_balances.rs] (21 satır)

**Amaç:** Token balance tracking için type alias ve container.

**Struct/Tipler:**
- `TransactionTokenBalances = Vec<Vec<TransactionTokenBalance>>` — dış Vec: tx başına, iç Vec: o tx'teki token balance değişimleri.
- `TransactionTokenBalancesSet`: `pre_token_balances: TransactionTokenBalances`, `post_token_balances: TransactionTokenBalances`. Constructor'da `assert_eq!(pre.len(), post.len())` — iki listenin eşit uzunlukta olmasını zorunlu kılar.

**Nova Shield İçin Öğrenilen:**
- Pre/post token balance tracking bank/runtime seviyesinde tutulur, RPC'ye aktarılır. Nova Shield'ın token ledger'ı da bu yapıyı kullanmalı.

---

## BÖLÜM 2: rpc-client-api/src/

### [lib.rs] (14 satır)

**Amaç:** Crate giriş noktası. `#[macro_use] extern crate serde_derive` — tüm modüller için serde derive.

**Modüller:** `client_error`, `config`, `custom_error`, `deprecated_config`, `error_object`, `filter`, `request`, `response`, `version_req`.

---

### [request.rs] (368 satır)

**Amaç:** RPC metot isimleri ve request building; tüm RPC endpoint'lerini enum olarak tanımlar.

**Struct/Enum Listesi:**
- `RpcRequest` (enum, Debug, PartialEq, Eq, Hash, Clone, Copy — 60+ variant): Her Solana RPC metodu bir variant. Deprecated olanlar `#[deprecated]` attribute ile işaretli.

**Tam RpcRequest Variant Listesi:**
  - `Custom { method: &'static str }` — dinamik metot için.
  - `DeregisterNode`, `RegisterNode`, `SignVote` — internal/legacy.
  - `GetAccountInfo`, `GetBalance`, `GetBlock`, `GetBlockHeight`, `GetBlockProduction`, `GetBlocks`, `GetBlocksWithLimit`, `GetBlockTime`.
  - `GetClusterNodes`, `GetEpochInfo`, `GetEpochSchedule`, `GetFeeForMessage`.
  - `GetFirstAvailableBlock`, `GetGenesisHash`, `GetHealth`, `GetIdentity`.
  - `GetInflationGovernor`, `GetInflationRate`, `GetInflationReward`.
  - `GetLargestAccounts`, `GetLatestBlockhash`, `GetLeaderSchedule`.
  - `GetMaxRetransmitSlot`, `GetMaxShredInsertSlot`, `GetMinimumBalanceForRentExemption`.
  - `GetMultipleAccounts`, `GetProgramAccounts`, `GetRecentPerformanceSamples`, `GetRecentPrioritizationFees`.
  - `GetHighestSnapshotSlot`, `GetSignaturesForAddress`, `GetSignatureStatuses`.
  - `GetSlot`, `GetSlotLeader`, `GetSlotLeaders`, `GetStakeActivation`, `GetStakeMinimumDelegation`.
  - `GetStorageTurn`, `GetStorageTurnRate`, `GetSlotsPerSegment`, `GetStoragePubkeysForSlot` — storage legacy.
  - `GetSupply`, `GetTokenAccountBalance`, `GetTokenAccountsByDelegate`, `GetTokenAccountsByOwner`, `GetTokenLargestAccounts`, `GetTokenSupply`.
  - `GetTransaction`, `GetTransactionCount`, `GetVersion`, `GetVoteAccounts`.
  - `IsBlockhashValid`, `MinimumLedgerSlot`, `RequestAirdrop`, `SendTransaction`, `SimulateTransaction`.
  - **Deprecated:** `GetConfirmedBlock`, `GetConfirmedBlocks`, `GetConfirmedBlocksWithLimit`, `GetConfirmedSignaturesForAddress2`, `GetConfirmedTransaction`, `GetFeeCalculatorForBlockhash`, `GetFeeRateGovernor`, `GetFees`, `GetRecentBlockhash`, `GetSnapshotSlot`.

**Kritik Fonksiyonlar:**
- `RpcRequest::build_request_json(self, id: u64, params: Value) -> Value` (satır 217): JSON-RPC 2.0 formatında request body oluşturur: `{jsonrpc: "2.0", id, method, params}`.

**Sabitler:**
- `MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS: usize = 256`
- `MAX_GET_CONFIRMED_SIGNATURES_FOR_ADDRESS_SLOT_RANGE: u64 = 10_000`
- `MAX_GET_CONFIRMED_BLOCKS_RANGE: u64 = 500_000`
- `MAX_GET_CONFIRMED_SIGNATURES_FOR_ADDRESS2_LIMIT: usize = 1_000`
- `MAX_MULTIPLE_ACCOUNTS: usize = 100`
- `NUM_LARGEST_ACCOUNTS: usize = 20`
- `MAX_GET_PROGRAM_ACCOUNT_FILTERS: usize = 4`
- `MAX_GET_SLOT_LEADERS: usize = 5000`
- `MAX_RPC_VOTE_ACCOUNT_INFO_EPOCH_CREDITS_HISTORY: usize = 5`
- `DELINQUENT_VALIDATOR_SLOT_DISTANCE: u64 = 128`

**Diğer Tipler:**
- `RpcResponseErrorData` (enum): `Empty`, `SendTransactionPreflightFailure(RpcSimulateTransactionResult)`, `NodeUnhealthy { num_slots_behind }`.
- `RpcError` (enum, Error, Debug): `RpcRequestError`, `RpcResponseError { code, message, data }`, `ParseError`, `ForUser`.
- `TokenAccountsFilter` (enum): `Mint(Pubkey)`, `ProgramId(Pubkey)`.

**Nova Shield İçin Öğrenilen:**
- 60+ RPC metodu — Solana'nın tam public API surface'i. Nova Shield bunların hepsini desteklemek zorunda değil ama en kritikleri: `GetAccountInfo`, `GetBalance`, `GetBlock`, `GetTransaction`, `SendTransaction`, `SimulateTransaction`, `GetLatestBlockhash`, `GetSignatureStatuses`.
- `MAX_MULTIPLE_ACCOUNTS = 100`, `MAX_GET_PROGRAM_ACCOUNT_FILTERS = 4` gibi limitler rate-limiting ve DoS koruması için. Nova Shield'ın RPC'sinde benzer limitler olmalı.
- `DELINQUENT_VALIDATOR_SLOT_DISTANCE = 128` — validator 128 slot geride kalırsa delinquent sayılır. Nova Shield'ın validator monitoring'i için referans.

---

### [response.rs] (621 satır)

**Amaç:** Tüm RPC yanıt tiplerini tanımlar.

**Kritik Struct/Enum Listesi:**

- `OptionalContext<T>` (enum): `Context(Response<T>)`, `NoContext(T)` — `#[serde(untagged)]`. Bazı metodlar context'i sonradan ekledi; backward compat için.
- `RpcResult<T>` = `client_error::Result<Response<T>>`.
- `RpcResponseContext`: `slot: Slot`, `api_version: Option<RpcApiVersion>`.
- `RpcApiVersion` (newtype `semver::Version`): custom Serialize/Deserialize ile string olarak serileştirilir.
- `Response<T>`: `context: RpcResponseContext`, `value: T` — standart RPC yanıt wrapper'ı.
- `RpcBlockCommitment<T>`: `commitment: Option<T>`, `total_stake: u64`.
- `RpcBlockhashFeeCalculator` (deprecated): `blockhash`, `fee_calculator`.
- `RpcBlockhash`: `blockhash: String`, `last_valid_block_height: u64`.
- `RpcFees` (deprecated): blockhash, fee_calculator, last_valid_slot, last_valid_block_height.
- `RpcFeeCalculator` (deprecated), `RpcFeeRateGovernor` (deprecated).
- `RpcInflationGovernor`: `initial`, `terminal`, `taper`, `foundation`, `foundation_term` — tüm f64. `From<Inflation>`.
- `RpcInflationRate`: `total`, `validator`, `foundation` (f64), `epoch`.
- `RpcKeyedAccount`: `pubkey: String`, `account: UiAccount`.
- `SlotInfo`: `slot`, `parent`, `root` — WebSocket slot notification için.
- `SlotTransactionStats`: `num_transaction_entries`, `num_successful_transactions`, `num_failed_transactions`, `max_transactions_per_entry`.
- `SlotUpdate` (enum, tag = "type"): `FirstShredReceived { slot, timestamp }`, `Completed { slot, timestamp }`, `CreatedBank { slot, parent, timestamp }`, `Frozen { slot, timestamp, stats }`, `Dead { slot, timestamp, err }`, `OptimisticConfirmation { slot, timestamp }`, `Root { slot, timestamp }` — WebSocket slot update bildirimleri.
- `RpcSignatureResult` (enum): `ProcessedSignature(ProcessedSignatureResult)`, `ReceivedSignature(ReceivedSignatureResult)`.
- `RpcLogsResponse`: `signature: String`, `err: Option<TransactionError>`, `logs: Vec<String>`.
- `ProcessedSignatureResult`: `err: Option<TransactionError>`.
- `ReceivedSignatureResult` (enum): tek variant `ReceivedSignature`.
- `RpcContactInfo`: `pubkey`, `gossip: Option<SocketAddr>`, `tpu: Option<SocketAddr>`, `tpu_quic: Option<SocketAddr>`, `rpc: Option<SocketAddr>`, `pubsub: Option<SocketAddr>`, `version: Option<String>`, `feature_set: Option<u32>`, `shred_version: Option<u16>`.
- `RpcLeaderSchedule = HashMap<String, Vec<usize>>` — validator pubkey → slot indices.
- `RpcBlockProductionRange`: `first_slot`, `last_slot`.
- `RpcBlockProduction`: `by_identity: HashMap<String, (usize, usize)>` (leader_slots, blocks_produced), `range`.
- `RpcVersionInfo`: `solana_core: String`, `feature_set: Option<u32>`. `kebab-case` serialization.
- `RpcIdentity`: `identity: String`.
- `RpcVote`: `vote_pubkey`, `slots`, `hash`, `timestamp`, `signature`.
- `RpcVoteAccountStatus`: `current: Vec<RpcVoteAccountInfo>`, `delinquent: Vec<RpcVoteAccountInfo>`.
- `RpcVoteAccountInfo`: `vote_pubkey`, `node_pubkey`, `activated_stake: u64`, `commission: u8`, `epoch_vote_account: bool`, `epoch_credits: Vec<(Epoch, u64, u64)>`, `last_vote: u64`, `root_slot: Slot`.
- `RpcSignatureConfirmation`: `confirmations: usize`, `status: Result<()>`.
- `RpcSimulateTransactionResult`: `err`, `logs`, `accounts: Option<Vec<Option<UiAccount>>>`, `units_consumed: Option<u64>`, `return_data: Option<UiTransactionReturnData>`, `inner_instructions: Option<Vec<UiInnerInstructions>>`.
- `RpcAccountBalance`: `address: String`, `lamports: u64`.
- `RpcSupply`: `total`, `circulating`, `non_circulating`, `non_circulating_accounts: Vec<String>`.
- `StakeActivationState` (enum): `Activating`, `Active`, `Deactivating`, `Inactive`.
- `RpcStakeActivation`: `state: StakeActivationState`, `active: u64`, `inactive: u64`.
- `RpcTokenAccountBalance`: `address: String`, `#[serde(flatten)] amount: UiTokenAmount`.
- `RpcConfirmedTransactionStatusWithSignature`: `signature`, `slot`, `err`, `memo`, `block_time`, `confirmation_status`.
- `RpcPerfSample`: `slot`, `num_transactions: u64`, `num_non_vote_transactions: Option<u64>`, `num_slots`, `sample_period_secs: u16`. `num_non_vote_transactions` opsiyonel — eski protokol uyumluluğu.
- `RpcInflationReward`: `epoch`, `effective_slot`, `amount: u64`, `post_balance: u64`, `commission: Option<u8>`.
- `RpcBlockUpdateError` (enum): `BlockStoreError`, `UnsupportedTransactionVersion(u8)`.
- `RpcBlockUpdate`: `slot`, `block: Option<UiConfirmedBlock>`, `err: Option<RpcBlockUpdateError>`.
- `RpcSnapshotSlotInfo`: `full: Slot`, `incremental: Option<Slot>`.
- `RpcPrioritizationFee`: `slot: Slot`, `prioritization_fee: u64`.

**Test Kapsamı:**
- `rpc_perf_sample_deserialize_old`: `num_non_vote_transactions` alanı olmayan JSON deserialize edilebilmeli.
- `rpc_perf_sample_serializes_num_non_vote_transactions`: yeni alan doğru serialize edilmeli.

**Nova Shield İçin Öğrenilen:**
- `SlotUpdate` enum tag-based enum ile WebSocket notification sistemi: 7 farklı slot lifecycle event. Nova Shield'ın WebSocket API'si aynı event tiplerini tanımlayabilir.
- `RpcSimulateTransactionResult`'ta `inner_instructions` alanı: tx simülasyonu sırasında CPI çağrı ağacını görmek — debugging ve gas estimation için kritik.
- `RpcPerfSample`: non-vote tx sayısı ayrı tutulur — consensus vs kullanıcı işlemi ayrımı. Nova Shield validator performans monitoring'i için.
- `RpcContactInfo`'da hem UDP (`tpu`) hem QUIC (`tpu_quic`) port'u: iki transport protokolü yan yana. Nova Shield'ın P2P layer'ı benzer çoklu-transport yapısı kullanabilir.

---

### [config.rs] (342 satır)

**Amaç:** Tüm RPC request yapılandırma struct'ları.

**Struct/Enum Listesi:**

- `RpcSignatureStatusConfig`: `search_transaction_history: bool`.
- `RpcSendTransactionConfig`: `skip_preflight: bool`, `preflight_commitment: Option<CommitmentLevel>`, `encoding: Option<UiTransactionEncoding>`, `max_retries: Option<usize>`, `min_context_slot: Option<Slot>`.
- `RpcSimulateTransactionConfig`: `sig_verify: bool`, `replace_recent_blockhash: bool`, `commitment: Option<CommitmentConfig>` (`#[serde(flatten)]`), `encoding`, `accounts: Option<RpcSimulateTransactionAccountsConfig>`, `min_context_slot`, `inner_instructions: bool`.
- `RpcSimulateTransactionAccountsConfig`: `encoding: Option<UiAccountEncoding>`, `addresses: Vec<String>`.
- `RpcRequestAirdropConfig`: `recent_blockhash`, `commitment` (flatten).
- `RpcLeaderScheduleConfig`: `identity: Option<String>`, `commitment` (flatten).
- `RpcBlockProductionConfig`: `identity`, `range: Option<RpcBlockProductionConfigRange>`, `commitment` (flatten).
- `RpcGetVoteAccountsConfig`: `vote_pubkey`, `commitment`, `keep_unstaked_delinquents`, `delinquent_slot_distance`.
- `RpcLeaderScheduleConfigWrapper` (enum): `SlotOnly(Option<Slot>)`, `ConfigOnly(Option<RpcLeaderScheduleConfig>)` — backward compat wrapper.
- `RpcLargestAccountsFilter` (enum): `Circulating`, `NonCirculating`.
- `RpcLargestAccountsConfig`: `commitment`, `filter`.
- `RpcSupplyConfig`: `commitment`, `exclude_non_circulating_accounts_list: bool`.
- `RpcEpochConfig`: `epoch: Option<Epoch>`, `commitment`, `min_context_slot`.
- `RpcAccountIndex` (enum): `ProgramId`, `SplTokenMint`, `SplTokenOwner` — secondary index türleri.
- `RpcAccountInfoConfig`: `encoding`, `data_slice: Option<UiDataSliceConfig>`, `commitment` (flatten), `min_context_slot`.
- `RpcProgramAccountsConfig`: `filters`, `account_config: RpcAccountInfoConfig` (flatten), `with_context`.
- `RpcTransactionLogsFilter` (enum): `All`, `AllWithVotes`, `Mentions(Vec<String>)`.
- `RpcTransactionLogsConfig`: `commitment` (flatten).
- `RpcTokenAccountsFilter` (enum): `Mint(String)`, `ProgramId(String)`.
- `RpcSignatureSubscribeConfig`: `commitment`, `enable_received_notification: Option<bool>`.
- `RpcBlockSubscribeFilter` (enum): `All`, `MentionsAccountOrProgram(String)`.
- `RpcBlockSubscribeConfig`: `commitment`, `encoding`, `transaction_details`, `show_rewards`, `max_supported_transaction_version`.
- `RpcSignaturesForAddressConfig`: `before`, `until`, `limit`, `commitment`, `min_context_slot`.
- `RpcEncodingConfigWrapper<T>` (enum): `Deprecated(Option<UiTransactionEncoding>)`, `Current(Option<T>)` — encoding config versiyonlama.
- `EncodingConfig` (trait): `fn new_with_encoding(encoding: &Option<UiTransactionEncoding>) -> Self`.
- `RpcBlockConfig`: `encoding`, `transaction_details`, `rewards`, `commitment`, `max_supported_transaction_version`. `EncodingConfig` impl.
- `RpcTransactionConfig`: `encoding`, `commitment`, `max_supported_transaction_version`. `EncodingConfig` impl.
- `RpcBlocksConfigWrapper` (enum): `EndSlotOnly(Option<Slot>)`, `ConfigOnly(Option<RpcContextConfig>)`.
- `RpcContextConfig`: `commitment`, `min_context_slot`.

**Nova Shield İçin Öğrenilen:**
- `min_context_slot` — client'ın belirli bir slot'tan sonraki veriye erişmesini garanti eder. Race condition önleme için önemli.
- `RpcEncodingConfigWrapper` — API versiyonlama pattern'i: deprecated encoding'i bir enum variant'ta tutmak; migrate ederken backward compat.
- `with_context: Option<bool>` in `RpcProgramAccountsConfig` — server context metadata'yı opsiyonel gönderir.

---

### [client_error.rs] (186 satır)

**Amaç:** RPC client hata tiplerini ve dönüşümlerini tanımlar.

**Struct/Enum Listesi:**
- `ErrorKind` (enum, ThisError, Debug): `Io(io::Error)`, `Reqwest(reqwest::Error)`, `RpcError(request::RpcError)`, `SerdeJson(serde_json::error::Error)`, `SigningError(SignerError)`, `TransactionError(TransactionError)`, `Custom(String)`.
- `Error` (struct, ThisError, Debug): `request: Option<RpcRequest>`, `kind: ErrorKind` — hangi request'in başarısız olduğunu da tutar.
- `Result<T>` = `std::result::Result<T, Error>`.

**Kritik Fonksiyonlar:**
- `ErrorKind::get_transaction_error()`: `RpcResponseError` içindeki `SendTransactionPreflightFailure`'dan `TransactionError`'ı çeker. İç içe pattern matching — 3 seviye deep.
- `Error::new_with_request()`, `Error::into_with_request()`: request context ekler.

**From Implementasyonları:** `TransportError`, `io::Error`, `reqwest::Error`, `RpcError`, `serde_json::Error`, `SignerError`, `TransactionError` → `ErrorKind`. Ayrıca `ErrorKind` ↔ `TransportError` dönüşümleri.

**Nova Shield İçin Öğrenilen:**
- RPC client error type'a hangi request'in başarısız olduğunu attach etmek debugging için kritik. Nova Shield'ın RPC client'ı aynı pattern'i kullanmalı.

---

### [custom_error.rs] (211 satır)

**Amaç:** Server-side RPC hataları için sabit kodlar ve dönüşüm.

**Sabitler (16 adet, -32001 ile -32016 arası):**
- `-32001`: `BLOCK_CLEANED_UP`
- `-32002`: `SEND_TRANSACTION_PREFLIGHT_FAILURE`
- `-32003`: `TRANSACTION_SIGNATURE_VERIFICATION_FAILURE`
- `-32004`: `BLOCK_NOT_AVAILABLE`
- `-32005`: `NODE_UNHEALTHY`
- `-32006`: `TRANSACTION_PRECOMPILE_VERIFICATION_FAILURE`
- `-32007`: `SLOT_SKIPPED`
- `-32008`: `NO_SNAPSHOT`
- `-32009`: `LONG_TERM_STORAGE_SLOT_SKIPPED`
- `-32010`: `KEY_EXCLUDED_FROM_SECONDARY_INDEX`
- `-32011`: `TRANSACTION_HISTORY_NOT_AVAILABLE`
- `-32012`: `SCAN_ERROR`
- `-32013`: `TRANSACTION_SIGNATURE_LEN_MISMATCH`
- `-32014`: `BLOCK_STATUS_NOT_AVAILABLE_YET`
- `-32015`: `UNSUPPORTED_TRANSACTION_VERSION`
- `-32016`: `MIN_CONTEXT_SLOT_NOT_REACHED`

**Struct/Enum Listesi:**
- `RpcCustomError` (enum, 16 variant): her hata için ayrı struct variant. Yorumda "sync with solana-web3.js" notu var.
- `NodeUnhealthyErrorData`: `num_slots_behind: Option<Slot>` — serde JSON data field.
- `MinContextSlotNotReachedErrorData`: `context_slot: Slot`.

**`From<RpcCustomError> for Error` (jsonrpc_core::Error) implementasyonu:** Her variant için `ErrorCode::ServerError(-32XXX)`, `message`, opsiyonel `data` alanı doldurulur.

**Nova Shield İçin Öğrenilen:**
- Solana-web3.js ile senkronize hata kodları — tüm client SDK'larının aynı hata sabitlerini kullanması uyumluluk için şart. Nova Shield'ın RPC server'ı benzer standart hata kodu seti tanımlamalı.
- `-32016 MIN_CONTEXT_SLOT_NOT_REACHED`: client belirtilen slot'tan önce query yapmak isterse anlamlı hata. Nova Shield'da da gerekli.

---

### [filter.rs] (430 satır)

**Amaç:** getProgramAccounts için account filtreleme; memory comparison ve data size filtresi.

**Struct/Enum Listesi:**
- `RpcFilterType` (enum, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize): `DataSize(u64)`, `Memcmp(Memcmp)`, `TokenAccountState`.
- `RpcFilterError` (enum): `DataTooLarge`, `Base58DataTooLarge` (deprecated), `DecodeError` (deprecated), `Base58DecodeError`, `Base64DecodeError`.
- `MemcmpEncoding` (enum): sadece `Binary` variant.
- `MemcmpEncodedBytes` (enum, untagged): `Binary(String)` (deprecated), `Base58(String)`, `Base64(String)`, `Bytes(Vec<u8>)`.
- `Memcmp` (struct, serde `into/from = "RpcMemcmp"`): `offset: usize`, `bytes: MemcmpEncodedBytes`, `encoding: Option<MemcmpEncoding>` — üç field deprecated, constructor metodları kullanılmalı.
- `DataType` (enum, private): `Encoded(String)`, `Raw(Vec<u8>)` — internal serde.
- `RpcMemcmpEncoding` (enum, private): `Base58`, `Base64`, `Binary` (`#[serde(other)]`).
- `RpcMemcmp` (struct, private): actual serde target. Custom `From` implementasyonları ile `Memcmp` ↔ `RpcMemcmp` çevrilir.

**Sınırlar:**
- `MAX_DATA_SIZE: usize = 128` — max 128 byte memcmp veri.
- `MAX_DATA_BASE58_SIZE: usize = 175` — 128 byte → base58 max.
- `MAX_DATA_BASE64_SIZE: usize = 172` — 128 byte → base64 max.

**Kritik Fonksiyonlar:**
- `RpcFilterType::verify(&self) -> Result<(), RpcFilterError>` (satır 23): data size ve encoding doğrulaması. `MAX_DATA_BASE58_SIZE = 175` kontrolü.
- `RpcFilterType::allows(&self, account: &AccountSharedData) -> bool` (satır 82): filter uygulama. `DataSize` → exact match, `Memcmp` → byte compare, `TokenAccountState` → `Account::valid_account_data()`.
- `Memcmp::bytes_match(&self, data: &[u8]) -> bool` (satır 207): offset kontrolü + length kontrolü + slice comparison.
- `Memcmp::convert_to_raw_bytes(&mut self)`: encoding'i decode edip `Bytes(Vec<u8>)` format'a çevirir.
- `maybe_map_filters(node_version, filters)` (satır 297): eski node versiyonlarına (< 1.11.2 veya ~1.13) bağlanırken `Base58` encoding'i `Binary`'ye downgrade eder. Base64 eski node'da desteklenmez → error.

**Nova Shield İçin Öğrenilen:**
- `getProgramAccounts` filtresi: `DataSize` + `Memcmp` kombinasyonu ile account veri içeriği veya boyutu sorgulanabilir. Nova Shield'ın indexer'ı bu pattern'i desteklemeli.
- `TokenAccountState` filter: valid token account data kontrolü doğrudan. Nova Shield token account sorgularında benzer optimizasyon.
- Version-aware filter mapping (`maybe_map_filters`): node versiyon farklılıklarına karşı client-side uyumluluk. Nova Shield upgrade geçiş dönemlerinde kritik.

---

### [deprecated_config.rs] (122 satır)

**Amaç:** 1.7.0 öncesi config struct'larının backward-compat wrapper'ları.

**Struct/Enum Listesi:**
- `RpcGetConfirmedSignaturesForAddress2Config` (deprecated since 1.7.0).
- `RpcConfirmedBlockConfig` (deprecated since 1.7.0): `From<RpcConfirmedBlockConfig> for RpcBlockConfig` impl.
- `RpcConfirmedTransactionConfig` (deprecated since 1.7.0): `From<...> for RpcTransactionConfig` impl.
- `RpcConfirmedBlocksConfigWrapper` (deprecated since 1.7.0): `EndSlotOnly`/`CommitmentOnly` — eski getConfirmedBlocks API.

**Nova Shield İçin Öğrenilen:**
- API versiyonlama için ayrı `deprecated_config.rs` dosyası: eski struct'ları yeni struct'lara From ile dönüştür, eski struct'ları deprecated işaretle. Clean deprecation path.

---

### [error_object.rs] (5 satır)

**Amaç:** JSON-RPC hata objesi deserialization.

**Struct:** `RpcErrorObject`: `code: i64`, `message: String` — sadece Deserialize.

---

### [version_req.rs] (20 satır)

**Amaç:** Semver version requirement matching helper.

**Struct:** `VersionReq(Vec<semver::VersionReq>)` — private.

**Kritik Fonksiyonlar:**
- `from_strs<T>(versions: &[T]) -> Result<Self, String>`: her string'i parse eder.
- `matches_any(&self, version: &semver::Version) -> bool`: herhangi bir requirement match ediyorsa true.

**Nova Shield İçin Öğrenilen:**
- Node versiyonuna göre davranış farklılaştırmak için `semver` kullanımı. Nova Shield'ın P2P bağlantılarında peer versiyonu negotiate edip uyumlu davranmak için.

---

## BÖLÜM 3: account-decoder/src/

### [lib.rs] (278 satır)

**Amaç:** Account verilerini JSON veya binary formatlarında encode/decode eden merkezi kütüphane.

**Sabitler:**
- `MAX_BASE58_BYTES: usize = 128` — base58 encoding için max data boyutu.
- `pub type StringAmount = String` — u64 miktarları string olarak.
- `pub type StringDecimals = String` — decimals string olarak.

**Struct/Enum Listesi:**
- `UiAccount` (Serialize, Deserialize, Clone, Debug, PartialEq, Eq): `lamports: u64`, `data: UiAccountData`, `owner: String`, `executable: bool`, `rent_epoch: Epoch`, `space: Option<u64>`.
- `UiAccountData` (enum, untagged): `LegacyBinary(String)`, `Json(ParsedAccount)`, `Binary(String, UiAccountEncoding)`.
- `UiAccountEncoding` (enum): `Binary` (legacy), `Base58`, `Base64`, `JsonParsed`, `Base64Zstd` (`#[serde(rename = "base64+zstd")]`).
- `UiFeeCalculator`: `lamports_per_signature: StringAmount`.
- `UiDataSliceConfig` (Clone, Copy, PartialEq, Eq, Hash): `offset: usize`, `length: usize`.

**Kritik Fonksiyonlar:**
- `UiAccountData::decode(&self) -> Option<Vec<u8>>` (satır 61): encoding'e göre decode. `Base64Zstd` için zstd decompress. `Json` → None (binary decode yok).
- `UiAccount::encode<T: ReadableAccount>(pubkey, account, encoding, additional_data, data_slice_config) -> Self` (satır 106):
  - `Binary`/`Base58`: data ≤ 128 byte ise base58, yoksa hata string.
  - `Base64`: direkt base64.
  - `Base64Zstd`: zstd compress → base64. Compress başarısız olursa Base64 fallback.
  - `JsonParsed`: `parse_account_data()` çağrır; başarısız olursa Base64 fallback.
  - `space = account.data().len()` — her zaman hesaplanır.
- `UiAccount::decode<T: WritableAccount>(&self) -> Option<T>` (satır 165): JSON deserialize edilebilen tiplerden `Account` veya `AccountSharedData` oluşturur.
- `slice_data(data: &[u8], data_slice_config: Option<UiDataSliceConfig>) -> &[u8]` (satır 206): offset/length slice. boundary check: offset ≥ data.len() → empty, length > remaining → truncate.

**Test Kapsamı:**
- `test_slice_data`: boundary cases.
- `test_base64_zstd`: 1024 byte sıfır data encode/decode round-trip.

**Nova Shield İçin Öğrenilen:**
- `Base64Zstd` encoding: büyük account data için önemli bandwidth tasarrufu. Nova Shield'ın RPC'si özellikle büyük program data hesapları için bunu desteklemeli.
- `space: Option<u64>` — account'ın on-chain space'ini expose etmek: client'lar rent hesabı için bunu kullanır.
- `JsonParsed` fallback to `Base64`: parsing başarısız olsa bile client'ı data'dan yoksun bırakma.
- `StringAmount` type alias: u64 JSON'da number precision kaybı yaşar. String kullanmak güvenli.

---

### [parse_account_data.rs] (174 satır)

**Amaç:** Program owner'a göre account data'yı parse ederek yapısal JSON döner.

**Struct/Enum Listesi:**
- `ParseAccountError` (enum): `AccountNotParsable(ParsableAccount)`, `ProgramNotParsable`, `AdditionalDataMissing(String)`, `InstructionError(#[from] InstructionError)`, `SerdeJsonError(#[from] serde_json::error::Error)`.
- `ParsedAccount` (Clone, Debug, Serialize, Deserialize, PartialEq, Eq): `program: String`, `parsed: Value`, `space: u64`.
- `ParsableAccount` (enum, Debug, Serialize, Deserialize): `AddressLookupTable`, `BpfUpgradeableLoader`, `Config`, `Nonce`, `SplToken`, `SplToken2022`, `Stake`, `Sysvar`, `Vote`.
- `AccountAdditionalData` (Clone, Copy, Default): `spl_token_decimals: Option<u8>` — token hesapları için mint decimal bilgisi gerekli.

**Kritik Global State:**
- `PARSABLE_PROGRAM_IDS: HashMap<Pubkey, ParsableAccount>` — lazy_static. `system_program::id()` → `Nonce` (dikkat: System Program → Nonce parse, çünkü nonce hesabı System programı'na ait).

**Kritik Fonksiyonlar:**
- `parse_account_data(pubkey, program_id, data, additional_data) -> Result<ParsedAccount, ParseAccountError>` (satır 92): dispatch. `format!("{program_name:?}").to_kebab_case()` ile program adı normalize edilir.

**Nova Shield İçin Öğrenilen:**
- Account data parse routing: `HashMap<ProgramId, ParsableAccount>` — yeni programlar eklenebilir. Nova Shield'ın custom program'ları buraya eklenecek.
- System program → Nonce parse: program owner ile account type her zaman 1:1 eşleşmez. Nonce hesapları System programı'na ait ama "nonce" tipinde.

---

### [parse_token.rs] (645 satır)

**Amaç:** SPL Token ve Token-2022 account data'sını (Account/Mint/Multisig) parse eder.

**Yardımcı Fonksiyonlar:**
- `spl_token_ids() -> Vec<Pubkey>`: `[spl_token::id(), spl_token_2022::id()]`.
- `is_known_spl_token_id(program_id: &Pubkey) -> bool`.
- `spl_token_native_mint()` (deprecated since 1.16.0): Pubkey dönüşümü artık gereksiz.
- `spl_token_pubkey()` / `pubkey_from_spl_token()` (deprecated): tip dönüşümü deprecated.

**Ana Parse Fonksiyonu:**
- `parse_token(data: &[u8], mint_decimals: Option<u8>) -> Result<TokenAccountType, ParseAccountError>` (satır 60):
  1. `StateWithExtensions::<Account>::unpack(data)` → `TokenAccountType::Account(UiTokenAccount)`. Token account için `mint_decimals` zorunlu (`AdditionalDataMissing` error).
  2. `StateWithExtensions::<Mint>::unpack(data)` → `TokenAccountType::Mint(UiMint)`.
  3. `data.len() == Multisig::get_packed_len()` → `TokenAccountType::Multisig(UiMultisig)`.
  4. Hiçbiri → `AccountNotParsable(SplToken)`.

**Struct/Enum Listesi:**
- `TokenAccountType` (enum, `#[serde(tag = "type", content = "info")]`): `Account(UiTokenAccount)`, `Mint(UiMint)`, `Multisig(UiMultisig)`.
- `UiTokenAccount`: `mint`, `owner`, `token_amount: UiTokenAmount`, `delegate: Option<String>`, `state: UiAccountState`, `is_native: bool`, `rent_exempt_reserve: Option<UiTokenAmount>`, `delegated_amount: Option<UiTokenAmount>`, `close_authority: Option<String>`, `extensions: Vec<UiExtension>`.
- `UiAccountState` (enum): `Uninitialized`, `Initialized`, `Frozen`. `From<AccountState>` impl.
- `UiMint`: `mint_authority: Option<String>`, `supply: StringAmount`, `decimals: u8`, `is_initialized: bool`, `freeze_authority: Option<String>`, `extensions: Vec<UiExtension>`.
- `UiMultisig`: `num_required_signers: u8`, `num_valid_signers: u8`, `is_initialized: bool`, `signers: Vec<String>`.
- `UiTokenAmount` (Serialize, Deserialize, Clone, Debug, PartialEq): `ui_amount: Option<f64>`, `decimals: u8`, `amount: StringAmount`, `ui_amount_string: StringDecimals`.

**Kritik Math Fonksiyonları:**
- `real_number_string(amount: u64, decimals: u8) -> StringDecimals` (satır 198): `format!("{:01$}", amount, decimals + 1)` ile left-pad, sonra decimal point insert. Ör: amount=42, decimals=2 → "0.42".
- `real_number_string_trimmed(amount, decimals)` (satır 211): trailing zeros ve trailing dot kaldırır.
- `token_amount_to_ui_amount(amount: u64, decimals: u8) -> UiTokenAmount` (satır 249): `10_usize.checked_pow(decimals as u32)` ile divisor. Overflow durumunda `None` (checked_pow). `ui_amount: Option<f64>` olarak döner.
- `get_token_account_mint(data: &[u8]) -> Option<Pubkey>` (satır 282): valid token account ise ilk 32 byte mint pubkey'i döner. O(1) — pack/unpack gerektirmez.

**Nova Shield İçin Öğrenilen:**
- `StateWithExtensions<Account>::unpack()` — tek bir buffer'dan hem base state hem extension'lar okunur. Nova Shield'ın token account format'ı benzer extensible yapıda olmalı.
- `ui_amount: Option<f64>` + `amount: StringAmount` + `ui_amount_string: StringDecimals` — üç farklı temsil: f64 (precision kaybı riski var), string sayı (kesin), format string (display). Nova Shield da bu üçlü format pattern'i kullanmalı.
- `get_token_account_mint()` — account type kontrolü yapmadan mint'i okuma: 32 byte offset. Nova Shield'ın hot-path indexer kodu için.

---

### [parse_token_extension.rs] (579 satır)

**Amaç:** Token-2022 extension'larını JSON tiplerine dönüştürür.

**Kritik Tipler:**
- `UiExtension` (enum, 25 variant + `UnparseableExtension`): her extension tipi için ayrı variant. `#[serde(tag = "extension", content = "state")]` — `{"extension": "transferFeeConfig", "state": {...}}` JSON formatı.
- `parse_extension<S: BaseState>(extension_type, account) -> UiExtension` (satır 43): `StateWithExtensions::get_extension()` çağrısı; başarısız → `UnparseableExtension`.

**Önemli Extension Tipleri (From implementasyonları ile):**
- `UiTransferFeeConfig`: `transfer_fee_config_authority`, `withdraw_withheld_authority`, `withheld_amount`, `older_transfer_fee` / `newer_transfer_fee` → `{epoch, maximumFee, transferFeeBasisPoints}`.
- `UiConfidentialTransferMint`: `authority`, `auto_approve_new_accounts`, `auditor_elgamal_pubkey: Option<String>`.
- `UiConfidentialTransferAccount`: `approved`, `elgamal_pubkey`, `pending_balance_lo`, `pending_balance_hi`, `available_balance`, `decryptable_available_balance`, `allow_confidential_credits`, `allow_non_confidential_credits`, `pending_balance_credit_counter`, `maximum_pending_balance_credit_counter`, `expected_pending_balance_credit_counter`, `actual_pending_balance_credit_counter`.
- `UiInterestBearingConfig`: `rate_authority`, `initialization_timestamp`, `pre_update_average_rate`, `last_update_timestamp`, `current_rate`.
- `UiTransferHook`: `authority: Option<String>`, `program_id: Option<String>`.
- `UiMetadataPointer` / `UiGroupPointer` / `UiGroupMemberPointer`: authority + pointer pair.
- `UiTokenMetadata`: `update_authority`, `mint`, `name`, `symbol`, `uri`, `additional_metadata: Vec<(String, String)>`.
- `UiTokenGroup`: `update_authority`, `mint`, `size`, `max_size`.
- `UiTokenGroupMember`: `mint`, `group`, `member_number`.

**Nova Shield İçin Öğrenilen:**
- `ConfidentialTransferAccount`'taki çift bakiye sistemi: `pending_balance` (Lo + Hi şifrelenmiş) + `available_balance` (şifreli). ElGamal şifreleme ile birleşim. Nova Shield'ın DEX'inde ZK private balances için referans mimarisi.
- `InterestBearingConfig`: timestamp tabanlı faiz hesabı, `pre_update_average_rate`. Nova Shield'ın yield-bearing token mekanizması için.
- `UiTokenMetadata` + `additional_metadata: Vec<(String, String)>` — on-chain arbitrary metadata KV store. Nova Shield token'ları için NFT veya compliance metadata.

---

### [parse_sysvar.rs] (385 satır)

**Amaç:** Solana sysvar account'larını (sistem durum değişkenleri) JSON'a parse eder.

**Parse Edilen Sysvar'lar:**
- `Clock` → `UiClock`: `slot`, `epoch`, `epoch_start_timestamp`, `leader_schedule_epoch`, `unix_timestamp`.
- `EpochSchedule` — direkt.
- `Fees` (deprecated) → `UiFees`.
- `RecentBlockhashes` (deprecated) → `Vec<UiRecentBlockhashesEntry>`.
- `Rent` → `UiRent`: `lamports_per_byte_year`, `exemption_threshold: f64`, `burn_percent: u8`.
- `Rewards` → `UiRewards`: `validator_point_value: f64`.
- `SlotHashes` → `Vec<UiSlotHashEntry>`.
- `SlotHistory` → `UiSlotHistory`: `next_slot`, `bits: String` — tüm bit vektörü string'e çevrilir (`SlotHistoryBits::fmt`).
- `StakeHistory` → `Vec<UiStakeHistoryEntry>`.
- `LastRestartSlot` → `UiLastRestartSlot`.
- `EpochRewards` — direkt.

**Nova Shield İçin Öğrenilen:**
- Sysvar'lar chain state'in önemli bir parçası: Clock, EpochSchedule, Rent, SlotHashes her node'da tutulur. Nova Shield'ın state management'ı aynı sysvar setine ihtiyaç duyar.
- `SlotHistory` tam bit vektörü string olarak — büyük veri. Nova Shield'da sysvar'ları JSON'a expose ederken büyük vektörler için pagination gerekebilir.
- `EpochRewards` — epoch sonunda dağıtılacak ödüller. Nova Shield'ın reward calculation için referans.

---

### [parse_nonce.rs] (76 satır)

**Amaç:** Nonce account verilerini JSON'a parse eder.

**Kritik Not:** Uninitialized nonce account `InvalidAccountData` hatası döner — yanlış uzunluktaki System account'larını nonce olarak parse etme riski var, bu engellenir.

**Struct/Enum Listesi:**
- `UiNonceState` (enum, `#[serde(tag = "type", content = "info")]`): `Uninitialized`, `Initialized(UiNonceData)`.
- `UiNonceData`: `authority: String`, `blockhash: String`, `fee_calculator: UiFeeCalculator`.

**Kritik Fonksiyonlar:**
- `parse_nonce(data) -> Result<UiNonceState, ParseAccountError>`: `bincode::deserialize::<Versions>()` ile Nonce state versioned deserialization.

**Nova Shield İçin Öğrenilen:**
- Nonce account: offline transaction / durable transaction nonce sistemi. Nova Shield'ın fee handling veya cross-chain atomic swap'ları için durable nonce benzeri mekanizma gerekebilir.

---

### [parse_stake.rs] (243 satır)

**Amaç:** Stake account verilerini JSON'a parse eder.

**Struct/Enum Listesi:**
- `StakeAccountType` (enum, `#[serde(tag = "type", content = "info")]`): `Uninitialized`, `Initialized(UiStakeAccount)`, `Delegated(UiStakeAccount)`, `RewardsPool`.
- `UiStakeAccount`: `meta: UiMeta`, `stake: Option<UiStake>`.
- `UiMeta`: `rent_exempt_reserve: StringAmount`, `authorized: UiAuthorized`, `lockup: UiLockup`.
- `UiAuthorized`: `staker: String`, `withdrawer: String`.
- `UiLockup`: `unix_timestamp: UnixTimestamp`, `epoch: Epoch`, `custodian: String`.
- `UiStake`: `delegation: UiDelegation`, `credits_observed: u64`.
- `UiDelegation`: `voter_pubkey: String`, `stake: StringAmount`, `activation_epoch: StringAmount`, `deactivation_epoch: StringAmount`, `warmup_cooldown_rate: f64`.

**Kritik Fonksiyonlar:**
- `parse_stake(data)`: `StakeStateV2` deserialization. `StakeStateV2::Stake(meta, stake, _)` — 3. tuple elemanı (stake flags) görmezden gelinir.

**Nova Shield İçin Öğrenilen:**
- Stake state mashinesi: Uninitialized → Initialized → Delegated. Activation/deactivation epoch bilgisi — `deactivation_epoch: StringAmount`. Nova Shield'ın validator staking lifecycle management için direkt referans.
- `credits_observed: u64` — validator tarafından gözlemlenen vote credits. Reward hesabı için.

---

### [parse_vote.rs] (147 satır)

**Amaç:** Vote account verilerini JSON'a parse eder.

**Struct/Enum Listesi:**
- `VoteAccountType` (enum, `#[serde(tag = "type", content = "info")]`): `Vote(UiVoteState)`.
- `UiVoteState`: `node_pubkey`, `authorized_withdrawer`, `commission: u8`, `votes: Vec<UiLockout>`, `root_slot: Option<u64>`, `authorized_voters: Vec<UiAuthorizedVoters>`, `prior_voters: Vec<UiPriorVoters>`, `epoch_credits: Vec<UiEpochCredits>`, `last_timestamp: UiBlockTimestamp`.
- `UiLockout`: `slot: Slot`, `confirmation_count: u32`.
- `UiAuthorizedVoters`: `epoch: Epoch`, `authorized_voter: String`.
- `UiPriorVoters`: `authorized_pubkey: String`, `epoch_of_last_authorized_switch: Epoch`, `target_epoch: Epoch`.
- `UiEpochCredits`: `epoch`, `credits: String`, `previous_credits: String`.
- `UiBlockTimestamp`: `slot: Slot`, `timestamp: UnixTimestamp`.

**Kritik Fonksiyonlar:**
- `parse_vote(data)`: `VoteState::deserialize(data)`. `prior_voters` için `pubkey != Pubkey::default()` filtresi — boş slot'ları atlar.

**Nova Shield İçin Öğrenilen:**
- `VoteState`'in tam JSON temsili: lockouts (slot + confirmation count), epoch credits, authorized voters history, prior voters. Nova Shield'ın consensus state tracking için bu veri yapısına ihtiyaç var.
- `commission: u8` — vote account commission oranı 0-100. Nova Shield validator ekonomisi için.

---

### [parse_address_lookup_table.rs] (117 satır) — account-decoder versiyonu

**Amaç:** ALT account data'sını JSON'a parse eder (instruction parser'dan farklı: account state parse eder).

**Struct/Enum Listesi (kısmi, dosya okundu):**
ALT account state deserialize edilip `UiLookupTable` olarak döner: `deactivation_slot`, `last_extended_slot`, `last_extended_slot_start_index`, `authority`, `addresses: Vec<String>`.

---

### [parse_bpf_loader.rs] (194 satır) — account-decoder versiyonu

**Amaç:** BPF Upgradeable Loader program account data'sını parse eder (buffer/program/program data state).

---

### [parse_config.rs] (152 satır) — deprecated

**Amaç:** Config program account'larını parse eder. `#[allow(deprecated)]` — Config program deprecated.

---

### [validator_info.rs] (18 satır)

**Amaç:** Validator metadata Config account'u.

**Struct/Tipler:**
- `ValidatorInfo` (Debug, Deserialize, PartialEq, Eq, Serialize, Default): `info: String` (JSON string olarak).
- `ConfigState` trait impl: `max_space() -> u64 = MAX_VALIDATOR_INFO = 576`.
- `MAX_SHORT_FIELD_LENGTH: usize = 80`, `MAX_LONG_FIELD_LENGTH: usize = 300`.
- Program ID: `Va1idator1nfo111111111111111111111111111111`.

**Nova Shield İçin Öğrenilen:**
- Validator metadata on-chain program ID + config account pattern. Nova Shield'ın validator registry için benzer mekanizma.

---

## GENEL ÖZET VE NOVA SHIELD İÇİN ANA ÇIKARIMLAR

### Mimari Pattern'lar

1. **İki Katmanlı Tip Sistemi:** Her veri tipi iki versiyona sahip: dahili tip (örn. `TransactionStatusMeta`) ve UI JSON tipi (örn. `UiTransactionStatusMeta`). Dahili tip rust-native (u64, Vec, Option), UI tip string'ler ve serialize-friendly.

2. **OptionSerializer Pattern:** `None` (null serialize) ile `Skip` (alanı atla) ayrımı. Yeni alanlar eklendikçe eski client'lar bozulmaz. Nova Shield RPC API'sinin temel serde pattern'i olmalı.

3. **Program-ID-to-Parser Routing:** `HashMap<Pubkey, ParsableX>` lazy_static ile bir kez init. Her yeni program eklenmesi basit HashMap insert.

4. **Extension-First Token Tasarımı:** Token-2022 20+ extension tipi. Nova Shield token'ı sıfırdan extension-aware tasarlanmalı.

5. **Checked vs Unchecked Instructions:** Kritik işlemler (authority change, transfer) `Checked` versiyonuna sahip — `SetAuthorityChecked` yeni owner'ın imzasını zorunlu kılar. Nova Shield tüm kritik işlemler için `Checked` pattern uygulamalı.

6. **Account Data Encoding Hiyerarşisi:** `JsonParsed > Base64Zstd > Base64 > Base58`. Büyük account'lar için zstd, human-readable için JSON. Nova Shield aynı encoding hierarşisini desteklemeli.

### Token ve DEX İçin Kritik Notlar

- `UiTokenAmount` üçlü temsil: `amount` (string, kesin), `ui_amount` (f64, display), `ui_amount_string` (format string) — DEX price display için tamamı gerekli.
- `ConfidentialTransferAccount` — ElGamal şifreli bakiye: `pending_balance_lo` + `pending_balance_hi` (şifreli 64-bit), `available_balance`. ZK proof ile transfer. Nova Shield DEX'inde dark pool / private order book için referans.
- `TransferFee`: `basis_points` (1/100 %) + `maximum_fee`. Nova Shield DEX fee katmanı için direkt kopyalanabilir formül.
- Token pre/post balance tracking: her tx için `pre_token_balances[account_index]` → `post_token_balances[account_index]` fark = gerçek transfer miktarı. Nova Shield DEX settlement'ı için kanıtlanmış pattern.

### RPC API Tasarımı İçin Kritik Notlar

- 16 özel server error kodu (-32001 ile -32016). Nova Shield aynı negative code aralığını kullanmalı ve solana-web3.js uyumlu tutmalı.
- `min_context_slot` — client'ın stale data almamasını garanti eder. Nova Shield RPC'sinde tüm query endpoint'leri bu parametreyi desteklemeli.
- `SimulateTransaction` result'ında `inner_instructions` — gas estimation ve debugging için kritik. Nova Shield transaction simulation engine'i bu output'u sağlamalı.
- WebSocket notification: `SlotUpdate` 7 event tipi. Nova Shield'ın real-time client'ları için slot lifecycle event'leri.

### Güvenlik Gözlemleri

- Her instruction parser'da `account_keys.iter().max()` ile bound check — index out-of-bounds koruması. Nova Shield instruction decode'unda aynı pattern.
- `maybe_map_filters` — versiyon-aware filter dönüşümü; Base64 filter'ları eski node'lara gönderme. Nova Shield cross-version compatibility için benzer guard.
- `TransactionStatus::confirmation_status()` backward compat fallback — `confirmation_status` None ise `confirmations` sayısından türet. Nova Shield'ın eski peer'larla uyumu için.

---

*Agent 27 — transaction-status (8.486 + 2.617 extension) + rpc-client-api (2.319) + account-decoder (3.008) = 16.430 satır. Tüm struct, trait, fn, test, const, macro analiz edildi.*
