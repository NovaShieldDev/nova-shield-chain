# Penumbra Analizi — Agent 11: core/app (26 Dosya)
## ABCI Entegrasyonu, Action Handler, Mempool, Snapshot, Genesis, Params
**Analiz Tarihi:** 2026-07-22
**Toplam Satır:** ~1850 satır

---

## [action_handler.rs]
**Dosya:** `crates/core/app/src/action_handler.rs`
**Satır Sayısı:** 49

### Genel Amaç
`AppActionHandler` trait'ini tanımlar ve crate bölme sürecindeki geçiş dönemini yönetir. `cnidarium_component::ActionHandler` (bileşen trait'i) ile `AppActionHandler` (uygulama seviyesi trait) arasındaki köprüyü kurar.

### Struct/Enum/Type Listesi
Yok — sadece trait tanımı ve blanket impl.

### Trait Implementasyonları

#### `AppActionHandler` (Satır 26–33)
Async trait. Üç metod:
- `check_stateless(&self, context: Self::CheckStatelessContext) -> Result<()>` — durumsuz doğrulama (ZK proof, imza, format). Associated type `CheckStatelessContext` sayesinde her action kendi context tipini tanımlar.
- `check_historical<S: StateRead + 'static>(&self, _state: Arc<S>) -> Result<()>` — geçmişe dayalı stateful doğrulama. Default impl: `Ok(())` — opsiyonel. `Arc<S>` ile state paylaşımı, ownership güvenli.
- `check_and_execute<S: StateWrite>(&self, state: S) -> Result<()>` — son doğrulama + state mutation. `StateWrite` gerektirir.

**Tasarım Kararı:** `check_historical` varsayılan `Ok()` dönüşü, her action'ın tarihsel kontrol gerektirmediğini kabul eder. `check_and_execute` hem son kontrolü hem de state değişikliğini aynı fonksiyonda birleştirir — ayrı `execute` metodu yok.

#### Blanket impl `AppActionHandler` for `Compat<'a, T>` (Satır 37–49)
Herhangi bir `ComponentActionHandler` implemente eden `T` için `AppActionHandler` otomatik impl.
`Compat<'a, T>(&'a T)` — bir referans newtype wrapper. Bu sayede component trait ile app trait arasında orphan rule ihlali olmadan köprü kurulmuş.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- `check_stateless / check_historical / check_and_execute` üçlüsü temiz bir separation of concerns sağlar. Nova Shield'da aynı pattern doğrudan uygulanmalıdır.
- `Compat` wrapper pattern: Orphan rule sorunlarını çözmek için geçiş dönemlerinde kullanışlı.
- `Arc<S>` ile stateful okuma, parallel check'lere kapı açar (bkz. transaction.rs).

---

## [action_handler/actions.rs]
**Dosya:** `crates/core/app/src/action_handler/actions.rs`
**Satır Sayısı:** 147

### Genel Amaç
`penumbra_sdk_transaction::Action` enum'u için `AppActionHandler` implementasyonu. Tüm action türlerini ilgili component handler'larına yönlendiren üç büyük `match` bloğu içerir.

### Action Türleri (Toplam 22 variant)
- **Shielded Pool:** `Spend`, `Output`, `SwapClaim`
- **DEX:** `Swap`, `PositionOpen`, `PositionClose`, `PositionWithdraw`
- **Staking:** `Delegate`, `Undelegate`, `UndelegateClaim`, `ValidatorDefinition`
- **Governance:** `DelegatorVote`, `ValidatorVote`, `ProposalSubmit`, `ProposalWithdraw`, `ProposalDepositClaim`
- **IBC:** `IbcRelay`, `Ics20Withdrawal`
- **Community Pool:** `CommunityPoolSpend`, `CommunityPoolOutput`, `CommunityPoolDeposit`
- **Auction:** `ActionDutchAuctionSchedule`, `ActionDutchAuctionEnd`, `ActionDutchAuctionWithdraw`
- **Liquidity Tournament:** `ActionLiquidityTournamentVote`

### Trait Implementasyonları

#### `AppActionHandler for Action` — `check_stateless` (Satır 21–61)
Her action için `context: TransactionContext` iletilir:
- `SwapClaim`, `Spend`, `DelegatorVote`, `ActionLiquidityTournamentVote` → gerçek `context` alır
- Çoğu action → `()` alır (context gerektirmez)
- `IbcRelay` → `.with_handler::<Ics20Transfer, PenumbraHost>().check_stateless(())` — çift handler type param
- `Ics20Withdrawal` → `.with_handler::<PenumbraHost>().check_stateless(())`

#### `check_historical` (Satır 63–103)
IBC action'lar için de `with_handler` pattern tekrar edilir. `Output`, `Spend`, `SwapClaim` da `check_historical` alır.

#### `check_and_execute` (Satır 105–145)
IBC action'lar için `action.clone()` — `StateWrite` gerektiği için ownership transfer edilemez, clone zorunlu.

### Tasarım Kararları
1. **Exhaustive match, `_` yok:** Yeni action eklendiğinde derleyici hatası alınır — güvenlik mekanizması.
2. **IBC handler composability:** `with_handler::<Ics20Transfer, PenumbraHost>()` — IBC protokolü uygulama tipini bilmeden, host chain adaptörü üzerinden genelleştirilebilir.
3. **`action.clone()` IBC için:** StateWrite ownership semantiği nedeniyle clone kaçınılmaz.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- Tüm action türlerini tek `match` bloğunda yönetmek, yeni action eklemelerini merkezi bir yerde tutar.
- IBC entegrasyonu için `HostInterface` pattern — Nova Shield'da L1-specific `NovaShieldHost {}` tasarlanmalıdır.
- Exhaustive match + compiler error = güvenli action ekleme prosedürü.

---

## [action_handler/actions/submit.rs]
**Dosya:** `crates/core/app/src/action_handler/actions/submit.rs`
**Satır Sayısı:** 373

### Genel Amaç
Governance proposal submit action handler. Community Pool spend transaction build ve doğrulama dahil — en karmaşık action handler.

### Sabitler
```rust
pub const PROPOSAL_TITLE_LIMIT: usize = 80;        // ⚠️ DON'T CHANGE THIS
pub const PROPOSAL_DESCRIPTION_LIMIT: usize = 10_000; // ⚠️ DON'T CHANGE THIS
```
Consensus kuralları — değiştirilemez, network upgrade gerektirir.

### Static Değişkenler

#### `COMMUNITY_POOL_FULL_VIEWING_KEY: Lazy<FullViewingKey>` (Satır 312–344)
Community Pool'un "burner key"i — hiçbir spend key'e karşılık gelmiyor:
1. `ak_personalization = b"Penumbra_CP_ak"` + `ak_hash_input = b"This hash input is used to form..."` → `blake2b_simd::Params::new().personal(...).hash(...)` → `ak_hash`
2. `nk_personalization = b"Penumbra_CP_nk"` → benzer `nk_hash`
3. `ak = VerificationKey::try_from(decaf377::Element::encode_to_curve(&Fq::from_le_bytes_mod_order(ak_hash.as_bytes())).vartime_compress().0)`
4. `nk = NullifierKey(Fq::from_le_bytes_mod_order(nk_hash.as_bytes()))`
5. `FullViewingKey::from_components(ak, nk)`

