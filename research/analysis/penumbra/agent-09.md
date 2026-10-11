# Penumbra Analiz — Agent 09
## Kapsam: component/governance (34 dosya)
## Analiz Tarihi: 2026-07-22

---

## Genel Bakis

Penumbra'nin `governance` component'i, on-chain yonetisim sisteminin tamamini kapsar.
Sistem iki katmanli oy kullanimini destekler: validator oylamasi (acik, imzali) ve delegator
oylamasi (gizli, Groth16 ZK-proof ile). Proposal yasam dongusu, NFT tabanli bir durum
makinesiyle yonetilir. Oylama mekanizmasi, validator oylarini delegator oylariyla orantili
olarak birlestiren sofistike bir tally sistemi kullanir.

Toplam analiz edilen kod: ~2.400 satir (34 dosya).

---

## Dosya Analizi

---

## [action_handler.rs]
**Dosya:** `.../governance/src/action_handler.rs`
**Satir Sayisi:** 11

### Genel Amac

Governance action handler'larini submodul olarak organize eder. `cnidarium_component::ActionHandler`
trait'ini kullanir. `ProposalSubmit` action handler'i bu modülde YOKTUR; `penumbra-app` crate'inde
tanimlidir cunku tum diger action'lari ve `TransactionPlan`'i bilmesi gerekir.

### Struct/Enum/Type Listesi

Yok — sadece `pub mod` bildirimleri.

### Tasarim Kararlari

1. **ProposalSubmit ayrilmasi:** Proposal gonderimi, diger action'larla cross-cutting bagimlilik
   gerektiriyor. Bu, circular dependency'den kacmak icin bilinçli bir ayirma kararidir.
2. **Alt modul organizasyonu:** Her governance action tipi kendi handler dosyasinda: temiz SRP.

### Nova Shield Chain Icin Dersler

- Governance action'larini crate sinirlarinda ayirmak, bagimlilik yonetimi acisindan kritik.
  Proposal gonderimi, transaction validation'a bagimli ise ayri bir crate'te konumlandirilmali.

---

## [action_handler/delegator_vote.rs]
**Dosya:** `.../governance/src/action_handler/delegator_vote.rs`
**Satir Sayisi:** 98

### Genel Amac

`DelegatorVote` action'inin stateless ve stateful dogrulamasini uygular.
ZK-proof tabanlı gizli oy kullaniminin tam dogrulama mantigi burada.

### Trait Implementasyonlari

#### `impl ActionHandler for DelegatorVote`
- `type CheckStatelessContext = TransactionContext` — stateless kontrol transaction context gerektirir.

### Fonksiyonlar

#### `check_stateless(&self, context: TransactionContext) -> Result<()>` (Satir 20-53)

Parametreler: `self: &DelegatorVote, context: TransactionContext`
Donus: `Result<()>`

Algoritma:
1. **Auth signature dogrulama (Satir 38-39):**
   ```rust
   rk.verify(context.effect_hash.as_ref(), auth_sig)
   ```
   Randomize edilmis dogrulama anahtari (`rk`) ile spend auth imzasini dogrular.
   `effect_hash` = transaction'in etkisinin hash'i.

2. **ZK-Proof dogrulama (Satir 42-51):**
   Public girisler olusturulur:
   - `anchor` = TCT (State Commitment Tree) Merkle root'u
   - `balance_commitment` = `value.commit(Fr::zero())` — sifir blinding ile commit
   - `nullifier` = not'un gecersizlestirici degeri
   - `rk` = randomize spend auth key
   - `start_position` = proposal'in TCT'daki baslangic pozisyonu

   `proof.verify(&DELEGATOR_VOTE_PROOF_VERIFICATION_KEY, public)` ile Groth16 proof dogrulanir.

#### `check_and_execute<S: StateWrite>(&self, mut state: S) -> Result<()>` (Satir 56-97)

Algoritma adim adim:
1. `check_proposal_votable(*proposal)` — proposal oylama durumunda mi?
2. `check_proposal_started_at_position(*proposal, *start_position)` — TCT pozisyonu dogru mu?
3. `check_nullifier_unspent_before_start_block_height(*proposal, nullifier)` — not proposal baslangicinda harcanmamis miydi?
4. `check_nullifier_unvoted_for_proposal(*proposal, nullifier)` — bu nullifier daha once oy kullanmadi mi?
5. `check_unbonded_amount_correct_exchange_for_proposal(*proposal, value, unbonded_amount)` — delegasyon/unbonding doviz kuru dogru mu?
6. `mark_nullifier_voted_on_proposal(*proposal, nullifier)` — nullifier oylandi olarak isaretlenir
7. `validator_by_delegation_asset(value.asset_id)` — hangi validator'a delege edildigini bulur
8. `cast_delegator_vote(*proposal, identity_key, *vote, nullifier, *unbonded_amount)` — oyu kaydeder
9. Event emit edilir

### Tasarim Kararlari

1. **Stateless ZK-Proof:** Zincir durumuna bakmadan proof dogrulamasi yapilabilir. Bu, para
   parcalama (sharding) ve erken reddetme icin cok degerlidir.
2. **Çift bağımsız dogrulama:** `check_stateless` ZK-proof'u dogrular; `check_and_execute`
   durum kontrollerini yapar. Bu iki faz tamamen ayridir.
3. **Nullifier = double-vote prevention:** Ayni note iki kez kullanilamaz. ZK-proof, kullanicinin
   note'a sahip oldugunu ispat eder; nullifier double-spend'i onler.
4. **`Fr::zero()` blinding:** Delegator oy bakiyesinde blinding faktoru sifir. Bu, bakiye
   miktarinin acik oldugu anlamina gelir (ancak kimin oy kullandigi gizlidir).

### Nova Shield Chain Icin Dersler

- **ZK-Proof + Nullifier kombinasyonu:** Gizli oy kullaniminin standart kalipsi. Nova Shield
  governance'inda da bu yaklasim kullanilabilir: oy kullananin kimligi gizli, ama oy miktari acik.
- **Effect hash kullanimi:** `rk.verify(context.effect_hash.as_ref(), auth_sig)` — auth imzasi
  transaction'in etkisi uzerinde. Bu, tum action'larin birlestirildigi bir hash'e imza atmak
  demektir; replay attack'a karsi cok saglamdir.
- **Iki fasli dogrulama (stateless + stateful):** Nova Shield'de bu pattern mutlaka kullanilmali.
  ZK-proof'lar paralel dogrulanabilir; durum kontrolu seri yapmak zorunda.

---

## [action_handler/deposit_claim.rs]
**Dosya:** `.../governance/src/action_handler/deposit_claim.rs`
**Satir Sayisi:** 75

### Genel Amac

Tamamlanan bir proposal'in deposit'ini geri talep etmeyi saglar. Depozit, proposal'in
sonucuna gore iade edilir (slash edilmisse iade yok).

### Trait Implementasyonlari

#### `impl ActionHandler for ProposalDepositClaim`
- `type CheckStatelessContext = ()` — stateless kontrol yok.

### Fonksiyonlar

#### `check_stateless(&self, _context: ()) -> Result<()>` (Satir 18-21)

Bos — hic stateless kontrol gerekmiyor. Tum kontroller durum gerektiriyor.

#### `check_and_execute<S: StateWrite>(&self, mut state: S) -> Result<()>` (Satir 23-74)

Algoritma:
1. `check_proposal_claimable(self.proposal)` — proposal "Finished" durumunda mi?
2. `check_proposal_claim_valid_deposit(self.proposal, self.deposit_amount)` — deposit miktari dogru mu?
3. Proposal durumu okunur; `Outcome` beklenen outcome ile eslestirilir
4. NFT denominasyonu kaydedilir (passed/failed/slashed NFT)
5. Proposal durumu `Claimed` olarak guncellenir — tekrar claim engellenir
6. Event emit edilir

**Kritik guvenlik notu (Satir 43-51):**
```rust
if *resupplied_outcome != outcome.as_ref().map(|_| ()) {
    anyhow::bail!("proposal {} has outcome {:?}, but deposit claim has outcome {:?}", ...)
}
```
Kullanicinin belirttigi outcome, zincirdeki gercek outcome ile eslesmeli. Bu, sahte outcome
ile claim engeller.

**NFT mekanizmasi (Satir 54-67):**
- `Passed` -> `ProposalNft::passed(proposal).denom()` olarak kayit
- `Failed` -> `ProposalNft::failed(proposal).denom()` olarak kayit
- `Slashed` -> `ProposalNft::slashed(proposal).denom()` olarak kayit

**Not (Satir 35-38):** "The deposit amount is self-minted in the transaction (proof of
deserving-ness is the supplied proposal NFT, which is burned in the transaction)"
Depozit, transaction'a dahil edilen not araciligiyla self-mint edilir. NFT yakildiktan sonra
depozit bu sekilde geri alinir. Bu, ayriyeten bir transfer/withdraw logic'i gerektirmez.

### Tasarim Kararlari

1. **Self-mint pattern:** Depozit para transfer edilmez; transaction'daki denge esitligi ile
   "geri mint edilir". Bu, settlement mantigi gerektirmeden atomik deposit/reclaim saglar.
2. **NFT state machine:** Proposal durumu NFT mint/burn ile takip edilir. Daha cok oy kullanma
   veya claim yapmayi blockchain consensus'u ile onler.
3. **Cift koruma:** Durum `Claimed` olarak degistirilmesi re-entrancy ve double-claim'i onler.

### Nova Shield Chain Icin Dersler

- **Deposit slashability:** Kalitesiz veya spam oneriler icin deposit slash mekanizmasi
  Nova Shield governance'inda benimsenebilir. Bu, spam teklif maliyetini arttirir.
- **Self-mint deposit:** Deposit geri alinirken ayri transfer logic yazmak yerine transaction
  balance denklemi kullanmak daha temiz ve hatasiz bir pattern.

---

## [action_handler/validator_vote.rs]
**Dosya:** `.../governance/src/action_handler/validator_vote.rs`
**Satir Sayisi:** 125

### Genel Amac

Validator oylamasini isler. Validator'lar governance key ile imzali, acik oy kullanir.
Emergency proposal'lar icin anlik enactment mantigi burada.

### Trait Implementasyonlari

#### `impl ActionHandler for ValidatorVote`
- `type CheckStatelessContext = ()` — stateless icin context yok.

### Fonksiyonlar

#### `check_stateless(&self, _context: ()) -> Result<()>` (Satir 18-37)

1. **Governance key imza dogrulama (Satir 22-26):**
   ```rust
   body.governance_key.0.verify(&body_bytes, auth_sig)
   ```
   Body protobuf bytes olarak encode edilir; governance key ile dogrulanir.
   NOT: Bu, spend key degil governance key ile imzalanir. Ayri key = ayri risk profili.

2. **Reason uzunluk kontrolu (Satir 29-31):**
   `body.reason.0.len() > MAX_VALIDATOR_VOTE_REASON_LENGTH` (1024 byte)

#### `check_and_execute<S: StateWrite>(&self, mut state: S) -> Result<()>` (Satir 39-123)

