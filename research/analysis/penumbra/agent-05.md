# Penumbra SDK Analizi — Agent 05
## Kapsam: core/app-tests — Entegrasyon Test Suite (30 dosya)
## Analiz Tarihi: 2026-07-22

---

## Genel Bakış

`penumbra-sdk-app-tests` crate'i, Penumbra uygulamasının end-to-end entegrasyon test altyapısını içerir. Bu crate doğrudan üretim kodu içermez; bunun yerine tüm sistem bileşenlerini birbirine bağlayan test senaryoları, mock consensus motoru, IBC relayer simülasyonu ve DEX VCB (Value Circuit Breaker) doğrulamalarını sağlar. Test paternleri, sistemin doğru davrandığı sınır koşulları hakkında kritik bilgiler ortaya koyar.

**Önemli Özellikler:**
- Mock Tendermint/CometBFT implementasyonu ile gerçek consensus akışı test edilir
- ZK spend proof'ları gerçek `arkworks` devresiyle üretilip doğrulanır
- DEX swap → end-of-block execution → swap claim zinciri uçtan uca test edilir
- IBC: tam el sıkışma (CreateClient → OpenInit → OpenTry → OpenAck → OpenConfirm → ChanOpen* → Transfer)
- Value Circuit Breaker (VCB) bütünlük testleri regression testleri içerir

---

## Dosya Analizleri

---

## [lib.rs]
**Dosya:** `/research/chains/penumbra/crates/core/app-tests/src/lib.rs`
**Satır Sayısı:** 3

### Genel Amaç
Crate giriş noktası. Tek satır belge yorumuyla crate'in amacını tanımlar: "workspace-wide test logic for the Penumbra app components." İçi boş — saf konfigürasyon crate'i.

### Sistem Davranış Notları
Penumbra test mimarisi, tüm integration test'leri tek bir crate'te toplar. Bu, cross-crate bağımlılıkların düzgünce test edilmesini sağlar.

### Nova Shield Chain İçin Dersler
- Integration test crate'ini production kodu crate'lerinden ayrı tutmak doğru mimari. Nova Shield için aynı pattern: `nova-shield-app-tests` adında bağımsız bir crate.
- Bu pattern sayesinde test bağımlılıkları (mock, test-subscriber vb.) üretim binary'sine karışmaz.

---

## [app_blocktimes_increment.rs]
**Dosya:** `.../tests/app_blocktimes_increment.rs`
**Satır Sayısı:** 99

### Genel Amaç
Mock consensus motorunun blok zamanlarını doğru şekilde artırıp artırmadığını test eder. İki farklı senaryoyu kapsar: varsayılan 5s artış ve özelleştirilmiş 13s artış.

### Helper Fonksiyonlar / Struct'lar
- `common::set_tracing_subscriber()` — test tracing subscriber kurulumu
- `TestNode::builder().single_validator()` — tek validatörlü node oluşturucu
- `.with_penumbra_auto_app_state(app_state)` — validatör anahtarlarını genesis state'e enjekte eder
- `.with_initial_timestamp(start_time)` — sabit başlangıç zamanı
- `.ts_callback(move |t| t.checked_add(block_duration).unwrap())` — özelleştirilmiş zaman artış callback'i

### Test Senaryoları

#### `mock_tendermint_block_times_correct`
**Setup:**
- Sabit başlangıç zamanı: `"2022-02-11T17:30:50.425417198Z"`
- `TempStorage::new()` ile geçici depolama
- Tek validatör, varsayılan genesis state

**Senaryo 1 — Varsayılan 5s artış:**
```
for i in 0..10:
    node.block().execute().await
    assert timestamp == start_time + 5s * (i+1)
```

**Senaryo 2 — Özelleştirilmiş 13s artış:**
```
ts_callback = |t| t.checked_add(Duration::from_secs(13))
for i in 0..10:
    node.block().execute().await
    assert timestamp == start_time + 13s * (i+1)
```

**Assertion'lar:**
- `*test_node.timestamp() == start_time` — ilk bloktan önce zaman değişmez
- Her blok sonrası zaman artışı tam ve deterministik olmalı

**İnvaryantlar:**
- Blok zamanı monoton artar
- `checked_add` overflow güvenlidir

### Sistem Davranış Notları
- Default blok süresi: **5 saniye**
- `ts_callback` kapanışı (`move`) blok süresini esnek biçimde tanımlamaya izin verir
- Test başlangıcında ve bitişinde `drop(test_node)`, `drop(storage)`, `drop(guard)` çağrısı — belirleyici kaynak temizleme

### Nova Shield Chain İçin Dersler
- Mock consensus motorunda zaman yönetimi için callback pattern ideal. Nova Shield'ın kendi mock consensus implementasyonunda da `ts_callback` tarzı bir mekanizma kullanılmalı.
- Sabit başlangıç zamanı deterministik test çıktısı sağlar — testler her çalışmada aynı durumu üretmeli.
- Default blok süresi olarak 5s seçilmiş. Nova Shield için bu parametre genesis'te konfigüre edilebilir olmalı.

---

## [app_can_define_and_delegate_to_a_validator.rs]
**Dosya:** `.../tests/app_can_define_and_delegate_to_a_validator.rs`
**Satır Sayısı:** 530

### Genel Amaç
Yeni validatör tanımlama, delegasyon ve delegasyonun geri alınması (undelegation) süreçlerini uçtan uca test eder. Epoch geçişlerinin validatör durumunu nasıl etkilediğini doğrular.

### Sabitler
- `EPOCH_DURATION: u64 = 8` — test için kısaltılmış epoch süresi

### Test Senaryoları

#### `app_can_define_and_delegate_to_a_validator`
**Setup:**
- `TempStorage::new_with_penumbra_prefixes()` — Penumbra prefix'leriyle depolama
- Genesis'ten tek validatör
- EPOCH_DURATION=8 blok

**Adım 1 — İlk epoch'a ileri sarma:**
```
node.fast_forward_to_next_epoch(&storage).await
assert snapshot_start.get_current_epoch().index == 0
assert snapshot_end.get_current_epoch().index == 1
```

**Adım 2 — Mevcut validatörü doğrulama:**
```
assert existing_validator_state == State::Active  (başlangıçta ve sonunda)
assert consensus_set == [existing_validator_id]
```

**Adım 3 — Yeni validatör oluşturma:**
```rust
let new_validator_id_sk = SigningKey::<SpendAuth>::new(OsRng);
let new_validator_consensus_sk = ed25519_consensus::SigningKey::new(OsRng);
// auth_sig = new_validator_id_sk.sign(OsRng, &bytes)
// ValidatorDefinition{validator, auth_sig} tx gönder
```

**Adım 4 — Validatör tanımından hemen sonra:**
```
assert new_validator_state == State::Defined  (Active değil)
assert consensus_set.len() == 1  (yeni validatör henüz consensus'ta değil)
assert existing_rate != new_rate  (farklı zamanlarda oluşturulan validatörler farklı rate'e sahip)
```

**Adım 5 — Delegasyon tx'i:**
```
rate = get_validator_rate(new_validator_id)
delegate = rate.build_delegate(current_epoch, note.amount())
// SpendPlan + OutputPlan + delegate tx gönder
```

**Adım 6 — Delegasyondan hemen sonra:**
```
assert new_validator_state == State::Defined  (hala active değil — epoch bekleniyor)
assert pre_delegate_power == post_delegate_power  (güç hemen değişmez)
```

**Adım 7 — Sonraki epoch'a ileri sarma:**
```
node.fast_forward_to_next_epoch(&storage).await
assert new_validator_state == State::Active
assert consensus_set.len() == 2
assert new_validator_power > original_power
```

**Adım 8 — Undelegasyon tx'i:**
```
undelegate = rate.build_undelegate(current_epoch, note.amount())
```

**Adım 9 — Undelegasyondan hemen sonra:**
```
assert new_validator_state == State::Active  (hala active — epoch bekleniyor)
assert power == pre_undelegate_power  (güç hemen değişmez)
```

**Adım 10 — Sonraki epoch:**
```
assert new_validator_state == State::Defined  (active değil)
assert consensus_set.len() == 1
assert power == original_power  (eski güce geri döndü)
```

### Sistem Davranış Notları
- **Epoch geçişi kritik:** Delegasyon ve undelegasyon etkileri yalnızca epoch sınırında uygulanır
- **Oran farklılığı:** Farklı blok yüksekliklerinde oluşturulan validatörler farklı exchange rate'lere sahip
- **Durum makinesi:** `Defined → Active → Defined` döngüsü mümkün
- **Validatör identity key:** `decaf377_rdsa::SigningKey<SpendAuth>` — Penumbra'nın özel eğri üzerinde
- **Consensus key:** `ed25519_consensus::SigningKey` — CometBFT için

### Nova Shield Chain İçin Dersler
- Delegasyon/undelegasyon etkileri epoch sınırında uygulanmalı — anında değil. Bu, staking ekonomisini manipüle etmeyi zorlaştırır.
- Validatör tanımlama işlemi imzalı bir auth_sig içermeli: `identity_sk.sign(OsRng, &validator_bytes)`. İmzasız validatör tanımları reddedilmeli.
- `State::Defined` vs `State::Active` ayrımı kritik: tanımlı ama aktif olmayan validatörler consensus'a katılamaz.
- Validatör oranı (exchange rate) validatörün oluşturulduğu bloğa bağlı — farklı validatörler farklı zamanlarda farklı oranlara sahip olabilir.
- `keyring.insert(consensus_vk, consensus_sk)` — test ortamında keyring yönetimi, validatörlerin blok imzalamasına izin verir.

---

## [app_can_deposit_into_community_pool.rs]
**Dosya:** `.../tests/app_can_deposit_into_community_pool.rs`
**Satır Sayısı:** 135

