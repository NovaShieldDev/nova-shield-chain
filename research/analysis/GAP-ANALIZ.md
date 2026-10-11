# Nova Shield Chain — Tam Mimari Gap Analizi
# Tüm araştırma repoları okunduktan sonra üretilen bulgular
# Türkçe — Kapsamlı Teknik Analiz

## OKUMA DURUMU
- [x] multi-party-ecdsa (47 .rs) — TAMAMEN OKUNDU
- [x] tss-lib (123 .go) — TAMAMEN OKUNDU
- [x] rolling-shutter (331 .go) — ANA DOSYALAR OKUNDU
- [x] cow-protocol (828 .rs) — ANA DOSYALAR OKUNDU
- [x] penumbra (894 .rs) — ANA DOSYALAR OKUNDU
- [x] go-ethereum (1414 .go) — ANA DOSYALAR OKUNDU
- [x] solana (1152 .rs) — ANA DOSYALAR OKUNDU
- [x] cosmos-sdk (2392 .go) — ANA DOSYALAR OKUNDU
- [x] avalanchego (2876 .go) — ANA DOSYALAR OKUNDU
- [x] polkadot-sdk (5080 .rs) — ANA DOSYALAR OKUNDU

---

## BÖLÜM 1 — REPO'LARDAN ÖĞRENILENLER

### 1.1 multi-party-ecdsa (GG2020 Threshold ECDSA)

**Protokol:** Gennaro-Goldfeder 2020 — t-of-n ECDSA eşik imzalama

**Keygen Protokolü (4 tur):**
- Round0: Parametre hazırlığı
- Round1: u_i sırrı oluştur → VSS payları hesapla → Paillier keypair üret → commitment yayınla → DLN proof gönder
- Round2: Commitment doğrula → DLN proof doğrula → VSS paylarını diğerlerine gönder → FacProof + ModProof
- Round3: xi hesapla (Σ shares) → VSS verify → ECDSA pubkey hesapla → Paillier proof gönder
- Round4: Paillier proof doğrula → LocalKey kaydedilir

**Signing Protokolü (10 tur: 9 offline + 1 online):**
- Round1: k ve γ sıfır gizlisi oluştur → Paillier ile k'yı şifrele → Alice ZK range proof
- Round2: Bob MtA — γ*α hesapla, beta gürültüsü ekle → Bob ZK proof
- Round3: δ_i = k_i*γ_i + Σαij + Σβij hesapla
- Round4: Commitment
- Round5: R noktasını belirle (r = δ^-1 * Σ(γ_i*G))
- Round6: Schnorr + V proof
- Round7: U_i, T_i hesapla (commitment)
- Round8: Decommit
- Round9: U == T kontrol → s_i gönder
- Finalization: s = Σs_i → (r, s, recid) imzası → ECDSA.Verify

**MtA (Multiplicative-to-Additive) Protokolü:**
- AliceInit: k_i'yi Paillier ile şifrele (cA), Alice ZK range proof (|k_i| < q^3)
- BobMid: cB = E(k_i)^γ_j * E(β') — homojen çarpma + toplama, Bob ZK proof
- AliceEnd: cB'yi çöz → α = k_i*γ_j + β (mod q)

**Kullanılan ZK İspatları:**
- `AliceProof` (RangeProofAlice): |a| < q^3 (Fiat-Shamir, Pedersen bağlama, Paillier modülü)
- `BobProof` / `BobProofExt`: MtA doğruluğu
- `PDLwSlackProof`: Q = xG ve c = Enc(pk, x, r) — Pedersen+Paillier ZK
- `DLNProof`: h2 = h1^x (mod Ñ) — 128 iterasyon Sigma protokol
- `FacProof`: N = p*q (safe prime faktorizasyon kanıtı)
- `ModProof`: N modülü kanıtı

**Blame Modülü (GlobalStatePhase5/6/7):**
- Protokol başarısız olunca kötü niyetli tarafı tespit eder
- Phase5: delta_vec yeniden inşa, sapan tarafları bul
- Phase6: miu doğruluğu, ECDDH proofları
- Phase7: s_i kısmi imza doğruluğu

**Güvenlik:**
- Safe Germain primes (Paillier için: p = 2p'+1, p' asal)
- Constant-time Paillier şifre çözme (timing side-channel)
- 200ms timing protection + 20ms jitter (MtA)
- Session binding: SSID = hash(parties || nonce)

---

### 1.2 tss-lib (Binance TSS — GG18/GG20)

**Mimari:**
- `Party` interface: Start(), Update(), Running(), WaitingFor()
- `Round` interface: Start(), Update(), CanAccept(), CanProceed(), NextRound()
- `Parameters`: ec curve, partyIDs, threshold, concurrency, safePrimeGenTimeout, sessionNonce
- `MessageRouting`: From, To, IsBroadcast, IsToOldCommittee flags
- Protobuf mesajlar (WireBytes() ile serileştirme)

**Keygen (4 tur, GG18 spesifikasyonu):**
- Round1: ui oluştur → VSS payları → commitment → DLN proofları → Paillier PK
- Round2: DLN proof doğrula (paralel goroutine) → VSS paylarını gönder → FacProof + ModProof
- Round3: xi hesapla → commitmentları doğrula → VSS verify → FacProof verify → ECDSAPub hesapla → Paillier proof
- Round4: Paillier proof doğrula → LocalPartySaveData çıktı

**LocalPartySaveData:**
- `LocalPreParams`: PaillierSK, NTildei, H1i, H2i, Alpha, Beta, P, Q
- `LocalSecrets`: Xi (gizli pay), ShareID
- `LocalPartySaveData`: ECDSAPub, BigXj[], NTildej[], H1j[], H2j[], Ks, PaillierPKs[]
- `BuildLocalSaveDataSubset()`: t+1 kişilik alt küme için yeniden düzenleme

**Signing (9 tur):**
- signRound1Message1s (p2p), signRound1Message2s (broadcast) ... signRound9Messages
- `NewLocalPartyWithKDD()`: HD key türetimi desteği

**Resharing (7 tur):**
- Eski komite → yeni komite arasında güvenli anahtar aktarımı
- dgRound1-4: Eski ve yeni komite eş zamanlı çalışır
- `ReSharingParameters`: newPartyCount, newThreshold

**EdDSA desteği:**
- Ayrı `eddsa/keygen/` ve `eddsa/signing/` modülleri
- Aynı Round interface üzerinden

---

### 1.3 rolling-shutter (Şifreli Mempool — Threshold ElGamal)

**Genel Mimari:**
- `KeyperCore`: P2P mesajlaşma + PostgreSQL + Ethereum bağlantısı
- `ShutterApp`: Tendermint ABCI uygulaması (on-chain DKG koordinasyonu)
- `EpochKG`: Her epoch için gizli anahtar payı hesaplama
- `KeyShareHandler`: Tetikleyici alınca anahtar paylarını P2P'ye gönderir

**DKG Faz Yönetimi (dkgphase/phase.go):**
- Fazlar: Off → Dealing → Accusing → Apologizing → Finalized
- `PhaseLength` ile her faz blok sayısı konfigüre edilir
- `GetPhaseAtHeight(height, eonStartHeight)`: Mevcut fazı döndürür

**On-chain DKG (app/dkg.go):**
- `DKGInstance`: RegisterPolyEvalMsg, RegisterPolyCommitmentMsg, RegisterAccusationMsg, RegisterApologyMsg
- Accusation/Apology mekanizması: Kötü niyetli keyper'ı tespit ve dışla

**Epoch Anahtar Oluşturma:**
- `EpochKG.ComputeEpochSecretKeyShare(identityPreimage)`: shcrypto kullanarak
- `EpochKG.HandleEpochSecretKeyShare()`: Toplanan payları birleştir
- `shcrypto.VerifyEpochSecretKeyShare()`: Pay doğrulama

**Anahtar Payı Dağıtımı:**
- libp2p gossipsub üzerinden `DecryptionKeyShares` mesajları
- PostgreSQL'de persist (dkg results, batch configs, keyper sets)
- `DecryptionKeyShareHandler`: Pubsub validator + PostgreSQL kayıt + threshold aggregation

**Batch Konfigürasyonu:**
- `BatchConfig`: keypers[], threshold, activationBlockNumber, keyperConfigIndex
- `BatchConfig.EnsureValid()`: Keyper sayısı > 0, threshold > 0, threshold <= keyper sayısı
- `validateBatchConfig()`: Activation block sıralaması doğrulaması

**Depolama:**
- PostgreSQL (sqlc ile üretilmiş sorgular)
- `database/` ve `chainobserver/db/` — keyper, collator, sync tabloları
- WAL benzeri: ScheduleShutterMessage ile batch yazma

---

### 1.4 cow-protocol (Batch Auction — Solver Rekabeti)

**Genel Mimari:**
- `Orderbook`: HTTP API, order saklama, quoting
- `Autopilot`: Merkezi müzayedeci (her ~12-15s)
- `Driver`: Likidite toplama, calldata kodlama, simülasyon, zincire gönderme
- `Solver Engine`: Rota bulma (dahili: baseline/balancer, harici: 1inch/0x vb.)

**Order Türleri (model/src/order.rs):**
- `OrderClass`: Market | Limit | Liquidity
- `OrderStatus`: PresignaturePending | Open | Fulfilled | Cancelled | Expired
- `Signature`: EcdsaSignature (EIP-712 domain separation) — alloy_primitives kullanır
- EIP-712 tip hash: keccak256 üzerinden

**Fee Policy (model/src/fee_policy.rs):**
- `FeePolicy::Surplus { factor, max_volume_factor }`: Surplus üzerinden ücret
- `FeePolicy::Volume { factor }`: İşlem hacmi üzerinden ücret
- `FeePolicy::PriceImprovement { factor, max_volume_factor, quote }`: Fiyat iyileştirmesi

**Solver Rekabeti:**
- `SolverCompetitionDB`: auction_start_block, competition_simulation_block, auction, solutions
- `SolverSettlement`: solver adresi, clearing_prices (BTreeMap<Address, U256>), orders, score, ranking
- `Score` türleri: Solver | Protocol | ProtocolWithSolverRisk | Discounted
- `CompetitionAuction`: orders[], prices (token → fiyat)

**Kazanan Seçimi (autopilot/src/domain/competition/winner_selection.rs):**
- Uniform directional clearing prices: Her token için 2 fiyat (satma ve satın alma)
- Fairness guarantee: Bir order ancak daha iyi bir fiyat alıyorsa batch'e dahil edilir
- Reference score: S'nin olmadığı durumda toplam skor (solver'ın katkı ölçüsü)
- `max_winners_per_auction`, `max_solutions_per_solver` konfigürasyonu

**Run Loop:**
- `submission_deadline` blok sayısı
- `max_settlement_transaction_wait` duration
- `sync_solve_deadline_to_blockchain`: Her blokta çalışma
- Circuit breaker: Beklenmedik on-chain sonuç → solver jail

---

### 1.5 penumbra (ZK Shielded Blockchain)

**Transaction Yapısı (core/transaction/src/transaction.rs):**
- `TransactionBody`: actions[], transaction_parameters, detection_data, memo
- `EffectHash`: BLAKE2b ile "PenumbraEfHs" personalize (parameters + memo + detection + actions hash)
- `TransactionParameters`: chain_id, expiry_height, fee
- `DetectionData`: Fuzzy message detection (FMD) clue'ları

**Action Türleri:**
- `Spend`: Shielded harcama (nullifier açıklama)
- `Output`: Yeni şifreli not oluşturma
- `Delegate` / `Undelegate` / `UndelegateClaim`: Staking
- `Swap` / `SwapClaim`: DEX
- `PositionOpen` / `PositionClose`: LP pozisyonları
- `ProposalSubmit` / `ValidatorVote` / `DelegatorVote`: Governance
- `IbcRelay`: IBC köprüsü
- `CommunityPoolSpend / Deposit / Output`