Algoritma:
1. `check_proposal_votable(*proposal)` — proposal acik mi?
2. `check_validator_active_at_proposal_start(*proposal, identity_key)` — validator baslangicta aktif miydi?
3. `check_validator_has_not_voted(*proposal, identity_key)` — daha once oy kullanmadi mi?
4. `check_governance_key_matches_validator(identity_key, governance_key)` — governance key dogru validator'a mi ait?
5. Proposal durumu kontrol edilir; `is_withdrawn()` ise oy kabul edilmez
6. `cast_validator_vote(proposal, identity_key, vote, reason)` — oy kaydedilir
7. **Emergency proposal kontrolu (Satir 89-115):**
   - `proposal_payload.is_emergency()` veya `proposal_payload.is_ibc_freeze()` ise
   - `current_tally` hesaplanir
   - `tally.emergency_pass(total_voting_power, &governance_params)` kontrolu
   - Eger gecerse: `enact_proposal` cagirilir; durum `Finished { Passed }` yapilir
8. Voting power kaydedilir; event emit edilir

### Tasarim Kararlari

1. **Governance key ayrimi:** Validator'larin oy icin ayri bir governance key kullanmasi,
   spend key gizliligini korur. Validator serveri sadece governance key'e sahip olabilir.
2. **Emergency anlik enactment:** Emergency ve IBC freeze proposal'lar +1/3 voting power'a
   ulasinca aninda yururluge girer. Normal oylama bitmez.
3. **Cekilen proposal guard (Satir 73-76):**
   ```rust
   if proposal_state.is_withdrawn() {
       return Ok(());
   }
   ```
   Oy gercekte cast edilmez ama hata da donulmez. Bu, cekilen proposal'lara oy gondermis
   olan validator'larin transaction'larini basarisiz yapmamasini saglar — kullanici deneyimi icin iyi.

### Nova Shield Chain Icin Dersler

- **Governance key vs spend key ayrimi:** Nova Shield validator'larinda governance icin ayri
  key kullanmak production guvenligi icin kritik. Validator sunucu compromise edilse bile
  oy anlaminda zarar sinirlanir.
- **Emergency proposal pattern:** Kritik olaylar (IBC dondurma, zincir durdurma) icin
  dusuk threshold ile aninda enactment = agir saldirgan zararini minimize eder.
- **Graceful withdrawn handling:** Cekilen oneri uzerine oy kullanimi hataya donusmemeli,
  sessizce gecip gitmeli.

---

## [action_handler/withdraw.rs]
**Dosya:** `.../governance/src/action_handler/withdraw.rs`
**Satir Sayisi:** 56

### Genel Amac

Proposal submitter'in kendi teklifini oylama bitmeden geri cekmesini saglar.
Geri cekilen proposal oylama periyodunu tamamlasa da "failed" sayilir, slash edilmez.

### Fonksiyonlar

#### `check_stateless(&self, _context: ()) -> Result<()>` (Satir 18-27)

Tek kontrol: `self.reason.len() > PROPOSAL_WITHDRAWAL_REASON_LIMIT (80)`.
80 karakter siniri — insan tarafindan okunabilir kisa aciklama icin.

#### `check_and_execute<S: StateWrite>(&self, mut state: S) -> Result<()>` (Satir 31-55)

1. `check_proposal_votable(self.proposal)` — proposal hala oylanabilir mi?
2. Durum `Withdrawn { reason }` olarak guncellenir
3. `ProposalNft::unbonding_deposit(*proposal).denom()` kaydedilir — "unbonding deposit" NFT
4. Event emit edilir

### Tasarim Kararlari

1. **NFT degisimi:** Cekme islemi, `deposit` NFT'yi yakar, `unbonding_deposit` NFT uretir.
   Bu, deposit claim sirasinda oylama mi yoksa geri cekilme mi oldugunu izlemeyi saglar.
2. **Sadece votable proposal cekilebilir:** Withdrawn veya Finished olanlar cekilemez.

### Nova Shield Chain Icin Dersler

- **Proposal withdrawal + NFT state tracking:** Geri cekilme durumunu ayri bir NFT ile
  temsil etmek, deposit claim mantigi icin cok elegantir. Nova Shield'de benzer pattern.

---

## [change.rs]
**Dosya:** `.../governance/src/change.rs`
**Satir Sayisi:** 423

### Genel Amac

Yonetisim icin parametre degisikliklerini modelleyen `EncodedParameter` ve `ParameterChange`
tiplerini tanimlar. JSON tabanlı uygulama parametrelerini bilesene/anahtara gore degistirmeyi
saglar. Onkosullu degisiklik (precondition) destegi vardir.

### Struct/Enum/Type Listesi

#### `EncodedParameter` (Satir 10-14)
```rust
pub struct EncodedParameter {
    pub component: String,  // max 64 chars
    pub key: String,        // max 64 chars
    pub value: String,      // max 2048 chars
}
```
- Bir uygulama parametresinin bileseni, anahtari ve JSON encode edilmis degerini tanimlar.
- `DomainType for EncodedParameter` — protobuf ile donusturulur.

**Validasyon (Satir 22-44):**
- `key.len() > 64` -> hata
- `value.len() > 2048` -> hata
- `component.len() > 64` -> hata

#### `ParameterChange` (Satir 64-67)
```rust
pub struct ParameterChange {
    pub changes: Vec<EncodedParameter>,
    pub preconditions: Vec<EncodedParameter>,
}
```
- `changes`: Uygulanacak degisikliklerin listesi.
- `preconditions`: Bu degerlerin sahip olmasi gereken mevcut degerler.
  Oncekosul saglanmazsa degisiklik reddedilir — optimistik kilitleme gibi.

### Fonksiyonlar

#### `encode_parameters(parameters: serde_json::Value) -> Self` (Satir 113-128)

- `parameters` JSON degerini icerigini `EncodedParameter` listesine donusturur.
- Iki katli iç içe JSON nesnelerini (bilesenler/anahtarlar) iterasyonla isler.
- Olusturulan liste hem `changes` hem `preconditions` olarak atanir (template amacli).

#### `apply_changes_raw(mut app_parameters: serde_json::Value) -> Result<serde_json::Value>` (Satir 139-178)

Algoritma:
1. Her `precondition` icin:
   - JSON'dan bilesenin mevcut degerini bul
   - Beklenen degerle esit mi? Degilse bail!
2. Her `change` icin:
   - Bileseni JSON'da bul
   - Yeni degeri insert et (eksikse bile — default degerleri override eder)
3. Degistirilmis `app_parameters` donderilir (ownership alinik)

**Not:** Sahiplik alinmasi, hatali bir degisiklik uygulaninca kismi-mutasyonun caller'a sizmamasi
icin. Basarisizlika orjinal `app_parameters` erisilemez.

#### `fn get_component<'a>(app_parameters, change) -> Result<&'a mut serde_json::Map<...>>` (Satir 181-197)

JSON'dan bileseni bulur ve mutable Map referansi donderir. Bulunamazsa hata.

### Test'ler (Satir 199-421)

- `dump_encoded_parameters`: Ornek JSON parametrelerini encode eder, stdout'a yazdirir.
- `apply_changes_to_gov_params`: Yonetisim parametrelerini degistirip dogrular.
  `proposalVotingBlocks: 17280 -> 17281`, `proposalDepositAmount: 10_000_000 -> 10_000_001`.
- `protojson_rules_block_snake_case_parameter_changes`:
  snake_case anahtar (`proposal_voting_blocks`) eklenirse, JSON'da iki anahtar olusur.
  Protobuf JSON deserializasyonu bunu "duplicate key" olarak reddeder. Bu, snake_case
  parametre gonderme girisimleri saglamca bloke edilir.
- `preconditions_prevent_applying_changes`:
  Karsilanan oncekosul gecti; karsilanmayan oncekosul reddedildi.

### Tasarim Kararlari

1. **Typed parametreler yerine JSON string:** Parametre degisikligi kodu, tum uygulama
   parametrelerinin yapısini bilmek zorunda degil. Generic JSON manipulasyonu yapilir.
   Tip dogrulama, protobuf deserializasyonu sirasinda gerceklesir.
2. **Precondition = optimistic locking:** Iki proposal ayni parametreyi degistirmek isterse,
   birinin precondition'i karsilanmayacaktir. Bu, paralel proposal conflict'ini onler.
3. **Ownership transfer:** `apply_changes_raw` `app_parameters`'i sahiplenir. Basarisiz durumda
   kismi-mutasyona erisim engellenir.
4. **camelCase zorunlulugu:** pbjson format camelCase kullanir. snake_case anahtar gonderilirse
   protobuf bunu ayri anahtar olarak isler ama sonradan duplicate key hatasi verir.

### Nova Shield Chain Icin Dersler

- **JSON tabanlı parametrik yönetisim:** Governance component'in butun parametre yapısını
  bilmesine gerek yok; bilesene/anahtara gore JSON patch semantigi kullanmak esnek ve
  genisletilebilirdir.
- **Precondition / optimistik kilitleme:** Ayni parametreye iki proposal esit zamanda
  gelirse konflikt onlenir. Nova Shield'in governance sisteminde bu muhtemelen gerekli.
- **snake_case blokajı + pbjson:** camelCase zorlamak, parametre anahtarlarini tutarli
  tutar ve duplicate key saldirilarini onler.

---

## [component.rs]
**Dosya:** `.../governance/src/component.rs`
**Satir Sayisi:** 197

### Genel Amac

`Governance` component struct'ini ve `Component` trait implementasyonunu tanimlar. Zincir
yasam dongusu olaylarini (init_chain, begin_block, end_block, end_epoch) isler.

### Struct/Enum/Type Listesi

#### `pub struct Governance {}` (Satir 29)
Bos struct. Sadece `Component` trait'ini implemente etmek icin.

### Trait Implementasyonlari

#### `impl Component for Governance` (Satir 32-75)

**`init_chain<S: StateWrite>(state, app_state)`** (Satir 36-46):
- `governance_params` JMT'ye yazilir
- `init_proposal_counter()` cagrilir — counter her zaman okunabilir olmali

**`begin_block<S: StateWrite + 'static>(_state, _begin_block)`** (Satir 48-53):
- Bos. Governance begin_block'ta hic is yapmaz.

**`end_block<S: StateWrite + 'static>(state, _end_block)`** (Satir 55-67):
```rust
enact_all_passed_proposals(&mut state).await.expect(...)
```
Her blok sonunda: oylama tamamlanan tum proposal'lar degerlendirilir ve uygulanir.
`expect` kullanimi: bu islem hicbir zaman basarisiz olmamali.

**`end_epoch<S: StateWrite + 'static>(state)`** (Satir 69-74):
```rust
state.tally_delegator_votes(None).await?
```
Her epoch sonunda: bekleyen tum delegator oylar toplanir (tally edilir).

### Fonksiyonlar

#### `pub async fn enact_all_passed_proposals<S: StateWrite>(mut state: S) -> Result<()>` (Satir 78-196)

En kritik fonksiyon. Tamamlanan tum proposal'lari isler.

**Algoritma:**
1. `state.unfinished_proposals()` ile bekleyen tum proposal ID'leri alinir
2. Her proposal icin:
   a. `get_block_height() >= proposal_voting_end(proposal_id)` — oylama bitti mi?
   b. Bitmemisse `continue`
   c. Final delegator tally: `state.tally_delegator_votes(Some(proposal_id))`
   d. Mevcut durum okunur:
      - `Voting` -> tally hesaplanir: `current_tally.outcome(total_voting_power, &governance_params)`
        - `Pass` -> `enact_proposal` cagrilir (basarisizsa `warn` loglanir, hata degil!)
        - `Fail` -> sadece log
        - `Slash` -> sadece log
      - `Withdrawn { reason }` -> `Failed { withdrawn: WithReason { reason } }` olmadan enact
      - `Finished` veya `Claimed` -> bu durumda olmamali -> bail!
