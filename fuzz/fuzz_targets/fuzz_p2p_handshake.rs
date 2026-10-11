//! GÖREV 80: cargo-fuzz — P2P el sıkışma fuzzing
//!
//! Hedef: HandshakeMessage deserializasyonu rastgele baytlarla çökmemeli.
//! Çalıştır: cargo +nightly fuzz run fuzz_p2p_handshake

#![no_main]
use libfuzzer_sys::fuzz_target;
use nova_network::{HandshakeMessage, HandshakeValidator, PeerRole};

fuzz_target!(|data: &[u8]| {
    // 1. JSON deserializasyonu rastgele baytlarla
    if let Ok(s) = std::str::from_utf8(data) {
        let _: Result<HandshakeMessage, _> = serde_json::from_str(s);
    }

    // 2. HandshakeValidator ile doğrulama
    let genesis = [0xABu8; 32];
    let validator = HandshakeValidator::new("/nova-shield/1.0", genesis);

    if data.len() >= 32 {
        let mut node_id = [0u8; 32];
        node_id.copy_from_slice(&data[..32]);

        // addr string'i fuzz verisinden oluştur (UTF-8 güvenli)
        let addr = if data.len() > 32 {
            String::from_utf8_lossy(&data[32..]).to_string()
        } else {
            "127.0.0.1:30333".to_string()
        };

        let msg = HandshakeMessage::new(
            node_id,
            addr,
            "nova-shield/0.1.0",
            "/nova-shield/1.0",
            genesis,
            PeerRole::Full,
        );

        // Doğrulama — panik üretmemeli
        let _ = validator.validate(&msg);
    }

    // 3. Yanlış genesis hash ile doğrulama — hata döndürmeli, panic değil
    if data.len() >= 32 {
        let mut node_id = [0u8; 32];
        node_id.copy_from_slice(&data[..32]);

        // Farklı genesis hash ile sahte mesaj
        let wrong_genesis = [0xFFu8; 32];
        let msg = HandshakeMessage::new(
            node_id,
            "10.0.0.1:30333",
            "nova-shield/0.1.0",
            "/nova-shield/1.0",
            wrong_genesis,
            PeerRole::Validator,
        );

        // Hata döndürmeli (panic değil)
        let result = validator.validate(&msg);
        assert!(result.is_err(), "yanlış genesis ile başarı beklenmiyor");
    }
});
