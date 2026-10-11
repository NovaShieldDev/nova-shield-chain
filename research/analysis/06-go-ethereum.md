# Go-Ethereum (Geth) — Kapsamlı Teknik Analiz
## Nova Shield Chain Araştırması — Repo 6

**Repo:** `research/chains/go-ethereum`
**Kaynak:** github.com/ethereum/go-ethereum
**Dil:** Go 1.24.0
**Lisans:** LGPL-3.0
**Analiz Tarihi:** 2026-07-17
**Satır Sayısı:** ~2.5 milyon satır

---

## İçindekiler

1. [Genel Mimari ve Bağımlılıklar](#1-genel-mimari-ve-bağımlılıklar)
2. [Transaction Tipleri ve Veri Yapıları](#2-transaction-tipleri-ve-veri-yapıları)
3. [Block Header ve Block Yapısı](#3-block-header-ve-block-yapısı)
4. [Receipt (İşlem Makbuzu) Yapısı](#4-receipt-işlem-makbuzu-yapısı)
5. [EVM — Ethereum Sanal Makinesi](#5-evm--ethereum-sanal-makinesi)
6. [EVM Interpreter — Run Döngüsü](#6-evm-interpreter--run-döngüsü)
7. [Precompiled Contracts](#7-precompiled-contracts)
8. [Transaction Pool (Mempool)](#8-transaction-pool-mempool)
9. [State — Durum Yönetimi](#9-state--durum-yönetimi)
10. [Merkle Patricia Trie](#10-merkle-patricia-trie)
11. [State Processor — Blok İşleme](#11-state-processor--blok-işleme)
12. [Consensus Engine Arayüzü](#12-consensus-engine-arayüzü)
13. [Ethash — Proof of Work](#13-ethash--proof-of-work)
14. [Clique — Proof of Authority](#14-clique--proof-of-authority)
15. [Beacon Consensus (PoS Geçişi)](#15-beacon-consensus-pos-geçişi)
16. [EIP-1559 BaseFee Mekanizması](#16-eip-1559-basefee-mekanizması)
17. [P2P Ağ Katmanı](#17-p2p-ağ-katmanı)
18. [Transaction İmzalama](#18-transaction-imzalama)
19. [BlockChain Core](#19-blockchain-core)
20. [EIP Evrim Zaman Çizelgesi](#20-eip-evrim-zaman-çizelgesi)
21. [Nova Shield için Çıkarımlar](#21-nova-shield-için-çıkarımlar)
22. [Güvenlik Analizi](#22-güvenlik-analizi)
23. [Kritik Bulgular Özeti](#23-kritik-bulgular-özeti)

---

## 1. Genel Mimari ve Bağımlılıklar

### 1.1 Modül Yapısı

```
module github.com/ethereum/go-ethereum
go 1.24.0
```

### 1.2 Kritik Bağımlılıklar

| Kütüphane | Amaç | Önem |
|-----------|------|------|
| `cockroachdb/pebble v1.1.5 + v2.1.4` | LSM-tree tabanlı KV storage | Yerine LevelDB/RocksDB/Pebble seçimi var |
| `consensys/gnark-crypto v0.18.1` | BLS12-381, BN256 kriptografik işlemler | ZK proof backend |
| `crate-crypto/go-eth-kzg v1.5.0` | KZG commitment (EIP-4844) | Blob tx için |
| `ethereum/c-kzg-4844/v2 v2.1.8` | C binding KZG (daha hızlı) | Blob tx için |
| `decred/dcrd/dcrec/secp256k1/v4` | secp256k1 curve operasyonları | ECDSA imzalama |
| `holiman/uint256 v1.3.2` | 256-bit integer aritmetiği | EVM stack değerleri |
| `supranational/blst v0.3.16` | BLS imzalama (Beacon/PoS) | Validator imzaları |
| `protolambda/bls12-381-util v0.1.0` | BLS12-381 yardımcıları | Beacon consensus |
| `ProjectZKM/Ziren/zkvm_runtime` | ZK-VM runtime | Yeni ekleme — zkVM desteği |
| `grafana/pyroscope-go v1.2.7` | Profiling | Üretim profilleme |
| `go.opentelemetry.io/otel v1.41.0` | Tracing/Observability | Telemetri |

### 1.3 Dizin Yapısı (Ana Modüller)

```
go-ethereum/
├── core/                    # Çekirdek blockchain mantığı
│   ├── types/               # Transaction, Block, Receipt, Log
│   ├── vm/                  # EVM implementasyonu
│   ├── state/               # StateDB (world state)
│   ├── txpool/              # Mempool
│   ├── blockchain.go        # BlockChain ana struct
│   ├── state_processor.go   # Blok işleme (ApplyTransaction)
│   └── genesis.go           # Genesis bloğu
├── consensus/               # Consensus engine'leri
│   ├── consensus.go         # Engine interface
│   ├── ethash/              # PoW
│   ├── clique/              # PoA
│   └── beacon/              # PoS (Merge sonrası)
├── p2p/                     # P2P ağ katmanı
│   ├── server.go            # Peer yönetimi
│   ├── discover/            # Node discovery (Kademlia)
│   ├── enode/               # Ethereum Node Record (ENR)
│   └── rlpx/                # RLPx transport protokolü
├── trie/                    # Merkle Patricia Trie
├── eth/                     # Ethereum protokol servisleri
│   ├── handler.go           # ETH protokol handler
│   └── sync.go              # Block/state senkronizasyonu
├── miner/                   # Block üretimi
├── accounts/                # Anahtar yönetimi
│   ├── keystore/            # Şifrelenmiş keystore
│   └── accounts.go          # Account interface
├── crypto/                  # Kriptografik primitifler
│   ├── crypto.go            # Keccak256, secp256k1
│   └── kzg4844/             # KZG commitment
└── params/                  # Zincir parametreleri, fork block numaraları
```

---

## 2. Transaction Tipleri ve Veri Yapıları

### 2.1 Transaction Tip Sabitleri

```go
// core/types/transaction.go
const (
    LegacyTxType     = 0x00  // Frontier'dan beri
    AccessListTxType = 0x01  // EIP-2930 (Berlin)
    DynamicFeeTxType = 0x02  // EIP-1559 (London)
    BlobTxType       = 0x03  // EIP-4844 (Cancun)
    SetCodeTxType    = 0x04  // EIP-7702 (Prague)
)
```

### 2.2 TxData Interface

Her transaction tipinin implement etmesi gereken arayüz:

```go
type TxData interface {
    txType() byte
    chainID() *big.Int
    accessList() AccessList
    data() []byte
    gas() uint64
    gasPrice() *big.Int
    gasTipCap() *big.Int     // maxPriorityFeePerGas
    gasFeeCap() *big.Int     // maxFeePerGas
    value() *big.Int
    nonce() uint64
    to() *common.Address
    rawSignatureValues() (v, r, s *big.Int)
    effectiveGasPrice(dst *big.Int, baseFee *big.Int) *big.Int
    encode(*bytes.Buffer) error
    decode([]byte) error
    sigHash(chainID *big.Int) common.Hash
    copy() TxData
}
```

### 2.3 Transaction Struct

```go
type Transaction struct {
    inner TxData          // Her tip için farklı implement
    time  time.Time       // Mempool'a girdiği zaman (yerel)

    // Yüksek frekanslı erişim için atomic cache'ler
    hash atomic.Pointer[common.Hash]
    size atomic.Uint64
    from atomic.Pointer[sigCache]  // {signer, from}
}
```

**Tasarım Kararı:** `inner` field'ı bir interface olarak tutulması, fork'lar arası uyumluluk sağlar. Yeni tx tipi eklemek için sadece yeni bir struct implement etmek yeterlidir.

### 2.4 EffectiveGasTip Hesabı

```
effectiveGasTip = min(gasFeeCap - baseFee, gasTipCap)
```

Eğer `gasFeeCap < baseFee` ise transaction pool'a kabul edilmez.

### 2.5 EIP-2718 Encoding (Typed Transaction Envelope)

```
LegacyTx:    RLP([nonce, gasPrice, gas, to, value, data, v, r, s])
TypedTx:     type_byte || RLP(tx_fields)
```

Ağ üzerinde gönderimde `type_byte || payload` formatı kullanılır.

### 2.6 BlobTx (EIP-4844) — Detaylı Analiz

```go
// core/types/tx_blob.go
type BlobTx struct {
    ChainID    *uint256.Int
    Nonce      uint64
    GasTipCap  *uint256.Int
    GasFeeCap  *uint256.Int
    Gas        uint64
    To         common.Address    // NOT nil — blob tx'de hedef zorunlu
    Value      *uint256.Int
    Data       []byte
    AccessList AccessList
    BlobFeeCap *uint256.Int     // maxFeePerBlobGas
    BlobHashes []common.Hash    // kzg4844.CalcBlobHashV1(sha256, commitment)
    Sidecar    *BlobTxSidecar   `rlp:"-"` // Ağda gönderilir, blockchain'de saklanmaz
    V, R, S    *uint256.Int
}

type BlobTxSidecar struct {
    Version     byte                 // 0=tam kanıt, 1=hücre kanıtları (EIP-7594 PeerDAS)
    Blobs       []kzg4844.Blob       // Her blob 128KB (4096 field element * 32 byte)
    Commitments []kzg4844.Commitment // KZG commitment (48 byte her blob için)
    Proofs      []kzg4844.Proof      // KZG proof
}
```

**BlobHash Hesabı:**
```go
func BlobHashes() []common.Hash {
    hasher := sha256.New()
    h := make([]common.Hash, len(sc.Commitments))
    for i := range sc.Commitments {
        h[i] = kzg4844.CalcBlobHashV1(hasher, &sc.Commitments[i])
    }
    return h
}
```

**Sidecar Versiyonlama:**
- **Version 0:** Tek kanıt — tüm blob'u bir bütün olarak doğrular (eskiden kullanılan)
- **Version 1:** Hücre kanıtları — her blob 128 hücreye (1KB) bölünür, her hücre ayrı kanıtlanır (PeerDAS/EIP-7594)

**Encoding Dualite:**
Ağda: `[tx, blobs, commitments, proofs]` — blob verisi sidecar ile birlikte
Blockchain'de: Sadece `tx` (blobs saklanmaz, finality sonrası silinir)

**Gas Hesabı:**
```go
func (tx *BlobTx) blobGas() uint64 {
    return params.BlobTxBlobGasPerBlob * uint64(len(tx.BlobHashes))
    // BlobTxBlobGasPerBlob = 131072 (2^17)
}
```

### 2.7 SetCodeTx (EIP-7702) — Account Abstraction

```go
// core/types/tx_setcode.go
type SetCodeTx struct {
    ChainID    *uint256.Int
    Nonce      uint64
    GasTipCap  *uint256.Int
    GasFeeCap  *uint256.Int
    Gas        uint64
    To         common.Address
    Value      *uint256.Int
    Data       []byte
    AccessList AccessList
    AuthList   []SetCodeAuthorization  // EIP-7702 yetkilendirme listesi
    V, R, S    *uint256.Int
}

type SetCodeAuthorization struct {
    ChainID uint256.Int
    Address common.Address  // Delege edilecek contract adresi
    Nonce   uint64
    V       uint8           // yParity
    R, S    uint256.Int
}
```

**Delegation Prefix:**
```go
var DelegationPrefix = []byte{0xef, 0x01, 0x00}  // 3 byte prefix
```

Hesap kodu: `0xef0100 || address` (23 byte) — hesabın başka bir contract'a delege ettiğini gösterir.

**Authority Recovery:**
```go
func (a *SetCodeAuthorization) SigHash() common.Hash {
    return prefixedRlpHash(0x05, []any{a.ChainID, a.Address, a.Nonce})
}

func (a *SetCodeAuthorization) Authority() (common.Address, error) {
    sighash := a.SigHash()
    // secp256k1 ecrecover
    pub, _ := crypto.Ecrecover(sighash[:], sig[:])
    addr = keccak256(pub[1:])[12:]
    return addr, nil
}
```

**EIP-7702 Önemi:** EOA'ların geçici olarak akıllı sözleşme davranışı göstermesine izin verir. Batched transaction'lar, sponsorlu transaction'lar, permission delegation mümkün olur. Bu, EIP-3074 yerine geçen, daha güvenli bir account abstraction yaklaşımı.

---

## 3. Block Header ve Block Yapısı

### 3.1 Header Struct — Tam Alan Listesi

```go
// core/types/block.go
type Header struct {
    // Temel alanlar (Frontier'dan beri)
    ParentHash  common.Hash    // Önceki bloğun hash'i
    UncleHash   common.Hash    // keccak256(RLP(uncles)) — PoS'ta EmptyUncleHash
    Coinbase    common.Address // Miner/validator adresi
    Root        common.Hash    // State root (Merkle Patricia Trie kökü)
    TxHash      common.Hash    // Transaction trie kökü
    ReceiptHash common.Hash    // Receipt trie kökü
    Bloom       Bloom          // 2048-bit bloom filter (log topics için)
    Difficulty  *big.Int       // PoW zorluğu (PoS'ta 0)
    Number      *big.Int       // Blok numarası
    GasLimit    uint64         // Maksimum gas
    GasUsed     uint64         // Kullanılan gas
    Time        uint64         // Unix timestamp
    Extra       []byte         // Ekstra veri (max 32 byte Ethash'ta)
    MixDigest   common.Hash    // Ethash mining kanıtı (PoS'ta random beacon)
    Nonce       BlockNonce     // Ethash nonce (PoS'ta 0)

    // EIP-1559 — London fork
    BaseFee     *big.Int       // Wei cinsinden minimum gas fiyatı

    // EIP-4895 — Shanghai fork
    WithdrawalsHash *common.Hash  // Validator geri çekme trie kökü

    // EIP-4844 — Cancun fork
    BlobGasUsed    *uint64    // Bu blokta harcanan blob gas
    ExcessBlobGas  *uint64    // EIP-4844 baseFee hesabı için

    // EIP-4788 — Cancun fork
    ParentBeaconRoot *common.Hash  // Önceki beacon bloğu kökü

    // EIP-7685 — Prague fork
    RequestsHash *common.Hash  // Execution layer requestleri (deposit/withdrawal/consolidation)

    // EIP-7928 — Amsterdam fork
    BlockAccessListHash *common.Hash  // Blok erişim listesi (Verkle geçişi için)

    // EIP-7843 — Amsterdam fork
    SlotNumber *uint64  // Beacon chain slot numarası
}
```

### 3.2 Header Hash Hesabı

```go
func (h *Header) Hash() common.Hash {
    return rlpHash(h)  // keccak256(RLP(header))
}
```

**Önemli:** Hash hesabında TÜM alanlar dahil edilir. Bu nedenle opsiyonel alanlar (`*big.Int`, `*common.Hash`) nil ise RLP'de omit edilir ama hash değişir.

### 3.3 Block Struct

```go
type Block struct {
    header       *Header
    uncles       []*Header
    transactions Transactions
    withdrawals  Withdrawals   // EIP-4895

    // Mutable fields
    hash atomic.Pointer[common.Hash]
    size atomic.Uint64

    // EIP-7928 (Amsterdam)
    accessList *bal.EncodingBlockAccessList
}
```

**Tasarım:** Block immutable'dır — bir kez oluşturulduktan sonra header/tx değiştirilmez. `WithBody()` ile yeni Block oluşturulur.

### 3.4 Body Struct

```go
type Body struct {
    Transactions []*Transaction
    Uncles       []*Header
    Withdrawals  []*Withdrawal   // EIP-4895: validator para çekme
}
```

---

## 4. Receipt (İşlem Makbuzu) Yapısı

### 4.1 Receipt Struct

```go
// core/types/receipt.go
type Receipt struct {
    // Consensus alanları (Yellow Paper'da tanımlı)
    Type              uint8    // Tx tipi
    PostState         []byte   // Pre-Byzantium: state root; Byzantium+: status
    Status            uint64   // 0=başarısız, 1=başarılı
    CumulativeGasUsed uint64   // Blok başından birikimli gas
    Bloom             Bloom    // 2048-bit bloom filter (log topic'leri)
    Logs              []*Log   // EVM event'leri

    // Implementation alanları (geth ekler)
    TxHash            common.Hash
    ContractAddress   common.Address  // Sadece contract creation'da dolu
    GasUsed           uint64
    EffectiveGasPrice *big.Int
    BlobGasUsed       uint64     // EIP-4844
    BlobGasPrice      *big.Int   // EIP-4844

    // Inclusion bilgisi
    BlockHash        common.Hash
    BlockNumber      *big.Int
    TransactionIndex uint
}
```

### 4.2 RLP Encoding Stratejisi

```go
// Consensus (ağ üzerinde): PostStateOrStatus + CumulativeGasUsed + Bloom + Logs
type receiptRLP struct {
    PostStateOrStatus []byte
    CumulativeGasUsed uint64
    Bloom             Bloom
    Logs              []*Log
}

// Depolama (Bloom hariç, yeniden hesaplanır):
type storedReceiptRLP struct {
    PostStateOrStatus []byte
    CumulativeGasUsed uint64
    Logs              []*Log
}
```

**Tasarım Kararı:** Bloom filter depolanmaz, her seferinde Log'lardan yeniden hesaplanır. Bu depolama alanı tasarrufu sağlar ama okumada maliyet getirir.

### 4.3 Status Encoding

```
Pre-Byzantium:  PostState = 32-byte state root
Byzantium+:     PostState = [] (empty, failed)
                PostState = [0x01] (successful)
```

### 4.4 DeriveFields — Post-İşlem Zenginleştirme

```go
func (r *Receipt) DeriveFields(signer Signer, context DeriveReceiptContext) {
    r.Type = context.Tx.Type()
    r.TxHash = context.Tx.Hash()
    r.GasUsed = context.GasUsed
    r.EffectiveGasPrice = tx.inner.effectiveGasPrice(new(big.Int), context.BaseFee)
    // BlobTx için:
    r.BlobGasUsed = tx.BlobGas()
    r.BlobGasPrice = context.BlobGasPrice
    // Contract creation tespiti:
    if context.Tx.To() == nil {
        from, _ := Sender(signer, context.Tx)
        r.ContractAddress = crypto.CreateAddress(from, tx.Nonce())
    }
    // Log zenginleştirme:
    for j, log := range r.Logs {
        log.BlockNumber = context.BlockNumber
        log.BlockHash = context.BlockHash
        log.TxHash = r.TxHash
        log.Index = logIndex + uint(j)
    }
}
```

---

## 5. EVM — Ethereum Sanal Makinesi

### 5.1 EVM Struct

```go
// core/vm/evm.go
type EVM struct {
    Context     BlockContext    // Blok seviyesi context
    TxContext   TxContext       // Transaction seviyesi context
    StateDB     StateDB         // Dünya durumu (okuma/yazma)

    table       *JumpTable      // Fork'a göre instruction set
    depth       int             // Call derinliği (max 1024)

    chainConfig *params.ChainConfig
    chainRules  params.Rules    // Hangi EIP'lerin aktif olduğu
    Config      Config          // Tracer ve diğer ayarlar

    abort       atomic.Bool     // Çalışmayı durdurma sinyali
    callGasTemp uint64          // call instruction için gas hesabı

    precompiles map[common.Address]PrecompiledContract
    jumpDests   JumpDestCache   // JUMP hedeflerinin cache'i

    readOnly    bool            // STATICCALL context'i
    returnData  []byte          // Son CALL'dan dönen veri

    arena       freeList        // Stack pool (allocation optimizasyonu)
}
```

### 5.2 BlockContext

```go
type BlockContext struct {
    CanTransfer CanTransferFunc   // Bakiye yeterli mi?
    Transfer    TransferFunc      // ETH transferi
    GetHash     GetHashFunc       // Önceki blok hash'leri (BLOCKHASH opcode)
    Coinbase    common.Address    // Block producer adresi
    GasLimit    uint64
    BlockNumber *big.Int
    Time        uint64
    Difficulty  *big.Int          // PoS'ta 0
    BaseFee     *big.Int          // EIP-1559
    BlobBaseFee *big.Int          // EIP-4844
    Random      *common.Hash      // EIP-4399: PREVRANDAO (PoS beacon randomness)
    SlotNum     *uint64           // EIP-7843
    CostPerStateByte uint64       // Amsterdam fork için
}
```

### 5.3 TxContext

```go
type TxContext struct {
    Origin       common.Address   // msg.sender (dış hesap)
    GasPrice     *big.Int
    BlobHashes   []common.Hash    // EIP-4844
    BlobFeeCap   *big.Int
    AccessEvents *state.AccessEvents  // EIP-4762 Verkle witness
}
```

### 5.4 Fork'a Göre JumpTable Seçimi

```go
// core/vm/evm.go NewEVM()
func NewEVM(blockCtx BlockContext, statedb StateDB, chainConfig *params.ChainConfig, config Config) *EVM {
    evm := &EVM{...}
    switch {
    case chainConfig.IsBogota(blockCtx.BlockNumber, blockCtx.Time):
        evm.table = &bogotaInstructionSet
    case chainConfig.IsAmsterdam(blockCtx.BlockNumber, blockCtx.Time):
        evm.table = &amsterdamInstructionSet
    case chainConfig.IsOsaka(blockCtx.BlockNumber, blockCtx.Time):
        evm.table = &osakaInstructionSet
    case chainConfig.IsPrague(blockCtx.BlockNumber, blockCtx.Time):
        evm.table = &pragueInstructionSet
    case chainConfig.IsCancun(blockCtx.BlockNumber, blockCtx.Time):
        evm.table = &cancunInstructionSet
    case chainConfig.IsShanghai(blockCtx.BlockNumber, blockCtx.Time):
        evm.table = &shanghaiInstructionSet
    case chainConfig.IsMerge(blockCtx.BlockNumber):
        evm.table = &mergeInstructionSet
    case chainConfig.IsLondon(blockCtx.BlockNumber):
        evm.table = &londonInstructionSet
    case chainConfig.IsBerlin(blockCtx.BlockNumber):
        evm.table = &berlinInstructionSet
    case chainConfig.IsIstanbul(blockCtx.BlockNumber):
        evm.table = &istanbulInstructionSet
    case chainConfig.IsConstantinople(blockCtx.BlockNumber):
        evm.table = &constantinopleInstructionSet
    case chainConfig.IsByzantium(blockCtx.BlockNumber):
        evm.table = &byzantiumInstructionSet
    case chainConfig.IsHomestead(blockCtx.BlockNumber):
        evm.table = &homesteadInstructionSet
    default:
        evm.table = &frontierInstructionSet
    }
    return evm
}
```

**13 farklı instruction set** — her fork yeni opcode ekler veya mevcut opcode'ların gas maliyetini değiştirir.

### 5.5 EVM.Call() Mekanizması

```go
func (evm *EVM) Call(caller common.Address, addr common.Address,
    input []byte, gas vm.GasBudget, value *uint256.Int) (ret []byte, leftOverGas uint64, err error) {

    // 1. Derinlik kontrolü
    if evm.depth > int(params.CallCreateDepth) {
        return nil, gas, ErrDepth
    }

    // 2. Bakiye yeterli mi?
    if !evm.Context.CanTransfer(evm.StateDB, caller, value) {
        return nil, gas, ErrInsufficientBalance
    }

    // 3. Snapshot al (revert için)
    snapshot := evm.StateDB.Snapshot()

    // 4. Hedef hesap yoksa oluştur
    if !evm.StateDB.Exist(addr) {
        // EIP-161: empty accounts on transfer
        evm.StateDB.CreateAccount(addr)
    }

    // 5. ETH transferi
    evm.Context.Transfer(evm.StateDB, caller, addr, value)

    // 6. Precompile mi?
    if p, isPrecompile := evm.precompiles[addr]; isPrecompile {
        return RunPrecompiledContract(p, input, gas)
    }

    // 7. Normal contract çalıştır
    code := evm.StateDB.GetCode(addr)
    ret, err = evm.interpreter.Run(contract, input, false)

    // 8. Hata durumunda revert
    if err != nil {
        evm.StateDB.RevertToSnapshot(snapshot)
        if err != ErrExecutionReverted {
            gas.UseAll()  // Gas tüket
        }
    }
    return ret, gas.RegularGas, err
}
```

### 5.6 CallCode vs DelegateCall vs StaticCall

| Opcode | Caller Context | Value | Code |
|--------|----------------|-------|------|
| CALL | Yeni scope | Yeni value | Hedefin kodu |
| CALLCODE | Caller'ın scope | Yeni value | Hedefin kodu |
| DELEGATECALL | Caller'ın scope ve value | Kalıtım | Hedefin kodu |
| STATICCALL | Yeni scope, readOnly=true | 0 | Hedefin kodu |

**DelegateCall Nüansı:**
```go
func (evm *EVM) DelegateCall(caller ContractRef, addr common.Address, ...) {
    // parent call'dan caller ve value'yu kalıt al
    p := evm.depth - 1
    contract.CallerAddress = callers[p].Address()
    contract.value = values[p]
}
```

---

## 6. EVM Interpreter — Run Döngüsü

### 6.1 Tam Run() Implementasyonu Analizi

```go
// core/vm/interpreter.go
func (evm *EVM) Run(contract *Contract, input []byte, readOnly bool) (ret []byte, err error) {
    // Derinlik artır (max 1024)
    evm.depth++
    defer func() { evm.depth-- }()

    // readOnly propagation: bir kez set edilince child call'lar da readOnly
    if readOnly && !evm.readOnly {
        evm.readOnly = true
        defer func() { evm.readOnly = false }()
    }

    // returnData sıfırla
    evm.returnData = nil

    // Boş kod: hemen dön
    if len(contract.Code) == 0 {
        return nil, nil
    }

    var (
        op          OpCode
        jumpTable   *JumpTable = evm.table
        mem                    = NewMemory()
        stack                  = evm.arena.stack()  // pool'dan al
        callContext = &ScopeContext{
            Memory:   mem,
            Stack:    stack,
            Contract: contract,
        }
        pc   = uint64(0)
        cost uint64
    )
    defer func() {
        stack.release()  // pool'a geri ver
        mem.Free()
    }()

    contract.Input = input

    // EIP-4762 (Verkle) kod chunk gas mekanizması
    _ = jumpTable[0]  // nil-check döngü dışında

    for {
        op = contract.GetOp(pc)
        operation := jumpTable[op]

        // Stack validation
        if sLen := stack.len(); sLen < operation.minStack {
            return nil, &ErrStackUnderflow{stackLen: sLen, required: operation.minStack}
        } else if sLen > operation.maxStack {
            return nil, &ErrStackOverflow{stackLen: sLen, limit: operation.maxStack}
        }

        // Constant gas çek
        cost = operation.constantGas
        if !contract.Gas.ChargeRegularOnly(cost) {
            return nil, ErrOutOfGas
        }

        // Dynamic gas (memory expansion ve op-specific)
        if operation.dynamicGas != nil {
            var memorySize uint64
            if operation.memorySize != nil {
                memSize, overflow := operation.memorySize(stack)
                if overflow { return nil, ErrGasUintOverflow }
                memorySize, overflow = math.SafeMul(toWordSize(memSize), 32)
                if overflow { return nil, ErrGasUintOverflow }
            }
            dynamicCost, err = operation.dynamicGas(evm, contract, stack, mem, memorySize)
            cost += dynamicCost.RegularGas
            if !contract.Gas.charge(dynamicCost) {
                return nil, ErrOutOfGas
            }
        }

        // Memory expand
        if memorySize > 0 {
            mem.Resize(memorySize)
        }

        // Execute
        res, err = operation.execute(&pc, evm, callContext)
        if err != nil { break }
        pc++
    }

    if err == errStopToken {
        err = nil  // STOP opcode = normal sonuç
    }
    return res, err
}
```

### 6.2 GasCosts Yapısı (EIP-4762 — Verkle)

```go
type GasCosts struct {
    RegularGas uint64  // Normal EVM gas
    StateGas   uint64  // Verkle state access gas (Amsterdam+)
}
```

Amsterdam fork ile iki ayrı gas boyutu var: normal hesaplamalar için RegularGas, state erişimi için StateGas.

### 6.3 Memory Expansion Gas Formülü

```
memory_cost = (memory_size_in_words^2 / 512) + (3 * memory_size_in_words)
```

Quadratic büyüme — büyük bellek tahsisi maliyetli.

### 6.4 JumpTable Operation Struct

```go
type operation struct {
    execute     executionFunc        // Opcode çalıştırma fonksiyonu
    constantGas uint64               // Sabit gas maliyeti
    dynamicGas  gasFunc              // Dinamik gas fonksiyonu (nil ise yok)
    minStack    int                  // Minimum stack derinliği
    maxStack    int                  // Maksimum stack derinliği
    memorySize  memorySizeFunc       // Gerekli memory boyutu hesabı
}
```

---

## 7. Precompiled Contracts

### 7.1 Fork'a Göre Precompile Listesi

```go
// core/vm/contracts.go

// Homestead (0x1-0x4)
precompiledContractsHomestead = map[common.Address]PrecompiledContract{
    common.BytesToAddress([]byte{1}): &ecrecover{},        // ECDSA recovery
    common.BytesToAddress([]byte{2}): &sha256hash{},       // SHA-256
    common.BytesToAddress([]byte{3}): &ripemd160hash{},    // RIPEMD-160
    common.BytesToAddress([]byte{4}): &dataCopy{},         // Identity (data copy)
}

// Byzantium (+ 0x5-0x8)
precompiledContractsByzantium = {
    ...homestead,
    {5}: &bigModExp{},        // Big integer modular exponentiation
    {6}: &bn256Add{},         // BN256 elliptic curve add
    {7}: &bn256ScalarMul{},   // BN256 scalar multiplication
    {8}: &bn256Pairing{},     // BN256 pairing check
}

// Istanbul (+ 0x9)
precompiledContractsIstanbul = {
    ...byzantium,
    {9}: &blake2F{},          // BLAKE2b compression function
}

// Cancun (+ 0xa)
precompiledContractsCancun = {
    ...istanbul,
    {10}: &kzgPointEvaluation{},  // KZG polynomial commitment verify (EIP-4844)
}

// Prague (+ 0xb-0x11)
precompiledContractsPrague = {
    ...cancun,
    {0xb}: &bls12381G1Add{},        // BLS12-381 G1 toplama
    {0xc}: &bls12381G1MultiExp{},   // BLS12-381 G1 çok noktalı çarpım
    {0xd}: &bls12381G2Add{},        // BLS12-381 G2 toplama
    {0xe}: &bls12381G2MultiExp{},   // BLS12-381 G2 çok noktalı çarpım
    {0xf}: &bls12381Pairing{},      // BLS12-381 pairing
    {0x10}: &bls12381MapG1{},       // BLS12-381 field-to-G1 mapping
    {0x11}: &bls12381MapG2{},       // BLS12-381 field-to-G2 mapping
}
```

### 7.2 KZG Point Evaluation (EIP-4844)

```go
// Girdi: versioned_hash(32) || z(32) || y(32) || commitment(48) || proof(48) = 192 byte
func (kzgPointEvaluation) Run(input []byte) ([]byte, error) {
    if len(input) != blobVerifyInputLength { // 192 byte
        return nil, errors.New("invalid input length")
    }
    versionedHash := input[:32]
    z := input[32:64]    // Evaluation point
    y := input[64:96]    // Claimed output
    commitment := input[96:144]
    proof := input[144:192]

    // commitment hash'i versioned_hash ile eşleşmeli
    if kzg4844.CalcBlobHashV1(sha256, commitment) != versionedHash {
        return nil, errors.New("mismatched versioned hash")
    }
    // KZG kanıtını doğrula: p(z) == y
    if err := kzg4844.VerifyProof(commitment, z, y, proof); err != nil {
        return nil, err
    }
    // Başarı: FIELD_ELEMENTS_PER_BLOB || BLS_MODULUS
    return kzgSuccess, nil
}
```

### 7.3 BigModExp (EIP-198)

```go
// base^exp mod mod hesabı
// Gas hesabı: EIP-2565 — ağırlıklı karmaşıklık
func (c *bigModExp) RequiredGas(input []byte) uint64 {
    bLen, eLen, mLen := parseModExpLengths(input)
    // Gas = max(200, mult_complexity(m_len) * iteration_count / 3)
}
```

---

## 8. Transaction Pool (Mempool)

### 8.1 Çok Katmanlı Pool Mimarisi

```go
// core/txpool/txpool.go
type TxPool struct {
    subpools []SubPool    // Farklı tx tipi için farklı pool
    chain    BlockChain
    state    *state.StateDB

    quit     chan struct{}
    term     chan struct{}
    newHeadCh chan core.ChainHeadEvent
}
```

**SubPool interface:**
```go
type SubPool interface {
    Filter(tx *types.Transaction) bool  // Bu pool bu tx'i kabul eder mi?
    Init(gasTip *big.Int, head *types.Header, reserve AddressReserver) error
    Close() error
    Reset(oldHead, newHead *types.Header)
    SetGasTip(tip *big.Int)
    Add(txs []*types.Transaction, local bool, sync bool) []error
    Pending(filter PendingFilter) map[common.Address][]*LazyTransaction
    SubscribeTransactions(ch chan<- core.NewTxsEvent, reorgs bool) event.Subscription
    Nonce(addr common.Address) uint64
    Stats() (int, int)  // pending, queued
    Content() (map[common.Address][]*types.Transaction, map[common.Address][]*types.Transaction)
    ContentFrom(addr common.Address) ([]*types.Transaction, []*types.Transaction)
    Locals() []common.Address
    Status(hash common.Hash) TxStatus
}
```

**SubPool tipleri:**
- `LegacyPool`: Legacy + EIP-2930 + EIP-1559 tx'leri
- `BlobPool`: EIP-4844 blob tx'leri (ayrı depolama ve fiyatlandırma)

### 8.2 LegacyPool Yapılandırması

```go
// core/txpool/legacypool/legacypool.go
var DefaultConfig = Config{
    Journal:   "transactions.rlp",
    Rejournal: time.Hour,

    PriceLimit: 1,    // Minimum gas fiyatı (Wei) — 1 Wei
    PriceBump:  10,   // Mevcut tx'i replace etmek için %10 artış gerekli

    AccountSlots: 16,          // Her adres için pending slot
    GlobalSlots:  4096 + 1024, // Toplam pending slot (5120)
    AccountQueue: 64,          // Her adres için queued slot
    GlobalQueue:  1024,        // Toplam queued slot

    Lifetime: 3 * time.Hour,   // Queued tx TTL
}

const (
    txSlotSize = 32 * 1024    // 32KB: tx byte büyüklüğü ağırlığı
    txMaxSize  = 4 * txSlotSize // 128KB: maksimum tek tx boyutu
)
```

### 8.3 Pool Döngüsü (loop goroutine)

```go
func (pool *TxPool) loop(head *types.Header) {
    defer pool.loopWG.Done()
    newTxs := make(chan core.NewTxsEvent)
    sub := pool.subs.Subscribe(newTxs)
    defer sub.Unsubscribe()

    for {
        select {
        case event := <-pool.newHeadCh:
            // Yeni blok geldi — state güncelle, geçersiz tx'leri temizle
            pool.reinit(event.Block.Header())

        case <-pool.quit:
            return
        }
    }
}
```

**Chain Head Event Driven Reset:**
Her yeni blok geldiğinde:
1. State yeniden başlatılır
2. Her subpool için ayrı goroutine'de `Reset(oldHead, newHead)` çağrılır
3. Reorg durumunda eski blokların tx'leri yeniden eklenir

### 8.4 LegacyPool İç Yapısı

```go
type LegacyPool struct {
    config      Config
    chainconfig *params.ChainConfig
    chain       BlockChain
    gasTip      atomic.Pointer[big.Int]  // Minimum tip

    txFeed event.Feed     // Yeni tx bildirimi
    signer types.Signer

    mu       sync.RWMutex
    istanbul bool   // Fork aktif mi?

    currentHead   atomic.Pointer[types.Header]
    currentState  *state.StateDB  // Güncel state'in görünümü
    pendingNonces *noncer         // Adres nonce cache

    locals  *accountSet   // Yerel tx gönderenler (eviction'dan korunur)
    journal *journal       // Disk journal (restart'ta geri yükleme)

    pending map[common.Address]*list  // Sıralı, yürütülmeye hazır tx'ler
    queue   map[common.Address]*list  // Sırasız / gelecek tx'ler
    beats   map[common.Address]time.Time  // Son heartbeat (eviction için)
    all     *lookup       // Hash → Transaction indeksi
    priced  *pricedList   // Fiyata göre heap (eviction için)
}
```

**Pending vs Queue Ayrımı:**
- **pending:** nonce sıralı, yürütülmeye hazır tx'ler (nonce = currentNonce + 1, +2, ...)
- **queue:** nonce boşluğu olan tx'ler (örn. nonce=5 var ama 3 ve 4 yok)

Queue'dakiler pending'e taşınmak için sıradaki nonce'ın gelmesini bekler.

---

## 9. State — Durum Yönetimi

### 9.1 StateDB Struct

```go
// core/state/statedb.go
type StateDB struct {
    db       Database         // Trie node veritabanı
    prefetcher *triePrefetcher  // Paralel trie node önyükleme
    reader   *stateReader      // Tek snapshot görünümü
    trie     Trie              // Account trie (account → nonce/balance/code/storageRoot)

    originalRoot common.Hash   // Bu StateDB'nin temel aldığı state root

    // Object cache'leri
    stateObjects         map[common.Address]*stateObject  // Yüklenen hesaplar
    stateObjectsDestruct map[common.Address]*types.StateAccount  // Silinen hesaplar
    mutations            map[common.Address]*mutation    // Pending değişiklikler

    // Journal (snapshot/revert için)
    journal        *journal        // Geri alınabilir değişiklik kaydı
    validRevisions []revision      // Anlık görüntü noktaları

    // EVM meta bilgisi
    dbErr  error
    refund uint64           // Gas geri iadesi (SSTORE refund, SELFDESTRUCT)
    thash  common.Hash      // Güncel tx hash
    txIndex int             // Güncel tx indeksi

    // Log yönetimi
    logs     map[common.Hash][]*types.Log
    logSize  uint
    preimages map[common.Hash][]byte  // Keccak preimage kaydı (debug)

    // EIP-2929: Gas optimizasyonu için erişim listesi
    accessList *accessList

    // EIP-4762: Verkle witness
    accessEvents *AccessEvents

    // EIP-7928: Block access list
    stateAccessList  *bal.ConstructionBlockAccessList
    blockAccessIndex uint32

    // EIP-1153: Transient storage (tx süresi dolunca silinir)
    transientStorage transientStorage

    // Witness verisi
    witness *stateless.Witness
}
```

**TriesInMemory = 128** — 128 son blok state'i bellekte tutulur, geri kalanlar diske yazılır.

### 9.2 StateAccount (Hesap Verisi)

```go
type StateAccount struct {
    Nonce    uint64
    Balance  *uint256.Int
    Root     common.Hash   // Storage trie kökü (boş → EmptyRootHash)
    CodeHash []byte        // keccak256(code) veya EmptyCodeHash
}
```

**Empty Account (EIP-161):**
```go
func (s *stateObject) empty() bool {
    return s.data.Nonce == 0 &&
           s.data.Balance.IsZero() &&
           bytes.Equal(s.data.CodeHash, types.EmptyCodeHash.Bytes())
}
```

### 9.3 stateObject — Hesap State Katmanları

```go
// core/state/state_object.go
type stateObject struct {
    address     common.Address
    addressHash common.Hash     // keccak256(address) — trie key

    origin *types.StateAccount  // DB'den yüklendiğindeki orijinal hal
    data   types.StateAccount   // Güncel hal (mutasyonlar uygulandı)

    trie  Trie   // Storage trie (lazy loaded)
    code  []byte // Contract byte kodu

    // Storage katmanları (4 katman)
    originStorage    Storage  // DB'den yüklenen orijinal değerler
    dirtyStorage     Storage  // Tx'de değiştirilen değerler
    pendingStorage   Storage  // Tx'ler arası bekleyen değerler
    uncommittedStorage Storage // Commit'e hazır değerler

    dirtyCode      bool   // Kod değişti mi?
    selfDestructed bool   // SELFDESTRUCT çağrıldı mı?
    newContract    bool   // EIP-6780: Bu tx'te mi oluşturuldu?
}
```

**Storage Katman Akışı:**
```
Okuma: dirtyStorage → pendingStorage → originStorage → DB
Yazma: dirtyStorage (her SSTORE)
Tx sonu: dirtyStorage → pendingStorage (Finalise)
Blok sonu: pendingStorage → uncommittedStorage → DB (Commit)
```

### 9.4 Journal — Snapshot/Revert Mekanizması

Journal, state değişikliklerini geri alınabilir bir şekilde kaydeder:

```go
type journalEntry interface {
    revert(*StateDB)
    dirtied() *common.Address
}

// Değişiklik tipleri:
// createObjectChange, resetObjectChange (SELFDESTRUCT geri al)
// suicideChange, touchChange
// balanceChange, nonceChange, storageChange
// codeChange, addLogChange, addPreimageChange
// accessListAddAccountChange, accessListAddSlotChange
// transientStorageChange
```

**Snapshot/Revert:**
```go
func (s *StateDB) Snapshot() int {
    id := s.nextRevisionId
    s.nextRevisionId++
    s.validRevisions = append(s.validRevisions, revision{id, s.journal.length()})
    return id
}

func (s *StateDB) RevertToSnapshot(revid int) {
    // Revision bul
    idx := sort.Search(len(s.validRevisions), func(i int) bool {
        return s.validRevisions[i].id >= revid
    })
    snap := s.validRevisions[idx]

    // Journal entry'leri tersine çalıştır
    s.journal.revert(s, snap.journalIndex)
    s.validRevisions = s.validRevisions[:idx]
}
```

**Bu, CALL failure → revert mekanizmasının temelidir.**

### 9.5 EIP-2929: Access List (Berlin Fork)

```go
type accessList struct {
    addresses map[common.Address]int  // -1: slot'suz, >=0: slotList indeksi
    slots     []map[common.Hash]struct{}
}
```

İlk erişimde "cold" gas ödenir, sonraki erişimlerde "warm" (daha ucuz) gas ödenir. Bu MEV/re-entrancy koruması sağlar.

---

## 10. Merkle Patricia Trie

### 10.1 Trie Struct

```go
// trie/trie.go
type Trie struct {
    root      node            // Kök düğüm
    owner     common.Hash     // Bu trie'ın sahibi (storage trie için adres hash)
    committed bool            // Commit sonrası kullanılamaz
    unhashed  int             // Son hash'lemeden bu yana eklenen yaprak sayısı
    uncommitted int           // Son commit'ten bu yana güncelleme sayısı
    reader    *Reader         // DB okuyucu
    opTracer  *opTracer       // Hangi anahtarların erişildiğini izle (witness için)
    prevalueTracer *PrevalueTracer  // Değer değişimlerini izle
}
```

### 10.2 Node Tipleri

```go
type node interface{ cache() (hashNode, bool) }

type (
    fullNode  struct { Children [17]node; flags nodeFlag }  // Branch node (16 nibble + value)
    shortNode struct { Key []byte; Val node; flags nodeFlag }  // Extension / Leaf
    hashNode  []byte  // 32-byte keccak hash (lazy loading)
    valueNode []byte  // Gerçek değer (yaprak verisi)
)
```

**MPT Yapısı:**
```
                Root (fullNode veya shortNode)
               /    |    ...    \
          fullNode      ...   shortNode
         / ... \                    |
    shortNode  hashNode        valueNode
        |          |
    valueNode  (DB'den yükle → node)
```

**Key Encoding (Hex Prefix):**
```
keybytesToHex: her byte → 2 nibble (4-bit)
key[0:2] = hex nibble'ları
key[-1] = 0x10 (leaf terminator)
```

### 10.3 Get Operasyonu

```go
func (t *Trie) get(origNode node, key []byte, pos int) (value []byte, newnode node, didResolve bool, err error) {
    switch n := (origNode).(type) {
    case nil:
        return nil, nil, false, nil
    case valueNode:
        return n, n, false, nil
    case *shortNode:
        if !bytes.HasPrefix(key[pos:], n.Key) {
            return nil, n, false, nil  // Anahtar bu path'te değil
        }
        // Derinliğe in
        return t.get(n.Val, key, pos+len(n.Key))
    case *fullNode:
        return t.get(n.Children[key[pos]], key, pos+1)
    case hashNode:
        // DB'den yükle (lazy resolution)
        child, err := t.resolveAndTrack(n, key[:pos])
        return t.get(child, key, pos)
    }
}
```

### 10.4 Update (Insert/Delete)

```go
func (t *Trie) Update(key, value []byte) error {
    k := keybytesToHex(key)  // key → nibble dizisi
    if len(value) != 0 {
        // insert: t.insert(t.root, nil, k, valueNode(value))
    } else {
        // delete: t.delete(t.root, nil, k)
    }
}
```

**Insert Stratejisi:**
- nil → yeni yaprak oluştur
- shortNode: Ortak prefix bul, fullNode oluştur, böl
- fullNode: İlgili child'a rekursif ekle
- hashNode: Önce çözümle, sonra ekle

### 10.5 Paralel Prefetch

```go
func (t *Trie) Prefetch(keylist [][]byte) error {
    fn, ok := t.root.(*fullNode)
    if !ok || len(keylist) < 16 {
        // Az anahtar: sıralı
        for _, key := range keylist { t.Get(key) }
        return nil
    }
    // Çok anahtar: ilk nibble'a göre grupla, goroutine'lerle paralel
    keys := make(map[byte][][]byte)
    for _, key := range keylist {
        hkey := keybytesToHex(key)
        keys[hkey[0]] = append(keys[hkey[0]], hkey)
    }
    var eg errgroup.Group
    for pos, ks := range keys {
        eg.Go(func() error { /* pos için paralel get */ })
    }
    return eg.Wait()
}
```

**4 farklı iterator tipi:**
- `NodeIterator`: Tüm düğümleri iterate
- `NodeIteratorWithPrefix`: Sadece belirli prefix altındakileri
- `NodeIteratorWithRange`: [start, end) aralığındakileri
- `SubtreeIterator`: Alt ağaç traverse

---

## 11. State Processor — Blok İşleme

### 11.1 Process() — Ana Blok Yürütme Fonksiyonu

```go
// core/state_processor.go
func (p *StateProcessor) Process(ctx context.Context, block *types.Block,
    statedb *state.StateDB, jumpDestCache vm.JumpDestCache, cfg vm.Config) (*ProcessResult, error) {

    config := p.chainConfig()
    gp := NewGasPool(block.GasLimit())  // Blok gas bütçesi

    // 1. DAO Fork hard fork uygulaması
    if config.DAOForkSupport && block.Number() == config.DAOForkBlock {
        misc.ApplyDAOHardFork(statedb)
    }

    // 2. EVM oluştur
    context := NewEVMBlockContext(header, p.chain, nil)
    signer := types.MakeSigner(config, header.Number, header.Time)
    evm := vm.NewEVM(context, statedb, config, cfg)

    // 3. Pre-execution sistem çağrıları
    blockAccessList.Merge(PreExecution(ctx, block.BeaconRoot(), parent, config, evm, ...))

    // 4. Her transaction'ı işle
    for i, tx := range block.Transactions() {
        msg, _ := TransactionToMessage(tx, signer, header.BaseFee)
        statedb.SetTxContext(tx.Hash(), i, uint32(i+1))
        receipt, bal, _ := ApplyTransactionWithEVM(msg, gp, statedb, blockNumber, ...)
        receipts = append(receipts, receipt)
        blockAccessList.Merge(bal)
    }

    // 5. Post-execution sistem çağrıları (EIP-6110/7002/7251)
    requests, bal, _ := PostExecution(ctx, config, ...)

    // 6. Engine.Finalize (block reward)
    p.chain.Engine().Finalize(p.chain, header, statedb, block.Body(), ...)

    return &ProcessResult{
        Receipts: receipts,
        Requests: requests,
        Logs:     allLogs,
        GasUsed:  gp.Used(),
        Bal:      blockAccessList,
    }, nil
}
```

### 11.2 PreExecution Sistem Çağrıları

```go
func PreExecution(ctx context.Context, beaconRoot *common.Hash, parent *types.Header,
    config *params.ChainConfig, evm *vm.EVM, number *big.Int, time uint64) *bal.ConstructionBlockAccessList {

    // EIP-7997 (Amsterdam): Deterministic deployment factory
    if IsAmsterdam && !WasAmsterdam {
        misc.ApplyEIP7997(evm.StateDB)
    }

    // EIP-4788 (Cancun): Beacon root'u contract'a yaz
    if beaconRoot != nil {
        ProcessBeaconBlockRoot(*beaconRoot, evm, blockAccessList)
        // from: 0x000...000 (system address)
        // to:   0x000F3df6D732807Ef1319fB7B8bB8522d0Beac02 (BeaconRootsAddress)
        // data: beaconRoot (32 byte)
    }

    // EIP-2935/7709 (Prague): Parent block hash'ini history contract'a yaz
    if IsPrague || IsUBT {
        ProcessParentBlockHash(parent.Hash(), evm, blockAccessList)
        // to: 0x0F792be4B0c0cb4DAE440Ef133E90C0eCD48CCCC (HistoryStorageAddress)
    }
}
```

### 11.3 PostExecution Sistem Çağrıları (Prague)

```go
func PostExecution(...) (requests [][]byte, ...) {
    // EIP-6110: Deposit loglarını parse et
    ParseDepositLogs(&requests, allLogs, config)
    // depositTopic = 0x649bbc62...

    // EIP-7002: Withdrawal queue contract'ını çağır
    ProcessWithdrawalQueue(&requests, rules, evm, ...)
    // to: WithdrawalQueueAddress, requestType = 0x01

    // EIP-7251: Consolidation queue contract
    ProcessConsolidationQueue(&requests, rules, evm, ...)
    // to: ConsolidationQueueAddress, requestType = 0x02

    // EIP-8282 (Amsterdam): Builder deposit/exit
    ProcessBuilderDepositQueue(&requests, ...)  // requestType = 0x03
    ProcessBuilderExitQueue(&requests, ...)     // requestType = 0x04
}
```

### 11.4 ApplyTransactionWithEVM

```go
func ApplyTransactionWithEVM(msg *Message, gp *GasPool, statedb *state.StateDB,
    blockNumber *big.Int, blockHash common.Hash, blockTime uint64,
    tx *types.Transaction, evm *vm.EVM) (receipt *types.Receipt, bal *bal.ConstructionBlockAccessList, err error) {

    // Tracer hook'ları
    if hooks.OnTxStart != nil { hooks.OnTxStart(evm.GetVMContext(), tx, msg.From) }
    defer func() { if hooks.OnTxEnd != nil { hooks.OnTxEnd(receipt, err) } }()

    // Mesajı uygula (EVM çalıştır)
    result, err := ApplyMessage(evm, msg, gp)

    // State finalize (dirty → pending)
    if IsByzantium {
        bal = evm.StateDB.Finalise(true)
    } else {
        root = statedb.IntermediateRoot(IsEIP158)
    }

    // Verkle: tx-local access event'leri blok-local'a merge et
    if IsUBT { statedb.AccessEvents().Merge(evm.AccessEvents) }

    return MakeReceipt(evm, result, statedb, ...)
}
```

### 11.5 AssembleBlock (Miner)

```go
func AssembleBlock(chain consensus.ChainHeaderReader, header *types.Header,
    state *state.StateDB, body *types.Body, receipts []*types.Receipt,
    blockAccessList *bal.ConstructionBlockAccessList) *types.Block {

    // State root hesapla
    header.Root = state.IntermediateRoot(chain.Config().IsEIP158(header.Number))

    if IsAmsterdam {
        // EIP-7928: BlockAccessListHash
        bal := blockAccessList.ToEncodingObj()
        balHash := bal.Hash()
        header.BlockAccessListHash = &balHash
        return types.NewBlock(header, body, receipts, trie.NewStackTrie(nil)).WithAccessListUnsafe(bal)
    }
    return types.NewBlock(header, body, receipts, trie.NewStackTrie(nil))
}
```

---

## 12. Consensus Engine Arayüzü

### 12.1 Engine Interface

```go
// consensus/consensus.go
type Engine interface {
    // Author: Bloğu üreten adresi döndür (miner/validator)
    Author(header *types.Header) (common.Address, error)

    // VerifyHeader: Consensus kurallarına uygunluk kontrolü
    VerifyHeader(chain ChainHeaderReader, header *types.Header) error

    // VerifyHeaders: Batch başlık doğrulama (concurrent)
    VerifyHeaders(chain ChainHeaderReader, headers []*types.Header) (chan<- struct{}, <-chan error)

    // VerifyUncles: Uncle blok doğrulama (PoS'ta her zaman error)
    VerifyUncles(chain ChainReader, block *types.Block) error

    // Prepare: Header consensus alanlarını doldur (difficulty, mix, nonce)
    Prepare(chain ChainHeaderReader, header *types.Header) error

    // Finalize: Blok ödüllerini uygula (PoW: miner reward, PoA: nothing)
    Finalize(chain ChainHeaderReader, header *types.Header, state vm.StateDB,
        body *types.Body, blockAccessIndex uint32, bal *bal.ConstructionBlockAccessList)

    // Seal: Bloğu imzala/mühürle (PoW: mining, PoA: private key imzası)
    Seal(chain ChainHeaderReader, block *types.Block, results chan<- *types.Block, stop <-chan struct{}) error

    // SealHash: Mühürlenmeden önceki header hash'i (imzalanan mesaj)
    SealHash(header *types.Header) common.Hash

    // CalcDifficulty: Sonraki blok için zorluk hesabı
    CalcDifficulty(chain ChainHeaderReader, time uint64, parent *types.Header) *big.Int

    // Close: Engine'i kapat
    Close() error
}
```

---

## 13. Ethash — Proof of Work

### 13.1 Block Ödülleri

```go
// consensus/ethash/consensus.go
var (
    FrontierBlockReward       = uint256.NewInt(5e+18)  // 5 ETH
    ByzantiumBlockReward      = uint256.NewInt(3e+18)  // 3 ETH (EIP-649)
    ConstantinopleBlockReward = uint256.NewInt(2e+18)  // 2 ETH (EIP-1234)
)
```

### 13.2 Uncle Ödülü Formülü

```go
// Uncle miner ödülü: (uncle.Number + 8 - block.Number) * blockReward / 8
for _, uncle := range uncles {
    r.AddUint64(uncle.Number, 8)
    r.Sub(r, header.Number)
    r.Mul(r, blockReward)
    r.Rsh(r, 3)   // /8
    stateDB.AddBalance(uncle.Coinbase, r, ...)

    // Block miner'a da uncle dahil etme ödülü: blockReward / 32
    r.Rsh(blockReward, 5)
    reward.Add(reward, r)
}
stateDB.AddBalance(header.Coinbase, reward, ...)
```

### 13.3 Difficulty Adjustment (Byzantium+ / EIP-100)

```
diff = parent_diff + (parent_diff / 2048) * max(
    (2 if parent_has_uncles else 1) - (block_time - parent_time) // 9,
    -99
) + 2^(fake_block_number // 100000 - 2)
```

Fark Homestead'den: Uncle dahil olunca +2 (1 yerine), hedef 9 saniye aralık.

**Ice Age (Difficulty Bomb):**
```
fake_block_number = max(0, block_number - bomb_delay)
period_count = fake_block_number // 100000
bomb = 2^(period_count - 2)  # period_count > 1 ise
```

Bomb delay'ler: Constantinople→5M, London→9.7M, GrayGlacier→11.4M

### 13.4 SealHash — Mühürsüz Header Hash'i

```go
// Nonce ve MixDigest hariç tüm header alanları hash'lenir
enc := []interface{}{
    ParentHash, UncleHash, Coinbase, Root, TxHash, ReceiptHash,
    Bloom, Difficulty, Number, GasLimit, GasUsed, Time, Extra,
    // + BaseFee (EIP-1559'dan sonra)
}
return keccak256(RLP(enc))
```

### 13.5 VerifyHeader Kontrolleri

1. Extra data: max 32 byte
2. Timestamp: parent.Time < header.Time < now+15s
3. Difficulty: CalcDifficulty() ile eşleşmeli
4. GasLimit: ≤ 2^63-1 ve parent'a göre %1024 değişim limiti
5. GasUsed: ≤ GasLimit
6. BaseFee: EIP-1559 formülüne göre (London+)
7. Sayı: parent.Number + 1

**Önemli:** Ethash Shanghai/Cancun desteklemez — Merge sonrası sadece Beacon kullanılır.

---

## 14. Clique — Proof of Authority

### 14.1 Temel Sabitler

```go
const (
    checkpointInterval = 1024   // Her 1024 blokta bir snapshot disk'e yaz
    inmemorySnapshots  = 128    // Bellekte tutulan snapshot sayısı
    inmemorySignatures = 4096   // Bellekte tutulan imza sayısı
)

var (
    epochLength = uint64(30000)  // Her 30000 blokta signer listesi sıfırlanır
    extraVanity = 32             // Extra data'nın ilk 32 byte'ı (vanity)
    extraSeal   = 65             // Extra data'nın son 65 byte'ı (ECDSA imza)

    nonceAuthVote = 0xffffffffffffffff  // Signer ekleme oyu
    nonceDropVote = 0x0000000000000000  // Signer çıkarma oyu

    diffInTurn = big.NewInt(2)  // Sırası gelen signer'ın bloğu
    diffNoTurn = big.NewInt(1)  // Sırası olmayan signer'ın bloğu
)
```

### 14.2 Signer Recovery — ecrecover

```go
func ecrecover(header *types.Header, sigcache *sigLRU) (common.Address, error) {
    // Cache kontrolü
    if address, known := sigcache.Get(header.Hash()); known {
        return address, nil
    }

    // Extra data'nın son 65 byte'ı imza
    signature := header.Extra[len(header.Extra)-extraSeal:]

    // SealHash (imzasız header hash'i) hesapla ve recover
    pubkey, _ := crypto.Ecrecover(SealHash(header).Bytes(), signature)

    // pubkey → Ethereum address
    var signer common.Address
    copy(signer[:], crypto.Keccak256(pubkey[1:])[12:])
    return signer, nil
}
```

### 14.3 Snapshot Yapısı

Snapshot, belirli bir blok yüksekliğindeki signer setini ve oy durumunu tutar:

```go
type Snapshot struct {
    Number  uint64                      // Blok numarası
    Hash    common.Hash                 // Blok hash'i
    Signers map[common.Address]struct{} // Geçerli signer'lar
    Recents map[uint64]common.Address   // Hangi blokta kim imzaladı (double-sign engeli)
    Votes   []*Vote                     // Oy listesi
    Tally   map[common.Address]Tally    // Oy tally
}
```

### 14.4 Sıra Belirleme — inturn()

```go
func (s *Snapshot) inturn(number uint64, signer common.Address) bool {
    signers := s.signers()  // Sıralı signer listesi
    offset := number % uint64(len(signers))
    return signers[offset] == signer
}
```

**Fork Seçimi:** Sırası gelen signer (difficulty=2) tercih edilir, sırası olmayan (difficulty=1) kabul edilir. Bu, fork durumunda ağırlıklı seçimi sağlar.

### 14.5 Recently Signed Engeli

```go
// Aynı signer art arda blok üretemez
for seen, recent := range snap.Recents {
    if recent == signer {
        limit := uint64(len(snap.Signers)/2 + 1)
        if seen > number-limit {
            return errRecentlySigned
        }
    }
}
```

N signer varsa, aynı signer en az N/2+1 blok beklemek zorunda. Bu, tek noktadan ele geçirme riskini azaltır.

### 14.6 Signer Set Değişikliği (Governance)

```
Coinbase: Oylanan adres
Nonce: 0xffffffffffffffff → ekleme oyu
       0x0000000000000000 → çıkarma oyu

Karar: Signer sayısının yarısından fazlası (>N/2) oy verirse uygulanır
Epoch bloğunda (number % epoch == 0): Tüm oylar sıfırlanır, signer listesi Extra data'ya yazılır
```

### 14.7 Header Format (Clique)

```
Extra: [32-byte vanity][signer_list (checkpoint'ta)][65-byte ECDSA seal]
MixDigest: 0x0 (PoW anlamı yok)
UncleHash: EmptyUncleHash (uncle yok)
Difficulty: 1 veya 2 (turn-ness)
```

---

## 15. Beacon Consensus (PoS Geçişi)

### 15.1 Beacon Struct

```go
// consensus/beacon/consensus.go
type Beacon struct {
    ethone consensus.Engine  // Pre-merge engine (Ethash veya Clique)
}
```

**Beacon, Ethash veya Clique'i wrap eder.** Merge öncesi bloklar orijinal engine tarafından işlenir, merge sonrası bloklar Beacon tarafından.

### 15.2 PoW → PoS Geçiş Tespiti

```go
// difficulty == 0 ise post-merge (PoS), > 0 ise pre-merge (PoW)
func (beacon *Beacon) verifyHeader(chain consensus.ChainHeaderReader, header *types.Header, parent *types.Header) error {
    if header.Difficulty.Sign() == 0 {
        // PoS bloğu — Beacon kuralları
        return beacon.verifyPoSHeader(chain, header, parent)
    }
    // PoW bloğu — orijinal engine'e devret
    return beacon.ethone.VerifyHeader(chain, header)
}

func (beacon *Beacon) splitHeaders(chain consensus.ChainHeaderReader, headers []*types.Header) (preMerge, postMerge []*types.Header) {
    for _, h := range headers {
        if h.Difficulty.Sign() == 0 {
            postMerge = append(postMerge, h)
        } else {
            preMerge = append(preMerge, h)
        }
    }
}
```

### 15.3 PoS Header Kuralları

Post-merge bloğunda:
- `Difficulty == 0`
- `Nonce == [0,0,0,0,0,0,0,0]`
- `UncleHash == EmptyUncleHash`
- `MixDigest == PREVRANDAO` (beacon randomness)
- `SlotNumber` mevcut (EIP-7843 Amsterdam+)

### 15.4 Finalize (PoS)

```go
func (beacon *Beacon) Finalize(chain ChainHeaderReader, header *types.Header, state vm.StateDB, ...) {
    if header.Difficulty.Sign() == 0 {
        // PoS: Blok ödülü yok
        // EIP-4895: Validator withdrawal'larını uygula
        processWithdrawals(state, body.Withdrawals)
        return
    }
    // PoW: Orijinal engine'e devret
    beacon.ethone.Finalize(...)
}
```

---

## 16. EIP-1559 BaseFee Mekanizması

### 16.1 CalcBaseFee Algoritması

```go
// consensus/misc/eip1559/eip1559.go
func CalcBaseFee(config *params.ChainConfig, parent *types.Header) *big.Int {
    if !config.IsLondon(parent.Number) {
        return new(big.Int).SetUint64(params.InitialBaseFee)  // 1 Gwei (1e9 Wei)
    }

    parentGasTarget := parent.GasLimit / config.ElasticityMultiplier()  // ElasticityMultiplier=2, target=GasLimit/2

    if parent.GasUsed == parentGasTarget {
        return new(big.Int).Set(parent.BaseFee)  // Değişmez
    }

    if parent.GasUsed > parentGasTarget {
        // Hedeften fazla kullanım → BaseFee artar
        // delta = parentBaseFee * (gasUsed - gasTarget) / gasTarget / BaseFeeChangeDenominator
        // BaseFeeChangeDenominator = 8 (en fazla %12.5 değişim)
        num = (gasUsed - gasTarget) * parentBaseFee / gasTarget / 8
        num = max(num, 1)  // En az 1 Wei artış
        return parentBaseFee + num
    } else {
        // Hedeften az kullanım → BaseFee azalır
        num = (gasTarget - gasUsed) * parentBaseFee / gasTarget / 8
        return max(parentBaseFee - num, 0)
    }
}
```

### 16.2 BaseFee Özellikleri

- **Maksimum değişim:** Bir blokta en fazla %12.5 artış veya azalış (bölü 8)
- **Hedef:** GasLimit / 2 (ElasticityMultiplier = 2)
- **Başlangıç:** 1 Gwei = 1,000,000,000 Wei
- **Yakılır:** BaseFee tüm validator'a gitmez, yakılır (EIP-1559)
- **Miner/validator geliri:** Sadece `tip` (gasTipCap)

### 16.3 VerifyEIP1559Header

```go
func VerifyEIP1559Header(config *params.ChainConfig, parent, header *types.Header) error {
    // GasLimit değişim limiti: ±1/1024
    parentGasLimit := parent.GasLimit * ElasticityMultiplier  // Pre-London ise
    if err := VerifyGaslimit(parentGasLimit, header.GasLimit); err != nil {
        return err
    }
    // BaseFee hesaplandığı gibi olmalı
    expected := CalcBaseFee(config, parent)
    if header.BaseFee.Cmp(expected) != 0 {
        return errInvalidBaseFee
    }
    return nil
}
```

---

## 17. P2P Ağ Katmanı

### 17.1 Server Struct

```go
// p2p/server.go
type Server struct {
    Config                    // PrivateKey, MaxPeers, BootstrapNodes, vb.

    // Transport factory
    newTransport func(net.Conn, *ecdsa.PublicKey) transport  // varsayılan: newRLPX

    listener     net.Listener
    ourHandshake *protoHandshake

    // Node discovery
    nodedb    *enode.DB
    localnode *enode.LocalNode
    discv4    *discover.UDPv4   // Kademlia (EIP-8 / devp2p)
    discv5    *discover.UDPv5   // Discovery v5 (Ethereum Node Records)
    discmix   *enode.FairMix    // v4 + v5 karıştırıcı

    dialsched *dialScheduler    // Bağlantı zamanlayıcı

    // Kanallar (run loop'a mesaj için)
    quit                    chan struct{}
    addtrusted              chan *enode.Node
    removetrusted           chan *enode.Node
    checkpointPostHandshake chan *conn  // İki el sıkışma arasında kontrol
    checkpointAddPeer       chan *conn  // Peer tam kuruldu
    delpeer                 chan peerDrop

    inboundHistory expHeap  // IP başına gelen bağlantı geçmişi (throttling)
}
```

### 17.2 Bağlantı Tipleri

```go
type connFlag int32
const (
    dynDialedConn  connFlag = 1 << iota  // Keşif yoluyla bağlanılan
    staticDialedConn                      // Manuel eklenen (admin_addPeer)
    inboundConn                           // Gelen bağlantı
    trustedConn                           // Güvenilir peer (slot limiti bypass)
)
```

### 17.3 Transport Interface (RLPx)

```go
type transport interface {
    doEncHandshake(prv *ecdsa.PrivateKey) (*ecdsa.PublicKey, error)   // ECIES handshake
    doProtoHandshake(our *protoHandshake) (*protoHandshake, error)    // Capability negotiation
    MsgReadWriter                                                       // Mesaj okuma/yazma
    close(err error)
}
```

**İki Aşamalı Handshake:**
1. **Encryption Handshake (RLPx):** ECDH ile oturum anahtarı türetme (ECIES)
2. **Protocol Handshake:** Capability listesi alışverişi (hangi protokoller destekleniyor: `eth/68`, `snap/1`, vb.)

### 17.4 Server.Start() — Başlatma Akışı

```go
func (srv *Server) Start() error {
    // 1. Local node oluştur (ENR oluştur, handshake hazırla)
    srv.setupLocalNode()
    // devp2p handshake: Version + Name + ID(pubkey[1:]) + Caps

    // 2. Port mapping (UPnP/NAT-PMP)
    srv.setupPortMapping()

    // 3. TCP listen
    srv.setupListening()

    // 4. Discovery (UDP)
    srv.setupDiscovery()
    // discv4: Kademlia (UDP/4) — ENR desteksiz
    // discv5: ENR tabanlı discovery (UDP/5) — daha modern

    // 5. Dial scheduler — outbound bağlantı yönetimi
    srv.setupDialScheduler()

    // 6. Main loop goroutine
    go srv.run()
}
```

### 17.5 Discovery Mimarisi

```
discv4 (UDP/30303)          discv5 (UDP/30303, shared)
    |                              |
    +----------> FairMix <---------+
                     |
               dialScheduler
                     |
                 Connections
                     |
                  Peers
```

**FairMix:** Her kaynaktan (v4, v5, static) sırayla peer önerir. Bir kaynağı tüketmeden diğerine geçilmez.

### 17.6 Rate Limiting (Inbound Throttle)

```go
const inboundThrottleTime = 30 * time.Second
// Aynı IP'den 30 saniye içinde sadece bir bağlantı kabul edilir
// expHeap ile son bağlantı zamanları tutulur
```

### 17.7 Zaman Aşımları

```go
const (
    defaultDialTimeout  = 15 * time.Second
    frameReadTimeout    = 30 * time.Second   // Mesaj okuma limiti
    frameWriteTimeout   = 20 * time.Second   // Mesaj yazma limiti
)
```

---

## 18. Transaction İmzalama

### 18.1 MakeSigner — Fork'a Göre Signer Seçimi

```go
// core/types/transaction_signing.go
func MakeSigner(config *params.ChainConfig, blockNumber *big.Int, blockTime uint64) Signer {
    switch {
    case config.IsPrague(blockNumber, blockTime):
        return NewPragueSigner(config.ChainID)
    case config.IsCancun(blockNumber, blockTime):
        return NewCancunSigner(config.ChainID)
    case config.IsLondon(blockNumber):
        return NewLondonSigner(config.ChainID)
    case config.IsBerlin(blockNumber):
        return NewEIP2930Signer(config.ChainID)
    case config.IsEIP155(blockNumber):
        return NewEIP155Signer(config.ChainID)
    case config.IsHomestead(blockNumber):
        return new(HomesteadSigner)
    default:
        return new(FrontierSigner)
    }
}
```

### 18.2 SignTx

```go
func SignTx(tx *Transaction, s Signer, prv *ecdsa.PrivateKey) (*Transaction, error) {
    h := s.Hash(tx)           // sigHash: prefix + RLP(fields)
    sig, _ := crypto.Sign(h[:], prv)  // secp256k1 ECDSA
    return tx.WithSignature(s, sig)
}
```

**EIP-155 sigHash:**
```
keccak256(RLP([nonce, gasPrice, gas, to, value, data, chainID, 0, 0]))
```

**EIP-1559 sigHash:**
```
keccak256(0x02 || RLP([chainID, nonce, maxPriorityFeePerGas, maxFeePerGas, gas, to, value, data, accessList]))
```

### 18.3 Sender — Adresi Recovery

```go
func Sender(signer Signer, tx *Transaction) (common.Address, error) {
    // Atomic cache kontrolü
    if sc := tx.from.Load(); sc != nil {
        if sc.signer.Equal(signer) {
            return sc.from, nil
        }
    }
    // secp256k1 ecrecover
    addr, _ := signer.Sender(tx)
    tx.from.Store(&sigCache{signer, addr})
    return addr, nil
}
```

### 18.4 Signer Hierarchy

```
FrontierSigner
    └── HomesteadSigner (v/r/s replay protection yok)
        └── EIP155Signer (v = chainID*2+35/36)
            └── EIP2930Signer (+accessList)
                └── LondonSigner (+maxFeePerGas/maxPriorityFeePerGas)
                    └── CancunSigner (+blobFeeCap/blobHashes)
                        └── PragueSigner (+authList EIP-7702)
```

---

## 19. BlockChain Core

### 19.1 BlockChain Struct

```go
// core/blockchain.go
type BlockChain struct {
    chainConfig *params.ChainConfig
    cacheConfig *CacheConfig

    db        ethdb.Database   // LevelDB/Pebble
    snaps     *snapshot.Tree   // Düz key-value snapshot (hızlı okuma)
    triedb    *triedb.Database // MPT node veritabanı
    codedb    ethdb.KeyValueStore  // Contract kodu ayrı depola

    hc    *HeaderChain         // Sadece header chain (light client)

    chainmu ClosableMutex     // Blockchain yazma kilidi

    // Atomic pointers (lock-free okuma)
    currentBlock      atomic.Pointer[types.Header]
    currentFinalBlock atomic.Pointer[types.Header]  // Finalized (BFT kesin)
    currentSafeBlock  atomic.Pointer[types.Header]  // Safe (fork olsa bile geçerli)

    // LRU cache'ler
    bodyCache     *lru.Cache[common.Hash, *types.Body]       // 256 entry
    receiptsCache *lru.Cache[common.Hash, []*types.Receipt]  // 32 entry
    blockCache    *lru.Cache[common.Hash, *types.Block]      // 256 entry
    jumpDestCache JumpDestCache  // JUMPDEST analiz cache

    engine    consensus.Engine
    validator Validator
    processor Processor

    // Event subscription
    scope        event.SubscriptionScope
    chainFeed    event.Feed  // ChainEvent (yeni blok)
    chainHeadFeed event.Feed // ChainHeadEvent (en uzun chain)
    logsFeed     event.Feed
    rmLogsFeed   event.Feed  // Reorg sırasında kaldırılan log'lar
}
```

### 19.2 CacheConfig (State Stratejisi)

```go
type CacheConfig struct {
    TrieCleanLimit int    // MB: clean trie node cache
    TrieDirtyLimit int    // MB: dirty trie node cache (commit tetikleyicisi)
    StateScheme    string // "hash" (geleneksel) veya "path" (yeni)
    ArchiveMode    bool   // Tüm state'leri sakla
    StateHistory   uint64 // Kaç blok geriye state sakla
    SnapshotLimit  int    // MB: snapshot cache
    HistoryPolicy  HistoryPolicy  // State history politikası
    VmConfig       vm.Config
    TxLookupLimit  uint64  // Tx hash → block number indeksi kaç blok geriye
}

const BlockChainVersion = 9  // DB schema versiyonu
```

**State Scheme'leri:**
- **Hash scheme:** Her node keccak256(RLP(node)) ile depolanır. Düğüm paylaşımı var ama pruning zor.
- **Path scheme:** Her node path'ine göre depolanır. State history tutmak kolay, pruning etkin.

### 19.3 InsertChain — Blok Ekleme

Yüksek seviye akış:
```
InsertChain(chain []*types.Block)
    → validateBody (basic checks)
    → VerifyHeaders (concurrent, engine)
    → insertChain (asıl işleme döngüsü)
        → statedb = StateDB(parent.Root)
        → processor.Process(block, statedb)
        → validator.ValidateState (stateRoot, gasUsed, receipts)
        → WriteBlockAndSetHead
            → WriteBlock (DB'ye yaz)
            → SetHead (en uzun chain?)
            → ChainHeadEvent yayınla
```

---

## 20. EIP Evrim Zaman Çizelgesi

| Fork | EIP'ler | Nova Shield İlgisi |
|------|---------|-------------------|
| **Frontier** (2015) | Temel EVM, Gas, secp256k1 | EVM core |
| **Homestead** (2016) | DELEGATECALL, CREATE2 yok | EVM call mekanizması |
| **EIP-155** (2016) | Replay protection (chainID) | İmzalama |
| **Byzantium** (2017) | REVERT, STATICCALL, bn256, bigModExp | EVM güvenlik |
| **Constantinople** (2019) | CREATE2, SHR/SHL/SAR, EIP-1234 (block reward ↓) | CREATE2 kritik |
| **Istanbul** (2019) | BLAKE2, ChainID opcode, EIP-2929 erken versiyonu | Gas optimizasyonu |
| **Berlin** (2021) | EIP-2929 (access list gas), EIP-2930 (typed tx) | Access list |
| **London** (2021) | **EIP-1559 BaseFee**, EIP-3529 (refund azaltma), EIP-3541 (0xEF) | Fee mekanizması |
| **Merge** (2022) | PoS geçişi, PREVRANDAO, uncle yok, block reward yok | Consensus |
| **Shanghai** (2023) | EIP-4895 (withdrawal), PUSH0, EIP-3855 | Withdrawal |
| **Cancun** (2024) | **EIP-4844** (blob tx, KZG), TLOAD/TSTORE (EIP-1153), MCOPY, BLOBHASH/BLOBBASEFEE | Blob, Transient storage |
| **Prague** (2025) | **EIP-7702** (SetCode), EIP-2537 (BLS precompile), EIP-6110/7002/7251 (staking) | Account abstraction, BLS |
| **Amsterdam** (2026+) | EIP-7928 (Block access list), EIP-7843 (SlotNumber), EIP-8282 (Builder), Verkle tree geçişi | Verkle proof |

---

## 21. Nova Shield için Çıkarımlar

### 21.1 Transaction Tipleri — Doğrudan Uygulanabilir

**Nova Shield'e adapte edilecek tx tipleri:**

```
NSC Transaction Tipleri (önerilen):
0x00: LegacyTx (geriye uyumluluk için)
0x01: GuardianTx (MPC guardian yönetimi)
0x02: EncryptedTx (encrypted mempool — MEV koruması)
0x03: ThresholdTx (3/5 MPC ile imzalanmış tx)
0x04: InheritanceTx (miras protokolü — zaman kilidi + devralan)
0x05: DilithiumTx (post-quantum imzalı tx)
```

**Go-ethereum'dan alınacak yapı:** `TxData` interface pattern. Her tx tipi için ayrı struct, ortak `Transaction` wrapper, atomic from cache.

### 21.2 EVM Uyumluluk Katmanı

Go-ethereum'dan öğrenilen EVM mimarisi Nova Shield'e şöyle entegre edilir:

```
Nova Shield VM Katmanları:
1. Nova VM (WASM native) — asıl execution
2. EVM Compatibility Layer
   ├── EVM interpreter (tam olarak aynı Run döngüsü)
   ├── Precompiled contracts (keccak256, ecrecover, bn256, BLS12-381)
   ├── Jump table (fork versiyonu destekleme)
   └── StateDB adapter (WASM state → EVM state bridge)
```

**Kritik:** EVM semantiğini değiştirmemek gerekiyor. `Call → Snapshot → Execute → RevertToSnapshot` akışı aynen korunmalı. Bunu WASM üzerine port etmek gerekmektedir.

### 21.3 State DB Mimari Kararlar

**Go-ethereum StateDB'den alınacak pattern'ler:**

1. **4-katmanlı Storage:** originStorage → dirtyStorage → pendingStorage → uncommittedStorage
2. **Journal + Snapshot/Revert:** CALL failure recovery için zorunlu
3. **Access List (EIP-2929):** Gas optimizasyonu + MEV azaltma
4. **Transient Storage (EIP-1153):** Tx-scoped storage (Nova Shield'de MEV-safe pattern olarak kullanılabilir)

**Nova Shield'e özgü eklemeler:**
- Encrypted mempool state: Şifreli tx'lerin çözülmeden önce state'e yazılmaması
- Guardian state: MPC threshold durumlarının takibi
- Post-quantum imza doğrulama state: Dilithium doğrulama sonuçları

### 21.4 Consensus Engine Interface

Go-ethereum'un `Engine` interface'i Nova Shield'in DPoS + BLS Threshold yapısı için mükemmel uyum sağlar:

```go
// Nova Shield Consensus Engine (Go-ethereum pattern'i ile)
type NSCEngine interface {
    Author(header *types.Header) (common.Address, error)
    VerifyHeader(chain ChainHeaderReader, header *types.Header) error
    VerifyHeaders(chain ChainHeaderReader, headers []*types.Header) (chan<- struct{}, <-chan error)
    Prepare(chain ChainHeaderReader, header *types.Header) error
    Finalize(chain ChainHeaderReader, header *types.Header, state vm.StateDB, body *types.Body) error
    Seal(chain ChainHeaderReader, block *types.Block, results chan<- *types.Block, stop <-chan struct{}) error

    // Nova Shield'e özgü eklemeler
    VerifyBLSThreshold(block *types.Block, signatures []BLSSignature) error
    RotateValidatorSet(epoch uint64) error
    ApplySlashingCondition(validator common.Address, evidence SlashingEvidence) error
}
```

### 21.5 Fee Mekanizması (EIP-1559 Tabanlı)

Go-ethereum'un EIP-1559 implementasyonu Nova Shield fee mekanizmasına aynen uygulanabilir:

```
Nova Shield Fee:
BaseFee = CalcBaseFee(parent)  // Go-ethereum algoritması ile aynı
          ← %12.5 max değişim per block
          ← Hedef: blockGasLimit / 2
Tip      = transaction.gasTipCap  // Validator'a gider
BaseFee  = YAKILIYOR (Nova Shield Protocol burn)
Protocol fee = %0.01 (her DEX trade'den ayrı)
```

**Kritik Fark:** Go-ethereum'da BaseFee yakılır (deflasyon). Nova Shield'de de aynı mekanizma uygulanacak: supply sınırlı 1 Milyar NSC.

### 21.6 P2P Katmanı — libp2p ile Değiştirme

Go-ethereum kendi devp2p protokolünü kullanır; Nova Shield kararı libp2p+gossipsub. Ancak go-ethereum'dan öğrenilenler:

1. **İki Aşamalı Handshake Zorunluluğu:** Encryption + Protocol negotiation
2. **Fair Mix:** Birden fazla discovery kaynağından eşit oranda peer alma
3. **Inbound Throttle:** IP başına bağlantı hız sınırı (DoS koruması)
4. **Trusted Peer Listesi:** Validator'lar birbirini trusted olarak eklemelidir
5. **Connection Types:** dynDial, staticDial, inbound, trusted ayrımı

### 21.7 Mempool (Encrypted Mempool Tasarımı)

Go-ethereum'un SubPool mimarisi Nova Shield'in encrypted mempool için mükemmel:

```
Nova Shield TxPool SubPool'ları:
1. EncryptedPool (MEV koruması)
   ├── Tx şifreli gelir (IBE — rolling-shutter'dan alınan şifreleme)
   ├── İçerik göremez → MEV imkânsız
   └── Epoch slot kapanınca hepsi çözülür

2. PlainPool (geriye uyumluluk, acil durum)
   ├── Go-ethereum LegacyPool'un özdeşi
   └── Priority gas fee (EIP-1559 ile)

3. ThresholdPool (MPC tx'leri)
   └── 3/5 signer aggregation bekleme
```

**Slot kapanma mekanizması:**
```
SlotStart → txler şifreli eklenir
SlotEnd → DKG epoch key açıklanır → toplu şifre çöz → sort → batch execute
```

### 21.8 Merkle Trie vs Verkle Trie

Go-ethereum Verkle trie'ye geçiş yapıyor (EIP-4762, Amsterdam fork). Nova Shield kararı:

**Öneri:** Başlangıçta Merkle Patricia Trie (MPT), daha sonra Verkle Trie geçişi.
- MPT: Kanıtlanmış, güvenli, EVM uyumlu
- Verkle: Daha küçük proof boyutu (~150 byte vs ~3KB), stateless client desteği

Go-ethereum'dan alınacak MPT implementasyonu: `shortNode / fullNode / hashNode / valueNode` yapısı Rust'a port edilebilir.

### 21.9 System Calls (EIP-4788, EIP-2935 Pattern)

Nova Shield'in özellikli sistem çağrıları (go-ethereum PreExecution/PostExecution pattern'i):

```go
// Nova Shield System Calls
PreExecution:
  - ProcessMPCEpochSetup(epochKey, evm)         // Encrypted mempool için epoch key
  - ProcessGuardianRotation(rotationData, evm)   // Guardian seti güncellemesi
  - ProcessValidatorRewards(epoch, evm)           // Validator ödül dağıtımı

PostExecution:
  - ProcessInheritanceTriggers(logs, evm)         // Hareketsizlik tespiti
  - ProcessSlashingRequests(evidences, evm)        // Validator ceza
  - ProcessBatchAuction(bids, evm)                // Sealed-bid auction settlement
```

---

## 22. Güvenlik Analizi

### 22.1 Güçlü Taraflar

1. **Snapshot Isolation:** Journal tabanlı revert mekanizması mükemmel izolasyon sağlar. Başarısız işlemler state'e etki etmez.

2. **Depth Limit (1024):** Reentrant call saldırılarına karşı hard limit.

3. **Gas Bound Checking:** Her operasyon öncesi gas kontrolü, gas overflow için `uint64` sınırı.

4. **readOnly Propagation:** STATICCALL içinden state değişikliği yapılamaması. Parent'tan child'a readOnly prop.

5. **EIP-2929 Access List:** Storage slot ilk erişimde 2100 gas, sonraki erişimlerde 100 gas. Bu, reentrancy saldırılarında yeniden erişimi pahalı kılar.

6. **EIP-155 Replay Protection:** Chain ID imzada, farklı ağlarda tx replay edilemez.

### 22.2 Bilinen Güvenlik Riskleri

1. **Reentrancy (Dışarıdan):** Solidity seviyesinde korunma gerekir, EVM seviyesinde guarantee yok. Go-ethereum bunu etkileyemez.

2. **Integer Overflow:** `holiman/uint256` kullanılıyor — Go'nun native `big.Int`'ten daha güvenli. Ama kontrat seviyesinde overflow riski devam eder.

3. **Front-running (MEV):** Go-ethereum'da şifreli mempool YOK. Transaction ordering tam anlamıyla serbest. Nova Shield bunu çözüyor.

4. **Clique Centralization:** Clique PoA küçük signer seti gerektirir. N signer, N/2+1 kompromis yeterli. Nova Shield'de 200-1000 validator ile bu risk çok daha dağıtık.

5. **Selfdestruct EIP-6780:** Selfdestruct artık sadece aynı tx içinde çalışıyor (CREATE + SELFDESTRUCT). Tarihi kod daha güvensiz.

### 22.3 Nova Shield'de Alınacak Güvenlik Önlemleri

1. **Encrypted Mempool:** MEV saldırılarını matematiksel olarak imkânsız kılır
2. **Post-quantum imzalar (Dilithium3):** secp256k1 gelecekte quantum ile kırılabilir
3. **MPC Hesap Modeli:** Private key yok → key çalınması imkânsız
4. **BLS Threshold Finality:** 2/3 validator anlaşması → single slot finality
5. **Slashing Conditions:** Double-sign, surround vote tespiti + stake kesme

---

## 23. Kritik Bulgular Özeti

### 23.1 Mimari Bulgular

| Bulgu | Detay | Nova Shield Etkisi |
|-------|-------|--------------------|
| EVM call stack depth=1024 | Hard limit, DoS koruması | Aynı limit alınmalı |
| 4-katmanlı storage | originStorage→dirty→pending→uncommitted | Rust StateDB'de aynı pattern |
| Journal-based revert | O(1) snapshot, O(n) revert | Zorunlu bileşen |
| SubPool architecture | Blob, Legacy ayrı pool | EncryptedPool + PlainPool |
| Fork-based JumpTable | 13 farklı instruction set | EVM compat katmanı |
| Atomic from cache | Transaction sender cache | Validation performansı |
| LRU + Pebble | Hierarchical caching | Nova Shield storage katmanı |

### 23.2 Protokol Bulguları

| Protokol | Gözlem | Nova Shield Adaptasyonu |
|----------|--------|------------------------|
| EIP-1559 BaseFee | Proven supply/demand mekanizması | Doğrudan uygula, BaseFee yak |
| EIP-4844 Blob | Data availability için KZG + ayrı gas market | Gelecekte L2 destesi için hazır |
| EIP-7702 SetCode | EOA → contract geçici delegation | Account abstraction foundation |
| Clique PoA | Signer rotation, snapshot-based | DPoS validator rotasyonu için referans |
| Beacon PoS | Finality, slot-based, PREVRANDAO | Direct adoption: slot=400ms |
| RLPx Transport | Bidirectional ECIES | libp2p Noise ile değiştirilecek |

### 23.3 Gas Mekanizması

| Bileşen | Go-Ethereum Değeri | Nova Shield Önerisi |
|---------|-------------------|---------------------|
| GasLimit | Dinamik (parent±1/1024) | Aynı mekanizma |
| BaseFee | CalcBaseFee (±12.5% per block) | Aynı mekanizma |
| Blob Gas | Ayrı gas market, ~1 blob/tx | EIP-4844 pattern |
| StateGas | Amsterdam Verkle (ayrı gas) | Verkle geçişinde |

### 23.4 Sonuç: go-ethereum'dan Alınacaklar

**Doğrudan Port Edilecek (Rust'a):**
1. Transaction tip sistemi (TxData interface pattern)
2. EVM Run döngüsü (interpreter.go)
3. StateDB snapshot/revert mekanizması (journal)
4. Merkle Patricia Trie (4 node tipi)
5. EIP-1559 BaseFee hesabı
6. EIP-2929 Access List gas mekanizması
7. Consensus Engine interface

**İlham Alınacak (Nova Shield'e özgü implementasyon):**
1. SubPool mimarisi → EncryptedPool, PlainPool, ThresholdPool
2. PreExecution/PostExecution → MPCEpochSetup, BatchAuction settlement
3. P2P Server → libp2p ile aynı kavramlar
4. Block validation pipeline → NSC validator pipeline

**Atılacak / Değiştirilecek:**
1. secp256k1 ECDSA → Dilithium3 (post-quantum)
2. Keccak256 → BLAKE3
3. devp2p → libp2p + gossipsub
4. Ethash PoW → DPoS + BLS Threshold
5. Single-key EOA → MPC-native account model
6. Unencrypted mempool → Encrypted mempool (IBE/shcrypto)

---

*Analiz tamamlandı. Dosya: research/analysis/06-go-ethereum.md*
*Sonraki: Repo 7 — avalanchego (Go, Avalanche)*