**Kriptografi:**
- `decaf377`: Ristretto benzeri prime-order eğrisi (ark-ff tabanlı)
- `decaf377-rdsa`: Randomized DSA (Binding signature)
- `decaf377-fmd`: Fuzzy Message Detection (clue keys + detection keys)
- `ExpandedClueKey`: root_pub + türetilmiş alt anahtarlar (hkd)
- Transaction binding imzası: `VerificationKey<Binding>` + balance flow sıfırdan fark

**Eddy (Threshold Şifreleme — WIP):**
- `Committee { shares: Vec<PublicKeyShare>, threshold }`
- `EncryptionKey(decaf377::Element)`: Eğri elemanı olarak şifreleme anahtarı
- Ciphertext: 4 limb (c0, c1, c2, c3) — her limb (blinding*G, blinding*PK + limb*G) çifti
- `TransparentEncryptionProof`: Şimdilik şeffaf (WIP — gerçek ZK-SNARK yok)
- `dkg.rs`: `Committee { shares, threshold }` — sadece yapı iskelet

**Not Commitment Mekanizması:**
- `StateCommitment` (tct): Tiered commitment tree
- `Nullifier` (sct): Harcama sıfırlaması
- `Note`: value, asset_id, transmission_key, rseed — her bilgi şifreli

---

### 1.6 go-ethereum (Ethereum Tam İmplementasyonu)

**Blockchain Mimarisi:**
- `StateDB`: Ethereum state trie + caching + metriks (128 trie in memory)
- `mutation`: update | deletion — yazma sıralaması
- Bellek: `accountCacheHitMeter`, `storageCacheHitMeter` — prometheus metrikleri
- Commit sonrası state artık kullanılamaz (yeni root + yeni DB gerekir)

**Transaction Havuzu (legacypool):**
- `txSlotSize = 32KB`, `txMaxSize = 128KB`
- `ErrTxPoolOverflow`, `ErrOutOfOrderTxFromDelegated`, `ErrAuthorityReserved`
- Eviction interval: 1 dakika
- Stats raporu: 8 saniye aralıklı
- `prque` (priority queue) ile pending/queue sıralama

**State Transition (core/state_transition.go):**
- `IntrinsicGas()`: Temel gas hesaplama (EIP-2780, Homestead, EIP-1559 kuralları)
- `ExecutionResult`: UsedGas, MaxUsedGas, Err, ReturnData
- KZG4844 blob desteği (`kzg4844` paket)
- `uint256` kütüphanesi (256-bit sabit genişlik)

**Blockchain (core/blockchain.go):**
- Headler: headBlock, headHeader, headFastBlock, headFinalized, headSafe
- Metriklere sahip: TPS (mgasps), hesap okuma/yazma süreleri, trie commit süreleri
- `triedb/pathdb` ve `triedb/hashdb` — iki farklı trie DB stratejisi
- `rawdb` + `ethdb` → fiziksel depolama

**VM:**
- 150+ opcode (`core/vm/instructions.go`)
- JUMPDEST analizi (`core/vm/jumpdests.go`)
- Memory table + jump table
- Verkle tree ops (`core/vm/operations_verkle.go`)
- Gas hesabı: intrinsic + opcode başına

---

### 1.7 cosmos-sdk (Modüler Blockchain Framework)

**Bank Modülü Mimarisi:**
- `Keeper` interface: SendKeeper + Mint + Burn + Module transfer
- `BaseKeeper`: BaseSendKeeper + AccountKeeper + codec + storeService + logger
- `SendKeeper`: ViewKeeper + SendCoins + InputOutputCoins + send restrictions
- `blockedAddrs`: Belirli adreslere transfer yasağı
- `sendRestriction`: Eklenti tabanlı transfer kısıtlaması

**Send Restrictions Sistemi:**
- `AppendSendRestriction()` / `PrependSendRestriction()`: Zincir kısıtlama ekleme
- `ClearSendRestriction()`: Tüm kısıtlamaları temizle
- `MintingRestrictionFn`: Mint kısıtlamaları için de aynı pattern

**Depolama:**
- `cosmossdk.io/core/store` — soyut KV store service
- `collections` paketi: Map, IndexedMap, Vec, Item, Set — tip güvenli koleksiyonlar
- `codec` paketi: bool, time, bytes, int, uint, string, naming kodekleri

**Telemetri:**
- `github.com/cosmos/cosmos-sdk/telemetry` — yerleşik metrik sistemi
- Her işlem için: hesap transferi, mint, burn metrikleri

**Module System:**
- `module.go` → AppModuleBasic + AppModule interface
- `InitGenesis`, `ExportGenesis`, `BeginBlock`, `EndBlock`, `RegisterInterfaces`
- Governance modülü entegrasyonu (MsgUpdateParams için)

**Protobuf Mesajlar:**
- Her mesaj türü `.pb.go` dosyasında (protoc generated)
- `types/bank.pb.go`, `types/query.pb.go`, `types/query.pb.gw.go`

---

### 1.8 avalanchego (Snowball Consensus)

**Snowball Protokolü:**
- `Parameters`: K=20 (sorgu sayısı), AlphaPreference=15, AlphaConfidence=15, Beta=20
- Sağlıklı bağlantı: AlphaConfidence/K * (1-0.2) + 0.2 oranının üstünde
- `ConcurrentRepolls`: Aynı anda bekleyen anket sayısı (≤ Beta)
- `MaxItemProcessingTime`: 30 saniye — bunun üzerinde sağlıksız raporlar

**Nnary Snowball:**
- `nnarySnowball`: preference + maxPreferenceStrength + preferenceStrength map
- `RecordPoll(count, choice)`: count >= alphaPreference → strength artır → preference güncelle → Snowflake çağır
- `Preference()`: Finalize ise Snowflake tercihi, değilse Snowball tercihi

**Snowflake (convergence layer):**
- `nnarySnowflake`: consecutiveSuccesses + finalized
- `RecordPoll`: alphaConfidence eşiği aşarsa → consecutiveSuccesses++ ; aşmazsa sıfırla
- Finalization: consecutiveSuccesses >= beta

**Terminasyon Koşulları:**
- `terminationCondition { alphaConfidence, beta }` çifti
- Birden fazla koşul destekli (çok seviyeli finalizasyon)

**Sağlık Kontrolleri:**
- `MaxOutstandingItems = 256` — kuyrukta aşılırsa unhealthy
- `MinPercentConnectedBuffer = 0.2` — güvenlik tamponu
- `K/2 < AlphaPreference <= AlphaConfidence <= K` doğrulaması

---

### 1.9 cosmos-sdk, solana — Banka ve Konsensüs Detayları

**Solana Bank (runtime/src/bank.rs):**
- Her bank bir leader slot'una karşılık gelir (parent bank'e işaret eder)
- Yaşam döngüsü: Open → Frozen → Rooted
- `process_transactions()`: Ana giriş noktası
- `AccountsDB`: Bölünmüş depolama, paralel işleme (rayon)
- `EpochStakes`, `NodeVoteAccounts`: Staking ve oy hesapları
- `StatusCache`: İşlem durumları + SlotDelta
- `StakesCache`: DashMap tabanlı thread-safe stake önbelleği
- `TransactionBatch`, `InstalledSchedulerRwLock`: Zamanlayıcı entegrasyonu
- `CostTracker`: Hesaplama maliyeti izleme
- Epoch rewards: `hash_rewards_into_partitions()` — bölünmüş ödül dağıtımı
- Snapshot desteği: `SnapshotHash`, `serde_snapshot`

**Polkadot Executive (substrate/frame/executive/src/lib.rs):**
- `Executive<System, Block, Context, UnsignedValidator, AllPalletsWithSystem, OnRuntimeUpgrade>`
- `execute_block()` akışı: on_runtime_upgrade → System::initialize → on_initialize → PreInherents → Inherents → PostInherents → MBM check → transactions → on_idle → on_finalize
- `apply_extrinsic()`: Checkable + Applyable + GetDispatchInfo trait zincirleri
- `no_std` uyumlu (WASM runtime için zorunlu)
- `BeforeAllRuntimeMigrations`, `OnInitializeWithWeightRegistration`, `OnIdle`, `OnFinalize`, `OffchainWorker`, `OnPoll` — tam yaşam döngüsü hook'ları

---

## BÖLÜM 2 — NOVA SHIELD CHAIN GAP ANALİZİ

### 2.1 nova-mpc — KRİTİK GAP (YÜKSEK ÖNCELİK)

**Mevcut Durum:** Tüm işlemler BLAKE3 hash mock — gerçek kriptografi yok

**Eksik Bileşenler:**

| Bileşen | Production (tss-lib/multi-party-ecdsa) | Nova Shield Chain |
|---------|---------------------------------------|-------------------|
| Paillier şifreleme | 2048-bit safe prime modülüs, HomoMult, HomoAdd | YOK |
| Feldman VSS | `vss.Create()`, `Share.Verify()`, `Shares.ReConstruct()` | YOK |
| DLN Proof | 128 iterasyon Sigma protokol (h2 = h1^x mod Ñ) | YOK |
| FacProof | Paillier modülünün safe prime faktorizasyon kanıtı | YOK |
| ModProof | Modülüs kanıtı | YOK |
| Alice ZK Range Proof | \|a\| < q^3 (Fiat-Shamir + Pedersen) | YOK |
| Bob ZK Proof | MtA doğruluğu | YOK |
| PDL with Slack | Q = xG ve c = Enc(pk, x, r) | YOK |
| MtA Protokolü | AliceInit → BobMid → AliceEnd | YOK |
| Session Binding (SSID) | hash(parties \|\| nonce) tekrarlanmaz session ID | YOK |
| Blame Modülü | Kötü niyetli tarafı tespit (Phase5/6/7) | YOK |
| Key Resharing | Eski → yeni komite güvenli aktarım | YOK |
| Safe Prime Üretimi | GetRandomSafePrimesConcurrent (paralel) | YOK |
| Constant-time şifre çözme | Timing side-channel koruması | YOK |
| Timing protection | 200ms + 20ms jitter (MtA işlemleri için) | YOK |
| Round State Machine | StateMachine trait, round_blame() | MOCK iskelet |
| EdDSA TSS | Ayrı eddsa keygen/signing protokolü | YOK |

**Gerçek Protokol Round'ları:**
- Keygen: 4 tur (Round1→2→3→4→LocalKey çıktısı)
- Signing: 10 tur (9 offline + 1 online — AliceInit → ... → finalization)

**Kritik Güvenlik Açığı:** Mevcut mock, herhangi bir kriptografik güvenlik sunmuyor.
Gerçek threshold güvenliği için cggmp21 veya tss-lib Rust port'u gerekli.

---

### 2.2 nova-mempool — KRİTİK GAP (YÜKSEK ÖNCELİK)

**Mevcut Durum:** Threshold ElGamal mock — gerçek DKG ve şifreleme yok

**Eksik Bileşenler:**

| Bileşen | Production (rolling-shutter) | Nova Shield Chain |
|---------|------------------------------|-------------------|
| shcrypto kütüphanesi | puredkg + threshold ElGamal | YOK |
| DKG Faz Yönetimi | Off→Dealing→Accusing→Apologizing→Finalized | MOCK |
| Accusation/Apology | Kötü niyetli keyper tespiti ve dışlama | YOK |
| EonPublicKey (gerçek) | shcrypto.EonPublicKeyShare birleştirme | MOCK struct |
| EpochSecretKeyShare | shcrypto.ComputeEpochSecretKeyShare() | MOCK |
| Decryption Key Aggregation | VerifyEpochSecretKeyShare + threshold birleştirme | MOCK |
| Kalıcı depolama | PostgreSQL (sqlc) — eon/keyper/batch kayıtları | YOK |
| BatchConfig doğrulama | Threshold, activation block, benzersiz adresler | MOCK |
| P2P anahtar payı dağıtımı | libp2p gossipsub DecryptionKeyShares | YOK |
| Tendermint/ABCI koordinasyon | On-chain DKG koordinasyonu | YOK |
| IdentityPreimage | Her batch'e özgü kimlik ön görüntüsü | MOCK |
| retry mekanizması | 4 yeniden deneme, 200ms aralık | YOK |

