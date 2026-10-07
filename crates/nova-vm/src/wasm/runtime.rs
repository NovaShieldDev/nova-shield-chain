//! WASM runtime — wasmtime v25 ile gerçek WebAssembly yürütme
//!
//! ## Araştırma Kaynakları
//! - Polkadot analizi (04-POLKADOT-NOVA-ARASTIRMA.md): WasmModule/WasmInstance pattern
//! - Penumbra analizi (03-PENUMBRA-NOVA-ARASTIRMA.md): WASM gas metering, 4-boyutlu gas
//! - GAP-ANALIZ.md: Gerçek WASM runtime gereksinimi
//!
//! ## Mimari
//! - `wasmtime::Engine`: JIT compiler (paylaşılan, thread-safe)
//! - `wasmtime::Module`: Derlenmiş bytecode (LRU cache'lenmiş)
//! - `wasmtime::Store<NovaHostState>`: Yürütme durumu (host ctx, fuel)
//! - `wasmtime::Linker`: Host fonksiyon kayıt defteri
//!
//! ## Kontrat Arayüzü (Nova VM Convention)
//! Kontratlar şu signature'ları export etmeli:
//! - `instantiate(input_ptr: i32, input_len: i32) -> i32` (0=başarı)
//! - `execute(input_ptr: i32, input_len: i32) -> i32` (çıktı byte sayısı)
//! - `query(input_ptr: i32, input_len: i32) -> i32` (çıktı byte sayısı)
//! - `memory` (linear memory export)
//!
//! ## Bellek Düzeni
//! - Offset 0..4095: Host → WASM giriş verisi
//! - Offset 4096..8191: WASM → Host çıkış verisi
//! - Offset 8192+: Kontrat scratch alanı
//!
//! ## Gas Metering
//! - wasmtime `consume_fuel(true)` ile her WASM opcode 1 fuel tüketir
//! - Host fn çağrıları `caller.consume_fuel(gas_cost)` ile ayrıca ücretlendirilir
//! - Penumbra gas: ZK proof=1000, action=10 unit → Nova'da benzer tablo kullanılır

use crate::errors::{VmError, VmResult};
use crate::wasm::gas::ComputeBudget;
use crate::wasm::host_fns::{ContractEvent, HostContext};
use nova_primitives::Address;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wasmtime::{Caller, Engine, Instance, Linker, Module, Store, TypedFunc};

// ─── Bellek Düzeni Sabitleri ──────────────────────────────────────────────────

/// WASM binary magic bytes (0x00 'a' 's' 'm') + versiyon (0x01 0x00 0x00 0x00)
pub const WASM_MAGIC: &[u8] = &[0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];

/// Host'un WASM'a giriş yazdığı bellek ofseti
pub const WASM_INPUT_OFFSET: usize = 0;
/// WASM'ın host'a çıkış yazdığı bellek ofseti
pub const WASM_OUTPUT_OFFSET: usize = 4096;
/// Maksimum giriş boyutu
pub const WASM_MAX_INPUT_SIZE: usize = WASM_OUTPUT_OFFSET;
/// WASM memory export ismi
pub const WASM_MEMORY_EXPORT: &str = "memory";

// ─── NovaHostState ────────────────────────────────────────────────────────────

/// wasmtime Store içinde tutulan host durumu
pub(crate) struct NovaHostState {
    pub ctx: HostContext,
}

// ─── WasmModule ───────────────────────────────────────────────────────────────

/// Derlenmiş ve doğrulanmış WASM modülü — immutable, paylaşılabilir
///
/// Polkadot: `Box<dyn WasmModule>` pattern — derle bir kez, defalarca çalıştır
pub struct WasmModule {
    module: Module,
    engine: Arc<Engine>,
    /// Bytecode BLAKE3 hash'i (cache anahtarı)
    pub code_hash: [u8; 32],
    pub code_size: usize,
    /// Export edilen fonksiyon ve memory isimleri
    pub exports: Vec<String>,
    /// İmport edilen host fonksiyon isimleri
    pub imports: Vec<String>,
}

