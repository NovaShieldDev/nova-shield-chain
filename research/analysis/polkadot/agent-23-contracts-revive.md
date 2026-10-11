# Agent-23: pallet-contracts + pallet-revive Analizi

## Kapsanan Dosyalar

### pallet-contracts
- `substrate/frame/contracts/src/lib.rs` — Config trait, pallet tanımları, dispatchables
- `substrate/frame/contracts/src/gas.rs` — GasMeter, EngineMeter, Token trait
- `substrate/frame/contracts/src/schedule.rs` — Schedule, Limits, InstructionWeights
- `substrate/frame/contracts/src/exec.rs` — Stack, Frame, Ext trait, çağrı akışı
- `substrate/frame/contracts/src/wasm/mod.rs` — WasmBlob, CodeInfo, Determinism
- `substrate/frame/contracts/src/wasm/prepare.rs` — LoadedModule, validate(), prepare()
- `substrate/frame/contracts/src/wasm/runtime.rs` — Runtime, RuntimeCosts, seal_* host fn
- `substrate/frame/contracts/src/storage.rs` — ContractInfo, DeletionQueueManager
- `substrate/frame/contracts/src/storage/meter.rs` — RawMeter, Diff, depo metering
- `substrate/frame/contracts/src/primitives.rs` — StorageDeposit, ExecReturnValue, ContractResult
- `substrate/frame/contracts/src/transient_storage.rs` — TransientStorage, StorageMeter
- `substrate/frame/contracts/src/address.rs` — DefaultAddressGenerator
- `substrate/frame/contracts/src/chain_extension.rs` — ChainExtension trait

### pallet-revive
- `substrate/frame/revive/src/lib.rs` — Config trait, events, errors, dispatchables, depo
- `substrate/frame/revive/src/limits.rs` — tüm sayısal sabitler
- `substrate/frame/revive/src/exec.rs` — Stack, Frame, Ext/PrecompileExt/PrecompileWithInfoExt
- `substrate/frame/revive/src/storage.rs` — ContractInfo, AccountInfo, AccountType
- `substrate/frame/revive/src/address.rs` — AddressMapper trait, AccountId32Mapper
- `substrate/frame/revive/src/metering/gas.rs` — SignedGas, gaz dönüşüm formülleri
- `substrate/frame/revive/src/metering/weight.rs` — EngineMeter (PolkaVM), ChargedAmount
- `substrate/frame/revive/src/metering/storage.rs` — RawMeter, Diff, depo metering
- `substrate/frame/revive/src/transient_storage.rs` — StorageMeter (fraction limit)

---

## 1. Mimari Genel Bakis

### pallet-contracts

Wasm akıllı sözleşme yürütme ortamı. Wasmi interpreter üzerinde çalışır. Mimari katmanlar:

```
Dispatchable (call/instantiate/upload_code)
    ↓
Stack<T, WasmBlob>  (exec.rs — çağrı yığıtı)
    ↓
GasMeter + StorageMeter  (çift kaynak takibi)
    ↓
WasmBlob::execute()  (wasm/mod.rs)
    ↓
wasmi::Instance  + Runtime<E>  (host fn köprüsü)
    ↓
seal_*  host functions  (wasm/runtime.rs)
    ↓
Ext trait implementasyonu  (Stack → ContractInfoOf, DeletionQueue, vb.)
```

Kod bir kez yüklenir (`PristineCode`, `CodeInfoOf`), hash üzerinden referans sayılır. Çoklu kontrat aynı kodu paylaşabilir.

### pallet-revive

pallet-contracts'ın EVM/PolkaVM destekli halefi. İki bytecode türünü destekler:

- **PolkaVM (PVM)**: Solidity → revive compiler → PolkaVM bytecode. `BLOB_MAGIC` prefix ile tanınır.
- **EVM bytecode**: Standart EVM runtime code. `revm` interpreter üzerinde yürütülür.

```
eth_transact / call / instantiate  (dispatchables)
    ↓
ExecStack<T, ContractBlob>  (exec.rs)
    ↓
FrameMeter (weight) + RawMeter (depo) + TransientStorageMeter
    ↓
ContractBlob::execute()  (vm/mod.rs)
    ├── PVM: polkavm::Engine
    └── EVM: revm interpreter
    ↓
Syscall / Precompile  (seal_* → sycall interface)
```

**Kritik fark**: pallet-contracts Ink! için, pallet-revive Solidity/EVM toolchain uyumluluğu için tasarlanmıştır.

---

## 2. Contract Lifecycle

### pallet-contracts

#### 2.1 Deploy (Instantiate)
Kaynak: `exec.rs:738-766`, `lib.rs`

```rust
pub fn run_instantiate(
    origin: T::AccountId,
    executable: E,
    gas_meter: &'a mut GasMeter<T>,
    storage_meter: &'a mut storage::meter::Meter<T>,
    ...
    salt: &[u8],
) -> Result<(T::AccountId, ExecReturnValue), ExecError>
```

Akış:
1. `FrameArgs::Instantiate` oluşturulur — sender, nonce, code_hash, salt, input_data
2. Adres türetme: `hash("contract_addr_v1" ++ deployer ++ code_hash ++ input_data ++ salt)` (`address.rs:63`)
3. `ContractInfo::new()` → duplicate contract kontrolü, trie_id = hash(account, nonce)
4. `charge_instantiate()` — depo taahhüdü: base deposit + code lockup deposit
5. `initial_transfer()` — value transfer
6. Wasm `deploy` fonksiyonu yürütülür (export adı: `"deploy"`)
7. `Instantiated` event emit edilir