Deterministik: aynı hash input → aynı FVK. Kimsenin private key'i yok; görüntülenebilir ama harcanamaz.

### Fonksiyonlar

#### `check_stateless(context: ()) -> Result<()>` (Satır 48–124)
1. `title.len() > 80` → hata
2. `description.len() > 10_000` → hata
3. Payload türüne göre:
   - `Signaling` / `Emergency` → ek kontrol yok
   - `ParameterChange` → stateless kontrol yok
   - `CommunityPoolSpend { transaction_plan }` → action whitelist:
     - **Yasaklanan (ZK gerektiren):** `Spend`, `Output`, `Swap`, `SwapClaim`, `DelegatorVote`, `UndelegateClaim`
     - **Yasaklanan (output claim problemi):** `Delegate`, `Undelegate`
     - **Yasaklanan (iç içe proposal):** `ProposalSubmit`, `ProposalWithdraw`, `ProposalDepositClaim`
     - **İzin verilenler:** `ValidatorDefinition`, `IbcAction`, `ValidatorVote`, `Position*`, `CommunityPool*`, `Ics20Withdrawal`, Dutch auction action'ları
   - `FreezeIbcClient` / `UnfreezeIbcClient` → `ClientId::from_str(client_id)` parse kontrolü

#### `check_and_execute(mut state: S) -> Result<()>` (Satır 126–305)
**15 adım:**
1. Deposit amount kontrolü: `deposit_amount != governance_parameters.proposal_deposit_amount`
2. Proposal ID sıradaki ID ile eşleşmeli: `proposal.id != state.next_proposal_id()`
3. Payload stateful kontrolleri:
   - `ParameterChange`: `change.apply_changes(current_params)` — şu anki parametrelere uygulanabilir mi?
   - `CommunityPoolSpend`: enabled kontrolü → plan decode → `build_community_pool_transaction` → `check_stateless`
   - `FreezeIbcClient` / `UnfreezeIbcClient`: `get_client_state(client_id)`
4. CP tx tekrar build (check'ten execute'a aktarılamaz)
5. `state.new_proposal(proposal)` → proposal_id tahsis
6. `state.put_deposit_amount(proposal_id, deposit_amount)`
7. `state.register_denom(&ProposalNft::deposit(proposal_id).denom())`
8. `state.register_denom(&VotingReceiptToken::new(proposal_id).denom())`
9. `state.put_proposal_state(proposal_id, ProposalState::Voting)`
10. `voting_end = current_block + governance_params.proposal_voting_blocks`
11. `state.put_proposal_voting_start(proposal_id, current_block)`
12. `state.put_proposal_voting_end(proposal_id, voting_end)`
13. SCT position: `(sct_position.epoch(), sct_position.block(), 0).into()` — blok başına sabitlenir
14. `state.mark_proposal_started()`
15. `state.record_proto(event::proposal_submit(self, current_block, voting_end))`

#### `build_community_pool_transaction(transaction_plan: TransactionPlan) -> Result<Transaction>` (Satır 346–363)
```rust
let effect_hash = transaction_plan.effect_hash(&COMMUNITY_POOL_FULL_VIEWING_KEY)?;
transaction_plan.build(
    &COMMUNITY_POOL_FULL_VIEWING_KEY,
    &WitnessData { anchor: Tree::new().root(), state_commitment_proofs: Default::default() },
    &AuthorizationData { effect_hash: Some(effect_hash), spend_auths: Default::default(), ... },
)
```
Boş SCT ile build — CP tx spend içermez.

### Test'ler
`community_pool_fvk_can_be_constructed` — `Lazy` static'in panic etmediğini doğrular.

### Tasarım Kararları
1. **Double-build:** `check_tx_stateful`'dan `execute`'a state aktarılamadığı için CP tx iki kez build edilir.
2. **SCT blok başı pozisyonu:** `(epoch, block, 0)` — intra-block sıralama avantajı engellenir.
3. **Proposal NFT + Vote Receipt Token:** Governance katılımı on-chain kanıtlanabilir.
4. **Commented-out stateful check:** `tx.check_historical` kasten skip edilmiş — "proposal geçerken geçerli olmayabilir" riski.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- "Burner key" pattern: Kimsenin sahip olmadığı FVK ile protocol-owned treasury — Nova Shield treasury için.
- Community Pool action whitelist: ZK proof gerektiren action'lar governance tx'lerinde yasaklı.
- SCT blok başı sabitlemesi → intra-block MEV koruması — Nova Shield governance'ında uygulanmalı.

---

## [action_handler/transaction.rs]
**Dosya:** `crates/core/app/src/action_handler/transaction.rs`
**Satır Sayısı:** 307

### Genel Amaç
`Transaction` için `AppActionHandler` implementasyonu. Paralel action doğrulama için `tokio::task::JoinSet` kullanır.

### Fonksiyonlar

#### `check_stateless(context: ()) -> Result<()>` (Satır 32–71)
Sıra kritik:
1. `valid_binding_signature(self)?` — **ilk.** Binding signature tüm ZK proof'ları bağlar, ucuz kontrol.
2. `num_clues_equal_to_num_outputs(self)?` — FMD clue = output sayısı
3. `check_memo_exists_if_outputs_absent_if_not(self)?`
4. `check_non_empty_transaction(self)?`
5. `JoinSet` ile paralel action check:
```rust
for (i, action) in self.actions().cloned().enumerate() {
    let span = action.create_span(i);
    action_checks.spawn(async move { action.check_stateless(context2).await }.instrument(span));
}
while let Some(check) = action_checks.join_next().await { check??; }
```

#### `check_historical<S: StateRead + 'static>(state: Arc<S>) -> Result<()>` (Satır 74–103)
SAFETY yorumları ile:
1. `tx_parameters_historical_check(state.clone(), self)` — chain_id, expiry
2. `claimed_anchor_is_valid(state.clone(), self)` — SCT anchor
3. `fmd_parameters_valid(state.clone(), self)` — FMD precision
4. Paralel action `check_historical` (JoinSet)

#### `check_and_execute<S: StateWrite>(mut state: S) -> Result<()>` (Satır 107–143)
```rust
state.put_current_source(Some(self.id())); // NoteSource cache
let gas_used = self.gas_cost();
let fee = self.transaction_body.transaction_parameters.fee;
state.pay_fee(gas_used, fee).await?; // Fee önce ödenir
for (i, action) in self.actions().enumerate() {
    action.check_and_execute(&mut state).instrument(span).await?; // Sıralı
}
state.put_current_source(None);
// FMD clue'lar kaydedilir:
for clue in self.transaction_body.detection_data.iter().flat_map(|x| x.fmd_clues.iter()) {
    state.record_clue(clue.clone(), id.clone()).await?;
}
```

### Test'ler

#### `check_stateless_succeeds_on_valid_spend` (Satır 163–244)
- 2 note → SCT → auth path → `SpendPlan + OutputPlan + CluePlan`
- `plan.authorize(OsRng, sk)` + `plan.build_concurrent(fvk, witness, auth)` → `check_stateless`

#### `check_stateless_fails_on_auth_path_with_wrong_root` (Satır 246–307)
- `tx.anchor = wrong_root` → `check_stateless` → `Err`