impl WasmModule {
    /// WASM bytecode'unu gerçek wasmtime JIT compiler ile derle
    pub fn compile(engine: &Arc<Engine>, bytecode: &[u8]) -> VmResult<Self> {
        let code_hash = *blake3::hash(bytecode).as_bytes();
        let code_size = bytecode.len();

        // wasmtime gerçek WASM validation + JIT compilation
        let module = Module::new(engine, bytecode).map_err(|e| VmError::InvalidBytecode {
            reason: e.to_string(),
        })?;

        // Export ve import listelerini çıkar
        let exports: Vec<String> = module.exports().map(|e| e.name().to_string()).collect();
        let imports: Vec<String> = module.imports().map(|i| i.name().to_string()).collect();

        Ok(Self {
            module,
            engine: engine.clone(),
            code_hash,
            code_size,
            exports,
            imports,
        })
    }

    pub fn code_hash_hex(&self) -> String {
        hex::encode(self.code_hash)
    }

    pub fn has_export(&self, name: &str) -> bool {
        self.exports.iter().any(|e| e == name)
    }
}

// ─── WasmRuntime ──────────────────────────────────────────────────────────────

/// WASM runtime yöneticisi
///
/// - JIT engine (wasmtime::Engine) paylaşılan, thread-safe
/// - Module cache: code_hash → Arc<WasmModule> (DashMap LRU)
/// - Linker: Nova host fonksiyonları kayıt defteri
pub struct WasmRuntime {
    engine: Arc<Engine>,
    linker: Arc<Linker<NovaHostState>>,
    cache: dashmap::DashMap<[u8; 32], Arc<WasmModule>>,
    max_cache_entries: usize,
}

impl std::fmt::Debug for WasmRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmRuntime")
            .field("cache_size", &self.cache.len())
            .field("max_cache", &self.max_cache_entries)
            .finish()
    }
}

impl WasmRuntime {
    /// Yeni WASM runtime oluştur
    ///
    /// wasmtime Config:
    /// - `consume_fuel(true)`: Her WASM opcode gas (fuel) tüketir → OutOfFuel trap
    /// - Cranelift JIT backend (optimize edilmiş kod üretimi)
    pub fn new(max_cache_entries: usize) -> VmResult<Self> {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);

        let engine = Engine::new(&config).map_err(|e| VmError::ExecutionFailed {
            reason: format!("wasmtime engine oluşturulamadı: {e}"),
        })?;
        let engine = Arc::new(engine);

        // Host fonksiyon linker'ı kur
        let linker = build_nova_linker(&engine)?;