### Genel Amaç
Community Pool'a (topluluk havuzu) not yatırabilme yeteneğini test eder.

### Test Senaryoları

#### `app_can_deposit_into_community_pool`
**Setup:**
- Tek validatör, varsayılan genesis
- MockClient ile test wallet sync

**Adımlar:**
1. Test wallet'tan bir note al
2. `CommunityPoolDeposit { value }` action'ı ile SpendPlan'ı birleştir
3. Tx'i çalıştır
4. Bakiyeleri karşılaştır

**Assertion'lar:**
```rust
// Yatırılan asset için:
assert pre_tx_amount + note.amount() == post_tx_amount

// Diğer asset'ler etkilenmemeli:
assert count_other_assets(pre_tx_balance) == count_other_assets(post_tx_balance)
```

**İnvaryantlar:**
- Deposit, yalnızca belirtilen asset'in community pool bakiyesini artırır
- Diğer asset bakiyeleri değişmez

### Sistem Davranış Notları
- Community Pool bakiye takibi `BTreeMap<asset::Id, Amount>` ile
- Deposit tx: `SpendPlan` + `CommunityPoolDeposit` action'ları birleşiyor
- `pre_tx_snapshot` ve `post_tx_snapshot` karşılaştırması için anlık görüntü alınıyor

### Nova Shield Chain İçin Dersler
- Community pool / treasury mekanizması Nova Shield için şarttır. Tokenomics'te %50 treasury planı ile uyumlu.
- Deposit tx'leri atomik olmalı: spend + deposit aynı tx'te gerçekleşir, her ikisi de ya başarılı ya da başarısız olur.
- `BTreeMap<asset::Id, Amount>` — farklı asset türlerini destekleyen treasury veri yapısı için ideal.

---

## [app_can_disable_community_pool_spends.rs]
**Dosya:** `.../tests/app_can_disable_community_pool_spends.rs`
**Satır Sayısı:** 393

### Genel Amaç
Community Pool harcama önerileri devre dışı bırakıldığında governance tekliflerinin reddedildiğini test eder.

### Sabitler
- `PROPOSAL_VOTING_BLOCKS: u64 = 3`

### Helper'lar
- `pool_balance()` — mevcut community pool bakiyesini döndüren closure
- `pending_pool_txs()` — bekleyen havuz tx'lerini döndüren closure

### Test Senaryoları

#### `app_can_disable_community_pool_spends`
**Genesis Konfigürasyonu:**
```rust
CommunityPoolParameters {
    community_pool_spend_proposals_enabled: false  // DEVRE DIŞI
}
governance_params:
    proposal_deposit_amount: 0
    proposal_voting_blocks: 3
```

**Adımlar:**
1. Genesis'te özel validatör oluştur (governance key dahil)
2. Community pool'a deposit tx
3. `ProposalPayload::CommunityPoolSpend` teklifi gönder
4. Validator oyu (`ValidatorVote::Yes`) gönder
5. 3 blok bekle (oy kullanma süresi)

**Assertion'lar:**
```rust
// Pool başlangıçta boş (staking token sıfır):
assert original_pool_balance[STAKING_TOKEN] == Amount::zero()

// Deposit sonrası:
assert post_deposit_pool_balance == {note.asset_id: note.amount(), STAKING_TOKEN: 0}

// Teklif anında reddedilmeli (spend_proposals_enabled=false):
assert post_proposal_state == None  // hemen reddedildi
assert post_proposal_pending_txs.len() == 0

// Oy da etkisiz:
assert post_vote_state == None
assert post_vote_pending_txs.len() == 0

// Oy kullanma süresi bittikten sonra:
assert post_voting_period_pool_balance == {note.asset_id: note.amount(), STAKING_TOKEN: 0}
// Havuz azalmadı — teklif uygulanmadı
```

### Sistem Davranış Notları
- `community_pool_spend_proposals_enabled: false` → proposal hemen reddedilir, `State` hiç `Voting`'e geçmez
- `pending_community_pool_transactions()` — onaylanan tekliflerin bekleyen tx'lerini sayar
- Governance key, validatörün spend key'inden türetilir (gerçek üretimde farklı olması tavsiye edilir)
- Validator oyu için `ValidatorVoteBody` + `governance_sk.sign()` gerekli

### Nova Shield Chain İçin Dersler
- Parametre ile governance özelliklerini açıp kapatabilme kritik. Devre dışı bırakılan teklifler `None` durumuyla anında reddedilmeli.
- `ValidatorVoteBody` yapısı: `proposal_id + vote + identity_key + governance_key + reason`. Governance key ayrı bir anahtar olmalı (soğuk depolama).
- `pending_community_pool_transactions()` — onaylanan ama henüz çalıştırılmamış tx'leri takip etmek için kritik state.

---

## [app_can_propose_community_pool_spends.rs]
**Dosya:** `.../tests/app_can_propose_community_pool_spends.rs`
**Satır Sayısı:** 400

### Genel Amaç
Community Pool harcama teklifinin başarıyla geçip pool'u azalttığını test eder (`spend_proposals_enabled: true` ile).

### Test Senaryoları

#### `app_can_propose_community_pool_spends`
`app_can_disable_community_pool_spends` ile aynı senaryo, ama `community_pool_spend_proposals_enabled: true`.

**Kritik Farklar:**

```rust
// Teklif gönderilince Voting durumuna girmeli:
assert post_proposal_state == Some(State::Voting)
assert post_proposal_pending_txs.len() == 0  // henüz beklemede değil

// Oy kullanılınca hala Voting:
assert post_vote_state == Some(State::Voting)

// Oy kullanma süresi bitince:
assert post_voting_period_state == Some(State::Finished {
    outcome: Outcome::Passed
})
assert post_voting_period_pending_txs.len() == 1  // artık tx beklemede

// Pool azaldı:
assert post_voting_period_pool_balance == {STAKING_TOKEN: 0}  // note geri çıktı

// +1 blok sonra tx çalıştı:
assert pending_pool_txs().len() == 0
```

### Sistem Davranış Notları
- **2 aşamalı uygulama:** Teklif geçer → tx beklemedeki listeye eklenir → sonraki blokta çalıştırılır
- Oy kullanma süresi bitmeden teklif sonuçlanmaz (3 blok beklemek gerekiyor)
- Oy salt çoğunluğu validator gücü üzerinden hesaplanıyor (tek validator = otomatik quorum)

### Nova Shield Chain İçin Dersler
- Governance tx'lerinin iki aşamalı uygulanması güvenlik için önemli: onay → bekleme süresi → uygulama. Ani uygulama, saldırgana reaction süresi bırakmaz.
- `ProposalPayload::CommunityPoolSpend { transaction_plan: bytes }` — governance teklifi içinde transaction plan'ı encode edilmiş bytes olarak saklamak esneklik sağlar.
- `proposal_nft_value()` — governance NFT pattern'i: teklif sahibi bir NFT alır, teklif sonuçlanınca bu NFT yakılabilir.

---

## [app_can_spend_notes_and_detect_outputs.rs]
**Dosya:** `.../tests/app_can_spend_notes_and_detect_outputs.rs`
**Satır Sayısı:** 132

### Genel Amaç
Shielded (gizli) notların harcanabileceğini ve çıktı notların tespit edilebildiğini doğrular. Nullifier mekanizmasını test eder.

### Test Senaryoları

#### `app_can_spend_notes_and_detect_outputs`
**Adımlar:**
1. Test wallet'tan bir input note al
2. `SpendPlan::new(OsRng, input_note, position)` — nullifier hesaplanabilmesi için note pozisyonu gerekli
3. `OutputPlan::new(OsRng, value, ADDRESS_1)` — farklı adrese çıktı
4. `with_populated_detection_data` — FMD clue'larını otomatik doldur
5. Tx çalıştır
6. Nullifier kontrolü ve client sync

**Assertion'lar:**
```rust
// Nullifier'lar önceden harcanmamış:
for nf in tx.spent_nullifiers():
    assert pre_tx_snapshot.spend_info(nf).await.is_none()
    assert post_tx_snapshot.spend_info(nf).await.is_some()

// Client output note'u tespit etti:
let output_nc = tx.outputs().next().body.note_payload.note_commitment
assert client.notes.contains_key(&output_nc)
```

### Sistem Davranış Notları
- `SpendPlan` note'un SCT (State Commitment Tree) içindeki pozisyonunu gerektirir — ZK proof için
- `detection_data: None` → `with_populated_detection_data(OsRng, Default::default())` ile otomatik FMD clue üretimi
- Client sync sonrası note tespiti — shielded pool'dan gelen kompakt blok verileriyle

### Nova Shield Chain İçin Dersler
- Nullifier mekanizması çift harcamayı önler. Her note için benzersiz nullifier: `nullifier = prf(nk, position)`.
- `spend_info(nullifier)` — nullifier'ın harcandığı blok bilgisini içermeli (sadece boolean değil).
- FMD (Fuzzy Message Detection) clue'ları, gizliliği korurken note tespitini mümkün kılar. Nova Shield shielded pool için bu mekanizma gerekli.
- `note_commitment` — notun gizli içeriğinin kriptografik taahhüdü. Client bu commitment'ı arayarak sahip olduğu note'ları tespit eder.

---

## [app_can_sweep_a_collection_of_small_notes.rs]
**Dosya:** `.../tests/app_can_sweep_a_collection_of_small_notes.rs`
**Satır Sayısı:** 195

### Genel Amaç
Wallet'taki küçük note'ları daha büyük note'lara birleştiren "sweep" (süpürme) işlemini test eder. View server entegrasyonunu kapsar.

### Sabitler
- `COUNT: usize = SWEEP_COUNT + 1` — sweep batch boyutunun bir fazla note