### Tasarım Kararları
1. **Binding signature önce:** Ucuz → pahalı ZK proof sırası. Büyük bir geçersiz tx sınıfı erken elenir.
2. **Paralel stateless, sıralı execute:** CPU-bound ZK paralel; state mutation sıralı (dependency riski).
3. **Fee-first:** Gas ödenir → action execution. Başarısız action'da fee alınmış olur — spam koruması.
4. **`check??`:** `JoinError` + `anyhow::Error` ayrı `?` ile propagate.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- Binding signature önce → ZK proof paralel: Nova Shield ZK tx'lerde aynı sıra.
- `JoinSet` ile paralel ZK: CPU-bound proof verification thread pool'da koşturulur.
- Gas-first: Spam önleme için fee her şeyden önce alınmalı.

---

## [action_handler/transaction/stateful.rs]
**Dosya:** `crates/core/app/src/action_handler/transaction/stateful.rs`
**Satır Sayısı:** 141

### Genel Amaç
Transaction seviyesi stateful (historical) doğrulama: chain ID, expiry height, SCT anchor, FMD precision.

### Fonksiyonlar

#### `tx_parameters_historical_check<S: StateRead>` (Satır 11–33)
```rust
let TransactionParameters { chain_id, expiry_height, fee: _ } = transaction.transaction_parameters();
// fee: _ bilinçli ignore — execution'da kontrol edilir
chain_id_is_correct(&state, chain_id).await?;
expiry_height_is_valid(&state, expiry_height).await?;
```

#### `chain_id_is_correct<S: StateRead>` (Satır 35–46)
Tam string eşleşme zorunlu. Prefix match yok.

#### `expiry_height_is_valid<S: StateRead>` (Satır 48–65)
```
expiry_height == 0 → sonsuz geçerlilik (early return Ok)
expiry_height >= current_height → geçerli
```

#### `fmd_parameters_valid<S: StateRead>` (Satır 67–89)
Önceki ve mevcut FMD parametrelerini okur, `fmd_precision_within_grace_period` çağırır.

#### `fmd_precision_within_grace_period` (Satır 91–134)
```rust
let using_current_precision = clue_precision == current_fmd_parameters.precision;
let using_previous_precision = clue_precision == previous_fmd_parameters.precision;
let within_grace_period = block_height < previous_fmd_parameters.as_of_block_height + meta_params.fmd_grace_period_blocks;
if using_current_precision || (using_previous_precision && within_grace_period) { continue; }
else { bail!("invalid clue precision"); }
```
Parametreler değişince eski precision'a grace period tanınır.

#### `claimed_anchor_is_valid<S: StateRead>` (Satır 136–141)
`state.check_claimed_anchor(transaction.anchor)`

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- FMD grace period: ZK privacy parametreleri değişirken kullanıcılara uyum süresi.
- `expiry_height == 0` convention: Sonsuz geçerlilik için özel değer — basit ve tutarlı.
- `SAFETY` yorum pattern: Her stateful kontrolde "neden historical state'e karşı güvenli?" belgelenmiş.

---

## [action_handler/transaction/stateless.rs]
**Dosya:** `crates/core/app/src/action_handler/transaction/stateless.rs`
**Satır Sayısı:** 61

### Genel Amaç
Transaction seviyesi saf (pure) fonksiyon doğrulama.

### Fonksiyonlar

#### `valid_binding_signature(tx: &Transaction) -> Result<()>` (Satır 6–15)
```rust
let auth_hash = tx.auth_hash();
tx.binding_verification_key().verify(auth_hash.as_bytes(), tx.binding_sig())
    .context("binding signature failed to verify")
```
Binding signature: ZK proof'ları transaction'a kriptografik bağlama. Başka tx'den proof kopyalamayı imkansız kılar.

#### `num_clues_equal_to_num_outputs(tx: &Transaction) -> anyhow::Result<()>` (Satır 17–32)
`detection_data.fmd_clues.len() != tx.outputs().count()` → hata.

#### `check_memo_exists_if_outputs_absent_if_not(tx: &Transaction) -> anyhow::Result<()>` (Satır 34–50)
`#[allow(clippy::if_same_then_else)]` — bilinçli verbosity:
- `outputs > 0 && memo.is_none()` → hata ("must have memo if outputs present")
- `outputs > 0 && memo.is_some()` → ok
- `outputs == 0 && memo.is_none()` → ok
- `outputs == 0 && memo.is_some()` → hata ("cannot have memo if no outputs")

#### `check_non_empty_transaction(tx: &Transaction) -> anyhow::Result<()>` (Satır 52–61)
`num_actions == 0` → hata.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- Memo-output zorunlu bağlantısı: Alıcı bilgisi output'la birlikte gelmelidir.
- FMD clue/output eşleşmesi: Privacy-preserving detection için zorunlu invariant.

---

## [app/mod.rs]
**Dosya:** `crates/core/app/src/app/mod.rs`
**Satır Sayısı:** 833

### Genel Amaç
Ana uygulama yapısı. CometBFT ABCI interface'ini uygular. Block lifecycle: init_chain → prepare_proposal → process_proposal → begin_block → deliver_tx* → end_block → commit.

### Sabitler
```rust
pub const MAX_BLOCK_TXS_PAYLOAD_BYTES: usize = 1024 * 1024;  // 1MB
pub const MAX_TRANSACTION_SIZE_BYTES: usize = 96 * 1024;      // 96KB
pub const MAX_EVIDENCE_SIZE_BYTES: usize = 30 * 1024;         // 30KB
```

### Struct/Type Tanımları

#### `InterBlockState = Arc<StateDelta<Snapshot>>` (Satır 53)
Bloklar arası state tipi.

#### `App` (Satır 69–71)
```rust
pub struct App { state: InterBlockState }
```

### Trait Implementasyonları

#### `StateReadExt` (Satır 655–758)
- `get_chain_id(&self) -> Result<String>` — `state_key::data::chain_id()`'den raw bytes → UTF8
- `check_chain_id(&self, provided: &str) -> Result<()>` — boş string veya eşleşme = ok
- `get_revision_number(&self) -> Result<u64>` — IBC için chain ID'den versiyon sayısı (`ChainId::from_string(&cid_str).version()`)
- `get_app_params(&self) -> Result<AppParameters>` — 11 component parametresini toplar
- `transactions_by_height(&self, block_height: u64) -> Result<TransactionsByHeightResponse>` — nonverifiable storage'dan tx listesi

**Blanket impl:** `StateRead + tüm component StateReadExt'ler` → `StateReadExt for T`

#### `StateWriteExt` (Satır 760–833)
- `put_chain_id(&mut self, chain_id: String)` — raw bytes olarak yazar
- `put_block_transaction(&mut self, height: u64, tx: ...)` — mevcut liste + yeni tx → nonverifiable_put_raw
- `put_app_params(&mut self, params: AppParameters)` — destructure ile her component'a yazar; `chain_id` **drop** edilir (`#3617` TODO: IBC host change bekliyor)

**Blanket impl:** `StateWrite` → `StateWriteExt for T`

### Fonksiyonlar

#### `App::new(snapshot: Snapshot) -> Self` (Satır 77–85)
`Arc::new(StateDelta::new(snapshot))` — Arc wrapping burada, başka yerde kopyalanmaz.

#### `App::is_ready(state: Snapshot) -> bool` (Satır 88–94)
`!state.is_chain_halted().await`