---

### 2.3 nova-consensus — KRİTİK GAP

**Mevcut Durum:** Senkron mock BFT — gerçek dağıtılmış konsensüs yok

**Eksik Bileşenler:**

| Bileşen | Production (avalanchego/cosmos) | Nova Shield Chain |
|---------|--------------------------------|-------------------|
| Snowball/Snowflake | K=20, alpha>K/2, beta=20 consecutive | YOK |
| veya Tendermint BFT | 2/3+1 quorum, propose/prevote/precommit | MOCK iskelet |
| Async runtime | tokio::task::spawn, mpsc kanallar | SYNC |
| Slot timer | tokio::time::interval(400ms) | MOCK |
| P2P gossip | Block announce, oy paylaşımı | MOCK |
| Fork choice | GHOST veya longest chain | YOK |
| Finalization guarantee | Quorum certificate ile | MOCK |
| View change | Timeout durumunda yeni proposer | YOK |
| Leader election | Verifiable Random Function (VRF) | MOCK round-robin |
| Block validation | Signature + state root doğrulaması | MOCK |

---

### 2.4 nova-runtime — ORTA GAP

**Mevcut Durum:** DashMap bellek içi — gerçek kalıcı depolama yok

**Eksik Bileşenler:**

| Bileşen | Production (go-ethereum/solana) | Nova Shield Chain |
|---------|--------------------------------|-------------------|
| Merkle Patricia Trie | State root hesaplama | YOK |
| RocksDB/LevelDB | Kalıcı blok + state + tx depolaması | YOK |
| StateDB | Caching + dirty tracking + revert | DashMap |
| WAL | Write-ahead log, restart recovery | YOK |
| Snapshot | Durum anlık görüntüsü | YOK |
| Account trie | Her adres için ayrı storage trie | YOK |
| Column families | blocks/state/txs/receipts/canonical | YOK |
| Paralel TX işleme | Rayon par_iter() — account bazlı | Temel |
| Gas metering | IntrinsicGas + opcode başına gas | MOCK |
| EIP-1559 | max_fee_per_gas, base_fee, priority_fee | İskelet |
| Telemetri | prometheus metrikleri (hesap oku/yaz) | YOK |
| Seri hale getirme | RLP (go-ethereum) / SCALE (polkadot) | JSON serde |

---

### 2.5 nova-zk — ORTA GAP

**Mevcut Durum:** BLAKE3 hash mock — gerçek ZK devreleri yok

**Eksik Bileşenler:**

| Bileşen | Production (penumbra) | Nova Shield Chain |
|---------|----------------------|-------------------|
| Gerçek ZK Devreler | Groth16 / PLONK (ark-groth16) | YOK |
| EffectHash | blake2b "PenumbraEfHs" personalize | BLAKE3 mock |
| decaf377 eğrisi | Prime-order, ark-ff tabanlı | YOK |
| Note commitment | Şifreli not (rseed + transmission_key) | MOCK struct |
| Nullifier derivation | Gerçek sct::Nullifier | MOCK |
| Tiered commitment tree | `penumbra_sdk_tct` — Merkle tree | YOK |
| Binding signature | VerificationKey<Binding> | YOK |
| FMD Clue keys | Fuzzy message detection | YOK |
| TransferCircuit | input_value == output_value + fee | YOK |
| ShieldCircuit | commitment reveal + nullifier | YOK |
| MintCircuit | Yeni not oluşturma | YOK |

---

### 2.6 nova-network — YÜKSEK GAP

**Mevcut Durum:** Channel tabanlı mock — gerçek P2P yok

**Eksik Bileşenler:**

| Bileşen | Production | Nova Shield Chain |
|---------|-----------|-------------------|
| libp2p transport | TCP + QUIC | YOK |
| Noise XX handshake | Gerçek kimlik doğrulama | MOCK |
| Yamux multiplexing | Çok akışlı bağlantı | YOK |
| Gossipsub | Block/TX/consensus gossip | YOK |
| Kademlia DHT | Peer keşfi + bootstrap | YOK |
| Peer scoring | Kötü peer cezalandırma | YOK |
| Message validation | İmza doğrulaması | YOK |
| Block announcement | `/nova/blocks/1` topic | YOK |
| Consul/bootnode | İlk bağlantı bootstrap | YOK |

---

### 2.7 nova-vm — YÜKSEK GAP

**Mevcut Durum:** BLAKE3 mock executor — gerçek bytecode yürütme yok

**Eksik Bileşenler:**

| Bileşen | Production | Nova Shield Chain |
|---------|-----------|-------------------|
| wasmtime | Gerçek WASM yürütme | YOK |
| Host fonksiyonları | nova_storage_get/set, nova_emit_event | YOK |
| Gas metering | ResourceLimiter | YOK |
| WASM validation | Magic bytes + section validation | Temel |
| Cross-contract call | CPI (solana) / CALL (EVM) | YOK |
| EVM uyumluluğu | revm entegrasyonu | YOK |
| ABI codec | keccak256 function selector | BLAKE3 mock |
| Stack depth limit | Max call depth | YOK |
| Memory limit | Max memory pages | YOK |

---

### 2.8 nova-auction — ORTA GAP

**Mevcut Durum:** Basit batch mock — gerçek solver rekabeti yok

**Eksik Bileşenler:**

| Bileşen | Production (cow-protocol) | Nova Shield Chain |
|---------|--------------------------|-------------------|
| Solver rekabeti | Birden fazla solver, en iyi seçilir | YOK |
| Uniform directional prices | Her token için 2 fiyat | Basit uniform |
| Fairness guarantee | Order daha kötü fiyat alamaz | YOK |
| Reference score | Solver olmadan toplam skor | YOK |
| FeePolicy | Surplus/Volume/PriceImprovement | YOK |
| Circuit breaker | Kötü solver jail | YOK |
| PostgreSQL state | Auction geçmişi kalıcılığı | YOK |
| EIP-712 order imzalama | Gerçek order UID | YOK |
| Liquidity aggregation | DEX likidite toplama | YOK |

---

### 2.9 Serileştirme ve Protokol — GAP

| Bileşen | Production | Nova Shield Chain |
|---------|-----------|-------------------|
| RLP encoding | go-ethereum (blok/tx serileştirme) | YOK |
| SCALE codec | polkadot-sdk (no_std uyumlu) | YOK |
| Protobuf | cosmos-sdk, tss-lib (mesajlar) | YOK |
| EIP-712 domain separation | cow-protocol (order imzalama) | YOK |
| EIP-55 checksum adresi | nova-primitives'e eklendi | VAR |
| Transaction nonce | go-ethereum stili sıralı nonce | VAR (temel) |

---

### 2.10 Güvenlik Sertleştirme — GAP

| Bileşen | Production | Nova Shield Chain |
|---------|-----------|-------------------|
| Constant-time kriptografi | tss-lib CTModInt | YOK |
| Timing protection | 200ms + jitter (MtA) | YOK |
| Panic recovery | go-ethereum | YOK |
| Fuzzing | cargo-fuzz | YOK |
| Rate limiting | cow-protocol (dakikada 100 istek/IP) | YOK |
| Compute budget | solana (işlem başına limit) | YOK |
| Rent mekanizması | solana (hesap tutma maliyeti) | YOK |
| Slashing | Kötü davranış → stake kesilmesi | İskelet |

---

## BÖLÜM 3 — ÖNCELİKLİ GAP ÖZETİ

### Kritiklik Sıralaması:

```
1. YÜKSEK — nova-mpc
   ↳ Paillier + VSS + ZK proofs = Gerçek MPC güvenliği
   ↳ Olmadan: Tüm "eşik imzalama" sahte

2. YÜKSEK — nova-mempool
   ↳ shcrypto + DKG + P2P = Gerçek şifreli mempool
   ↳ Olmadan: MEV koruması çalışmıyor

3. YÜKSEK — nova-network
   ↳ libp2p = Gerçek validator iletişimi
   ↳ Olmadan: Multi-node test bile mümkün değil

4. YÜKSEK — nova-runtime (storage)
   ↳ RocksDB + MPT = Veri kalıcılığı
   ↳ Olmadan: Her restart'ta tüm state kaybolur

5. ORTA — nova-consensus
   ↳ Async tokio + gerçek BFT = Gerçek blok üretimi
   ↳ Olmadan: Tek node dahi gerçekçi çalışamaz

6. ORTA — nova-vm
   ↳ wasmtime = Akıllı kontrat yürütme
   ↳ Olmadan: Kontrat platformu değil

7. ORTA — nova-zk
   ↳ Groth16/PLONK = Gerçek gizlilik
   ↳ Olmadan: "Shielded" sadece isimde kalır

8. DÜŞÜK — nova-auction
   ↳ Solver rekabeti = CoW benzeri MEV koruması
   ↳ Mevcut basit batch auction yeterli başlangıç için

9. DÜŞÜK — nova-guardian/nova-freeze/nova-sdk
   ↳ Mantıksal doğru, mock'tan bağımsız çalışabilir
```

---

## BÖLÜM 4 — KARŞILAŞTIRMA TABLOSU

| Bileşen | Nova Shield Chain | go-ethereum | cosmos-sdk | polkadot-sdk | solana | penumbra |
|---------|-------------------|-------------|------------|--------------|--------|---------|
| Kriptografi | MOCK (BLAKE3) | secp256k1 | secp256k1/ed25519 | sr25519/ed25519 | ed25519 | decaf377 |
| Storage | DashMap (bellek) | LevelDB+MPT | IAVL+IAVL | Substrate DB | AccountsDB | JMT |
| VM | MOCK | EVM (150+ opcode) | Yok (CosmWasm ayrı) | WASM/PolkaVM | BPF/SBF | Yok |
| Network | Mock channel | devp2p | Tendermint P2P | libp2p | QUIC | libp2p |
| Consensus | Mock BFT | PoS/Casper | Tendermint BFT | BABE+GRANDPA | TowerBFT | Tendermint |
| ZK | MOCK | Yok | Yok | Yok | Yok | Groth16 |
| MPC | MOCK (BLAKE3) | Yok | Yok | Yok | Yok | Yok |
| Şifreli Mempool | MOCK | Yok | Yok | Yok | Yok | Kısmi (eddy) |
| Batch Auction | Basit mock | Yok | Yok | Yok | Yok | DEX var |
| Test coverage | 2123 test | Binlerce | Binlerce | Binlerce | Binlerce | Yüzlerce |
| Production? | HAYIR | EVET | EVET | EVET | EVET | EVET |

---

## BÖLÜM 5 — FAZ PLANI (GAP'E GÖRE REVİZE)

### FAZ 1A — Gerçek Kriptografi (Öncelik: ACİL)
```
k256 + blst + ed25519
  ↓
nova-crypto: mock → gerçek
  ↓
cggmp21 veya round_based + tss-lib algoritmalarıyla nova-mpc
  ↓
shcrypto (Rust port) veya bls12_381 ile nova-mempool DKG
```

### FAZ 1B — Gerçek Depolama (Öncelik: YÜKSEK)
```
rocksdb crate
  ↓
nova-storage crate (yeni)
  ↓
Column families: blocks/state/txs/receipts/canonical
  ↓
nova-runtime: DashMap → RocksDB bağlantısı
```