3. Durum `Finished { outcome }` olarak guncellenir

**Kritik tasarim notu (Satir 122-126):**
```
// IMPORTANT: We **ONLY** enact proposals that have concluded, and whose
// tally is `Pass`, and whose state is not `Withdrawn`. This is the sole
// place in the codebase where we prevent withdrawn proposals from being passed!
```
Withdrawn proposal'in oylama bitmisken Pass olmasi mumkun. Ama bu noktada atlanir.

**Proposal enactment hatasi (Satir 133-138):**
```rust
Err(error) => {
    tracing::warn!(proposal = %proposal_id, %error, "proposal passed but failed to enact");
}
```
Geçen proposal uygulanamıyorsa: warn loglanir, ama durum yine de `Passed` yapilir.
Bu bilinçli: kullanicinin teklif ettiği parametre gecersizse, proposal yine de gecti sayilir.

### Tasarim Kararlari

1. **end_block'ta enactment:** Proposal sonuclari her blok sonunda kontrol edilir. Bu,
   oylama bitis blogunun sonunda aninda uygulanir — epoch beklenmez.
2. **end_epoch'ta tally:** Delegator oylari epoch bazli toplanir. Bu, aggregation maliyetini
   dagitir ve state'in buyumesini sinirlar.
3. **Partial failure tolerance:** Proposal gecebilir ama uygulanamayabilir. Bu bilincli bir
   tasarim karari: "proposal semantik hatasi" governance hatasindan ayrilir.

### Nova Shield Chain Icin Dersler

- **Epoch vs block cycle ayrimi:** Delegator tally'i epoch'a, enactment'i bloga ayirmak
  performans ve guvenlik acisindan mantikli. Nova Shield'de ayni ayrilim uygulanmali.
- **Enactment partial failure:** Parametre degisikligi gecersizse proposal durumu yine de
  Passed olmali — ama degisiklik uygulanmaz. Bu, governance mesajinin uygulamasini
  ayirmak anlamina gelir.

---

## [component/rpc.rs]
**Dosya:** `.../governance/src/component/rpc.rs`
**Satir Sayisi:** 413

### Genel Amac

gRPC `QueryService` implementasyonu. Governance component'in disariya sundugu sorgulama API'si.
Tonic async gRPC sunucu.

### Struct/Enum/Type Listesi

#### `pub struct Server { storage: Storage }` (Satir 36-38)

- `storage: Storage` — cnidarium depolama katmani
- `Server::new(storage)` fabrika metodu

### Trait Implementasyonlari

#### `impl QueryService for Server` (Satir 47-412)

**`proposal_info(request)`** (Satir 49-72):
- `proposal_id` ile baslangic blogu ve TCT baslangic pozisyonunu donderir
- `start_block_height` ve `start_position` bilgisi — delegator vote icin gerekli

**`next_proposal_id(_request)`** (Satir 75-90):
- Siralinacak sonraki proposal ID'yi donderir
- `u64` olarak JMT'den okunur

**`proposal_data(request)`** (Satir 93-155):
- Eksiksiz proposal verisi: baslangic/bitis blogu, TCT pozisyonu, tanim, durum, deposit
- `ProposalDataResponse` — birlesik yanit

**`proposal_rate_data(request)`** (Satir 162-184):
- `state_key::all_rate_data_at_proposal_start(proposal_id)` prefix ile streaming
- Her validator'in baslangic rate data'si stream olarak donderilir

**`proposal_list(request)`** (Satir 190-286):
- `inactive=true`: tum proposal'lar (0..next_id araligi)
- `inactive=false`: sadece tamamlanmamis proposal'lar
- `try_stream!` makrosu ile async stream
- Her proposal icin: tanim + durum + baslangic pozisyonu + baslangic/bitis blogu

**`validator_votes(request)`** (Satir 292-323):
- `state_key::all_validator_votes_for_proposal(proposal_id)` prefix ile streaming
- Identity key, state key'in son segmentinden parse edilir

**`voting_power_at_proposal_start(request)`** (Satir 326-371):
- `identity_key` alanina gore:
  - Varsa: belirli validator'in oylama gucunu donderir
  - Yoksa: toplam oylama gucunu donderir

**`all_tallied_delegator_votes_for_proposal(request)`** (Satir 381-412):
- Belirli proposal icin tum toplanmis delegator oylarini stream eder
- `IdentityKey` state key'in son segmentinden parse edilir

### Tasarim Kararlari

1. **Streaming yanit tipleri:** Bircok yanit streaming: `Pin<Box<dyn Stream<...>>>`.
   Bu, buyuk proposal listelerinde bellek asimi onler.
2. **State key parse etme:** Identity key, key string'in son segmentinden `rsplit('/').next()` ile
   alinir. Bu brittle bir pattern ama tutarli sekilde uygulanmis.
3. **Snapshot kullanimi:** `self.storage.latest_snapshot()` — her yanit icin en son anlık goruntuye
   bakilir. Tutarli okuma saglar.

### Nova Shield Chain Icin Dersler

- **gRPC streaming:** Buyuk veri setleri icin her zaman streaming API kullan. Proposal list
  ve vote list cok buyuk olabilir — bellege yuklemek tehlikeli.
- **State key ayrimi:** identity_key'i key string'in sonundan cikarmak yerine, state key
  yapisinda dogrudan field olarak saklamak daha saglamdir.

---

## [component/view.rs]
**Dosya:** `.../governance/src/component/view.rs`
**Satir Sayisi:** 983

### Genel Amac

`StateReadExt` ve `StateWriteExt` trait'lerini tanimlar. Governance state'ine erisim ve
yazma icin tum yardimci metodlari icerir. Bu dosya governance component'inin en buyuk
ve en kritik dosyasidir.

### Trait Implementasyonlari

#### `pub trait StateReadExt: StateRead + penumbra_sdk_stake::StateReadExt` (Satir 43-585)

Blanket impl: `impl<T: StateRead + penumbra_sdk_stake::StateReadExt + ?Sized> StateReadExt for T {}`

**Kritik metodlar:**

**`is_pre_upgrade_height(&self) -> Result<bool>`** (Satir 47-59):
- Bir sonraki blok upgrade yuksekligi mi kontrol eder.
- Zincir, upgrade oncesi bloktan sonra durdurulacak.

**`get_governance_params(&self) -> Result<GovernanceParameters>`** (Satir 62-66):
- JMT'den governance parametrelerini okur.

**`next_proposal_id(&self) -> Result<u64>`** (Satir 69-74):
- JMT'den counter okur; yoksa 0 donderir.

**`unfinished_proposals(&self) -> Result<BTreeSet<u64>>`** (Satir 105-118):
- `governance/unfinished_proposals/` prefix'i ile tum bitmemis proposal'lari okur.
- Key sonundan proposal_id parse edilir.
- BTreeSet — sirali ve unique.

**`check_proposal_votable(&self, proposal_id) -> Result<()>`** (Satir 202-221):
- `Voting` -> OK
- `Withdrawn` -> bail! (zaten cekilmis)
- `Finished | Claimed` -> bail! (oylama bitmis)
- Mevcut degilse -> bail! (proposal yok)

**`check_proposal_started_at_position(&self, proposal_id, claimed_position) -> Result<()>`** (Satir 223-241):
- TCT pozisyonu kayitli ile claimed eslesmeli.

**`check_nullifier_unspent_before_start_block_height(&self, proposal_id, nullifier) -> Result<()>`** (Satir 244-265):
- `spend_info.spend_height < start_height` -> bail! (not proposal baslangicinda harcanmisti)
- Henuz harcanmamissa OK.

**`check_unbonded_amount_correct_exchange_for_proposal(&self, proposal_id, value, unbonded_amount) -> Result<()>`** (Satir 282-317):
- Delegasyon token'ina sahip olan validator bulunur
- `rate_data_at_proposal_start(proposal_id, validator_identity)` okunur
- `rate_data.unbonded_amount(value.amount).value() != unbonded_amount.value()` -> bail!

Bu, delegator'in baslangic oranini kullanarak ne kadar "unbonded" oylama gucune sahip
oldugunu dogrular. Deflasyon/enflasyon sonrasi oranin dogru kullanildigini garantiler.

**`current_tally(&self, proposal_id) -> Result<Tally>`** (Satir 533-566):

