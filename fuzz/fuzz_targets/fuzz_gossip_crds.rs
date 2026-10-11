//! Gossip CRDS mesajlarına kötü/rastgele veri fuzz testi.
//!
//! Hedef: Herhangi bir byte dizisi için CRDS parse paniğe yol açmaz.
//! Çalıştırma: `cargo fuzz run fuzz_gossip_crds`
#![no_main]

use libfuzzer_sys::fuzz_target;
use nova_network::gossip::{
    CrdsTable, CrdsValue, CrdsData,
    pull::{CrdsGossipPull, PullResponse},
};
use nova_primitives::SlotId;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    // CrdsTable'a rastgele değerler eklemeye çalış
    let mut table = CrdsTable::new();

    // İlk 32 byte: kaynak node ID
    let node_id: [u8; 32] = if data.len() >= 32 {
        data[..32].try_into().unwrap_or([0u8; 32])
    } else {
        let mut buf = [0u8; 32];
        buf[..data.len()].copy_from_slice(data);
        buf
    };

    // CrdsData oluştur: rastgele slot ve value
    let slot = SlotId(
        u64::from_le_bytes(
            data.get(32..40).and_then(|s| s.try_into().ok()).unwrap_or([0u8; 8])
        )
    );

    // CRDS value: ContactInfo, Vote veya custom — serde ile deserialize dene
    // Doğrudan JSON fuzz: rastgele byte'ları JSON olarak parse etmeye çalış
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = serde_json::from_str::<CrdsValue>(s);
    }

    // Pull gossip: rastgele node için pull request oluştur
    let mut pull = CrdsGossipPull::new(node_id);
    let wallclock = u64::from_le_bytes(
        data.get(40..48).and_then(|s| s.try_into().ok()).unwrap_or([0u8; 8])
    );
    let _ = pull.build_pull_request(&table, wallclock);

    // Rastgele CRDS değer ekleme
    if data.len() >= 64 {
        let origin: [u8; 32] = data[..32].try_into().unwrap_or([0u8; 32]);
        let value_data = &data[32..];
        // JSON string olarak parse et ve table'a eklemeye çalış
        if let Ok(s) = std::str::from_utf8(value_data) {
            if let Ok(val) = serde_json::from_str::<CrdsValue>(s) {
                table.insert(val);
            }
        }
    }
});