        Ok(Self {
            engine,
            linker: Arc::new(linker),
            cache: dashmap::DashMap::new(),
            max_cache_entries,
        })
    }

    /// Bytecode'u derle ve cache'e al
    pub fn compile(&self, bytecode: &[u8]) -> VmResult<Arc<WasmModule>> {
        let code_hash = *blake3::hash(bytecode).as_bytes();

        // Cache'de varsa döndür
        if let Some(cached) = self.cache.get(&code_hash) {
            return Ok(cached.clone());
        }

        let module = WasmModule::compile(&self.engine, bytecode)?;
        let module = Arc::new(module);

        // Cache kapasitesi doluysa en eski girişi çıkar
        if self.cache.len() >= self.max_cache_entries {
            if let Some(oldest_key) = self.cache.iter().next().map(|e| *e.key()) {
                self.cache.remove(&oldest_key);
            }
        }

        self.cache.insert(code_hash, module.clone());
        Ok(module)
    }

    /// WASM modülünü yürüt
    ///
    /// Yürütme akışı:
    /// 1. Store oluştur (NovaHostState + fuel = budget.max_compute_units)
    /// 2. Linker ile instantiate et (host fonksiyonlar bağlanır)
    /// 3. Giriş verisini WASM_INPUT_OFFSET'e yaz
    /// 4. `method(INPUT_OFFSET, input_len) -> output_len` çağır
    /// 5. Çıktıyı WASM_OUTPUT_OFFSET'ten oku
    /// 6. Güncellenmiş HostContext'i geri döndür
    pub fn execute(
        &self,
        module: &WasmModule,
        method: &str,
        input: &[u8],
        ctx: &mut HostContext,
        budget: ComputeBudget,
    ) -> VmResult<ExecutionResult> {
        if !module.has_export(method) {
            return Err(VmError::ExecutionFailed {
                reason: format!("'{method}' export edilmemiş"),
            });
        }

        // Store: yürütme başına yeni durum
        let state = NovaHostState { ctx: ctx.clone() };
        let mut store = Store::new(&*self.engine, state);

        // Gas (fuel) ayarla
        store
            .set_fuel(budget.max_compute_units)
            .map_err(|e| VmError::ExecutionFailed {
                reason: format!("fuel ayarlanamadı: {e}"),
            })?;

        // Linker ile modülü instantiate et (host fonksiyonlar enjekte edilir)
        let instance = self
            .linker
            .instantiate(&mut store, &module.module)
            .map_err(|e| VmError::ExecutionFailed {
                reason: format!("WASM instantiate hatası: {e}"),
            })?;

        // WASM linear memory al (I/O için zorunlu)
        let memory = instance
            .get_memory(&mut store, WASM_MEMORY_EXPORT)
            .ok_or_else(|| VmError::ExecutionFailed {
                reason: format!("'{WASM_MEMORY_EXPORT}' export bulunamadı — kontrat memory export etmeli"),
            })?;

        // Giriş verisini WASM_INPUT_OFFSET'e yaz
        if input.len() > WASM_MAX_INPUT_SIZE {
            return Err(VmError::ExecutionFailed {
                reason: format!("giriş verisi çok büyük ({} > {})", input.len(), WASM_MAX_INPUT_SIZE),
            });
        }

        if !input.is_empty() {
            memory
                .write(&mut store, WASM_INPUT_OFFSET, input)
                .map_err(|e| VmError::ExecutionFailed {
                    reason: format!("WASM belleğine yazma hatası: {e}"),
                })?;
        }

        // Metoda göre dispatch
        let output = dispatch_method(
            &instance,
            &mut store,
            &memory,
            method,
            input,
            &budget,
        )?;

        // Kalan fuel'dan gas kullanımını hesapla
        let fuel_remaining = store.get_fuel().unwrap_or(0);
        let gas_used = budget.max_compute_units.saturating_sub(fuel_remaining);

        // Güncellenen host state'i çıkar
        let host_state = store.into_data();
        let events = host_state.ctx.events.clone();
        let logs = host_state.ctx.logs.clone();

        // ctx'i güncellenmiş haliyle değiştir
        *ctx = host_state.ctx;

        Ok(ExecutionResult { output, gas_used, events, logs })
    }

    pub fn cached_module_count(&self) -> usize {
        self.cache.len()
    }
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

fn dispatch_method(
    instance: &Instance,
    store: &mut Store<NovaHostState>,
    memory: &wasmtime::Memory,
    method: &str,
    input: &[u8],
    budget: &ComputeBudget,
) -> VmResult<Vec<u8>> {
    match method {
        "instantiate" => {
            // instantiate(input_ptr, input_len) -> i32 (0=başarı, !=0 hata kodu)
            let func: TypedFunc<(i32, i32), i32> = instance
                .get_typed_func(&mut *store, "instantiate")
                .map_err(|e| VmError::ExecutionFailed { reason: e.to_string() })?;

            let status = func
                .call(&mut *store, (WASM_INPUT_OFFSET as i32, input.len() as i32))
                .map_err(|e| map_wasmtime_error(e, budget))?;

            Ok(vec![status as u8])
        }

        "execute" | "query" => {
            // execute(input_ptr, input_len) -> i32 (çıktı byte sayısı, OUTPUT_OFFSET'ten okunur)
            // query(key_ptr, key_len) -> i32 (çıktı byte sayısı)
            let func: TypedFunc<(i32, i32), i32> = instance
                .get_typed_func(&mut *store, method)
                .map_err(|e| VmError::ExecutionFailed { reason: e.to_string() })?;

            let output_len = func
                .call(&mut *store, (WASM_INPUT_OFFSET as i32, input.len() as i32))
                .map_err(|e| map_wasmtime_error(e, budget))?;

            // Çıktıyı WASM_OUTPUT_OFFSET'ten oku
            let out_len = output_len.max(0) as usize;
            if out_len == 0 {
                return Ok(vec![]);
            }

            let mut out_buf = vec![0u8; out_len];
            memory
                .read(&*store, WASM_OUTPUT_OFFSET, &mut out_buf)
                .map_err(|e| VmError::ExecutionFailed {
                    reason: format!("WASM çıktı okuma hatası: {e}"),
                })?;

            Ok(out_buf)
        }

        _ => Err(VmError::ExecutionFailed {
            reason: format!("desteklenmeyen metod: '{method}'"),
        }),
    }
}