#### 2.2 Call
Kaynak: `exec.rs:704-726`

1. `FrameArgs::Call` → `ContractInfoOf::get(dest)` — kontrat var mı?
2. `E::from_storage(code_hash, gas_meter)` — Wasm blob yüklenir, `CodeLoadToken` ile ücretlendirilir
3. `Frame::new()` — `nested_gas` + `nested_storage` oluşturulur
4. Wasm `call` export yürütülür
5. `Called` event emit edilir

#### 2.3 Terminate (seal_terminate)
Kaynak: `exec.rs`, `wasm/runtime.rs`

- `self.ext.terminate(beneficiary)` çağrısı
- Kontrat aynı call stack'teyse hata: "Termination of a contract while on call stack is denied"
- Kontrat bilgisi `CachedContract::Terminated` durumuna geçer
- `ContractInfo::queue_trie_for_deletion()` → `DeletionQueue`'ya eklenir
- Tüm bakiye beneficiary'ye transfer edilir
- Constructor içinde terminate çağrısı: `Error::TerminatedInConstructor` (`exec.rs:1014-1015`)

#### 2.4 Code Upgrade (set_code_hash)
- Mevcut kod referansı serbest bırakılır, yeni koda kilitlenir
- `storage_base_deposit` güncellenir (`ContractInfo::update_base_deposit`)

### pallet-revive

pallet-contracts ile benzer ama farklılıklar:
- Adres H160 (Ethereum uyumlu)
- `terminate_if_same_tx`: yalnızca aynı tx'te oluşturulan kontratlar silinebilir (EVM selfdestruct semantiği)
- CREATE / CREATE2 opcode'ları desteklenir (create1, create2 fonksiyonları `address.rs`'de)
- EOA (Externally Owned Account) kavramı mevcut — `AccountType::EOA`

---

## 3. Gas Metering

### pallet-contracts Gas Modeli
Kaynak: `gas.rs`, `schedule.rs`

**İki katmanlı model:**
- **Host tarafı**: `GasMeter<T>` — `Weight` cinsinden
- **Engine tarafı**: `EngineMeter<T>` — wasmi "fuel" cinsinden

**Dönüşüm oranı** (`schedule.rs:67-69`):
```rust
pub fn ref_time_by_fuel(&self) -> u64 {
    self.instruction_weights.base as u64
}
```

`base` değeri benchmark'tan türetilir:
```rust
let instr_cost = T::WeightInfo::instr_i64_load_store(1)
    .saturating_sub(T::WeightInfo::instr_i64_load_store(0))
    .ref_time() as u32;
let base = instr_cost / 6;   // 6 farklı instruction ölçülüyor
```

**Gas senkronizasyonu** (`gas.rs:239-258`):
- `sync_from_executor(engine_fuel)` — host fn girişinde, engine'in tükettiği gaz aktarılır
- `sync_to_executor(before)` — host fn çıkışında, host'un tükettiği gaz engine'e geri verilir

**Nested gas** (`gas.rs:171-187`):
```rust
pub fn nested(&mut self, amount: Weight) -> Self {
    // amount=0 → "tüm kalan gas"
    let amount = ... .min(self.gas_left);
    self.gas_left -= amount;
    GasMeter::new(amount)
}
```

**OutOfGas davranışı**: Yeterli gas olmadığında işlem yapılmaz (checked_sub → Err), ücret tahsil edilmez.
**gas_required()**: Gerçek tüketimden bağımsız olarak minimum gerekli gas. `adjust_gas` sonrasında `gas_left_lowest` izlenir.

**Host function maliyetleri** (`wasm/runtime.rs:302-385`, `RuntimeCosts` enum):
- `HostFn`: base host function overhead
- `CopyFromContract(len)` / `CopyToContract(len)`: bellek kopyalama
- `SetStorage { old_bytes, new_bytes }`: write (worst-case ilk chargelanır, sonra adjust)
- `DepositEvent { num_topic, len }`: `WeightInfo + event_ref_time * len` (60,000 picosec/byte)
- `CallBase` / `DelegateCallBase` / `Instantiate { input_data_len, salt_len }`: cross-contract

### pallet-revive Gas Modeli
Kaynak: `metering/gas.rs`, `metering/weight.rs`

**Ethereum gaz uyumluluğu için `SignedGas<T>`**:
```rust
pub enum SignedGas<T: Config> {
    Positive(BalanceOf<T>),
    Negative(BalanceOf<T>),  // depo refund durumunda
}
```

**Dönüşüm zinciri**:
- Ethereum gas → `from_ethereum_gas()`: `gas * GasScale`
- Weight fee → `from_weight_fee()`
- Storage deposit → `from_adjusted_deposit_charge()`: `deposit / next_fee_multiplier`

**GasScale** (`Config::GasScale: Get<u32>`): Varsayılan 10. Gas fiyatını/tahminini ölçekler. Ethereum toolchain'lerin hardcoded gas limitlerinden etkilenmemek için.