#### `App::apply(&mut self, state_tx: StateDelta<InterBlockState>) -> Vec<Event>` (Satır 104–115)
```rust
let (state2, mut cache) = state_tx.flatten();
std::mem::drop(state2); // Referans sayısını 1'e düşür
let events = cache.take_events();
cache.apply_to(Arc::get_mut(&mut self.state).expect("no other references to inter-block state"));
events
```
İnvariant: `state_tx` ve `self.state` tek iki referanstır.

#### `App::init_chain(&mut self, app_state: &AppState)` (Satır 117–174)
`Content` genesis:
- Sıra: Sct → ShieldedPool → Distributions → Staking → Ibc → Auction → Dex → CommunityPool → Governance → FeeComponent → Funding → `finish_block()`

`Checkpoint` genesis:
- Sct ve Auction hariç benzer; `None` genesis content ile init

**Not:** `init_chain` event emit etmez — indexer'lar genesis'i ayrıca parse etmeli.

#### `App::prepare_proposal` (Satır 176–240)
1. `is_chain_halted()` → `process::exit(0)`
2. Her tx: boyut kontrolleri → `deliver_tx_bytes` → başarılı ise listeye ekle
3. `response::PrepareProposal { txs: included_txs }`

#### `App::process_proposal` (Satır 242–299)
1. Evidence boyutu ≤ `MAX_EVIDENCE_SIZE_BYTES`; proto encode ile ölçüm
2. Her tx: boyut ≤ `MAX_TRANSACTION_SIZE_BYTES`; toplam ≤ `MAX_BLOCK_TXS_PAYLOAD_BYTES`
3. Her tx `deliver_tx_bytes` → başarısız = `Reject`

#### `App::begin_block` (Satır 301–391)
1. **Parametre değişikliği önce:** `param_changes_for_height(height)` → `change.apply_changes(old_params)` → `put_app_params(new_params)` → `record_proto(EventAppParametersChange)`
2. Component `begin_block` sırası: Sct → ShieldedPool → Distributions → Ibc → Auction → Dex → CommunityPool → Governance → Staking → FeeComponent → Funding
3. `Arc::try_unwrap` — exclusive state
4. **Community Pool tx'leri:** `pending_community_pool_transactions()` → `deliver_tx_allowing_community_pool_spends`

#### `App::deliver_tx_bytes` (Satır 393–399)
`Transaction::decode(tx_bytes)` → `Arc::new(tx)` → `deliver_tx`

#### `App::deliver_tx` (Satır 401–416)
CP spend/output içeren kullanıcı tx'leri reddedilir.

#### `App::deliver_tx_allowing_community_pool_spends` (Satır 418–470) — KRİTİK
```rust
// PARALEL stateless + stateful:
let stateless = tokio::spawn(async move { tx2.check_stateless(()).await }.instrument(...));
let stateful = tokio::spawn(async move { tx2.check_historical(state2).await }.instrument(...));
stateless.await??;
stateful.await??;

// EXCLUSIVE state:
let mut state_tx = self.state.try_begin_transaction().expect("unique Arc");

// INDEX:
state_tx.put_block_transaction(height, transaction.into()).await?;

// EXECUTE:
tx.check_and_execute(&mut state_tx).await?;

Ok(state_tx.apply().1) // events
```

#### `App::end_block` (Satır 472–590)
1. Component `end_block` (aynı sıra)
2. Epoch sonu kontrolü: `is_scheduled_epoch_end || is_epoch_ending_early || is_pre_upgrade_height`
3. Epoch sonu: Sct, Distributions, Ibc, Auction, Dex, CommunityPool, Governance, ShieldedPool, Staking, FeeComponent, Funding `end_epoch` → `finish_epoch()` → yeni epoch kaydı
4. Değilse: `finish_block()`

#### `App::commit` (Satır 597–643)
1. Dummy state swap (`std::mem::replace`)
2. `is_chain_halted()` + `is_pre_upgrade_height()`
3. `state.signal_halt()` (upgrade öncesi)
4. `storage.commit(state)` → `jmt_root`
5. Halt/upgrade: `sleep(2s)` → `process::exit(0)` (tokio spawn — non-blocking)
6. `self.state = Arc::new(StateDelta::new(storage.latest_snapshot()))`

#### `App::cometbft_validator_updates` (Satır 645–651)
`self.state.cometbft_validator_updates().unwrap_or_default()`

### Tasarım Kararları
1. **Arc<StateDelta> mimarisi:** Paylaşımlı okuma + exclusive write ayrımı.
2. **Parametre değişikliği blok başında:** Tüm component'lardan önce — yeni parametrelerle çalışırlar.
3. **2s halt delay:** ABCI commit response önce CometBFT'ye gönderilir, sonra exit.
4. **CP güvenliği:** Kullanıcı tx'leri CP spend/output içeremez — iki ayrı deliver_tx fonksiyonu.
5. **Checkpoint genesis:** Sadece root hash ile yeniden başlatma.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- `Arc<StateDelta<Snapshot>>` state mimarisi doğrudan referans alınmalıdır.
- Paralel check → exclusive state → sıralı execute: Nova Shield consensus katmanında zorunlu sıra.
- Component başlatma sırası: Bağımlılık grafiğine göre belirlenmeli (Sct her zaman ilk).
- Halt mekanizması: ABCI response guarantee + 2s delay + process::exit — production-grade halt.

---

## [app/state_key.rs]
**Dosya:** `crates/core/app/src/app/state_key.rs`
**Satır Sayısı:** 28

### Genel Amaç
State storage key string'lerini merkezi modülde tanımlar. Magic string eliminasyonu.

### Fonksiyonlar

#### `app_version::safeguard() -> &'static str`
`"application/version/safeguard"`

#### `genesis::app_state() -> &'static str`
`"application/genesis/app_state"`

#### `data::chain_id() -> &'static str`
`"application/data/chain_id"`

#### `cometbft_data::transactions_by_height(block_height: u64) -> String`
```rust
format!("{}/transactions_by_height/{block_height:020}", COMETBFT_SUBSTORE_PREFIX)
```
`{:020}` — 20 haneli sıfır dolgu: lexicographic sıralama = sayısal sıralama. 64-bit height için 20 hane yeterli.

### Tasarım Kararları
- **Hiyerarşik key:** `component/category/item` — prefix scan kolay.
- **Zero-padding:** Range query'lerin doğru çalışması için.

### Nova Shield Chain İçin Dersler
- State key'leri merkezi modülde: Magic string dağılmasını önler.
- Height key zero-padding: Lexicographic range query zorunluluğu.

---

## [app_version.rs]
**Dosya:** `crates/core/app/src/app_version.rs`
**Satır Sayısı:** 10

### Genel Amaç
Uygulama versiyon sabiti. Crate versiyonundan bağımsız — consensus breaking değişiklikleri izler.

### Sabitler
```rust
pub const APP_VERSION: u64 = 12;
```

### cfg_if
```rust
if #[cfg(feature="component")] {
    mod component;
    pub use component::{check_and_update_app_version, migrate_app_version};
}
```

### Nova Shield Chain İçin Dersler
- `APP_VERSION` crate semver'den ayrı: Consensus breaking = app version artışı.

---

## [app_version/component.rs]
**Dosya:** `crates/core/app/src/app_version/component.rs`
**Satır Sayısı:** 183

### Genel Amaç
Version safeguard: yanlış binary ile node başlatmayı önler.

### Enum Tanımları
```rust
enum CheckContext { Running, Migration }
```

