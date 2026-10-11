//! JSON-RPC input fuzzing — rastgele/malformed JSON → server panic yok.
//!
//! Hedef: Her türlü JSON-RPC inputu için parse/validate güvenli hata döner.
//! Çalıştır: cargo +nightly fuzz run fuzz_rpc_json
#![no_main]
use libfuzzer_sys::fuzz_target;
use nova_rpc::types::{RpcRequest, RequestId};
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    // 1. Rastgele byte → JSON parse → RpcRequest deserialize
    if let Ok(s) = std::str::from_utf8(data) {
        // JSON-RPC string olarak
        let result: Result<RpcRequest, _> = serde_json::from_str(s);
        if let Ok(req) = result {
            // Geçerli JSON ama geçersiz RPC alanları olabilir — validate güvenli
            let _ = req.validate();
        }

        // Value olarak parse et — her zaman güvenli olmalı
        let _: Result<Value, _> = serde_json::from_str(s);
    }

    // 2. Yapısal fuzz: alanları byte'lardan oluştur
    if data.len() >= 8 {
        let method_len = (data[0] as usize % 32) + 1;
        let method = String::from_utf8_lossy(
            data.get(1..1 + method_len.min(data.len() - 1)).unwrap_or(b"test")
        ).to_string();

        let req = RpcRequest {
            jsonrpc: if data[0] & 1 == 0 { "2.0".to_string() } else { "1.0".to_string() },
            method,
            params: vec![],
            id: RequestId::Number(data[0] as u64),
        };

        // Validate: geçersiz version → InvalidRequest, ama panic yok
        let _ = req.validate();

        // JSON serialize + deserialize round-trip — her zaman güvenli
        if let Ok(json) = serde_json::to_string(&req) {
            let _: Result<RpcRequest, _> = serde_json::from_str(&json);
        }
    }

    // 3. Çok uzun method adı → DoS koruması
    if data.len() >= 2 {
        let long_method = "A".repeat(data[0] as usize * 100);
        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            method: long_method,
            params: vec![],
            id: RequestId::Null,
        };
        let _ = req.validate(); // Panic yok
    }
});