### Test Senaryoları

#### `app_can_sweep_a_collection_of_small_notes`
**NOT:** `#[ignore = "Flaked in #4626"]` — zaman zaman başarısız oluyor (multi-thread test)

**Setup:**
- `COUNT` adet 1 birimlik küçük allocation
- `tokio::test(flavor = "multi_thread", worker_threads = 4)`
- Mock Tendermint proxy ile gRPC sunucu başlatma (127.0.0.1:8081)
- View server + ViewServiceClient

**Adımlar:**
1. `COUNT` adet küçük note genesis'te oluştur
2. 10 blok ileri sar
3. View server'ı sync et
4. `penumbra_sdk_wallet::plan::sweep()` loop:
   ```rust
   loop:
       plans = sweep(&mut view_client, OsRng)
       if plans.is_empty(): break
       for plan in plans:
           tx = client.witness_auth_build(&plan)
           node.block().with_data(tx).execute()
   ```
5. Son not sayısını kontrol et

**Assertion'lar:**
```rust
// Sweep öncesi COUNT note:
assert client.notes_by_asset(STAKING_TOKEN).count() == COUNT

// Sweep sonrası 2 note (sweep batch + remainder):
assert post_sweep_notes[AddressIndex(0)][STAKING_TOKEN].len() == Some(2)
```

**İnvaryantlar:**
- Sweep, `SWEEP_COUNT` note'u birleştirir → 1 output + değişim = 2 note

### Sistem Davranış Notları
- `MockTendermintProxy` — gRPC çağrılarını gerçek node'a yönlendiren proxy
- View server: `ViewServer::load_or_initialize(None, None, &FVK, grpc_url)` — disk'e yazmıyor (None paths)
- `StatusResponse { full_sync_height: 10, partial_sync_height: 10, catching_up: false }` — sync doğrulaması
- Flaky test: port çakışması veya race condition (4 worker thread)

### Nova Shield Chain İçin Dersler
- "Note sweep" — UTXO modelinde kaçınılmaz fragment'leri birleştirme işlemi. Nova Shield UTXO modeli kullanırsa sweep mekanizması şart.
- `SWEEP_COUNT` bir batch sınırı — tek tx'te birleştirilebilecek max note sayısı. Bu parametreyi ayarlamak gas optimizasyonu açısından kritik.
- Multi-thread test'lerde port çakışmaları sorun yaratabiliyor — test portlarını dikkatli yönet.

---

## [app_can_undelegate_from_a_validator.rs]
**Dosya:** `.../tests/app_can_undelegate_from_a_validator.rs`
**Satır Sayısı:** 411

### Genel Amaç
Undelegasyon + unbonding süreci ve `UndelegateClaimPlan` ile staking token'larını geri alma işlemini test eder. Exchange rate doğrulaması içerir.

### Sabitler
- `EPOCH_DURATION: u64 = 3`
- `UNBONDING_DELAY: u64 = 4`

### Test Senaryoları

#### `app_can_undelegate_from_a_validator`
**Adımlar:**
1. 2. epoch'a kadar ilerle (exchange rate 1:1 değil olsun)
2. Delegate tx: `delegate_rate.build_delegate(epoch, note.amount())`
3. Nullifier harcandı mı kontrol et
4. `fast_forward_to_next_epoch`
5. Undelegate tx: `undelegate_rate.build_undelegate(epoch, note.amount())`
6. `compute_unbonding_height` ile beklenen unbonding bitiş yüksekliği hesapla
7. Unbonding bitimine kadar ilerle
8. `UndelegateClaimPlan` ile staking token'larını geri al

**Exchange Rate Doğrulaması:**
```rust
use penumbra_sdk_stake::BPS_SQUARED_SCALING_FACTOR;

let delegate_exchange_rate = delegate_rate.validator_exchange_rate
let scaled_delegate_rate = delegate_exchange_rate / BPS_SQUARED_SCALING_FACTOR

// Delegasyon sırasında alınan delegation token miktarı:
expected = (staking_note_amount / scaled_delegate_rate).round_down()
assert delegate_note.amount() == expected

// Undelegasyon sırasında alınan unbonding token miktarı:
expected = (delegate_note_amount * scaled_undelegate_rate).round_down()
assert undelegate_note.amount() == expected
```

