# substrate/client/consensus + network — Tam Analiz
**Agent**: 20 | **Tarih**: 2026-07-22

---

## KAPSANAN DOSYALAR

### Consensus
- `substrate/client/consensus/babe/src/lib.rs` (~1237 satır)
- `substrate/client/consensus/babe/src/authorship.rs` (326 satır)
- `substrate/client/consensus/babe/src/aux_schema.rs` (220 satır)
- `substrate/client/consensus/babe/src/verification.rs` (264 satır)
- `substrate/client/consensus/grandpa/src/lib.rs` (~1237 satır)
- `substrate/client/consensus/grandpa/src/environment.rs` (~800 satır)
- `substrate/client/consensus/grandpa/src/justification.rs` (316 satır)
- `substrate/client/consensus/grandpa/src/authorities.rs` (~600 satır)
- `substrate/client/consensus/grandpa/src/import.rs` (~500 satır)
- `substrate/client/consensus/grandpa/src/communication/mod.rs` (~400 satır)
- `substrate/client/consensus/grandpa/src/communication/gossip.rs` (~1200 satır, ilk 150 satır)
- `substrate/client/consensus/grandpa/src/voting_rule.rs` (~200 satır)
- `substrate/client/consensus/aura/src/lib.rs` (~400 satır)
- `substrate/client/consensus/aura/src/standalone.rs` (~200 satır)
- `substrate/client/consensus/slots/src/lib.rs` (~450 satır)
- `substrate/client/consensus/slots/src/slots.rs` (223 satır)
- `substrate/client/consensus/slots/src/aux_schema.rs` (224 satır)
- `substrate/client/consensus/common/src/import_queue/basic_queue.rs` (~300 satır)

### Network
- `substrate/client/network/sync/src/engine.rs` (~800 satır)
- `substrate/client/network/sync/src/lib.rs` (46 satır)
- `substrate/client/network/sync/src/strategy/chain_sync.rs` (~200 satır)
- `substrate/client/network/sync/src/strategy/warp.rs` (~100 satır)
- `substrate/client/network/sync/src/block_request_handler.rs` (~80 satır)
- `substrate/client/network/common/src/sync.rs` (55 satır)
- `substrate/client/network/transactions/src/lib.rs` (~150 satır)

---

## BOLUM 1: BABE Client Implementation

### 1.1 Block Authoring Akisi

BABE block authoring `start_babe()` ile baslatilir (lib.rs:466). Temel akis:

1. `Slots::next_slot()` — slot zamanlayicisi, Unix epoch zamanini slot duration ile bolarak mevcut slot numarasini hesaplar. (slots.rs:42-47)
2. Her slot icin `SlotWorker::on_slot()` cagirilir (slots/src/lib.rs:281)
3. `SimpleSlotWorker::on_slot()` icinde sirasyla:
   - `aux_data()` — epoch bilgisi alinir
   - `claim_slot()` cagrilir — once primary slot, sonra secondary slot
   - Eger slot claim edilebiliyorsa `proposer()` olusturulur
   - `propose()` ile blok olusturulur (max_duration = kalan_sure * 0.98, lib.rs:197)
   - `block_import_params()` ile seal eklenir
   - `block_import().import_block()` ile blok yerele yazilir

**Fork secimi**: Fork choice rule agirlik tabanlidir. Agirlik = zincirdeki primary blok sayisi. Esitlik durumunda daha uzun zincir kazanir. (lib.rs:59-62)

### 1.2 VRF Slot Claim Mekanizmasi

**Primary slot (VRF-tabanli)** — authorship.rs:227-270:

```
threshold = calculate_primary_threshold(c, authorities, authority_index)
data = make_vrf_sign_data(randomness, slot, epoch_index)
vrf_signature = keystore.sr25519_vrf_sign(AuthorityId::ID, authority_id, data)
score = authority_id.make_bytes::<16>(AUTHORING_SCORE_VRF_CONTEXT, data, vrf_signature.pre_output)
can_claim = u128::from_le_bytes(score) < threshold
```

**Threshold hesabi** (authorship.rs:37-97):
```
theta = authority_weight / total_weight   // goreli agirlik
p = 1 - (1 - c)^theta                    // slot sahibi olma olasiligi
threshold = (2^128) * p                  // u128 olarak normalize
```
`c` parametresi: slot doluluk olasiligi. `1-c` = bos slot olasiligi.
`AUTHORING_SCORE_VRF_CONTEXT = b"substrate-babe-vrf"` (lib.rs:156)
`AUTHORING_SCORE_LENGTH = 16` (bayt, 128-bit u128 icin) (lib.rs:159)

