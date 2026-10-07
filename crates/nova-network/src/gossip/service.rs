//! Gossip servisi — push + pull birleşik yönetim
//!
//! Solana: `GossipService` — UDP soketi + crossbeam kanalları
//! Polkadot: `NetworkWorker` → kanal tabanlı mesaj akışı
//! Avalanchego: `ValidatorGossiper` + `PullGossiper` + `PushGossiper`

use crate::gossip::crds::{CrdsData, CrdsTable, CrdsValue};
use crate::gossip::pull::{CrdsGossipPull, PullRequest, PullResponse};
use crate::gossip::push::{CrdsGossipPush, PushMessage, PUSH_FANOUT};
use crate::peer::info::PeerId;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── GossipEvent ──────────────────────────────────────────────────────────────

/// Gossip servisine gelen olaylar
#[derive(Debug, Clone)]
pub enum GossipEvent {
    /// Yeni değer yayınla
    Publish(CrdsValue),
    /// Peer'dan push mesajı alındı
    ReceivedPush { from: [u8; 32], msg: PushMessage },
    /// Peer'dan pull isteği alındı
    ReceivedPullRequest { from: [u8; 32], req: PullRequest },
    /// Peer'dan pull yanıtı alındı
    ReceivedPullResponse { from: [u8; 32], resp: PullResponse },
    /// Peer bağlandı
    PeerConnected([u8; 32]),
    /// Peer ayrıldı
    PeerDisconnected([u8; 32]),
    /// Periyodik tick
    Tick { now_ms: u64 },
}

/// Gossip servisinden çıkan olaylar
#[derive(Debug, Clone)]
pub enum GossipOutput {
    /// Peer'a push mesajı gönder
    SendPush { to: [u8; 32], msg: PushMessage },
    /// Peer'a pull isteği gönder
    SendPullRequest { to: [u8; 32], req: PullRequest },
    /// Peer'a pull yanıtı gönder
    SendPullResponse { to: [u8; 32], resp: PullResponse },
    /// Yeni CRDS değeri öğrenildi
    NewValue(CrdsValue),
}

// ─── GossipStats ──────────────────────────────────────────────────────────────

/// Gossip istatistikleri
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GossipStats {
    pub pushed_count: u64,
    pub pulled_count: u64,
    pub received_push_count: u64,
    pub received_pull_count: u64,
    pub new_values_learned: u64,
    pub duplicate_count: u64,
}

// ─── GossipService ────────────────────────────────────────────────────────────

/// Gossip servisi
/// Solana: `GossipService` + kanal tabanlı mesaj akışı
pub struct GossipService {
    local_id: [u8; 32],
    crds: CrdsTable,
    push: CrdsGossipPush,
    pull: CrdsGossipPull,
    stats: GossipStats,
    /// Olaylar için kanal
    event_tx: Sender<GossipEvent>,
    event_rx: Receiver<GossipEvent>,
    /// Çıktı kuyruğu
    output: Vec<GossipOutput>,
}

impl GossipService {
    pub fn new(local_id: [u8; 32]) -> Self {
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        Self {
            local_id,
            crds: CrdsTable::new(),
            push: CrdsGossipPush::new(local_id, PUSH_FANOUT),
            pull: CrdsGossipPull::new(local_id),
            stats: GossipStats::default(),
            event_tx,
            event_rx,
            output: Vec::new(),
        }
    }

    /// Olay kanalına yazma ucu
    pub fn event_sender(&self) -> Sender<GossipEvent> {
        self.event_tx.clone()
    }