### Fonksiyonlar

#### `version_to_software_version(version: u64) -> &'static str` (Satır 9–25)
```
1 → "v0.70.x" ... 12 → "v2.1.x" ... _ → "unknown"
```

#### `check_version(ctx: CheckContext, expected: u64, found: Option<u64>) -> anyhow::Result<()>` (Satır 36–100)
- `found.unwrap_or(expected)` — safeguard yoksa mevcut version varsayılan
- `Running`: found == expected-1 → "migration?" önerisi; başka fark → "wrong directory"
- `Migration`: found == APP_VERSION → "already migrated"; başka → "wrong migration"

#### `check_and_update_app_version(s: Storage)` (Satır 132–151)
Startup'ta:
1. `latest_version() == u64::MAX` → pregenesis, skip
2. Safeguard yok → `write_app_version_safeguard` + `commit_in_place`
3. Safeguard var → `check_version(Running, APP_VERSION, found)`

#### `migrate_app_version<S: StateWriteProto>(s: &mut S, to: u64)` (Satır 159–165)
- `to > 1` zorunlu
- `check_version(Migration, to - 1, found)?` — önceki version olmalı
- `write_app_version_safeguard(s, to)`

### Test'ler
`ensure_app_version_is_current_in_checks` — `version_to_software_version(APP_VERSION) != "unknown"`.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- Version safeguard startup kontrolü: Yanlış binary tespiti — Nova Shield'da zorunlu.
- `to > 1` zorunluluğu: İlk versiyona migration mantıksız — iyi guard.
- Test: APP_VERSION her değiştirildiğinde match arm güncellenmeli — otomatik test garanti eder.

---

## [community_pool_ext.rs]
**Dosya:** `crates/core/app/src/community_pool_ext.rs`
**Satır Sayısı:** 54

### Genel Amaç
Community Pool tx okuma/yazma extension trait'leri. Circular dependency (`penumbra-transaction` → `penumbra-governance`) nedeniyle burada tanımlı.

### Trait Implementasyonları

#### `CommunityPoolStateReadExt` (Satır 16–42)
```rust
async fn pending_community_pool_transactions(&self) -> Result<Vec<Transaction>> {
    let prefix = state_key::deliver_community_pool_transactions_at_height(self.get_block_height().await?);
    let proposals: Vec<u64> = self.prefix_proto::<u64>(&prefix)...try_collect().await?;
    for proposal in proposals {
        transactions.push(self.get(&state_key::community_pool_transaction(proposal)).await?...);
    }
    Ok(transactions)
}
```

#### `CommunityPoolStateWriteExt` (Satır 47–53)
```rust
fn put_community_pool_transaction(&mut self, proposal: u64, transaction: Transaction) {
    self.put(state_key::community_pool_transaction(proposal), transaction);
}
```

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- Circular dependency çözümü: Extension trait ile — bağımlılık grafiği temiz kalır.

---

## [event.rs]
**Dosya:** `crates/core/app/src/event.rs`
**Satır Sayısı:** 38

### Genel Amaç
`EventAppParametersChange` event tipi.

### Struct Tanımları
```rust
pub struct EventAppParametersChange {
    pub new_parameters: AppParameters,
}
```

### Trait Implementasyonları
`TryFrom<pb::EventAppParametersChange>`, `From<EventAppParametersChange> for pb`, `DomainType { type Proto = pb::EventAppParametersChange }`.

### Nova Shield Chain İçin Dersler
- Her önemli state değişikliği için event tipi: Indexer ve light client için.

---

## [genesis.rs]
**Dosya:** `crates/core/app/src/genesis.rs`
**Satır Sayısı:** 228

### Genel Amaç
Genesis state yapıları.

### Enum/Struct Tanımları

#### `AppState` (Satır 19–24)
```rust
#[allow(clippy::large_enum_variant)]
pub enum AppState {
    Content(Content),       // Tam genesis
    Checkpoint(Vec<u8>),    // Hash ile yeniden başlatma
}
```

#### `Content` (Satır 36–62)
12 field: `chain_id: String` + 11 component content.

### Fonksiyonlar

#### Builder metodları (Satır 185–210)
- `with_chain_id(self, chain_id: String) -> Self`
- `with_epoch_duration(self, epoch_duration: u64) -> Self`
- `with_unbonding_delay(self, unbonding_delay: u64) -> Self`

### Trait Implementasyonları
`DomainType`, `From<Content> for pb::GenesisContent`, `TryFrom<pb::GenesisContent>`, `From<AppState>`, `TryFrom<pb::GenesisAppState>`. Her field için `ok_or_else(|| anyhow::anyhow!("proto response missing X content"))?.try_into()`.

### Test'ler
`check_validator_defaults` — `stake_content.validators.is_empty()`.

**Not:** Validators `AppState.Content.stake_content`'ta — `InitChain.validators` ignore edilir, validator listesi AppState'ten alınır.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- `Checkpoint` variant: State snapshot ile yeniden başlatma — migration stratejisi.
- Validator genesis uygulama tarafında: Consensus engine'in validators listesi ignore edilir.
- Builder pattern: test ve deployment kolaylığı için.

---

## [lib.rs]
**Dosya:** `crates/core/app/src/lib.rs`
**Satır Sayısı:** 43

### Genel Amaç
Crate root. Feature flag yönetimi.

### Sabitler/Statics
```rust
pub static SUBSTORE_PREFIXES: Lazy<Vec<String>> = Lazy::new(|| vec![
    penumbra_sdk_ibc::IBC_SUBSTORE_PREFIX.to_string(),
    COMETBFT_SUBSTORE_PREFIX.to_string(),
]);
pub static COMETBFT_SUBSTORE_PREFIX: &'static str = "cometbft-data";
```

### Struct Tanımları
```rust
pub struct Compat<'a, T>(&'a T);
```

### cfg_if Feature Gate
`component` feature: `app`, `metrics`, `rpc`, `server`, `action_handler`, `community_pool_ext`, `penumbra_host_chain`.
Feature olmadan: `app_version`, `event`, `genesis`, `params` — Wasm uyumu.

### Tasarım Kararları
- `#![deny(clippy::unwrap_used)]` — `unwrap` yasak; sadece `expect` veya `?`.
- Wasm/non-Wasm split: ABCI/storage feature-gated.

### Nova Shield Chain İçin Dersler
- `#![deny(clippy::unwrap_used)]` crate-wide zorunlu kılınmalı.
- Feature-gated ABCI: Farklı deployment target'ları için esneklik.

---

## [metrics.rs]
**Dosya:** `crates/core/app/src/metrics.rs`
**Satır Sayısı:** 34

### Genel Amaç
Metrics tanımı ve kayıt. `metrics` crate'ini monkey-patch yapar.

### Sabitler
```rust
pub const MEMPOOL_CHECKTX_TOTAL: &str = "penumbra_pd_mempool_checktx_total";
```

### Fonksiyonlar

#### `register_metrics()` (Satır 17–32)
7 component metrics kaydı + mempool counter:
```rust
describe_counter!(MEMPOOL_CHECKTX_TOTAL, Unit::Count, "The total number of checktx requests...");
```

### Nova Shield Chain İçin Dersler
- `register_metrics()` pattern: Her component kendi metrics'ini kaydeder, merkezi toplanma noktası.
- `describe_counter!`: Prometheus scraping için açıklama ekler.