**Secondary slot (deterministik)** — authorship.rs:102-123:
```
rand = blake2_256(epoch_randomness ++ slot_number)
idx = U256::from_big_endian(rand) % authorities_len
expected_author = authorities[idx]
```
Slot claim olmamissa ve `SecondaryPlain` veya `SecondaryVRF` izin verilmisse fallback uygulama.
(authorship.rs:206-220)

### 1.3 Epoch Yonetimi

**Epoch yapisi** (lib.rs:163-211):
- `epoch_index: u64` — epoch numarasi
- `start_slot: Slot` — baslagic slotu
- `duration: u64` — epoch uzunlugu (slot sayisi)
- `authorities: Vec<(AuthorityId, BabeAuthorityWeight)>` — yetkili seti
- `randomness: [u8; 32]` — VRF icin rastgelelik
- `config: BabeEpochConfiguration { c, allowed_slots }`

**Epoch N+2 kurali** (lib.rs:33-34):
Epoch N biterken N+2 icin parametreler (randomness, authorities) duyurulur. Dogrudan N+1 degil, ileriden duyuru yapilir.

**Epoch increment** (lib.rs:189-201):
```rust
fn increment(&self, (descriptor, config)) -> Epoch {
    sp_consensus_babe::Epoch {
        epoch_index: self.epoch_index + 1,
        start_slot: self.start_slot + self.duration,
        ...
    }
}
```

**clone_for_slot** (lib.rs:239-262):
Bir slot birden fazla epoch atliyorsa:
```
skipped_epochs = (slot - start_slot) / duration
epoch_index += skipped_epochs
start_slot += skipped_epochs * duration
```

### 1.4 Auxiliary Storage (Epoch Data)

**Depolama anahtarlari** (aux_schema.rs:34-36):
```rust
const BABE_EPOCH_CHANGES_VERSION: &[u8] = b"babe_epoch_changes_version";
const BABE_EPOCH_CHANGES_KEY: &[u8] = b"babe_epoch_changes";
const BABE_EPOCH_CHANGES_CURRENT_VERSION: u32 = 3;
```

**Migrasyon zinciri** (aux_schema.rs:62-84):
- Version yok → EpochV0 → Epoch (v3)
- Version 1 → EpochChangesV1For (EpochV0) → Epoch (v3)
- Version 2 → EpochChangesV1For (Epoch) → v3 (migrate())
- Version 3 → mevcut format

**Block weight depolama** (aux_schema.rs:39-41):
```
block_weight_key(block_hash) = ("block_weight", block_hash).encode()
```
Zincir agirligi (primary blok sayisi) her blok hashiyle eslestirilir.

### 1.5 Pre-digest ve Seal

**Pre-digest turleri** (authorship.rs:26-27):
- `PreDigest::Primary(PrimaryPreDigest { slot, authority_index, vrf_signature })`
- `PreDigest::SecondaryPlain(SecondaryPlainPreDigest { slot, authority_index })`
- `PreDigest::SecondaryVRF(SecondaryVRFPreDigest { slot, authority_index, vrf_signature })`

**Seal**: Header hash (pre-hash, seal cikarilmis hali) `AuthorityPair` ile imzalanir.
Seal, header digest listesinin SON elemanidir — bu guvenlik gerektiriyor, degistirilemez.
(verification.rs:61-62, 75-78)

**Header dogrulama akisi** (verification.rs:67-133):
1. Pre-digest bul
2. Son digest elemani cikar (seal)
3. `pre_digest.slot() > slot_now` ise `CheckedHeader::Deferred` don
4. Slot turune gore dogrulama: Primary → VRF + threshold, SecondaryPlain → deterministik yazar kontrolu, SecondaryVRF → hem yazar hem VRF

### 1.6 Equivocation Reporting

**Tespit** (slots/src/aux_schema.rs:53-133):
```
MAX_SLOT_CAPACITY = 1000  // kapasite siniri
PRUNING_BOUND = 2000      // temizleme esigi
```
Ayn slotta iki farkli header varsa ve imzalayanlar eslesmisse `EquivocationProof` olusturulur.
Eski slotlar (`slot_now - slot > MAX_SLOT_CAPACITY`) kontrol edilmez.

