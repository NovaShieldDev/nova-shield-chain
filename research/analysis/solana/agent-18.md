# Solana Program Runtime Analizi — Agent 18

**Analiz Tarihi:** 2026-07-22
**Kapsam:** program-runtime/src/ (12 dosya)
**Toplam Satır:** ~3870 (invoke_context.rs: 1202, loaded_programs.rs: 2668, diger: ~1000)

---

## Dizin Yapisi

```
program-runtime/src/
├── lib.rs
├── invoke_context.rs        (1202 satir) — CPI motoru, VM baglami
├── loaded_programs.rs       (2668 satir) — Program binary cache
├── compute_budget.rs        (221 satir)  — CU limit tanimlari
├── compute_budget_processor.rs (500 satir) — TX-level budget isleme
├── sysvar_cache.rs          (315 satir)  — Runtime sysvar cache
├── timings.rs               (552 satir)  — Execution metrikleri
├── log_collector.rs         (123 satir)  — Program log sistemi
├── stable_log.rs            (111 satir)  — Stable log format
├── message_processor.rs     (690 satir)  — TX mesaj isleyici
├── runtime_config.rs        (17 satir)   — Runtime konfigurasyonu
└── prioritization_fee.rs    (115 satir)  — Oncelik ucreti hesabi
```

---

## 1. lib.rs — Satir 1-24

### Ozellikler
- Satir 1: `#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]`
- Satir 2: `#![deny(clippy::arithmetic_side_effects)]` — Tasma kontrolu zorunlu
- Satir 3: `#![deny(clippy::indexing_slicing)]` — Guvensiz index erisimi yasak
- Satir 4: `#![recursion_limit = "2048"]`
- Satir 6-11: `extern crate eager` ve `extern crate solana_metrics` — macro sistemi

### Moduller (Satir 13-24)
```rust
pub use solana_rbpf;                   // re-export: sBPF VM
pub mod compute_budget;                // CU limit struct
pub mod compute_budget_processor;      // TX budget isleme
pub mod invoke_context;                // CPI baglami
pub mod loaded_programs;               // Program cache
pub mod log_collector;                 // Log toplayici
pub mod message_processor;            // Mesaj isleyici
pub mod prioritization_fee;           // Oncelik ucreti
pub mod runtime_config;               // Runtime konfigu
pub mod stable_log;                   // Stable log API
pub mod sysvar_cache;                 // Sysvar cache
pub mod timings;                      // Zamanlama metrikleri
```

**Nova Shield icin Not:** `arithmetic_side_effects` deny direktifi kritik — her matematiksel islemde tasma kontrolu zorunlu.

---

## 2. invoke_context.rs — Satir 1-1202

Bu dosya Solana'nin program calistirma motorunun kalbidir.

### Import'lar ve Tip Aliasi (Satir 1-42)

```rust
// Satir 42
pub type BuiltinFunctionWithContext = BuiltinFunction<InvokeContext<'static>>;
```

Builtin program entrypoint'leri icin tip aliasi. `InvokeContext<'static>` lifetime'i runtime boyunca gecerli.

### declare_process_instruction! Makrosu (Satir 45-80)

```rust
// Satir 46-80
macro_rules! declare_process_instruction {
    ($process_instruction:ident, $cu_to_consume:expr, |$invoke_context:ident| $inner:tt) => {
        $crate::solana_rbpf::declare_builtin_function!(
            $process_instruction,
            fn rust(
                invoke_context: &mut InvokeContext,
                _arg0: u64, _arg1: u64, _arg2: u64, _arg3: u64, _arg4: u64,
                _memory_mapping: &mut MemoryMapping,
            ) -> Result<u64, Box<dyn std::error::Error>> {
                fn process_instruction_inner(
                    $invoke_context: &mut InvokeContext,
                ) -> Result<(), InstructionError> {
                    $inner
                }
                let consumption = u64::from($cu_to_consume);
                invoke_context.consume_checked(consumption)?;
                process_instruction_inner(invoke_context)
                    .map(|_| 0)
                    .map_err(...)
            }
        );
    };
}
```

**Kritik Tasarim:** Her builtin program macro kullanarak tanimlanir. Makro otomatik olarak:
1. Belirtilen `$cu_to_consume` kadar CU tuketir
2. `InvokeContext` referansini inject eder
3. `InstructionError` -> `Box<dyn Error>` donusumunu yapar

### ContextObject Implementation (Satir 83-103)

`InvokeContext` artik `solana_rbpf::vm::ContextObject` trait'ini implemente eder:

```rust
// Satir 83-91: trace() — SBPF trace kaydi
fn trace(&mut self, state: [u64; 12]) {
    self.syscall_context
        .last_mut().unwrap()
        .as_mut().unwrap()
        .trace_log.push(state);
}

// Satir 93-98: consume() — CU tuketen SBPF VM callback
fn consume(&mut self, amount: u64) {
    // 1:1 instruction -> compute unit mapping
    // overflow ignore edilir, eBPF exceed durumunda kendisi durur
    let mut compute_meter = self.compute_meter.borrow_mut();
    *compute_meter = compute_meter.saturating_sub(amount);
}

// Satir 100-102: get_remaining() — kalan CU
fn get_remaining(&self) -> u64 {
    *self.compute_meter.borrow()
}
```

**Kritik Tasarim:** `consume()` `saturating_sub` kullanir, ancak yorum "ignore overflow" der — SBPF VM kendisi `get_remaining()` ile kontrol eder ve bails.

### AllocErr ve BpfAllocator (Satir 105-138)

```rust
// Satir 105-110
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AllocErr;