### FAZ 2 — Gerçek Ağ (Öncelik: YÜKSEK)
```
libp2p { tcp, noise, yamux, gossipsub, kad }
  ↓
nova-network: mock channel → gerçek transport
  ↓
Gossipsub topics: /nova/blocks/1, /nova/txs/1, /nova/consensus/1
```

### FAZ 3 — Async Konsensüs (Öncelik: ORTA)
```
tokio::task::spawn (async)
  ↓
Slot timer: tokio::time::interval(400ms)
  ↓
BFT döngüsü: propose → broadcast → vote aggregate → finalize
```

### FAZ 4 — Gerçek VM (Öncelik: ORTA)
```
wasmtime crate
  ↓
nova-vm: mock → gerçek WASM yürütme
  ↓
Host fonksiyonları: storage/emit/caller/balance
```

### FAZ 5 — Gerçek ZK (Öncelik: DÜŞÜK/UZUN)
```
ark-groth16 veya halo2
  ↓
TransferCircuit / ShieldCircuit / MintCircuit
  ↓
decaf377-rdsa binding signature
```

---

## BÖLÜM 6 — KRİTİK BULGULAR VE SONUÇ

### Nova Shield Chain'in Güçlü Yönleri:
1. **Mimari iskelet sağlam** — 19 crate doğru bölünmüş, bağımlılıklar mantıklı
2. **Test altyapısı kapsamlı** — 2123 test, 0 başarısız; mock'lar için geçerli davranış test edilmiş
3. **API tasarımı üretim benzeri** — Tip güvenli, serde ile JSON serileştirme
4. **Domain mantığı doğru** — Deadman, inheritance, MEV koruma senaryoları gerçekçi
5. **10 repo'dan iyileştirme** — SpendKey→FVK→IVK hiyerarşisi, TSS round state, CoW batch

### Kritik Eksiklikler:
1. **Sıfır gerçek kriptografi** — Tüm imzalar, prooflar, şifreleme BLAKE3 mock
2. **Sıfır kalıcı depolama** — Restart'ta tüm veri kaybolur
3. **Sıfır gerçek ağ** — Tek process dışında validator koordinasyonu yok
4. **Sıfır gerçek VM** — Akıllı kontrat derleme/çalıştırma mock
5. **Sıfır gerçek ZK** — "Shielded" özellik tamamen isimde

### Sonuç:
Nova Shield Chain şu an **tam iskelet + mock bileşenler** aşamasındadır. Protokol spesifikasyonu ve
mimari tasarımı üretim kalitesinde, ancak gerçek kriptografik güvenlik sağlamaz.

Tahmini gerçek üretim geçiş süresi (sırayla uygulanırsa):
- FAZ 1A (Kriptografi): 6-10 hafta
- FAZ 1B (Storage): 2-3 hafta
- FAZ 2 (Network): 3-4 hafta
- FAZ 3 (Async Consensus): 3-5 hafta
- FAZ 4 (VM): 2-3 hafta
- FAZ 5 (ZK): 8-12 hafta
- **Toplam: ~6-9 ay**

---

## BÖLÜM 7 — EK DOSYA OKUMA BULGULARI (İkinci Oturum)

### 7.1 rolling-shutter — Ek Dosyalar

**SlotTicker (medley/slotticker/slotticker.go):**
- `Slot{Number, genesisSlotTime, slotDuration}` — her slot'un başlangıç zamanı `genesisSlotTime + slotDuration * Number`
- `calcNextTick(now, genesisSlotTime, slotDuration, offset)` — bir sonraki slot numarasını ve zamanını hesaplar
- Kaçırılan slotlar için log uyarısı (`missing slots due to slow slot processing`)
- `context.Done()` ile iptal desteği — graceful shutdown

**P2PMessaging (p2p/messaging.go):**
- `HandlerRegistry map[protoreflect.FullName][]HandlerFunc` — mesaj türüne göre handler listesi
- `ValidatorRegistry map[string][]pubsub.ValidatorEx` — topic başına validator zinciri
- `addValidatorImpl()`: topic uyuşmazlığı → reject; trace context kontrolü; tip kontrolü
- `runHandleMessages()`: `P2P.GossipMessages` kanalından okur → handler çağırır → çıktı mesajlarını broadcast
- `SendMessage()`: protobuf marshal → `retry.FunctionCall()` ile gönderme
- Prometheus metrikleri: `metricsP2PMessageValidationTime`, `metricsP2PMessageHandlingTime`

**EpochKG (keyper/epochkg/epochkg.go):**
- `EpochKG{Eon, NumKeypers, Threshold, Keyper, SecretKeyShare, PublicKey, PublicKeyShares}`
- `ComputeEpochSecretKeyShare(identityPreimage)` → `shcrypto.ComputeEpochSecretKeyShare()`
- `HandleEpochSecretKeyShare()`: duplicate sender kontrolü → `VerifyEpochSecretKeyShare()` → eşik dolunca `ComputeEpochSecretKey()`
- `len(shares) == int(epochkg.Threshold)` ile eşik kontrolü (tam eşit, değil büyük-eşit)

**DecryptionKeyShareHandler (keyper/epochkghandler/keyshare.go):**
- `ValidateMessage()`: instanceID → eon range → keyper doğrulama → DKG sonuç → share doğrulama
- `checkKeyShares()`: `VerifyEpochSecretKeyShare()` + sıralama kontrolü (`bytes.Compare`)
- `HandleMessage()`: DB insert → key var mı kontrol → DKG result çek → aggregate → `DecryptionKeys` broadcast
- `MaxNumKeysPerMessage` ile mesaj boyut sınırı

**DKG Phase (keyper/dkgphase/phase.go):**
- `PhaseLength{off, dealing, accusing, apologizing}` — blok sayısı kümülatif
- `NewConstantPhaseLength(l)` — 4 faz, her biri `l` blok
- `GetPhaseAtHeight(height, eonStartHeight)` → `Off | Dealing | Accusing | Apologizing | Finalized`

**Voting Sistemi (app/voting.go):**
- `Voting[T any, E Equals[T]]{Votes map[Address]int, Candidates []T}` — generic oy yapısı
- `SetVote(sender, candidate)` — Aynı candidate varsa index güncelle, yoksa ekle
- `AddVote(sender, candidate)` — Tekrar oy verme hatasını döner
- `Outcome(numRequiredVotes int)` — threshold'ı aşan candidate'i döner

**ShutterApp Tipleri (app/types.go):**
- `ShutterApp{Configs, DKGMap map[uint64]*DKGInstance, ConfigVoting, Identities, BlocksSeen, Validators Powermap, ForkHeights}`
- `DKGInstance{Config BatchConfig, Eon, SuccessVoting, PolyEvalsSeen, PolyCommitmentsSeen, AccusationsSeen, ApologiesSeen}`
- `ForkHeights{CheckInUpdateNew ForkHeight}` — yükseklik + enabled flag
- `ValidatorPubkey{Ed25519pubkey string}` — map key olarak kullanılabilir

**Retry Mekanizması (medley/retry/retry.go):**
- `retrier{numRetries=3, interval=2s, maxInterval=60s, multiplier=1.0}` — üstel geri çekilme
- `FunctionCall[T any](ctx, fn, opts)` — generic retry fonksiyonu
- `cancelingErrors []error` — bu hataları alınca yeniden deneme yapmaz
- `iterator()` goroutine ile non-blocking zaman hesaplama

**ValidatorRegistry (medley/validatorregistry/validatorregistry.go):**
- `AggregateRegistrationMessage`: version(1) + chainID(8) + addr(20) + index(8) + count(4) + nonce(4) + flag(1) = 46 byte
- `LegacyRegistrationMessage`: version(1) + chainID(8) + addr(20) + index(8) + nonce(8) + flag(1) = 46 byte
- `ValidatorIndices()` — birden fazla validator tek mesajda kayıt (aggregate version)

---

### 7.2 cow-protocol — Ek Dosyalar

**Solution ve Score Tipleri (domain/competition/mod.rs):**
- `Solution{id: u64, solver: Address, orders: HashMap<OrderUid, TradedOrder>}`
- `TradedOrder{side, sell: eth::Asset, buy: eth::Asset, executed_sell, executed_buy}`
- `Score(eth::Ether)` — `num::Saturating` + `num::CheckedSub` implemente eder
- Score'lar `saturating_add_assign()` ile güvenli toplama

**Order Yapısı (domain/auction/order.rs):**
- `Order{uid, sell, buy, protocol_fees: Vec<fee::Policy>, side, pre_interactions, post_interactions, ...}`
- `OrderUid([u8; 56])` — 32 byte order digest + 20 byte owner address + 4 byte validTo
- `OrderUid.owner()` → `Address::from_slice(&self.0[32..52])`
- `Side::Buy | Side::Sell` — iki taraflı order

**Run Loop (autopilot/src/run_loop.rs):**
- `submission_deadline: u64` — blok sayısı (birkaç blok penceresi)
- `LeaderLockTracker`, `ShutdownController`, `SolvableOrdersCache` servisleri
- Competition döngüsü: solvable orders → solvers'a gönder → çözümler topla → kazanan seç → settle
- Solver'lara paralel gönderim, asenkron cevap toplama

---

### 7.3 go-ethereum — Ek Dosyalar

**TxPool (core/txpool/txpool.go):**
- `TxPool{subpools []SubPool, chain, state, subs, quit, sync}`
- SubPool mimarisi: `legacypool`, `blobpool` — her tür TX için ayrı havuz
- `TxStatusUnknown | TxStatusQueued | TxStatusPending | TxStatusIncluded`
- `BlockChain` interface: `Config()`, `CurrentBlock()`, `Genesis()`, `SubscribeChainHeadEvent()`, `StateAt()`

**Miner (miner/miner.go):**
- `Miner{config, chainConfig, engine, txpool, prio []Address, chain, pending}`
- `DefaultConfig: GasCeil=60_000_000, GasPrice=GWei/1000, Recommit=2s`
- `prio []common.Address` — öncelikli sender listesi (MEV önleme benzeri)
- `Pending()` → `(block, receipts, statedb)` üçlüsü döner

**Clique PoA (consensus/clique/clique.go):**
- `epochLength=30000`, `checkpointInterval=1024`, `inmemorySnapshots=128`
- `extraVanity=32` byte + `extraSeal=65` byte (imza)
- `nonceAuthVote=0xffffffffffffffff` / `nonceDropVote=0x0000000000000000`
- `diffInTurn=2`, `diffNoTurn=1` — sırasında bloklama daha ağır

**StateDB (core/state/statedb.go):**
- `TriesInMemory=128` — 128 blok geçmişi bellekte
- `mutation{typ: update|deletion, applied bool}` — yazma sıralaması izleme
- Commit sonrası: state artık kullanılamaz, yeni root + DB gerekir
- `errgroup.Group` ile paralel state işleme

**EVM BlockContext (core/vm/evm.go):**
- `BlockContext{CanTransfer, Transfer, GetHash, Coinbase, GasLimit, BlockNumber, Time, Difficulty, BaseFee, BlobBaseFee, Random, SlotNum, CostPerStateByte}`
- `SlotNum` — Ethereum slot numarası (EIP ile eklendi)
- `CostPerStateByte` — EIP-8037: Yeni state byte başına maliyet
- Precompile registry: `evm.precompiles[addr]` map'inden bakılır

---

### 7.4 penumbra — Ek Dosyalar