**Raporlama** (environment.rs:497-589):
1. Kendi equivocasyonumuz ise raporlama — Safety hatasi don
2. `authority_set.next_change()` ile mevcut setin son blogunu bul
3. `runtime_api.generate_key_ownership_proof(hash, set_id, offender)` ile uyelik kaniti uret
4. `submit_report_equivocation_unsigned_extrinsic()` ile on-chain gonder

---

## BOLUM 2: GRANDPA Client Implementation

### 2.1 Voter State Machine

**VoterSetState** (environment.rs:177-190):
```rust
pub enum VoterSetState<Block: BlockT> {
    Live {
        completed_rounds: CompletedRounds<Block>,  // son N tamamlanan round
        current_rounds: CurrentRounds<Block>,       // aktif roundlardaki oy durumu
    },
    Paused {
        completed_rounds: CompletedRounds<Block>,
    },
}
```

**NUM_LAST_COMPLETED_ROUNDS = 2** (environment.rs:98)
Son 2 round'un verisi saklanir.

**HasVoted** (environment.rs:257-317):
```rust
pub enum HasVoted<Header: HeaderT> {
    No,
    Yes(AuthorityId, Vote<Header>),
}
pub enum Vote<Header: HeaderT> {
    Propose(PrimaryPropose<Header>),
    Prevote(Option<PrimaryPropose<Header>>, Prevote<Header>),
    Precommit(Option<PrimaryPropose<Header>>, Prevote<Header>, Precommit<Header>),
}
```

**VoterWork** (lib.rs:881): Ana future. `poll()` methodu:
1. Voter future'ini poll et
2. Voter command kanalini poll et (dis komutlar)
3. Network bridge'i poll et

### 2.2 Prevote / Precommit Akisi

GRANDPA finality-grandpa kutuphanesini temel alir. `voter::Voter::new()` ile baslatilir (lib.rs:1020-1028):
```rust
let voter = voter::Voter::new(
    self.env.clone(),
    (*self.env.voters).clone(),
    global_comms,
    last_completed_round.number,
    last_completed_round.votes.clone(),
    last_completed_round.base,
    last_finalized,
);
```

**Gossip mesaj tipleri** (communication/gossip.rs:43-85):
- `Prevote` / `Precommit` — round icindeki oylar
- `PrimaryPropose` — onceki roundun tahminini yayan mesaj
- `Commit` — gecmis finality anlasmalarini duyurur
- `CatchUp` — geri kalmis peer'in ilerlemesi icin
- `Neighbor Packet` — round, set_id, son finalize hash bilgisi

**ROUND_DURATION = 5** gossip periyodu (gossip.rs:121):
- 2 periyot prevote timer
- 2 periyot precommit timer
- 1 periyot precommit yayilimi

**Gossip propagasyon stratejisi** (gossip.rs:125-133):
- `LUCKY_PEERS = 4` fanout (ilk asama)
- `PROPAGATION_SOME = 1.5` round sonrasi: daha fazla node'a yay
- `PROPAGATION_ALL = 3.0` round sonrasi: tum bagli node'lara yay

### 2.3 Finality Threshold

Threshold hesabi finality-grandpa external kutuphanesinde. GRANDPA'da finality icin **2/3 + 1 supermajority precommit** gerekli (VoterSet agirligi bazli). `validate_commit()` ile dogrulanir (justification.rs:178-185):
```rust
match finality_grandpa::validate_commit(&commit, voters, &ancestry_chain) {
    Ok(ref result) if result.is_valid() => {},
    _ => return Err(ClientError::BadJustification(...)),
}
```

### 2.4 Authority Set Degisimi

**Iki tur degisim** (authorities.rs:148-171, import.rs:155-168):

**Standard change (DelayKind::Finalized)**:
- `ConsensusLog::ScheduledChange` — header'da gorunur
- Belirli bir blokta gecerli olur ve finality ile aktive edilir
- `pending_standard_changes: ForkTree<H, N, PendingChange>` — catal agaci
- Birden fazla standard change farkli fork'larda olabilir ama sirali olmali

**Forced change (DelayKind::Best { median_last_finalized })**:
- `ConsensusLog::ForcedChange` — header'da gorunur
- `pending_forced_changes: Vec<PendingChange>` — fork basina bir adet
- `apply_forced_changes()` — best block depth'e gore hemen uygulanir, finality beklenmez
- Uygulanmadan once bagli standard change'ler tamamlanmali
- Forced change sonrasi voter durdurulur (do_pause = true, import.rs:346-347)