// Satir 113-116
pub struct BpfAllocator {
    len: u64,   // Heap'in toplam boyutu
    pos: u64,   // Mevcut pozisyon (bump allocator)
}
```

```rust
// Satir 123-138
pub fn alloc(&mut self, layout: Layout) -> Result<u64, AllocErr> {
    let bytes_to_align = (self.pos as *const u8).align_offset(layout.align()) as u64;
    if self.pos
        .saturating_add(bytes_to_align)
        .saturating_add(layout.size() as u64)
        <= self.len
    {
        self.pos = self.pos.saturating_add(bytes_to_align);
        let addr = MM_HEAP_START.saturating_add(self.pos);
        self.pos = self.pos.saturating_add(layout.size() as u64);
        Ok(addr)
    } else {
        Err(AllocErr)
    }
}
```

**Tasarim Karari:** Basit bump allocator. Heap once hesaplanir (MM_HEAP_START ofset), alignment destegi var. Serbest birakma YOK — program sonunda heap temizlenir.

### SyscallContext (Satir 141-145)

```rust
pub struct SyscallContext {
    pub allocator: BpfAllocator,                         // Heap bump allocator
    pub accounts_metadata: Vec<SerializedAccountMetadata>, // Serialize edilmis hesap metadata
    pub trace_log: Vec<[u64; 12]>,                       // SBPF trace kayitlari
}
```

Her syscall frame'i icin ayri bir `SyscallContext` tutulur. `syscall_context: Vec<Option<SyscallContext>>` stack olarak kullanilir.

### SerializedAccountMetadata (Satir 147-154)

```rust
#[derive(Debug, Clone)]
pub struct SerializedAccountMetadata {
    pub original_data_len: usize,  // Hesabin orijinal data boyutu
    pub vm_data_addr: u64,         // VM bellegindeki data adresi
    pub vm_key_addr: u64,          // VM bellegindeki pubkey adresi
    pub vm_lamports_addr: u64,     // VM bellegindeki lamport adresi
    pub vm_owner_addr: u64,        // VM bellegindeki owner adresi
}
```

Serialized edilmis hesap adreslerini tutar. BPF VM bellek haritalamasi icin kullanilir.

### InvokeContext<'a> Struct (Satir 156-171) — ANA YAPI

```rust
pub struct InvokeContext<'a> {
    // Satir 157: Transaction seviyesi hesap ve instruction context
    pub transaction_context: &'a mut TransactionContext,

    // Satir 158: Runtime sysvar cache'i
    sysvar_cache: &'a SysvarCache,

    // Satir 159: Program log toplayici (Rc<RefCell> = paylasilabilir, mutasyona acik)
    log_collector: Option<Rc<RefCell<LogCollector>>>,

    // Satir 160: Transaction'in genel compute budget'i
    compute_budget: ComputeBudget,

    // Satir 161: Mevcut instruction'in compute budget'i (CPI'da degisebilir)
    current_compute_budget: ComputeBudget,

    // Satir 162: Kalan compute unit sayisi (RefCell = ic mutabilite)
    compute_meter: RefCell<u64>,

    // Satir 163: TX batch icin yuklenmis programlar (immutable ref)
    pub programs_loaded_for_tx_batch: &'a LoadedProgramsForTxBatch,

    // Satir 164: TX tarafindan degistirilen programlar (mutable ref)
    pub programs_modified_by_tx: &'a mut LoadedProgramsForTxBatch,

    // Satir 165: Aktif feature set
    pub feature_set: Arc<FeatureSet>,

    // Satir 166: Execution zamanlama metrikleri
    pub timings: ExecuteDetailsTimings,

    // Satir 167: Mevcut block'un hash'i
    pub blockhash: Hash,

    // Satir 168: Imza basina lamport maliyeti
    pub lamports_per_signature: u64,

    // Satir 169: Syscall stack (Vec = invocation derinligi kadar buyur)
    pub syscall_context: Vec<Option<SyscallContext>>,

    // Satir 170: SBPF trace kayitlari (debug icin)
    traces: Vec<Vec<[u64; 12]>>,
}
```

**Kritik Gozlem:**
- `transaction_context` mutable ref olarak tutulur — tek sahip
- `programs_loaded_for_tx_batch` immutable — TX baslamadan once yuklenir
- `programs_modified_by_tx` mutable — TX sirasinda deploy edilen yeni programlar
- `compute_budget` vs `current_compute_budget`: iki farkli budget. `current_compute_budget` CPI sirasinda override edilebilir
- `syscall_context: Vec<Option<...>>` — stack olarak kullanilir, her `push()` None ekler, `set_syscall_context()` ile doldurulur

### InvokeContext::new() (Satir 175-202)

```rust
pub fn new(
    transaction_context: &'a mut TransactionContext,
    sysvar_cache: &'a SysvarCache,
    log_collector: Option<Rc<RefCell<LogCollector>>>,
    compute_budget: ComputeBudget,
    programs_loaded_for_tx_batch: &'a LoadedProgramsForTxBatch,
    programs_modified_by_tx: &'a mut LoadedProgramsForTxBatch,
    feature_set: Arc<FeatureSet>,
    blockhash: Hash,
    lamports_per_signature: u64,
) -> Self {
    Self {
        transaction_context,
        sysvar_cache,
        log_collector,
        current_compute_budget: compute_budget,  // baslangicta ayni
        compute_budget,
        compute_meter: RefCell::new(compute_budget.compute_unit_limit),  // maks CU ile basla
        // ...
        syscall_context: Vec::new(),
        traces: Vec::new(),
    }
}
```

Compute meter `compute_unit_limit` ile baslatilir.

### find_program_in_cache() (Satir 204-210)

```rust
pub fn find_program_in_cache(&self, pubkey: &Pubkey) -> Option<Arc<LoadedProgram>> {
    // Once TX tarafindan degistirilen programlara bak
    self.programs_modified_by_tx
        .find(pubkey)
        // Bulamazsan TX batch icin onceden yuklenmis programlara bak
        .or_else(|| self.programs_loaded_for_tx_batch.find(pubkey))
}
```

**Tasarim:** Iki katmanli lookup. `programs_modified_by_tx` (yeni deploy'lar) once kontrol edilir, sonra onceden yuklenmis programlar.

### push() — Invocation Stack Push (Satir 213-258)

```rust
pub fn push(&mut self) -> Result<(), InstructionError> {
    // Mevcut instruction context'ten program ID'sini al
    let instruction_context = self.transaction_context
        .get_instruction_context_at_index_in_trace(...)?;
    let program_id = instruction_context
        .get_last_program_key(self.transaction_context)
        .map_err(|_| InstructionError::UnsupportedProgramId)?;

    if self.transaction_context.get_instruction_context_stack_height() == 0 {
        // Top-level instruction: compute budget'i sifirla
        self.current_compute_budget = self.compute_budget;
    } else {
        // CPI: yeniden giris kontrolu
        let contains = (0..stack_height).any(|level| {
            // Stack'teki herhangi bir frame ayni program ID'sine sahip mi?
            // program_account.get_key() == program_id
        });
        let is_last = /* stack'in en ustundeki frame bu program mi? */;

        if contains && !is_last {
            // Ayni program stack'te var AMA en ustte degil -> REENTRANCE HATASI
            return Err(InstructionError::ReentrancyNotAllowed);
        }
    }

    // Syscall context stack'ine None ekle (sonra set_syscall_context ile doldurulacak)
    self.syscall_context.push(None);
    self.transaction_context.push()
}
```

**Kritik Kural:** Program yeniden giris (reentrancy) sadece programin kendisini cagirmasi durumunda izin verilir. Farkli bir program tarafindan cagrilirken stack'te olmak hataya yol acar.

### pop() — Invocation Stack Pop (Satir 261-266)

```rust
pub fn pop(&mut self) -> Result<(), InstructionError> {
    if let Some(Some(syscall_context)) = self.syscall_context.pop() {
        // Trace logu kaydet (debug icin)
        self.traces.push(syscall_context.trace_log);
    }
    self.transaction_context.pop()
}
```

Hem syscall stack'ini hem de transaction context stack'ini pop eder.

### get_stack_height() (Satir 270-273)

```rust
pub fn get_stack_height(&self) -> usize {
    self.transaction_context
        .get_instruction_context_stack_height()
}
```

Stack derinligini doner. `TRANSACTION_LEVEL_STACK_HEIGHT` = 1 (top level). CPI her katmanda arttirir.

### native_invoke() — Builtin CPI Entrypointi (Satir 276-292)

```rust
pub fn native_invoke(
    &mut self,
    instruction: StableInstruction,
    signers: &[Pubkey],    // Program tarafindan imzalanacak adresler (PDA'lar)
) -> Result<(), InstructionError> {
    let (instruction_accounts, program_indices) =
        self.prepare_instruction(&instruction, signers)?;
    let mut compute_units_consumed = 0;
    self.process_instruction(
        &instruction.data,
        &instruction_accounts,
        &program_indices,
        &mut compute_units_consumed,
        &mut ExecuteTimings::default(),
    )?;
    Ok(())
}
```

Builtin program CPI'si buradan gecer. `signers` PDA imzalari icin kullanilir.

### prepare_instruction() — CPI Hazirlik (Satir 296-415)

Bu fonksiyon CPI'nin guvenlik dogrulama kismini yapar:

```
Satir 296-415:
1. Instruction'daki her hesap icin index bul (transaction_context icinde)
2. Duplicate hesaplari tespit et ve privilege'lari birlestir (OR mantigi)
3. Guvenlik kontrolleri:
   a. "Caller'da readonly olan hesap callee'de writable olamaz"
      -> InstructionError::PrivilegeEscalation
   b. "Caller'da imzali olmayan hesap callee'de imzali olamaz
      (program'in PDA imzasi olmadigi surece)"
      -> InstructionError::PrivilegeEscalation
4. Program hesabinin executable oldugunu dogrula
   -> InstructionError::AccountNotExecutable
5. (instruction_accounts, program_indices) doner
```

**Onemli Algoritma:**
- Duplicate hesap tespiti O(n^2) ama "very small slice" oldugundan kabul edilebilir
- `is_signer |= account_meta.is_signer` — duplicate hesaplarda imza OR'lanir
- `is_writable |= account_meta.is_writable` — writable OR'lanir

### process_instruction() — Ana CPI Dispatcher (Satir 418-435)

```rust
pub fn process_instruction(
    &mut self,
    instruction_data: &[u8],
    instruction_accounts: &[InstructionAccount],
    program_indices: &[IndexOfAccount],
    compute_units_consumed: &mut u64,
    timings: &mut ExecuteTimings,
) -> Result<(), InstructionError> {
    *compute_units_consumed = 0;

    // Instruction context'i hazirla
    self.transaction_context
        .get_next_instruction_context()?
        .configure(program_indices, instruction_accounts, instruction_data);

    self.push()?;

    // Calistir, sonra POP (push basardiysa mutlaka pop)
    self.process_executable_chain(compute_units_consumed, timings)
        .and(self.pop())  // .and() = result ne olursa olsun pop yap
}
```

**Kritik Pattern:** `.and(self.pop())` kullanimi — `and_then` degil `and`. Bu, `process_executable_chain` hata verse bile `pop()` calistirir. Stack bozulmaz.

### process_executable_chain() — Builtin Program Execution (Satir 438-535)

```rust
fn process_executable_chain(
    &mut self,
    compute_units_consumed: &mut u64,
    timings: &mut ExecuteTimings,
) -> Result<(), InstructionError> {
    // Satir 443-456: Owner ID'yi bul
    // native_loader ownered -> programin kendisi builtin
    // diger owner -> owner'i builtin ID olarak kullan
    let builtin_id = {
        let borrowed_root_account = instruction_context
            .try_borrow_program_account(self.transaction_context, 0)
            .map_err(|_| InstructionError::UnsupportedProgramId)?;
        let owner_id = borrowed_root_account.get_owner();
        if native_loader::check_id(owner_id) {
            *borrowed_root_account.get_key()
        } else {
            *owner_id
        }
    };

    // Satir 459-471: "entrypoint" fonksiyonunu hash ile ara
    const ENTRYPOINT_KEY: u32 = 0x71E3CF81;  // Murmur3("entrypoint")
    let entry = self.programs_loaded_for_tx_batch.find(&builtin_id)
        .ok_or(InstructionError::UnsupportedProgramId)?;
    let function = match &entry.program {
        LoadedProgramType::Builtin(program) => program
            .get_function_registry()
            .lookup_by_key(ENTRYPOINT_KEY)
            .map(|(_name, function)| function),
        _ => None,
    }.ok_or(InstructionError::UnsupportedProgramId)?;

    // Satir 472: Kullanim sayacini arttir
    entry.ix_usage_counter.fetch_add(1, Ordering::Relaxed);

    // Satir 474-478: Return data sifirla, log yaz
    self.transaction_context.set_return_data(program_id, Vec::new())?;
    stable_log::program_invoke(&logger, &program_id, self.get_stack_height());

    // Satir 480-496: UNSAFE: lifetime transmute ile EbpfVm olustur
    let empty_memory_mapping = MemoryMapping::new(Vec::new(), &mock_config, &SBPFVersion::V1).unwrap();
    let mut vm = EbpfVm::new(
        self.programs_loaded_for_tx_batch.environments.program_runtime_v2.clone(),
        &SBPFVersion::V1,
        unsafe { std::mem::transmute::<&mut InvokeContext, &mut InvokeContext>(self) },
        empty_memory_mapping,
        0,
    );

    // Satir 497: Calistir
    vm.invoke_function(function);

    // Satir 498-518: Sonucu isle
    let result = match vm.program_result {
        ProgramResult::Ok(_) => {
            stable_log::program_success(&logger, &program_id);
            Ok(())
        }
        ProgramResult::Err(ref err) => {
            if let EbpfError::SyscallError(syscall_error) = err {
                if let Some(instruction_err) = syscall_error.downcast_ref::<InstructionError>() {
                    stable_log::program_failure(&logger, &program_id, instruction_err);
                    Err(instruction_err.clone())
                } else {
                    Err(InstructionError::ProgramFailedToComplete)
                }
            } else {
                Err(InstructionError::ProgramFailedToComplete)
            }
        }
    };

    // Satir 519-523: CU tuketimini hesapla
    let post_remaining_units = self.get_remaining();
    *compute_units_consumed = pre_remaining_units.saturating_sub(post_remaining_units);

    // Builtin programlar en az 1 CU tuketmeli
    if builtin_id == program_id && result.is_ok() && *compute_units_consumed == 0 {
        return Err(InstructionError::BuiltinProgramsMustConsumeComputeUnits);
    }
    result
}
```

### UNSAFE Blok (Satir 493)

```rust
unsafe { std::mem::transmute::<&mut InvokeContext, &mut InvokeContext>(self) }
```

**Bu UNSAFE'in amaci:** `EbpfVm` `'static` lifetime icin `InvokeContext` istiyor, ama mevcut `InvokeContext` `'a` lifetime'ina sahip. `transmute` lifetime'i silip gercek omrunu gizler. Bu "program-runtime v2 icin gecici" oldugu yorum satirinda belirtilmis.

### Diger Accessor Metotlari (Satir 537-615)

| Satir | Metot | Aciklama |
|-------|-------|----------|
| 538 | `get_log_collector()` | `Option<Rc<RefCell<LogCollector>>>` doner |
| 543 | `consume_checked()` | CU tuketir, asimi `ComputationalBudgetExceeded` |
| 556 | `mock_set_remaining()` | Test icin CU ayarla |
| 561 | `get_compute_budget()` | `current_compute_budget` referansi |
| 566 | `get_sysvar_cache()` | Sysvar cache referansi |
| 571 | `get_check_aligned()` | Pointer alignment kontrolu (deprecated loader'larda kapali) |
| 585 | `set_syscall_context()` | Mevcut stack frame icin syscall context ata |
| 597 | `get_syscall_context()` | Mevcut frame'in syscall context'ini al (immutable) |
| 605 | `get_syscall_context_mut()` | Mevcut frame'in syscall context'ini al (mutable) |
| 613 | `get_traces()` | Tum trace kayitlarini doner |

### Test Fonksiyonlari (Satir 868-1202)

```
test_instruction_stack_height()     — satir 868
test_max_instruction_trace_length() — satir 926
test_process_instruction()          — satir 941 (en kapsamli test)
test_invoke_context_compute_budget() — satir 1065
test_process_instruction_accounts_resize_delta() — satir 1090
```

---

## 3. loaded_programs.rs — Satir 1-2668

Bu dosya Solana'nin global program binary cache sistemidir. En buyuk ve karmasik dosya.

### Sabitler ve Tip Aliasi (Satir 33-35)

```rust
pub type ProgramRuntimeEnvironment = Arc<BuiltinProgram<InvokeContext<'static>>>;
pub const MAX_LOADED_ENTRY_COUNT: usize = 256;        // Maksimum cache girisi
pub const DELAY_VISIBILITY_SLOT_OFFSET: Slot = 1;     // Program gorsunurluk gecikme
```

### BlockRelation Enum (Satir 38-50)

```rust
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BlockRelation {
    Ancestor,     // A, B'nin atasadir (ayni fork, daha eski)
    Equal,        // Ayni slot, ayni fork
    Descendant,   // A, B'nin torunudur (ayni fork, daha yeni)
    Unrelated,    // Farkli fork'lar
    Unknown,      // Latest root'tan eski veya gelecekte
}
```

### ForkGraph Trait (Satir 52-61)

```rust
pub trait ForkGraph {
    // A slotunun B slotuyla iliskisini doner
    fn relationship(&self, a: Slot, b: Slot) -> BlockRelation;

    // Slot'un epoch'unu doner (varsayilan 0)
    fn slot_epoch(&self, _slot: Slot) -> Option<Epoch> {
        Some(0)
    }
}
```

**Nova Shield icin:** Fork-aware program cache icin bu trait gerekli. Validator'daki blockstore ile entegre calisir.

### LoadedProgramType Enum (Satir 64-89)

```rust
#[derive(Default)]
pub enum LoadedProgramType {
    // TOMBSTONE'LAR:
    FailedVerification(ProgramRuntimeEnvironment),  // Dogrulama hatasi
    #[default]
    Closed,                                          // Kapatilmis/silinmis program
    DelayVisibility,                                 // Yeni deploy, henuz aktif degil

    // AKTIF PROGRAMLAR:
    Unloaded(ProgramRuntimeEnvironment),            // Dogru ama binary evict edilmis
    LegacyV0(Executable<InvokeContext<'static>>),   // loader-v1 (deprecated)
    LegacyV1(Executable<InvokeContext<'static>>),   // loader-v2 (upgradeable)
    Typed(Executable<InvokeContext<'static>>),       // loader-v4 (yeni)

    #[cfg(test)]
    TestLoaded(ProgramRuntimeEnvironment),           // Test icin

    Builtin(BuiltinProgram<InvokeContext<'static>>), // Validator'a gomulu
}
```

**Tasarim Deseni:** Tombstone + Active ayirimi kritik. Tombstone'lar tekrar yuklenmeyi engeller (cache poisoning savar), ayni zamanda neden yuklenmedigi aciklanir.

### LoadedProgram Struct (Satir 130-146)

```rust
#[derive(Debug, Default)]
pub struct LoadedProgram {
    pub program: LoadedProgramType,          // Program icerik/durum
    pub account_size: usize,                 // On-chain hesap boyutu
    pub deployment_slot: Slot,               // Deploy edildigi slot
    pub effective_slot: Slot,               // Aktif olacagi slot (genelde deployment_slot + 1)
    pub tx_usage_counter: AtomicU64,         // TX bazinda kullanim sayaci
    pub ix_usage_counter: AtomicU64,         // Instruction bazinda kullanim sayaci
    pub latest_access_slot: AtomicU64,      // Son erisim slotu (eviction icin)
}
```

**Not:** `AtomicU64` kullanimi — cache paylasilabilir ve lock-free guncelleme saglanir.

### Stats Struct (Satir 149-173)

```rust
#[derive(Debug, Default)]
pub struct Stats {
    pub hits: AtomicU64,               // Cache hit sayisi
    pub misses: AtomicU64,             // Cache miss sayisi
    pub evictions: HashMap<Pubkey, u64>, // Hangi program kac kez evict edildi
    pub reloads: AtomicU64,            // Unloaded -> Loaded reload sayisi
    pub insertions: AtomicU64,         // Yeni girdi ekleme sayisi
    pub lost_insertions: AtomicU64,    // Fork'ta gorunmez olan eklemeler
    pub replacements: AtomicU64,       // Hatali degistirme (bug indicator)
    pub one_hit_wonders: AtomicU64,    // Sadece 1 kez kullanilip evict edilenler
    pub prunes_orphan: AtomicU64,      // Orphan fork temizleme sayisi
    pub prunes_environment: AtomicU64, // Eski epoch environment temizleme
    pub empty_entries: AtomicU64,      // Bos SecondLevel silen sayisi
}
```

### Stats::submit() (Satir 177-231)

`datapoint_info!` makrosu ile Solana metrik sistemine gonderir. Eviction detaylari log::Level::Trace seviyesinde programa gore sirali yazilir.

### LoadProgramMetrics Struct (Satir 235-246)

```rust
#[derive(Debug, Default)]
pub struct LoadProgramMetrics {
    pub program_id: String,              // Program adresi (text)
    pub register_syscalls_us: u64,       // Syscall kaydi suresi
    pub load_elf_us: u64,               // ELF binary yukleme suresi
    pub verify_code_us: u64,            // Dogrulama suresi
    pub jit_compile_us: u64,            // JIT derleme suresi
}
```

### LoadedProgram::new() — Yeni Kullanici Programi (Satir 278-297)

```rust
pub fn new(
    loader_key: &Pubkey,
    program_runtime_environment: ProgramRuntimeEnvironment,
    deployment_slot: Slot,
    effective_slot: Slot,
    elf_bytes: &[u8],
    account_size: usize,
    metrics: &mut LoadProgramMetrics,
) -> Result<Self, Box<dyn std::error::Error>> {
    Self::new_internal(/* reloading: false */)
}
```

### UNSAFE: LoadedProgram::reload() (Satir 303-326)

```rust
/// # Safety
/// Programin daha once dogrulandigini varsayar.
/// Sadece once yuklenip sonra evict edilen programlar icin kullanilmali.
pub unsafe fn reload(
    loader_key: &Pubkey,
    program_runtime_environment: Arc<BuiltinProgram<InvokeContext<'static>>>,
    deployment_slot: Slot,
    effective_slot: Slot,
    elf_bytes: &[u8],
    account_size: usize,
    metrics: &mut LoadProgramMetrics,
) -> Result<Self, Box<dyn std::error::Error>> {
    Self::new_internal(/* reloading: true */)
}
```

**UNSAFE Nedeni:** Verifier gecilir (dogrulama atlanir). Bu sadece daha once dogrulanmis ve `Unloaded` durumuna getirilmis programlar icin guvenli.

### new_internal() — Yukleme Süreci (Satir 328-380)

```rust
fn new_internal(..., reloading: bool) -> Result<Self, Box<dyn std::error::Error>> {
    // 1. ELF yukle
    let mut load_elf_time = Measure::start("load_elf_time");
    let mut executable = Executable::load(elf_bytes, program_runtime_environment.clone())?;
    metrics.load_elf_us = load_elf_time.as_us();

    // 2. Dogrula (sadece ilk yukleme)
    if !reloading {
        let mut verify_code_time = Measure::start("verify_code_time");
        executable.verify::<RequisiteVerifier>()?;
        metrics.verify_code_us = verify_code_time.as_us();
    }

    // 3. JIT derle (sadece x86_64 Linux/Mac)
    #[cfg(all(not(target_os = "windows"), target_arch = "x86_64"))]
    {
        let mut jit_compile_time = Measure::start("jit_compile_time");
        executable.jit_compile()?;
        metrics.jit_compile_us = jit_compile_time.as_us();
    }

    // 4. Loader tipine gore kategorize et
    let program = if bpf_loader_deprecated::check_id(loader_key) {
        LoadedProgramType::LegacyV0(executable)
    } else if bpf_loader::check_id(loader_key) || bpf_loader_upgradeable::check_id(loader_key) {
        LoadedProgramType::LegacyV1(executable)
    } else if loader_v4::check_id(loader_key) {
        LoadedProgramType::Typed(executable)
    } else {
        panic!();  // Bilinmeyen loader
    };

    Ok(Self { deployment_slot, account_size, effective_slot, ... })
}
```

### decayed_usage_counter() — Zamanla Azalan Sayac (Satir 464-469)

```rust
pub fn decayed_usage_counter(&self, now: Slot) -> u64 {
    let last_access = self.latest_access_slot.load(Ordering::Relaxed);
    // 63'ten fazla kaydirma overflow yapar, min ile sinirla
    let decaying_for = std::cmp::min(63, now.saturating_sub(last_access));
    self.tx_usage_counter.load(Ordering::Relaxed) >> decaying_for
}
```

**Akilli Algoritma:** Her gecen slot icin sayac bit shift ile yarilir. 63 slot sonra sayac sifira yaklasar. Bu LRU-benzeri davranis saglar.

### ProgramRuntimeEnvironments (Satir 475-494)

```rust
#[derive(Clone, Debug)]
pub struct ProgramRuntimeEnvironments {
    pub program_runtime_v1: ProgramRuntimeEnvironment,  // loader-v1/v2 icin
    pub program_runtime_v2: ProgramRuntimeEnvironment,  // loader-v4 icin
}
```

Her epoch icin ayri environment. Feature aktiflestiginde environment degisir.

### LoadingTaskCookie (Satir 496-508)

```rust
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct LoadingTaskCookie(u64);

impl LoadingTaskCookie {
    fn new() -> Self { Self(0) }
    fn update(&mut self) { *cookie = cookie.wrapping_add(1); }
}
```

Cooperative loading icin cookie mekanizmasi. Her `notify()` cagrisi cookie'yi arttirir, bekleyen thread'ler cookie degisimini bekler.

### LoadingTaskWaiter (Satir 511-541)

```rust
#[derive(Debug, Default)]
pub struct LoadingTaskWaiter {
    cookie: Mutex<LoadingTaskCookie>,
    cond: Condvar,
}

impl LoadingTaskWaiter {
    pub fn notify(&self) {
        let mut cookie = self.cookie.lock().unwrap();
        cookie.update();
        self.cond.notify_all();  // Tum bekleyen thread'leri uyandır
    }

    pub fn wait(&self, cookie: LoadingTaskCookie) -> LoadingTaskCookie {
        let cookie_guard = self.cookie.lock().unwrap();
        *self.cond.wait_while(cookie_guard, |current_cookie| {
            *current_cookie == cookie  // Cookie degismeyene kadar bekle
        }).unwrap()
    }
}
```

**Cooperative Loading Mekanizmasi:** Ayni missing program'a ihtiyac duyan birden fazla TX batch varsa sadece biri yukler, digerleri bekler. `Condvar` + cookie ile senkronize edilir.

### SecondLevel Struct (Satir 544-554)

```rust
#[derive(Debug, Default)]
struct SecondLevel {
    // Slota gore sirali program versiyonlari (tum fork'larda)
    slot_versions: Vec<Arc<LoadedProgram>>,
    // Cooperative yukleme kilidi: (slot, thread_id) ciftine gore
    cooperative_loading_lock: Option<(Slot, std::thread::ThreadId)>,
}
```

**Iki seviyeli index:** `HashMap<Pubkey, SecondLevel>` — program adresi -> tum versiyonlari.

### ProgramCache<FG: ForkGraph> Struct (Satir 571-596)

```rust
pub struct ProgramCache<FG: ForkGraph> {
    // Iki seviyeli index: Pubkey -> SecondLevel
    entries: HashMap<Pubkey, SecondLevel>,

    // Son rerooting slotu
    pub latest_root_slot: Slot,

    // Son rerooting epoch'u
    pub latest_root_epoch: Epoch,

    // Mevcut epoch'un environment'lari
    pub environments: ProgramRuntimeEnvironments,

    // Bir sonraki epoch icin hazirlanmakta olan environment'lar
    // (epoch sinirinda Some, diger zamanlarda None)
    pub upcoming_environments: Option<ProgramRuntimeEnvironments>,

    // Bir sonraki epoch icin yeniden derlenmesi gereken programlar
    pub programs_to_recompile: Vec<(Pubkey, Arc<LoadedProgram>)>,

    // Istatistikler
    pub stats: Stats,

    // Fork graph referansi
    pub fork_graph: Option<Arc<RwLock<FG>>>,

    // Cooperative loading koordinatoru
    pub loading_task_waiter: Arc<LoadingTaskWaiter>,
}
```

**Konu Guvenlik Yapisi:** `fork_graph: Arc<RwLock<FG>>` — coklu okuyucu, tek yazici. `loading_task_waiter: Arc<LoadingTaskWaiter>` paylasilabilir.

### LoadedProgramsForTxBatch Struct (Satir 614-622)

```rust
#[derive(Clone, Debug, Default)]
pub struct LoadedProgramsForTxBatch {
    // Program adresi -> Gecerli program girisi (bu slot icin)
    entries: HashMap<Pubkey, Arc<LoadedProgram>>,

    // Bu batch'in slotu
    slot: Slot,

    // Bu batch icin environment'lar
    pub environments: ProgramRuntimeEnvironments,

    // Maksimum limit asildiysa true
    pub hit_max_limit: bool,
}
```

**Global cache'in yerel gorunumu.** TX batch baslamadan once `ProgramCache::extract()` ile doldurulur.

### LoadedProgramsForTxBatch::find() (Satir 647-661)

```rust
pub fn find(&self, key: &Pubkey) -> Option<Arc<LoadedProgram>> {
    self.entries.get(key).map(|entry| {
        if entry.is_implicit_delay_visibility_tombstone(self.slot) {
            // Program bu slot icin henuz aktif degil
            // Tombstone doner
            Arc::new(LoadedProgram::new_tombstone(
                entry.deployment_slot,
                LoadedProgramType::DelayVisibility,
            ))
        } else {
            entry.clone()
        }
    })
}
```

**Gizli Gecikme Gorunurlugu:** `effective_slot > slot` durumunda program tombstone olarak gosterilir. Kullanici programin var oldugunu gorur ama calistiramaz.

### LoadedProgramMatchCriteria Enum (Satir 678-682)

```rust
pub enum LoadedProgramMatchCriteria {
    DeployedOnOrAfterSlot(Slot),  // Belirli bir slot'tan sonra deploy edilmis
    Tombstone,                     // Sadece tombstone'lar
    NoCriteria,                    // Her turlu program
}
```

### ProgramCache::assign_program() (Satir 715-761)

```rust
pub fn assign_program(&mut self, key: Pubkey, entry: Arc<LoadedProgram>) -> bool {
    let slot_versions = &mut self.entries.entry(key).or_default().slot_versions;

    // Binary search ile dogru pozisyonu bul (effective_slot + deployment_slot sirali)
    match slot_versions.binary_search_by(|at| {
        at.effective_slot.cmp(&entry.effective_slot)
            .then(at.deployment_slot.cmp(&entry.deployment_slot))
    }) {
        Ok(index) => {
            // Mevcut giris bul
            let existing = slot_versions.get_mut(index).unwrap();
            match (&existing.program, &entry.program) {
                // Izin verilen degistirmeler:
                (Builtin, Builtin) => {}           // Builtin update
                (Unloaded, LegacyV0/V1/Typed) => {} // Reload

                // Izin verilmeyen:
                _ => {
                    error!("Unexpected replacement");
                    self.stats.replacements.fetch_add(1, Ordering::Relaxed);
                    return true;
                }
            }
            // Kullanim sayaclarini aktar
            entry.tx_usage_counter.fetch_add(existing.tx_usage_counter.load(...), ...);
            entry.ix_usage_counter.fetch_add(existing.ix_usage_counter.load(...), ...);
            *existing = Arc::clone(&entry);
            self.stats.reloads.fetch_add(1, Ordering::Relaxed);
        }
        Err(index) => {
            // Yeni giris ekle
            self.stats.insertions.fetch_add(1, Ordering::Relaxed);
            slot_versions.insert(index, Arc::clone(&entry));
        }
    }
    false
}
```

### ProgramCache::prune() — Fork Temizleme (Satir 773-850)

En karmasik metot. Yeni root slot belirlendikten sonra orphan fork girdilerini temizler:

```
Prune algoritmi:
1. Epoch degistiyse:
   - upcoming_environments -> environments
   - programs_to_recompile temizle

2. Her SecondLevel icin:
   - Ters sirali (en yeni -> en eski) tara
   - first_ancestor_found bayragi ile izle

   Koruma kurallari:
   a. deployment_slot >= new_root_slot: Sadece Equal/Descendant fork'lar korunsun
   b. Ancestor veya latest_root_slot oncesi:
      - Ilk ancestor korunsun (first_ancestor_found = true)
      - Farkli environment'li eski epoch girislerini koru
      - Digerleri: prunes_orphan++, sil
   c. Epoch degistiyse eski environment'li girdileri sil: prunes_environment++

3. remove_programs_with_no_entries()
4. latest_root_slot guncelle
```

### ProgramCache::extract() — TX Batch Icin Cikarma (Satir 878-961)

```
Algoritma:
- search_for listesindeki her program icin:
  1. SecondLevel'i bul
  2. En yeni versiyondan geriye dogru tara
  3. deployment_slot <= latest_root_slot VEYA fork graph'ta ancestor/equal ise:
     - effective_slot <= batch_slot ise: programa bak
       - Kriterleri kontrol et (match_criteria)
       - Unloaded ise break (reload gerekiyor)
       - Aksi halde: batch'e ekle, search_for'dan cikar
     - implicit_delay_visibility ise: tombstone ekle
  4. Hic bulunmadiysa: cooperative_loading_task sec
     - Zaten sec edilmemisse ve lock yoksa bu programi sec
     - cooperative_loading_lock'u ayarla

- Istatistikleri guncelle (hits/misses)
- cooperative_loading_task doner (None ise yukleme yok)
```

### Eviction Algoritmalari (Satir 1031-1073)

**sort_and_unload() — Sirali Tahliye (Satir 1031-1038):**
```rust
pub fn sort_and_unload(&mut self, shrink_to: PercentageInteger) {
    let mut sorted_candidates = self.get_flattened_entries(true, true);
    sorted_candidates.sort_by_cached_key(|(_id, program)|
        program.tx_usage_counter.load(Ordering::Relaxed)
    );
    // En az kullanilandan itibaren tahliye et
    let num_to_unload = sorted_candidates.len()
        .saturating_sub(shrink_to.apply_to(MAX_LOADED_ENTRY_COUNT));
    self.unload_program_entries(sorted_candidates.iter().take(num_to_unload));
}
```

**evict_using_2s_random_selection() — Rastgele Tahliye (Satir 1043-1073):**
```rust
pub fn evict_using_2s_random_selection(&mut self, shrink_to: PercentageInteger, now: Slot) {
    // "2's random selection" algoritmasi:
    // Iki rastgele aday sec, az kullanilanini evict et
    for _ in 0..num_to_unload {
        let (index1, usage1) = random_index_and_usage_counter(&candidates, now);
        let (index2, usage2) = random_index_and_usage_counter(&candidates, now);
        // decayed_usage_counter kullanir (slot-bazli bozunma ile)
        let (program, entry) = if usage1 < usage2 {
            candidates.swap_remove(index1)
        } else {
            candidates.swap_remove(index2)
        };
        self.unload_program_entry(&program, &entry);
    }
}
```

### unload_program_entry() — Tahliye Islemi (Satir 1092-1113)

```rust
fn unload_program_entry(&mut self, program: &Pubkey, remove_entry: &Arc<LoadedProgram>) {
    let candidate = second_level.slot_versions.iter_mut()
        .find(|entry| entry == &remove_entry)
        .expect("Program entry not found");

    // Tombstone ve already-unloaded girdiler tahliye edilemez
    if let Some(unloaded) = candidate.to_unloaded() {
        if candidate.tx_usage_counter.load(Ordering::Relaxed) == 1 {
            self.stats.one_hit_wonders.fetch_add(1, Ordering::Relaxed);
        }
        // Eviction sayacini artir
        self.stats.evictions.entry(*program)
            .and_modify(|c| saturating_add_assign!(*c, 1))
            .or_insert(1);
        // Executable -> Unloaded donustur (binary'yi birak, metadata'yi koru)
        *candidate = Arc::new(unloaded);
    }
}
```

**Kritik:** Eviction sadece `Unloaded` durumuna getirir, tamamen silmez. Kullanim istatistikleri korunur.

### Test Fonksiyonlari (Satir 1156-2668)

```
test_usage_counter_decay()           — satir 1276: Bozunma algoritmasi testi
program_deploy_test_helper()         — satir 1304: Deploy/evict yardimci fonksiyonu
test_random_eviction()               — satir 1346: Rastgele eviction testi
test_eviction()                      — satir 1439: Deterministic eviction testi
test_usage_count_of_unloaded_program() — satir 1549
test_fuzz_assign_program_order()     — satir 1604: Fuzz: atama sirasi
test_assign_program_failure()        — satir 1663
test_assign_program_success()        — satir 1700
test_tombstone()                     — satir 1730
test_prune_empty()                   — satir 1800
test_prune_different_env()           — satir 1854: Epoch gecisi testi
```

---

## 4. compute_budget.rs — Satir 1-221

### DEFAULT_HEAP_COST Sabiti (Satir 16)

```rust
pub const DEFAULT_HEAP_COST: u64 = 8;
// ~0.5us/page, 1 sayfa = 32K, 15CU/us => 0.5 * 15 ~= 8 CU/sayfa
```

### ComputeBudget Struct (Satir 18-123)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComputeBudget {
    pub compute_unit_limit: u64,          // Maks CU (tx veya instruction)
    pub log_64_units: u64,               // log_u64 maliyeti
    pub create_program_address_units: u64, // PDA olusturma maliyeti
    pub invoke_units: u64,               // CPI baslatma maliyeti
    pub max_invoke_stack_height: usize,  // Maks CPI derinligi
    pub max_instruction_trace_length: usize, // Maks toplam instruction sayisi
    pub sha256_base_cost: u64,           // SHA256 temel maliyeti
    pub sha256_byte_cost: u64,           // SHA256 byte basina maliyet
    pub sha256_max_slices: u64,          // SHA256 maks dilim sayisi
    pub max_call_depth: usize,           // Maks SBF cagri derinligi
    pub stack_frame_size: usize,         // Stack frame boyutu
    pub log_pubkey_units: u64,           // Pubkey loglama maliyeti
    pub max_cpi_instruction_size: usize, // Maks CPI instruction boyutu
    pub cpi_bytes_per_unit: u64,         // CPI veri byte/CU orani
    pub sysvar_base_cost: u64,           // Sysvar erisim temel maliyeti
    pub secp256k1_recover_cost: u64,     // secp256k1 recovery maliyeti
    pub syscall_base_cost: u64,          // Syscall temel maliyeti
    // Curve25519 Edwards maliyetleri:
    pub curve25519_edwards_validate_point_cost: u64,
    pub curve25519_edwards_add_cost: u64,
    pub curve25519_edwards_subtract_cost: u64,
    pub curve25519_edwards_multiply_cost: u64,
    pub curve25519_edwards_msm_base_cost: u64,
    pub curve25519_edwards_msm_incremental_cost: u64,
    // Curve25519 Ristretto maliyetleri:
    pub curve25519_ristretto_validate_point_cost: u64,
    pub curve25519_ristretto_add_cost: u64,
    pub curve25519_ristretto_subtract_cost: u64,
    pub curve25519_ristretto_multiply_cost: u64,
    pub curve25519_ristretto_msm_base_cost: u64,
    pub curve25519_ristretto_msm_incremental_cost: u64,
    pub heap_size: u32,                  // Heap boyutu (bayt)
    pub heap_cost: u64,                  // Ek heap CU maliyeti
    pub mem_op_base_cost: u64,           // Bellek operasyonu temel maliyeti
    // alt_bn128 maliyetleri (BN254 pairing):
    pub alt_bn128_addition_cost: u64,
    pub alt_bn128_multiplication_cost: u64,
    pub alt_bn128_pairing_one_pair_cost_first: u64,
    pub alt_bn128_pairing_one_pair_cost_other: u64,
    pub big_modular_exponentiation_cost: u64,
    // Poseidon maliyetleri (ZK hash):
    pub poseidon_cost_coefficient_a: u64,
    pub poseidon_cost_coefficient_c: u64,
    pub get_remaining_compute_units_cost: u64,
    // alt_bn128 compress/decompress:
    pub alt_bn128_g1_compress: u64,
    pub alt_bn128_g1_decompress: u64,
    pub alt_bn128_g2_compress: u64,
    pub alt_bn128_g2_decompress: u64,
}
```

### Compute Unit Maliyet Tablosu (ComputeBudget::new() — Satir 132-178)

| Operasyon | CU Maliyeti |
|-----------|-------------|
| log_64 | 100 |
| create_program_address (PDA) | 1_500 |
| invoke (CPI baslat) | 1_000 |
| sha256 base | 85 |
| sha256 per byte | 1 |
| log_pubkey | 100 |
| sysvar_base | 100 |
| secp256k1_recover | 25_000 |
| syscall_base | 100 |
| curve25519_edwards validate | 159 |
| curve25519_edwards add | 473 |
| curve25519_edwards subtract | 475 |
| curve25519_edwards multiply | 2_177 |
| curve25519_edwards msm base | 2_273 |
| curve25519_edwards msm incremental | 758 |
| curve25519_ristretto validate | 169 |
| curve25519_ristretto add | 521 |
| curve25519_ristretto subtract | 519 |
| curve25519_ristretto multiply | 2_208 |
| curve25519_ristretto msm base | 2_303 |
| curve25519_ristretto msm incremental | 788 |
| heap_cost per 32K page | 8 |
| mem_op_base | 10 |
| alt_bn128 add | 334 |
| alt_bn128 multiply | 3_840 |
| alt_bn128 pairing first pair | 36_364 |
| alt_bn128 pairing other pairs | 12_121 |
| big_modular_exponentiation | 33 |
| poseidon coefficient_a | 61 |
| poseidon coefficient_c | 542 |
| get_remaining_compute_units | 100 |
| alt_bn128_g1_compress | 30 |
| alt_bn128_g1_decompress | 398 |
| alt_bn128_g2_compress | 86 |
| alt_bn128_g2_decompress | 13_610 |

**Onemli Sinirlar:**
- `max_invoke_stack_height: 5` — CPI derinlik siniri
- `max_instruction_trace_length: 64` — TX basina maks instruction
- `max_cpi_instruction_size: 1280` — IPv6 Min MTU boyutu
- `cpi_bytes_per_unit: 250` — ~50MB @ 200_000 CU
- `stack_frame_size: 4_096` — LLVM backend ile eslesmeli
- `max_call_depth: 64` — SBF cagri derinligi

### poseidon_cost() (Satir 211-219)

```rust
pub fn poseidon_cost(&self, nr_inputs: u64) -> Option<u64> {
    // f(n) = 61*n^2 + 542 (quadratic)
    let squared_inputs = nr_inputs.checked_pow(2)?;
    let mul_result = self.poseidon_cost_coefficient_a.checked_mul(squared_inputs)?;
    let final_result = mul_result.checked_add(self.poseidon_cost_coefficient_c)?;
    Some(final_result)
}
```

Poseidon hash maliyeti kuadratik fonksiyon: `61*n^2 + 542`. Overflow kontrolu `checked_*` ile yapilir.

---

## 5. compute_budget_processor.rs — Satir 1-500

### Sabitler (Satir 17-23)

```rust
const MAX_HEAP_FRAME_BYTES: u32 = 256 * 1024;              // 256 KB
pub const DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT: u32 = 200_000; // 200K CU/instruction
pub const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;          // 1.4M CU/transaction
pub const MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES: u32 = 64 * 1024 * 1024; // 64 MB
```

### ComputeBudgetLimits Struct (Satir 25-31)

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputeBudgetLimits {
    pub updated_heap_bytes: u32,    // Talep edilen heap boyutu
    pub compute_unit_limit: u32,    // CU siniri
    pub compute_unit_price: u64,    // micro-lamport/CU (oncelik ucreti)
    pub loaded_accounts_bytes: u32, // Yuklenen hesap verisi limiti
}
```

### process_compute_budget_instructions() (Satir 68-147)

```rust
pub fn process_compute_budget_instructions<'a>(
    instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
) -> Result<ComputeBudgetLimits, TransactionError> {
    let mut num_non_compute_budget_instructions: u32 = 0;
    let mut updated_compute_unit_limit = None;
    let mut updated_compute_unit_price = None;
    let mut requested_heap_size = None;
    let mut updated_loaded_accounts_data_size_limit = None;

    for (i, (program_id, instruction)) in instructions.enumerate() {
        if compute_budget::check_id(program_id) {
            // compute_budget program ID bulundu
            match try_from_slice_unchecked(&instruction.data) {
                Ok(ComputeBudgetInstruction::RequestHeapFrame(bytes)) => {
                    // Duplicate kontrolu: Err(DuplicateInstruction)
                    // Boyut kontrolu: sanitize_requested_heap_size()
                    requested_heap_size = Some(bytes);
                }
                Ok(ComputeBudgetInstruction::SetComputeUnitLimit(cu)) => {
                    updated_compute_unit_limit = Some(cu);
                }
                Ok(ComputeBudgetInstruction::SetComputeUnitPrice(price)) => {
                    updated_compute_unit_price = Some(price);
                }
                Ok(ComputeBudgetInstruction::SetLoadedAccountsDataSizeLimit(bytes)) => {
                    updated_loaded_accounts_data_size_limit = Some(bytes);
                }
                _ => return Err(TransactionError::InstructionError(
                    i as u8, InstructionError::InvalidInstructionData
                )),
            }
        } else {
            num_non_compute_budget_instructions += 1;
        }
    }

    // Varsayilan CU = num_non_compute_budget_instructions * 200_000
    let compute_unit_limit = updated_compute_unit_limit
        .unwrap_or_else(|| num_non_compute_budget_instructions
            .saturating_mul(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT))
        .min(MAX_COMPUTE_UNIT_LIMIT);

    // ...
    Ok(ComputeBudgetLimits { ... })
}
```

**Onemli Kural:** Compute budget instruction'lari duplicate olamaz — `DuplicateInstruction` hatasi. Boyut %1024 katinda olmali.

### sanitize_requested_heap_size() (Satir 149-152)

```rust
fn sanitize_requested_heap_size(bytes: u32) -> bool {
    (u32::try_from(MIN_HEAP_FRAME_BYTES).unwrap()..=MAX_HEAP_FRAME_BYTES).contains(&bytes)
        && bytes % 1024 == 0  // 1KB katinda olmali
}
```

---

## 6. sysvar_cache.rs — Satir 1-315

### SysvarCache Struct (Satir 27-40)

```rust
#[derive(Default, Clone, Debug)]
pub struct SysvarCache {
    clock: Option<Arc<Clock>>,                    // Slot, epoch, unix_timestamp
    epoch_schedule: Option<Arc<EpochSchedule>>,   // Epoch zamanlamasi
    epoch_rewards: Option<Arc<EpochRewards>>,     // Dogrulayici oduller
    #[allow(deprecated)]
    fees: Option<Arc<Fees>>,                      // (Deprecated) Ucret yapisi
    rent: Option<Arc<Rent>>,                      // Kira hesaplama
    slot_hashes: Option<Arc<SlotHashes>>,         // Son 512 slot hash'i
    #[allow(deprecated)]
    recent_blockhashes: Option<Arc<RecentBlockhashes>>, // (Deprecated)
    stake_history: Option<Arc<StakeHistory>>,     // Stake gecmisi
    last_restart_slot: Option<Arc<LastRestartSlot>>, // Son validator yeniden baslama
}
```

**Tum alanlari:** `Option<Arc<T>>` — `Arc` ile coklu sahiplik, `Option` ile "henuz yuklenmedi" durumu.

### fill_missing_entries() (Satir 137-208)

```rust
pub fn fill_missing_entries<F: FnMut(&Pubkey, &mut dyn FnMut(&[u8]))>(
    &mut self,
    mut get_account_data: F,
) {
    // Her sysvar icin: None ise hesap verisinden yukle
    if self.clock.is_none() {
        get_account_data(&Clock::id(), &mut |data: &[u8]| {
            if let Ok(clock) = bincode::deserialize(data) {
                self.set_clock(clock);
            }
        });
    }
    // ... diger sysvar'lar icin ayni pattern
}
```

**Fonksiyon pointer ile callback:** `get_account_data` hesap verisine erisim saglayan kapanisir. Bank ya da test ortami bu callback'i saglar.

### get_sysvar_with_account_check Modulu (Satir 220-314)

TX icindeki hesap indexini dogrulayarak sysvar'lara erisim saglar:

```rust
fn check_sysvar_account<S: Sysvar>(...) -> Result<(), InstructionError> {
    let index_in_transaction = instruction_context
        .get_index_of_instruction_account_in_transaction(instruction_account_index)?;
    if !S::check_id(transaction_context.get_key_of_account_at_index(index_in_transaction)?) {
        return Err(InstructionError::InvalidArgument);
    }
    Ok(())
}
```

Tutarlilik saglamak icin: keyed_account davranisini taklit eder.

---

## 7. timings.rs — Satir 1-552

### ProgramTiming Struct (Satir 11-19)

```rust
#[derive(Default, Debug, PartialEq, Eq)]
pub struct ProgramTiming {
    pub accumulated_us: u64,                      // Toplam sure (mikrosaniye)
    pub accumulated_units: u64,                   // Toplam basarili CU
    pub count: u32,                               // Basarili tx sayisi
    pub errored_txs_compute_consumed: Vec<u64>,   // Hatali tx'lerin CU'lari
    pub total_errored_units: u64,                 // Hatali tx toplam CU
}
```

### ExecuteTimingType Enum (Satir 43-54)

```rust
#[derive(Debug, Sequence)]
pub enum ExecuteTimingType {
    CheckUs,               // Dogrulama suresi
    LoadUs,                // Yukleme suresi
    ExecuteUs,             // Yurutme suresi
    StoreUs,               // Depolama suresi
    UpdateStakesCacheUs,   // Stake cache guncelleme
    NumExecuteBatches,     // Batch sayisi
    CollectLogsUs,         // Log toplama
    TotalBatchesLen,       // Toplam batch uzunlugu
    UpdateTransactionStatuses, // TX durum guncelleme
}
```

Enum-indexed array: `Metrics([u64; ExecuteTimingType::CARDINALITY])`

### ExecuteTimings Struct (Satir 312-317)

```rust
#[derive(Debug, Default)]
pub struct ExecuteTimings {
    pub metrics: Metrics,
    pub details: ExecuteDetailsTimings,
    pub execute_accessories: ExecuteAccessoryTimings,
}
```

### ExecuteDetailsTimings Struct (Satir 383-397)

```rust
#[derive(Default, Debug, PartialEq, Eq)]
pub struct ExecuteDetailsTimings {
    pub serialize_us: u64,
    pub create_vm_us: u64,
    pub execute_us: u64,
    pub deserialize_us: u64,
    pub get_or_create_executor_us: u64,
    pub changed_account_count: u64,
    pub total_account_count: u64,
    pub create_executor_register_syscalls_us: u64,
    pub create_executor_load_elf_us: u64,
    pub create_executor_verify_code_us: u64,
    pub create_executor_jit_compile_us: u64,
    pub per_program_timings: HashMap<Pubkey, ProgramTiming>,  // Program bazinda metrikler
}
```

### ThreadExecuteTimings (Satir 280-310)

```rust
#[derive(Debug, Default)]
pub struct ThreadExecuteTimings {
    pub total_thread_us: u64,
    pub total_transactions_executed: u64,
    pub execute_timings: ExecuteTimings,
}
```

`report_stats()` ile `datapoint_info!("replay-slot-end-to-end-stats", ...)` olarak raporlar.

---

## 8. log_collector.rs — Satir 1-123

### LOG_MESSAGES_BYTES_LIMIT (Satir 4)

```rust
const LOG_MESSAGES_BYTES_LIMIT: usize = 10 * 1000;  // 10 KB
```

### LogCollector Struct (Satir 6-11)

```rust
pub struct LogCollector {
    messages: Vec<String>,       // Toplanan mesajlar
    bytes_written: usize,        // Yazilan toplam byte
    bytes_limit: Option<usize>,  // None = sinirsiz
    limit_warning: bool,         // Limit uyarisi gonderildi mi?
}
```

### log() Metodu (Satir 25-41)

```rust
pub fn log(&mut self, message: &str) {
    let Some(limit) = self.bytes_limit else {
        // Limit yok, direkt ekle
        self.messages.push(message.to_string());
        return;
    };

    let bytes_written = self.bytes_written.saturating_add(message.len());
    if bytes_written >= limit {
        if !self.limit_warning {
            self.limit_warning = true;
            self.messages.push(String::from("Log truncated"));  // Tek uyari
        }
    } else {
        self.bytes_written = bytes_written;
        self.messages.push(message.to_string());
    }
}
```

**Tasarim:** Limit asildiginda yeni mesajlar susturulur ama "Log truncated" bir kez eklenir.

### Makrolar

```rust
// ic_logger_msg! — Option<Rc<RefCell<LogCollector>>> ile kullanim
macro_rules! ic_logger_msg {
    ($log_collector:expr, $message:expr) => {
        log::debug!(target: "solana_runtime::message_processor::stable_log", ...);
        if let Some(log_collector) = $log_collector.as_ref() {
            if let Ok(mut lc) = log_collector.try_borrow_mut() {
                lc.log($message);
            }
        }
    };
}

// ic_msg! — InvokeContext ile kullanim
macro_rules! ic_msg {
    ($invoke_context:expr, $message:expr) => {
        ic_logger_msg!($invoke_context.get_log_collector(), $message)
    };
}
```

---

## 9. stable_log.rs — Satir 1-111

**Kritik Not:** Bu log mesajlarinin formati degistirilmemeli — downstream consumer'lar bozulur.

### Fonksiyonlar

```rust
// Satir 20-31: program_invoke()
// Format: "Program <address> invoke [<depth>]"
pub fn program_invoke(
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
    program_id: &Pubkey,
    invoke_depth: usize,
)

// Satir 42-44: program_log()
// Format: "Program log: <message>"
pub fn program_log(log_collector: &Option<Rc<RefCell<LogCollector>>>, message: &str)

// Satir 55-61: program_data()
// Format: "Program data: <base64_1> <base64_2> ..."
pub fn program_data(log_collector: &Option<Rc<RefCell<LogCollector>>>, data: &[&[u8]])

// Satir 73-84: program_return()
// Format: "Program return: <program-id> <base64>"
pub fn program_return(
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
    program_id: &Pubkey,
    data: &[u8],
)

// Satir 93-95: program_success()
// Format: "Program <address> success"
pub fn program_success(log_collector: &Option<Rc<RefCell<LogCollector>>>, program_id: &Pubkey)

// Satir 104-110: program_failure()
// Format: "Program <address> failed: <error>"
pub fn program_failure<E: std::fmt::Display>(
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
    program_id: &Pubkey,
    err: &E,
)
```

---

## 10. message_processor.rs — Satir 1-690

### MessageProcessor Struct (Satir 26-27)

```rust
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct MessageProcessor {}
```

Bos struct — durum tutmaz. Tum islevsellik `process_message()` static metodu ile saglanir.

### MessageProcessor::process_message() (Satir 45-169)

```rust
pub fn process_message(
    message: &SanitizedMessage,           // Islenecek mesaj
    program_indices: &[Vec<IndexOfAccount>], // Her instruction icin program indeksleri
    transaction_context: &mut TransactionContext,
    log_collector: Option<Rc<RefCell<LogCollector>>>,
    programs_loaded_for_tx_batch: &LoadedProgramsForTxBatch,
    programs_modified_by_tx: &mut LoadedProgramsForTxBatch,
    feature_set: Arc<FeatureSet>,
    compute_budget: ComputeBudget,
    timings: &mut ExecuteTimings,
    sysvar_cache: &SysvarCache,
    blockhash: Hash,
    lamports_per_signature: u64,
    accumulated_consumed_units: &mut u64,  // TX seviyesi toplam CU
) -> Result<(), TransactionError> {
    // InvokeContext olustur
    let mut invoke_context = InvokeContext::new(...);

    // Her instruction icin:
    for (instruction_index, ((program_id, instruction), program_indices)) in ... {
        let is_precompile = is_precompile(program_id, |id| invoke_context.feature_set.is_active(id));

        // Ozel: instructions sysvar'ini guncelle
        if let Some(account_index) = invoke_context.transaction_context
            .find_index_of_account(&instructions::id()) {
            instructions::store_current_index(
                mut_account_ref.data_as_mut_slice(),
                instruction_index as u16,
            );
        }

        // Instruction hesap listesini hazirla (deduplication)
        let mut instruction_accounts = Vec::with_capacity(instruction.accounts.len());
        for (ix_account_idx, index_in_transaction) in instruction.accounts.iter().enumerate() {
            let index_in_callee = /* duplicate index hesapla */;
            instruction_accounts.push(InstructionAccount { ... });
        }

        let result = if is_precompile {
            // Precompile (secp256k1, ed25519): sadece stack push/pop
            invoke_context.transaction_context.get_next_instruction_context()
                .map(|ic| ic.configure(program_indices, ...))
                .and_then(|_| {
                    invoke_context.transaction_context.push()?;
                    invoke_context.transaction_context.pop()
                })
        } else {
            // Normal program: process_instruction cagir
            let mut time = Measure::start("execute_instruction");
            let mut compute_units_consumed = 0;
            let result = invoke_context.process_instruction(
                &instruction.data,
                &instruction_accounts,
                program_indices,
                &mut compute_units_consumed,
                timings,
            );
            time.stop();

            // Metrikleri guncelle
            *accumulated_consumed_units += compute_units_consumed;
            timings.details.accumulate_program(program_id, time.as_us(), compute_units_consumed, result.is_err());
            invoke_context.timings = { timings.details.accumulate(&invoke_context.timings); Default::default() };
            result
        };

        result.map_err(|err| TransactionError::InstructionError(instruction_index as u8, err))?;
    }
    Ok(())
}
```

**Kritik Tasarim:**
- Precompile'lar sadece push/pop yapar, gercekten calistirilmaz (host tarafinda dogrulanir)
- Her instruction'da `instructions::id()` hesabi guncellenir
- Zamanlama metrikleri instruction bazinda toplantir

---

## 11. runtime_config.rs — Satir 1-18

```rust
#[derive(Debug, Default, Clone)]
pub struct RuntimeConfig {
    pub compute_budget: Option<ComputeBudget>,           // Override compute budget
    pub log_messages_bytes_limit: Option<usize>,         // Override log limiti
    pub transaction_account_lock_limit: Option<usize>,   // Override hesap kilitleme limiti
}
```

Test ve benchmark icin runtime davranisini degistirme altyapisi.

---

## 12. prioritization_fee.rs — Satir 1-115

### PrioritizationFeeType Enum (Satir 6-8)

```rust
pub enum PrioritizationFeeType {
    ComputeUnitPrice(u64),  // micro-lamport/CU
}
```

### PrioritizationFeeDetails Struct (Satir 10-14)

```rust
#[derive(Default, Debug, PartialEq, Eq)]
pub struct PrioritizationFeeDetails {
    fee: u64,                // Hesaplanan toplam ucret (lamport)
    compute_unit_price: u64, // CU basina micro-lamport
}
```

### PrioritizationFeeDetails::new() — Ucret Hesabi (Satir 17-34)

```rust
pub fn new(fee_type: PrioritizationFeeType, compute_unit_limit: u64) -> Self {
    match fee_type {
        PrioritizationFeeType::ComputeUnitPrice(compute_unit_price) => {
            let micro_lamport_fee: MicroLamports =
                (compute_unit_price as u128).saturating_mul(compute_unit_limit as u128);
            // Yukari yuvarla: (fee + 999_999) / 1_000_000
            let fee = micro_lamport_fee
                .saturating_add(MICRO_LAMPORTS_PER_LAMPORT.saturating_sub(1) as u128)
                .checked_div(MICRO_LAMPORTS_PER_LAMPORT as u128)
                .and_then(|fee| u64::try_from(fee).ok())
                .unwrap_or(u64::MAX);  // Overflow -> maksimum ucret
            Self { fee, compute_unit_price }
        }
    }
}
```

**Hesaplama:** `fee = ceil(price * cu_limit / 1_000_000)`

Ornekler:
- `price=999_999, cu_limit=1` -> `fee=1` (yukarı yuvarlama)
- `price=1_000_000, cu_limit=1` -> `fee=1`
- `price=1_000_001, cu_limit=1` -> `fee=2`
- `price=200, cu_limit=100_000` -> `fee=20`

---

## Genel Mimarinin Ozeti

### Execution Akisi

```
MessageProcessor::process_message()
  |
  +--> InvokeContext::new()
  |
  +--> Her instruction icin:
        |
        +--> [Precompile] push/pop (yalnizca)
        |
        +--> [Normal] InvokeContext::process_instruction()
              |
              +--> TransactionContext'i hazirla
              +--> push() — stack'e ekle, reentrancy kontrol
              +--> process_executable_chain()
                    |
                    +--> Builtin ID bul (owner ya da kendisi)
                    +--> find() ile LoadedProgramsForTxBatch'ten program al
                    +--> EbpfVm::invoke_function() [UNSAFE lifetime transmute]
                    +--> CU tuketimi hesapla
                    +--> stable_log yaz
              +--> pop() — mutlaka (hata olsa bile)
```

### CPI Derinlik Kontrol Mekanizmasi

```
max_invoke_stack_height = 5 (varsayilan)
Stack: [tx_level=1, cpi_1=2, cpi_2=3, cpi_3=4, cpi_4=5]
            5. seviyede baska CPI -> TransactionContext push() HATA
```

### Program Cache Hiyerarsisi

```
ProgramCache (global, fork-aware)
    |
    +--> HashMap<Pubkey, SecondLevel>
              |
              +--> slot_versions: Vec<Arc<LoadedProgram>> (sirali)
              +--> cooperative_loading_lock: Option<(Slot, ThreadId)>

LoadedProgramsForTxBatch (TX batch yerel gorunumu)
    |
    +--> HashMap<Pubkey, Arc<LoadedProgram>>
    +--> slot: Slot
    +--> environments: ProgramRuntimeEnvironments
```

---

## Nova Shield Chain icin Dersler

### 1. VM Runtime Tasarimi

**Solana Pattern:**
- `InvokeContext` tek bir yapida tum runtime durumunu barindiriyor
- `RefCell<u64>` ile ic mutabilite (compute_meter)
- Lifetime `'a` ile borç kontrolu; UNSAFE `transmute` zorunlu olduğunda kullanılıyor

**Nova Shield Onerileri:**
- VM context struct'inda tum shared state (log, budget, sysvar) tutun
- CU sayacini `RefCell` veya `AtomicU64` ile yonetin
- CPI guvenlik kontrollerini `push()` aninda yapin (reentrancy + privilege escalation)
- Her CPI push basardiktan sonra POP'u garantilemek icin `.and()` kullanin (`.and_then()` degil)

### 2. Compute Unit Sistemi

**Solana'nin Yaklasimiyle:**
- Tum kriptografik islemler icin onceden benchmark edilmis CU degerleri
- Karmasik operasyonlar (pairing, MSM) icin dogru maliyet belirlenmeli
- Poseidon icin kuadratik maliyet fonksiyonu (61n^2 + 542) — ZK kullanicilari icin referans
- Heap maliyeti: 8 CU/32KB sayfa

**Nova Shield ZK sistemi icin:**
- Groth16 proof dogrulama: bn254 pairing maliyeti referans alinabilir (36k+ CU)
- BLS12-381 operasyonlari icin kendi benchmark'larini yaparak CU belirle
- Poseidon kullaniyorsan ayni kuadratik formulu kullan

### 3. Program Cache Tasarimi

**Solana'nin Guc Noktalari:**
- Fork-aware iki seviyeli index
- `decayed_usage_counter()` ile bit-shift tabanli LRU
- "2's random selection" eviction — O(1) ama adil
- Tombstone'lar: baskansiz yuklemeleri onler
- Cooperative loading: ayni programa ihtiyac duyan batchleri koordine eder
- MAX_LOADED_ENTRY_COUNT = 256 ile bellek siniri

**Nova Shield icin:**
- Baslangiçta tombstone destekli basit bir `HashMap<ProgramId, CachedProgram>` yeterli
- Eviction icin `decayed_usage_counter` uygulanabilir
- Program cache'in epoch gecislerinde degismesi (environment update) kritik — feature flag sisteminizle entegre edin

### 4. CPI Guvenlik Kontrolleri

Solana'nin `prepare_instruction()` icin uygulanan guvenlik kurallari:
- Readonly -> Writable escalation yasak
- Imzasiz -> Imzali escalation yasak (PDA haric)
- Program executable olmali
- Duplicate hesaplarda privilege OR'lanir

**Nova Shield'e dogrudan kopyalanabilir guvenlik modeli.**

### 5. Log Sistemi Tasarimi

- 10KB log limiti (bayt bazli, mesaj sayisi degil)
- "Log truncated" mesaji bir kez eklenir
- `stable_log` formatlari degistirilemez (downstream API)
- `Rc<RefCell<LogCollector>>` — single-threaded, paylasilabilir sahiplik

### 6. Prioritization Fee Hesabi

```rust
fee = ceil(price_micro_lamports * compute_units / 1_000_000)
```

- `u128` ile hesaplama (overflow onleme)
- Yukari yuvarlama (kullanici lehine degil, validator lehine)
- Overflow durumunda `u64::MAX`

### 7. Unsafe Kullanimi

Solana'da iki kritik unsafe:
1. `transmute<&mut InvokeContext, &mut InvokeContext>` — lifetime silme (SBPF VM entegrasyonu icin gecici)
2. `reload()` — verifier atlama (onceden dogrulanmis program reload)

**Nova Shield:** Unsafe kullanimi mutlaka dokumante edilmeli ve guvenlik invariant'lari net aciklanmali.

---

## Ozet Istatistikler

| Dosya | Satir | Kritik Yapilar |
|-------|-------|----------------|
| invoke_context.rs | 1202 | InvokeContext, BpfAllocator, SyscallContext |
| loaded_programs.rs | 2668 | ProgramCache, LoadedProgram, LoadedProgramType |
| compute_budget.rs | 221 | ComputeBudget (45+ alan) |
| compute_budget_processor.rs | 500 | ComputeBudgetLimits, process_compute_budget_instructions |
| sysvar_cache.rs | 315 | SysvarCache (9 sysvar) |
| timings.rs | 552 | ExecuteTimings, ProgramTiming, 3 nested struct |
| log_collector.rs | 123 | LogCollector, ic_msg!, ic_logger_msg! |
| stable_log.rs | 111 | 6 stable log fonksiyonu |
| message_processor.rs | 690 | MessageProcessor::process_message() |
| runtime_config.rs | 17 | RuntimeConfig |
| prioritization_fee.rs | 115 | PrioritizationFeeDetails |
| lib.rs | 24 | Modul bildirimleri |
| **TOPLAM** | **~3870** | |
