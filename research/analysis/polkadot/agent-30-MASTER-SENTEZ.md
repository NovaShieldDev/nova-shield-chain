# Agent-30: Polkadot SDK MASTER SENTEZ (Tüm 29 Agent)

## Kapsam: Tüm Batch 1+2+3

**Tarih:** 2026-07-22
**Kapsanan Agentler:** agent-01 .. agent-29 (29 agent, 5080 dosya, ~157k satir)
**Cikti:** Nova Shield Chain icin eksiksiz mimari referans

---

## 1. Mimari Genel Bakis

Polkadot SDK (Substrate), "meta-protocol" yaklasimyla calisir: runtime WASM olarak derlenir ve zincir verisi olarak tutulur; boylece forkless upgrade mumkun olur.

### 1.1 Temel Katmanlar

```
+----------------------------------------------------------+
|  Runtime (WASM)                                           |
|  +----------+ +----------+ +----------+ +-----------+    |
|  |frame-sys | |balances  | |staking   | | pallets…  |    |
|  +----------+ +----------+ +----------+ +-----------+    |
|  frame-executive (blok lifecycle orchestration)           |
|  frame-support  (macro altyapisi, storage, dispatch)      |
+----------------------------------------------------------+
|  Client (Native Rust)                                     |
|  consensus | tx-pool | rpc | network | database           |
+----------------------------------------------------------+
|  Primitives (sp-*)                                        |
|  sp-core | sp-runtime | sp-arithmetic | sp-trie | …       |
+----------------------------------------------------------+
```

### 1.2 Runtime Upgrade Yasam Dongusu [agent-08]

1. `set_code` → WASM dogrulama → storage'a kayit
2. Sonraki blokta `on_runtime_upgrade()` cagirilir
3. `StorageVersion` kontrol edilir; migration yalnizca dogru versiyon icin calisir
4. `OnRuntimeUpgrade` trait'i (frame-executive) — Eylul 2026'da DEPRECATED olacak
5. Yeni sistem: `pallet-migrations` multi-block framework [agent-29]

### 1.3 Blok Yasam Dongusu [agent-08]

```
initialize_block()
  -> on_initialize() (her pallet, weight limiti dahilinde)
  -> execute_extrinsics() (inherent once, signed sonra)
  -> on_idle() (kalan weight ile)
  -> on_finalize()
  -> finalize_block()
```

### 1.4 construct_runtime! ve Config Trait [agent-07]

- `construct_runtime!` makrosu tum pallet tiplerini birlestir
- Her pallet `trait Config: frame_system::Config` ile yapilandirilir
- `#[pallet::storage]`, `#[pallet::event]`, `#[pallet::error]` attribute macro'lari
- `#[pallet::hooks]`: `on_initialize`, `on_finalize`, `on_idle`, `on_poll`, `offchain_worker`
- `#[pallet::validate_unsigned]`: unsigned transaction dogrulama
- `EnsureOrigin` trait: origin sistemi (Root, Signed, None)

### 1.5 EXTRINSIC_FORMAT_VERSION [agent-01]

- Versiyon: **5**
- CheckWeight extension zorunlu
- WeightReclaim: blok sonunda kullanilmayan weight geri verilir
- Inherent extrinsic'ler imzasiz, dispatch sirasi belirlenmiş

---

## 2. Ekonomik Model

### 2.1 Fee Sistemi [agent-04]

```
final_fee = base_fee + len_fee + (multiplier * weight_fee) + tip
```

- **base_fee**: `ExtrinsicBaseWeight * WeightToFee`
- **len_fee**: `LengthToFee * len(encoded_extrinsic)`
- **weight_fee**: `dispatch_weight * WeightToFee`
- **multiplier**: `TargetedFeeAdjustment` (EIP-1559 benzeri, hedef %25 blok doluluk)
- **tip**: istege bagli, priority'yi arttirir
- `Pays::No` ile bazi dispatch'ler ucretsiz (ornegin sudo [agent-24])
- `ChargeTransactionPayment` signed extension, `pre_dispatch` -> `post_dispatch` zinciri

### 2.2 Staking ve Oduller [agent-03]

NPoS (Nominated Proof of Stake):