**Effective number** = `canon_height + delay`

**Set ID artisi**: Her degisimde `set_id` bir artar. `AuthoritySetChanges` ile hangi bloklarda degisim oldugu takip edilir.

**Hard fork destegi**: Off-chain tanimlanmis authority set degisimleri icin `AuthoritySetHardFork` yapisi (lib.rs:540-553). Gecikme sifir olan bir `PendingChange` olusturulur.

### 2.5 Justification Yapisi

**GrandpaJustification struct** (justification.rs:42-46):
```rust
pub struct GrandpaJustification<Block: BlockT> {
    pub justification: sp_consensus_grandpa::GrandpaJustification<Block::Header>,
    _block: PhantomData<Block>,
}
// Icerik:
//   round: u64
//   commit: Commit<Header>   // target_hash, target_number, precommits
//   votes_ancestries: Vec<Header>  // ancestry kaniti
```

**Olusturma** (justification.rs:67-126):
1. En dusuk hedef bloku baz olarak sec (base_hash)
2. Her precommit'in base'e giden ata zincirini votes_ancestries'e ekle
3. Ayni hash iki kez eklenmez (HashSet kontrolu)

**Dogrulama** (justification.rs:130-264):
1. `decode_all()` — SCALE decode
2. Target hash/number eslesmesi kontrol
3. `validate_commit()` — finality-grandpa ile supermajority dogrulamasi
4. Her precommit icin signature kontrol: `check_message_signature_with_buffer(Message::Precommit, id, sig, round, set_id)`
5. `ancestry_chain.ancestry(base, target)` — ata zinciri gecerliligi
6. `visited_hashes == ancestry_hashes` — fazla header yok kontrolu

**Pruning Filter** (lib.rs:155-167): GRANDPA justification'li bloklar `should_retain()` ile budanmaz — warp sync icin gerekli.

### 2.6 Catch-up Mekanizmasi

**CATCH_UP_THRESHOLD = 2** (gossip.rs:115): Bir peer bizden 2+ round ilerideyse catch-up istegi gonderilir.

**CATCH_UP_REQUEST_TIMEOUT = 45s** (gossip.rs:111)
**CATCH_UP_PROCESS_TIMEOUT = 30s** (gossip.rs:112)

Catch-up istegi yalnizca ayn voter set ID'ye sahip peerlere gonderilir.

### 2.7 GRANDPA Networking

**Protokol adi** (communication/mod.rs:79-97):
```
NAME = "/grandpa/1"
LEGACY_NAMES = ["/paritytech/grandpa/1"]
standard_name = "/{genesis_hash}[/{fork_id}]/grandpa/1"
```

**Notification config** (lib.rs:724-745):
- Max mesaj boyutu: `1024 * 1024` (1MB) — Kusama/Polkadot'ta ~256KB mesajlar gorulmus
- `in_peers = 0, out_peers = 0` — slot sayisi yok, reserved modunda
- `NonReservedPeerMode::Deny` — yalnizca reserved peer'ler kabul edilir

**NEIGHBOR_REBROADCAST_PERIOD = 2 dakika** (communication/mod.rs:73)

**Gossip maliyetleri** (communication/mod.rs):
- `PAST_REJECTION = -50`
- `BAD_SIGNATURE = -100`
- `MALFORMED_CATCH_UP = -1000`
- `INVALID_CATCH_UP = -5000`
- `INVALID_COMMIT = -5000`
- `FUTURE_MESSAGE = -500`

**REBROADCAST_AFTER = 5 dakika** (gossip.rs:110): Oy mesajlari 5 dakika sonra yeniden yayinlanir.

### 2.8 VoterWork Akisi

`handle_voter_command()` (lib.rs:1045-1118):
- `VoterCommand::ChangeAuthorities`: yeni VoterSetState olustur, `rebuild_voter()` cagir
- `VoterCommand::Pause`: `VoterSetState::Paused` yaz, `rebuild_voter()` cagir

`rebuild_voter()` (lib.rs:965-1043): Yeni `voter::Voter` ornegi olusturulur, `shared_voter_state` guncellenir (RPC endpoint icin).

---

## BOLUM 3: AURA Client

### 3.1 Slot Authorship