**PolkaVM fuel** (`metering/weight.rs:67-74`):
```rust
fn ref_time_per_fuel() -> u64 {
    let loop_iteration = T::WeightInfo::instr(1) - T::WeightInfo::instr(0);
    let empty_loop = T::WeightInfo::instr_empty_loop(1) - T::WeightInfo::instr_empty_loop(0);
    loop_iteration.saturating_sub(empty_loop)
}
```

---

## 4. Host Functions (seal_* API)

### pallet-contracts seal_* API

**111 stable API** (`lib.rs:235` kullanır, `ApiVersion = 4`) — DIKKAT: `STABLE_API_COUNT` hardcoded degil, `proc-macro/src/lib.rs:533`'te `pub const STABLE_API_COUNT: usize = #stable_api_count;` ile compile-time'da stable host function sayimi yapilarak uretilir. Mevcut derlemede 111 olarak sonuclanir ama yeni stable API eklendikce otomatik degisir.

Temel host functions (`wasm/runtime.rs`, `RuntimeCosts` enum'dan çıkarılan):

| Host Function | Parametre | Açıklama |
|---|---|---|
| `seal_caller` | — | Çağıran hesap |
| `seal_address` | — | Mevcut kontrat adresi |
| `seal_balance` | — | Mevcut kontrat bakiyesi |
| `seal_value_transferred` | — | Bu çağrıda transfer edilen değer |
| `seal_gas_left` | — | Kalan gas |
| `seal_block_number` | — | Mevcut blok numarası |
| `seal_now` | — | Zaman damgası |
| `seal_minimum_balance` | — | Existential deposit |
| `seal_weight_to_fee` | weight | Weight → fee dönüşümü |
| `seal_caller_is_origin` | — | Caller == origin? |
| `seal_caller_is_root` | — | Root origin mi? |
| `seal_is_contract` | address | Adres kontrat mı? |
| `seal_code_hash` | address | Adresin kod hash'i |
| `seal_own_code_hash` | — | Kendi kod hash'i |
| `seal_set_storage` | key, value | Storage yazma |
| `seal_get_storage` | key | Storage okuma |
| `seal_clear_storage` | key | Storage silme |
| `seal_contains_storage` | key | Storage var mı? |
| `seal_take_storage` | key | Storage al (sil+döndür) |
| `seal_set_transient_storage` | key, value | Geçici storage yaz |
| `seal_get_transient_storage` | key | Geçici storage oku |
| `seal_clear_transient_storage` | key | Geçici storage sil |
| `seal_contains_transient_storage` | key | Geçici storage var mı? |
| `seal_take_transient_storage` | key | Geçici storage al |
| `seal_deposit_event` | topics, data | Event emit |
| `seal_call` | callee, gas, deposit, value, input, out | Cross-contract çağrı |
| `seal_delegate_call` | code_hash, input, out | Delegate çağrı |
| `seal_instantiate` | code_hash, gas, deposit, value, input, salt, out | Yeni kontrat |
| `seal_terminate` | beneficiary | Kontratı yok et |
| `seal_transfer` | to, value | Token transfer |
| `seal_return` | flags, data | Kontrat çıkışı |
| `seal_hash_sha2_256` | input, out | SHA256 |
| `seal_hash_keccak_256` | input, out | Keccak256 |
| `seal_hash_blake2_256` | input, out | Blake2-256 |
| `seal_hash_blake2_128` | input, out | Blake2-128 |
| `seal_ecdsa_recover` | sig, msg_hash, out | ECDSA kurtarma |
| `seal_ecdsa_to_eth_address` | pk, out | ECDSA → Ethereum adresi |
| `seal_sr25519_verify` | sig, message, pk | Sr25519 doğrulama |
| `seal_call_chain_extension` | id, input, out | Chain extension |
| `seal_call_runtime` | call | Runtime dispatch |
| `seal_xcm_execute` | msg, weight | XCM yürütme |
| `seal_xcm_send` | dest, msg | XCM gönderme |
| `seal_random` | subject, out | Rastgele sayı (deprecated) |
| `seal_debug_message` | msg | Debug mesajı (off-chain only) |
| `seal_set_code_hash` | hash | Kod yükseltme |
| `seal_reentrance_count` | — | Reentrance sayısı |
| `seal_account_reentrance_count` | account | Hesap reentrance sayısı |
| `seal_instantiation_nonce` | — | Kontrat nonce |
| `lock_delegate_dependency` | code_hash | Bağımlılık kilitle |
| `unlock_delegate_dependency` | code_hash | Bağımlılık serbest bırak |

**Kritik `seal_call` imzası** (`wasm/runtime.rs:983-1067`):
```rust
fn call(
    memory: &mut [u8],
    flags: CallFlags,        // ALLOW_REENTRY | FORWARD_INPUT | CLONE_INPUT | TAIL_CALL | READ_ONLY
    call_type: CallType,     // Call { callee_ptr, value_ptr, deposit_ptr, weight }
    input_data_ptr: u32,
    input_data_len: u32,
    output_ptr: u32,
    output_len_ptr: u32,
) -> Result<ReturnErrorCode, TrapReason>
```

**`seal_instantiate` imzası** (`wasm/runtime.rs:1069-1130`):
```rust
fn instantiate(
    memory: &mut [u8],
    code_hash_ptr: u32,
    weight: Weight,
    deposit_ptr: u32,       // SENTINEL = 0 deposit limit
    value_ptr: u32,
    input_data_ptr: u32, input_data_len: u32,
    address_ptr: u32, address_len_ptr: u32,  // çıktı
    output_ptr: u32, output_len_ptr: u32,    // çıktı
    salt_ptr: u32, salt_len: u32,
) -> Result<ReturnErrorCode, TrapReason>
```

### pallet-revive Syscall API

Syscall tabanlı yaklaşım (host functions değil). PolkaVM için `ecalli` (external call instruction). EVM için `revm` precompile/opcode map.

Temel farklar:
- Adresler `H160` (pallet-contracts'ta `AccountId`)
- Cross-contract çağrı `CallResources` enum ile (NoLimits / WeightDeposit / Ethereum gas)
- `seal_call` → `call(call_resources, to: &H160, value: U256, input, reentrancy, read_only)`
- `seal_instantiate` → `instantiate(limits, code: Code, value, input, salt: Option<&[u8;32]>)`
- Immutable data desteği (`get_immutable_data`, `set_immutable_data`) — constructor/call ayrımı

---

## 5. Storage Modeli

### pallet-contracts Storage

**Child Trie yapısı** (`storage.rs:50-110`):
```rust
pub struct ContractInfo<T: Config> {
    pub trie_id: TrieId,         // hash(account, nonce)
    pub code_hash: CodeHash<T>,
    storage_bytes: u32,          // birikmiş byte sayısı
    storage_items: u32,          // item sayısı
    pub storage_byte_deposit: BalanceOf<T>,
    storage_item_deposit: BalanceOf<T>,
    storage_base_deposit: BalanceOf<T>,  // kontrat varlığının deposu (ayrı izlenir)
    delegate_dependencies: BoundedBTreeMap<CodeHash<T>, BalanceOf<T>, T::MaxDelegateDependencies>,
}
```

**Key tipleri** (`exec.rs:68-98`):
- `Key::Fix([u8; 32])` → `blake2_256(key)` ile hashlenir
- `Key::Var(BoundedVec<u8, MaxStorageKeyLen>)` → `Blake2_128Concat::hash(key)`

**Storage okuma/yazma** (`storage.rs:141-231`):
- `ContractInfo::read(key)` → `child::get_raw(&trie_info, key.hash())`
- `ContractInfo::write(key, value, storage_meter, take)` — Diff hesaplar, storage_meter'a bildirir

**Depo hesaplama** (`storage/meter.rs:151-215`):
```rust
pub fn update_contract<T: Config>(&self, info: Option<&mut ContractInfo<T>>) -> DepositOf<T> {
    let per_byte = T::DepositPerByte::get();
    let per_item = T::DepositPerItem::get();
    // bytes_deposit = per_byte * bytes_added
    // Refund hesaplaması pro rata:
    let ratio = FixedU128::checked_from_rational(bytes_removed, info.storage_bytes);
    // refund = ratio * info.storage_byte_deposit
}
```

**Depo ödeme sırası** (`storage/meter.rs:395-408`): Önce refund'lar, sonra charge'lar. Bu sayede net ödeme yapılır.

**Transient Storage** (`transient_storage.rs`):
- `StorageMeter::STORAGE_FRACTION_DENOMINATOR = 16`
- Her sub-frame en fazla `(1 - 1/16)` = 15/16 kalan transient storage kullanabilir
- `BTreeMap` tabanlı in-memory, transaction rollback destekli

### pallet-revive Storage

pallet-contracts ile aynı temel yapı ama ek alanlar:

```rust
pub struct ContractInfo<T: Config> {
    pub trie_id: TrieId,
    pub code_hash: sp_core::H256,  // pallet-contracts'tan farklı: H256
    pub storage_bytes: u32,
    pub storage_items: u32,
    pub storage_byte_deposit: BalanceOf<T>,
    pub storage_item_deposit: BalanceOf<T>,
    pub storage_base_deposit: BalanceOf<T>,
    pub immutable_data_len: u32,   // YENİ: immutable data boyutu
}
```

**AccountInfo/AccountType ayrımı** (`storage.rs:58-84`):
```rust
pub struct AccountInfo<T: Config> {
    pub account_type: AccountType<T>,
    pub dust: u32,   // NativeToEthRatio'dan küçük native bakiye artığı
}

pub enum AccountType<T: Config> {
    Contract(ContractInfo<T>),
    EOA,
}
```

**Immutable Data** (`ImmutableDataOf: StorageMap<_, Identity, H160, ImmutableData>`): Constructor'da set edilir, call'larda okunabilir. Maksimum 4 KB (`limits::IMMUTABLE_BYTES = 4096`).

**NativeDepositOf**: `StorageDoubleMap<holder, contributor, amount>` — kimin ne kadar depo koyduğunu izler.

---

## 6. Reentrance ve Güvenlik

### pallet-contracts Reentrance

**Varsayılan davranış**: Reentrance YASAK (`allows_reentry: bool = true` ama `ALLOW_REENTRY` flag gerekir).

**Frame yapısı** (`exec.rs:517-536`):
```rust
pub struct Frame<T: Config> {
    allows_reentry: bool,   // false ise bu kontrata tekrar giriş yasak
    read_only: bool,        // true ise storage yazma yasak
    ...
}
```

**Reentrance kontrolü** (exec.rs terminate fonksiyonu):
```rust
fn terminate(&mut self, beneficiary: &AccountIdOf<Self::T>) -> DispatchResult {
    // ...
    // "contract is present on the call stack" kontrolü
    // Hata: Kontrat call stack'te varken terminate edilemez
}
```

**`seal_call` `ALLOW_REENTRY` flag** (`wasm/runtime.rs:1032`):
```rust
self.ext.call(
    weight, deposit_limit, callee, value, input_data,
    flags.contains(CallFlags::ALLOW_REENTRY),  // reentrance izni
    read_only,
)
```

**`READ_ONLY` flag**: Storage değişiklikleri yasak. Value transfer varken read_only çağrı → `Error::StateChangeDenied` (`wasm/runtime.rs:1021-1023`).

**Reentrancy count API** (`exec.rs:344-349`):
- `reentrance_count()` — mevcut kontratın stack'teki tekrar sayısı
- `account_reentrance_count(account_id)` — herhangi bir hesabın stack'teki sayısı

**Call stack depth** (`exec.rs:918`):
```rust
if self.frames.len() == T::CallStack::size() {
    return Err(Error::<T>::MaxCallDepthReached.into());
}
```

**Transaction isolation** (`exec.rs:1063-1072`):
```rust
let transaction_outcome = with_transaction(|| {
    let output = do_transaction();
    match output {
        Ok(result) if !result.did_revert() => TransactionOutcome::Commit(...),
        _ => TransactionOutcome::Rollback(...),
    }
});
```
Her frame kendi `with_transaction` bloğunda çalışır. Revert durumunda storage değişiklikleri geri alınır.

### pallet-revive Reentrance

**Üç seviyeli kontrol** (`exec.rs:127-142`):
```rust
pub enum ReentrancyProtection {
    AllowReentry,   // reentrance serbest
    Strict,         // callee ve callee'nin calleeları bu kontratı çağıramaz
    AllowNext,      // callee direkt çağırabilir ama recursive olarak çağıramaz
                    // (Ethereum'un gas stipend korumasına benzer)
}
```

`AllowNext` semantiği: Ethereum'daki gas stipend (2300 gas) yerine explicit reentrancy koruması. Neden? Çünkü farklı GasScale değerlerinde gas stipend garantisi sağlanamaz.

---

## 7. pallet-revive (EVM Uyumluluğu)

### PolkaVM Bytecode Türü

**revive compiler**: Solidity → LLVM IR → PolkaVM bytecode. Polkavm `ReviveV1` ISA kullanır (64-bit).

**Doğrulama** (`limits/code.rs:136-274`):
- `BLOB_BYTES = 1 MB` limit kontrol
- `ReviveV1` ISA zorunluluğu
- `sbrk` instruction yasak
- Bilinmeyen syscall import yasak
- Max basic block: `BASIC_BLOCK_SIZE = 1000` instruction
- Purgeable RAM: `PURGABLE_MEMORY_LIMIT = INTERPRETER_CACHE_BYTES + 2MB = 3MB`
- Baseline RAM: `BASELINE_MEMORY_LIMIT = BLOB_BYTES + 512KB = 1.5MB`

### EVM Bytecode Türü

`AllowEVMBytecode: Get<bool>` config parametresi ile kontrol edilir. Varsayılan test: `true`.

`revm` kütüphanesi ile EVM bytecode yorumlama. Standart EVM opcodes ve precompile'lar desteklenir:

**Desteklenen precompile'lar** (`precompiles/builtin/`):
- `ecrecover` (0x01)
- `sha256` (0x02)
- `ripemd160` (0x03)
- `identity` (0x04)
- `modexp` (0x05)
- `bn128_add` (0x06), `bn128_mul` (0x07), `bn128_pairing` (0x08)
- `blake2f` (0x09)
- `point_eval` (0x0A) — EIP-4844
- `p256_verify` — EIP-7212
- `system` — pallet-revive özel (storage, terminate, vb.)

### Ethereum Adres Uyumluluğu

**AddressMapper** (`address.rs`):
- `AccountId32Mapper`: `AccountId32` → `H160` (keccak dönüşümü, geri dönüşüm için `OriginalAccount` map'i)
- `H160Mapper`: `H160` native (Frontier benzeri chainler)
- `AutoMapper`: `AutoMap = true` ise otomatik hesap eşleme

**create1/create2** (`address.rs`):
- `create1(address, nonce)` → RLP(address, nonce) → keccak
- `create2(address, salt, init_code_hash)` → keccak(0xff ++ address ++ salt ++ hash)

### EVM İşlem Desteği

`eth_transact` dispatchable (`lib.rs`): Ethereum formatında imzalı işlemleri yürütür.

`EthereumBlock` storage: Her blok sonunda inşa edilir, RPC uyumluluğu için.

**Gas ↔ Weight dönüşümü** (`metering/gas.rs`):
```rust
// Gas → SignedGas (internal)
SignedGas::from_ethereum_gas(gas) → gas * GasScale

// SignedGas → Ethereum gas (external)
amount.to_ethereum_gas() → (amount + GasScale - 1) / GasScale  // ceiling division
```

**Depo ↔ Gas dönüşümü**:
```rust
// Deposit Charge → Gas
from_adjusted_deposit_charge(Charge(amount)) → amount / next_fee_multiplier
// refund negatif gas olur
from_adjusted_deposit_charge(Refund(amount)) → Negative(amount / next_fee_multiplier)
```

---

## 8. Kritik Sayısal Limitler ve Sabitler

### pallet-contracts Limitleri (Schedule::Limits varsayılanları)

Kaynak: `schedule.rs:124-136`

| Parametre | Değer | Açıklama |
|---|---|---|
| `event_topics` | **4** | Max event topic sayısı |
| `memory_pages` | **16** | Max memory page (16 × 64KB = **1 MB**) |
| `subject_len` | **32** | PRNG subject max uzunluğu |
| `payload_len` | **16 × 1024 = 16 KB** | Max storage value + event payload |
| `runtime_memory` | **128 MB** | Node runtime bellek (integrity check) |
| `validator_runtime_memory` | **512 MB** | Validator bellek (integrity check) |
| `event_ref_time` | **60_000 picosec/byte** | Event başına ek weight (≈ 32 MB/block limit) |

### pallet-contracts Config Limitleri

| Config | Açıklama |
|---|---|
| `MaxCodeLen: Get<u32>` | Max Wasm kod boyutu |
| `MaxStorageKeyLen: Get<u32>` | Max variable storage key uzunluğu |
| `MaxTransientStorageSize: Get<u32>` | Max transient storage toplam boyutu |
| `MaxDelegateDependencies: Get<u32>` | Max delegate_dependency kilit sayısı |
| `MaxDebugBufferLen: Get<u32>` | Max debug buffer uzunluğu |
| `CodeHashLockupDepositPercent: Get<Perbill>` | Kod kilit depozu yüzdesi |
| `DepositPerByte: Get<BalanceOf<T>>` | Byte başına depo |
| `DepositPerItem: Get<BalanceOf<T>>` | Item başına depo |
| `DefaultDepositLimit: Get<BalanceOf<T>>` | Varsayılan depo limiti |

**Call stack depth**: `T::CallStack::size() + 1` — örneğin `[Frame; 5]` → 6 derinlik

**Wasm kısıtlamaları** (`wasm/prepare.rs:79-92`):
- `wasm_multi_value: false`
- `wasm_mutable_global: false`
- `wasm_bulk_memory: false`
- `wasm_reference_types: false`
- `wasm_tail_call: false`
- `wasm_extended_const: false`
- `wasm_saturating_float_to_int: false`
- Float: yalnızca `Determinism::Relaxed` modunda (off-chain only)
- Required exports: `call` ve `deploy` (başka export yasak)
- Required import: `env.memory` (başka memory import yasak)
- Stack limits: validation'da minimal (1,1,0)

**Wasm decode nesting**: `MAX_DECODE_NESTING = 256`

### pallet-revive Limitleri

Kaynak: `limits.rs`

| Sabit | Değer | Açıklama |
|---|---|---|
| `CALL_STACK_DEPTH` | **25** | Max çağrı derinliği |
| `NUM_EVENT_TOPICS` | **4** | Max event topic (Ethereum ile aynı) |
| `STORAGE_BYTES` | **416** | Max storage item boyutu |
| `EVENT_BYTES` | **64 × 1024 = 64 KB** | Max event payload |
| `EXTRA_EVENT_CHARGE_PER_BYTE` | **256 × 1024 picosec/byte** | Event başına ek weight |
| `CALLDATA_BYTES` | **128 × 1024 = 128 KB** | Max calldata ve return data |
| `TRANSIENT_STORAGE_BYTES` | **4 × 1024 = 4 KB** | Max toplam transient storage |
| `STORAGE_KEY_BYTES` | **128** | Max storage key uzunluğu |
| `PAGE_SIZE` | **4 × 1024 = 4 KB** | PolkaVM bellek sayfa boyutu |
| `IMMUTABLE_BYTES` | **4 × 1024 = 4 KB** | Max immutable data (16 U256) |
| `EVM_MEMORY_BYTES` | **1 MB** | EVM interpreter max bellek |
| `EVM_STACK_LIMIT` | **1024** | EVM interpreter stack derinliği |
| `code::BLOB_BYTES` | **1 MB** | Max PolkaVM/EVM kod blob |
| `code::INTERPRETER_CACHE_BYTES` | **1 MB** | Max interpreter cache |
| `code::BASIC_BLOCK_SIZE` | **1000** | Max temel blok instruction sayısı |
| `code::PURGABLE_MEMORY_LIMIT` | **3 MB** | Max tasfiye edilebilir bellek |
| `code::BASELINE_MEMORY_LIMIT` | **1.5 MB** | Max kalıcı frame bellek |

**MEMORY_REQUIRED hesabı** (`limits.rs:280-297`):
```rust
const fn memory_required() -> u32 {
    let max_call_depth = CALL_STACK_DEPTH + 1;  // 26
    let per_stack_memory = PURGABLE_MEMORY_LIMIT + TRANSIENT_STORAGE_BYTES * 2 + MAX_ACCESS_LIST_BYTES;
    let per_frame_memory = max(BASELINE_MEMORY_LIMIT, evm_overhead) + CALLDATA_BYTES * 2;
    per_stack_memory + max_call_depth * per_frame_memory
}
```

**pallet-revive Config Değerleri** (`config_preludes::TestDefaultConfig`):
```
DepositPerItem: deposit(1, 0) = 20 CENTS
DepositPerChildTrieItem: deposit(1, 0) / 100 = 0.2 CENTS
DepositPerByte: deposit(0, 1) = 1 MILLICENT
CodeHashLockupDepositPercent: 0%
GasScale: 10
NativeToEthRatio: 1_000_000
ChainId: 42
RuntimeMemory: 128 MB
PVFMemory: 512 MB
MaxEthExtrinsicWeight: 9/10 (0.9)
```

### Depo Formülleri

**pallet-contracts** (`storage/meter.rs:160-215`):
```
deposit_charge = DepositPerByte * bytes_added + DepositPerItem * items_added
refund = (bytes_removed / storage_bytes) * storage_byte_deposit  [pro rata]
```

**Base deposit** (`storage.rs:237-250`):
```
base_deposit = cost(ContractInfo.encoded_size() bytes + 1 item)
             + CodeHashLockupDepositPercent * code_info.deposit
```

**Kod upload deposu** (`wasm/prepare.rs:341-348`):
```
code_deposit = DepositPerByte * (code_len + CodeInfo::max_encoded_len())
             + DepositPerItem * 2  (PristineCode + CodeInfoOf)
```

---

## 9. Test Analizi

### pallet-contracts Test Coverage

**gas.rs test'leri**:
- `it_works`: `GasMeter::new(50000).gas_left() == 50000`
- `tracing`: Token izleme doğrulama
- `refuse_to_execute_anything_if_zero`: 0 gas → her charge reddedilir
- `overcharge_does_not_charge`: Hatalı charge gas'ı tüketmez
- `charge_exact_amount`: Tam limite kadar charge mümkün

**Fixture contracts** (test için derlenen Ink! kontratları):
- `call.rs`, `delegate_call.rs`, `delegate_call_lib.rs` — cross-contract ve delegate call
- `recurse.rs` — özyinelemeli çağrı (stack depth testi)
- `self_destruct.rs`, `destroy_and_transfer.rs` — terminate testi
- `reentrance_count_call.rs`, `account_reentrance_count_call.rs` — reentrance sayacı
- `create_storage_and_call.rs`, `store_call.rs` — storage depo testi
- `create_transient_storage_and_call.rs` — transient storage
- `chain_extension.rs`, `chain_extension_temp_storage.rs` — chain extension
- `xcm_execute.rs`, `xcm_send.rs` — XCM entegrasyonu
- `float_instruction.rs` — float yasağı testi
- `run_out_of_gas.rs` — gas tükenmesi testi
- `crypto_hashes.rs`, `ecdsa_recover.rs`, `sr25519_verify.rs` — kripto test
- `set_code_hash.rs` — kod yükseltme
- `locking_delegate_dependency.rs` — delegate dependency kilitleme
- `read_only_call.rs` — read-only kontrol

**mock-network**: Parachain ortamında XCM entegrasyon testleri

### pallet-revive Test Coverage

**Fixture contracts**: pallet-contracts fixture'larına paralel set + EVM spesifik:
- `gas_left.rs`, `gas_price.rs`, `gas_limit.rs`, `gas_price_n.rs` — gas API testleri
- `base_fee.rs`, `chain_id.rs` — Ethereum compat
- `block_hash.rs`, `block_author.rs` — blok bilgisi
- `extcodesize.rs` — EVM EXTCODESIZE opcode
- `immutable_data.rs`, `immutable_reader.rs` — immutable data
- `create1_with_value.rs`, `create2_with_value.rs` — CREATE/CREATE2
- `delegate_call_evm.rs`, `delegate_call_deposit_limit.rs` — EVM delegate
- `oom_ro.rs`, `oom_rw_trailing.rs`, `oom_rw_included.rs` — bellek sınırı testleri
- `self_destruct_by_precompile.rs`, `self_destructing_constructor_by_precompile.rs` — selfdestruct
- `tracing.rs`, `tracing_callee.rs` — execution tracing
- `rpc_demo.rs` — RPC uyumluluğu

**tests/sol.rs**: Solidity tabanlı entegrasyon testleri (arithmetic.rs vb.)

---

## 10. Nova Shield Chain İçin Çıkarsamalar

### A. Hangi Pallet Seçilmeli?

| Kriter | pallet-contracts | pallet-revive |
|---|---|---|
| Dil desteği | Ink! (Rust) | Solidity/EVM + Ink! (gelecekte) |
| Bytecode | Wasm (wasmi) | PolkaVM + EVM (revm) |
| Ethereum uyumu | Kısmi | Tam (EIP-1559, eth_transact) |
| Olgunluk | Üretim-kanıtlı | Aktif geliştirme (v0) |
| Toolchain | cargo-contract | revive-cc + Foundry |
| Storage_version | 16 (mature) | 0 (erken) |

**Öneri**: Nova Shield Chain MPC/ZK tabanlı ve DeFi odaklıysa, Ethereum toolchain ekosistemi kritiktir. pallet-revive seçin ama dikkat: `STORAGE_VERSION = 0`, hâlâ aktif geliştirme.

### B. Gas Modeli Tasarım Notları

**pallet-contracts için**:
```
weight_to_gas_ratio = ref_time_by_fuel = instr_cost / 6
```
Kendi benchmark'larınızı çalıştırmadan bu değeri varsayılan tutun.

**pallet-revive için**:
```
GasScale = 10 (varsayılan)
NativeToEthRatio = 1_000_000 (varsayılan)
```
Bu değerler birlikte chain'in "wei" değerini belirler. Yüksek NativeToEthRatio → çok küçük wei değerleri, araç uyumsuzluğu riski.

### C. Reentrance Koruması Yaklaşımı

**Güvenli varsayılan**: pallet-contracts'ta `ALLOW_REENTRY` flag vermeden çağrı = reentrance yasak. DEX ve AMM kontratları için güçlü koruma.

**pallet-revive'da `AllowNext`**: Ethereum gas stipend semantiğini emüle eder ama explicit. Eğer chain'de DeFi varsa, protokol seviyesinde reentrancy koruma standartı belirleyin.

### D. Storage Deposit Tasarımı

**DepositPerByte** seçimi kritik. Çok düşük → spam depo saldırısı. Çok yüksek → geliştiriciler kontrat optimize etmek zorunda.

**Önerilen**: Polkadot Relay Chain değerlerini referans alın:
```
DepositPerByte = 1 MILLICENT
DepositPerItem = 20 CENTS  (pallet-contracts)
DepositPerChildTrieItem = 0.2 CENTS  (pallet-revive — alt trie daha ucuz)
```

**CodeHashLockupDepositPercent**: pallet-revive varsayılan `0%`. Kod kullanımdan kaldırılamayacağından bu değer güvenlidir ama aynı kodu kullanan tüm kontratlara karşı zayıf bir spam koruması sağlar. `%30-50` düşünülebilir.

### E. Call Stack Depth

pallet-contracts: `T::CallStack` = `[Frame; N]`, N yapılandırılabilir.
pallet-revive: `CALL_STACK_DEPTH = 25` (sabit constant, değiştirilemez).

DEX gibi çok katmanlı protokoller için yeterli. Ancak çok karmaşık DeFi composability 25 derinliği tüketebilir.

### F. Transient Storage (EIP-1153 Eşdeğeri)

pallet-contracts: `MaxTransientStorageSize` configurable, varsayılan yok (custom).
pallet-revive: `TRANSIENT_STORAGE_BYTES = 4 KB` (sabit).

DeFi protokollerinde within-tx reentrancy lock için kullanışlı. Örneğin:
```
TSTORE(lock_slot, 1)   // lock
// işlem...
TSTORE(lock_slot, 0)   // unlock
```

### G. Lazy Deletion Mekanizması

Her iki pallet de `DeletionQueue` kullanır. `on_idle` hook'unda weight budget dahilinde trie'lar temizlenir.

pallet-contracts (`storage.rs:316-352`): `process_deletion_queue_batch(meter)` — per-key weight ile budget hesaplanır.

pallet-revive: Ayrıca `NativeDepositOf` girdileri de temizlenmesi gerekir (iki kaynak).

**Nova Shield için**: `on_idle` yetersiz kalabilir. Yoğun terminate periyotlarında deletion backlog birikebilir. `MaxDeletionPerBlock` benzeri explicit bir limit eklenebilir.

### H. Kritik Güvenlik Notları

1. **Constructor'da terminate yasak**: Her iki pallet de `TerminatedInConstructor` hatasıyla bunu önler.

2. **Cyclic delegate bağımlılıkları**: `MaxDelegateDependencies` limiti ve `CannotAddSelfAsDelegateDependency` kontrolü mevcut.

3. **Wasm float yasağı**: `Determinism::Enforced` modunda float instruction yasak. On-chain kontratlar non-deterministic olmamalı.

4. **`seal_call_runtime` filter**: `T::CallFilter` ile kontrol edilir. DoS vektörü önlemi için whitelist kullanın.

5. **pallet-revive `sbrk` yasağı**: PolkaVM bytecode'da `sbrk` illegal. polkavm >= 0.30.0 ile linker zaten reddeder.

6. **EVM bytecode: `AllowEVMBytecode`**: Üretimde dikkatli değerlendirin. EVM bytecode'lar revive compiler'dan geçmemiş olabilir ve bilinmeyen vektörler açabilir.

### I. Ink! vs Solidity Öneri

Nova Shield Chain MPC ve threshold signature odaklı ise:
- **MPC koordinasyon kontratları**: Ink! (pallet-contracts) — daha iyi `#[no_std]` desteği
- **DeFi/DEX kontratları**: Solidity/revive (pallet-revive) — geniş Ethereum ekosistemi

İki pallet aynı runtime'da çalışabilir (farklı pallet slot'larında).

### J. Önemli Tasarım Değerleri Özeti

```
[pallet-contracts]
Max memory pages:          16 (= 1 MB)
Max payload size:          16 KB
Max event topics:          4
event_ref_time charge:     60,000 picosec/byte
Call stack depth:          T::CallStack::size() + 1 (örn. 5+1=6)
Code kod depozu:           DepositPerByte * (code_len + CodeInfo_len) + DepositPerItem * 2
Base contract depozu:      info_deposit + CodeHashLockupPercent * code_deposit
Storage refund:            pro rata (bytes_removed / total_bytes * total_deposit)
Wasm export gereksinim:    "call" ve "deploy" (diğer export yasak)
MAX_DECODE_NESTING:        256
Determinism::Enforced:     float yasak

[pallet-revive]
Call stack depth:          25 (sabit)
Max event topics:          4
Max event payload:         64 KB
Max calldata/returndata:   128 KB
Max transient storage:     4 KB
Max storage item:          416 byte
Max immutable data:        4 KB (16 U256)
Max blob size:             1 MB
Max basic block:           1000 instructions
GasScale (default):        10
NativeToEthRatio (default): 1_000_000
Ethereum gas model:        EIP-1559 uyumlu
Address format:            H160 (Ethereum native)
```