---

## [params.rs]
**Dosya:** `crates/core/app/src/params.rs`
**Satır Sayısı:** 133

### Genel Amaç
`AppParameters` — tüm component parametrelerini birleştiren agregat tip.

### Struct Tanımları
```rust
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AppParameters {
    pub chain_id: String,
    pub auction_params: AuctionParameters,
    pub community_pool_params: CommunityPoolParameters,
    pub distributions_params: DistributionsParameters,
    pub dex_params: DexParameters,
    pub fee_params: FeeParameters,
    pub funding_params: FundingParameters,
    pub governance_params: GovernanceParameters,
    pub ibc_params: IBCParameters,
    pub sct_params: SctParameters,
    pub shielded_pool_params: ShieldedPoolParameters,
    pub stake_params: StakeParameters,
}
```

### Trait Implementasyonları
`DomainType { Proto = pb::AppParameters }`, `TryFrom<pb::AppParameters>`, `From<AppParameters> for pb`, `TryFrom<pb_view::AppParametersResponse>`, `TryFrom<pb::AppParametersResponse>`.

### Nova Shield Chain İçin Dersler
- Tüm parametreleri tek `AppParameters` altında: Governance proposal'larında tek hedef.
- Her component kendi params'ını yönetir; `AppParameters` sadece birleştirici.

---

## [params/change.rs]
**Dosya:** `crates/core/app/src/params/change.rs`
**Satır Sayısı:** 323

### Genel Amaç
Governance parametre değişikliği doğrulama ve uygulama.

### Trait Implementasyonları

#### `ParameterChangeExt for ParameterChange` (Satır 24–50)
```rust
fn apply_changes(&self, app_parameters: AppParameters) -> Result<AppParameters> {
    let app_parameters_raw = serde_json::value::to_value(app_parameters.clone())?;
    let new_app_parameters_raw = self.apply_changes_raw(app_parameters_raw)?;
    let new_app_parameters = serde_json::value::from_value(new_app_parameters_raw)?;
    app_parameters.check_valid_update(&new_app_parameters)?;
    Ok(new_app_parameters)
}
```
JSON round-trip: serialize → JSON merge/patch → deserialize → validate.

### Fonksiyonlar

#### `AppParameters::check_valid_update(&self, new: &AppParameters) -> Result<()>` (Satır 57–158)
`#[deny(unused)]` + tam destructure (no `..`).

**Değiştirilemez invariant'lar** (`check_invariant`):
- `chain_id`
- `epoch_duration`
- `active_validator_limit`
- `signed_blocks_window_len`
- `proposal_valid_quorum`
- `proposal_pass_threshold`
- `proposal_slash_threshold`

#### `AppParameters::check_valid(&self) -> Result<()>` (Satır 160–288)
`check_all` ile tüm geçerlilik kontrolleri:
- `!chain_id.is_empty()`
- `epoch_duration >= 1`
- `unbonding_delay >= epoch_duration * 2 + 1`
- `active_validator_limit > 3`
- `slashing_penalty_misbehavior` in `[1, 100_000_000]`
- `slashing_penalty_downtime` in `[1, 100_000_000]`
- `signed_blocks_window_len >= 2`
- `missed_blocks_maximum >= 1`
- `(!inbound_ics20 && !outbound_ics20) || ibc_enabled`
- `proposal_voting_blocks >= 1`
- `proposal_deposit_amount >= 1u64`
- `proposal_valid_quorum > Ratio::new(0, 1)`
- `proposal_pass_threshold >= Ratio::new(1, 2)`
- `proposal_slash_threshold > Ratio::new(1, 2)`
- `min_validator_stake >= 1_000_000u128` (1 penumbra)

#### `check_all<'a>` (Satır 293–310)
`(bool, description)` listesini filtreler, başarısız olanları tek hata mesajında birleştirir.

#### `check_invariant<'a, T: Eq + 'a>` (Satır 314–322)
`check_all`'a mapper: `(old == new, "{name} can't be changed")`.

### Tasarım Kararları
1. **JSON round-trip:** Governance değişikliği insan-okunabilir JSON patch formatında.
2. **`#[deny(unused)]` + no `..`:** Yeni field eklenmesinde otomatik lint hatası.
3. **Denylist TODO (satır 26–29):** Henüz implementasyonu yok — `#3593`.
4. **`unbonding_delay >= epoch_duration * 2 + 1`:** Slashing penceresi güvenliği.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- JSON round-trip governance: İnsan-okunabilir proposal formatı, audit kolaylığı.
- Değiştirilemez parametreler: chain_id, epoch_duration, validator limit — konsensus stabilitesi.
- `check_all` pattern: Tüm hataları birleştir, kullanıcıya tek mesajda göster.
- `slashing_penalty <= 100_000_000` (basis points²): Aşırı ceza üst sınırı.

---

## [penumbra_host_chain.rs]
**Dosya:** `crates/core/app/src/penumbra_host_chain.rs`
**Satır Sayısı:** 31

### Genel Amaç
IBC `HostInterface` implementasyonu — chain-specific data'yı IBC protokolüne sunar.

### Struct Tanımları
```rust
#[derive(Clone)]
pub struct PenumbraHost {}  // Zero-size type
```

### Trait Implementasyonları

#### `HostInterface for PenumbraHost`
- `get_chain_id<S: StateRead>(state: S) -> Result<String>` → `state.get_chain_id()`
- `get_revision_number<S: StateRead>(state: S) -> Result<u64>` → `state.get_revision_number()`
- `get_block_height<S: StateRead>(state: S) -> Result<u64>` → `state.get_block_height()`
- `get_block_timestamp<S: StateRead>(state: S) -> Result<tendermint::Time>` → `state.get_current_block_timestamp()`

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- ZST host adapter: `NovaShieldHost {}` — sıfır runtime maliyeti.
- Chain-agnostic IBC: Host interface adaptörü chain'i bildirir, IBC kodu chain'i bilmez.

---

## [rpc.rs]
**Dosya:** `crates/core/app/src/rpc.rs`
**Satır Sayısı:** 136

### Genel Amaç
gRPC servis router — 20+ component query server'ını tek `Routes`'a toplar.

### Fonksiyonlar