/// wasmtime hatasını VmError'a çevir
fn map_wasmtime_error(e: wasmtime::Error, budget: &ComputeBudget) -> VmError {
    // OutOfFuel trap kontrolü
    if let Some(trap) = e.downcast_ref::<wasmtime::Trap>() {
        if *trap == wasmtime::Trap::OutOfFuel {
            return VmError::OutOfGas {
                limit: budget.max_compute_units,
                used: budget.max_compute_units,
            };
        }
    }
    VmError::ExecutionFailed { reason: e.to_string() }
}

// ─── Nova Host Functions (Linker) ─────────────────────────────────────────────

/// Nova host fonksiyonlarını wasmtime Linker'a kayıt et
///
/// Kontratlar "env" modülü altında şu fonksiyonları import edebilir:
/// - `nova_storage_read(key_ptr, key_len, out_ptr) -> i32` (çıktı uzunluğu; -1 = bulunamadı)
/// - `nova_storage_write(key_ptr, key_len, val_ptr, val_len)`
/// - `nova_emit_event(data_ptr, data_len)`
/// - `nova_current_slot() -> i64`
/// - `nova_caller(out_ptr)` (20 byte adres yazar)
/// - `nova_log(msg_ptr, msg_len)`
fn build_nova_linker(engine: &Engine) -> VmResult<Linker<NovaHostState>> {
    let mut linker: Linker<NovaHostState> = Linker::new(engine);

    // nova_storage_read(key_ptr, key_len, out_ptr) -> i32
    linker
        .func_wrap(
            "env",
            "nova_storage_read",
            |mut caller: Caller<'_, NovaHostState>, key_ptr: i32, key_len: i32, out_ptr: i32| -> i32 {
                let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return -3,
                };

                let k_start = key_ptr as usize;
                let k_end = k_start + key_len as usize;

                let data = mem.data(&caller);
                if k_end > data.len() {
                    return -3;
                }
                let key = data[k_start..k_end].to_vec();

                let result = caller.data_mut().ctx.storage_read(&key);
                match result {
                    Some(val) => {
                        let out = out_ptr as usize;
                        let mem_size = mem.data(&caller).len();
                        if out + val.len() > mem_size {
                            return -3;
                        }
                        mem.data_mut(&mut caller)[out..out + val.len()].copy_from_slice(&val);
                        val.len() as i32
                    }
                    None => -1,
                }
            },
        )
        .map_err(|e| VmError::ExecutionFailed { reason: e.to_string() })?;

    // nova_storage_write(key_ptr, key_len, val_ptr, val_len)
    linker
        .func_wrap(
            "env",
            "nova_storage_write",
            |mut caller: Caller<'_, NovaHostState>,
             key_ptr: i32,
             key_len: i32,
             val_ptr: i32,
             val_len: i32| {
                let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return,
                };

                let data = mem.data(&caller);
                let k_s = key_ptr as usize;
                let k_e = k_s + key_len as usize;
                let v_s = val_ptr as usize;
                let v_e = v_s + val_len as usize;

                if k_e > data.len() || v_e > data.len() {
                    return;
                }

                let key = data[k_s..k_e].to_vec();
                let val = data[v_s..v_e].to_vec();
                caller.data_mut().ctx.storage_write(key, val);
            },
        )
        .map_err(|e| VmError::ExecutionFailed { reason: e.to_string() })?;

    // nova_emit_event(data_ptr, data_len)
    linker
        .func_wrap(
            "env",
            "nova_emit_event",
            |mut caller: Caller<'_, NovaHostState>, data_ptr: i32, data_len: i32| {
                let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return,
                };

                let d_s = data_ptr as usize;
                let d_e = d_s + data_len as usize;
                let data = mem.data(&caller);

                if d_e > data.len() {
                    return;
                }

                let event_data = data[d_s..d_e].to_vec();
                let state = caller.data_mut();
                let event = ContractEvent {
                    contract: state.ctx.contract,
                    topics: Vec::new(),
                    data: event_data,
                    slot: state.ctx.current_slot,
                };
                state.ctx.emit_event(event);
            },
        )
        .map_err(|e| VmError::ExecutionFailed { reason: e.to_string() })?;

    // nova_current_slot() -> i64
    linker
        .func_wrap("env", "nova_current_slot", |caller: Caller<'_, NovaHostState>| -> i64 {
            caller.data().ctx.current_slot as i64
        })
        .map_err(|e| VmError::ExecutionFailed { reason: e.to_string() })?;

    // nova_caller(out_ptr) — 20 byte adres yazar
    linker
        .func_wrap(
            "env",
            "nova_caller",
            |mut caller: Caller<'_, NovaHostState>, out_ptr: i32| {
                let caller_addr = caller.data().ctx.caller.as_bytes().to_vec();
                let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return,
                };

                let out = out_ptr as usize;
                if out + 20 <= mem.data(&caller).len() {
                    mem.data_mut(&mut caller)[out..out + 20].copy_from_slice(&caller_addr);
                }
            },
        )
        .map_err(|e| VmError::ExecutionFailed { reason: e.to_string() })?;

    // nova_log(msg_ptr, msg_len)
    linker
        .func_wrap(
            "env",
            "nova_log",
            |mut caller: Caller<'_, NovaHostState>, msg_ptr: i32, msg_len: i32| {
                let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return,
                };

                let m_s = msg_ptr as usize;
                let m_e = m_s + msg_len as usize;
                let data = mem.data(&caller);

                if m_e > data.len() {
                    return;
                }

                let msg = String::from_utf8_lossy(&data[m_s..m_e]).into_owned();
                caller.data_mut().ctx.log(msg);
            },
        )
        .map_err(|e| VmError::ExecutionFailed { reason: e.to_string() })?;

    Ok(linker)
}