**IBC Transfer (shielded-pool/component/transfer.rs):**
- `Ics20Transfer{}` — IBC fungible token transfer handler
- `is_source(source_port, source_channel, denom, is_refund)` — native mi bridged mi?
- Native token: denom prefix `{port}/{channel}/` yoksa native
- Bridged token: denom prefix `{port}/{channel}/` varsa bridged
- `AppHandlerCheck + AppHandlerExecute` trait'leri ile ayrı validation/execution

**Dutch Auction (auction/dutch.rs):**
- `DutchAuction{description: DutchAuctionDescription, state: DutchAuctionState}`
- `DutchAuctionDescription{input: Value, output_id: asset::Id, max_output, min_output, start_height, end_height, step_count: u64, nonce: [u8; 32]}`
- ID hesaplama: `blake2b_simd::Params::default().personal(DUTCH_AUCTION_DOMAIN_SEP)` ile
- `DUTCH_AUCTION_DOMAIN_SEP = b"penumbra_DA_nft"`

**Dutch Auction Manager (auction/component/dutch_auction.rs):**
- `schedule_auction()`: TriggerData hesaplama → VCB credit → trigger kaydı → state yazımı
- `DutchAuctionState{sequence: 0, current_position: None, next_trigger, input_reserves, output_reserves}`
- `try_next_trigger_height(current_height)` — süresi dolmuş auction'ı reddeder

**Note Yapısı (shielded-pool/note.rs):**
- `Note{value: Value, rseed: Rseed, address: Address, transmission_key_s: Fq}`
- `NOTE_LEN_BYTES = 160`, `NOTE_CIPHERTEXT_BYTES = 176`
- `transmission_key_s: Fq` — eğri noktası validasyonu garantili
- `NoteView{value: ValueView, rseed, address: AddressView}` — kullanıcıya gösterim için

---

### 7.5 cosmos-sdk — Ek Dosyalar

**BaseApp (baseapp/baseapp.go):**
- `BaseApp{cms CommitMultiStore, qms MultiStore, grpcQueryRouter, msgServiceRouter, mempool, anteHandler, postHandler}`
- Yürütme modları: `execModeCheck | execModeReCheck | execModeSimulate | execModePrepareProposal | execModeProcessProposal | execModeVoteExtension | execModeVerifyVoteExtension | execModeFinalize`
- `snapshotManager`, `interBlockCache`, `stateManager`, `optimisticExecution`
- `sigverifyTx bool` — simulation test için imza doğrulama atlanabilir

**ABCI (baseapp/abci.go):**
- `InitChain()`: chainID eşleşmesi → initialHeight=1 (0 ise) → storeService version ayarı → ConsensusParams persist
- `FinalizeBlock()`: txnrunner (blockstm veya default) seçimi
- `BlockSTM`: `baseapp/txnrunner/blockstm.go` — paralel tx yürütme (Aptos tarzı)

**Staking Validator (staking/keeper/validator.go):**
- `GetValidator(ctx, addr ValAddress)` → `store.Get(GetValidatorKey(addr))` → unmarshal
- `GetValidatorByConsAddr()` → consensus addr → operator addr → `GetValidator()`
- `ValidatorByHistoricalConsAddr()` — key rotation sonrası eski consensus addr'den bul
- `ConsAddrLockRotatedFrom` — rotasyon kilidi türü (güvenlik özelliği)

**Staking Slash (staking/keeper/slash.go):**
- `Slash(ctx, consAddr, infractionHeight, power, slashFactor math.LegacyDec)` → `(math.Int, error)`
- Unbonded validator slash edilmez (CONTRACT)
- `TokensFromConsensusPower(ctx, power)` → gerçek token miktarı
- `slashAmountDec = amount * slashFactor` → truncate → tamsayı slash miktarı

**Staking Delegation (staking/keeper/delegation.go):**
- `GetDelegation(ctx, delAddr, valAddr)` → `GetDelegationKey(delAddr, valAddr)` ile KV okuma
- `IterateAllDelegations()` — tüm delegasyonları iterator ile tarar
- `GetValidatorDelegations()` — prefix iterator: `GetDelegationsByValPrefixKey(valAddr)`

---

### 7.6 avalanchego — Ek Dosyalar

**Health Sistemi (api/health/health.go):**
- Üç katmanlı sağlık: `readiness` (monotonic check), `health` (düzenli check), `liveness`
- `RegisterReadinessCheck()` → `worker.RegisterMonotonicCheck()` — bir kez healthy, hep healthy
- `RegisterHealthCheck()` / `RegisterLivenessCheck()` → `worker.RegisterCheck()`
- `AllTag = "all"`, `ApplicationTag = "application"` — her check'te otomatik eklenir
- Prometheus gauge: `checks_failing{check, tag}` — başarısız check sayısı

---

### 7.7 solana — Ek Dosyalar

**AccountsDB (accounts-db/src/accounts_db.rs):**
- `AppendVec` — memory-mapped dosyalar: tek thread append + çok thread okuma
- `APPEND_VEC_MMAPPED_FILES_OPEN`, `STORE_META_OVERHEAD` sabitler
- Global `write_version: AtomicU64` — tüm commit'leri takip eder, indeks bootstrap için
- `AccountsIndex{in_mem: DashMap, disk_index: BucketMapHolder}` — çift katmanlı indeks
- `AccountsCache` — slot bazlı önbellek (frozen olmadan önce)
- `AccountsHasher` — paralel hash hesaplama (rayon par_iter)
- `EpochAccountsHashManager` — epoch bazlı hash doğrulama
- `RentPayingAccountsByPartition` — rent hesapları bölümleme
- `AncientAppendVecs` — eski veri sıkıştırma

---

### 7.8 polkadot-sdk — Ek Dosyalar

**Balances Pallet (frame/balances/src/lib.rs):**
- `fungible` trait ailesi: `Inspect, Mutate, Unbalanced, Balanced, BalancedHold, InspectHold, MutateHold, InspectFreeze, MutateFreeze, Imbalance`
- `Hold` ve `Freeze` kavramları: stake/lock için ayrı mekanizmalar
- `ExistentialDeposit` — minimum bakiye (bu altında hesap "reaped" olur)
- Hesap reaping: nonce sıfırlama → hesabı kaldır
- `LockableCurrency` — eski API (deprecated → `fungible::MutateFreeze` kullan)

**Staking Pallet (frame/staking/src/lib.rs):**
- NPoS (Nominated Proof of Stake) — stash account + controller (deprecated)
- Staker durumları: `Validator | Nominator | Idle` (`StakerStatus`)
- Era sistemi: `bond → validate/nominate → reward/slash`
- `pallet-staking` deprecated → `pallet-staking-async` kullanılacak
- Slash mekanizması: `slash_era()` ile geçmişe dönük ceza
- `MaxNominatorRewardedPerValidator` — bir validator'dan ödül alan nominator limiti

---

## BÖLÜM 8 — GÜNCEL GAP TABLOSU (İkinci Oturum Sonrası)

### Nova Shield Chain'e Eklenebilecek Yeni Özellikler

| Özellik | Kaynak Repo | Nova Shield Chain'de Eksik |
|---------|-------------|---------------------------|
| SlotTicker (genesis zamanına göre) | rolling-shutter | MOCK — sabit interval |
| Generic Voting[T, E] mekanizması | rolling-shutter | YOK — hardcoded BFT |
| Retry mekanizması (exponential backoff) | rolling-shutter | YOK |
| SubPool mimarisi | go-ethereum | YOK — tek havuz |
| Miner.prio[] öncelikli sender listesi | go-ethereum | YOK |
| Clique benzeri PoA | go-ethereum | YOK (BFT var) |
| StateDB TriesInMemory (128 blok) | go-ethereum | YOK — her block fresh |
| EVM SlotNum + CostPerStateByte | go-ethereum | YOK |
| Üç katmanlı health check | avalanchego | YOK |
| AppendVec / memory-mapped storage | solana | YOK |
| Hold/Freeze token mekanizması | polkadot balances | YOK |
| NPoS staking + Era sistemi | polkadot staking | YOK |
| BaseApp execMode sabitleri | cosmos-sdk | YOK |
| BlockSTM paralel TX | cosmos-sdk | Temel paralel var |
| ConsAddr rotasyon | cosmos-sdk staking | YOK |
| IBC token bridging (native/bridged) | penumbra | YOK |
| Dutch Auction (DEX) | penumbra | YOK |
| Note transmission_key_s Fq doğrulama | penumbra | YOK |
| 3-tier health (readiness/health/liveness) | avalanchego | YOK |
| ValidatorRegistry aggregate mesajlar | rolling-shutter | YOK |

### nova-consensus için Yeni Bulgular:
- rolling-shutter'ın `SlotTicker` → genesis zamanı + offset tabanlı hassas slot zamanlama
- `calcNextTick()` algoritması: `slot = (now - genesis - offset + duration - 1) / duration`
- Kaçırılan slotlar için arka arkaya tick gönderme (slot gap fill)
- Cosmos-sdk BaseApp'teki `execMode` sabitleri → Nova'nın runtime'ına benzer mod desteği gerektirir

### nova-staking için Yeni Bulgular:
- Polkadot staking pallet: Era sistemi, NPoS seçimi, slash geçmişi
- Cosmos-sdk slash: `Slash(consAddr, infractionHeight, power, slashFactor)` — unbonded validator'ı slash etme
- Cosmos-sdk delegation: prefix iterator ile validator delegasyonları
- Nova shield chain şu an sadece basit stake mekanizması var; tam NPoS + Era sistemi yok

### nova-runtime için Yeni Bulgular:
- go-ethereum TxPool SubPool mimarisi → legacy + blob ayrı havuzlar
- StateDB TriesInMemory=128 → son 128 blok trie'si bellekte → hızlı state erişimi
- Cosmos-sdk BlockSTM → paralel TX yürütme (Aptos benzeri optimistik execution)
- Nova'nın mevcut paralel TX işlemesi (rayon) daha basit; dependency graph yönetimi eksik

### nova-vm için Yeni Bulgular:
- go-ethereum EVM BlockContext: `SlotNum` ve `CostPerStateByte` alanları (EIP-8037) — Nova VM'de yok
- go-ethereum precompile adresleme: `precompiles[addr]` map, Nova'da da benzer yapı var ama gerçek EVM precompile seti eksik
- Cosmos-sdk `execModeSimulate` — TX simülasyonu için özel mod; Nova runtime'ında yok
- Cosmos-sdk `sigverifyTx bool` — simulation'da imza atlanabilir; Nova'da daima doğrulanıyor

### nova-mempool / DKG için Yeni Bulgular:
- rolling-shutter EpochKG: `len(shares) == int(epochkg.Threshold)` tam eşit kontrol (>=  değil) — Nova'da eşik kriteri gözden geçirilmeli
- rolling-shutter DKG: `Off → Dealing → Accusing → Apologizing → Finalized` — 4 fazlı, Nova mock'ı 2 fazlı (sadece dealing + finalized)
- rolling-shutter retry: exponential backoff (numRetries=3, interval=2s, maxInterval=60s) — Nova'da retry yok
- rolling-shutter AggregateRegistrationMessage: 46 byte sabit format — Nova'da mesaj boyutu standartlaştırılmamış

### nova-guardian için Yeni Bulgular:
- polkadot `ExistentialDeposit` — minimum bakiye (guardian'ın da minimum stake'i olmalı, altı "reaped")
- polkadot `Hold/Freeze` — guardian stake'i "freeze" ile kilitlenmeli (transfer edilemez ama kesilebilir); Nova'da basit lock
- cosmos-sdk `ConsAddrLockRotatedFrom` — key rotation sırasında eski address kilitlenir; Nova guardian değişimi için benzer mekanizma eksik
- cosmos-sdk `ValidatorByHistoricalConsAddr` — rotasyon sonrası eski adresle sorgulama; Nova'da tarihi guardian adresi takibi yok