#### `routes(storage: &Storage, tm_proxy: ...) -> Result<Routes>` (Satır 65–136)
Her servis `tonic_web::enable` (HTTP/1.1 uyum) ile sarılı:
- Storage, Auction, App, CompactBlock, Dex + Simulation, Fee, Governance, Sct, ShieldedPool + Transfer + Bank, CommunityPool, Stake, IBC (Client + Channel + Connection), TendermintProxy, Funding
- `tonic_reflection::server::Builder::configure()` — gRPC reflection
- Timeout disable (#2932) — streaming request'ler için

### Nova Shield Chain İçin Dersler
- Modüler RPC: Her component kendi server'ını expose eder.
- gRPC reflection: API keşfini kolaylaştırır.

---

## [rpc/query.rs]
**Dosya:** `crates/core/app/src/rpc/query.rs`
**Satır Sayısı:** 58

### Struct Tanımları
```rust
pub(super) struct AppQueryServer { storage: Storage }
```

### Trait Implementasyonları

#### `QueryService for AppQueryServer`
- `transactions_by_height` — `state.transactions_by_height(block_height)` → response
- `app_parameters` — `state.get_app_params()` → proto → response

---

## [server.rs]
**Dosya:** `crates/core/app/src/server.rs`
**Satır Sayısı:** 86

### Genel Amaç
ABCI server factory — dört service'i birleştirir.

### Fonksiyonlar

#### `new(storage: Storage) -> Server<...>` (Satır 24–74)
```rust
let consensus = tower::ServiceBuilder::new()
    .layer(request_span::layer(...))
    .layer(EventIndexLayer::index_all())  // Tüm attribute'lar indexed
    .service(Consensus::new(storage.clone()));

let mempool = tower::ServiceBuilder::new()
    .layer(request_span::layer(...))
    .service(tower_actor::Actor::new(10, |queue| Mempool::new(storage.clone(), queue).run()));

tower_abci::v037::Server::builder()
    .consensus(consensus).snapshot(Snapshot {}).mempool(mempool).info(info).finish().expect(...)
```

### Test'ler
`servers_can_listen` — `#[allow(dead_code, unreachable_code)]` — compile-time trait bound testi.

### Nova Shield Chain İçin Dersler
- Tower middleware stack: request span → event index → service.
- `tower_actor::Actor::new(10, ...)` — mempool actor, 10 kapasiteli queue.

---

## [server/consensus.rs]
**Dosya:** `crates/core/app/src/server/consensus.rs`
**Satır Sayısı:** 264

### Genel Amaç
ABCI consensus service. CometBFT mesajlarını `App`'e yönlendirir.

### Struct Tanımları
```rust
pub struct Consensus {
    queue: mpsc::Receiver<Message<Request, Response, BoxError>>,
    storage: Storage,
    app: App,
}
pub type ConsensusService = tower_actor::Actor<Request, Response, BoxError>;
```

### Fonksiyonlar

#### `Consensus::run(mut self) -> Result<(), BoxError>` (Satır 59–109)
Actor döngüsü:
```rust
while let Some(Message { req, rsp_sender, span }) = self.queue.recv().await {
    let _ = rsp_sender.send(Ok(match req {
        Request::InitChain(x) => Response::InitChain(self.init_chain(x).await.expect("must succeed")),
        Request::PrepareProposal(x) => Response::PrepareProposal(self.prepare_proposal(x).await.expect(...)),
        Request::ProcessProposal(x) => Response::ProcessProposal(self.process_proposal(x).await.expect(...)),
        Request::BeginBlock(x) => Response::BeginBlock(self.begin_block(x).await.expect(...)),
        Request::DeliverTx(x) => Response::DeliverTx(self.deliver_tx(x).instrument(span.clone()).await),
        Request::EndBlock(x) => Response::EndBlock(self.end_block(x).instrument(span).await),
        Request::Commit => Response::Commit(self.commit().await.expect("must succeed")),
    }));
}
```
`.expect` — InitChain, Prepare, Process, Begin, Commit başarısız olamaz; DeliverTx ve EndBlock fallible.

#### `init_chain` (Satır 115–161)
Validators `init_chain.validators` ignore edilir — `app_state`'ten alınır.
Checkpoint: `latest_snapshot().root_hash()`
Content: `latest_version() == u64::MAX` kontrolü → `app.commit(storage)`

#### `prepare_proposal` / `process_proposal` (Satır 163–184)
```rust
let mut tmp_app = App::new(self.storage.latest_snapshot()); // İzole fork
Ok(tmp_app.prepare_proposal/process_proposal(proposal).await)
```

#### `deliver_tx` (Satır 199–222)
Fallible — hata: `code: 1, log: format!("{e:#}")`.

#### `end_block` (Satır 224–253)
Version sanity check: `latest_state_version >= end_block.height` → `tracing::warn`.
Validator updates → response.

#### `commit` (Satır 255–263)
`retain_height: 0u32` — tüm geçmişi koru (IBC, light client için).

### Tasarım Kararları
1. **PrepareProposal izolasyonu:** tmp_app fork — state kirlenmez.
2. **`retain_height: 0`:** Penumbra tam geçmişe ihtiyaç duyar.
3. **Consensus mesajları `.expect`:** Bu mesajlar başarısız olamaz; başarısız olursa node crash etmeli.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- PrepareProposal tmp_app fork: Nova Shield consensus'da zorunlu.
- `retain_height: 0`: IBC ve light client için tam geçmiş.
- Fallible DeliverTx: ABCI spec uyumlu — kod=1 ile hata, node crash etmez.

---

## [server/events.rs]
**Dosya:** `crates/core/app/src/server/events.rs`
**Satır Sayısı:** 121

### Genel Amaç
`EventIndexLayer` — Tower middleware. ABCI event attribute'larını regex pattern ile index=true/false işaretler.

### Struct Tanımları

#### `EventIndexLayer` (Satır 16–24)
```rust
pub struct EventIndexLayer {
    pub index: RegexSet,    // index=true yapılacak key pattern'ları (priority)
    pub no_index: RegexSet, // index=false yapılacak key pattern'ları
}
```

#### `EventIndex<S>` (Satır 11–14)
```rust
pub struct EventIndex<S> { svc: S, config: EventIndexLayer }
```

### Fonksiyonlar

#### `EventIndexLayer::index_all() -> Self` (Satır 26–33)
`index: RegexSet::new([""])` — boş regex her şeyle eşleşir.

#### `adjust_events(&self, events: &mut [Event])` (Satır 35–65)
Key format: `"{event.kind}.{attr.key}"` — Cosmos SDK format.
Sıra: no_index önce → index sonra (`index` öncelikli).
Non-UTF8 key → warn + skip.

### Trait Implementasyonları

#### `Service<Request> for EventIndex<S>` (Satır 82–120)
```rust
async move {
    let mut rsp = rsp.await?;
    match rsp {
        Response::BeginBlock(ref mut msg) => config.adjust_events(&mut msg.events),
        Response::DeliverTx(ref mut msg) => config.adjust_events(&mut msg.events),
        Response::EndBlock(ref mut msg) => config.adjust_events(&mut msg.events),
        _ => {} // InitChain, Commit, Prepare/Process → event yok
    }
    Ok(rsp)
}.boxed()
```

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- Event indexing middleware: Selective indexing depolama maliyetini düşürür.
- `RegexSet`: O(1) multi-pattern matching.
- Tower Layer pattern: Nova Shield ABCI middleware stack referansı.

---

## [server/info.rs]
**Dosya:** `crates/core/app/src/server/info.rs`
**Satır Sayısı:** 649

### Genel Amaç
ABCI Info service + IBC query handler.

### Sabitler
```rust
const ABCI_INFO_VERSION: &str = env!("CARGO_PKG_VERSION");
```

### Struct Tanımları
```rust
#[derive(Clone, Debug)]
pub struct Info { storage: Storage }
```

### Fonksiyonlar

#### `Info::info` (Satır 65–95)
State'ten `block_height` (JMT versiyonu değil, çünkü genesis migration phantom block problemi) ve `app_version`, `last_block_app_hash` alır.

#### `Info::get_snapshot_for_height` (Satır 97–128)
- `height == 0` → `latest.version().saturating_sub(1)` (ABCI semantic: latest-1)
- `height > 0` → `storage.snapshot(height)`

#### `Info::query` (Satır 130–604)
Path-based dispatch (10+ path):
- `"state/key"` — JMT proof: `snapshot.get_with_proof(key)` → ICS23 `jmt:v` proof ops (Exist/Nonexist; Batch/Compressed → hata)
- IBC paths: connections, channels, connection_channels, client_states (sadece `07-tendermint-*`), packet_commitments (1'den başlar), packet_acknowledgements, unreceived_packets, unreceived_acks
- `_` → hata

#### `Service<InfoRequest> for Info` (Satır 607–649)
- `Info` → soft-return (code=1, panic yok)
- `Query` → soft-return
- `Echo` → echo

### Tasarım Kararları
1. **ABCI Info + IBC query:** IBC light client query'leri ABCI path üzerinden.
2. **PacketCommitments 1'den:** IBC spec gereği.
3. **`height=0` ABCI semantic:** latest-1.
4. **Sadece `07-tendermint`:** Başka client type henüz yok.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- ICS23 proof format: JMT proof'larını IBC'ye expose etme.
- `height=0` semantic: Nova Shield ABCI'da korunmalı.
- IBC query via ABCI Info: Relayer uyumluluğu için gerekli.

---

## [server/mempool.rs]
**Dosya:** `crates/core/app/src/server/mempool.rs`
**Satır Sayısı:** 83

### Genel Amaç
ABCI Mempool service. Gelen tx'leri izole App fork'u üzerinde doğrular.

### Struct Tanımları
```rust
pub struct Mempool {
    queue: mpsc::Receiver<Message<Request, Response, BoxError>>,
    storage: Storage,
}
```

### Fonksiyonlar

#### `check_tx(&mut self, req: Request) -> Result<Response, BoxError>` (Satır 29–64)
```rust
let mut app = App::new(self.storage.latest_snapshot()); // İzole fork
match app.deliver_tx_bytes(tx_bytes.as_ref()).await {
    Ok(events) => {
        metrics::counter!(MEMPOOL_CHECKTX_TOTAL, "kind" => kind_str, "code" => "0").increment(1);
        Ok(Response::CheckTx(CheckTxRsp { events, ..Default::default() }))
    }
    Err(e) => {
        metrics::counter!(MEMPOOL_CHECKTX_TOTAL, "kind" => kind_str, "code" => "1").increment(1);
        Ok(Response::CheckTx(CheckTxRsp { code: 1.into(), log: format!("{e:#}"), ..Default::default() }))
    }
}
```
`CheckTxKind::New` / `CheckTxKind::Recheck` — metrics label'da ayrışır.

#### `run(mut self) -> Result<(), BoxError>` (Satır 66–83)
Actor döngüsü. TODO: `recv_many` ile batch processing.

### Tasarım Kararları
1. **Tam execution doğrulama:** `deliver_tx_bytes` — sanki gerçek tx gibi tam check.
2. **İzolasyon:** Her check için yeni snapshot fork.
3. **Sequential şimdi:** TODO: batch processing ile throughput artışı.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- CheckTx ≈ DeliverTx: Full execution path ile mempool doğrulama → spam koruması.
- Metrics `kind + code`: Mempool health monitoring.
- `recv_many` TODO: Nova Shield'da başlangıçtan implement edilmeli.

---

## [server/snapshot.rs]
**Dosya:** `crates/core/app/src/server/snapshot.rs`
**Satır Sayısı:** 38

### Genel Amaç
ABCI Snapshot service — no-op.

### Struct Tanımları
```rust
#[derive(Clone, Debug)]
pub struct Snapshot {}
```

### Trait Implementasyonları

#### `Service<SnapshotRequest> for Snapshot`
```rust
Ok(match req {
    Request::ListSnapshots => Response::ListSnapshots(Default::default()),
    Request::OfferSnapshot(_) => Response::OfferSnapshot(Default::default()),
    Request::LoadSnapshotChunk(_) => Response::LoadSnapshotChunk(Default::default()),
    Request::ApplySnapshotChunk(_) => Response::ApplySnapshotChunk(Default::default()),
})
```
Tüm request'ler boş/default yanıt.

### Tasarım Kararları
Snapshot state sync, Penumbra ZK state'i ile karmaşık — kasıtlı no-op.

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- Başlangıçta no-op kabul edilebilir; ZK state sync özel mekanizma gerektirir.
- Protocol uyumu için doğru response type döndürülmeli.

---

## GENEL MİMARİ DEĞERLENDİRMESİ

### Kritik Sayısal Değerler
| Değer | Sayı | Kaynak |
|---|---|---|
| `APP_VERSION` | 12 (v2.1.x) | `app_version.rs:3` |
| `MAX_BLOCK_TXS_PAYLOAD_BYTES` | 1MB | `app/mod.rs:56` |
| `MAX_TRANSACTION_SIZE_BYTES` | 96KB | `app/mod.rs:59` |
| `MAX_EVIDENCE_SIZE_BYTES` | 30KB | `app/mod.rs:62` |
| `PROPOSAL_TITLE_LIMIT` | 80 | `submit.rs:39` |
| `PROPOSAL_DESCRIPTION_LIMIT` | 10,000 | `submit.rs:43` |
| `Consensus::QUEUE_SIZE` | 10 | `consensus.rs:38` |
| `min_validator_stake` | >= 1,000,000 (1 penumbra) | `change.rs:284` |
| `unbonding_delay` | >= epoch_duration * 2 + 1 | `change.rs:228` |
| `active_validator_limit` | > 3 | `change.rs:231` |
| `slashing_penalty` max | 100,000,000 | `change.rs:239` |
| Halt delay | 2s | `app/mod.rs:631` |

### Action Handler Mimarisi Özeti
```
Transaction::check_stateless
  └── valid_binding_signature (önce, ucuz)
  └── num_clues_equal_to_num_outputs
  └── check_memo_exists...
  └── check_non_empty_transaction
  └── JoinSet: Action::check_stateless (paralel)
      └── Spend/Output/Swap/... → component handlers

Transaction::check_historical
  └── tx_parameters_historical_check
      └── chain_id_is_correct
      └── expiry_height_is_valid
  └── claimed_anchor_is_valid
  └── fmd_parameters_valid
  └── JoinSet: Action::check_historical (paralel)

Transaction::check_and_execute
  └── put_current_source
  └── pay_fee (ÖNCE)
  └── for each action: check_and_execute (SIRALΙ)
  └── put_current_source(None)
  └── record_clue
```

### Nova Shield Chain İçin Kritik Eylem Planı

**Doğrudan alınacaklar:**
1. `AppActionHandler` üçlüsü — tam olarak aynı interface
2. `Arc<StateDelta<Snapshot>>` state mimarisi
3. Paralel ZK doğrulama: `JoinSet + tokio::spawn`
4. Fee-first execution: `pay_fee` her şeyden önce
5. Binding signature önceliği
6. Version safeguard: startup DB version kontrolü
7. Community Pool / Treasury: burner FVK pattern
8. JSON round-trip parametre governance
9. State key zero-padding
10. Exhaustive action match (no `_`)
11. `EventIndexLayer` Tower middleware
12. PrepareProposal tmp_app fork
13. `retain_height: 0`

**Farklı yapılacaklar:**
1. Snapshot state sync — ZK-compatible mekanizma
2. Async batch CheckTx — `recv_many` başlangıçtan
3. IBC multi-client type desteği
4. Parametre denylist component-level

*Agent 11 — core/app analizi tamamlandı. 26 dosya, ~1850 satır.*