- **Odul Formulu:** `REWARD_F1 = 50%` (validators'a), kalan nominators'a
- **Yillik hesaplama:** `MILLISECONDS_PER_YEAR = 31_557_600_000`
- **Slash Formulu (staking/equivocation):** `min((3k/n)^2, 1)` — k=suclu validator sayisi, n=toplam validator
- **im-online Slash:** `min((3*(k - n/10 - 1))/n, 1) * 0.07` [agent-09]
- Era hesabi: `reward = era_reward_points_ratio * total_era_reward`
- **UYARI:** pallet-staking DEPRECATED, yeni chain'ler `pallet-staking-async` kullanmali

**Polkadot ideal staking parametreleri [agent-25]:**
- `ideal_stake = 75%`
- `min_inflation = 2.5%`
- `max_inflation = 10%`
- `falloff = 5%`

**Fee dagitimi [agent-25]:**
- %80 treasury
- %20 block author
- %100 tip -> block author

### 2.3 Treasury ve Governance Finans [agent-18]

```
burned = budget * (1 - (1-Burn)^N)
```

- Burn hesabi N donem boyunca birikimli, additive DEGIL
- Burn yalnizca `missed_any == false` iken gerceklesir (tum harcamalar onaylandiysa)
- `spend_local` DEPRECATED (Mayis 2025'te kaldirildi)
- Tip median formulu: `tips[len/2]` (lower-median, NOT ceiling)
- Tip threshold: `ceil(N/2)` (yaridan fazla imzaci)
- Finder fee: `TipFindersFee * median_tip`

---

## 3. Consensus

### 3.1 BABE [agent-05, agent-20]

**Primary Slot Threshold:**
```
p = 1 - (1-c)^theta
threshold = (2^128) * p
```
- `c` = hedef doluluk orani
- `theta` = `authority_stake / total_stake`

**Secondary Slot (VRF fallback):**
```
idx = blake2_256(randomness || slot) % n_authorities
```

**Epoch Randomness:**
- Epoch N'nin randomness'i, Epoch N-2'de uretilmis VRF ciktilarindan turetilir
- Session N+2 gecikme kurali (yeni auth set 2 oturum sonra aktif)

**Equivocation Slash:** `min((3k/n)^2, 1)` [agent-05]

**Equivocation window:** `MAX_SLOT_CAPACITY = 1000` slot [agent-20]

### 3.2 GRANDPA [agent-06, agent-20]

- Chain-based BFT finality protokolu
- `CATCH_UP_THRESHOLD = 2` round
- Finality icin 2/3+1 supermajority gerekli
- Prevote -> Precommit -> Commit asamalari
- Notification maksimum: **1 MB**
- `LUCKY_PEERS = 4` (gossip hedifi)
- Justification: imzalanmis precommit seti, hafif istemci dogrulamasi icin yeterli

**Equivocation Slash:** `min((3k/n)^2, 1)` (staking'le ayni formul)

### 3.3 AURA ve Slots [agent-20]

```
author = authorities[slot % n_authorities]
```

- Deterministik, VRF kullanmaz
- Slot suresi sabit (runtime konfigurasyonu)
- Slots pallet: `SlotDuration`, `on_new_slot()` callback
- Blok import MAX: **128** (sync sirasinda)
- Warp sync minimum peer: **3**

---

## 4. Token ve Bakiye Modeli [agent-01, agent-02]

### 4.1 Balance Sistemi

```
spendable = free - max(frozen, reserved) - existential_deposit
```

- `free`: kullanilabilir bakiye havuzu
- `reserved`: hold'lar icin ayrilmis (erisilemez)
- `frozen`: transfer kisitlamasi (lock/freeze tabanli)

### 4.2 Locks, Freezes, Holds

**KRITIK FORMUL:**
```
frozen = max(all_locks, all_freezes)   // ADDITIVE DEGIL, MAX alinir
```

- **Locks** (eski API): birden fazla lock MAX'a gore birlesir
- **Freezes** (yeni API): named, pallet bazinda, MAX'a gore birlesir
- **Holds** (en yeni API): named, SUM'lanir (birbirinden bagimsiz azalir)
- `fungible::Inspect`, `fungible::Mutate`, `fungible::hold::Mutate` trait'leri

**Untouchable formulu [agent-13, asset-level]:**
```
untouchable = frozen.saturating_sub(held).max(min_balance)
```
(PR #4530 ile duzeltildi)

**ExistentialDeposit:** hesap acik kalmak icin minimum bakiye; altina dusunce hesap silinir

### 4.3 Vesting [agent-16]

```
locked_at(n) = locked - ((n - starting_block) * per_block)
```

- `vest()` manuel cagri gerektirir (otomatik DEGIL)
- `vested_transfer()`: baskasina vesting schedule baslat
- `merge_schedules()`: iki schedule'i birlestir
- `MaxVestingSchedules`: konfigurasyona bagli (genelde 28)

---

## 5. DEX [agent-11, agent-12, agent-13, agent-14]

### 5.1 AMM Formuller (TAM ve DOGRULANAN)

**Liquidity Pool:** `x * y = k` (Uniswap V2 modeli)

**Olcekleme Sabiti:** `A = 1_000_000` (fee hassasiyeti icin)

**Amount Out (verilen miktar icin cikti):**
```
get_amount_out = (amount_in * (A - fee) * reserve_out)
                 / (reserve_in * A + amount_in * (A - fee))
```

**Amount In (istenen cikti icin gerekli girdi):**
```
get_amount_in = (reserve_in * amount_out * A)
                / ((reserve_out - amount_out) * (A - fee)) + 1
```
NOT: +1 = ceiling division (aliciyi koruma)

**Ilk Likidite LP Token Mint:**
```
lp_minted = sqrt(amount1 * amount2) - MintMinLiquidity
```
- `MintMinLiquidity = 100` (dead shares, pool'un omru boyunca kilitli)

**Sonraki Likidite LP Token Mint:**
```
lp_minted = min(amount1 * supply / reserve1, amount2 * supply / reserve2)
```

**Konfigurasyon Degerleri:**
- `LPFee = 3` (= 0.3%, A=1_000_000 baz alinarak)
- `MaxSwapPathLength = 4` (maksimum hop sayisi)
- `HigherPrecisionBalance = U256` (overflow onlemi)
- Pool ID: `(AssetId, AssetId)` cifti, sirali

### 5.2 Asset Sistemi [agent-13, agent-14]

**Roller:**
- `Owner`: tam kontrol
- `Issuer`: token mint edebilir
- `Admin`: dondurabilir/zorunlu transfer
- `Freezer`: dondurabilir

**Approval Sistemi (KRITIK BUG ALANI):**
- Approval'lar ADDITIVE (saturating_add) — ERC-20 gibi reset DEGIL
- `approve_transfer` her cagirida mevcut approval'a EKLER
- `TransferredApproved` eventi tanimlanmis ama ASLA emit edilmiyor (lib.rs:662) — BUG

**Destroying State (4 adim, GERI ALIŞI YOK):**
1. `start_destroy()` -> Freezing durumuna gecer
2. `destroy_accounts()` -> tum hesaplari temizler
3. `destroy_approvals()` -> tum approval'lari temizler
4. `finish_destroy()` -> asset'i siler

**Untouchable:**
```
untouchable = frozen.saturating_sub(held).max(min_balance)
```

**Limit:** `MAX_RESERVES = 5` per asset per account

### 5.3 Guvenlik

**TOCTOU Workaround [agent-12]:**
- SDK issue #1698: ayni blokta fiyat manipulasyonu riski
- Workaround: swap oncesi/sonrasi reserve kontrolu
- `with_transaction` ile atomik swap garantisi

**Eksik Ozellikler (Nova Shield icin dikkat):**
- Deadline parametresi YOK (Uniswap V2'de var)
- `Swap` trait'inde slippage opsiyonel (risk)
- Price oracle entegrasyonu runtime'a birakilmis

---

## 6. Governance [agent-10, agent-17, agent-28]

### 6.1 Democracy + Referenda [agent-10]

**pallet-democracy (ESKI, DEPRECATED yonde):**
- SuperMajority formulu: `nays / sqrt(turnout) < ayes / sqrt(electorate)`
- External proposals, public queue, council integration
- Simple majority vs super majority threshold'lari

**pallet-referenda (OpenGov, YENI):**
- Track-based sistemi: her track farkli `min_approval`, `min_support`, `prepare_period`
- `Decision Deposit` zorunlu (spam onlemi)
- Onay egrisi: `LinearDecreasing` veya `Reciprocal`
- `confirm_period`: onay esigi N blok korunmali
- Multi-track eszamanli referandum

### 6.2 Conviction Voting [agent-28]

```
vote_weight = balance * conviction_multiplier
```

| Conviction | Carpan | Lock Suresi |
|-----------|--------|-------------|
| None      | 0.1x   | 0 donem     |
| Locked1x  | 1x     | 1 donem     |
| Locked2x  | 2x     | 2 donem     |
| Locked3x  | 3x     | 4 donem     |
| Locked4x  | 4x     | 8 donem     |
| Locked5x  | 5x     | 16 donem    |
| Locked6x  | 6x     | 32 donem    |

Lock suresi HER KADEMEDE IKIYE KATLANIR.

**Lock Hesabi:**
```
lock = MAX(all active votes, prior_lock)
prior_lock = MAX(all durations, MAX(all amounts))
```

**Delegation:** yalnizca 1 seviye (no chaining)

**CONVICTION_VOTING_ID:** `b"pyconvot"`

**Support ve Approval eslikleri:**
```
Support   = aye_capital / MaxTurnout
Approval  = ayes / (ayes + nays)
```

### 6.3 Collective + Ranked Collective [agent-17]

**pallet-collective:**
- Threshold: tam sayi `MemberCount` (enum degil)
- Erken kapanma — onay: `yes_votes >= threshold`
- Erken kapanma — ret: `(seats - no_votes) < threshold`
- `MaxMembers` zorlanmaz (yalnizca weight hesabi icin)
- `prime` uye beraberligi bozar

**pallet-ranked-collective:**
- Geometrik oy agirligi: `v * (v + 1) / 2` (rank v icin)
- Rank-based membership, `promote_member` / `demote_member`
- `MemberCount` tracked per rank

### 6.4 Treasury + Bounties [agent-18]

- **Harcama dongusu:** `SpendPeriod` blok aralikla
- **Burn:** `burned = budget * (1 - (1-Burn)^N)` — birden fazla donem icin birikimli
- **Bounty:** Curator sistemi, `BountyDepositBase + BountyDepositMultiplier * value`
- **Child Bounty:** Ana bounty'den bagimsiz, curator ayri
- **Tip:** `tips[len/2]` (lower-median), threshold=`ceil(N/2)`
- **spend_local:** DEPRECATED (Mayis 2025), `spend()` kullanin

---

## 7. Kimlik ve Erisim [agent-19]

### 7.1 Identity

**Deposit Formulu:**
```
deposit = BasicDeposit + ByteDeposit * encoded_size
```

- Subs: `SubAccountDeposit` per sub-hesap
- `judgement`: Reasonable, KnownGood, Erroneous, FeePaid, OutOfDate, LowQuality
- Registrar sistemi: birden fazla onay kaynagi
- Acik kimlik alanlari: display, legal, web, riot, email, pgp, image, twitter

### 7.2 Proxy

**Pure Proxy Adres Turetme:**
```
address = blake2_256(
    b"modlpy/proxy____"
    || who         // olusturan hesap
    || height      // blok yuksekligi
    || ext_index   // extrinsic indeksi
    || proxy_type  // proxy tipi
    || index       // ek disambiguator
)
```

- `ProxyType`: Any, NonTransfer, Governance, Staking, vb. (runtime'a ozel)
- Delay: `announce` -> bekleme -> `proxy_announced`
- `AnnouncementDeposit`, `ProxyDepositBase + ProxyDepositFactor * n`

### 7.3 Multisig

**Multisig Hesap Adresi:**
```
address = blake2_256(b"modlpy/utilisuba" || sorted_signatories || threshold)
```

**Deposit Formulu (KRITIK):**
```
deposit = DepositBase + DepositFactor * threshold
```
NOT: imzalayin sayisi degil, `threshold` sayisi kullanilir!

**Calisma akisi:**
1. Ilk imzaci: `as_multi` -> MultisigCreated event + deposit
2. Ara imzacilar: `approve_as_multi` veya `as_multi`
3. Son imzaci (threshold tamamlaninca): cagri dispatch edilir

### 7.4 Utility [agent-19]

- `batch`: non-atomic, bir hata durumunda kalan islemler yine calisir
- `batch_all`: atomic, bir hata tum batch'i geri alir
- `force_batch`: hatalari ignore eder, `BatchCompletedWithErrors` event
- **YASAK:** `batch_all` icinde nested `batch_all` (filter lib.rs:334'te engellenir)

---

## 8. Nomination Pools [agent-15]

### 8.1 Mimari

```
Pool State: Open -> Blocked -> Destroying
```

- `Depositor`: pool kurucusu, en son ayrilan
- `Nominator`: staking hedeflerini secer
- `Bouncer`: uye kabul/ret
- `Root`: tam kontrol

### 8.2 Kritik Degerler

- `POINTS_TO_BALANCE_INIT_RATIO = 1` (baslangiçta 1 puan = 1 token)
- `RewardCounter = FixedU128` (reward per point)
- Aritmetik: `U256` zorunlu (overflow riski)

### 8.3 Uyarilar

- `update_records()` puanlar DEGISMEDEN ONCE cagirilmali (lib.rs:2142-2146)
- `TransferStake` DEPRECATED -> yeni chain'ler `DelegateStake` kullanmali
- Genesis defaults: `max_pools=16`, `max_members_per_pool=32`, `max_members=512`
- Pool reward hesabi: `pallet_id + pool_id` (deterministik turetme)

---

## 9. Network Katmani [agent-20, agent-21, agent-22]

### 9.1 Transaction Pool [agent-21]

**Iki Uygulama:**
- `BasicPool`: tek fork, basit, legacy
- `ForkAwareTxPool`: cok-fork destekli, yeni standart

**Tag-based Dependency Graph:**
```
tx.provides: [tag1, tag2]  // bu tx'in urettigi kaynaklar
tx.requires: [tag3]        // bu tx'in bagimli oldugu kaynaklar
```

**Ready Queue Siralaması (oncelik):**
1. Priority yuksek -> dusuk (DESC)
2. Valid_till kucuk -> buyuk (ASC)
3. Insertion_id buyuk -> kucuk (DESC — yeni gelen once)

**Future Queue Eviction:** en eski timestamp once cikar

**Limit Asimi Eviction:**
1. En dusuk priority
2. Sonra en yeni insertion_id (son gelen ilk cikar)

**Varsayilan Limitler:**
- Ready: 8192 tx / 20 MB
- Future: 512 tx / 1 MB
- Ban suresi: 30 dakika

**Kritik Kurallar:**
- `provides` listesi BOS olamaz -> `NoTagsProvided` hatasi
- `LimitsEnforced` ban turu ASLA otomatik kaldirilmaz
- `FINALITY_TIMEOUT = 512` blok (finality gelmezse revalidate)
- Revalidation: her 200ms, minimum 20 tx/batch

### 9.2 Block Sync [agent-20]

- Warp sync: once justification'lar, sonra state
- Full sync: tum bloklari tek tek
- Fast sync: blok headers + state snapshot
- Block request batch: MAX **128**
- Warp sync minimum peer: **3**

### 9.3 RPC API [agent-22]

**V1 (sc-rpc, Legacy):**
- `system_*`, `chain_*`, `state_*`, `author_*`
- `DenyUnsafe`: unsafe metodlar `MethodNotFound (-32601)` doner
- `import_notification_stream`: initial sync sirasinda bazi bloklari atlar
- `every_import_notification_stream`: her blok, sync dahil (guvenli alternatif)

**V2 (rpc-spec-v2, Yeni Standart):**
- `chainHead_unstable_follow`: subscription, block pinning
- `chainHead_unstable_body/header/storage/call`
- `transactionWatch_unstable_submitAndWatch`

**V2 Limitler:**
- `MAX_PINNED_BLOCKS = 512`
- `MAX_PINNED_DURATION = 60 saniye`
- `MAX_ONGOING_OPERATIONS = 16` per subscription
- `MAX_LAGGING_DISTANCE = 128` blok
- `MAX_FOLLOW_SUBSCRIPTIONS_PER_CONNECTION = 4`

**BUG [agent-22]:** `storage_keys` / `storage_pairs` tum sonuclari bellege toplar -> OOM riski (TODO yorumu mevcut: "horribly broken")

**Block Pinning:** RAII pattern, `UnpinHandleInner::drop()` otomatik unpin

---

## 10. Smart Contracts [agent-23]

### 10.1 pallet-contracts (Wasm / Ink!)

**Host Functions:** 111 stable `seal_*` API

**Limitler:**
- Memory: **1 MB** (16 pages)
- Payload: **16 KB**
- Event topics: **4**
- Call stack depth: runtime konfigurasyonu ile ayarlanabilir

**Storage Deposit:**
```
deposit = DepositPerByte * bytes_added + DepositPerItem * items_added
```

**Lifecycle:**
- `instantiate` -> code hash + salt -> deterministik adres
- `call` -> gas metering + host function
- `terminate` -> hesap silme + deposit iade

**Ink! makrolari:** `#[ink::contract]`, `#[ink(storage)]`, `#[ink(message)]`, `#[ink(event)]`

### 10.2 pallet-revive (PolkaVM / EVM Uyumlu)

**CALL_STACK_DEPTH = 25** (sabit, runtime config yok)

**Limitler:**
- EVM Memory: **1 MB**
- Calldata: **128 KB**
- Event bytes: **64 KB**
- Transient storage: **4 KB**

**Gas Olcegi:** `GasScale = 10` (EVM gas -> PolkaVM weight donusumu)

**Adres Formati:** H160 (Ethereum native)

**Ratio:** `NativeToEthRatio = 1_000_000` (native token -> wei donusumu)

**Onemli:** Solidity derleyici `resolc` ile PolkaVM bytecode uretilir; EVM bytecode DEGIL

---

## 11. XCM [agent-25]

### 11.1 Versiyon Gecmisi

- **v3**: desteklenen minimum
- **v4**: LTS (Long-Term Support), onerilen
- **v5**: en yeni, `InitiateTransfer` tek instruction ile multi-hop
- `MIN_XCM_VERSION = 3`

### 11.2 Limitler

- `MAX_XCM_DECODE_DEPTH = 8`
- `MAX_INSTRUCTIONS_TO_DECODE = 100`
- `MAX_ITEMS_IN_ASSETS = 20`
- `RECURSION_LIMIT = 10`
- v5'te 50 XCM instruction

### 11.3 Guvenlik Sistemi

**Barrier (filtre zinciri):**
- `AllowTopLevelPaidExecutionFrom`: `BuyExecution` veya `PayFees` arar
- `DenyReserveTransferToRelayChain`: relay chain'e reserve transfer yasak
- `WithComputedOrigin`: computed origin icin izin kontrolu

**BuyExecution:** `weight_limit` IN-PLACE MUTATE edilir (orijinal deger kaybolur)

**Asset Trap:** post-processing sirasinda kullanilmayan asset'ler tuzaga duser, sonra `ClaimAsset` ile kurtarilir

**v5 Yenilikleri:**
- `SetHints { AssetClaimer }`: asset trap alicisini onceden belirle
- `InitiateTransfer`: tek instruction ile multi-hop transfer

### 11.4 Reserve vs Teleport

| Ozellik | Reserve Transfer | Teleport |
|---------|-----------------|----------|
| Guven   | Orta (sovereign account) | Yuksek (tam guven) |
| Surec   | Lock -> Mint / Burn -> Unlock | Burn -> Mint |
| Risk    | Sovereign account kontrolu | Cift harcama riski minimal |

---

## 12. Primitives [agent-26]

### 12.1 sp-arithmetic

- `FixedU128`: 128-bit sabit noktali, 18 decimal
- `Perbill`: milyarda bir (1_000_000_000 = %100)
- `Permill`: binde bir (1_000_000 = %100)
- `Perquintill`: 10^18'de bir (en hassas)
- `BigUint`: coklu-hassasiyet tam sayi
- `helpers_128bit`: 256-bit carpim, bolme, sqrt
- Overflow korumasi: `saturating_*`, `checked_*`, `try_*` metodlari

### 12.2 sp-weights

Iki bilesenli agirlik sistemi:

```rust
Weight {
    ref_time: u64,    // hesaplama suresi (picosaniye)
    proof_size: u64,  // storage proof boyutu (byte)
}
```

- Her dispatch en az iki boyutu da belirtmeli
- Block weight limiti her iki boyut icin ayri kontrol edilir
- `WeightToFee`: `ref_time` baz alir genellikle

### 12.3 sp-trie

- **Patricia Merkle Trie** (Ethereum benzeri ama farkli encoding)
- `StorageProof`: trie node'larinin seti, hafif istemci dogrulamasi icin
- `CompactProof`: daha kucuk, sadece gerekli node'lar
- `ProofSizeExtension`: offchain proof boyutu olcumu
- Root hash: tum state'in kriptografik ozeti

---

## 13. Diger Palletler

### 13.1 NFTs + Broker [agent-27]

**pallet-nfts:**
- **Ayar Sistemi Kritik:** bitflag BIT SET = OZELLIK DEVRE DISI (inverted!)
- Roller: `Issuer`, `Freezer`, `Admin` (CollectionRole)
- Mint tipleri: `Issuer`, `Public`, `HolderOf`
- Pre-signed mint: off-chain imza ile mint
- Atomic swap: iki NFT'yi ayni anda degis tokus
- Attribute namespace: `Pallet`, `CollectionOwner`, `ItemOwner`, `Account`
- **Royalty sistemi YOK** (tasarim karari)

**pallet-uniques:** DEPRECATED, pallet-nfts'e gecin

**pallet-broker (Coretime):**
- `CORE_MASK_BITS = 80` (80-bit CoreMask)
- Dutch auction: `CenterTargetPrice`, `100x -> 10x -> 1x` fiyat araligi
- Yenileme fiyat ust siniri: `min(current_price, old_price * (1 + renewal_bump))`
- **`leadin_length` SIFIR OLAMAZ** (panic)
- Otomatik yenileme: `SovereignAccountOf`
- Coretime satisi: `sale_start`, `ideal_cores_sold`, `leadin_length`

### 13.2 Misc (sudo, recovery, society) [agent-24]

**pallet-sudo:**
- `dispatch_bypass_filter(Root)`: tum filtreleri bypass eder
- `Pays::No`: sudo cagirilari ucretsiz
- `remove_key()`: sudo yetkisini kalici kaldirır (GERI ALIS YOK)
- `CheckOnlySudoAccount`: bootstrap icin signed extension

**pallet-recovery:**
- `MAX_GROUPS_PER_ACCOUNT = 10`
- `cancel_delay >= 1` ZORUNLU (sifir olursa recovery aninda basarili olur!)
- `SecurityDeposit`: spam onlemi
- Bitfield voting sistemi (her arkadas 1 bit)
- Priority-based inheritor (en uzun suredur kilitli hesap)

**pallet-society:**
- Oy agirligi: `(rank + 1)^2`
- Onay: `ayes >= max(2 * rejections, 1)`
- Pot delta: `min(PeriodSpend, unaccounted / 2)`
- Voting sureci: mevcut uyeler yeni adaylari oylar

**pallet-lottery:**
- Ticket = call dispatch (belirli bir cagri turu)
- `AlreadyParticipating`: her call type icin tek bilet
- `LotteryIndex` overlay: eski katilimcilar/biletler TEMIZLENMEZ (residual storage BUG)
- Kazanan secimi: VRF randomness

**pallet-root-offences:**
- SADECE TEST icin
- `validator_set_count` -> `unreachable!()` runtime'da patlar

### 13.3 Guvenlik Palletleri (safe-mode, tx-pause) [agent-29]

**pallet-safe-mode:**
- Tum extrinsic'leri engeller (whitelist haric)
- `BaseCallFilter` integration: runtime cagri filtresi
- Safe-mode'un kendi cagirilari HICBIR ZAMAN engellenmez
- Etkinlestirme: governance veya configurable origin
- Sure siniri: `SafeModeMaxDuration`

**pallet-tx-pause:**
- Granular per-call pause (safe-mode'un aksine tek tek cagri)
- `PausedTransactionFilter`: `(pallet_name, call_name)` cifti
- Governance origin gerekli
- Whitelist: bazi cagilar duraklatılamaz

**pallet-fast-unstake:**
- Staking cozme icin: staker aktif era exposure'inda DEGILSE hizli cozer
- `ErasStakers` kontrolu: son N era icin exposure kontrolu
- Batch islem: her blokta MaxErasToCheck erasi kontrol eder
- Slash riski yoksa: aninda unstake, slash riski varsa: ret

**pallet-whitelist:**
- Cagrıyı onceden yetkilendir (hash sakla)
- Sonra `dispatch_whitelisted_call` ile ayricaliksiz calistir
- `WhitelistedCall` storage: `call_hash -> ()`

**pallet-parameters:**
- On-chain runtime parametre guncellemesi
- Runtime upgrade gerektirmez
- `Parameter` trait: `Key`, `Value`, `ParameterStore`

**pallet-state-trie-migration:**
- V1 -> V2 trie gecisi
- Blok basina limit: `MigrationLimits { size, item }`

**pallet-migrations:**
- Multi-block migration framework
- `MigrationList` trait: sirali migration calistirma
- Her blokta `step()` cagrilir, kalan work raporlanir

---

## 14. MASTER Sayisal Degerler Tablosu

| Deger | Sayi | Kaynak |
|-------|------|--------|
| EXTRINSIC_FORMAT_VERSION | 5 | agent-01 |
| MILLISECONDS_PER_YEAR | 31_557_600_000 | agent-03 |
| REWARD_F1 (validators payi) | 50% | agent-03 |
| Staking slash formulu | min((3k/n)^2, 1) | agent-03 |
| im-online slash formulu | min((3*(k-n/10-1))/n,1)*0.07 | agent-09 |
| Equivocation slash | min((3k/n)^2, 1) | agent-05/06 |
| GRANDPA CATCH_UP_THRESHOLD | 2 round | agent-06/20 |
| GRANDPA notification max | 1 MB | agent-20 |
| GRANDPA LUCKY_PEERS | 4 | agent-20 |
| MAX_SLOT_CAPACITY (BABE window) | 1000 | agent-20 |
| BABE secondary idx formulu | blake2_256(rand||slot)%n | agent-20 |
| AMM scaling A | 1_000_000 | agent-11 |
| AMM LPFee | 3 (= 0.3%) | agent-11 |
| MintMinLiquidity | 100 | agent-11 |
| MaxSwapPathLength | 4 | agent-11 |
| HigherPrecisionBalance | U256 | agent-11 |
| MAX_RESERVES per asset | 5 | agent-13 |
| POINTS_TO_BALANCE_INIT_RATIO | 1 | agent-15 |
| Nomination pools: max_pools (genesis) | 16 | agent-15 |
| max_members_per_pool (genesis) | 32 | agent-15 |
| max_members (genesis) | 512 | agent-15 |
| MaxVestingSchedules | ~28 (config) | agent-16 |
| Preimage MAX_SIZE | 4 MB | agent-16 |
| Tips median indeksi | tips[len/2] | agent-18 |
| Tips threshold | ceil(N/2) | agent-18 |
| Fee dagilim: treasury | 80% | agent-25 |
| Fee dagilim: author | 20% | agent-25 |
| Fee dagilim: tip | 100% author | agent-25 |
| Polkadot ideal_stake | 75% | agent-25 |
| Polkadot min_inflation | 2.5% | agent-25 |
| Polkadot max_inflation | 10% | agent-25 |
| Polkadot falloff | 5% | agent-25 |
| TX pool Ready limiti | 8192 tx / 20 MB | agent-21 |
| TX pool Future limiti | 512 tx / 1 MB | agent-21 |
| TX pool ban suresi | 30 dakika | agent-21 |
| TX pool FINALITY_TIMEOUT | 512 blok | agent-21 |
| TX pool revalidation interval | 200 ms | agent-21 |
| TX pool revalidation batch | min 20 tx | agent-21 |
| RPC MAX_PINNED_BLOCKS | 512 | agent-22 |
| RPC MAX_PINNED_DURATION | 60 saniye | agent-22 |
| RPC MAX_ONGOING_OPERATIONS | 16 | agent-22 |
| RPC MAX_LAGGING_DISTANCE | 128 blok | agent-22 |
| RPC MAX_FOLLOW_SUBSCRIPTIONS | 4 per connection | agent-22 |
| pallet-contracts memory | 1 MB (16 pages) | agent-23 |
| pallet-contracts payload | 16 KB | agent-23 |
| pallet-contracts event topics | 4 | agent-23 |
| pallet-revive CALL_STACK_DEPTH | 25 (sabit) | agent-23 |
| pallet-revive EVM_MEMORY_BYTES | 1 MB | agent-23 |
| pallet-revive CALLDATA_BYTES | 128 KB | agent-23 |
| pallet-revive EVENT_BYTES | 64 KB | agent-23 |
| pallet-revive TRANSIENT_STORAGE | 4 KB | agent-23 |
| pallet-revive GasScale | 10 | agent-23 |
| pallet-revive NativeToEthRatio | 1_000_000 | agent-23 |
| pallet-recovery MAX_GROUPS | 10 | agent-24 |
| Society oy agirligi | (rank+1)^2 | agent-24 |
| XCM MIN_XCM_VERSION | 3 | agent-25 |
| XCM MAX_XCM_DECODE_DEPTH | 8 | agent-25 |
| XCM MAX_INSTRUCTIONS_TO_DECODE | 100 | agent-25 |
| XCM MAX_ITEMS_IN_ASSETS | 20 | agent-25 |
| XCM RECURSION_LIMIT | 10 | agent-25 |
| XCM v5 instruction sayisi | 50 | agent-25 |
| Coretime CORE_MASK_BITS | 80 | agent-27 |
| Coretime Dutch auction araligi | 100x->10x->1x | agent-27 |
| Conviction None carpani | 0.1x / 0 donem | agent-28 |
| Conviction Locked6x carpani | 6x / 32 donem | agent-28 |
| CONVICTION_VOTING_ID | b"pyconvot" | agent-28 |
| Warp sync min peer | 3 | agent-20 |
| Block request MAX | 128 | agent-20 |
| Multisig deposit | DepositBase+DepositFactor*threshold | agent-19 |

---

## 15. Bilinen Buglar ve Sorunlar

### BUG-01: TransferredApproved Event Hic Emit Edilmiyor [agent-14]
- **Dosya:** `pallet-assets/src/lib.rs:662`
- **Etki:** Off-chain indexer'lar onaylanmis transferi izleyemez
- **Durum:** Upstream acik sorun

### BUG-02: AMM TOCTOU Saldirisi (SDK Issue #1698) [agent-12]
- **Dosya:** pallet-asset-conversion
- **Etki:** Ayni blokta rezerv manipulasyonu ile price manipulation
- **Workaround:** `with_transaction` + oncesi/sonrasi reserve kontrolu

### BUG-03: RPC storage_keys/storage_pairs OOM Riski [agent-22]
- **Dosya:** sc-rpc storage metodlari
- **Etki:** Buyuk state ile tum sonuclar bellege toplanir -> OOM crash
- **Durum:** TODO yorumu mevcut ("horribly broken")

### BUG-04: pallet-lottery Residual Storage [agent-24]
- **Dosya:** pallet-lottery
- **Etki:** `Participants` ve `Tickets` storage'i `LotteryIndex` degisince temizlenmez
- **Sonuc:** Storage bloat, yanlis state okuma riski

### BUG-05: pallet-root-offences Uretim Hatasi [agent-24]
- **Dosya:** pallet-root-offences
- **Etki:** `validator_set_count` -> `unreachable!()` — uretimde kullanilirsa panic
- **Durum:** SADECE TEST icin, uretim runtime'a dahil edilmemeli

### BUG-06: Scheduler Periodic Task Kaybi [agent-16]
- **Dosya:** `pallet-scheduler/src/lib.rs:1418` (TODO)
- **Etki:** Agenda dolu ise periyodik gorev sessizce kaybolur
- **Sinyal:** `PeriodicFailed` event firlatilir ama gorev silinir

### BUG-07: pallet-assets Approval Additive Davranisi [agent-14]
- **Dosya:** pallet-assets approval sistemi
- **Etki:** `approve_transfer` cagrisi mevcut approval'a EKLER (ERC-20'den farkli)
- **Risk:** Front-running ile cift harcama vektoru

### BUG-08: pallet-recovery cancel_delay=0 [agent-24]
- **Dosya:** pallet-recovery
- **Etki:** `cancel_delay = 0` ise recovery aninda basarili; kurban tepki suresiz
- **Cozum:** `cancel_delay >= 1` zorunlu tutulmali (pallet kontrol eder ama runtime config'e dikkat)

### BUG-09: import_notification_stream Blok Kacirma [agent-22]
- **Dosya:** sc-rpc
- **Etki:** Initial sync sirasinda bazi blok bildirimlerini atlar
- **Cozum:** `every_import_notification_stream` kullanin

---

## 16. Deprecated API Listesi

| Pallet / Modul | Deprecated Ozellik | Yeni Alternatif | Kaynak |
|----------------|-------------------|-----------------|--------|
| pallet-staking | pallet-staking (tamamiyla) | pallet-staking-async | agent-03 |
| pallet-nomination-pools | `TransferStake` stratejisi | `DelegateStake` | agent-15 |
| pallet-treasury | `spend_local` (Mayis 2025) | `spend` | agent-18 |
| frame-executive | `OnRuntimeUpgrade` (Eylul 2026) | pallet-migrations | agent-08 |
| pallet-uniques | tum pallet | pallet-nfts | agent-27 |
| pallet-democracy | tum pallet (aktif gelistirme yok) | pallet-referenda (OpenGov) | agent-10 |
| pallet-assets | `Locks` API | `Freezes` ve `Holds` API | agent-02/13 |
| Balances | `Currency` trait | `fungible::*` trait'leri | agent-02 |
| RPC V1 | `state_getStorage` bazi metodlar | rpc-spec-v2 (V2) | agent-22 |
| XCM v1/v2 | XCM v1, v2 | XCM v4 (LTS) / v5 | agent-25 |

---

## 17. Nova Shield Chain Icin En Degerli Icgorular

### 17.1 Balance ve Token Sistemi

**Uygula:**
- `frozen = max(locks, freezes)` prensibini benimse, additive sistem YANLISLIKLARA YOL ACAR
- Hold sistemi (SUM) ile freeze sistemi (MAX) arasindaki farki anlayarak kullan
- ExistentialDeposit: cok dusuk ayarlama -> hesap spam'i; cok yuksek -> kullanici deneyimi kotu

**Nova Shield icin oneri:** Lock API'yi kullanma, sadece Freeze ve Hold kullan. Eski paletlerle uyumluluk gerekirse zaten Lock'a fallback var.

### 17.2 DEX Tasarimi

**Kritik:**
- AMM formullerini OLDUGU GIBI al: A=1_000_000, LPFee=3, `get_amount_in` +1 ceiling
- Approval additive bug'ini COZMEK icin: her approval oncesi mevcut approval'i sifirla veya `approve(0)` -> `approve(amount)` seklinde iki adim zorla
- TOCTOU icin: swap'ta mutlaka `with_transaction` kullan
- Deadline parametresi ekle (Substrate'te YOK, eklemen gerekli)
- Slippage kontrolunu `min_amount_out` olarak zorunlu kil

### 17.3 Staking

**Kritik:**
- pallet-staking DEPRECATED, `pallet-staking-async` ile basla
- `update_records()` her puan degisikliginden ONCE cagrilmali (nomination pools icin)
- `DelegateStake` stratejisi kullan, `TransferStake` degil
- im-online slash formulunu hassasiyetle uygula: `(3*(k - n/10 - 1)) / n`
- Slash'in kumulatif oldugunu unutma (ayni era'daki birden fazla suc)

### 17.4 Governance

**Oneri:**
- pallet-referenda (OpenGov) kullan, pallet-democracy degil
- Conviction voting: lock surelerinin katlanarak buyudugunu UI'da ac goster
- Tips icin lower-median (`tips[len/2]`) kullan — ust median degil
- Treasury burn formulu: birikimli hesapla (`1-(1-Burn)^N`)

### 17.5 Consensus

**BABE'yi sectiysen:**
- Threshold formulunu dogru uygula: `p = 1-(1-c)^theta`; sonra `threshold = 2^128 * p`
- Secondary slot: `blake2_256(randomness || slot) % n` — pseudo-random ama herkes hesaplayabilir
- GRANDPA ile birlestir: BABE producer, GRANDPA finalizer

**AURA sectiysen:**
- Basit: `author = authorities[slot % n]`
- Guvenlik: sirali, tahmin edilebilir — PoA veya kucuk aglar icin uygun

### 17.6 Smart Contract

**Nova Shield kendi VM'ini tasarliyorsa:**
- pallet-revive yaklasimini izle: EVM uyumluluk katmani + native VM (PolkaVM)
- `CALL_STACK_DEPTH = 25` sabit tutmak guvenli
- GasScale=10: native gas -> EVM gas cevirimi icin makul
- `NativeToEthRatio = 1_000_000`: token precision farki icin

**Storage deposit sistemi:**
```
deposit = DepositPerByte * bytes + DepositPerItem * items
```
Bu model spam'i onlemek icin mukemmel; NovaShield'e adapt et.

### 17.7 XCM ve Cross-Chain

- XCM v4 (LTS) ile basla, v5 uzerine gelistir
- `MAX_INSTRUCTIONS_TO_DECODE = 100` sinirini asan mesajlar reddedilir — dizaynda goz onunde bulundur
- `DenyReserveTransferToRelayChain` benzeri kurallari her bridge icin tanimla
- Asset Trap mekanizmasi: kayip asset'leri kurtarma icin kritik
- v5 `SetHints { AssetClaimer }`: asset trap alicisini onceden belirle — guvenlik icin sart

### 17.8 Transaction Pool

- `ForkAwareTxPool` kullan, `BasicPool` degil (fork durumlarini yonetir)
- `provides` listesi bos birakilmamali
- Ready=8192/20MB, Future=512/1MB limitleri baslangic icin uygun
- Revalidation 200ms'de bir otomatik calisir — bunu goz onunde bulundur

### 17.9 Guvenlik Palletleri (Emergency)

**Nova Shield icin kritik mimari:**
```
pallet-safe-mode  -> tum islemleri durdur (acil)
pallet-tx-pause   -> belirli cagilari durdur (granular)
pallet-whitelist  -> izin verilen cagilari onceden yetkilendir
```
Bu uclu: acil durum, granular kontrol ve ayricaliksiz erisim piramidi olusturur.

### 17.10 Numerik Hassasiyet

- `FixedU128`: 18 decimal, yuksek hassasiyet, overflow dikkat
- `Perbill` vs `Permill` vs `Perquintill`: kullanim yerine gore sec
- AMM gibi yuksek hassasiyet gerektiren yerlerde `U256` kullan
- `saturating_*` metodlari uretimde tercih et, `unwrap()` degil
- Weight: `ref_time` + `proof_size` her ikisini de set et, biri sifir birakma

### 17.11 Multi-block Migration Pattern

`pallet-migrations` framework'unu benimse:
- Runtime upgrade sirasinda tek blokta tum migration'i bitirmeye calisma
- Her blokta `step()` cagrisi, weight budget icinde kalan isi yap
- `StorageVersion` ile migration idempotency sagla

### 17.12 Kritik Mimari Kararlar Ozeti

| Karar | Onerilen Secim | Alternatif | Neden |
|-------|---------------|------------|-------|
| Consensus | BABE + GRANDPA | AURA (kucuk ag) | VRF randomness guvenli |
| Finality | GRANDPA | - | Battle-tested |
| Staking | pallet-staking-async | - | pallet-staking deprecated |
| Balance API | fungible::* trait | Currency trait | Currency deprecated |
| Token locking | Freeze + Hold | Lock API | Lock deprecated |
| TX Pool | ForkAwareTxPool | BasicPool | Fork handling |
| RPC | V2 (rpc-spec-v2) | V1 (legacy) | V1 legacy, V2 yeni standart |
| Smart contract | pallet-revive | pallet-contracts | EVM uyumlu |
| XCM | v4/v5 | v3 | v3 minimum, v4 LTS |
| Migration | pallet-migrations | OnRuntimeUpgrade | Multi-block, guvenli |
| NFT | pallet-nfts | pallet-uniques | pallet-uniques deprecated |
| Governance | pallet-referenda | pallet-democracy | OpenGov, aktif gelistirme |
| DEX approval | Sifirla + set et | Direkt artir | Front-running korumasi |

---

*Agent-30 MASTER SENTEZ — Tum 29 agent kapsanmistir. Batch 1 (agent-01..10), Batch 2 (agent-11..20), Batch 3 (agent-21..29).*