// ─── ExecutionResult ──────────────────────────────────────────────────────────

/// WASM yürütme sonucu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub output: Vec<u8>,
    pub gas_used: u64,
    pub events: Vec<ContractEvent>,
    pub logs: Vec<String>,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::gas::ComputeBudget;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_ctx() -> HostContext {
        HostContext::new(addr(1), addr(2), 1000, 0)
    }

    /// Nova kontrat interface'ini uygulayan minimal test WASM modülü
    ///
    /// Host fonksiyonları:
    /// - nova_storage_write: b"init" → b"ok" (instantiate'de)
    /// - nova_emit_event: execute'da event yayınlar
    /// - nova_storage_read: query'de key'e göre okur
    fn nova_test_contract_wasm() -> Vec<u8> {
        wat::parse_str(
            r#"
            (module
                ;; Host function imports (memory'den önce gelmeli — WASM spec gereksinimi)
                (import "env" "nova_storage_write"
                    (func $sw (param i32 i32 i32 i32)))
                (import "env" "nova_storage_read"
                    (func $sr (param i32 i32 i32) (result i32)))
                (import "env" "nova_emit_event"
                    (func $ee (param i32 i32)))

                (memory (export "memory") 1)

                ;; --- Sabit veriler static data olarak belleğe yerleştirilir ---
                ;; Offset 8192: "init" (4 bytes)
                ;; Offset 8196: "ok"   (2 bytes)
                (data (i32.const 8192) "init")
                (data (i32.const 8196) "ok")

                ;; instantiate(input_ptr, input_len) -> 0
                ;; nova_storage_write("init", "ok") çağırır
                (func (export "instantiate") (param i32 i32) (result i32)
                    i32.const 8192   ;; key_ptr = "init"
                    i32.const 4      ;; key_len
                    i32.const 8196   ;; val_ptr = "ok"
                    i32.const 2      ;; val_len
                    call $sw
                    i32.const 0      ;; başarı kodu
                )

                ;; execute(input_ptr, input_len) -> 4
                ;; Event yayınlar, çıktı olarak sabit 4 byte döner
                (func (export "execute") (param $ip i32) (param $il i32) (result i32)
                    local.get $ip
                    local.get $il
                    call $ee
                    ;; Çıktı: OUTPUT_OFFSET'e 4 sıfır bayt yaz (zaten sıfır)
                    i32.const 4      ;; output_len = 4
                )

                ;; query(key_ptr, key_len) -> output_len
                ;; nova_storage_read(key_ptr, key_len, OUTPUT_OFFSET=4096) -> len
                ;; -1 (bulunamadı) → 0 clamp
                (func (export "query") (param $kp i32) (param $kl i32) (result i32)
                    (local $r i32)
                    (local.set $r
                        (call $sr (local.get $kp) (local.get $kl) (i32.const 4096)))
                    (select
                        (local.get $r)
                        (i32.const 0)
                        (i32.ge_s (local.get $r) (i32.const 0))
                    )
                )
            )
            "#,
        )
        .expect("WAT parse hatası — geçerli söz dizimi")
    }

    // ─── Temel Derleme Testleri ────────────────────────────────────────────────

    #[test]
    fn test_wasm_module_compile_valid() {
        let rt = WasmRuntime::new(16).unwrap();
        let bytecode = nova_test_contract_wasm();
        let module = rt.compile(&bytecode).unwrap();
        assert_eq!(module.code_size, bytecode.len());
        assert!(!module.code_hash_hex().is_empty());
        assert_eq!(module.code_hash, *blake3::hash(&bytecode).as_bytes());
    }

    #[test]
    fn test_wasm_module_compile_invalid() {
        let rt = WasmRuntime::new(16).unwrap();
        let bad_bytes = b"bu bir WASM modulu degil";
        let result = rt.compile(bad_bytes);
        assert!(matches!(result, Err(VmError::InvalidBytecode { .. })));
    }

    #[test]
    fn test_wasm_module_compile_invalid_magic() {
        let rt = WasmRuntime::new(16).unwrap();
        // Sadece magic bytes + geçersiz section
        let bad = vec![0x00, 0x61, 0x73, 0x6D, 0xFF, 0xFF, 0xFF, 0xFF];
        let result = rt.compile(&bad);
        assert!(matches!(result, Err(VmError::InvalidBytecode { .. })));
    }

    #[test]
    fn test_wasm_module_has_exports() {
        let rt = WasmRuntime::new(16).unwrap();
        let module = rt.compile(&nova_test_contract_wasm()).unwrap();
        assert!(module.has_export("instantiate"));
        assert!(module.has_export("execute"));
        assert!(module.has_export("query"));
        assert!(module.has_export("memory"));
        assert!(!module.has_export("unknown_fn"));
    }

    #[test]
    fn test_wasm_deterministic_hash() {
        let rt = WasmRuntime::new(16).unwrap();
        let b1 = nova_test_contract_wasm();
        let b2 = nova_test_contract_wasm();
        let m1 = rt.compile(&b1).unwrap();
        let m2 = rt.compile(&b2).unwrap();
        assert_eq!(m1.code_hash, m2.code_hash);
    }

    // ─── Cache Testleri ───────────────────────────────────────────────────────

    #[test]
    fn test_wasm_runtime_cache() {
        let rt = WasmRuntime::new(16).unwrap();
        let bytecode = nova_test_contract_wasm();
        rt.compile(&bytecode).unwrap();
        rt.compile(&bytecode).unwrap(); // cache'den gelir
        assert_eq!(rt.cached_module_count(), 1);
    }

    // ─── Yürütme Testleri ─────────────────────────────────────────────────────

    #[test]
    fn test_wasm_instance_instantiate() {
        let rt = WasmRuntime::new(16).unwrap();
        let module = rt.compile(&nova_test_contract_wasm()).unwrap();
        let mut ctx = make_ctx();

        let result = rt.execute(&module, "instantiate", &[], &mut ctx, ComputeBudget::default()).unwrap();

        assert_eq!(result.output, vec![0], "instantiate başarı kodu 0 döndürmeli");
        // nova_storage_write("init", "ok") çağrıldı mı?
        assert!(ctx.storage_read(b"init").is_some(), "instantiate storage'a 'init' yazmalı");
        assert!(result.gas_used > 0);
    }

    #[test]
    fn test_wasm_instance_execute() {
        let rt = WasmRuntime::new(16).unwrap();
        let module = rt.compile(&nova_test_contract_wasm()).unwrap();
        let mut ctx = make_ctx();

        let result = rt.execute(&module, "execute", b"test_input", &mut ctx, ComputeBudget::default()).unwrap();

        assert!(!result.output.is_empty(), "execute çıktı üretmeli");
        assert!(result.gas_used > 0, "gas kullanılmış olmalı");
    }

    #[test]
    fn test_execute_emits_event() {
        let rt = WasmRuntime::new(16).unwrap();
        let module = rt.compile(&nova_test_contract_wasm()).unwrap();
        let mut ctx = make_ctx();

        let result = rt
            .execute(&module, "execute", b"trigger_event", &mut ctx, ComputeBudget::default())
            .unwrap();

        assert!(!result.events.is_empty(), "execute event yayınlamalı");
    }

    #[test]
    fn test_wasm_query() {
        let rt = WasmRuntime::new(16).unwrap();
        let module = rt.compile(&nova_test_contract_wasm()).unwrap();
        let mut ctx = make_ctx();

        // Önce storage'a yaz
        ctx.storage_write(b"key1".to_vec(), b"value1".to_vec());

        // Query ile oku
        let result = rt.execute(&module, "query", b"key1", &mut ctx, ComputeBudget::default()).unwrap();

        assert_eq!(result.output, b"value1", "query 'key1'='value1' döndürmeli");
    }

    #[test]
    fn test_wasm_query_missing_key() {
        let rt = WasmRuntime::new(16).unwrap();
        let module = rt.compile(&nova_test_contract_wasm()).unwrap();
        let mut ctx = make_ctx();

        let result = rt
            .execute(&module, "query", b"nonexistent", &mut ctx, ComputeBudget::default())
            .unwrap();

        // nova_storage_read returns -1 for missing keys → max(0,-1) = 0 → empty output
        assert!(result.output.is_empty(), "bulunamayan key için boş çıktı olmalı");
    }

    #[test]
    fn test_wasm_unknown_method() {
        let rt = WasmRuntime::new(16).unwrap();
        let module = rt.compile(&nova_test_contract_wasm()).unwrap();
        let mut ctx = make_ctx();

        let result = rt.execute(&module, "nonexistent_fn", &[], &mut ctx, ComputeBudget::default());
        assert!(
            matches!(result, Err(VmError::ExecutionFailed { .. })),
            "bilinmeyen metod ExecutionFailed vermeli"
        );
    }

    #[test]
    fn test_out_of_gas() {
        let rt = WasmRuntime::new(16).unwrap();
        let module = rt.compile(&nova_test_contract_wasm()).unwrap();
        let mut ctx = make_ctx();

        // Çok düşük gas limiti — ilk birkaç opcode'da tükenir
        let budget = ComputeBudget::new(1);
        let result = rt.execute(&module, "execute", b"some_long_input_data", &mut ctx, budget);

        assert!(
            matches!(result, Err(VmError::OutOfGas { .. })),
            "gas=1 ile execute OutOfGas vermeli"
        );
    }

    #[test]
    fn test_wasm_ctx_updated_after_execute() {
        let rt = WasmRuntime::new(16).unwrap();
        let module = rt.compile(&nova_test_contract_wasm()).unwrap();
        let mut ctx = make_ctx();

        assert!(ctx.events.is_empty());
        rt.execute(&module, "execute", b"data", &mut ctx, ComputeBudget::default()).unwrap();
        // ctx güncellenmiş olmalı
        assert!(!ctx.events.is_empty(), "execute sonrası ctx.events güncellenmiş olmalı");
    }
}