    /// Tüm bekleyen olayları işle
    pub fn process_events(&mut self) -> Vec<GossipOutput> {
        let events: Vec<GossipEvent> = {
            let mut v = Vec::new();
            loop {
                match self.event_rx.try_recv() {
                    Ok(e) => v.push(e),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            v
        };

        for event in events {
            self.handle_event(event);
        }

        std::mem::take(&mut self.output)
    }

    fn handle_event(&mut self, event: GossipEvent) {
        match event {
            GossipEvent::Publish(value) => {
                if self.crds.insert(value.clone()) {
                    self.push.push_value(value.clone());
                    self.stats.pushed_count += 1;
                    self.output.push(GossipOutput::NewValue(value));
                }
            }

            GossipEvent::ReceivedPush { from, msg } => {
                self.stats.received_push_count += 1;
                let now_ms = current_ms();
                let new_values = self.push.process_push_message(&msg, now_ms);
                for v in new_values {
                    if self.crds.insert(v.clone()) {
                        self.stats.new_values_learned += 1;
                        self.output.push(GossipOutput::NewValue(v));
                    }
                }
            }

            GossipEvent::ReceivedPullRequest { from, req } => {
                self.stats.received_pull_count += 1;
                let now_ms = current_ms();
                let resp = self.pull.process_pull_request(&self.crds, &req, now_ms);
                self.output.push(GossipOutput::SendPullResponse { to: from, resp });
            }

            GossipEvent::ReceivedPullResponse { from, resp } => {
                let added = self.pull.process_pull_response(&mut self.crds, &resp);
                self.stats.new_values_learned += added as u64;
                self.pull.clear_request(&from);
                for v in resp.values.iter().take(added) {
                    self.output.push(GossipOutput::NewValue(v.clone()));
                }
            }

            GossipEvent::PeerConnected(peer_id) => {
                let slot = current_ms() / 400; // 400ms slot
                self.push.add_peer(peer_id, slot);
            }

            GossipEvent::PeerDisconnected(peer_id) => {
                self.push.remove_peer(&peer_id);
            }

            GossipEvent::Tick { now_ms } => {
                // Push döngüsü
                let push_msgs = self.push.new_push_messages(now_ms);
                for (to, msg) in push_msgs {
                    self.stats.pushed_count += 1;
                    self.output.push(GossipOutput::SendPush { to, msg });
                }
                // Cache temizle
                self.push.prune(now_ms);
            }
        }
    }

    /// CRDS tablosuna doğrudan eriş
    pub fn crds(&self) -> &CrdsTable {
        &self.crds
    }

    /// İstatistikler
    pub fn stats(&self) -> &GossipStats {
        &self.stats
    }

    /// Aktif peer sayısı
    pub fn active_peer_count(&self) -> usize {
        self.push.active_peer_count()
    }
}

fn current_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn node(n: u8) -> [u8; 32] {
        [n; 32]
    }

    fn contact(n: u8) -> CrdsValue {
        CrdsValue::new(CrdsData::ContactInfo {
            node_id: node(n),
            addr: format!("127.0.0.{n}:30333"),
            wallclock: 1000 + n as u64,
        })
    }

    #[test]
    fn test_gossip_publish() {
        let mut svc = GossipService::new(node(0));
        let tx = svc.event_sender();
        tx.send(GossipEvent::Publish(contact(1))).unwrap();
        let out = svc.process_events();
        assert!(out.iter().any(|o| matches!(o, GossipOutput::NewValue(_))));
        assert_eq!(svc.crds().len(), 1);
    }

    #[test]
    fn test_gossip_peer_connect_and_tick() {
        let mut svc = GossipService::new(node(0));
        let tx = svc.event_sender();

        // Peer bağla
        tx.send(GossipEvent::PeerConnected(node(1))).unwrap();
        // Değer yayınla
        tx.send(GossipEvent::Publish(contact(5))).unwrap();
        // Tick
        tx.send(GossipEvent::Tick { now_ms: 1000 }).unwrap();

        let out = svc.process_events();
        // Push mesajı gönderilmeli
        assert!(out.iter().any(|o| matches!(o, GossipOutput::SendPush { .. })));
    }

    #[test]
    fn test_gossip_receive_push() {
        let mut svc = GossipService::new(node(0));
        let tx = svc.event_sender();

        let msg = PushMessage {
            from: node(1),
            values: vec![contact(2), contact(3)],
        };
        tx.send(GossipEvent::ReceivedPush { from: node(1), msg }).unwrap();
        let out = svc.process_events();

        assert!(out.iter().any(|o| matches!(o, GossipOutput::NewValue(_))));
        assert_eq!(svc.crds().len(), 2);
        assert_eq!(svc.stats().new_values_learned, 2);
    }

    #[test]
    fn test_gossip_pull_request_response() {
        let mut server = GossipService::new(node(0));
        let server_tx = server.event_sender();

        // Server'a değerler ekle
        server_tx.send(GossipEvent::Publish(contact(1))).unwrap();
        server_tx.send(GossipEvent::Publish(contact(2))).unwrap();
        server.process_events();

        // İstemci pull request oluştur
        let mut client_pull = CrdsGossipPull::new(node(1));
        let client_crds = CrdsTable::new();
        let req = client_pull.build_pull_request(&client_crds, 1000);

        // Server pull isteğini işle
        server_tx.send(GossipEvent::ReceivedPullRequest { from: node(1), req }).unwrap();
        let out = server.process_events();

        let responses: Vec<_> = out.iter()
            .filter(|o| matches!(o, GossipOutput::SendPullResponse { .. }))
            .collect();
        assert_eq!(responses.len(), 1);
    }

    #[test]
    fn test_gossip_duplicate_not_republished() {
        let mut svc = GossipService::new(node(0));
        let tx = svc.event_sender();

        tx.send(GossipEvent::Publish(contact(1))).unwrap();
        tx.send(GossipEvent::Publish(contact(1))).unwrap(); // tekrar
        let out = svc.process_events();

        let new_vals: Vec<_> = out.iter()
            .filter(|o| matches!(o, GossipOutput::NewValue(_)))
            .collect();
        assert_eq!(new_vals.len(), 1); // Yalnızca bir kez
    }

    #[test]
    fn test_gossip_stats() {
        let mut svc = GossipService::new(node(0));
        let tx = svc.event_sender();

        for i in 1u8..=5 {
            tx.send(GossipEvent::Publish(contact(i))).unwrap();
        }
        svc.process_events();

        assert_eq!(svc.stats().pushed_count, 5);
    }

    #[test]
    fn test_gossip_peer_disconnect() {
        let mut svc = GossipService::new(node(0));
        let tx = svc.event_sender();

        tx.send(GossipEvent::PeerConnected(node(1))).unwrap();
        svc.process_events();
        assert_eq!(svc.active_peer_count(), 1);

        tx.send(GossipEvent::PeerDisconnected(node(1))).unwrap();
        svc.process_events();
        assert_eq!(svc.active_peer_count(), 0);
    }
}