**Slot → Yazar eslesmesi** (standalone.rs:71-87):
```rust
pub fn slot_author<P: Pair>(slot: Slot, authorities: &[AuthorityId<P>]) -> Option<&AuthorityId<P>> {
    let idx = *slot % (authorities.len() as u64);
    authorities.get(idx as usize)
}
```

**Tam formul**: `author = authorities[slot % n_authorities]`
Her yetkiliye tek bir slot atanir, birden fazla yazar yoktur (VRF'siz deterministik).

**Slot claim** (standalone.rs:93-105):
```rust
pub async fn claim_slot<P: Pair>(slot, authorities, keystore) -> Option<P::Public> {
    let expected_author = slot_author::<P>(slot, authorities);
    expected_author.and_then(|p| {
        if keystore.has_keys(&[(p.to_raw_vec(), key_types::AURA)]) {
            Some(p.clone())
        } else { None }
    })
}
```

### 3.2 BABE'dan Farklari

| Ozellik | BABE | AURA |
|---------|------|------|
| Slot atama | VRF (rastgele) | Deterministik (round-robin) |
| Bos slot olasiligi | Var (c parametresi) | Yok (her slotta yazar var) |
| Secondary slot | Var (SecondaryPlain/VRF) | Yok |
| Fork secimi | Agirlik (primary blok sayisi) | En uzun zincir |
| Randomness | On-chain VRF birikimi | Yok |
| Guvenlik | PoS benzeri, tahmin edilemez | Daha basit ama tahmin edilebilir |

**CompatibilityMode** (aura/src/lib.rs:82-106): Eski zincirlerde `initialize_block` oncesi cagrim gereksini icin uyumluluk kipi.

---

## BOLUM 4: Slots — Ortak Altyapi

### 4.1 SlotWorker Trait (slots/src/lib.rs:62-68)

```rust
#[async_trait::async_trait]
pub trait SlotWorker<B: BlockT> {
    async fn on_slot(&mut self, slot_info: SlotInfo<B>) -> Option<B>;
}
```

### 4.2 SimpleSlotWorker Trait (slots/src/lib.rs:75-440)

Ortak akis `on_slot()`:
1. `proposing_remaining_duration()` — kalan sure hesapla
2. `aux_data()` — epoch/yetki verisi al
3. `sync_oracle().is_offline()` → eger cevrimdiside ve >1 yetki varsa atla
4. `claim_slot()` — slot sahipligini dene
5. `should_backoff()` — geri cekme stratejisi kontrol
6. `proposer()` — proposer olustur
7. `propose()` ile blok olustur (max_duration * 0.98 siniri, lib.rs:197)
8. `block_import_params()` — import parametresi
9. `block_import().import_block()` — blogu iceride islet

### 4.3 Slot Zamanlama (slots.rs:42-48)

```rust
pub fn time_until_next_slot(slot_duration: Duration) -> Duration {
    let now = duration_now().as_millis();
    let next_slot = (now + slot_duration.as_millis()) / slot_duration.as_millis();
    let remaining_millis = next_slot * slot_duration.as_millis() - now;
    Duration::from_millis(remaining_millis as u64)
}
```
Unix epoch bolunerek hesaplama. Guvenilirlik icin `futures_timer::Interval` yerine elle yapilir.

**Ayn slot iki kez verilmez** (slots.rs:208-210): `slot > last_slot` kontrolu.

**Major sync atlamasi** (slots.rs:171-174): `sync_oracle.is_major_syncing()` ise slot atlanir.

---

## BOLUM 5: Network

### 5.1 Protokol Listesi

**Notification protokolleri**:
1. **Block announces**: `/{genesis_hash}[/{fork_id}]/block-announces/1` (engine.rs:92-107)
2. **Transactions**: `/{genesis_hash}[/{fork_id}]/transactions/1` (transactions/src/lib.rs:146-150)
3. **GRANDPA**: `/{genesis_hash}[/{fork_id}]/grandpa/1` (communication/mod.rs:86-97)

**Request-Response protokolleri**:
- Block requests: `block_request_handler.rs` — `MAX_BLOCKS_IN_RESPONSE = 128` (block_request_handler.rs:61)
- State requests: `state_request_handler.rs`
- Warp sync: `warp_request_handler.rs`
- Light requests: `light/` (ayri modul)

### 5.2 Peer Discovery

**libp2p entegrasyonu**: `NetworkBackend` trait ile soyutlanmis. Ozel discovery mekanizmasi bu katmanda degil, libp2p altinda (Kademlia DHT + mDNS).

**Boot nodes** (engine.rs:331-338): Genesis belgeden alinan boot node'lar HashSet olarak saklanir.

**Peerset yonetimi** (engine.rs:238-285):
- `default_peers_set_num_full`: tam dugum sayisi
- `default_peers_set_num_light`: hafif dugum sayisi
- `max_in_peers`: max giris peer = max_full - max_out
- `no_slot_peers`: reserved node'lar slot almaz
- `dynamic_no_slot_peers`: runtime'da eklenebilir no-slot peer'ler

### 5.3 Block Announces Protokolu

**BlockAnnouncesHandshake** (engine.rs:60-62): Baglanti kurulurken:
- roles: Roles (Authority, Full, Light)
- best_number, best_hash
- genesis_hash

**Genesis mismatch = fatal rep degisimi** (engine.rs:113): `Rep::new_fatal("Genesis mismatch")`

**MAX_KNOWN_BLOCKS = 1024 per peer** (engine.rs:86): LRU set, ~32KB per peer.

**MAX_BLOCK_ANNOUNCE_SIZE = 1MB** (engine.rs:89)

**Announce isleme** (engine.rs:451-483):
- `BlockAnnounceValidationResult::Process` → strategy'e bildir, peer info guncelle
- `BlockAnnounceValidationResult::Failure` → peer cikart ve rep dusur
- `BlockAnnounceValidationResult::Skip` → yoksay

**TICK_TIMEOUT = 1100ms** (engine.rs:83): Periyodik bakim araligi.

### 5.4 Transaction Protokolu

**Protocol adi**: `/{genesis_hash}/transactions/1` (transactions/src/lib.rs:146-150)

**Rep degisimleri**:
- `ANY_TRANSACTION = -16` (baslangrca, dogrulama maliyeti)
- `ANY_TRANSACTION_REFUND = +16` (gecersiz degilse iade)
- `GOOD_TRANSACTION = +128` (yeni, gecerli tx)
- `BAD_TRANSACTION = -4096` (kotu tx)

### 5.5 Sync Pipeline

**SyncMode turleri** (common/src/sync.rs:24-37):
- `Full` — tam blok indirme ve dogrulama
- `LightState { skip_proofs, storage_chain_mode }` — son state ile birlikte blok indir
- `Warp` — authority set gecislerini dogrula, son state indir

**Strateji katmani** (`SyncingStrategy` trait):
- `ChainSync` — tam senkronizasyon
- `StateSync` — state bazli senkronizasyon
- `WarpSync` — warp senkronizasyonu

**ChainSync sabitleri** (strategy/chain_sync.rs:81-98):
```
MAX_IMPORTING_BLOCKS = 2048      // import kuyrugu maks
MAX_DOWNLOAD_AHEAD = 2048        // ileriden indirme maks
MAX_BLOCKS_TO_LOOK_BACKWARDS = 1024  // geri bakilacak maks blok
STATE_SYNC_FINALITY_THRESHOLD = 8    // state sync: finalized - 8
MAJOR_SYNC_BLOCKS = 5            // ata arama gecikmesi
```

**WarpSync** (strategy/warp.rs:49-51):
```
MIN_PEERS_TO_START_WARP_SYNC = 3  // baslama icin min peer sayisi
```
Warp proof: encoded kanit. `VerificationResult::Partial` veya `::Complete`.

**BasicQueue** (common/src/import_queue/basic_queue.rs:45-103):
- Arkaplan gorevi olarak calisir (`spawn_essential_blocking`)
- `justification_sender` ve `block_import_sender` ayri kanallar
- `result_port` — sonuclar `BufferedLinkReceiver` uzerinden gelir

### 5.6 Import Queue Mekanizmasi

```
Network → SyncingEngine → ImportQueueService
                              ↓
                      BlockImportWorker (arkaplan task)
                              ↓
                      Verifier::verify()
                              ↓
                      BlockImport::import_block()
                              ↓
                      BufferedLink (sonuc yaz)
                              ↓
                      SyncingEngine (sonuc isle)
```

**IncomingBlock** yapisi: hash, header, body (optional), justification (optional), origin PeerId.

### 5.7 Peer Rep Sistemi

**ChainSync rep maliyetleri** (strategy/chain_sync.rs:100-136):
- `BLOCKCHAIN_READ_ERROR = -(1 << 16)` = -65536
- `GENESIS_MISMATCH = i32::MIN` = fatal
- `VERIFICATION_FAIL = -(1 << 29)` = -536870912
- `BAD_BLOCK = -(1 << 29)` = -536870912
- `NO_BLOCK = -(1 << 29)` = -536870912
- `BAD_JUSTIFICATION = -(1 << 16)` = -65536
- `BAD_MESSAGE = -(1 << 12)` = -4096

---

## BOLUM 6: Common Consensus Altyapisi

### 6.1 BlockImport Trait

`import_queue/block_import.rs`'de tanimli:
- `BlockImportParams { header, body, justifications, post_digests, auxiliary, fork_choice, state_action, ... }`
- `ForkChoiceStrategy::LongestChain` vs `Custom`
- `StateAction::Apply(StorageChanges)` vs `ApplyChanges::Continue`

### 6.2 LongestChain

`common/src/longest_chain.rs`: Tum bloklar arasindan en uzun zinciri secen `SelectChain` implementasyonu.

---

## Kritik Bulgular Ozeti

1. **BABE primary threshold formulu** (authorship.rs:54-96):
   `p = 1 - (1-c)^theta` | `threshold = (2^128) * p` — BigUint kullanilir, float precision sorunu yok

2. **Secondary slot formulu** (authorship.rs:111-116):
   `idx = blake2_256(randomness ++ slot) % n_authorities` — U256 modulo

3. **BABE AUTHORING_SCORE_VRF_CONTEXT = b"substrate-babe-vrf"** (lib.rs:156)
   **AUTHORING_SCORE_LENGTH = 16** (lib.rs:159) — 128-bit comparison

4. **Epoch N+2 kurali** (lib.rs:33-34): N biterken N+2 duyurulur, N+1 degil

5. **Epoch storage version = 3** (aux_schema.rs:36); 3 versiyon migrasyon destegi

6. **Seal son digest olmak zorunda** (verification.rs:61-62): Pre-hash seal olmadan hesaplanir

7. **Block weight key**: `("block_weight", block_hash).encode()` (aux_schema.rs:39-41)

8. **GRANDPA justification yapisi**: `{ round, commit: Commit<Header>, votes_ancestries: Vec<Header> }` (justification.rs:44-46)

9. **GrandpaJustification dogrulama**: visited_hashes == ancestry_hashes (justification.rs:256-261) — fazla header yasak

10. **Standard vs Forced change**: Standard = finality ile, Forced = best block depth ile (authorities.rs:402-406)

11. **CATCH_UP_THRESHOLD = 2** (gossip.rs:115): 2 round geride kalinca catch-up istegi

12. **GRANDPA notification max boyut = 1MB** (lib.rs:734); reserved peer modu (in_peers=0, out_peers=0)

13. **GRANDPA protokol adi** (communication/mod.rs:79-81): `/grandpa/1` | legacy: `/paritytech/grandpa/1`

14. **Block request MAX = 128 blok** (block_request_handler.rs:61)

15. **Warp sync min peer = 3** (strategy/warp.rs:50)

16. **Equivocation window: MAX_SLOT_CAPACITY = 1000 slot** (slots/src/aux_schema.rs:31)

17. **BasicQueue arkaplan task**: `spawn_essential_blocking("basic-block-import-worker")` (basic_queue.rs:92-96)

18. **AURA slot author**: `authorities[slot % n]` (standalone.rs:76) — tam deterministik

19. **Block propose sure siniri**: `kalan_sure * 0.98` (slots/src/lib.rs:197)

20. **NUM_LAST_COMPLETED_ROUNDS = 2** (environment.rs:98) — disk ve memory tasarrufu

---

## Nova Shield Chain Notlari

### Konsensus Tasarimi Icin Kritik Tavsiyeler

**1. BABE vs AURA Secimi**
- BABE: PoS ortaminda daha guvenli (VRF sayesinde ongorulemez). Cok dogru elestirilirse: bos slot riski `c` parametresiyle ayarlanir. Oneri: `c = (3, 10)` yani %30 dolulugun garantilenmesi hedeflenir.
- AURA: Daha basit, deterministic. Kucuk validator setleri icin uygun. Hybrid PoA/PoS icin uygun baslangic.
- Nova Shield icin: Ilk asama AURA, ilerleyen asama BABE hibrid modeli dusunulebilir.

**2. Epoch Süresi**
- BABE'da epoch suresi = slot sayisi. Polkadot'ta tipik 2400 slot = 4 saat.
- Cok kisa epoch → randomness birikmez. Cok uzun epoch → validator rotasyonu yavasz.
- Oneri: 600-1200 slot arasi baslangic.

**3. Slot Süresi**
- Polkadot: 6 saniye/slot. Kusama: 6 saniye. Ethereum: 12 saniye.
- 6 saniye iyi bir denge: yeterli ag yayilim suresi + iyi throughput.

**4. Secondary Slot**
- `SecondaryVRF` modunu kullan: parachainler VRF output'u tuketebilir.
- `PrimaryAndSecondaryVRFSlots` = en iyi doluluk garantisi.

**5. Fork Choice**
- BABE'in agirlik tabanli (primary blok sayisi) fork secimi, uzun blok uretimi olmayan sekonderlerin kazanmasini engeller.
- `ForkChoiceStrategy::Custom` kullanarak ek kural eklenebilir.

**6. GRANDPA Entegrasyonu**
- GRANDPA BABE ile birlikte kullanilir. BABE blok uretir, GRANDPA finalize eder.
- `justification_generation_period` ayari: her kac blokta bir justification uretilsin?
- Warp sync icin GRANDPA justification'larini budama — `GrandpaPruningFilter` zorunlu.

**7. Authority Set Degisimi**
- Scheduled change tercih edilmeli (finality garantisi var).
- Forced change yalnizca emergency durumunda (fork yapabilir, light client takip edemez).
- `AuthoritySetHardFork` — off-chain override mekanizmasi (eski chain kurtarma icin).

**8. Network Protokol Tasarimi**
- Genesis hash + fork_id ile protokol isimlendirme: farkli test agi/mainnet ayrimini kolay saglar.
- GRANDPA reserved peer modu (0 slot): validator gossip'ini kontrol altina alir. Kendi gossip protokolunuzde benzer uygulama dusunun.
- `MAX_BLOCKS_IN_RESPONSE = 128`: dusuk boyutlu request-response; yuksek tutulursa peer timeout riski.

**9. Import Queue**
- `BasicQueue` + `spawn_essential_blocking` → blok import hatalari node'u durdurur (essential task). Kritik.
- Verifier trait'i ozellestirilmeli: kendi consensus dogrulamanizi buraya koyun.

**10. Equivocation Handling**
- `MAX_SLOT_CAPACITY = 1000` slot gecmisi tutulur. Cok dusuk tutulursa eski equivocation kacabilir.
- Raporlama runtime API ile on-chain yapilir — runtime pallet entegrasyonu gerekli (bkz. pallet-babe/pallet-grandpa equivocation handler).

**11. Warp Sync**
- Yeni node'lar icin kritik: tam blok senkronizasyonu yerine warp proof ile binlerce blok atlayabilir.
- `MIN_PEERS_TO_START_WARP_SYNC = 3` — en az 3 peer gerekli.
- `WarpSyncProvider` trait implement edilmeli.

**12. VoterSetState Persistance**
- GRANDPA voter state disk'e yazilir. Node yeniden baslatildiginda ayni round'dan devam eder.
- `NUM_LAST_COMPLETED_ROUNDS = 2` — cok az; eger hizli restart gerekirse bu sayiyi artirabilirsiniz.

**13. Gossip Zamanlamasi**
- `REBROADCAST_AFTER = 5 dakika`: oy mesajlari 5 dakika sonra yeniden yayinlanir. Ag tikanikligi durumunda guvenlik.
- `NEIGHBOR_REBROADCAST_PERIOD = 2 dakika`: komsu paketi yenileme.
- Bu degerler validator sayisina ve slot suresine gore ayarlanmali.

**14. Reputation Sistemi**
- Genesis mismatch = fatal (baglanti hemen kopar). Bunu aynen kopyalayin.
- `BAD_BLOCK = -(1<<29)` cok buyuk ceza. DDoS korumasinda kritik.
- Pozitif rep: `GOOD_TRANSACTION = +128`, validator'in iyi davranis tesviki.

**15. Blok Propose Suresi Siniri**
- `propose_args.max_duration = kalan_sure * 0.98` — %2 marj.
- Bu marj yeterli degilse (yogun hesaplama yapan bir blok): `block_proposal_slot_portion` ve `max_block_proposal_slot_portion` ayarlari gerekli.