**Assertion'lar:**
- Delegasyondan hemen sonra delegation token var
- Undelegasyondan hemen sonra unbonding token var, delegation token HALA var (TODO yorumu)
- Unbonding claim sonrası staking token, delegation token, unbonding token hepsi mevcut (claim yaptıktan sonra bile old notlar görünüyor — SCT'de hala mevcut)

### Sistem Davranış Notları
- **BPS_SQUARED_SCALING_FACTOR**: Exchange rate değerleri basis point kareyle ölçeklenir
- **Unbonding delay**: Undelegasyondan sonra `UNBONDING_DELAY` blok beklemek gerekir
- **compute_unbonding_height**: Staking sisteminin unbonding bitiş yüksekliğini hesaplar
- `TODO`: Delegation token'lar undelegasyondan sonra hala görünüyor — bu bir bug işareti

### Nova Shield Chain İçin Dersler
- Unbonding delay staking güvenliği için kritik: uzun süreli saldırılara karşı koruma.
- Exchange rate hesaplaması `U128x128` fixed-point aritmetiği gerektirir — float kullanma.
- `BPS_SQUARED_SCALING_FACTOR`: Oranları integer olarak saklarken dikkat: ölçeklendirme faktörünü belgele.
- `round_down()` — delegasyon/undelegasyon hesaplarında kesirli tokenlar için aşağı yuvarlama protokol mantığıyla tutarlı olmalı.
- `UndelegateClaimPlan { penalty: Penalty::from_percent(0) }` — slashing olmadığında 0 ceza.

---

## [app_check_dex_vcb.rs]
**Dosya:** `.../tests/app_check_dex_vcb.rs`
**Satır Sayısı:** 78

### Genel Amaç
DEX Value Circuit Breaker (VCB)'ın swap flow'larını doğru şekilde takip ettiğini doğrular. Bug #4643'ün regression testi.

### Bug Açıklaması (Kritik DEX Bug)
```
HATA: aggregate = (delta + aggregate) + aggregate  // çift sayım
DOĞRU: aggregate = delta + aggregate
```
Bu bug `DexComponent` içinde swap flow tracking'de çift sayım yapıyordu.

### Test Senaryoları

#### `dex_vcb_tracks_multiswap`
**Setup:**
- `rand_chacha::ChaChaRng::seed_from_u64(1776)` — deterministik RNG
- Default genesis, `StateDelta` ile doğrudan state değişikliği

**Adımlar:**
```rust
// Swap 1:
let trading_pair = TradingPair::new(gm.id(), gn.id())
let plaintext = SwapPlaintext::new(&mut rng, trading_pair, 100_000, 0, Fee::default(), address)
let swap_one = SwapPlan::new(&mut rng, plaintext).swap(&FVK)
state_tx.put_mock_source(1u8)
swap_one.check_and_execute(&mut state_tx).await

// VCB kontrolü 1:
let gm_vcb = state_tx.get_dex_vcb_for_asset(&gm.id()).await
assert gm_vcb == 100_000

// Swap 2 (aynı swap):
let swap_two = swap_one.clone()
swap_two.check_and_execute(&mut state_tx).await

// VCB kontrolü 2:
let gm_vcb = state_tx.get_dex_vcb_for_asset(&gm.id()).await
assert gm_vcb == 200_000  // 2*100_000, çift sayım değil
```

**İnvaryantlar:**
- N swap işlemi → VCB = N * swap_amount (doğrusal birikme)
- VCB hiçbir zaman çift sayma yapmamalı

### Sistem Davranış Notları
- `put_mock_source(1u8)` — işlem kaynağını ayarlamak için gerekli (VCB güncellenmesi için)
- `get_dex_vcb_for_asset` — belirli asset için DEX VCB bakiyesini döndürür
- `ArcStateDeltaExt::try_begin_transaction()` — state delta üzerinde sub-transaction açma

### DEX Uygulama Notu — Nova Shield DEX için
Bu kritik bir regression testidir. VCB çift sayım bug'ı:
- DEX bileşeninin kendi bakiyesi hakkında yanlış bilgiye sahip olmasına yol açar
- Potansiyel chain halt'a neden olabilir (bileşen bakiyesi tutarsız)
- Nova Shield DEX'te VCB implementasyonunda `aggregate = delta + aggregate` formülünü zorunlu kıl — `aggregate = (delta + aggregate) + aggregate` asla değil
- Her VCB güncelleme noktasında assertion ekle: yeni_bakiye > eski_bakiye (single swap için)
- VCB regression testleri her DEX component değişikliğinde çalıştırılmalı

---

## [app_rejects_validator_definitions_with_invalid_auth_sigs.rs]
**Dosya:** `.../tests/app_rejects_validator_definitions_with_invalid_auth_sigs.rs`
**Satır Sayısı:** 145

### Genel Amaç
Geçersiz authentication signature içeren validatör tanım tx'lerinin reddedildiğini doğrular.

### Test Senaryoları

#### `app_rejects_validator_definitions_with_invalid_auth_sigs`
**Sahte İmza Saldırısı:**
```rust
let different_signing_key = SigningKey::<SpendAuth>::new(OsRng);  // farklı key!
let bytes = new_validator.encode_to_vec();
// NB: validator'ın kendi signing key'i KULLANILMIYOR — sahte imza
let auth_sig = different_signing_key.sign(OsRng, &bytes);
let action = ActionPlan::ValidatorDefinition(validator::Definition {
    validator: new_validator,
    auth_sig,  // GEÇERSIZ
})
```

**Assertion:**
```rust
// Tx çalıştırıldıktan sonra:
assert post_tx_snapshot.validator_definitions().len() == 1
// Hala 1 validatör — yeni geçersiz tanım eklenmedi
```

**İnvaryant:** Geçersiz auth sig içeren validatör tanımları sessizce görmezden gelinir (panik değil, sessiz red).

### Sistem Davranış Notları
- Tx çalıştırılır ama geçersiz içerik state'e yansımaz
- Consensus motoru geçersiz action'ları reddeder, block'u reddetmez
- Bu "silent drop" davranışı Tendermint/CometBFT modeliyle uyumlu

### Nova Shield Chain İçin Dersler
- Validatör tanımları, validatörün identity key'iyle imzalanmış auth_sig içermeli. Bu, başka birinin adına validatör tanımlamasını önler.
- Geçersiz action'lar sessizce görmezden gelinmeli — panikleme veya tüm bloğu reddetme.
- `ValidatorDefinition { validator, auth_sig }` yapısı: `auth_sig = identity_sk.sign(validator.encode_to_vec())` doğrulama formülü.

---

## [app_reproduce_testnet_75_vcb_close.rs]
**Dosya:** `.../tests/app_reproduce_testnet_75_vcb_close.rs`
**Satır Sayısı:** 267

### Genel Amaç
Testnet v0.75.0'da gerçekleşen VCB bug'ının minimal reproductions testi: Dutch auction erken kapatılırken LP pozisyon rezervleri auction bileşenine yanlış aktarılıyordu.

### Bug Açıklaması (Kritik Auction/DEX Bug)
```
Koşul: Dutch auction seq=0'da aktif, deploy edilmiş LP var
Tetik: Kullanıcı ActionDutchAuctionEnd gönderiyor (erken kapatma)
Sorun: DutchAuctionManager::end_auction_by_id LP pozisyon rezervlerini
       auction bileşeninin VCB'sine kredilendirmeyi atlıyor
Sonuç: Auction bileşeninin kendi bakiyesi hakkında yanlış bilgisi → chain halt
```

### Test Senaryoları

#### `app_can_reproduce_tesnet_75_vcb_close`

**Dutch Auction Konfigürasyonu:**
```rust
DutchAuctionDescription {
    input: { asset: STAKING_TOKEN, amount: 1_000_000 },
    output_id: "gm" asset,
    max_output: 100 gm,
    min_output: 1 gm,
    start_height: 50,
    end_height: 100,
    step_count: 50,
    nonce: [0u8; 32],
}
```

**Adımlar:**
1. Schedule auction tx gönder (spend + change + NFT(seq=0) outputs)
2. 51 blok ilerle (auction start_height=50 geçildi)
3. Auction state doğrula: `get_dutch_auction_by_id(auction_id).is_some()`
4. `ActionDutchAuctionEnd { auction_id }` gönder (erken kapatma)
   - NFT(seq=0) harcanır, NFT(seq=1) alınır
5. 1 blok daha ilerle
6. Auction state ve VCB kontrol et

**Assertion'lar:**
```rust
assert new_auction_state.state.sequence == 1  // kapatıldı
assert auction_vcb_staking_token == 1_000_000u128.into()  // rezervler korundu
```

**İnvaryant:** Auction kapatılınca LP pozisyon rezervleri auction VCB'sine tam olarak aktarılmalı.

### Sistem Davranış Notları
- `AuctionNft::new(auction_id, seq)` — her auction durumu için NFT (seq: 0=açık, 1=kapalı)
- NFT harcama + yeni NFT output — auction durumu değişimi atomik
- `get_auction_value_balance_for(asset_id)` — auction bileşeninin VCB bakiyesi
- Tracing filter: `penumbra_auction=trace`, `penumbra_dex=debug`

### DEX Uygulama Notu — Nova Shield DEX için
Dutch auction mekanizması için kritik güvenlik noktası:
- Erken kapatma senaryosu ayrı test edilmeli — normal kapatma yeterli değil
- LP pozisyon rezervleri, auction kapatıldığında ilgili bileşenin VCB'sine kredilenmeli
- VCB tutarsızlığı → chain halt: bu yüzden her bileşen değişikliğinde VCB integrity kontrol edilmeli
- `AuctionNft` pattern: auction durumunu NFT token olarak temsil etmek, cüzdan uyumluluğu için yararlı

---

## [app_tracks_uptime_for_genesis_validator_missing_blocks.rs]
**Dosya:** `.../tests/app_tracks_uptime_for_genesis_validator_missing_blocks.rs`
**Satır Sayısı:** 88

### Genel Amaç
Genesis validatörünün imzasız blokları kaçırmasını takip eder.

### Test Senaryoları

#### `app_tracks_uptime_for_genesis_validator_missing_blocks`
**Adımlar:**
```rust
// İmzasız bloklar:
for i in 1..=4:
    node.block().without_signatures().execute()

let uptime = get_uptime(identity_key)
assert uptime.as_of_height() == 4
assert uptime.num_missed_blocks() == 3  // off-by-one: 4-1
```

**Kritik Not — Off-by-one:**
```
// NB: this is off-by-one
assert uptime.num_missed_blocks() == (height - 1) as usize
```
Bu off-by-one kasıtlı ve yorumlanmış: ilk blok (genesis'ten hemen sonraki) farklı davranıyor.

**İnvaryantlar:**
- `.without_signatures()` — blok üretilir ama validatör imzası yok
- Uptime genesis'te 0 kaçırılan blokla başlar

### Sistem Davranış Notları
- `node.block().without_signatures()` — imzasız blok simülasyonu
- Off-by-one: Genesis sonrası ilk bloğun uptime sayımına dahil edilmeme durumu var

### Nova Shield Chain İçin Dersler
- Uptime takibi: `num_missed_blocks()` ve `as_of_height()` — ikisi birlikte anlam ifade eder.
- Off-by-one durumları belgelenmeli. Nova Shield'da uptime hesaplamasında genesis bloğunu özel case olarak ele al.
- `without_signatures()` API'si test ortamında validatör liveness sorunlarını simüle etmek için kritik.

---

## [app_tracks_uptime_for_genesis_validator_signing_blocks.rs]
**Dosya:** `.../tests/app_tracks_uptime_for_genesis_validator_signing_blocks.rs`
**Satır Sayısı:** 77

### Genel Amaç
Genesis validatörünün tüm blokları imzaladığı durumda uptime takibinin doğru çalıştığını test eder. Missing blocks versiyonuyla birebir aynı isim (`app_tracks_uptime_for_genesis_validator_missing_blocks`) ama farklı dosya — `fast_forward` kullanır.

### Test Senaryoları

#### `app_tracks_uptime_for_genesis_validator_missing_blocks` (aynı isim, farklı senaryo)
```rust
// fast_forward validatörün bloğu imzalamasını sağlar:
node.fast_forward(4)

let uptime = get_uptime(identity_key)
assert uptime.as_of_height() == 4
assert uptime.num_missed_blocks() == 0  // hiç kaçırılmamış
```

**NOT:** Bu dosyada fonksiyon ismi yanlışlıkla "missing_blocks" olarak yazılmış ama aslında "signing_blocks" senaryosu.

### Sistem Davranış Notları
- `fast_forward(n)` validatörün tüm blokları imzalamasına eşdeğer
- `node.block().without_signatures()` ise imzasız blok üretir

### Nova Shield Chain İçin Dersler
- `fast_forward` ve `block().without_signatures()` arasındaki fark test altyapısının özüdür. İki durum her zaman test edilmeli.

---

## [app_tracks_uptime_for_validators_only_once_active.rs]
**Dosya:** `.../tests/app_tracks_uptime_for_validators_only_once_active.rs`
**Satır Sayısı:** 421

### Genel Amaç
Uptime takibinin yalnızca aktif validatörler için yapıldığını; Defined ve Inactive validatörlerin uptime takibinin durdurulduğunu test eder.

### Test Senaryoları

#### `app_tracks_uptime_for_validators_only_once_active`
```
Epoch 0: new_validator tanımla (Defined)
    assert new_validator_uptime == None  // takip yok

    Delegate to new_validator
    assert new_validator_uptime == None  // hala takip yok

    fast_forward_to_next_epoch

Epoch 1: new_validator Active oldu
    assert consensus_set.len() == 2
    assert new_validator_uptime.is_some()
    assert new_validator_uptime.num_missed_blocks() == 1  // FIXME: promoted validators always miss one block
    assert new_validator_uptime.as_of_height() == existing.as_of_height()

    Undelegate from new_validator

    assert new_validator_uptime.is_some_and(|u| u.num_missed_blocks() == 2)

    fast_forward_to_next_epoch

Epoch 2: new_validator Defined'a döndü
    assert consensus_set.len() == 1
    // Uptime donduruldu (aktif değil):
    assert new_validator.as_of_height() == EPOCH_DURATION * 2 - 1  // son güncelleme
    assert existing.as_of_height() == EPOCH_DURATION * 2  // hala güncelleniyor

    fast_forward_to_next_epoch

Epoch 3:
    assert new_validator.as_of_height() == EPOCH_DURATION * 2 - 1  // hala dondurulmuş
    assert existing.as_of_height() == EPOCH_DURATION * 3  // devam ediyor
```

### Kritik Not — Promote Edilen Validatör Bug'ı
```
// FIXME: promoted validators always miss one block.
// "tracking is done at the beginning of block execution, and based on the previous
//  block commit h-1 so if a validator is promoted into the active set at h it will
//  always have 1 missed block - not sure this is worth fixing"
// - @erwanor
assert new.num_missed_blocks() == 1
```

### Sistem Davranış Notları
- Uptime takibi yalnızca `State::Active` validatörler için — Defined validatörler için `None`
- Aktiflikten çıkınca uptime dondurulur (silinmez, sadece güncellenmez)
- Promosyon anında her zaman 1 kaçırılan blok (tasarımsal karar)

### Nova Shield Chain İçin Dersler
- Uptime takibi sadece aktif validatörler için — bu bellek optimizasyonu.
- Aktiflikten çıkınca uptime geçmişi silinmemeli, sadece dondurulmalı — ileride yeniden aktif olursa önceki geçmiş silinmiş olmaz.
- Promosyon anında 1 kaçırılan blok off-by-one'u kabul edilebilir teknik borç — belgelenmiş olması önemli.

---

## [common/mod.rs]
**Dosya:** `.../tests/common/mod.rs`
**Satır Sayısı:** 37

### Genel Amaç
Test ortak altyapısını modüler hale getirir. `TempStorageExt`, `BuilderExt`, `TestNodeExt`, `ValidatorDataReadExt` ve IBC test modülünü dışa aktarır.

### Dışa Aktarılan Modüller
```rust
pub use {
    TempStorageExt,
    BuilderExt,
    TestNodeExt,
    ValidatorDataReadExt,
    set_tracing_subscriber,
    set_tracing_subscriber_with_env_filter,
}
pub mod ibc_tests;
```

### Nova Shield Chain İçin Dersler
- Test altyapısını `mod common` içinde toplamak ve re-export etmek kod tekrarını önler.
- `#[allow(unused_imports)]` — test modüllerinde kullanılmayan importları sessizce geç.

---

## [common/ibc_tests/mod.rs]
**Dosya:** `.../tests/common/ibc_tests/mod.rs`
**Satır Sayısı:** 193

### Genel Amaç
IBC testleri için `ValidatorKeys` yapısını ve `get_verified_genesis()` fonksiyonunu tanımlar. CometBFT uyumlu genesis formatı üretir.

### Struct'lar

#### `ValidatorKeys`
```rust
pub struct ValidatorKeys {
    pub validator_id_sk: SigningKey<SpendAuth>,
    pub validator_id_vk: VerificationKey<SpendAuth>,
    pub validator_spend_key: SpendKeyBytes,
    pub validator_cons_sk: tendermint::PrivateKey,
    pub validator_cons_pk: tendermint::PublicKey,
    pub node_key_sk: tendermint::PrivateKey,
    pub identity_key: IdentityKey,
    pub node_key_pk: tendermint::PublicKey,
}
```

`from_seed(seed: [u8; 32])` — tüm anahtarları deterministic olarak bir seed'den türetir:
- SpendKey → identity_sk / identity_vk
- ed25519 from seed → consensus key
- ed25519 from seed → node key (P2P auth)

#### `get_verified_genesis()`
CometBFT uyumlu `Genesis` struct üretir:
```rust
consensus_params: {
    block: { max_bytes: MAX_BLOCK_TXS_PAYLOAD_BYTES, max_gas: -1, time_iota_ms: 500 },
    evidence: { max_age_num_blocks: 130000, max_age_duration: ~7 gün, max_bytes: MAX_EVIDENCE_SIZE_BYTES },
    validator: { pub_key_types: [Ed25519] }
}
```

Genesis allocation: `25_000 * 10^6` delegation token

### Sistem Davranış Notları
- `max_gas: -1` — Penumbra'nın kendi gas modeli var, CometBFT'nin gas mekanizmasını kullanmıyor
- `time_iota_ms: 500` — minimum bloklar arası zaman artışı 500ms
- Evidence max_age: ~130K blok, ~7 gün — unbonding_delay ile senkronize

### Nova Shield Chain İçin Dersler
- `max_gas: -1` pattern'i: Eğer kendi gas sistemin varsa CometBFT gas'ı devre dışı bırak.
- Evidence max_age = unbonding_delay uyumu: Slashable davranışlar unbonding süresinden daha uzun süre delil olarak saklanmalı.
- `from_seed` deterministic key üretimi test reproducibility için kritik.

---

## [common/ibc_tests/node.rs]
**Dosya:** `.../tests/common/ibc_tests/node.rs`
**Satır Sayısı:** 334

### Genel Amaç
IBC testleri için `TestNodeWithIBC` yapısını tanımlar. Gerçek gRPC sunucu başlatarak IBC sorgularına cevap verir.

### Struct'lar

#### `TestNodeWithIBC`
```rust
pub struct TestNodeWithIBC {
    pub connection_id: ConnectionId,  // ConnectionId::new(0)
    pub channel_id: ChannelId,        // ChannelId::new(0)
    pub client_id: ClientId,          // "07-tendermint-0"
    pub port_id: PortId,              // PortId::transfer()
    pub chain_id: String,
    pub counterparty: Counterparty,
    pub connection_version: ConnectionVersion,
    pub channel_version: ChannelVersion,  // "ics20-1"
    pub signer: String,               // SHA256(pk)[0..20] hex
    pub node: TestNode<...>,
    pub storage: TempStorage,
    pub ibc_client_query_client: IbcClientQueryClient<Channel>,
    pub ibc_connection_query_client: IbcConnectionQueryClient<Channel>,
    pub ibc_channel_query_client: IbcChannelQueryClient<Channel>,
    pub tendermint_proxy_service_client: TendermintProxyServiceClient<Channel>,
}
```

**IBC Özellikleri:**
- `client_type = "07-tendermint"` — Tendermint light client
- `channel_version = "ics20-1"` — ICS-20 token transfer protokolü
- `IBC_COMMITMENT_PREFIX` — IBC taahhüt prefix'i
- Port: `PortId::transfer()` — ICS-20 standardı

**`create_tendermint_header` metodu:**
```rust
// Mevcut blok'tan Tendermint header oluşturur
// SignedHeader: header + commit
// ValidatorSet: validator bilgisi
// Power: 25_000 * 10^6
// TrustedHeight: önceki kontrol noktası
```

### Sistem Davranış Notları
- gRPC sunucu `999{index}` portlarında çalışır (0=9990, 1=9991) — standart portlarla çakışmaz
- `time::sleep(1s)` — gRPC sunucunun başlaması için bekleme
- `SHA256(pk)[0..20]` — proposer address hesaplama

### Nova Shield Chain İçin Dersler
- IBC için "07-tendermint" client type'ı CometBFT ile uyum şartı. Nova Shield CometBFT kullanıyorsa bu client type desteklenmeli.
- `IBC_COMMITMENT_PREFIX` — IBC state kanıtları için Merkle prefix'i. JMT (Jellyfish Merkle Tree) ile uyumlu olmalı.
- `PortId::transfer()` — ICS-20 için standart port adı "transfer". Farklı IBC protokolleri için farklı portlar.

---

## [common/ibc_tests/relayer.rs]
**Dosya:** `.../tests/common/ibc_tests/relayer.rs`
**Satır Sayısı:** ~800+ (büyük dosya, preview 2KB)

### Genel Amaç
IBC tam el sıkışma protokolünü (CreateClient → ConnectionOpen* → ChannelOpen* → Transfer) uygulayan `MockRelayer` yapısı.

### İçerik Özeti (Preview'dan)
- `IbcChannelQueryClient`, `IbcClientQueryClient`, `IbcConnectionQueryClient` — gRPC sorgu istemcileri
- `MsgAcknowledgement`, `MsgChannelOpenAck/Confirm/Init/Try` — kanal mesajları
- `MsgRecvPacket` — paket alım mesajı
- `MsgConnectionOpenAck/Confirm/Init/Try` — bağlantı el sıkışma mesajları
- `MsgCreateClient`, `MsgUpdateClient` — istemci yönetim mesajları
- `TrustThreshold { numerator: 1, denominator: 3 }` — 1/3 güven eşiği
- `ics20` transfer mesajları

### Nova Shield Chain İçin Dersler
- IBC tam el sıkışma: CreateClient → ConnectionOpenInit → ConnectionOpenTry → ConnectionOpenAck → ConnectionOpenConfirm → ChannelOpenInit → ... → Transfer. Bu 6+ adımlı protokol dikkatli test edilmeli.
- `TrustThreshold { 1, 3 }` — BFT güvenlik sınırı: validatör gücünün 1/3'ü kadar kötü aktörü tolere eder.
- Mock relayer pattern: iki zincir arasında mesaj geçişini simüle eden test aracı. Nova Shield IBC implementasyonu için aynı pattern.

---

## [common/temp_storage_ext.rs]
**Dosya:** `.../tests/common/temp_storage_ext.rs`
**Satır Sayısı:** 39

### Genel Amaç
`TempStorage`'ı Penumbra'ya özgü metodlarla genişleten extension trait.

### Trait Tanımı

#### `TempStorageExt`
```rust
#[async_trait]
pub trait TempStorageExt: Sized {
    async fn apply_genesis(self, genesis: AppState) -> anyhow::Result<Self>;
    async fn apply_default_genesis(self) -> anyhow::Result<Self>;
    async fn new_with_penumbra_prefixes() -> anyhow::Result<TempStorage>;
}
```

**`new_with_penumbra_prefixes()`:**
```rust
TempStorage::new_with_prefixes(SUBSTORE_PREFIXES.to_vec()).await
```
Penumbra'nın `SUBSTORE_PREFIXES`'ini kullanarak depolama başlatır.

**`apply_genesis()`:**
```rust
if self.latest_version() != u64::MAX:
    bail!("database already initialized")
let mut app = App::new(self.latest_snapshot())
app.init_chain(&genesis).await
app.commit(self.deref().clone()).await
```

**`apply_default_genesis()`:**
```rust
self.apply_genesis(Default::default()).await
```

### Sistem Davranış Notları
- `latest_version() == u64::MAX` — başlatılmamış depolama sinyali
- `App::new(snapshot)` → `init_chain(&genesis)` → `commit(storage)` — genesis başlatma üç adım

### Nova Shield Chain İçin Dersler
- Extension trait pattern: mevcut tipleri değiştirmeden yeni metodlar eklemek için ideal. Nova Shield'ın test storage trait'leri bu pattern'i takip etmeli.
- `latest_version() == u64::MAX` — başlatılmamış DB sinyali için sentinel değer. Nova Shield DB başlatma kontrolü için aynı pattern.
- `init_chain + commit` atomik olmalı — ikisi arasında başarısızlık tutarsız state bırakır.

---

## [common/test_node_builder_ext.rs]
**Dosya:** `.../tests/common/test_node_builder_ext.rs`
**Satır Sayısı:** 113

### Genel Amaç
`Builder`'a Penumbra'ya özgü `with_penumbra_auto_app_state` metodunu ekler. Keyring'deki her validatör anahtarı için otomatik genesis state üretir.

### Trait Tanımı

#### `BuilderExt`
```rust
pub trait BuilderExt: Sized {
    type Error;
    fn with_penumbra_auto_app_state(self, app_state: AppState) -> Result<Self, Self::Error>;
}
```

**İmplementasyon:**
```rust
fn with_penumbra_auto_app_state(mut self, app_state: AppState) -> Result<Self, Self::Error> {
    let content = match app_state { AppState::Content(c) => c, Checkpoint => bail! }

    for (consensus_vk, _) in keyring:
        let seed = SpendKeyBytes(consensus_vk.to_bytes())  // deterministic seed!
        let (validator, allocation) = generate_penumbra_sdk_validator(consensus_vk, seed)
        content.stake_content.validators.push(validator)
        content.shielded_pool_content.allocations.push(allocation)

    if !content.chain_id.is_empty():
        self.chain_id = Some(content.chain_id.clone())

    let bytes = serde_json::to_vec(&AppState::Content(content))?
    Ok(self.app_state(bytes))
}
```

**`generate_penumbra_sdk_validator(consensus_key, seed)`:**
```rust
// seed'den SpendKey üret:
let spend_key = SpendKey::from(seed)
// Validator:
PenumbraValidator {
    identity_key: spend_key.spend_auth_key().vk,
    governance_key: identity_vk,  // genesis testlerinde aynı key
    consensus_key: consensus_vk.as_bytes(),
    enabled: true, sequence_number: 0, ...
}
// 1000 birim delegation allocation:
Allocation { raw_amount: 1000, raw_denom: delegation_denom, address: payment_address }
```

### Sistem Davranış Notları
- `consensus_vk.to_bytes()` → SpendKeyBytes — consensus key'den deterministik spend key türetimi (sadece test için!)
- 1000 birim delegation — yeterince oy gücü için minimum allocation
- `AppState::Checkpoint` desteklenmiyor — test başlangıcı her zaman tam genesis

### Nova Shield Chain İçin Dersler
- Test ortamında deterministic key türetimi kritik: `seed = consensus_vk.to_bytes()` → her test çalışmasında aynı state.
- Genesis allocation boyutu önemli: çok az stake → validatör aktif olamaz. 1000 birim iyi bir test değeri.
- `with_penumbra_auto_app_state` pattern: test altyapısı genesis setup karmaşıklığını gizler. Nova Shield için aynı extension trait.

---

## [common/test_node_ext.rs]
**Dosya:** `.../tests/common/test_node_ext.rs`
**Satır Sayısı:** 48

### Genel Amaç
`TestNode<C>`'ye `fast_forward_to_next_epoch` metodunu ekler.

### Trait Tanımı

#### `TestNodeExt`
```rust
#[async_trait]
pub trait TestNodeExt: Sized {
    async fn fast_forward_to_next_epoch(
        &mut self, storage: &TempStorage
    ) -> anyhow::Result<penumbra_sdk_sct::epoch::Epoch>;
}
```

**İmplementasyon:**
```rust
async fn fast_forward_to_next_epoch(&mut self, storage: &TempStorage) -> Result<Epoch> {
    let start = storage.latest_snapshot().get_current_epoch().await?
    loop:
        self.block().execute().await?
        let current = storage.latest_snapshot().get_current_epoch().await?
        if current != start:
            return Ok(current)
}
```

### Sistem Davranış Notları
- Epoch geçişi blok-by-blok kontrol edilir — `EPOCH_DURATION` bilinmeden epoch beklenir
- `get_current_epoch()` her blokta state'den okunur
- Generic bound: `C: tower::Service<ConsensusRequest, Response=ConsensusResponse>` — herhangi bir consensus service ile çalışır

### Nova Shield Chain İçin Dersler
- `fast_forward_to_next_epoch` pattern: epoch bazlı testler için vazgeçilmez utility. Nova Shield için aynı trait.
- Epoch geçişini storage'dan okumak: epoch geçişi state'e yazıldı mı doğrulanmış olur.
- Tower::Service generic bound — farklı consensus implementasyonlarını soyutlamak için ideal.

---

## [common/validator_read_ext.rs]
**Dosya:** `.../tests/common/validator_read_ext.rs`
**Satır Sayısı:** 43

### Genel Amaç
`ValidatorDataRead` trait'ini genişleten test-specific extension: tüm validatör tanımlarını ve identity key'lerini döndürür.

### Trait Tanımı

#### `ValidatorDataReadExt`
```rust
#[async_trait]
pub trait ValidatorDataReadExt: ValidatorDataRead {
    async fn validator_definitions(&self) -> anyhow::Result<Vec<Validator>>;
    async fn validator_identity_keys(&self) -> anyhow::Result<Vec<IdentityKey>>;
}
```

**İmplementasyon:**
```rust
// Tüm validatörleri prefix scan ile al:
self.prefix(state_key::validators::definitions::prefix())
    .map_ok(|(_key, validator)| validator)
    .try_collect()
    .await
```

**Not:** Bu metodlar pahalı (O(N validatör)) — yalnızca test için. Production'da kullanılmaz.

### Sistem Davranış Notları
- `prefix()` — prefix scan stream döner
- `TryStreamExt::try_collect()` — stream'i Vec'e toplar
- `state_key::validators::definitions::prefix()` — validatör tanımlarının JMT key prefix'i

### Nova Shield Chain İçin Dersler
- Expensive prefix scan'leri test-only trait'lere hapsettirmek doğru pattern.
- `state_key` modülü: state anahtarlarını merkezi bir yerde yönetmek. Nova Shield için aynı pattern — magic string değil, tip-güvenli key fonksiyonları.

---

## [ics23_transfer.rs]
**Dosya:** `.../tests/ics23_transfer.rs`
**Satır Sayısı:** 145

### Genel Amaç
ICS-20 token transferini (`ics20_transfer_no_timeouts`) iki zincir arasında uçtan uca test eder.

### Test Senaryoları

#### `ics20_transfer_no_timeouts`
**Setup:**
- Zincir A: `start_time_a = "2022-02-11T17:30:50.425417198Z"`
- Zincir B: `start_time_b = start_time_a - 39 * 5s` (chain B 39 blok önde)
- Deterministik key'ler: `vkeys_a = from_seed([0u8;32])`, `vkeys_b = from_seed([1u8;32])`

**Adımlar:**
1. Chain A: 3 blok
2. Chain B: 42 blok (farklı yükseklik ama aynı zaman)
3. Timestamp eşitliği doğrula, yükseklik farklılığı doğrula
4. `MockRelayer { chain_a_ibc, chain_b_ibc }.handshake().await` — IBC el sıkışma
5. Transfer: A'dan B'ye %50 note miktarı
6. Bakiye doğrulama

**Assertion'lar:**
```rust
// Zaman eşit, yükseklik farklı:
assert chain_a_ibc.node.timestamp() == chain_b_ibc.node.timestamp()
assert chain_a_ibc.get_latest_height() != chain_b_ibc.get_latest_height()

// Transfer sonrası:
assert posttransfer_balance_a < pretransfer_balance_a  // A azaldı
assert posttransfer_balance_b > pretransfer_balance_b  // B arttı
assert posttransfer_balance_b == transfer_value.amount  // B tam transfer miktarını aldı
```

**IBC Token ID:**
```rust
let ibc_token = IbcToken::new(&channel_b_id, &port_b_id, &denom.to_string())
// Chain B'deki IBC wrapped token ID
```

### Sistem Davranış Notları
- Farklı yükseklik ama aynı zaman: IBC timeout height ve timeout timestamp'i test eder
- `IbcToken::new(channel, port, denom)` — cross-chain asset ID hesaplama
- El sıkışma 4 mesaj çiftinden oluşur: Create/Update client, Connection open (4 mesaj), Channel open (4 mesaj)

### Nova Shield Chain İçin Dersler
- IBC timestamp ve height tabanlı timeout'ları dikkatlice uygula — zincirler farklı hızda çalışabilir.
- `IbcToken` — cross-chain token identity. Channel ID + Port ID + original denom → yeni unique asset ID. Bu hash deterministik olmalı.
- El sıkışma testi production'dan önce şart — IBC mantığı karmaşık ve subtly hatalı olabilir.

---

## [mock_consensus_block_proving.rs]
**Dosya:** `.../tests/mock_consensus_block_proving.rs`
**Satır Sayısı:** 349

### Genel Amaç
IBC storage proof'larının doğruluğunu test eder: cnidarium'dan alınan proof ile gRPC üzerinden alınan proof karşılaştırılır.

### Test Senaryoları

#### `verify_storage_proof_simple`
**Adımlar:**
1. Fake IBC client oluştur (MsgCreateClient) → state'e yaz
2. `snapshot.get_with_proof(key)` — cnidarium'dan proof al
3. `proof.verify_membership(specs, root, path, value, 0)` — yerel doğrulama
4. gRPC üzerinden `KeyValueRequest { proof: true }` — uzak proof al
5. gRPC proof == cnidarium proof assert
6. `ibc_client_query_client.client_state()` — IBC API'den client state al
7. IBC proof doğrulama

**IBC Client State Değerleri:**
```rust
ClientState {
    chain_id: CHAIN_ID,
    trust_level: TrustThreshold { numerator: 1, denominator: 3 },
    trusting_period: 120_000s,
    unbonding_period: 240_000s,
    max_clock_drift: 5s,
    latest_height: { revision_height: 55 },
    proof_specs: IBC_PROOF_SPECS,
    upgrade_path: ["upgrade", "upgradedIBCState"],
}
```

**Proof Yüksekliği Paradoksu:**
```rust
// IBC proof height = current_height + 1 (henüz blok üretilmemiş)
assert u64::from(latest_height) + 1 == ibc_response.proof_height.revision_height

// Proof bloğu henüz mevcut değil:
assert proof_block.block.is_none()

// Sonraki bloğu üret:
node.block().execute().await

// Artık proof bloğu mevcut:
assert proof_block.block.is_some()
```

### Sistem Davranış Notları
- IBC proof height her zaman son kesinleşmiş bloğun bir fazlası
- `cnidarium::ics23_spec()` — Penumbra'nın özel ICS-23 proof spec'i
- `IBC_COMMITMENT_PREFIX.apply_string(ClientStatePath(...))` — IBC key prefix uygulaması

### Nova Shield Chain İçin Dersler
- IBC proof height = last_committed_height + 1: Bu IBC protokolünün gereği. State ispatları her zaman "sonraki" bloğun bakış açısından yapılır.
- `IBC_PROOF_SPECS` — Merkle tree yapısına uygun ics23 proof spec. JMT için özel spec gerekebilir.
- `verify_membership(specs, root, path, value, 0)` — 0. seviye (leaf) proof doğrulama.

---

## [mock_consensus_can_define_a_genesis_validator.rs]
**Dosya:** `.../tests/mock_consensus_can_define_a_genesis_validator.rs`
**Satır Sayısı:** 64

### Genel Amaç
Mock consensus'un genesis'te bir validatör tanımlayabildiğini ve bu validatörün `State::Active` olduğunu doğrular.

### Test Senaryoları

#### `mock_consensus_can_define_a_genesis_validator`
```rust
let snapshot = storage.latest_snapshot()
let validators = snapshot.validator_definitions().await?
match validators.as_slice():
    [v] => assert v.state == State::Active
    _ => panic!("expected one validator")
```

### Nova Shield Chain İçin Dersler
- Genesis validatörleri otomatik olarak `State::Active` başlamalı — hiçbir epoch geçişi beklenmez.

---

## [mock_consensus_can_send_a_sequence_of_empty_blocks.rs]
**Dosya:** `.../tests/mock_consensus_can_send_a_sequence_of_empty_blocks.rs`
**Satır Sayısı:** 49

### Genel Amaç
Boş blok dizisinin başarıyla işlendiğini ve blok yüksekliğinin doğru arttığını doğrular.

### Test Senaryoları

#### `mock_consensus_can_send_a_sequence_of_empty_blocks`
```rust
assert height() == 0  // başlangıç
test_node.fast_forward(8).await
assert height() == 8  // artış doğru
```

### Nova Shield Chain İçin Dersler
- Boş blok testi temel sağlık kontrolü — tüm sistemler çalışıyor mu?
- Her yeni consensus kodu değişikliğinde boş blok testi önce çalıştırılmalı.

---

## [spend.rs]
**Dosya:** `.../tests/spend.rs`
**Satır Sayısı:** 386

### Genel Amaç
Shielded spend (harcama) action'larını birim düzeyinde test eder. ZK spend proof doğrulama, sahte proof reddi ve çift harcama önleme testlerini kapsar.

### Test Senaryoları

#### `spend_happy_path`
**Adımlar:**
```rust
// BeginBlock simülasyonu:
state_tx.put_block_height(1)
state_tx.put_epoch_by_height(1, Epoch { index: 0, start_height: 0 })

// Spend action oluşturma:
let spend_plan = SpendPlan::new(&mut rng, note, tct_position)
let auth_sig = spend_key.spend_auth_key().randomize(randomizer).sign(effect_hash)
let spend = spend_plan.spend(&FVK, auth_sig, proof, root)

// Execution:
spend.check_stateless(transaction_context).await?
spend.check_historical(state.clone()).await?
spend.check_and_execute(&mut state_tx).await?

// EndBlock:
ShieldedPool::end_block(&mut state, &end_block).await
state_tx.finish_block().await
```

**TransactionContext:**
```rust
TransactionContext {
    anchor: root,          // SCT kök hash'i
    effect_hash: EffectHash([0u8; 64]),  // dummy
}
```

#### `invalid_dummy_spend`
**Zellic audit'ten (issue #3859): Sıfır değerli not ile sahte proof testi**

```rust
#[cfg_attr(
    debug_assertions,
    should_panic = "assertion failed: cs.is_satisfied().unwrap()"
)]
```

**Sahte proof oluşturma:**
```rust
let note_zero_value = Note::from_parts(address, Value { amount: 0, asset_id }, rseed)
let public = SpendProofPublic { anchor, balance_commitment, nullifier, rk }
let ak = VerificationKey::try_from([0u8; 32])  // sıfır verification key
let nk = NullifierKey(Fq::rand(&mut OsRng))     // rastgele nullifier key
let private = SpendProofPrivate { proof, note: note_zero_value, v_blinding, randomizer, ak, nk }
let bad_proof = SpendProof::prove(r, s, &PROVING_KEY, public, private)

spend.proof = bad_proof  // sahte proof'u ata
// check_stateless → "spend proof did not verify" hatası
```

### Sistem Davranış Notları
- `check_stateless` → `check_historical` → `check_and_execute` üçlü kontrol akışı
- ZK proof doğrulama `check_stateless`'ta yapılır (state bağımsız)
- Debug modda constraint system satisfaction assertion panikler
- Yorumlı çift harcama testleri (`/* ... */`) — gelecekteki regression test'ler için

### Nova Shield Chain İçin Dersler
- **Üçlü kontrol akışı kritik:** `check_stateless` (proof doğrulama) → `check_historical` (geçmiş state ile tutarlılık) → `check_and_execute` (state güncelleme). Bu ayrım DDoS koruması için şart: state'e bakmadan önce ZK proof reddet.
- **Zellic audit**: Sıfır değerli not ile sahte proof saldırısı. Nova Shield'da benzer attack vector için test yaz.
- **`#[cfg_attr(debug_assertions, should_panic)]`**: Debug/release farklılığını kabul etmek zorunda kalmak kötü işaret — mümkünse her modda aynı davranış sağla.
- `EffectHash` — tx'in tüm etkilerinin hash'i. ZK proof bu hash'i bağlar: imza değiştirme tx'i geçersiz kılar.

---

## [swap_and_swap_claim.rs]
**Dosya:** `.../tests/swap_and_swap_claim.rs`
**Satır Sayısı:** 337

### Genel Amaç
DEX swap ve swap claim (swap sonucu token alma) işlemlerini uçtan uca test eder.

### Test Senaryoları

#### `swap_and_swap_claim`
**Adımlar:**
```rust
// BeginBlock
state_tx.put_epoch_by_height(1, Epoch { index: 0, start_height: 0 })
state_tx.put_block_height(1)

// Swap action:
let trading_pair = TradingPair::new(gm.id(), gn.id())
let plaintext = SwapPlaintext::new(&mut rng, trading_pair, delta_1=100_000, delta_2=0, fee, address)
let swap = SwapPlan::new(&mut rng, plaintext).swap(&FVK)
swap.check_stateless(()).await?
swap.check_historical(state.clone()).await?
swap.check_and_execute(&mut state_tx).await?

// EndBlock — swap burada çalıştırılır:
Dex::end_block(&mut state, &end_block).await
ShieldedPool::end_block(&mut state, &end_block).await
state_tx.finish_block().await

// SwapClaim:
let output_data = state.output_data(height, trading_pair).await?.unwrap()
let swap_auth_path = client.witness_commitment(commitment).unwrap()
let detected_plaintext = client.swap_by_commitment(&commitment).unwrap()
assert plaintext == detected_plaintext  // swap tespit edildi

let claim_plan = SwapClaimPlan {
    swap_plaintext: plaintext,
    position: swap_auth_path.position(),
    output_data,
    epoch_duration,
    proof_blinding_r: Fq::rand(&mut rng),
    proof_blinding_s: Fq::rand(&mut rng),
}
let claim = claim_plan.swap_claim(&FVK, &swap_auth_path)

// SwapClaim anchor = client'ın bildiği son SCT kökü:
let context = Transaction { anchor: client.latest_height_and_sct_root().1, ..Default::default() }.context()
claim.check_stateless(context).await?
claim.check_historical(state.clone()).await?
claim.check_and_execute(&mut state_tx).await?
```

#### `swap_with_nonzero_fee`
Sıfır olmayan fee ile swap:
```rust
let fee = Fee::from_staking_token_amount(Amount::from(1u64))
// Aynı akış, fee kısımları için ek doğrulama
```

### Sistem Davranış Notları
- **Swap yürütme gecikme:** Swap işlemi blok içinde sadece kaydedilir; asıl yürütme `EndBlock`'ta yapılır (batch execution)
- `output_data(height, trading_pair)` — o blokta o trading pair için çıktı fiyatını döndürür
- `swap_by_commitment(commitment)` — client'ın tespit ettiği swap plaintext'i
- `finish_block()` — SCT'yi finalize eder, swap claim için gerekli auth path üretilir

### DEX Uygulama Notu — Nova Shield DEX için
Bu test DEX'in temel akışını ortaya koyuyor:
- **Batch execution:** Tüm swaplar EndBlock'ta toplu işlenir — MEV koruması için kritik
- `SwapPlaintext` — gizli swap detayları (miktar, trading pair, fee, address): ZK proof ile gizlenir
- `output_data(height, pair)` — batch swap sonrası clearing price. Her (height, pair) için tek bir clearing price → tüm kullanıcılara aynı fiyat
- `SwapClaimPlan.proof_blinding_*` — claim proof'u için blinding faktörleri. Nova Shield DEX'te swap claim ZK proof zorunlu.
- Sıfır fee desteklenir ama nonzero fee de test edilmeli — fee accounting ayrı test senaryosu gerektirir.

---

## [view_server_can_be_served_on_localhost.rs]
**Dosya:** `.../tests/view_server_can_be_served_on_localhost.rs`
**Satır Sayısı:** 218

### Genel Amaç
View server'ın localhost üzerinde gRPC ile erişilebilir olduğunu ve `Planner` API'si üzerinden tx oluşturulabildiğini test eder.

### Test Senaryoları

#### `view_server_can_be_served_on_localhost`
**NOT:** `#[ignore = "Flaked in #4627,#4632,#4624"]` — port çakışması kaynaklı flakiness

**Adımlar:**
1. Mock tendermint proxy + gRPC sunucu (127.0.0.1:8080)
2. `ViewServer::load_or_initialize(None, None, &FVK, grpc_url)`
3. `ViewClient::status_stream()` ile sync
4. `view_client.unspent_notes_by_address_and_asset()` — note listesi
5. `Planner` ile tx planı:
   ```rust
   let mut planner = Planner::new(OsRng)
   planner
       .set_gas_prices(gas_prices)
       .spend(note, position)
       .output(value, ADDRESS_1)
       .plan(&mut view_client, AddressIndex::default())
       .await
   ```
6. Tx çalıştır
7. Nullifier ve note tespit doğrula

**StatusResponse Doğrulaması:**
```rust
// 10 blok fast_forward sonrası:
assert status == StatusResponse {
    full_sync_height: 10,
    partial_sync_height: 10,
    catching_up: false
}
// Tx sonrası:
assert status.full_sync_height == 11
```

**Post-Tx Not Durumu:**
```rust
// Kaynak adres boş:
assert post_tx_notes[AddressIndex::default()][STAKING_TOKEN].is_none()
// Hedef adres dolu:
assert post_tx_notes[AddressIndex::from(1)][STAKING_TOKEN].len() == Some(1)
```

### Sistem Davranış Notları
- `Planner` yüksek seviyeli tx oluşturma API'si — otomatik input seçimi ve gas hesaplama
- `gas_prices` view server'dan alınır → `GasPricesRequest {}`
- `AddressIndex` — aynı spend key'den türetilen farklı adresler (privacy için)
- `load_or_initialize(None, None, &FVK, grpc_url)` — disk'e yazmadan view server başlatma

### Nova Shield Chain İçin Dersler
- `Planner` API pattern: kullanıcı "spend X to Y" diyebilmeli, altyapı input seçimi ve fee hesaplamasını yönetmeli. Nova Shield cüzdan SDK'sı için kritik.
- `unspent_notes_by_address_and_asset()` → `BTreeMap<AddressIndex, BTreeMap<AssetId, Vec<Note>>>` — adres ve asset türüne göre gruplu note indeksi.
- `status_stream()` — uzun süreli sync işlemi için streaming API. Sync tamamlanana kadar bekle pattern'i.
- Birden fazla adres indeksi: `AddressIndex::from(0)`, `AddressIndex::from(1)` — aynı cüzdandan farklı gizlilik seviyeleri.

---

## Genel Sistem Davranış Özeti

### Mock Consensus Motoru Mimarisi

```
TestNode::builder()
    .single_validator()          // keyring'e tek validatör ekle
    .with_penumbra_auto_app_state(app_state)  // genesis validatörleri enjekte et
    .with_initial_timestamp(t)   // başlangıç zamanı
    .ts_callback(|t| t + duration)  // blok zaman artışı
    .on_block(proxy.on_block_callback())  // her blok sonrası callback
    .init_chain(consensus)       // genesis başlat
    .await
```

### Blok Yürütme Akışı
```
node.block()
    [.add_tx(bytes)]             // tx ekle (opsiyonel)
    [.with_data(vec![bytes])]    // tx listesi (alternatif)
    [.without_signatures()]      // imzasız blok (uptime testi)
    .execute().await
```

### State Snapshot Sistemi
```
// Her blok sonrası yeni snapshot:
pre_tx_snapshot = storage.latest_snapshot()
tx çalıştır
post_tx_snapshot = storage.latest_snapshot()
// Karşılaştır
```

### DEX Yürütme Zamanlaması
```
Blok N: swap.check_and_execute()  // swap kaydedildi
EndBlock N: Dex::end_block()       // swap toplu yürütüldü, clearing price belirlendi
Blok N+1: SwapClaim              // çıktı token alındı
```

### VCB Bütünlüğü
```
Her swap/deposit/withdrawal → VCB güncelleme
VCB tutarsızlığı → potansiyel chain halt
Test: N işlem → VCB = N * amount (linear accumulation)
```

---

## Nova Shield Chain İçin Kritik Eylem Planı

### Doğrudan Uygulanabilecekler

1. **Mock Consensus Altyapısı:**
   - `TestNode` benzeri bir yapı: `ts_callback`, `keyring`, `on_block`, `fast_forward`
   - `TempStorageExt`: genesis başlatma, prefix yönetimi
   - `BuilderExt`: test için otomatik genesis state üretimi

2. **Üçlü Kontrol Akışı (ZK Spend):**
   ```
   check_stateless (proof) → check_historical (state) → check_and_execute (state mutate)
   ```
   Bu ayrım DDoS koruması için şart.

3. **Epoch-Tabanlı Delegasyon:**
   - Delegasyon/undelegasyon etkileri yalnızca epoch sınırında geçerli
   - `State::Defined → Active → Defined` geçişleri
   - Uptime takibi yalnızca `Active` validatörler için

4. **DEX Batch Execution:**
   - Swap'lar block içinde kaydedilir, EndBlock'ta toplu yürütülür
   - Tek clearing price per (block, trading_pair) — tüm kullanıcılara aynı fiyat
   - `SwapClaim` için ZK proof

5. **VCB Bütünlüğü:**
   - Her bileşen kendi value circuit breaker'ını tutar
   - `aggregate = delta + aggregate` — çift sayım yasak
   - Erken kapatma senaryoları özellikle test edilmeli

6. **IBC Entegrasyonu:**
   - "07-tendermint" client type
   - ICS-20 token transfer (port: "transfer", version: "ics20-1")
   - Proof height = last_committed_height + 1

### Nova Shield'da Farklı Yapılması Gerekenler

1. **Flaky test'ler:** Port çakışması sorunlarını port allocation ile çöz — sabit port yerine OS'tan dinamik port al.

2. **Debug/Release farklılığı:** `#[cfg_attr(debug_assertions, should_panic)]` kötü işaret — ZK proof doğrulama her modda aynı davranmalı.

3. **Promote edilen validatörün 1 kaçırılan bloğu:** Bu off-by-one'u başından doğru uygula — uptime tracking'i `h-1` commit değil, `h` commit üzerinden yap.

4. **Governance key ayrımı:** Genesis testlerinde `identity_key == governance_key` ama production'da farklı olmalı. Nova Shield genesis konfigürasyonunda bu uyarıyı belgele.

### Kritik Güvenlik Testleri (Mutlaka Yazılmalı)

1. Sıfır değerli not ile sahte spend proof (Zellic audit #3859 benzeri)
2. DEX VCB çift sayım regression (bug #4643)
3. Auction VCB erken kapatma (testnet-75 bug)
4. Geçersiz validatör auth_sig sessizce reddedilmeli
5. Community pool spend disabled iken governance teklifi reddedilmeli

---

## Kullanılan Önemli Kütüphaneler ve Bağımlılıklar

| Kütüphane | Amaç |
|---|---|
| `cnidarium` | JMT tabanlı verifiable storage |
| `penumbra_sdk_mock_consensus` | CometBFT mock |
| `penumbra_sdk_mock_client` | Shielded wallet mock |
| `decaf377_rdsa` | Penumbra'nın decaf377 eğrisi üzerinde RDSA |
| `ed25519_consensus` | CometBFT consensus key |
| `rand_chacha` | Deterministik RNG (testler için) |
| `tower` / `axum` | gRPC/HTTP sunucu |
| `ibc_types` / `ibc_proto` | IBC protokol tipleri |
| `tap` crate | `.tap()` / `.tap_ok()` — inspect without consuming |

---

## Test Altyapısı Tasarım Desenleri

| Pattern | Açıklama | Nova Shield Uygulaması |
|---|---|---|
| `tap(|_| drop(x))` | Sıralı kaynak temizleme | `drop` sırası: node → storage → guard |
| `debug_assert_eq!` | Release'de çalışmayan assert | Epoch/sync doğrulama için kullan |
| `Instrument::instrument(error_span!)` | İşlem span'larıyla tracing | Her önemli tx için |
| `seed_from_u64` deterministik RNG | Tekrarlanabilir test | Test RNG'lerinde `ChaChaRng::seed_from_u64` |
| `#[tokio::test(flavor = "multi_thread")]` | Multi-thread test | View server + gRPC testleri için |
| `#[ignore = "..."]` | Flaky test işaretleme | Port/race condition sorunları için |
| `TryInto::try_into().map_err(|keys| ...)` | Exact count destructuring | `[a, b] = vec.try_into()` pattern |