### nova-auction için Yeni Bulgular:
- cow-protocol `OrderUid([u8; 56])` = 32 byte digest + 20 byte owner + 4 byte validTo — Nova'da order kimliği basit u64
- cow-protocol `Score(eth::Ether)` + `saturating_add_assign` — taşma korumalı skor hesaplama; Nova'da skor f64 (kayan nokta, deterministik değil)
- cow-protocol `submission_deadline` — birkaç blok penceresi, Nova'da tek slot deadline
- cow-protocol `LeaderLockTracker` — auction sırasında concurrent submission önleme; Nova'da yok

### nova-network için Yeni Bulgular:
- rolling-shutter P2PMessaging: `HandlerRegistry` + `ValidatorRegistry` — topic başına birden fazla handler/validator zinciri; Nova'da tek handler
- rolling-shutter Prometheus metrikleri: `metricsP2PMessageValidationTime`, `metricsP2PMessageHandlingTime` — Nova'da P2P metrik yok
- avalanchego: `checks_failing{check, tag}` gauge — 3 katmanlı sağlık metrikleri; Nova'da yok
- rolling-shutter `context.Done()` ile graceful shutdown + slot gap fill — Nova'da slot timer context-aware değil

---

## BÖLÜM 8 — BÖLÜM 8 DEVAM: ÖNCELİKLENDİRME VE AKSİYON PLANI

### 8.1 İkinci Oturumdan Bulunan Yeni Gap'lerin Kritiklik Sıralaması

```
KRİTİK (Hemen düzeltilmeli):
━━━━━━━━━━━━━━━━━━━━━━━━━━
1. nova-auction → f64 skor yerine integer/newtype kullan
   ↳ f64 deterministik değil → validator'lar farklı sonuç üretebilir → konsensüs bozulur
   ↳ FIX: Score(u128) newtype + saturating_add

2. nova-mempool DKG → eşik: >= yerine == (rolling-shutter pattern)
   ↳ Mevcut mock'ta bu fark yok ama gerçek implementasyonda kritik
   ↳ FIX: Eşik dolunca (tam olarak N share gelince) aggregate et

3. nova-consensus SlotTicker → genesis zamanı + offset tabanlı olmalı
   ↳ Mevcut: sabit interval → kaçırılan slotlar yoksayılıyor
   ↳ FIX: calcNextTick(now, genesisTime, slotDuration, offset) algoritması

YÜKSEK (İlk fazda düzeltilmeli):
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
4. nova-mempool DKG → 4 faz eksik (Accusing + Apologizing yok)
   ↳ Kötü niyetli keyper tespiti imkansız → eşik şifrelemesi güvensiz
   ↳ FIX: DkgPhase enum'a Accusing + Apologizing ekle

5. nova-network → retry mekanizması yok
   ↳ Ağ hatasında mesaj kaybolur → validator oy kaybeder
   ↳ FIX: exponential backoff (3 deneme, 2s→60s arası)

6. nova-guardian → Hold/Freeze yerine basit bool lock
   ↳ Guardian stake "freeze" ile kilitlenmeli (slash edilebilir ama transfer edilemez)
   ↳ FIX: polkadot `fungible::MutateFreeze` pattern

7. nova-runtime → BlockSTM dependency graph yok
   ↳ Çakışan TX'ler serileştiriliyor → throughput sınırlanıyor
   ↳ FIX: read/write set beyanı + optimistic execution

ORTA (İkinci fazda ele alınabilir):
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
8. nova-auction → OrderUid basit u64 yerine [u8; 56] olmalı
   ↳ EIP-712 uyumluluğu ve wallet entegrasyonu için
   ↳ FIX: OrderUid([u8; 56]) = digest(32) + owner(20) + validTo(4)

9. nova-guardian → ConsAddr key rotation geçmişi yok
   ↳ Guardian key değişiminde eski imzaları doğrulamak imkansız
   ↳ FIX: HistoricalConsAddr map + rotasyon kilidi

10. nova-vm → execModeSimulate yok
    ↳ TX simülasyonu gerçek state değiştirir → gas tahmini güvensiz
    ↳ FIX: snapshot + simulate + revert pattern

11. nova-network → P2P mesaj metrikleri yok
    ↳ Validation/handling gecikmeleri izlenemiyor
    ↳ FIX: Prometheus gauge (validation_time, handling_time)

DÜŞÜK (Sonraki fazda):
━━━━━━━━━━━━━━━━━━━
12. nova-mempool → mesaj boyutu standartlaştırılmamış
    ↳ Farklı node versiyonları uyumsuz mesaj üretebilir
    ↳ FIX: AggregateRegistrationMessage fixed 46 byte format

13. nova-vm → SlotNum + CostPerStateByte (EIP-8037) yok
    ↳ EVM uyumluluk sorunları
    ↳ FIX: BlockContext'e alan ekle

14. nova-auction → submission_deadline birkaç blok penceresi
    ↳ Tek slot çok dar → ağ gecikmesinde auction başarısız
    ↳ FIX: deadline = current_slot + config.submission_window_slots
```

---

### 8.2 Crate Bazında Etki Özeti

