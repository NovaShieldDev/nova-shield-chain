//! CRDS Pull Gossip — Solana anti-entropy pattern
//!
//! Solana `crds_gossip_pull.rs`:
//! - Bloom filtresi ile pull request
//! - `CrdsFilter` ile büyük CRDS bölümlere ayrılır
//! - `FALSE_RATE = 0.1`, `KEYS = 8.0`
//! - Peer'a "sahip olduklarım" Bloom filtresi gönder
//! - Peer "filtrede olmayan" değerleri gönderir
//!
//! Avalanchego pattern: `PullGossiper.Gossip(ctx)` → `AppRequestAny` → `handleResponse`

use crate::gossip::bloom::CrdsFilter;
use crate::gossip::crds::{CrdsTable, CrdsValue};
use serde::{Deserialize, Serialize};

// ─── PullRequest ──────────────────────────────────────────────────────────────

/// Pull isteği — Bloom filtresi gönder
/// Solana: `CrdsFilter` → gönder, eksikleri al
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub from: [u8; 32],
    pub filter: CrdsFilter,
    pub wallclock: u64,
}

// ─── PullResponse ─────────────────────────────────────────────────────────────

/// Pull yanıtı — eksik değerler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    pub from: [u8; 32],
    pub values: Vec<CrdsValue>,
}

// ─── CrdsGossipPull ───────────────────────────────────────────────────────────

/// CRDS Pull protokolü
/// Solana `CrdsGossipPull` benzeri
#[derive(Debug, Default)]
pub struct CrdsGossipPull {
    local_id: [u8; 32],
    /// Gönderilen pull istekleri (from → wallclock)
    pending_requests: std::collections::HashMap<[u8; 32], u64>,
    /// Maksimum poll sayısı — Avalanchego: pollSize
    poll_size: usize,
}

impl CrdsGossipPull {
    pub fn new(local_id: [u8; 32]) -> Self {
        Self {
            local_id,
            pending_requests: std::collections::HashMap::new(),
            poll_size: 1,
        }
    }

    pub fn with_poll_size(mut self, poll_size: usize) -> Self {
        self.poll_size = poll_size;
        self
    }

    /// Pull isteği oluştur — yerel CRDS'deki değerlerin Bloom filtresini gönder
    /// Solana: `build_crds_filter(crds, bloom_size)` → `CrdsFilter`
    pub fn build_pull_request(
        &mut self,
        local_crds: &CrdsTable,
        wallclock: u64,
    ) -> PullRequest {
        let hashes = local_crds.all_hashes();
        let mut filter = CrdsFilter::new(hashes.len().max(1));
        for h in &hashes {
            filter.add(h);
        }
        PullRequest {
            from: self.local_id,
            filter,
            wallclock,
        }
    }

    /// Pull isteğini işle — filtrede olmayan değerleri döndür
    /// Solana: `process_pull_request(crds, callers, now)` → `Vec<CrdsValue>`
    pub fn process_pull_request(
        &self,
        local_crds: &CrdsTable,
        request: &PullRequest,
        _now_ms: u64,
    ) -> PullResponse {
        // Filtrede olmayan değerleri seç
        let values: Vec<CrdsValue> = local_crds
            .values()
            .filter(|v| !request.filter.filter_contains(&v.hash()))
            .cloned()
            .collect();

        PullResponse {
            from: self.local_id,
            values,
        }
    }

    /// Pull yanıtını işle — yerel CRDS'e yeni değerleri ekle
    /// Solana: `process_pull_response(crds, from, now, values)` → usize (yeni sayısı)
    pub fn process_pull_response(
        &self,
        local_crds: &mut CrdsTable,
        response: &PullResponse,
    ) -> usize {
        let mut added = 0usize;
        for value in &response.values {
            if local_crds.insert(value.clone()) {
                added += 1;
            }
        }
        added
    }

    /// Bekleyen istek kaydet
    pub fn record_request(&mut self, to: [u8; 32], wallclock: u64) {
        self.pending_requests.insert(to, wallclock);
    }

    /// Yanıt alındığında bekleyen isteği temizle
    pub fn clear_request(&mut self, from: &[u8; 32]) {
        self.pending_requests.remove(from);
    }

    pub fn pending_count(&self) -> usize {
        self.pending_requests.len()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gossip::crds::{CrdsData, CrdsValue};

    fn node(n: u8) -> [u8; 32] {
        [n; 32]
    }

    fn contact(n: u8) -> CrdsValue {
        CrdsValue::new(CrdsData::ContactInfo {
            node_id: node(n),
            addr: format!("127.0.0.{n}:30333"),
            wallclock: 1000,
        })
    }

    #[test]
    fn test_build_pull_request() {
        let mut pull = CrdsGossipPull::new(node(0));
        let mut local = CrdsTable::new();
        local.insert(contact(1));
        local.insert(contact(2));

        let req = pull.build_pull_request(&local, 1000);
        assert_eq!(req.from, node(0));
        // Filtre oluşturuldu
        let h1 = contact(1).hash();
        assert!(req.filter.filter_contains(&h1));
    }

    #[test]
    fn test_process_pull_request_returns_missing() {
        let pull = CrdsGossipPull::new(node(1));

        // Yerel CRDS'de 3 değer var
        let mut local = CrdsTable::new();
        for i in 1u8..=3 {
            local.insert(contact(i));
        }

        // İstekte yerel'de contact(1) var
        let mut pull_requester = CrdsGossipPull::new(node(0));
        let mut requester_crds = CrdsTable::new();
        requester_crds.insert(contact(1));
        let req = pull_requester.build_pull_request(&requester_crds, 1000);

        let response = pull.process_pull_request(&local, &req, 1000);
        // contact(1) filtrede var → gönderilmez
        // contact(2) ve contact(3) → gönderilir
        // (false positive nedeniyle bazıları atlanabilir)
        // En az 1 değer gönderilmeli
        assert!(!response.values.is_empty());
    }

    #[test]
    fn test_process_pull_response_adds_to_crds() {
        let pull = CrdsGossipPull::new(node(0));
        let mut local = CrdsTable::new();

        let response = PullResponse {
            from: node(1),
            values: vec![contact(10), contact(11), contact(12)],
        };

        let added = pull.process_pull_response(&mut local, &response);
        assert_eq!(added, 3);
        assert_eq!(local.len(), 3);
    }

    #[test]
    fn test_process_pull_response_no_duplicates() {
        let pull = CrdsGossipPull::new(node(0));
        let mut local = CrdsTable::new();
        local.insert(contact(10));

        let response = PullResponse {
            from: node(1),
            values: vec![contact(10), contact(11)], // contact(10) zaten var
        };

        let added = pull.process_pull_response(&mut local, &response);
        assert_eq!(added, 1); // Sadece contact(11) eklendi
        assert_eq!(local.len(), 2);
    }

    #[test]
    fn test_pull_pending_tracking() {
        let mut pull = CrdsGossipPull::new(node(0));
        pull.record_request(node(1), 1000);
        pull.record_request(node(2), 1000);
        assert_eq!(pull.pending_count(), 2);
        pull.clear_request(&node(1));
        assert_eq!(pull.pending_count(), 1);
    }

    #[test]
    fn test_empty_crds_pull_request() {
        let mut pull = CrdsGossipPull::new(node(0));
        let local = CrdsTable::new();
        let req = pull.build_pull_request(&local, 1000);
        // Boş CRDS → boş filtre
        assert_eq!(req.from, node(0));
    }
}
