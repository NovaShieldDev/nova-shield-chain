//! GÖREV 80: cargo-fuzz — işlem ayrıştırma fuzzing
//!
//! Hedef: NetworkMessage decode rastgele baytlarla çökmemeli.
//! Çalıştır: cargo +nightly fuzz run fuzz_tx_parse

#![no_main]
use libfuzzer_sys::fuzz_target;
use nova_network::NetworkMessage;

fuzz_target!(|data: &[u8]| {
    // Rastgele baytlardan NetworkMessage decode — panik olmamalı
    let _ = NetworkMessage::decode(data);

    // En az 33 bayt varsa yapısal decode dene
    if data.len() >= 33 {
        use nova_network::{MessageType, NetworkMessage};

        let msg_type_byte = data[0] % 8;
        let msg_type = match msg_type_byte {
            0 => MessageType::Gossip,
            1 => MessageType::Block,
            2 => MessageType::Transaction,
            3 => MessageType::MpcDecryptionShare,
            4 => MessageType::EonPublicKey,
            5 => MessageType::MpcCommitment,
            6 => MessageType::Ping,
            _ => MessageType::Pong,
        };

        let sender_bytes: [u8; 32] = {
            let mut b = [0u8; 32];
            if data.len() >= 33 {
                b.copy_from_slice(&data[1..33]);
            }
            b
        };

        let payload = data[33..].to_vec();

        let msg = NetworkMessage::new(msg_type, sender_bytes, payload);

        // encode-decode round-trip
        let encoded = msg.encode();
        if let Ok(decoded) = NetworkMessage::decode(&encoded) {
            assert_eq!(decoded.msg_type, msg.msg_type);
            assert_eq!(decoded.payload, msg.payload);
        }
    }
});