| Crate | Yeni Bulunan Gap | Kritik Bug? | Öncelik |
|-------|-----------------|-------------|---------|
| nova-auction | f64 skor (deterministik değil) | **EVET** | Acil |
| nova-mempool | DKG 2 faz (4 yerine), eşik == vs >= | EVET (gerçek impl'de) | Yüksek |
| nova-consensus | SlotTicker genesis offset yok | EVET | Yüksek |
| nova-network | Retry yok, metrik yok, handler zinciri yok | HAYIR | Orta |
| nova-guardian | Hold/Freeze yerine bool, key rotation geçmişi yok | HAYIR | Orta |
| nova-runtime | BlockSTM dependency graph yok, execModeSimulate yok | HAYIR | Orta |
| nova-vm | SlotNum/CostPerStateByte yok | HAYIR | Düşük |
| nova-auction | OrderUid basit u64, submission_deadline dar | HAYIR | Düşük |
| nova-mempool | Mesaj formatı standart değil | HAYIR | Düşük |

---

### 8.3 Acil Düzeltme Gerektiren Bug'lar (Mock Kodda Bile Yanlış)

**BUG #1 — nova-auction: f64 skor (deterministik değil)**
```
Konum: nova-auction/src/ (skor hesaplama)
Sorun: f64 kayan nokta işlemleri farklı CPU'larda farklı sonuç üretebilir
       → Validator A ve Validator B farklı kazanan seçer → konsensüs bozulur
Düzeltme: Score(u128) newtype, basis points (1/10000) veya wei precision kullan
Kaynak: cow-protocol Score(eth::Ether) + saturating_add_assign pattern
```

**BUG #2 — nova-consensus: Sabit interval slot timer**
```
Konum: nova-consensus/src/ (slot timer)
Sorun: tokio::time::interval(400ms) + sistem gecikmesi → slot kayması birikir
       → Uzun çalışmada validator'ların slot numaraları uyuşmaz
Düzeltme: calcNextTick(now, genesisTime, slotDuration) → absolute time bazlı
          Kaçırılan slotları doldurmak için gap fill döngüsü ekle
Kaynak: rolling-shutter slotticker/slotticker.go
```

**BUG #3 — nova-mempool: DKG eşik kontrolü (potansiyel)**
```
Konum: nova-mempool/src/ (DKG aggregation)
Sorun: Mevcut mock >= threshold ile aggregate ediyor
       Gerçek implementasyonda: tam olarak threshold kadar share gelmeli
       Fazla share kabul edilirse → eski/geçersiz share karışabilir
Düzeltme: len(shares) == threshold kontrolü (rolling-shutter pattern)
Kaynak: rolling-shutter keyper/epochkg/epochkg.go L87
```

---

### 8.4 İkinci Oturum Sonrası Güncel Öncelik Sıralaması (Tüm Gap'ler Birleşik)

```
FAZ 1A — ACİL (Üretim öncesi zorunlu):
  1. nova-auction f64 → u128 skor düzeltmesi
  2. k256 + blst + ed25519 gerçek kriptografi
  3. cggmp21 gerçek MPC keygen/signing

FAZ 1B — YÜKSEK:
  4. nova-consensus SlotTicker genesis-offset
  5. nova-mempool DKG 4 faz (Accusing+Apologizing)
  6. RocksDB kalıcı depolama
  7. libp2p gerçek P2P

FAZ 2 — ORTA:
  8. nova-network retry + metrik
  9. nova-guardian Hold/Freeze + key rotation geçmişi
  10. nova-runtime BlockSTM dependency graph
  11. async tokio runtime

FAZ 3 — DÜŞÜK:
  12. OrderUid [u8; 56] standardizasyonu
  13. execModeSimulate
  14. wasmtime gerçek VM
  15. ark-groth16 / halo2 ZK devreler
```

---

### 8.5 İkinci Oturum Sonucu

**Ne öğrenildi:**
- 8 ek kaynak dosyası okundu (rolling-shutter SlotTicker/P2PMessaging/EpochKG/DKGPhase/Voting, cow-protocol Solution/Order/RunLoop, go-ethereum TxPool/Miner/Clique/StateDB/EVM, penumbra IBC/DutchAuction/Note, cosmos-sdk BaseApp/ABCI/Staking, avalanchego Health, solana AccountsDB, polkadot Balances/Staking)
- 3 kritik bug tespit edildi (f64 skor, slot timer kayması, DKG eşik kontrolü)
- 14 orta/düşük öncelikli gap belirlendi
- Toplam gap sayısı: Bölüm 2'deki ~80 + Bölüm 8'deki 14 = ~94 gap

**Durum:**
Tüm 10 repo ana + ek dosyaları okundu, GAP-ANALIZ.md TAMAMLANDI.
Sıradaki adım: nova-auction'da f64 → u128 skor düzeltmesi (Acil BUG #1)
ve ardından FAZ 1A kriptografi implementasyonu.

---

## BÖLÜM 9 — ÜÇÜNCÜ OTURUM DERİN OKUMA: go-ethereum / penumbra / cosmos-sdk / avalanchego / solana

Bu bölümde şu dosyalar satır satır okundu:

**go-ethereum:**
- `core/blockchain.go` (2200+ satır) — BlockChain struct, InsertChain, insertChain, rewindHashHead, rewindPathHead, SetHead/SetFinalized/SetSafe, loadLastState, history pruning
- `core/state_processor.go` — Process(), PreExecution(), PostExecution(), EIP-4788/2935/6110/7002/7251 system calls
- `core/rawdb/database.go` — freezerdb, nofreezedb, Open(), genesis hash validation, freeze goroutine, Pebble/LevelDB backend seçimi
- `p2p/server.go` — Server struct, Start(), connection handshake pipeline (doEncHandshake→doProtoHandshake), connFlag (dynDialedConn/staticDialedConn/inboundConn/trustedConn), discv4/discv5/discmix, dialScheduler
- `p2p/discover/table.go` — Kademlia Table, alpha=3, bucketSize=16, maxReplacements=10, nBuckets=hashBits/15, IP limitleri (bucketIPLimit=2/24, tableIPLimit=10/24), seedCount=30
- `p2p/rlpx/rlpx.go` — RLPx v4 transport, Conn struct (sessionState: enc/dec AES-CTR + hashMAC), Handshake(), readFrame/writeFrame, authMsgV4 (Signature+InitiatorPubkey+Nonce+Version), eciesOverhead=65+16+32=113 byte, Snappy compression

**penumbra:**
- `crates/core/component/shielded-pool/src/note.rs` — Note struct (value, rseed, address, transmission_key_s: Fq), NOTE_LEN_BYTES=160, NOTE_CIPHERTEXT_BYTES=176, NOTECOMMIT_DOMAIN_SEP (blake2b), commitment() = poseidon377::hash_6(domain_sep, note_blinding, amount, asset_id, diversified_generator, transmission_key_s, clue_key), Note::encrypt() ECIES-style (esk→epk→shared_secret→PayloadKey), Note::decrypt() IVK key agreement, OVK wrapped key decryption
- `crates/core/component/shielded-pool/src/spend/proof.rs` — SpendProofPublic (anchor, balance_commitment, nullifier, rk), SpendProofPrivate (state_commitment_proof, note, v_blinding, spend_auth_randomizer, ak, nk), SpendCircuit: ark-groth16 Bls12_377, GROTH16_PROOF_LENGTH_BYTES, prove(blinding_r, blinding_s, pk, ...) → LibsnarkReduction, verify() with PreparedVerifyingKey, constraints: note_commitment_integrity + nullifier_integrity + merkle_auth_path + rk_integrity + diversified_address_integrity + balance_commitment_integrity

**cosmos-sdk:**
- `x/bank/keeper/keeper.go` — Keeper interface (SendCoinsFromModuleToAccount, MintCoins, BurnCoins, VirtualAccounts, DelegateCoins, UndelegateCoins), BaseKeeper (BaseSendKeeper+AccountKeeper+mintCoinsRestrictionFn), WithMintCoinsRestriction() zincirleme
- `x/bank/keeper/send.go` — BaseSendKeeper (sendRestriction zinciri), InputOutputCoins() (multi-send), SendCoins() (blockedAddrs kontrolü, IsSendEnabledCoins kontrolü, subUnlockedCoins+addCoins, EventManager emit), AppendSendRestriction/PrependSendRestriction/ClearSendRestriction
- `x/staking/keeper/keeper.go` — Keeper (storeService+authKeeper+bankKeeper+hooks+validatorAddressCodec+consensusAddressCodec), BondedPoolName+NotBondedPoolName+KeyRotationFeePoolName zorunlu module accounts, SetHooks() (bir kez çağrılabilir, ikinci kez panic), GetLastTotalPower/SetLastTotalPower (IntProto marshal)
- `baseapp/baseapp.go` — BaseApp struct (cms CommitMultiStore, grpcQueryRouter, msgServiceRouter, mempool, anteHandler, postHandler, abciHandlers, snapshotManager, stateManager, interBlockCache, gasConfig, haltHeight/haltTime, minRetainBlocks, optimisticExec, disableBlockGasMeter=true, txRunner), execMode sabitleri (Check/ReCheck/Simulate/PrepareProposal/ProcessProposal/VoteExtension/VerifyVoteExtension/Finalize)

**avalanchego:**
- `snow/consensus/simplex/parameters.go` — Parameters (MaxNetworkDelay=5s, MaxRebroadcastWait=5s, InitialValidators []ValidatorInfo), ValidatorInfo (NodeID, PublicKey kompres), Verify() kontrolü

**solana:**
- `core/src/banking_stage.rs` — NUM_THREADS=6, TOTAL_BUFFERED_PACKETS=700_000, NUM_VOTE_PROCESSING_THREADS=2, SLOT_BOUNDARY_CHECK_PERIOD=10ms, BankingStageStats (AtomicU64/AtomicUsize metrikleri), BatchedTransactionDetails (cost+error istatistikleri)
- `core/src/banking_stage/consumer.rs` — Consumer (committer+transaction_recorder+qos_service), TARGET_NUM_TRANSACTIONS_PER_BATCH=64, consume_buffered_packets() → process_packets() → do_process_packets() → process_packets_transactions(), reached_end_of_slot kontrolü (reached_max_poh_height || should_working_bank_still_be_processing_txs), ExecuteAndCommitTransactionsOutput (retryable_transaction_indexes, commit_transactions_result)

---

### 9.1 go-ethereum'dan Çıkarılan Kritik Bilgiler

#### 9.1.1 BlockChain Mimarisi

```
BlockChain struct içindeki atomik işaretçiler (atomic.Pointer):
  currentBlock      → Head of chain (Header only, not full Block)
  currentSnapBlock  → Snap sync head
  currentFinalBlock → Consensus finalized block (Engine API SetFinalized)
  currentSafeBlock  → Consensus safe block (Engine API SetSafe)

Bu 4 farklı "head" konsepti Nova'da eksik:
  Nova'da: sadece tek "best_block" kavramı var
  Ethereum'da: safe < finalized < canonical head ayrımı kritik

BlockChainVersion = 9 → DB şeması değişince resync zorunlu
```

#### 9.1.2 State Processor: EIP Sistemi

```
Process() akışı:
1. PreExecution(beaconRoot, parent, evm) → EIP-4788 beacon root, EIP-2935 parent hash
2. for tx in block.Transactions():
     msg = TransactionToMessage(tx, signer, baseFee)
     statedb.SetTxContext(txHash, txIndex)
     receipt, bal = ApplyTransactionWithEVM(msg, gp, statedb, ...)
3. PostExecution() → EIP-6110 deposit logs, EIP-7002 withdrawal, EIP-7251 consolidation
4. engine.Finalize() → block rewards (ethash) veya attestation (beacon)

Nova'nın StateProcessor'ı:
  - PreExecution sistemi YOK (system call'lar)
  - EIP-1559 gas hesabı VAR ama incomplete
  - PostExecution (epoch-level işlemler) YOK
```

#### 9.1.3 RawDB: Çift Katmanlı Depolama

```
freezerdb yapısı:
  KeyValueStore (LevelDB/Pebble) → yeni bloklar, state, receipts (hot)
  chainFreezer (dosya bazlı freezer) → eski bloklar (cold, immutable)

Freezer goroutine:
  - Her N dakikada yeni blokları frozen hale taşır
  - Atomic rename (tmp file → final) ile crash-safe
  - Genesis hash uyuşmazlığı kontrolü

Nova'da depolama:
  - Şu an mock (HashMap) → RocksDB ile değiştirilecek
  - Freezer kavramı hiç yok → tarihsel veriler için gerekli
  - Atomik yazma (WAL) yok → crash → veri kaybı
```

#### 9.1.4 P2P Stack: Katmanlı Mimari

```
Katman 1 — Discovery (UDP):
  discv4 (disc v4 Kademlia)
  discv5 (disc v5 topic-based)
  discmix (FairMix: ikisini dengeli birleştirir)

Katman 2 — Transport (TCP):
  RLPx Conn:
    Handshake: ECIES authMsg (Signature+InitiatorPubkey+Nonce) → reciprocal
    Session: AES-128-CTR encryption, Keccak256 MAC (hashMAC)
    Frame format: [header 16B][header MAC 16B][frame data][frame MAC 16B]
    Snappy compression: enabled after Hello message exchange

Katman 3 — Multiplexing:
  protoHandshake: Hello message (capabilities: eth/68, snap/1, ...)
  Cap: {Name string, Version uint}

Katman 4 — Protocols:
  eth: transaction gossip, block announce, state sync
  snap: fast state download

Nova'da P2P:
  - Noise XX encryption VAR (mock)
  - Discovery: Kademlia mock VAR
  - Gossipsub: mock VAR
  - Frame-level MAC: YOK → mesaj bütünlüğü güvensiz
  - Snappy compression: YOK → bandwidth verimsiz
  - Capability negotiation: YOK → protocol versiyonlama yok
```

#### 9.1.5 Kademlia Tablo Parametreleri

```
alpha = 3        // Paralel lookup concurrency
bucketSize = 16  // K-bucket max boyutu
maxReplacements = 10  // Bucket replacement listesi
nBuckets = 256/15 = 17  // Uzak → yakın uzaklık bucketları

IP limitleri (Sybil koruması):
  bucketIPLimit=2, bucketSubnet=24  // Tek /24'ten max 2 node per bucket
  tableIPLimit=10, tableSubnet=24   // Tabloda toplam max 10 node per /24

seedCount=30, seedMaxAge=5*24h  // DB'den seed node yükleme

Nova'da Kademlia:
  Mock implementasyon, IP limiti yok → Sybil saldırısına açık
```

---

### 9.2 Penumbra'dan Çıkarılan Kritik Bilgiler

#### 9.2.1 Note Commitment Formülü (Poseidon377)

```rust
// Penumbra note commitment = Poseidon hash (ZK-dostu)
commitment = poseidon377::hash_6(
    NOTECOMMIT_DOMAIN_SEP,  // blake2b("penumbra.notecommit")
    (
        note_blinding: Fq,                          // rseed'den türetilir
        value.amount: Fq,                           // u128 → field
        value.asset_id.0: Fq,                       // asset kimliği
        diversified_generator.vartime_compress_to_field(),  // curve point
        transmission_key_s: Fq,                    // addr doğrulama
        clue_key: Fq,                               // FMD clue key
    )
)

Nova'da NoteCommitment:
  BLAKE3(domain_sep || value || asset_id || randomness || transmission_key)
  → ZK-dostu değil (Poseidon kullanılmalı)
  → Groth16 devresine bağlamak için Poseidon gerekli
```

#### 9.2.2 SpendProof Devresindeki Kısıtlar

```
SpendCircuit kısıtları (ark-groth16 üzerinde, Bls12-377 eğrisi):
1. note_commitment_integrity: NoteVar.commit() == claimed_note_commitment
2. nullifier_integrity: NullifierVar::derive(nk, position, cm) == claimed_nullifier
3. merkle_auth_path: (amount > 0 ise) MerkleAuthPathVar.verify(anchor, cm)
   NOT: dummy spend (amount=0) için merkle path bypass edilir
4. rk_integrity: ak.randomize(spend_auth_randomizer) == rk
5. diversified_address: ivk.diversified_public(g_d) == transmission_key (note'daki)
6. balance_commitment: note.value.commit(v_blinding) == claimed_balance_commitment

Önemli: diversified_generator != identity + ak != identity kontrolleri
         allocation doğrulama (NoteVar içinde)

Nova'da ZK SpendCircuit:
  - Kısıtlar mock → gerçek R1CS yok
  - dummy spend bypass yok → her spend için merkle path gerekli (yanlış)
  - v_blinding commitment yok → balance değişimi gizlenmiyor
```

#### 9.2.3 Note Şifreleme / Şifre Çözme

```
Şifreleme (IVK ile alıcı okuyabilir):
  esk = rseed.derive_esk()                    // ephemeral secret key
  epk = esk.diversified_public(g_d)           // ephemeral public key
  shared_secret = esk.key_agreement(transmission_key)
  key = PayloadKey::derive(shared_secret, epk)
  ciphertext = key.encrypt(note_bytes, PayloadKind::Note)  // 160 → 176 bytes

Şifre Çözme (alıcı, IVK ile):
  shared_secret = ivk.key_agreement(epk)
  key = PayloadKey::derive(shared_secret, epk)
  plaintext = key.decrypt(ciphertext, PayloadKind::Note)
  // ZIP 212 integrity check: note.ephemeral_public_key() == epk

OVK ile gönderen de okuyabilir (OutgoingCipherKey):
  ock = OutgoingCipherKey::derive(ovk, cv, cm, epk)
  wrapped_key = ock.encrypt(shared_secret)  // OvkWrappedKey

Nova'nın şifreli not sistemi:
  - BLAKE3 ile şifreleme var (mock)
  - ZIP 212 epk integrity check YOK → re-encryption saldırısına açık
  - OVK wrapped key YOK → gönderen kendi gönderdiği notu göremez
```

---

### 9.3 Cosmos-SDK'dan Çıkarılan Kritik Bilgiler

#### 9.3.1 BaseApp ExecMode Sistemi

```go
execMode sabitleri:
  execModeCheck               // mempool kontrolü (imza + fee)
  execModeReCheck             // blok sonrası mempool temizliği
  execModeSimulate            // gas tahmini (state değişmez)
  execModePrepareProposal     // proposer TX seçimi
  execModeProcessProposal     // diğer validator'ların doğrulaması
  execModeVoteExtension       // pre-commit oy eki (ABCI 2.0)
  execModeVerifyVoteExtension // oy eki doğrulama
  execModeFinalize            // blok kesinleşti → state commit

Nova'da TX modu:
  Sadece tek "execute" modu var
  Simulate modu YOK → gas tahmini gerçek state değiştirir
  PrepareProposal/ProcessProposal ayrımı YOK → proposer önyüklemesi açık
```

#### 9.3.2 Bank Modülü Mimarisi

```
Keeper hiyerarşisi:
  BaseViewKeeper → read-only (GetBalance, GetAllBalances, GetAccountsBalances)
    ↓
  BaseSendKeeper → transfer (SendCoins, InputOutputCoins) + SendRestriction zinciri
    ↓
  BaseKeeper → tam yetkili (MintCoins, BurnCoins, DelegateCoins, VirtualAccounts)

SendRestriction zinciri:
  AppendSendRestriction() → sonra çalışır
  PrependSendRestriction() → önce çalışır
  → Birden fazla kısıtlama fonksiyonunu zincirleyerek uygulatmak mümkün
  → Nova'da SendRestriction zinciri yok → basit bool kontrolü

VirtualAccounts (yeni cosmos özelliği):
  SendCoinsFromAccountToModuleVirtual() → gerçekte deduction yapılmaz
  CreditVirtualAccounts() → blok sonunda toplu kredi
  → Nova'da yok
```

#### 9.3.3 Staking: Module Account Zorunluluğu

```
NewKeeper() kontrolleri:
  1. BondedPoolName module account mevcut olmalı (bonded stake)
  2. NotBondedPoolName module account mevcut olmalı (unbonding stake)
  3. KeyRotationFeePoolName module account mevcut olmalı (key rotation ücreti)
  4. authority (gov module address) geçerli AccAddress olmalı
  5. validatorAddressCodec != nil
  6. consensusAddressCodec != nil

Validator key rotation desteği:
  - KeyRotationFeePoolName → rotation başvurusunda ücret kesilir
  - ConsAddr rotasyonu → eski consensus address kilitlenir (period süresince)
  → Nova'da validator key rotation tamamen yok

SetHooks() kısıtı:
  - Sadece bir kez çağrılabilir
  - İkinci çağrı → panic
  → Nova'da hooks sistemi yok
```

---

### 9.4 Avalanchego Simplex BFT'den Çıkarılan Bilgiler

```go
Parameters:
  MaxNetworkDelay    = 5s  // Ağ gecikmesi üst sınırı
  MaxRebroadcastWait = 5s  // Yeniden yayın bekleme süresi
  InitialValidators  []ValidatorInfo  // NodeID + kompres PublicKey

ValidatorInfo:
  NodeID: ids.NodeID  // 20 byte keccak256(pubkey)
  PublicKey: []byte   // BLS kompres (48 byte, BLS12-381 G1)

Verify() kontrolleri:
  MaxNetworkDelay > 0
  MaxRebroadcastWait > 0
  len(InitialValidators) > 0

Nova consensus karşılaştırması:
  Nova nova-consensus: ValidatorId (u64), BLS imza (Vec<u8> mock)
  Avalanche Simplex: NodeID (20B keccak) + compressed BLS pubkey
  → Nova'da NodeID ≠ gerçek node kimliği, sadece index
  → Nova'da BLS pubkey doğrulaması hiç yok
```

---

### 9.5 Solana Banking Stage'den Çıkarılan Bilgiler

#### 9.5.1 İş Parçacığı Mimarisi

```
NUM_THREADS = 6           // Sabit (GCP'de en hızlı bu)
TOTAL_BUFFERED_PACKETS = 700_000  // Toplam buffer kapasitesi
NUM_VOTE_PROCESSING_THREADS = 2   // Oy TX'leri için ayrı thread'ler
SLOT_BOUNDARY_CHECK_PERIOD = 10ms // Slot sınırı ne sıklıkla kontrol edilir

Thread tipleri:
  2 thread → oy TX'leri (latency kritik)
  4 thread → normal TX'ler (throughput kritik)
  Her thread bağımsız UnprocessedTransactionStorage yönetir

Nova'da mempool:
  Tek thread, sıralı işleme → çok düşük throughput
  Oy TX'leri için öncelik yok
```

#### 9.5.2 Consumer: TX İşleme Akışı

```
consume_buffered_packets():
  bank_start (current bank + start time) →
  process_packets() → closure ile do_process_packets() →
  process_packets_transactions():
    1. QosService → cost model filter (bütçe kontrol)
    2. Bank::load_and_execute_transactions()
    3. Committer::commit_transactions() → PoH stream'e yaz

TARGET_NUM_TRANSACTIONS_PER_BATCH = 64  // Batch boyutu

reached_end_of_slot kriterleri:
  reached_max_poh_height  // PoH sayacı slot sonuna ulaştı
  OR !should_working_bank_still_be_processing_txs()  // bank "frozen"

retryable_transaction_indexes:
  - Block limit → yeniden dene
  - Account limit → yeniden dene
  - Fatal error (double-spend, too old) → drop

Nova'da TX işleme:
  Batch boyutu yok (tek tek işleme)
  PoH entegrasyonu yok (VDF clock yok)
  QosService (cost model) yok → blok dolar kontrolü yok
  retryable vs fatal hata ayrımı yok
```

---

### 9.6 Bu Oturumdan Bulunan Yeni Gap'ler

| # | Gap | Kaynak | Nova Durumu |
|---|-----|--------|-------------|
| 1 | 4 farklı "head" kavramı (safe/finalized/snap/canonical) | go-ethereum | Yok (tek head) |
| 2 | AES-CTR + Keccak256 MAC frame şifreleme (RLPx) | go-ethereum | Yok (Noise mock) |
| 3 | Snappy frame compression | go-ethereum | Yok |
| 4 | Capability negotiation (Hello protocol) | go-ethereum | Yok |
| 5 | Freezer: hot (KV store) + cold (file freezer) çift katman | go-ethereum | Yok (tek HashMap) |
| 6 | Kademlia IP limitleri (Sybil koruması, /24 subnet) | go-ethereum | Yok |
| 7 | Poseidon377 note commitment (ZK-dostu hash) | penumbra | Yok (BLAKE3 var, ZK uyumsuz) |
| 8 | dummy spend bypass (amount=0 → merkle skip) | penumbra | Yok |
| 9 | ZIP 212 epk integrity check | penumbra | Yok |
| 10 | OVK wrapped key (gönderen kendi notunu okuyabilir) | penumbra | Yok |
| 11 | execModeSimulate (gas tahmin → no state change) | cosmos-sdk | Yok |
| 12 | PrepareProposal / ProcessProposal ayrımı | cosmos-sdk | Yok |
| 13 | SendRestriction zinciri (bank transfer hook) | cosmos-sdk | Yok |
| 14 | Validator key rotation + KeyRotationFeePool | cosmos-sdk | Yok |
| 15 | Staking hooks sistemi (SetHooks, MultiStakingHooks) | cosmos-sdk | Yok |
| 16 | NodeID = keccak(pubkey)[20B] gerçek node kimliği | avalanchego | Yok (u64 index) |
| 17 | BLS pubkey doğrulama (Simplex ValidatorInfo) | avalanchego | Yok |
| 18 | 2 ayrı thread (vote TX vs normal TX işleme) | solana | Yok |
| 19 | QosService cost model (blok bütçe kontrolü) | solana | Yok |
| 20 | retryable vs fatal TX ayrımı + buffer re-queue | solana | Yok |

---

### 9.7 Bu Oturumdan Tespit Edilen Yeni Bug'lar

**BUG #4 — nova-zk: Dummy Spend için Merkle Path Bypass Yok**
```
Konum: nova-zk/src/ (spend circuit)
Sorun: Penumbra'da amount=0 olan "dummy spend"'ler için merkle path doğrulaması
       atlanır (ZK dışı da geçerli). Nova'da her spend için merkle proof gerekli.
       → Gizlilik arttırıcı dummy spend işlemi yapılamaz
       → Privacy setini zayıflatır (girdi sayısından bilgi sızar)
Düzeltme: is_not_dummy = amount > 0; merkle_path.conditional_verify(is_not_dummy)
Kaynak: penumbra spend/proof.rs L192-199
```

**BUG #5 — nova-zk: Note Commitment ZK-Uyumsuz**
```
Konum: nova-zk/src/ (note commitment)
Sorun: BLAKE3 kullanılıyor → R1CS devresine entegre etmek çok maliyetli
       (BLAKE3 kısıtlamaları >> Poseidon kısıtlamaları)
       → Groth16 proving time kabul edilemez derecede uzar
Düzeltme: Poseidon hash kullan (poseidon377 veya poseidon2)
Kaynak: penumbra note.rs L375-388 (poseidon377::hash_6)
```

**BUG #6 — nova-network: Frame-level MAC Yok**
```
Konum: nova-network/src/ (transport)
Sorun: Mock Noise XX el sıkışması var ama frame-level MAC yok
       → Bağlantı kurulduktan sonra mesaj değiştirilebilir
       → Man-in-the-middle saldırısına karşı koruma yok
Düzeltme: AES-128-CTR encryption + Keccak256 MAC (RLPx v4 pattern)
          veya Noise XX ile proper frame authentication
Kaynak: go-ethereum p2p/rlpx/rlpx.go (sessionState.enc/dec/egressMAC/ingressMAC)
```

---

### 9.8 Güncel Toplam Gap Özeti (3 Oturum Birleşik)

```
Bölüm 2 (ilk tarama): ~80 gap
Bölüm 8 (ikinci oturum): +14 gap, 3 bug
Bölüm 9 (bu oturum): +20 gap, 3 yeni bug

Toplam: ~114 gap
Kritik bug: 6 (BUG #1-#6)

BUG #1: nova-auction f64 skor → deterministik değil → konsensüs bozulur
BUG #2: nova-consensus sabit interval slot timer → slot kayması
BUG #3: nova-mempool DKG eşik >= vs == → fazla share kabul
BUG #4: nova-zk dummy spend merkle bypass yok → privacy set zayıflıyor
BUG #5: nova-zk BLAKE3 note commitment → Groth16 proving çok yavaş
BUG #6: nova-network frame MAC yok → MITM saldırısına açık
```

---

### 9.9 Üçüncü Oturum Sonucu

**Okunan toplam kaynak:**
- go-ethereum: blockchain.go (~2200 satır), state_processor.go, rawdb/database.go, p2p/server.go, p2p/discover/table.go, p2p/rlpx/rlpx.go
- penumbra: shielded-pool/note.rs, spend/proof.rs
- cosmos-sdk: bank/keeper/keeper.go, bank/keeper/send.go, staking/keeper/keeper.go, baseapp/baseapp.go
- avalanchego: snow/consensus/simplex/parameters.go
- solana: core/src/banking_stage.rs, banking_stage/consumer.rs

**Ana bulgular:**
1. go-ethereum'un 4-head sistemi (safe/finalized/snap/canonical) Nova'da hiç yok
2. RLPx frame-level şifreleme + MAC → Nova'da güvensiz transport
3. Penumbra Poseidon hash → Nova BLAKE3 ZK-uyumsuz
4. Cosmos BaseApp execMode → Nova'da sadece 1 mod
5. Solana 6 banking thread + QosService → Nova tek thread, bütçe kontrolü yok
6. Simplex BFT NodeID = keccak(pubkey) → Nova u64 index

**Durum:** Tüm 10 repo tüm ana yapı dosyaları okundu ve belgelendi.
Sonraki adım: Bug fix'lere başla (BUG #1 → nova-auction f64 → u128)
