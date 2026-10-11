# Repo 8: cosmos-sdk — Kapsamlı Analiz

**Kaynak:** `research/chains/cosmos-sdk/`
**Dil:** Go 1.26.5
**Yazar:** Cosmos Network / Interchain Foundation
**Consensus:** CometBFT (Tendermint fork) v0.39.3
**Lisans:** Apache 2.0
**Analiz Tarihi:** 2026-07-17

---

## İçindekiler

1. [Genel Mimari — Module Sistemi ve ABCI](#1-genel-mimari)
2. [go.mod — Bağımlılıklar](#2-gomod)
3. [BaseApp — ABCI Uygulama Çatısı](#3-baseapp)
4. [x/staking — Validator Yaşam Döngüsü](#4-staking-modülü)
5. [Validator Tipi ve Durum Makinesi](#5-validator-tipi)
6. [EndBlocker — ValidatorSet Güncellemesi](#6-endblocker)
7. [x/staking Slash — Ceza Mekanizması](#7-staking-slash)
8. [Delegation — Delegasyon Sistemi](#8-delegation)
9. [Unbonding — Ayrılma Süreci](#9-unbonding)
10. [ConsensusKey Rotasyonu](#10-consensus-key-rotasyonu)
11. [x/slashing — İmza Takibi ve Downtime](#11-slashing-modülü)
12. [x/gov — Yönetişim ve Oylama](#12-gov-modülü)
13. [Governance Tally — Oy Sayımı](#13-governance-tally)
14. [x/mint — Token Basımı ve Enflasyon](#14-mint-modülü)
15. [x/distribution — Ödül Dağıtımı](#15-distribution-modülü)
16. [CometBFT Entegrasyonu — ABCI Akışı](#16-cometbft-entegrasyonu)
17. [Store Katmanı — IAVL ve Collections](#17-store-katmanı)
18. [Parametre Yönetimi](#18-parametre-yönetimi)
19. [Hook Sistemi — Modüller Arası Olay](#19-hook-sistemi)
20. [Nova Shield için Çıkarımlar](#20-nova-shield-cikarimlar)
21. [Özet Karşılaştırma Tablosu](#21-ozet)

---

## 1. Genel Mimari

Cosmos SDK, ABCI (Application BlockChain Interface) üzerine kurulu **modüler blockchain çatısı**dır. CometBFT (Tendermint fork) consensus katmanını soyutlar; uygulama geliştiricisi sadece iş mantığını yazar.

```
CometBFT (Consensus + P2P + Networking)
    ↕ ABCI (gRPC / in-process)
BaseApp (SDK çatısı)
    ↓
x/ Modules:
├── x/staking    — Validator stake, delegasyon, unbonding
├── x/slashing   — Downtime/equivocation cezası
├── x/gov        — On-chain yönetişim
├── x/mint       — Token enflasyonu ve basımı
├── x/distribution — Ödül dağıtımı
├── x/bank       — Token transferleri
├── x/auth       — Hesap yönetimi, AnteHandler
├── x/authz      — Yetki devri
├── x/upgrade    — Zincir yükseltmeleri
└── x/epochs     — Periyodik görevler
```

### Modül Anatomisi

Her modül standart bir yapı izler:

```
x/<module>/
├── keeper/         — İş mantığı (KV store erişimi)
├── types/          — Protobuf yapıları, parametreler, hatalar
├── module.go       — AppModule interface (BeginBlock/EndBlock register)
├── abci.go         — BeginBlocker/EndBlocker implementasyonu
├── genesis.go      — Genesis durumu
└── simulation/     — Property-based testler
```

---

## 2. go.mod

**Go versiyonu:** 1.26.5

### Kritik Bağımlılıklar

```go
github.com/cometbft/cometbft v0.39.3        // BFT consensus (Tendermint fork)
cosmossdk.io/collections v1.4.0              // Type-safe KV store koleksiyonları
cosmossdk.io/math v1.5.3                     // Sabit nokta matematik (LegacyDec, Int)
cosmossdk.io/depinject v1.2.1               // Dependency injection (DI konteyneri)
cosmossdk.io/core v1.1.0                    // Temel interface'ler
github.com/cosmos/cosmos-sdk/store/v2 v2.0.0 // IAVL / Firewood store
github.com/RoaringBitmap/roaring/v2 v2.19.0  // Slashing bitmap (MissedBlocksCounter)
go.opentelemetry.io/otel v1.44.0            // Distributed tracing
google.golang.org/grpc v1.81.1              // gRPC (ABCI, sorgu)
```

**Önemli:** `roaring.Bitmap` kullanımı — Slashing modülü validator imzalarını roaring bitmap'te tutar. 10.000 blok penceresi için bellek etkin.

---

## 3. BaseApp — ABCI Uygulama Çatısı

**Dosya:** `baseapp/baseapp.go`

### BaseApp Struct

```go
type BaseApp struct {
    logger   log.Logger
    name     string
    db       dbm.DB                          // Disk DB (pebble/goleveldb)
    cms      storetypes.CommitMultiStore      // Ana state (IAVL tree)
    qms      storetypes.MultiStore            // Sadece sorgu için opsiyonel store

    // Routing
    grpcQueryRouter  *GRPCQueryRouter         // Query mesajları
    msgServiceRouter *MsgServiceRouter        // Tx mesajları

    // Tx işleme
    txDecoder   sdk.TxDecoder
    txEncoder   sdk.TxEncoder
    mempool     mempool.Mempool
    anteHandler sdk.AnteHandler               // Fee + imza doğrulama
    postHandler sdk.PostHandler               // Post-tx işleme

    // ABCI handler'ları
    abciHandlers sdk.ABCIHandlers

    // Snapshot yönetimi (state sync için)
    snapshotManager *snapshots.Manager
    stateManager    *state.Manager

    // Inter-block cache (bloklar arası cache)
    interBlockCache storetypes.MultiStorePersistentCache

    // Gas limitleri
    gasConfig config.GasConfig

    // Paralel tx çalıştırma (block-stm)
    txRunner     sdk.TxRunner
    optimisticExec *oe.OptimisticExecution

    // Bloğu durdurma koşulları
    haltHeight uint64
    haltTime   uint64

    sealed bool  // Başlatıldıktan sonra değiştirilemeyen parametreler
}
```

### ExecMode'lar

```go
const (
    execModeCheck               // mempool check: imza+fee kontrolü
    execModeReCheck             // blok commit sonrası mempool yeniden kontrol
    execModeSimulate            // Gas tahmini
    execModePrepareProposal     // Blok önerisi hazırlama
    execModeProcessProposal     // Gelen blok önerisini doğrula
    execModeVoteExtension       // Vote extension (ABCI++ özelliği)
    execModeVerifyVoteExtension // Vote extension doğrulama
    execModeFinalize            // Blok kesinleştirme (state write)
)
```

### ABCI Akışı (Her Blok)

```
1. PrepareProposal   — Validator bloğu hazırlar (tx seçimi)
   ↓
2. ProcessProposal   — Diğer validator'lar bloğu kontrol eder
   ↓
3. FinalizeBlock     — Konsensus sağlandı → state'i güncelle
   ├── BeginBlockers()  — Her modülün BeginBlock'u
   ├── DeliverTxs()     — Her tx için AnteHandler + MsgServer + PostHandler
   └── EndBlockers()    — Her modülün EndBlock'u (ValidatorSet güncel)
   ↓
4. Commit            — State DB'ye yazılır, IAVL commit
```

### AnteHandler Zinciri

```go
// auth modülü varsayılan ante handler:
// 1. MempoolFeeDecorator       — minimum fee kontrolü
// 2. ValidateBasicDecorator    — tx temel kontrol
// 3. TxTimeoutHeightDecorator  — timeout height
// 4. ValidateMemoDecorator     — memo uzunluğu
// 5. ConsumeGasTxSizeDecorator — tx boyutu gas'ı
// 6. DeductFeeDecorator        — ücreti çek
// 7. SetPubKeyDecorator        — public key kaydet
// 8. ValidateSigCountDecorator — imza sayısı
// 9. SigGasConsumeDecorator    — imza gas'ı
// 10. SigVerificationDecorator — imza doğrula
// 11. IncrementSequenceDecorator— nonce artır
```

---

## 4. x/staking — Validator Yaşam Döngüsü

**Dosya:** `x/staking/keeper/keeper.go`

### Keeper Struct

```go
type Keeper struct {
    storeService          storetypes.KVStoreService
    cdc                   codec.BinaryCodec
    authKeeper            types.AccountKeeper    // Hesap yönetimi
    bankKeeper            types.BankKeeper       // Token transferleri
    hooks                 types.StakingHooks     // Olay bildirimleri
    authority             string                 // Gov module adresi (param güncelleme için)
    validatorAddressCodec addresscodec.Codec
    consensusAddressCodec addresscodec.Codec
}

// Implement eder:
var _ types.ValidatorSet = Keeper{}
var _ types.DelegationSet = Keeper{}
```

### Staking Parametreleri

```go
const (
    DefaultUnbondingTime      = 21 * 24 * time.Hour   // 3 hafta (21 gün)
    DefaultMaxValidators uint32 = 100                  // Aktif validator sayısı
    DefaultMaxEntries    uint32 = 7                    // UBD/Redelegasyon per pair
    DefaultHistoricalEntries uint32 = 10000            // IBC için gerekli
)

var DefaultMinCommissionRate = math.LegacyZeroDec()   // %0 (minimum komisyon)
var DefaultKeyRotationFee = sdk.NewInt64Coin("uatom", 1_000_000)  // 1 ATOM
```

### Validator Pool Hesapları

```
BondedPool       — Stake edilmiş tokenlar (bonded validator'lara ait)
NotBondedPool    — Unbonding tokenlar (unbonding/unbonded validator'lara ait)
KeyRotationFeePool — Konsensus key rotasyon ücretleri
```

---

## 5. Validator Tipi ve Durum Makinesi

**Dosya:** `x/staking/types/validator.go`

### Validator Struct (Protobuf'tan üretilmiş)

```go
type Validator struct {
    OperatorAddress         string    // bech32 validator address
    ConsensusPubkey         *Any      // Konsensus imza anahtarı (ör. ed25519)
    Jailed                  bool      // Cezalandırılmış mı?
    Status                  BondStatus // Unbonded | Unbonding | Bonded
    Tokens                  math.Int   // Toplam stake (kendi + delege)
    DelegatorShares         math.LegacyDec  // Toplam delegasyon payı
    Description             Description     // Moniker, website, vb.
    UnbondingHeight         int64     // Unbonding başladığı blok
    UnbondingTime           time.Time // Unbonding bitiş zamanı
    Commission              Commission // Rate, MaxRate, MaxChangeRate
    MinSelfDelegation       math.Int   // Minimum öz delegasyon
    UnbondingOnHoldRefCount int64      // IBC için bekletme sayacı
}
```

### Durum Makinesi

```
          Başla
            ↓
         Unbonded  ←──────────────────┐
            ↓ (MsgCreateValidator)     │
         Bonded ←── unbondingToBonded  │
            ↓ (power düşüşü veya jail) │
         Unbonding ──────────────────→ Unbonded (21 gün sonra)
```

**Önemli Kurallar:**
- `Jailed=true` validator EndBlock'ta validator set'ten çıkarılır
- `Jailed` validator slice edilemez (çalışmaz, zaten çıkarıldı)
- `Unbonded` validator slice edilemez
- Power sıralaması: `ValidatorsPowerStoreIterator` en yüksekten en düşüğe iterate eder

### Description Limitleri

```go
MaxMonikerLength         = 70
MaxIdentityLength        = 3000
MaxWebsiteLength         = 140
MaxSecurityContactLength = 140
MaxDetailsLength         = 280
```

### ConsensusPower Hesabı

```go
func (v Validator) ConsensusPower(r math.Int) int64 {
    // Tokens / PowerReduction (varsayılan: 1_000_000 = 1 ATOM = 1 power)
    return v.PotentialConsensusPower(r)
}
// Örnek: 1_000_000 uATOM stake → 1 consensus power
```

### ABCIValidatorUpdate

```go
func (v Validator) ABCIValidatorUpdate(r math.Int) abci.ValidatorUpdate {
    return abci.ValidatorUpdate{
        PubKey: tmProtoPk,
        Power:  v.ConsensusPower(r),
    }
}

func (v Validator) ABCIValidatorUpdateZero() abci.ValidatorUpdate {
    return abci.ValidatorUpdate{
        PubKey: tmProtoPk,
        Power:  0,  // Power=0 → validator'ı setten çıkar
    }
}
```

---

## 6. EndBlocker — ValidatorSet Güncellemesi

**Dosya:** `x/staking/keeper/val_state_change.go`

### BlockValidatorUpdates — Her EndBlock'ta Çalışır

```go
func (k Keeper) BlockValidatorUpdates(ctx context.Context) ([]abci.ValidatorUpdate, error) {
    // 1. ValidatorSet değişikliklerini hesapla ve uygula
    validatorUpdates, err := k.ApplyAndReturnValidatorSetUpdates(ctx)

    // 2. Olgunlaşmış unbonding validator'ları serbest bırak
    k.UnbondAllMatureValidators(ctx)

    // 3. Olgunlaşmış UBD kuyruğundan işle
    matureUnbonds, _ := k.DequeueAllMatureUBDQueue(ctx, blockTime)
    for _, dvPair := range matureUnbonds {
        k.CompleteUnbonding(ctx, delegatorAddress, valAddr)
    }

    // 4. Olgunlaşmış redelegasyon kuyruğundan işle
    matureRedelegations, _ := k.DequeueAllMatureRedelegationQueue(ctx, blockTime)
    for _, dvvTriplet := range matureRedelegations {
        k.CompleteRedelegation(ctx, delegatorAddress, valSrcAddr, valDstAddr)
    }

    // 5. Olgunlaşmış konsensus key rotasyonlarını temizle
    k.PruneMaturedConsKeyRotations(ctx)

    return validatorUpdates, nil
}
```

### ApplyAndReturnValidatorSetUpdates — Validator Set Mantığı

```go
func (k Keeper) ApplyAndReturnValidatorSetUpdates(ctx context.Context) ([]abci.ValidatorUpdate, error) {
    maxValidators := params.MaxValidators       // varsayılan: 100
    powerReduction := k.PowerReduction(ctx)    // varsayılan: 1_000_000

    // Konsensus key rotasyonlarını uygula
    k.ApplyConsKeyRotations(ctx)

    // Önceki validator setini al
    last, _ := k.getLastValidatorsByAddr(ctx)

    // Güce göre sıralı iterator (en yüksekten başla)
    iterator, _ := k.ValidatorsPowerStoreIterator(ctx)

    for count := 0; iterator.Valid() && count < int(maxValidators); iterator.Next() {
        validator := k.GetValidator(ctx, valAddr)

        if validator.Jailed { panic("should never retrieve jailed validator from power store") }
        if validator.PotentialConsensusPower(powerReduction) == 0 { break }

        // Durum geçişleri
        switch {
        case validator.IsUnbonded():
            validator = k.unbondedToBonded(ctx, validator)
            amtFromNotBondedToBonded += validator.Tokens
        case validator.IsUnbonding():
            validator = k.unbondingToBonded(ctx, validator)
            amtFromNotBondedToBonded += validator.Tokens
        case validator.IsBonded():
            // değişiklik yok
        }

        // Power değiştiyse CometBFT'e bildir
        newPower := validator.ConsensusPower(powerReduction)
        if !found || oldPower != newPower {
            updates = append(updates, validator.ABCIValidatorUpdate(powerReduction))
            k.SetLastValidatorPower(ctx, valAddr, newPower)
        }

        totalPower += newPower
        count++
    }

    // Artık MaxValidators'ta olmayan validator'ları çıkar
    for _, valAddr := range noLongerBonded {
        validator = k.bondedToUnbonding(ctx, validator)
        updates = append(updates, validator.ABCIValidatorUpdateZero()) // Power=0
    }

    // Pool transferleri (tek seferde)
    if amtFromNotBondedToBonded > amtFromBondedToNotBonded {
        k.notBondedTokensToBonded(ctx, diff)
    } else {
        k.bondedTokensToNotBonded(ctx, diff)
    }

    k.SetLastTotalPower(ctx, totalPower)
    k.SetValidatorUpdates(ctx, updates)
    return updates, nil
}
```

---

## 7. x/staking Slash — Ceza Mekanizması

**Dosya:** `x/staking/keeper/slash.go`

```go
func (k Keeper) Slash(
    ctx context.Context,
    consAddr sdk.ConsAddress,
    infractionHeight, power int64,
    slashFactor math.LegacyDec,
) (math.Int, error) {
    // 1. Slash miktarını hesapla
    amount := k.TokensFromConsensusPower(ctx, power)  // power * PowerReduction
    slashAmount := LegacyDec(amount) * slashFactor

    // 2. Validator'ı bul
    validator := k.GetValidatorByConsAddr(ctx, consAddr)

    // 3. Unbonded validator slice edilemez
    if validator.IsUnbonded() { return error }

    remainingSlashAmount := slashAmount

    // 4. Geçmiş height'ta infraction → unbonding ve redelegasyonları da slice et
    if infractionHeight < currentHeight {
        // Unbonding delegasyonları slice et
        for _, ubd := range k.GetUnbondingDelegationsFromValidator(...) {
            amountSlashed := k.SlashUnbondingDelegation(ctx, ubd, infractionHeight, slashFactor)
            remainingSlashAmount -= amountSlashed
        }
        // Redelegasyonları slice et
        for _, red := range k.GetRedelegationsFromSrcValidator(...) {
            amountSlashed := k.SlashRedelegation(ctx, validator, red, infractionHeight, slashFactor)
            remainingSlashAmount -= amountSlashed
        }
    }

    // 5. Validator token'larını yak
    tokensToBurn := min(remainingSlashAmount, validator.Tokens)
    k.RemoveValidatorTokens(ctx, validator, tokensToBurn)

    // 6. Bonded/NotBonded pool'dan yak
    switch validator.GetStatus() {
    case Bonded:   k.burnBondedTokens(ctx, tokensToBurn)
    case Unbonding: k.burnNotBondedTokens(ctx, tokensToBurn)
    }

    return tokensToBurn, nil
}

// SlashWithInfractionReason — Interchain Security için
func (k Keeper) SlashWithInfractionReason(
    ctx context.Context, consAddr sdk.ConsAddress,
    infractionHeight, power int64, slashFactor math.LegacyDec,
    _ types.Infraction,  // Infraction tipi şu an kullanılmıyor
) (math.Int, error) {
    return k.Slash(ctx, consAddr, infractionHeight, power, slashFactor)
}

// Jail — Validator'ı devre dışı bırak
func (k Keeper) Jail(ctx context.Context, consAddr sdk.ConsAddress) error {
    validator := k.mustGetValidatorByConsAddr(ctx, consAddr)
    k.jailValidator(ctx, validator)  // validator.Jailed = true
}
```

---

## 8. Delegation — Delegasyon Sistemi

**Dosya:** `x/staking/keeper/delegation.go`

### Veri Yapıları

```go
type Delegation struct {
    DelegatorAddress string
    ValidatorAddress string
    Shares           math.LegacyDec  // Delegatörün validator'daki payı
}

// Share hesabı: Token / TotalTokens * TotalShares
// Slashing sırasında token/share oranı değişir → delegatörler de kaybeder
```

### Temel Operasyonlar

```go
// Delegasyon al
GetDelegation(ctx, delAddr, valAddr) Delegation

// Bir validator'ın tüm delegasyonları
GetValidatorDelegations(ctx, valAddr) []Delegation

// Bir delegatörün tüm delegasyonları
GetDelegatorDelegations(ctx, delAddr, maxRetrieve) []Delegation

// KV Key yapısı:
// DelegationKey = {DelegationKeyPrefix}{delAddr}{valAddr}
// GetDelegationsByValPrefixKey = {DelegationsByValKeyPrefix}{valAddr}
```

### Redelegasyon

Delegatör, bir validator'dan diğerine stake'i taşıyabilir. Ancak:
- **Redelegasyon unbonding süresini atlamaz** — kaynak validator slice edilirse, redelegasyon da kesilir
- Redelegasyondan redelegasyon yapılamaz (MaxEntries=7 limiti)

---

## 9. Unbonding — Ayrılma Süreci

**Dosya:** `x/staking/keeper/unbonding.go`

### UnbondingID Sistemi

Her unbonding operasyonunun (UnbondingDelegation, Redelegation, ValidatorUnbonding) benzersiz bir ID'si vardır:

```go
func (k Keeper) IncrementUnbondingID(ctx context.Context) (unbondingID uint64, err error) {
    // Monoton artan uint64 counter
    bz, _ := store.Get(types.UnbondingIDKey)
    unbondingID = binary.BigEndian.Uint64(bz) + 1
    store.Set(types.UnbondingIDKey, bigEndian(unbondingID))
    return unbondingID, nil
}

type UnbondingType int32
const (
    UnbondingType_UnbondingDelegation   UnbondingType = 0
    UnbondingType_Redelegation          UnbondingType = 1
    UnbondingType_ValidatorUnbonding    UnbondingType = 2
)
```

### Unbonding Kuyruğu

```
MsgUndelegate geldi
    ↓
UnbondingDelegation oluştur (ID atandı)
    ↓
UnbondingQueue'a ekle (CompletionTime = now + UnbondingTime)
    ↓
Her EndBlock: DequeueAllMatureUBDQueue(ctx, blockTime)
    → CompletionTime < blockTime olan UBD'leri tamamla
    → Tokenları delegatöre gönder
```

---

## 10. ConsensusKey Rotasyonu

**Dosya:** `x/staking/keeper/rotation.go`

Nova Shield için önemli: Validator konsensus anahtarını döndürebilir.

```go
// Parametreler
DefaultKeyRotationFee = 1_000_000 uatom  // 1 ATOM ücret

// Rotasyon akışı:
// 1. MsgRotateConsPubKey → validasyon
// 2. PendingConsKeyRotationUpdates'e ekle
// 3. EndBlock'ta ApplyConsKeyRotations çalışır
// 4. Eski key → PruneMaturedConsKeyRotations ile temizle (unbonding period sonra)
```

**Güvenlik:** Eski key unbonding period kadar saklanır. Bu sürede eski key ile yapılan slashable offense'lar hala slice edilebilir.

---

## 11. x/slashing — İmza Takibi ve Downtime

**Dosya:** `x/slashing/keeper/infractions.go`

### HandleValidatorSignature — Her BeginBlock'ta

```go
func (k Keeper) HandleValidatorSignature(
    ctx context.Context,
    addr cryptotypes.Address,
    power int64,
    signed comet.BlockIDFlag,
) error {
    consAddr := sdk.ConsAddress(addr)

    // Jailed validator'ı takip etme
    if k.sk.IsValidatorJailed(ctx, consAddr) { return nil }

    signInfo := k.GetValidatorSigningInfo(ctx, consAddr)
    signedBlocksWindow := k.SignedBlocksWindow(ctx)  // varsayılan: 100 blok

    // Index: sliding window içindeki pozisyon
    index := signInfo.IndexOffset % signedBlocksWindow
    signInfo.IndexOffset++

    // Roaring bitmap: hangi slot'larda imza eksik?
    missed := signed == comet.BlockIDFlagAbsent
    previous := k.GetMissedBlockBitmapValue(ctx, consAddr, index)

    switch {
    case !previous && missed:
        k.SetMissedBlockBitmapValue(ctx, consAddr, index, true)
        signInfo.MissedBlocksCounter++
    case previous && !missed:
        k.SetMissedBlockBitmapValue(ctx, consAddr, index, false)
        signInfo.MissedBlocksCounter--
    }

    minSignedPerWindow := k.MinSignedPerWindow(ctx)  // varsayılan: 0.5 (100 blokta 50)
    minHeight := signInfo.StartHeight + signedBlocksWindow
    maxMissed := signedBlocksWindow - minSignedPerWindow

    // Ceza eşiği aşıldı mı?
    if height > minHeight && signInfo.MissedBlocksCounter > maxMissed {
        validator := k.sk.ValidatorByConsAddr(ctx, consAddr)
        if validator != nil && !validator.IsJailed() {
            // DOWNTIME: Slash + Jail
            distributionHeight := height - sdk.ValidatorUpdateDelay - 1
            slashFractionDowntime := k.SlashFractionDowntime(ctx)  // varsayılan: 0.0001 (binde 1)

            k.sk.SlashWithInfractionReason(ctx, consAddr, distributionHeight, power,
                slashFractionDowntime, Infraction_INFRACTION_DOWNTIME)

            k.sk.Jail(ctx, consAddr)

            downtimeJailDur := k.DowntimeJailDuration(ctx)  // varsayılan: 10 dakika
            signInfo.JailedUntil = blockTime.Add(downtimeJailDur)

            // Sayacı ve bitmap'i sıfırla (yeniden bond edildiğinde temiz başlasın)
            signInfo.MissedBlocksCounter = 0
            signInfo.IndexOffset = 0
            k.DeleteMissedBlockBitmap(ctx, consAddr)
        }
    }

    k.SetValidatorSigningInfo(ctx, consAddr, signInfo)
    return nil
}
```

### Slashing BeginBlocker

```go
func BeginBlocker(ctx context.Context, k keeper.Keeper) error {
    // Her blok başında, o blokta imza atması gereken validator'lar için
    sdkCtx := sdk.UnwrapSDKContext(ctx)
    for _, voteInfo := range sdkCtx.VoteInfos() {
        k.HandleValidatorSignature(ctx,
            voteInfo.Validator.Address,
            voteInfo.Validator.Power,
            comet.BlockIDFlag(voteInfo.BlockIdFlag))
    }
    return nil
}
```

### Slashing Parametreleri (Varsayılan)

| Parametre | Değer | Açıklama |
|-----------|-------|---------|
| SignedBlocksWindow | 100 blok | İmza takip penceresi |
| MinSignedPerWindow | 0.5 (50%) | Minimum imzalanması gereken blok oranı |
| DowntimeJailDuration | 10 dakika | Jail süresi (downtime) |
| SlashFractionDowntime | 0.0001 (0.01%) | Downtime slice oranı |
| SlashFractionDoubleSign | 0.05 (5%) | Çift imza slice oranı |

### Equivocation (Çift İmza) Slashing

CometBFT'den gelen `Evidence.Equivocation` → `k.Slash(consensusAddr, infractionHeight, power, SlashFractionDoubleSign)`

Çift imza sonrası validator **kalıcı olarak jailed** olur (`JailedUntil = MaxTime`). Unjail edilemez.

---

## 12. x/gov — Yönetişim ve Oylama

**Dosya:** `x/gov/keeper/keeper.go`

### Keeper Struct

```go
type Keeper struct {
    authKeeper  types.AccountKeeper
    bankKeeper  types.BankKeeper
    distrKeeper types.DistributionKeeper
    hooks       types.GovHooks
    storeService corestoretypes.KVStoreService
    cdc         codec.Codec
    router      baseapp.MessageRouter          // Kabul edilen proposal'ları çalıştırır

    calculateVoteResultsAndVotingPowerFn CalculateVoteResultsAndVotingPowerFn

    // Collections API (tip-güvenli KV erişimi)
    Schema                 collections.Schema
    Constitution           collections.Item[string]
    Params                 collections.Item[v1.Params]
    Deposits               collections.Map[Pair[uint64, AccAddress], v1.Deposit]
    Votes                  collections.Map[Pair[uint64, AccAddress], v1.Vote]
    ProposalID             collections.Sequence
    Proposals              collections.Map[uint64, v1.Proposal]
    ActiveProposalsQueue   collections.Map[Pair[time.Time, uint64], uint64]
    InactiveProposalsQueue collections.Map[Pair[time.Time, uint64], uint64]
    VotingPeriodProposals  collections.Map[uint64, []byte]
}
```

### Proposal Yaşam Döngüsü

```
MsgSubmitProposal
    ↓
Deposit toplama (MinDeposit dolana kadar Inactive kuyrukta)
    ↓
MinDeposit doldu → ActiveProposalsQueue'a al (VotingPeriod başlar)
    ↓
VotingPeriod bitiş → Tally() çalışır
    ↓
Geçti → execute(proposal)  → MessageRouter ile messages çalışır
Geçmedi → Depozito yakılır veya iade edilir
```

### Oy Tipleri

```go
type VoteOption int32
const (
    OptionUnspecified VoteOption = 0
    OptionYes         VoteOption = 1
    OptionAbstain     VoteOption = 2
    OptionNo          VoteOption = 3
    OptionNoWithVeto  VoteOption = 4  // >1/3 veto → deposit yakılır
)

// Ağırlıklı oy (bir oy %30 evet + %70 hayır gibi bölünebilir)
type WeightedVoteOption struct {
    Option VoteOption
    Weight string   // LegacyDec olarak "0.3", "0.7" gibi
}
```

---

## 13. Governance Tally — Oy Sayımı

**Dosya:** `x/gov/keeper/tally.go`

### Oy Sayımı Algoritması

```go
func (k Keeper) Tally(ctx context.Context, proposal v1.Proposal) (passes, burnDeposits bool, tallyResults v1.TallyResult, err error) {
    totalVotingPower, totalValPower, results, _ := k.calculateVoteResultsAndVotingPowerFn(ctx, k, proposal)

    // Quorum kontrolü (toplam validator power'ın kaçı oy kullandı?)
    percentVoting := totalVotingPower / totalValPower
    quorum := params.Quorum  // varsayılan: 0.334 (%33.4)

    if percentVoting < quorum {
        return false, params.BurnVoteQuorum, tallyResults, nil  // Quorum yok → başarısız
    }

    // Herkes çekimser oy kullandıysa başarısız
    if totalVotingPower - results[Abstain] == 0 {
        return false, false, tallyResults, nil
    }

    // Veto kontrolü: >1/3 NoWithVeto → başarısız + deposit yakılır
    vetoThreshold := params.VetoThreshold  // varsayılan: 0.334
    if results[NoWithVeto] / totalVotingPower > vetoThreshold {
        return false, params.BurnVoteVeto, tallyResults, nil
    }

    // Çoğunluk kontrolü (çekimser hariç)
    threshold := params.Threshold  // varsayılan: 0.5 (%50)
    // Expedited proposal için: ExpeditedThreshold = 0.667 (%66.7)
    if proposal.Expedited {
        threshold = params.ExpeditedThreshold
    }

    nonAbstainPower := totalVotingPower - results[Abstain]
    if results[Yes] / nonAbstainPower > threshold {
        return true, false, tallyResults, nil  // Geçti!
    }

    return false, false, tallyResults, nil  // Başarısız
}
```

### Oy Gücü Hesabı

```go
// Delegatörün oy gücü = Delegasyon payı × Validator bonded token
votingPower := delegation.Shares * val.BondedTokens / val.DelegatorShares

// Validator kendi oy gücü = (TotalShares - DelegatorDeductions) × BondedTokens / TotalShares
// (Delegatörler kendi oylarını kullandıysa validator'ın o delegasyonlar üzerindeki gücü düşülür)
```

### Governance Parametreleri (Varsayılan)

| Parametre | Değer | Açıklama |
|-----------|-------|---------|
| MinDeposit | 10_000_000 uatom (10 ATOM) | Oylama için minimum depozito |
| MaxDepositPeriod | 2 × 24 × 7 × time.Hour (2 hafta) | Depozito toplama süresi |
| VotingPeriod | 2 × 24 × 7 × time.Hour (2 hafta) | Oylama süresi |
| Quorum | 0.334 (33.4%) | Minimum oy katılımı |
| Threshold | 0.5 (50%) | Geçme eşiği |
| VetoThreshold | 0.334 (33.4%) | Veto eşiği |
| ExpeditedThreshold | 0.667 (66.7%) | Hızlandırılmış geçme eşiği |
| MinInitialDepositRatio | 0.1 (10%) | İlk depozito oranı |

---

## 14. x/mint — Token Basımı ve Enflasyon

**Dosya:** `x/mint/keeper/mint.go`

### DefaultMintFn

```go
func DefaultMintFn(ic types.InflationCalculationFn) MintFn {
    return func(ctx sdk.Context, k *Keeper) error {
        minter, _ := k.Minter.Get(ctx)
        params, _ := k.Params.Get(ctx)

        // Stake oranı hesapla (bonded / total)
        totalStakingSupply, _ := k.StakingTokenSupply(ctx)
        bondedRatio, _ := k.BondedRatio(ctx)

        // Enflasyon oranını yeniden hesapla
        minter.Inflation = ic(ctx, minter, params, bondedRatio)
        minter.AnnualProvisions = minter.NextAnnualProvisions(params, totalStakingSupply)
        k.Minter.Set(ctx, minter)

        // Blok başına basılacak coin
        mintedCoin := minter.BlockProvision(params)

        // MaxSupply kontrolü
        totalSupply := k.bankKeeper.GetSupply(ctx, params.MintDenom).Amount
        if !maxSupply.IsZero() && totalSupply + mintedCoin.Amount > maxSupply {
            mintedCoin.Amount = maxSupply - totalSupply  // Farkı bas
        }

        // Bas ve fee collector'a gönder (distribution modülü dağıtır)
        k.MintCoins(ctx, sdk.NewCoins(mintedCoin))
        k.AddCollectedFees(ctx, mintedCoins)
    }
}
```

### Enflasyon Modeli (Varsayılan — Cosmos Hub)

```
Hedef: BondedRatio = %67
- BondedRatio < %67 → Enflasyon artar (max %20'ye kadar)
- BondedRatio > %67 → Enflasyon düşer (min %7'ye kadar)
- Her blokta ±(MaxChangeRate / BlocksPerYear) kadar değişir
```

### Minter Yapısı

```go
type Minter struct {
    Inflation        math.LegacyDec  // Yıllık enflasyon oranı (örn: 0.13 = %13)
    AnnualProvisions math.LegacyDec  // Yıllık toplam token basımı
}

func (m Minter) BlockProvision(params Params) sdk.Coin {
    // AnnualProvisions / BlocksPerYear
    provisionAmt := m.AnnualProvisions.QuoInt(sdk.NewInt(int64(params.BlocksPerYear)))
    return sdk.NewCoin(params.MintDenom, provisionAmt.TruncateInt())
}
```

---

## 15. x/distribution — Ödül Dağıtımı

**Paket:** `x/distribution/`

Cosmos SDK'da validator ödülleri F1 Fee Distribution algoritması ile hesaplanır:

### F1 Algoritması

```
Her validator için:
- HistoricalRewards[height] = kümülatif ödül/pay oranı
- CurrentRewards = bu blokta toplanan ödüller

Delegatörün ödülü hesabı:
- Withdraw sırasında: (HistoricalRewards[current] - HistoricalRewards[start]) × delegator.Shares
- Böylece her delegatör için ayrı hesap tutulmaz, sadece cumulative reward/share kaydedilir
```

### Ödül Akışı

```
Tx ücreti + Mint ödülü → FeeCollector hesabı
    ↓ (distribution BeginBlocker)
Validator Commission → Validator ödül havuzu
Kalan → Delegatörler (pay oranında)
    ↓
MsgWithdrawDelegatorReward → delegatör ödül alır
MsgWithdrawValidatorCommission → validator komisyon alır
```

### Community Pool

Toplam ödülün `CommunityTax` oranı (varsayılan %2) Community Pool'a gider. Governance tarafından harcanabilir.

---

## 16. CometBFT Entegrasyonu — ABCI Akışı

CometBFT, Cosmos SDK uygulamasıyla ABCI üzerinden konuşur:

### ABCI Mesajları

```protobuf
// CometBFT → App
InitChain(GenesisState)        → BeginBlock + module genesis
PrepareProposal(Txs)           → Uygulama tx seçimi/sıralama
ProcessProposal(Block)         → Diğer validator'lar doğrular
FinalizeBlock(Block)           → State değişikliklerini uygula
Commit()                       → IAVL root hash'i hesapla, DB commit

// App → CometBFT
ValidatorUpdates []ValidatorUpdate  → FinalizeBlock içinde döndürülür
AppHash []byte                      → Commit içinde döndürülür
Events []Event                      → Indexleme için
```

### VoteExtensions (ABCI++)

CometBFT v0.38+ ile gelen özellik:
- Her validator, oy öncesi ekstra data ekleyebilir (VoteExtension)
- Diğer validator'lar bunu doğrulayabilir (VerifyVoteExtension)
- NextBlock'ta bu extension'lar kullanılabilir (PrepareProposal'da)

Cosmos SDK BaseApp bunu `execModeVoteExtension` ve `execModeVerifyVoteExtension` modları ile destekler.

### CometBFT BFT Özellikleri

```
Byzantine fault tolerance: N = 3f+1, f = hata toleransı
Gereksinim: >2/3 validator (ağırlığa göre) imzalamış olmalı
Finality: Bir blok commit edilince kesindir (fork yok)
Liveness: >2/3 dürüst → ilerleme garantili
```

---

## 17. Store Katmanı — IAVL ve Collections

### IAVL Tree

```
CommitMultiStore
└── SubStore per module (storeKey → IAVL tree)
    ├── staking/      → IAVL (validators, delegations, vb.)
    ├── slashing/     → IAVL (signing info, pubkey → addr)
    ├── gov/          → IAVL (proposals, votes, deposits)
    └── ...
```

Her IAVL tree commit → Merkle root hash üretir. Tüm root hash'ler birleşerek `AppHash` oluşturur.

### Collections API

`cosmossdk.io/collections` v1.4.0 — tip güvenli KV store arayüzü:

```go
// Gov keeper'ından:
Proposals  collections.Map[uint64, v1.Proposal]
Votes      collections.Map[Pair[uint64, AccAddress], v1.Vote]
ProposalID collections.Sequence   // Auto-increment

// Kullanım:
proposal, err := k.Proposals.Get(ctx, proposalID)
err = k.Votes.Set(ctx, collections.Join(proposalID, voterAddr), vote)
```

**Avantajı:** Type-safe, range query destekli, otomatik serialize/deserialize.

---

## 18. Parametre Yönetimi

Cosmos SDK'da her modülün parametreleri on-chain saklanır ve governance ile değiştirilebilir:

### MsgUpdateParams Pattern

```go
// Her modülde:
func (k Keeper) UpdateParams(ctx context.Context, params types.Params) error {
    if k.GetAuthority() != msg.Authority {
        return errors.Wrapf(govtypes.ErrInvalidSigner, ...)
    }
    return k.Params.Set(ctx, params)
}
```

`authority = x/gov module account address` → Yalnızca governance tarafından değiştirilebilir.

---

## 19. Hook Sistemi — Modüller Arası Olay

```go
type StakingHooks interface {
    AfterValidatorCreated(ctx, valAddr)
    BeforeValidatorModified(ctx, valAddr)
    AfterValidatorRemoved(ctx, consAddr, valAddr)
    AfterValidatorBonded(ctx, consAddr, valAddr)
    AfterValidatorBeginUnbonding(ctx, consAddr, valAddr)
    BeforeDelegationCreated(ctx, delAddr, valAddr)
    BeforeDelegationSharesModified(ctx, delAddr, valAddr)
    AfterDelegationModified(ctx, delAddr, valAddr)
    BeforeValidatorSlashed(ctx, valAddr, fraction)
    AfterUnbondingInitiated(ctx, id)
}
```

Distribution modülü bu hook'ları dinleyerek reward tracking'i günceller. Slashing modülü de yeni validator bonded event'ini dinleyerek SigningInfo başlatır.

```go
// MultiStakingHooks: Birden fazla hook birleştirme
type MultiStakingHooks []StakingHooks

func (h MultiStakingHooks) AfterValidatorBonded(ctx, consAddr, valAddr) {
    for _, hook := range h {
        hook.AfterValidatorBonded(ctx, consAddr, valAddr)
    }
}
```

---

## 20. Nova Shield için Çıkarımlar

### A. x/staking Parametreleri — Doğrudan İlham

```
Nova Shield Staking Parametreleri (Cosmos'tan uyarlama):
- UnbondingTime: 7 gün (Cosmos'un 21 gününün 1/3'ü — daha hızlı)
- MaxValidators: 200 (başlangıç, Cosmos'un 100'ünün 2 katı)
- MaxEntries: 7 (aynı)
- MinCommissionRate: %5 (Cosmos'un %0 minimum'undan daha koruyucu)
- KeyRotationFee: 10 NSC
```

### B. Validator Durum Makinesi — Doğrudan Alınabilir

```rust
// Nova Shield Rust implementasyonu (Cosmos'tan birebir port):
enum ValidatorStatus {
    Unbonded,    // Henüz aktif değil
    Unbonding,   // Ayrılma sürecinde (7 gün bekleme)
    Bonded,      // Aktif validator set'te
}

struct Validator {
    operator_address: Address,
    consensus_pubkey: Dilithium3PubKey,  // Post-quantum!
    jailed: bool,
    status: ValidatorStatus,
    tokens: u128,
    delegator_shares: Decimal,
    commission: Commission,
    min_self_delegation: u128,
}
```

### C. EndBlocker ValidatorSet Güncelleme — Doğrudan Alınabilir

Cosmos'un `ApplyAndReturnValidatorSetUpdates()` mantığı:
1. Power sıralaması (en yükseği en aktif)
2. MaxValidators limiti
3. Durum geçişleri (Unbonded→Bonded, Bonded→Unbonding)
4. Tek seferde pool transferi

Nova Shield'de aynı mantık, CometBFT yerine **BLS Threshold imzası** ile bütünleşir.

### D. x/slashing — Doğrudan Alınabilir (Bitmap ile)

```rust
// Roaring Bitmap yerine nova shield'de:
struct SigningInfo {
    start_height: u64,
    index_offset: u64,
    missed_blocks_counter: u64,
    jailed_until: Option<u64>,     // Unix timestamp
    tombstoned: bool,              // Çift imza → kalıcı
}

// MissedBlockBitmap: son 100 blokta imza atıldı mı?
// Cosmos: RoaringBitmap / Nova Shield: Bitset (Rust bitvec crate)
```

**Slashing eşikleri Nova Shield için:**
- Downtime: %1 slash + 10 dakika jail (Cosmos ile aynı)
- Equivocation (çift imza): %5 slash + kalıcı jail (Cosmos ile aynı)

### E. Governance Tally — Doğrudan Alınabilir

```rust
// Nova Shield Governance Parametreleri:
struct TallyParams {
    quorum: Decimal,           // 0.334 — aynı
    threshold: Decimal,        // 0.500 — aynı
    veto_threshold: Decimal,   // 0.334 — aynı
    expedited_threshold: Decimal, // 0.667 — aynı
}

// Oy sayımı:
// 1. Quorum < %33.4 → başarısız
// 2. Veto > %33.4 → başarısız + deposit yakılır
// 3. Yes / (Total - Abstain) > %50 → geçti
```

### F. x/mint — Uyarlanabilir

Nova Shield için Cosmos'un enflasyon modeli yerine **sabit arz** seçtik (1 Milyar NSC). Ancak **distribution mekanizması** alınabilir:

```
Fee geliri → Fee Collector
    ↓
BeginBlocker (distribution)
Validator Commission → Validator Reward Pool
Kalan → Delegatörler (pay oranında)
    ↓
MsgWithdrawReward → delegatör talep eder
```

### G. F1 Fee Distribution — Doğrudan Alınabilir

F1 algoritması, her delegatör için ayrı hesap tutmak yerine kümülatif ödül/pay oranı kaydeder. O(1) withdraw, O(n) sıralama yok.

```rust
struct ValidatorHistoricalRewards {
    cumulative_reward_ratio: Vec<Decimal>,  // per token
    reference_count: u32,
}

struct DelegatorStartingInfo {
    previous_period: u64,
    stake: u128,
    height: u64,
}

// Ödül = (cum_ratio[current] - cum_ratio[start]) × stake
```

### H. Collections API — Alınabilir Pattern

`cosmossdk.io/collections` tip-güvenli KV store pattern'i Rust'ta doğal:

```rust
// Nova Shield'de Rust ile:
trait KVStore {
    fn get<K: Encode, V: Decode>(&self, key: K) -> Option<V>;
    fn set<K: Encode, V: Encode>(&mut self, key: K, value: V);
    fn delete<K: Encode>(&mut self, key: K);
    fn prefix_iter<K: Encode>(&self, prefix: K) -> impl Iterator<Item=(K, V)>;
}
```

### I. Hook Sistemi — Alınabilir

Rust'ta trait-based hook sistemi:

```rust
trait StakingHooks: Send + Sync {
    fn after_validator_bonded(&self, validator_addr: &Address);
    fn before_validator_slashed(&self, validator_addr: &Address, fraction: Decimal);
    fn after_delegation_modified(&self, delegator: &Address, validator: &Address);
}

struct MultiStakingHooks(Vec<Box<dyn StakingHooks>>);
```

### J. ConsensusKey Rotasyonu — Nova Shield'de Gerekli

MPC tabanlı hesap modelimizde:
- Guardian değişikliği = consensus key rotasyonuna benzer
- Nova Shield'de: `MsgRotateGuardian` → unbonding period sonra eski key silinir
- Ücret: `KeyRotationFee` = 10 NSC (Cosmos'ta 1 ATOM)
- Güvenlik: Eski key slash edilebilirliği unbonding period boyunca devam eder

### K. Module Authority Pattern — Alınabilir

```rust
// Her modülün yetkisi sadece governance'a ait:
struct ModuleParams {
    authority: Address,   // x/gov module address
}

impl ModuleParams {
    fn update(&mut self, caller: &Address, new_params: Params) -> Result<()> {
        ensure!(caller == &self.authority, "unauthorized");
        self.params = new_params;
        Ok(())
    }
}
```

### L. Nova Shield Özel Geliştirmeleri (Cosmos'ta Olmayan)

| Özellik | Cosmos SDK | Nova Shield |
|---------|-----------|------------|
| İmza algoritması | ed25519 | Dilithium3 (post-quantum) |
| MPC Hesap | Yok | Threshold (3/5) |
| Encrypted Mempool | Yok | IBE + Sealed-bid |
| Miras Protokolü | Yok | Emergency Guardian |
| Slashing | Var | Aynı + MPC equivocation |

---

## 21. Özet Karşılaştırma Tablosu

| Özellik | Cosmos SDK | Nova Shield |
|---------|-----------|-------------|
| Consensus | CometBFT (BFT, 2/3 supermajority) | BLS Threshold BFT (2/3 ağırlık) |
| Finality | Her blok kesin (instant finality) | Single-slot (400ms, kesin) |
| Validator | MaxValidators=100, stake ağırlıklı | MaxValidators=200 başlangıç |
| Unbonding | 21 gün | 7 gün (daha hızlı) |
| Slashing | Downtime 0.01%, Equivocation 5% | Aynı |
| Governance | Stake ağırlıklı, 50% threshold | Aynı + Guardian voting |
| Key Rotation | Var (KeyRotationFee) | Var (GuardianRotationFee) |
| Enflasyon | Dinamik (%7-%20, bonded ratio'ya göre) | Sabit arz (1B NSC) |
| Post-Quantum | Hayır (ed25519) | Evet (Dilithium3) |
| MPC | Hayır | Evet (CGGMP21, 3/5 threshold) |
| MEV Koruması | Hayır | Encrypted Mempool + Batch Auction |

---

## Sonuç

Cosmos SDK'dan alınacak en kritik 7 element:

1. **Validator durum makinesi** (Unbonded→Bonded→Unbonding) — Rust'a doğrudan port.

2. **EndBlocker ValidatorSet güncelleme** — Power sıralı iterator, MaxValidators limiti, pool transferleri.

3. **x/slashing HandleValidatorSignature** — Roaring bitmap sliding window, MissedBlocksCounter, downtime jail.

4. **Governance Tally** (quorum+veto+threshold) — Stake ağırlıklı oylama, nova shield'e birebir uygulanabilir.

5. **F1 Fee Distribution** — Kümülatif ödül/pay oranı, O(1) withdraw, delegatör ödülü.

6. **Hook sistemi** — Modüller arası olay bildirimi (MultiStakingHooks pattern).

7. **ConsensusKey rotasyonu güvenlik modeli** — Eski key unbonding period kadar sakla, nova shield'de guardian rotation için aynı.