**En kritik okuma metodudur.** Tally hesaplama algoritmasi:
1. `validator_voting_power_at_proposal_start` — tum validator'larin baslangic gucunu al
2. `validator_votes` — tum validator oylarini al
3. `tallied_delegator_votes` — tum toplanmis delegator oylarini al
4. Her validator icin:
   - `delegator_tally = delegator_tallies.remove(&validator).unwrap_or_default()`
   - Eger validator oy kullandiysa:
     - `effective_power = power.saturating_sub(delegator_tally.total())`
       (validator'in gucundan delegator oylayan kism cikarilir)
     - `tally += (vote, effective_power).into()`
   - Delegator tally eklenir (validator oy kullanmis olsa da olmasa da)
5. Kismi kontroller: hic inactive validator oylamamali; hic inactive validator'in delegator'i oylamamali

**Matematiksel ifade:**
```
validator_effective_power = validator_power_at_start - sum(delegator_voted_power_for_this_validator)
total_tally += validator_effective_power * validator_vote
total_tally += sum(delegator_vote * delegator_unbonded_power)
```

Bu, delegator'larin validator'lari "override etmesine" olanak tanir. Bir delegator
validator'un tam tersini oy kullanirsa, o delegator'un oylama gucu validator'in etkin
gucunden dusulur ve kendi oyuna eklenir.

#### `pub trait StateWriteExt: StateWrite + StateReadExt + ...` (Satir 589-982)

Blanket impl: `impl<T: StateWrite + StateReadExt + ?Sized> StateWriteExt for T {}`

**Kritik metodlar:**

**`new_proposal(&mut self, proposal: &Proposal) -> Result<u64>`** (Satir 603-671):

Yeni proposal kaydederken:
1. Proposal ID dogrulama (counter ile eslesme)
2. **Paralel JoinSet kullanimi (Satir 614-658):**
   ```rust
   let mut js = JoinSet::new();
   let mut validator_identity_stream = self.consensus_set_stream()?;
   while let Some(identity_key) = validator_identity_stream.next().await {
       js.spawn(async move {
           // state, rate_data, power futures paralel calisir
       });
   }
   while let Some(per_validator) = js.join_next().await... {
       // sonuclari yaz
   }
   ```
   Tum aktif validator'larin rate_data ve voting_power'i paralel olarak okunur.
   JoinSet ile async paralel okuma — cok sayida validator icin performans kritik.
3. Counter arttirilir (`proposal_id + 1`)
4. Proposal tanimi yazilir

**`put_proposal_state(&mut self, proposal_id, state)`** (Satir 694-710):
- Durumu yazar
- `Voting | Withdrawn` -> `unfinished_proposal` indexine ekle
- `Finished | Claimed` -> `unfinished_proposal` indexinden cikar
Index ile `unfinished_proposals()` verimli prefix tarama yapar.

**`cast_delegator_vote(&mut self, proposal_id, identity_key, vote, nullifier, unbonded_amount) -> Result<()>`** (Satir 762-781):
- `power = unbonded_amount.value() as u64`
- `tally: Tally = (vote, power).into()`
- `untallied_delegator_vote(proposal_id, identity_key, nullifier)` state key'ine yazar

Oy aninda toplanmaz (tallied olmaz). `tally_delegator_votes` end_epoch'ta cagrilir.

**`tally_delegator_votes(&mut self, just_for_proposal: Option<u64>) -> Result<()>`** (Satir 784-865):

Bekleyen (untallied) delegator oylarini toplar:
1. Prefix ile tum bekleyen oylari stream et
2. Key'den proposal_id ve identity_key parse et
3. Mevcut tallied degeri oku
4. Yeni oy eklenir: `current_tally += tally`
5. Yeni tally saklanir, eski key silinir

**Borrow checker sorununu cozme (Satir 793-844):**
```rust
// ...iterasyon sirasinda state mutate edemeyiz...
let mut keys_to_delete = vec![];
let mut new_tallies: BTreeMap<...> = BTreeMap::new();
// stream biter -> drop(prefix_stream)
// sonra yazma yapilir
```
Rust borrow checker nedeniyle: once okuma tamamlanir, drop edilir, sonra yazma yapilir.

**`enact_proposal(&mut self, proposal_id, payload) -> Result<Result<()>>`** (Satir 867-938):

Proposal payload'ina gore uygulama:
- `Signaling` -> hicbir sey yapilmaz (sadece koordinasyon)
- `Emergency { halt_chain: true }` -> `self.signal_halt()` cagirilir
- `ParameterChange(change)` -> `current_height + 1` icin `param_changes_for_height` yazilir
  (Note: iki proposal ayni bloktaki param_changes yazarsa biri digeri ezer — bilinen sorun)
- `CommunityPoolSpend` -> `deliver_community_pool_transaction(proposal_id)` zamanlanir
- `UpgradePlan { height }` -> `signal_upgrade(height)` cagirilir
- `FreezeIbcClient { client_id }` -> IBC client dondurulur (frozen_height = 0/1)
- `UnfreezeIbcClient { client_id }` -> IBC client serbest birakilir

**`signal_halt(&mut self)`** (Satir 971-973):
- `halt_bit = true` yazar. Consensus worker bu bitti okuyunca duracak.
- `nonverifiable_put_proto` kullanimi: JMT yerine nonverifiable store'a yazilir.
  Bu, reaplayabilirligi olmayan (consensus-critical olmayan) veriler icin.

**`ready_to_start(&mut self)`** (Satir 976-978):
- `halt_bit = false` — migration sonrasi yeniden baslatmak icin.

### Tasarim Kararlari

1. **JoinSet ile paralel okuma:** Yeni proposal baslarken tum aktif validator'larin verisi
   paralel okunur. Bu, cok validator li aglarda kritik performans optimizasyonu.
2. **Two-phase delegator tally:** Oylar aninda toplanmaz; epoch sonunda toplu toplanir.
   Bu, her oylama isleminin O(1) yazmaya karsilik gelmesini saglar; O(N) aggregation
   epoch'a ertelenir.
3. **Nonverifiable halt_bit:** Halt biti JMT'de degil; yeniden oynatma sirasinda zincirin
   yeniden durmasini onler.
4. **Prefix index pattern:** `unfinished_proposal` indexi, tum proposal'lari taramak yerine
   prefix sorgu ile aktif proposal'lari verimli bulmayı saglar.

### Nova Shield Chain Icin Dersler

- **Delegator override mekanizmasi:** `effective_power = power - delegator_total` formulunu
  Nova Shield governance'inda benimsemek, delege edilen tokenleri daha yaygin oy
  kullanimina tesvit eder.
- **JoinSet paralel okuma:** Startup veya snapshot sirasinda cok sayida validator'in
  verisini paralel okumak icin `tokio::task::JoinSet` standart pattern.
- **Two-phase tally:** Delegator oylarini hemen aggrege etmemek — bu, her oy transaction'inin
  ucretini sabit tutar (O(1) yaz, O(N) sonra okusur).
- **Halt bit nonverifiable store:** Chain duraklatma mekanizmasi JMT disinda olmali.
  Replay sirasinda (genesis'ten) zincir yeniden durmasin.

---

## [delegator_vote.rs]
**Dosya:** `.../governance/src/delegator_vote.rs`
**Satir Sayisi:** 11

Sadece re-export. `action`, `plan`, `proof`, `view` alt modullerini public yapar.

---

## [delegator_vote/action.rs]
**Dosya:** `.../governance/src/delegator_vote/action.rs`
**Satir Sayisi:** 135

### Genel Amac

Delegator oy eylemi (`DelegatorVote`) ve govde (`DelegatorVoteBody`) tiplerini tanimlar.

### Struct/Enum/Type Listesi

#### `pub struct DelegatorVote` (Satir 13-17)
```rust
pub struct DelegatorVote {
    pub body: DelegatorVoteBody,
    pub auth_sig: Signature<SpendAuth>,  // decaf377-rdsa spend auth imzasi
    pub proof: DelegatorVoteProof,       // Groth16 ZK-proof
}
```

#### `pub struct DelegatorVoteBody` (Satir 27-42)
```rust
pub struct DelegatorVoteBody {
    pub proposal: u64,            // Hangi proposal
    pub start_position: tct::Position, // TCT'de baslangic pozisyonu
    pub vote: Vote,               // Yes/No/Abstain
    pub value: Value,             // Staked not'un degeri (asil ID + miktar)
    pub unbonded_amount: Amount,  // Karsilik gelen unbonded miktar (oylama gucu)
    pub nullifier: Nullifier,     // Harcanan not'un nullifier'i
    pub rk: VerificationKey<SpendAuth>, // Randomize edilmis spend auth key
}
```

Yorumlar (Satir 33, 35):
- `vote: Vote // With flow encryption, this will be a triple of flow ciphertexts`
- `value: Value // With flow encryption, this will be a triple of balance commitments`
Gelecekte flow encryption eklenerek tam gizlilik saglanabilir.

### Trait Implementasyonlari

- `EffectingData for DelegatorVote` — `effect_hash()`: body'den hesaplanir
- `EffectingData for DelegatorVoteBody` — `effect_hash()`: protobuf encode -> hash
- Protobuf `From`/`TryFrom` implementasyonlari

### Nova Shield Chain Icin Dersler

- **Flow encryption gelecek plani:** Yorum satirlari, oy gizliliginin gelecekte arttirilabilecegini
  gosteriyor. Nova Shield, benzer asamali gizlilik yukseltme planini desteklemelidir.

---

## [delegator_vote/plan.rs]
**Dosya:** `.../governance/src/delegator_vote/plan.rs`
**Satir Sayisi:** 219

### Genel Amac

`DelegatorVotePlan`: ZK-proof uretimi icin gerekli herkese acik olmayan bilgileri icerir.
Kullanici tarafinda olusturulur; proof'u bu plandan uretilir.

### Struct/Enum/Type Listesi

#### `pub struct DelegatorVotePlan` (Satir 24-43)
```rust
pub struct DelegatorVotePlan {
    pub proposal: u64,
    pub start_position: tct::Position,
    pub vote: Vote,
    pub staked_note: Note,       // Kullanicinin stake edilmis notu (ozel!)
    pub unbonded_amount: Amount,
    pub position: tct::Position, // Not'un TCT'deki pozisyonu
    pub randomizer: Fr,          // Spend auth key randomizer'i
    pub proof_blinding_r: Fq,    // ZK-proof blinding faktoru 1
    pub proof_blinding_s: Fq,    // ZK-proof blinding faktoru 2
}
```

### Fonksiyonlar

#### `new<R: CryptoRng + RngCore>(rng, proposal, start_position, vote, staked_note, position, unbonded_amount) -> DelegatorVotePlan` (Satir 48-68)

- `randomizer = Fr::rand(rng)` — her oyda farkli randomizer
- `proof_blinding_r = Fq::rand(rng)` — proof blinding
- `proof_blinding_s = Fq::rand(rng)` — proof blinding

#### `delegator_vote(&self, fvk, auth_sig, auth_path) -> DelegatorVote` (Satir 71-82)

- Body + proof + auth_sig birlestirerek tam `DelegatorVote` olusturur.

#### `delegator_vote_body(&self, fvk) -> DelegatorVoteBody` (Satir 85-98)

- Nullifier turetimi:
  ```rust
  Nullifier::derive(fvk.nullifier_key(), self.position, &self.staked_note.commit())
  ```
- Randomize spend key:
  ```rust
  rk: fvk.spend_verification_key().randomize(&self.randomizer)
  ```

#### `delegator_vote_proof(&self, fvk, state_commitment_proof) -> DelegatorVoteProof` (Satir 101-130)

- Public girisler:
  - `anchor = state_commitment_proof.root()`
  - `balance_commitment = staked_note.value().commit(Fr::zero())` (sifir blinding)
  - `nullifier = self.nullifier(fvk)`
  - `rk = self.rk(fvk)`
  - `start_position`
- Private girisler:
  - `state_commitment_proof` — Merkle proof
  - `note = staked_note.clone()`
  - `v_blinding = Fr::from(0u64)` — sifir blinding (bakiye acik)
  - `spend_auth_randomizer = self.randomizer`
  - `ak = *fvk.spend_verification_key()`
  - `nk = *fvk.nullifier_key()`
- `DelegatorVoteProof::prove(r, s, &DELEGATOR_VOTE_PROOF_PROVING_KEY, public, private)`
  — `.expect(...)`: proof uretimi basarisiz olmayin...

#### `balance(&self) -> Balance` (Satir 146-152)

- Voting receipt token uretir: `VotingReceiptToken::new(self.proposal).id()`
- `unbonded_amount` kadar `uvoted_on_<proposal_id>` tokeni

### Tasarim Kararlari

1. **Sifir v_blinding:** Bakiye miktari (oylama gucu) acik. Bu tasarim bilinçli: oylama gucunun
   gizli olmasi ek karmasiklik yaratir; zaten delegasyon acik zincirde gorunur.
2. **VotingReceiptToken:** Oy kullaninca `uvoted_on_<id>` tokeni uretilir. Bu, oy kullanma
   kanitini tasinabilir kilar.

### Nova Shield Chain Icin Dersler

- **Plan/Action ayrimi:** Kullanici tarafinda olusturulan plan (ozel bilgi icerir) ile zincire
  gonderilen action (sadece public bilgi ve proof) kesinlikle ayrilmali.

---

## [delegator_vote/proof.rs]
**Dosya:** `.../governance/src/delegator_vote/proof.rs`
**Satir Sayisi:** 598

### Genel Amac

Delegator oy icin Groth16 ZK-proof devresini (circuit) ve dogrulama/uretim mantigi.
Bu, governance modülünün kriptografik kalbi.

### Struct/Enum/Type Listesi

#### `pub struct DelegatorVoteProofPublic` (Satir 35-46)
```rust
pub struct DelegatorVoteProofPublic {
    pub anchor: tct::Root,              // TCT Merkle root (kamuya acik)
    pub balance_commitment: balance::Commitment, // Bakiye commitment (sifir blinding)
    pub nullifier: Nullifier,           // Not nullifier'i (kamuya acik)
    pub rk: VerificationKey<SpendAuth>, // Randomize spend auth key
    pub start_position: tct::Position,  // Proposal baslangic pozisyonu
}
```

#### `pub struct DelegatorVoteProofPrivate` (Satir 49-63)
```rust
pub struct DelegatorVoteProofPrivate {
    pub state_commitment_proof: tct::Proof, // Merkle inclusion proof
    pub note: Note,                          // Harcanan not (gizli!)
    pub v_blinding: Fr,                      // Bakiye blinding (sifir)
    pub spend_auth_randomizer: Fr,           // rk randomizer'i
    pub ak: VerificationKey<SpendAuth>,      // Spend auth key
    pub nk: NullifierKey,                    // Nullifier key
}
```

#### `pub struct DelegatorVoteCircuit` (Satir 145-148)
```rust
pub struct DelegatorVoteCircuit {
    public: DelegatorVoteProofPublic,
    private: DelegatorVoteProofPrivate,
}
```
- `ConstraintSynthesizer<Fq>` implemente eder — Groth16 devresinin kendisi.

#### `pub struct DelegatorVoteProof([u8; GROTH16_PROOF_LENGTH_BYTES])` (Satir 318)
Sikistririlmis (compressed) Groth16 proof baytlari.

### Trait Implementasyonlari

#### `impl ConstraintSynthesizer<Fq> for DelegatorVoteCircuit` (Satir 150-242)

ZK devresi kisitlamalari — bu fonksiyon R1CS kısıtlamalarını üretir:

**Witness (ozel girisler) (Satir 154-176):**
- `note_var` — Not komitment degiskenini olusturur; adres kontrolu (diversified base not identity)
- `claimed_note_commitment` — iddia edilen not komitmenti
- `delegator_position_var` — TCT'deki pozisyon
- `merkle_path_var` — Merkle yolu (inclusion proof)
- `v_blinding_vars` — bakiye blinding (32 byte)
- `spend_auth_randomizer_var` — randomizer
- `ak_element_var` — spend auth key; identity check (not identity)
- `nk_var` — nullifier key

**Public girisler (Satir 178-185):**
- `anchor_var` — Merkle root
- `claimed_balance_commitment_var` — bakiye commitment
- `claimed_nullifier_var` — nullifier
- `rk_var` — randomize spend key
- `start_position` — baslangic pozisyonu

**Kisitlamalar (Satir 187-238):**

1. **Not komitment butunlugu (Satir 187-189):**
   ```rust
   note_commitment_var.enforce_equal(&claimed_note_commitment)?
   ```

2. **Nullifier butunlugu (Satir 191-194):**
   ```rust
   let nullifier_var = NullifierVar::derive(&nk_var, &delegator_position_var, &claimed_note_commitment)?;
   nullifier_var.enforce_equal(&claimed_nullifier_var)?
   ```

3. **Merkle path dogrulama (Satir 196-203):**
   ```rust
   merkle_path_var.verify(cs, &Boolean::TRUE, &delegator_position_bits, anchor_var, claimed_note_commitment.inner())?
   ```

4. **Randomize spend key butunlugu (Satir 205-207):**
   ```rust
   let computed_rk_var = ak_element_var.randomize(&spend_auth_randomizer_var)?;
   computed_rk_var.enforce_equal(&rk_var)?
   ```

5. **Adres butunlugu (Satir 209-213):**
   ```rust
   let ivk = IncomingViewingKeyVar::derive(&nk_var, &ak_element_var)?;
   let computed_transmission_key = ivk.diversified_public(&note_var.diversified_generator())?;
   computed_transmission_key.enforce_equal(&note_var.transmission_key())?
   ```

6. **Bakiye commitment butunlugu (Satir 215-217):**
   ```rust
   let balance_commitment = note_var.value().commit(v_blinding_vars)?;
   balance_commitment.enforce_equal(&claimed_balance_commitment_var)?
   ```

7. **Baslangic pozisyonu commitment index = 0 (Satir 219-223):**
   ```rust
   let zero_constant = FqVar::constant(Fq::from(0u64));
   let commitment = start_position.commitment()?;
   commitment.enforce_equal(&zero_constant)?
   ```
   Proposal'lar her zaman epoch baslangicinda baslar (index = 0).

8. **Pozisyon < baslangic pozisyonu (Satir 234-238):**
   ```rust
   delegator_position_var.position.enforce_cmp(&start_position.position, Ordering::Less, false)?
   ```
   Not, voting baslangicinda mevcuttu. `should_also_check_equality=false` -> kesinlikle kucuk.
   Ayni blokta delegate edip oy kullanilamasin.

#### `impl DummyWitness for DelegatorVoteCircuit` (Satir 244-293)

Sifir bilgi kaniti parametrelerini generate etmek icin kullanilan sahte witness:
- `[b'f'; 32]` seed ile deterministic key
- `"1upenumbra"` degeri
- Tek notlu SCT, epoch bitis, baslangic pozisyonu

### Fonksiyonlar

#### `check_satisfaction(public, private) -> Result<()>` (Satir 66-121) [cfg(test)]

Devresiz (native) kisitlama dogrulama. Test icin:
- Not komitment eslesmesi
- Nullifier turetimi eslesmesi
- TCT proof dogrulama
- rk eslesmesi
- Adres butunlugu
- Bakiye commitment
- Baslangic pozisyonu commitment = 0
- Pozisyon < baslangic pozisyonu

#### `DelegatorVoteProof::prove(blinding_r, blinding_s, pk, public, private) -> Result<Self>` (Satir 321-336)

- `Groth16::<Bls12_377, LibsnarkReduction>::create_proof_with_reduction` cagirilir
- Sikistririlmis proof (GROTH16_PROOF_LENGTH_BYTES) olarak saklanir

#### `DelegatorVoteProof::verify(&self, vk, public_inputs) -> Result<(), VerificationError>` (Satir 341-396)

Dogrulama:
1. Proof deserialize: `Proof::deserialize_compressed_unchecked`
2. `rk` decompress: `decaf377::Encoding(rk.to_bytes()).vartime_decompress()`
3. Public girisler field elementlerine donusturulur:
   - `to_field_elements!` makrosu
4. `Groth16::verify_with_processed_vk(vk, public_inputs, proof)`
5. Timing loglama: `std::time::Instant::now()` -> `start.elapsed()`

### Test'ler (Satir 431-597)

- `delegator_vote_happy_path`: proptest ile rastgele gecerli ifade uretilir;
  hem native hem circuit satisfaction kontrol edilir.
- `delegator_vote_invalid_start_position`: sifir olmayan commitment index ile baslangic
  pozisyonu verilince hem native hem circuit hatali olmali.

### Tasarim Kararlari

1. **Bls12-377 + LibsnarkReduction:** Penumbra, Bls12-377 pairing uzerinde Groth16 kullanir.
   Bu, decaf377 elliptic curve ile uyumlu (decaf377, Bls12-377 scalar field uzerinde calisir).
2. **`deserialize_compressed_unchecked`:** Proof dogrulamasi zaten kisitlamalari kontrol eder;
   double-check gereksiz ve yavas.
3. **timing log:** Proof dogrulama surekli loglanir. Bu, performans izleme icin.
4. **`DummyWitness`:** Groth16 parametre oretimi icin sahte witness — test ve setup icin.

### Nova Shield Chain Icin Dersler

- **Groth16 + Bls12-377 pattern:** Penumbra'nin tam proof devresini Nova Shield icin
  ornek almak mantiklidir. Arkworks/Groth16 kullaniyorsak ayni devre yapisini kullanabiliriz.
- **Pozisyon < baslangic zorunlulugu:** "Not, oylama baslangicinda mevcuttu" kisitlamasi
  ZK-devresi icinde. Bu, ayni bloktaki "quick-delegate-then-vote" saldirilarini tamamen onler.
- **Native check + circuit check ayrimi:** Test'te once kisitlamalar native kontrol edilir,
  sonra devre test edilir. Bu, hata ayiklamayi kolaylastirir.
- **Baslangic pozisyonu = epoch bas:** Proposal'in her zaman bir epoch basinda baslatilmasi
  gereksinimi, baslangic pozisyonunu belirleyici hale getirir.

---

## [delegator_vote/view.rs]
**Dosya:** `.../governance/src/delegator_vote/view.rs`
**Satir Sayisi:** 85

### Genel Amac

`DelegatorVoteView`: Delegator oyunun goruntulenme versiyonu. Kullanici kendi oyunu
tamamen gorebilirken (Visible), baskalarinin oylari sadece DelegatorVote struct'i icerir.

### Struct/Enum/Type Listesi

#### `pub enum DelegatorVoteView` (Satir 10-18)
```rust
pub enum DelegatorVoteView {
    Visible {
        delegator_vote: DelegatorVote,
        note: NoteView,  // Not'un icerigi de gorunur
    },
    Opaque {
        delegator_vote: DelegatorVote,
        // Note gizli
    },
}
```

### Tasarim Kararlari

1. **View pattern:** Ayni struct'in farkli taraflara farkli gorunum saglamasi. Kullanici kendi
   `note`'unu (ve dolayli olarak oylama gucunu) gorur; diger kullanicilara sadece DelegatorVote.
2. **`#[allow(clippy::large_enum_variant)]`:** `Visible` varyanti buyuk; kucuk `Opaque` kadar
   bellek israf eder. Clippy uyarisi supresyonu bilinçli bir tradeoff.

### Nova Shield Chain Icin Dersler

- **View layer pattern:** Kullanicilara sadece kendi gorebilecekleri verileri goster.
  Ayni on-chain veri icin birden fazla view tipi olusturmak, gizlilik ile kullanicililik
  arasindaki dengeyi saglar.

---

## [event.rs]
**Dosya:** `.../governance/src/event.rs`
**Satir Sayisi:** 68

### Genel Amac

Governance olaylarini (events) protobuf mesajlarina donusturan fonksiyonlari tanimlar.

### Fonksiyonlar

- `delegator_vote(vote, validator_identity_key) -> pb::EventDelegatorVote`
- `proposal_deposit_claim(deposit_claim) -> pb::EventProposalDepositClaim`
- `validator_vote(vote, voting_power) -> pb::EventValidatorVote` — oylama gucu de dahil
- `proposal_withdraw(withdraw) -> pb::EventProposalWithdraw`
- `proposal_submit(submit, start_height, end_height) -> pb::EventProposalSubmit`
- `proposal_passed(proposal) -> pb::EventProposalPassed`
- `proposal_failed(proposal) -> pb::EventProposalFailed`
- `proposal_slashed(proposal) -> pb::EventProposalSlashed`

### Tasarim Kararlari

1. **validator_vote'de voting_power eklenmesi:** On-chain event'e oylama gucu dahil.
   Bu, indexer'larin tally'i yeniden hesaplamasini kolaylastirir.
2. **8 farkli event tipi:** Governance yasam dongusu boyunca granul event coverage.

### Nova Shield Chain Icin Dersler

- **Granul governance events:** Her governance eylemi icin ayri event tipi tanimla.
  Indexer'lar ve UI'lar bu event'leri filtreleyerek ilgili bilgilere ulasabilir.
- **voting_power event'e dahil etme:** Oylama gucunu event'e koymak, stateless tally
  hesaplamasini dis sistemler icin kolaylastirir.

---

## [genesis.rs]
**Dosya:** `.../governance/src/genesis.rs`
**Satir Sayisi:** 37

### Genel Amac

Governance component'inin genesis yapilandirmasi.

### Struct/Enum/Type Listesi

#### `pub struct Content` (Satir 9-12)
```rust
pub struct Content {
    pub governance_params: GovernanceParameters,
}
```
`Default` derive: `GovernanceParameters::default()` kullaniyor.

### Nova Shield Chain Icin Dersler

- Genesis yapilandirmasi minimal tutulmali: sadece parametreler. Ilk durum
  `init_chain` cagrisinda bu parametrelerden kurulur.

---

## [lib.rs]
**Dosya:** `.../governance/src/lib.rs`
**Satir Sayisi:** 62

### Genel Amac

Public API yuzeyini tanimlar. Feature flag'lere (`component`) gore conditional export.

### Ozellikler

- `#![deny(clippy::unwrap_used)]` — Tum crate'te `unwrap()` yasak. Bu ciddi bir Rust guvenlik kurali.
- `#![recursion_limit = "512"]` — ZK devre yapisinda derin recursiyon olabiliyor.
- `#[cfg(feature = "component")]` ile runtime component kodlari kosullu export edilir.

### Nova Shield Chain Icin Dersler

- `#![deny(clippy::unwrap_used)]` uretim kodunda zorunlu kilmak: Nova Shield'de de bu
  lint kuralini zorunlu hale getir. Her `unwrap` potansiyel bir panic noktasidir.

---

## [metrics.rs]
**Dosya:** `.../governance/src/metrics.rs`
**Satir Sayisi:** 31

### Genel Amac

Metrics altyapisi. Simdilik bos (hic metrik kayitli degil).

### Tasarim Kararlari

1. **Shadow import pattern:** `use crate::metrics;` ile standart `metrics` crate'ini golgeler.
   Bu, metrik fonksiyonlarinin prefix olmadan kullanilmasini saglar.
2. **Bos `register_metrics`:** Metrikler henuz tanimlanmamis. Yorum satirlari ornegi gosteriyor.

### Nova Shield Chain Icin Dersler

- Metrik kaydi icin bu shadow pattern benimsenmeli.
- Governance icin izlenmesi gerekenler: oneri sayisi, oy katilim orani, tally sonuclari.

---

## [params.rs]
**Dosya:** `.../governance/src/params.rs`
**Satir Sayisi:** 84

### Genel Amac

`GovernanceParameters` struct'i. Tum yonetisim kurallarini tanimlar.

### Struct/Enum/Type Listesi

#### `pub struct GovernanceParameters` (Satir 14-26)
```rust
pub struct GovernanceParameters {
    pub proposal_voting_blocks: u64,    // Oylama periyodu (blok)
    pub proposal_deposit_amount: Amount, // Oneri gonderim depositi
    pub proposal_valid_quorum: Ratio,   // Minimum katilim orani (gecerli nisap)
    pub proposal_pass_threshold: Ratio, // Gecme esigi (yes/no+yes)
    pub proposal_slash_threshold: Ratio, // Slash esigi (no/toplam)
}
```

### Default Degerleri (Satir 71-83)

```rust
proposal_voting_blocks: 17_280    // 24 saat, 5 saniyeli blok
proposal_deposit_amount: 10_000_000 // 10 PENUMBRA
proposal_valid_quorum: Ratio::new(40, 100)   // %40 katilim gerekli
proposal_pass_threshold: Ratio::new(50, 100) // >%50 yes (no+yes icinde)
proposal_slash_threshold: Ratio::new(80, 100) // no > %80 toplam -> slash
```

Yorum: "governance parameters copied from cosmos hub"

### Nova Shield Chain Icin Dersler

- **Cosmos Hub kaynaklı parametreler:** Deneyimli bir zincirden kanıtlanmış değerler.
  Baslangic icin makul. Nova Shield'de: blok suresi 5s ise 17280 blok = 24 saat.
- **Ratio tipi:** `u64` pay/payda ile kesin oran karsilastirmasi. Floating point kullanma.

---

## [proposal.rs]
**Dosya:** `.../governance/src/proposal.rs`
**Satir Sayisi:** 435

### Genel Amac

Governance proposal yapisi, tipleri ve payload'larini tanimlar.

### Struct/Enum/Type Listesi

#### `pub struct Proposal` (Satir 13-25)
```rust
pub struct Proposal {
    pub id: u64,
    pub title: String,       // max 80 char
    pub description: String, // max 10_000 char
    pub payload: ProposalPayload,
}
```

#### `pub enum ProposalKind` (Satir 213-228)
- `Signaling` — koordinasyon amaçli
- `Emergency` — ivedi geçme, anlik uygulama
- `ParameterChange` — uygulama parametresi degisikligi
- `CommunityPoolSpend` — topluluk havuzundan harcama
- `UpgradePlan` — zincir yukseltme
- `FreezeIbcClient` — IBC istemci dondurma
- `UnfreezeIbcClient` — IBC istemci serbest birakma

#### `pub enum ProposalPayload` (Satir 294-334)
```rust
pub enum ProposalPayload {
    Signaling { commit: Option<String> },         // Opsiyonel commit hash
    Emergency { halt_chain: bool },               // Zincir durdurmak icin
    ParameterChange(ParameterChange),             // Parametre degisikligi
    CommunityPoolSpend { transaction_plan: Vec<u8> }, // Havuzdan tx plani
    UpgradePlan { height: u64 },                  // Yukseltme yuksekligi
    FreezeIbcClient { client_id: String },        // max 128 byte client ID
    UnfreezeIbcClient { client_id: String },      // max 128 byte client ID
}
```

#### `pub enum ProposalPayloadToml` (Satir 339-347)
TOML/JSON serializable insan okunabilir versiyon. `CommunityPoolSpend` base64 string kullanir.

### Validasyon (TryFrom, Satir 82-168)

- `title.len() > 80` -> bail!
- `description.len() > 10_000` -> bail!
- `signaling.commit.len() > 255` -> bail!
- IBC client_id: `len() > 128` -> bail! + `ClientId::from_str` validation

### Fonksiyonlar

#### `ProposalPayload` metodlari
- `is_signaling()`, `is_emergency()`, `is_ibc_freeze()`, `is_parameter_change()`, `is_community_pool_spend()`

### Nova Shield Chain Icin Dersler

- **7 proposal tipi:** Nova Shield governance'i en az bu 7 tipi desteklemeli. Ozellikle
  `UpgradePlan` ve `Emergency` kritik zincir operasyonlari icin.
- **IBC client yonetimi:** Governance ile IBC client dondurma/serbest birakma — interoperabilite
  guvenlik mekanizmasi.
- **Long description limit:** 10.000 karakter yeterli. Nova Shield icin benzer.

---

## [proposal_deposit_claim.rs + proposal_deposit_claim/action.rs]
**Dosya:** `.../governance/src/proposal_deposit_claim/action.rs`
**Satir Sayisi:** 135

### Genel Amac

`ProposalDepositClaim` action'i: Biten bir proposal'in deposinini geri almak.

### Struct/Enum/Type Listesi

#### `pub struct ProposalDepositClaim` (Satir 21-28)
```rust
pub struct ProposalDepositClaim {
    pub proposal: u64,
    pub deposit_amount: Amount,
    pub outcome: Outcome<()>,   // () = neden bilgisi yok (sadece durum)
}
```

### Fonksiyonlar

#### `balance(&self) -> Balance` (Satir 70-133)

NFT durum gecirisini modelleyen denge hesabi:

```
Passed, Failed{No}, Slashed{No}    -> deposit NFT yakilir
Failed{Withdrawn}, Slashed{Withdrawn} -> unbonding_deposit NFT yakilir
```
```
Passed -> passed NFT mint edilir + deposit iadesi
Failed  -> failed NFT mint edilir + deposit iadesi
Slashed -> slashed NFT mint edilir (DEPOSIT IADESI YOK!)
```

**Kritik satir (Satir 128-129):**
```rust
if self.outcome.should_be_refunded() {
    balance += Balance::from(deposit);
}
```
Slash durumunda `should_be_refunded()` false, deposit iade edilmez.

### Tasarim Kararlari

1. **Balance denklem mantigi:** NFT yakma/mint ve deposit iadesi, transaction balance
   denkleminde ifade edilir. Ayriyeten transfer logic gerekmez.
2. **Outcome<()>:** Claim'de neden bilgisi saklanmaz; sadece sonuc onemli.

---

## [proposal_nft.rs]
**Dosya:** `.../governance/src/proposal_nft.rs`
**Satir Sayisi:** 222

### Genel Amac

Proposal durumlarini temsil eden NFT'leri modelleyen `ProposalNft` struct'i.
Her proposal her durumda ayrı bir NFT denom'una sahiptir.

### Struct/Enum/Type Listesi

#### `pub enum Kind` (Satir 18-25)
```rust
pub enum Kind {
    Deposit,          // "proposal_{id}_deposit"
    UnbondingDeposit, // "proposal_{id}_unbonding_deposit"
    Slashed,          // "proposal_{id}_slashed"
    Failed,           // "proposal_{id}_failed"
    Passed,           // "proposal_{id}_passed"
}
```

#### `pub struct ProposalNft` (Satir 12-16)
```rust
pub struct ProposalNft {
    proposal_id: u64,
    proposal_state: Kind,
    base_denom: asset::Metadata,
}
```

### Fonksiyonlar

**Factory metodlar:**
- `deposit(proposal_id) -> Self`
- `unbonding_deposit(proposal_id) -> Self`
- `slashed(proposal_id) -> Self`
- `failed(proposal_id) -> Self`
- `passed(proposal_id) -> Self`

**Denom format:** `"proposal_{id}_{state}"` — REGISTRY'ye kayitli.

### Yayin/Parse

`TryFrom<asset::Metadata>` — Regex ile denom parse:
```
^proposal_(?P<data>(?P<proposal_id>[0-9]+)_(?P<proposal_state>deposit|unbonding_deposit|passed|failed|slashed))$
```
Named capture groups ile hem proposal_id hem state ayni anda parse edilir.

### Test'ler

`proposal_token_denomination_round_trip`: 5 farkli durum icin denom string round-trip.

### Nova Shield Chain Icin Dersler

- **NFT-tabanlı governance durumu:** Proposal durumunu NFT ile temsil etmek, durum
  gecislerini atomik ve kanıtlanabilir kilar. Nova Shield governance'inda bu yapi
  benimsenmeli. Durum = hangi NFT'ye sahipsin.

---

## [proposal_state.rs]
**Dosya:** `.../governance/src/proposal_state.rs`
**Satir Sayisi:** 407

### Genel Amac

Proposal durum makinesi: `State` ve `Outcome` tiplerini tanimlar.

### Struct/Enum/Type Listesi

#### `pub enum State` (Satir 9-14)
```rust
pub enum State {
    Voting,
    Withdrawn { reason: String },
    Finished { outcome: Outcome<String> },  // String = cekme nedeni
    Claimed { outcome: Outcome<String> },
}
```

Metodlar: `is_voting`, `is_withdrawn`, `is_finished`, `is_claimed`,
`is_passed`, `is_failed`, `is_slashed`, `withdrawn() -> Withdrawn<String>`.

#### `pub enum Outcome<W>` (Satir 142-146)
```rust
pub enum Outcome<W> {
    Passed,
    Failed { withdrawn: Withdrawn<W> },
    Slashed { withdrawn: Withdrawn<W> },
}
```
Generik `W` parametresi: `String` (neden) veya `()` (neden yok).

Metodlar:
- `should_be_refunded()` — slash degilse true
- `is_slashed`, `is_failed`, `is_passed`
- `as_ref()`, `map(f)` — standart functor operasyonlari

#### `pub enum Withdrawn<W>` (Satir 195-198)
```rust
pub enum Withdrawn<W> {
    No,
    WithReason { reason: W },
}
```
`From<Option<W>>` ve `From<Withdrawn<W>> for Option<W>` implementasyonlari.

### Tasarim Kararlari

1. **Generic Outcome:** `Outcome<String>` ve `Outcome<()>` farkli kullanim senaryolari
   icin. Claim aninda neden bilgisi gerekli degil (`()`); full proposal state'de gerekli (`String`).
2. **DomainType impls:** `Outcome<String>` ve `Outcome<()>` ikisi icin ayri protobuf donusumu.
   Ortak `pb::ProposalOutcome` protobuf tipi kullanilir.
3. **Withdrawn max length:** `MAX_VALIDATOR_VOTE_REASON_LENGTH` (1024) limiti uygulanir.

### Nova Shield Chain Icin Dersler

- **Genericized outcome tipi:** Claim ve tam durum icin ayri tipler ama ayni enum.
  Bu pattern benim gozumden kacabilirdi — dikkat et.

---

## [proposal_submit.rs + proposal_submit/action.rs]
**Dosya:** `.../governance/src/proposal_submit/action.rs`
**Satir Sayisi:** 88

### Genel Amac

`ProposalSubmit` action'i: Yeni bir governance proposal'i gondermek.

### Struct/Enum/Type Listesi

#### `pub struct ProposalSubmit` (Satir 17-22)
```rust
pub struct ProposalSubmit {
    pub proposal: Proposal,
    pub deposit_amount: Amount,
}
```

### Fonksiyonlar

#### `balance(&self) -> Balance` (Satir 32-40)
```rust
Balance::from(proposal_nft) - Balance::from(deposit)
```
- Proposal gonderimi deposit harcar (`-deposit` bakiye etkisi)
- Karsiliginda `deposit` NFT alir

Yorum: "Proposal submissions *require* the deposit amount in order to be accepted"
Deposit, transaction bakiyesinin negatif tarafi. Bu, deposit'in kullanicinin notundan
otomatik olarak alindigini garanti eder.

### Nova Shield Chain Icin Dersler

- **Balance denklem ile deposit:** Deposit gonderimi, balance denkleminde negatif olarak
  gosterilir. Ayri "escrow" logic yazmak gerekmez — sifir bilgi notlarla uyumlu.

---

## [proposal_withdraw.rs + proposal_withdraw/action.rs]
**Dosya:** `.../governance/src/proposal_withdraw/action.rs`
**Satir Sayisi:** 76

### Genel Amac

`ProposalWithdraw` action'i: Proposal geri cekme.

### Struct/Enum/Type Listesi

#### `pub struct ProposalWithdraw` (Satir 13-18)
```rust
pub struct ProposalWithdraw {
    pub proposal: u64,
    pub reason: String,
}
```

### Fonksiyonlar

#### `balance(&self) -> Balance` (Satir 37-43)
```rust
Balance::from(withdrawn_proposal_nft) - Balance::from(voting_proposal_nft)
```
- `deposit` NFT yakilir
- `unbonding_deposit` NFT mint edilir
Bu, cekme eyleminin NFT degisimini balance denkleminde ifade eder.

---

## [state_key.rs]
**Dosya:** `.../governance/src/state_key.rs`
**Satir Sayisi:** 142

### Genel Amac

Tum governance state key'lerini tanimlayan sabitleri/fonksiyonlari saglar.
State key = JMT veya nonverifiable store'daki veri anahtar yolu.

### Fonksiyonlar (Ozet)

| Fonksiyon | Key Pattern |
|---|---|
| `governance_params()` | `"governance/params"` |
| `next_proposal_id()` | `"governance/next_proposal_id"` |
| `proposal_definition(id)` | `"governance/proposal/{id:020}/data"` |
| `proposal_state(id)` | `"governance/proposal/{id:020}/state"` |
| `proposal_deposit_amount(id)` | `"governance/proposal/{id:020}/deposit_amount"` |
| `proposal_voting_start(id)` | `"governance/proposal/{id:020}/voting_start"` |
| `proposal_voting_end(id)` | `"governance/proposal/{id:020}/voting_end"` |
| `proposal_voting_start_position(id)` | `"governance/proposal/{id:020}/voting_start_position"` |
| `unfinished_proposal(id)` | `"governance/unfinished_proposals/{id:020}"` |
| `all_unfinished_proposals()` | `"governance/unfinished_proposals/"` |
| `voted_nullifier_lookup_for_proposal(id, nullifier)` | `"governance/proposal/{id:020}/voted_nullifiers/{nullifier}"` |
| `rate_data_at_proposal_start(id, ik)` | `"governance/proposal/{id:020}/rate_data_at_start/{ik}"` |
| `voting_power_at_proposal_start(id, ik)` | `"governance/proposal/{id:020}/voting_power_at_start/{ik}"` |
| `validator_vote(id, ik)` | `"governance/validator_vote/{id:020}/{ik}"` |
| `tallied_delegator_votes(id, ik)` | `"governance/tallied_delegator_votes/{id:020}/{ik}"` |
| `untallied_delegator_vote(id, ik, nullifier)` | `"governance/untallied_delegator_vote/{id:020}/{ik}/{nullifier}"` |
| `deliver_single_community_pool_transaction_at_height(h, id)` | `"governance/deliver_community_pool_transactions/{h}/{id:020}"` |
| `param_changes_for_height(h)` | `"governance/param_changes/{h}/"` |
| `upgrades::next_upgrade()` | `"governance/upgrades/next_upgrade"` |
| `persistent_flags::halt_bit()` | `"governance/persistent_flags/halt_bit"` |

### Tasarim Kararlari

1. **`{id:020}` padding:** 20 haneli sifir dolgu ile leksikografik siralama, sayisal siralamaya esit.
   Prefix sorgu sirasinda proposal'lar dogal sirayla gelir.
2. **Prefix/tekil cift pattern:** Her veri tipi icin hem tekil key hem prefix key fonksiyonu var.
   `unfinished_proposal(id)` ve `all_unfinished_proposals()` — birbirleriyle tutarli olmali (yorum var).
3. **Submodul organizasyonu:** `upgrades` ve `persistent_flags` submodul olarak organize edilmis.

### Nova Shield Chain Icin Dersler

- **20-haneli sifir dolgu:** State key tasariminda leksikografik = sayisal olmasini saglamak
  prefix tabanlı sorgular icin kritik. Nova Shield'de tum numerik ID'ler sabit genislikli
  olarak encode edilmeli.
- **Prefix/tekil cift:** Her state key icin hem tekil hem all-prefix versiyonu olustur.
  Bu, stream-based tarama ile tekil okuma arasinda tutarlilik saglar.

---

## [tally.rs]
**Dosya:** `.../governance/src/tally.rs`
**Satir Sayisi:** 283

### Genel Amac

`Tally` (oy sayimi) ve `Ratio` (oran) tiplerini tanimlar. Governance oylamasi sonuc
hesaplama mantigi buradadir.

### Struct/Enum/Type Listesi

#### `pub struct Tally` (Satir 19-23)
```rust
pub struct Tally {
    yes: u64,
    no: u64,
    abstain: u64,
}
```
`Default`, `Add`, `AddAssign` implementasyonlari.
- `total() -> u64` = yes + no + abstain

`From<(Vote, u64)>` ve `From<(u64, Vote)>` ile ikili donusum.

#### `pub enum Outcome` (Satir 106-110)
```rust
pub enum Outcome {
    Pass,
    Fail,
    Slash,
}
```
`From<Outcome> for StateOutcome<T>` implementasyonu.

#### `pub struct Ratio` (Satir 199-203)
```rust
pub struct Ratio {
    numerator: u64,
    denominator: u64,
}
```
**Karsilastirma (Satir 252-263):**
```rust
impl Ord for Ratio {
    fn cmp(&self, other: &Self) -> Ordering {
        (u128::from(self.numerator) * u128::from(other.denominator))
            .cmp(&(u128::from(self.denominator) * u128::from(other.numerator)))
    }
}
```
Cross-multiply karsilastirma, floating point olmadan. `u128` ile overflow onlenmis:
maksimum `u64::MAX * u64::MAX = u128` icinde.

**Parse (Satir 210-231):** `"40/100"` gibi string'den parse.
**Display (Satir 204-208):** `"{numerator}/{denominator}"` formatinda yazilir.

### Fonksiyonlar

#### `Tally::meets_quorum(&self, total_voting_power, params) -> bool` (Satir 141-143)
```rust
Ratio::new(self.total(), total_voting_power) >= params.proposal_valid_quorum
```
Oy katilim orani >= gecerli nisap.

#### `Tally::slashed(&self, params) -> bool` (Satir 145-147)
```rust
Ratio::new(self.no, self.total()) > params.proposal_slash_threshold
```
No oylarinin toplam icindeki orani > slash esigi.

#### `Tally::yes_ratio(&self) -> Ratio` (Satir 149-153)
```rust
Ratio::new(self.yes, (self.yes + self.no).min(1))
```
`yes / (yes + no)` — abstain dahil degil. `.min(1)` -> sadece abstain varsa 0/1 = asla gecmez.

#### `Tally::outcome(self, total_voting_power, params) -> Outcome` (Satir 156-175)

**Ana tally algoritması:**
```
Adim 1: quorum saglaniyor mu? -> Hayir: FAIL
Adim 2: slash kosulu saglaniyor mu? -> Evet: SLASH
Adim 3: yes_ratio > pass_threshold -> Evet: PASS, Hayir: FAIL
```

Sira onemli:
1. Once quorum kontrolu — katilim yoksa her seyi reddet
2. Sonra slash — manipulatif oneriler cezalandirilir
3. Son olarak pass — kalan oylar degerlendirililir

#### `Tally::emergency_pass(self, total_voting_power, params) -> bool` (Satir 177-192)

Emergency geçme kosulu:
```rust
Ratio::new(self.yes, total_voting_power) > Ratio::new(1, 3)
```
Toplam voting power'in 1/3'unden fazlasi YES oy kullandi mi?
Quorum ve slash kontrolu da yapilir.

### Matematiksel Ozetleme

**Normal gecme:**
```
quorum: sum(all_votes) / total_power >= 40/100
not_slash: no_votes / sum(all_votes) <= 80/100
pass: yes_votes / (yes + no) > 50/100
```

**Emergency gecme:**
```
yes_votes / total_power > 1/3
quorum ve slash koullari da saglaniyor olmali
```

**Slash:**
```
no_votes / sum(all_votes) > 80/100
```
Bu olursa, quorum saglanmissa, depozit iade edilmez.

### Tasarim Kararlari

1. **Sadece karsilastirma, aritmetik yok:** `Ratio` toplama/cikarma implementasyonu yok.
   Bu "minimizes the trusted codebase for governance" - governance parametrelerine yapilmis
   saldiri yuzeyini en aza indirir.
2. **u128 cross-multiply:** `u64 * u64` overflow yapar. u128 kullanimi overflow'u onler.
3. **yes_ratio hesabinda `.min(1)`:** Sifira bolunme korunmasi. Tamamen abstain oy = asla gecmez.
4. **Quorum kontrolu once slash'ten once:** Katilim yoksa slash da yapmak anlamsiz.

### Nova Shield Chain Icin Dersler

- **Ratio cross-multiply:** Floating point kullanmadan oran karsilastirmasi. Ayni pattern
  Nova Shield governance tally'sinde zorunlu.
- **Governance oncesi quorum kontrolu:** Quorum saglanmazsa teklif reddedilmeli.
  Katilim yoksa slash da yanlis olur.
- **Abstain semantigi:** Abstain, yes/no oranini etkilemez ama quorum'u etkiler.
  Cok say abstain = quorum saglamamak mumkun.
- **1/3 emergency threshold:** Bu, Cosmos BFT'nin hata toleransindan geliyor. 1/3+ validator
  emergency gec derse, zaten %33+ onay anlamina geliyor — BFT guvenli.

---

## [validator_vote.rs + validator_vote/action.rs]
**Dosya:** `.../governance/src/validator_vote/action.rs`
**Satir Sayisi:** 153

### Genel Amac

Validator oyunu temsil eden tipler: `ValidatorVote`, `ValidatorVoteBody`, `ValidatorVoteReason`.

### Struct/Enum/Type Listesi

#### `pub struct ValidatorVote` (Satir 12-17)
```rust
pub struct ValidatorVote {
    pub body: ValidatorVoteBody,
    pub auth_sig: Signature<SpendAuth>, // GOVERNANCE key ile imzalanmis
}
```

#### `pub struct ValidatorVoteBody` (Satir 66-78)
```rust
pub struct ValidatorVoteBody {
    pub proposal: u64,
    pub vote: Vote,
    pub identity_key: IdentityKey,    // Validator kimligi
    pub governance_key: GovernanceKey, // Oy icin kullanilan governance key
    pub reason: ValidatorVoteReason,  // max 1 KB aciklama
}
```

#### `pub struct ValidatorVoteReason(pub String)` (Satir 129)
- `MAX_VALIDATOR_VOTE_REASON_LENGTH` = 1024 byte limit

#### `pub const MAX_VALIDATOR_VOTE_REASON_LENGTH: usize = 1024` (validator_vote.rs Satir 6)

### Tasarim Kararlari

1. **Governance key vs Identity key:** `ValidatorVoteBody` ikisini birden icerir:
   - `identity_key`: Validator'in kimlik anahtari (stake, staking icin)
   - `governance_key`: Sadece yonetisim icin kullanilan ayri anahtar
   Imza governance_key ile atilir. Bu, stake key compromise edilse bile governance
   oylarinin guvenligini korur (ve tam tersi).
2. **1 KB reason limit:** Uzun "oyumu justifiye etme" metinleri state'i sistmatik olarak
   sisiriyor olurdu. 1 KB makul bir sinir.

### Nova Shield Chain Icin Dersler

- **Governance key ayrimi:** Validator'larin governance icin ayri key kullanmasi
  production deployment'ta zorunlu guvenlik gereksinimidir.
- **Gerekce siniri:** Validator oy gerekceleri icin 1 KB limit benimsenmeli.

---

## [vote.rs]
**Dosya:** `.../governance/src/vote.rs`
**Satir Sayisi:** 100

### Genel Amac

`Vote` enum'u: oy tipleri.

### Struct/Enum/Type Listesi

#### `pub enum Vote` (Satir 14-21)
```rust
pub enum Vote {
    Yes,
    No,
    Abstain,
}
```
- `proptest_derive::Arbitrary` ile property-based testing destegi (`#[cfg_attr(test, ...)]`)
- `FromStr`: "yes" | "y", "no" | "n", "abstain" | "a"
- `Display`: "yes", "no", "abstain"
- `From<Vote>` for `pb::Vote`: const fn ile protobuf donusumu

### Test'ler

`vote_roundtrip_serialize`: proptest ile rastgele Vote degerlerinin protobuf round-trip.

### Nova Shield Chain Icin Dersler

- **Kisa takma adlar:** "y", "n", "a" gibi kisa oy takma adlari CLI kullaniminda pratik.
- **proptest Arbitrary:** Tip-guvenli property testing icin `proptest_derive::Arbitrary`
  derive etmek test yazimini kolaylastirir.

---

## [voting_receipt_token.rs]
**Dosya:** `.../governance/src/voting_receipt_token.rs`
**Satir Sayisi:** 135

### Genel Amac

`VotingReceiptToken`: Delegator'in belirli bir proposal'da oy kullandigini kanitlayan token.

### Struct/Enum/Type Listesi

#### `pub struct VotingReceiptToken` (Satir 10-13)
```rust
pub struct VotingReceiptToken {
    proposal_id: u64,
    base_denom: asset::Metadata,
}
```
**Denom format:** `"uvoted_on_{proposal_id}"` (prefix `u` = micro birim)

### Fonksiyonlar

- `new(proposal_id) -> Self`: REGISTRY'de `"uvoted_on_{id}"` olarak kaydeder
- `denom(), default_unit(), id(), proposal_id()`: accessor'lar

`TryFrom<asset::Metadata>`:
Regex: `^uvoted_on_(?P<data>(?P<proposal_id>[0-9]+))$`

### Test'ler

`unbonding_token_denomination_round_trip`: denom -> token -> denom round-trip.

### Tasarim Kararlari

1. **Receipt token olarak oy kaniti:** `uvoted_on_{proposal_id}` tokenini tutmak,
   o proposal icin oy kullandiginin tasinabilir kaniti. Bu, oy sertifikalari icin kullanilabilir.
2. **Denom prefix "u":** Micro birim gosterimi. 1 `uvoted_on_1` = 1 mikro birim oy makbuzu.

### Nova Shield Chain Icin Dersler

- **Voting receipt token:** Oy kullanma kaniti olarak token almak, on-chain protokollerin
  "katilimi ispatla" gereksinimlerini karsilar. Nova Shield'de benzer receipt mekanizmasi.

---

## Genel Mimari Degerlendirme

### Penumbra Governance'inin Gucu

1. **ZK-Proof tabanlı gizlilik:** Delegator'lar, kim oldugunu aciklamadan oy kullanir.
   Sadece nullifier ve oylama gucu acik.

2. **Validator override mekanizmasi:**
   ```
   effective_validator_power = power_at_start - delegator_voted_power
   ```
   Delegator'lar validator'i override edebilir. Bu, daha yaygin katilimi tesvik eder.

3. **NFT tabanlı durum makinesi:** Proposal durumu NFT mint/burn ile izlenir.
   Bu, durum gecislerini atomik ve zincirde kanıtlanabilir kilar.

4. **Two-phase delegator tally:** Oylar aninda aggrege edilmez; epoch'ta toplu toplanir.
   Her oy transaction'i O(1) yazma maliyet.

5. **Precondition tabanlı parametre degisimi:** "Mevcut deger X ise Y'ye degistir" semantigi.
   Bu, paralel tekliflerin guvenli conflict cozumunu saglar.

6. **Governance key ayrimi:** Validator'lar stake key'den ayri governance key kullanir.
   Ayri risk profili, ayri guvensiz ortamda saklama.

7. **Emergency anlik enactment:** 1/3 validator (+quorum) ile emergency teklifleri
   oylama periyodu bitmeden aninda uygulanir.

### Potansiyel Zayifliklar

1. **IBC freeze proposal'da bilerek yanli key (validator_vote.rs Satir 115):**
   ```rust
   .map_err(|e| tonic::Status::aborted(format!("invalid client id: {e}")))
   ```
   Bu, `governance` modülü icinde `tonic::Status` kullanimi. Governance core icinde
   RPC katman referansi olmali; bu bir katmanlama ihlali.

2. **Parametre cakismasi (view.rs Satir 898-901):**
   "Note: if two parameter changes are scheduled for the same height by two passing proposals
   that were submitted in exactly the same block, one will clobber the other."
   Bu bilinen bir bug. Iki ayni bloktaki parametre degisikligi birbirini ezebilir.

3. **State key parse ile brittle (rpc.rs):**
   `key.rsplit('/').next()` ile identity_key parse etmek kırılgandır.
   State key formati degisirse silent bug olusar.

4. **`expect` kullanimi (view.rs Satir 128-129):**
   ```rust
   .expect("randomizer malformed")
   .expect("proof_blinding_r malformed")
   ```
   Plan parse sirasinda `expect` kullanimi. `#![deny(clippy::unwrap_used)]` bunu yakalamali
   ama `expect` unwrap_used lint'ine takilmiyor.

---

## Nova Shield Chain Icin DEX Uygulama Notu

(Governance modulu dogrudan DEX ile ilgili degil, ama DEX yonetimi icin kritik.)

### Governance ve DEX Entegrasyonu

**DEX parametreleri governance ile yonetilmeli:**
- Varsayilan testnet JSON'u (change.rs Satir 253-279):
  ```json
  "dexParams": {
    "isEnabled": true,
    "fixedCandidates": [...7 asset...],
    "maxHops": 4,
    "maxPositionsPerPair": 10
  }
  ```
  Bu parametreler governance proposal'i ile degistirilebilir.

**Nova Shield DEX governance plani:**
1. `fee_tier_params` — DEX ücret kademeleri
2. `lp_incentive_params` — Likidite saglayici odulllendirmesi
3. `routing_params` — Rotalama algoritma parametreleri
4. `emergency_pause` — DEX durdurma komutu (Emergency proposal)

**Kritik eylem:** Nova Shield DEX'in parametre listesi, `ParameterChange` proposal tipiyle
yonetilmeli. Zincir konsey/multisig gibi merkezi kontrol yerine tamamen on-chain governance.

---

## Kritik Sayisal Degerler (Dogrulanmis)

| Deger | Kaynak |
|---|---|
| `proposal_voting_blocks = 17_280` | params.rs Satir 74 (24 saat @5s blok) |
| `proposal_deposit_amount = 10_000_000` | params.rs Satir 75 (10 PENUMBRA) |
| `proposal_valid_quorum = 40/100` | params.rs Satir 77 (%40 katilim) |
| `proposal_pass_threshold = 50/100` | params.rs Satir 78 (>%50 yes/(yes+no)) |
| `proposal_slash_threshold = 80/100` | params.rs Satir 80 (>%80 no/total) |
| `emergency_pass_threshold = 1/3` | tally.rs Satir 190 |
| `MAX_VALIDATOR_VOTE_REASON_LENGTH = 1024` | validator_vote.rs Satir 6 (1 KB) |
| `PROPOSAL_WITHDRAWAL_REASON_LIMIT = 80` | action_handler/withdraw.rs Satir 20 |
| `title.len() max = 80` | proposal.rs Satir 88 |
| `description.len() max = 10_000` | proposal.rs Satir 93 |
| `signaling.commit max = 255` | proposal.rs Satir 111 |
| `IBC client_id max = 128` | proposal.rs Satir 145 |
| `EncodedParameter key max = 64` | change.rs Satir 26 |
| `EncodedParameter value max = 2048` | change.rs Satir 31 |
| `EncodedParameter component max = 64` | change.rs Satir 36 |
| `GROTH16_PROOF_LENGTH_BYTES` | proof.rs (extern sabit) |
| State key ID padding: 020 (20 hane) | state_key.rs (tum ID fonksiyonlari) |

---

## Nova Shield Chain Icin Dersler — Ozet Tablosu

| Konu | Penumbra Yaklaşimi | Nova Shield Onerisi |
|---|---|---|
| Delegator gizliligi | Groth16 ZK-proof + nullifier | Ayni pattern, arkworks Groth16 |
| Validator oy | Governance key ile acik imza | Governance key zorunlu |
| Parametre degisimi | JSON patch + precondition | Ayni, fixed-point degerler icin |
| Tally mekanizmasi | Delegator override + epoch aggregation | Ayni, u128 cross-multiply |
| Emergency geçme | 1/3 voting power threshold | Ayni |
| NFT durum makinesi | 5 durum NFT | Benimse, daha az durum da olabilir |
| Deposit slash | Spam onlemek icin | Kesinlikle benimse |
| Two-phase tally | Epoch'ta aggregation | Kesinlikle benimse |
| State key tasarimi | 20-haneli padding, prefix/tekil cift | Kesinlikle benimse |
| Halt/upgrade mekanizmasi | Nonverifiable halt_bit + signal_upgrade | Ayni |
| Governance key ayrimi | Stake'ten bağımsız governance key | Zorunlu |
