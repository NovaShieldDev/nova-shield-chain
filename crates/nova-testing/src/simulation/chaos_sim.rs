//! Kaos mühendisliği simülasyonu — ağ hataları + Byzantine node'lar.
//!
//! Test senaryoları:
//! 1. **Split-brain**: N node'u iki gruba böl, partition sonrası birleştir.
//! 2. **Byzantine azınlık**: f < N/3 Byzantine node ile güvenlik garantisi.
//! 3. **Mesaj kaybı**: %30, %50 mesaj kayıp oranıyla canlılık testi.
//! 4. **Gecikmeli teslim**: Yüksek gecikme altında finalizasyon.
//! 5. **Kademeli kurtarma**: Partition sonrası ağ iyileşmesi.

use nova_primitives::{BlockHash, SlotId, ValidatorId};
use crate::simulation::bft_sim::{BftRoundResult, BftSimulation, SafetyCheckResult};
use crate::simulation::network_sim::{PartitionConfig, SimNetwork};

// ─── ChaosScenario ───────────────────────────────────────────────────────────

/// Kaos senaryosu çalıştırma sonucu.
#[derive(Debug, Clone)]
pub struct ChaosResult {
    pub scenario_name: &'static str,
    pub safety_maintained: bool,
    pub rounds_completed: u64,
    pub honest_nodes_finalized: usize,
    pub total_nodes: usize,
    pub notes: Vec<String>,
}

impl ChaosResult {
    pub fn liveness_ratio(&self) -> f64 {
        if self.total_nodes == 0 {
            return 0.0;
        }
        self.honest_nodes_finalized as f64 / self.total_nodes as f64
    }
}

// ─── ChaosSimulator ──────────────────────────────────────────────────────────

/// Kaos simülatörü — farklı hata senaryolarını çalıştırır.
pub struct ChaosSimulator {
    pub results: Vec<ChaosResult>,
}

impl ChaosSimulator {
    pub fn new() -> Self {
        Self { results: Vec::new() }
    }

    /// Senaryo 1: Tam ağda normal çalışma.
    pub fn run_healthy_network(&mut self, n: usize, rounds: usize) -> ChaosResult {
        let mut sim = BftSimulation::new(n);

        for i in 1..=rounds as u64 {
            let hash = make_block_hash(i as u8);
            sim.run_round(i, hash);
        }

        let safety = sim.check_safety() == SafetyCheckResult::Safe;
        let finalized = sim.finalized_count_at(rounds as u64);

        let result = ChaosResult {
            scenario_name: "sağlıklı_ağ",
            safety_maintained: safety,
            rounds_completed: sim.rounds_completed,
            honest_nodes_finalized: finalized,
            total_nodes: n,
            notes: vec![format!("{}/{} dürüst node finalize etti", finalized, n)],
        };
        self.results.push(result.clone());
        result
    }

    /// Senaryo 2: Ağ bölmesi (split-brain) — partition sonrası heal.
    ///
    /// - Rounds 1-3: Normal çalışma
    /// - Rounds 4-6: Ağ bölmesi (2 grup)
    /// - Rounds 7-9: Heal sonrası kurtarma
    pub fn run_split_brain(&mut self, n: usize) -> ChaosResult {
        let mut sim = BftSimulation::new(n);
        let mut notes = Vec::new();

        // Round 1-3: Normal
        for i in 1..=3u64 {
            sim.run_round(i, make_block_hash(i as u8));
        }
        notes.push("Round 1-3: normal".to_string());

        // Round 4-6: Partition (network erişim simülasyonunu manuel yaparız)
        // Partition sırasında oylar sadece aynı gruptaki node'lara gider
        let half = n / 2;
        for i in 4..=6u64 {
            let hash = make_block_hash(i as u8);
            // Partition: sadece yarısı broadcast yapıyor
            // Quorum'a ulaşmak zor — simulate network-level restriction
            sim.network.apply_partition(PartitionConfig::split_half(n));
            let result = sim.run_round(i, hash);
            match result {
                BftRoundResult::Finalized { nodes_with_qc } => {
                    notes.push(format!("Round {i}: partition'a rağmen {nodes_with_qc} QC"));
                }
                BftRoundResult::QuorumNotReached { .. } => {
                    notes.push(format!("Round {i}: partition - quorum sağlanamadı (beklenen)"));
                }
                BftRoundResult::SafetyViolation { .. } => {
                    notes.push(format!("Round {i}: GÜVENLİK İHLALİ (beklenmedik)"));
                }
            }
        }

        // Heal
        sim.network.heal_partition();
        notes.push(format!("Round 7: partition heal edildi"));

        // Round 7-9: Kurtarma
        for i in 7..=9u64 {
            let hash = make_block_hash(i as u8);
            let result = sim.run_round(i, hash);
            if let BftRoundResult::Finalized { nodes_with_qc } = result {
                notes.push(format!("Round {i}: kurtarma sonrası {nodes_with_qc} QC"));
            }
        }

        let safety = sim.check_safety() == SafetyCheckResult::Safe;
        let finalized = sim.finalized_count_at(9);

        let result = ChaosResult {
            scenario_name: "split_brain",
            safety_maintained: safety,
            rounds_completed: sim.rounds_completed,
            honest_nodes_finalized: finalized,
            total_nodes: n,
            notes,
        };
        self.results.push(result.clone());
        result
    }

    /// Senaryo 3: Byzantine azınlık — güvenlik garantisi.
    ///
    /// n node, f = floor((n-1)/3) Byzantine node.
    /// BFT güvencesi: f < n/3 → güvenlik korunur.
    pub fn run_byzantine_minority(&mut self, n: usize) -> ChaosResult {
        let f = (n - 1) / 3; // Maksimum güvenli hatalı sayı
        let mut sim = BftSimulation::new_with_faulty(n, f);
        let mut notes = vec![format!("n={}, f={} Byzantine (f < n/3)", n, f)];

        for i in 1..=5u64 {
            let hash = make_block_hash(i as u8);
            let result = sim.run_round(i, hash);
            match result {
                BftRoundResult::Finalized { nodes_with_qc } => {
                    notes.push(format!("Round {i}: {nodes_with_qc} QC (Byzantine var ama quorum sağlandı)"));
                }
                BftRoundResult::QuorumNotReached { votes_cast, needed } => {
                    notes.push(format!("Round {i}: quorum yok ({votes_cast}/{needed})"));
                }
                BftRoundResult::SafetyViolation { .. } => {
                    notes.push(format!("Round {i}: GÜVENLİK İHLALİ (f < n/3 koşuluyla bu beklenmiyor)"));
                }
            }
        }

        let safety = sim.check_safety() == SafetyCheckResult::Safe;
        let finalized = sim.finalized_count_at(5);

        let result = ChaosResult {
            scenario_name: "byzantine_azinlik",
            safety_maintained: safety,
            rounds_completed: sim.rounds_completed,
            honest_nodes_finalized: finalized,
            total_nodes: n,
            notes,
        };
        self.results.push(result.clone());
        result
    }

    /// Senaryo 4: %30 mesaj kaybı altında canlılık.
    pub fn run_message_loss(&mut self, n: usize, drop_rate_bps: u32) -> ChaosResult {
        let mut sim = BftSimulation::new(n);
        sim.network = sim.network.with_drop_rate(drop_rate_bps);

        let mut finalized_rounds = 0u64;
        for i in 1..=10u64 {
            let result = sim.run_round(i, make_block_hash(i as u8));
            if let BftRoundResult::Finalized { .. } = result {
                finalized_rounds += 1;
            }
        }

        let safety = sim.check_safety() == SafetyCheckResult::Safe;
        let finalized = sim.finalized_count_at(10);

        let result = ChaosResult {
            scenario_name: "mesaj_kaybi",
            safety_maintained: safety,
            rounds_completed: sim.rounds_completed,
            honest_nodes_finalized: finalized,
            total_nodes: n,
            notes: vec![
                format!("Drop rate: {}bps ({}%)", drop_rate_bps, drop_rate_bps / 100),
                format!("{}/10 round finalize edildi", finalized_rounds),
                format!("Toplam düşürülen mesaj: {}", sim.network.stats.total_dropped_rate),
            ],
        };
        self.results.push(result.clone());
        result
    }

    /// Senaryo 5: Tek node izolasyonu — diğerleri çalışmaya devam eder mi?
    pub fn run_isolated_node(&mut self, n: usize) -> ChaosResult {
        let mut sim = BftSimulation::new(n);
        let mut notes = Vec::new();

        // Node 0'ı izole et
        sim.network.apply_partition(PartitionConfig::isolate(0, n));
        notes.push(format!("Node 0 izole edildi ({} node kalan)", n - 1));

        let mut remaining_finalize = 0;
        for i in 1..=5u64 {
            let hash = make_block_hash(i as u8);
            let result = sim.run_round(i, hash);
            if let BftRoundResult::Finalized { nodes_with_qc } = result {
                remaining_finalize += nodes_with_qc;
                notes.push(format!("Round {i}: {nodes_with_qc} QC (izole node 0 hariç)"));
            }
        }

        let safety = sim.check_safety() == SafetyCheckResult::Safe;

        let result = ChaosResult {
            scenario_name: "izole_node",
            safety_maintained: safety,
            rounds_completed: sim.rounds_completed,
            honest_nodes_finalized: remaining_finalize,
            total_nodes: n,
            notes,
        };
        self.results.push(result.clone());
        result
    }

    /// Tüm kaos senaryolarını çalıştır ve özet istatistik döndür.
    pub fn run_all(&mut self, n: usize) -> ChaosReport {
        let healthy = self.run_healthy_network(n, 10);
        let split = self.run_split_brain(n);
        let byzantine = self.run_byzantine_minority(n);
        let msg_loss = self.run_message_loss(n, 3_000); // %30 kayıp
        let isolated = self.run_isolated_node(n);

        ChaosReport {
            node_count: n,
            healthy,
            split_brain: split,
            byzantine_minority: byzantine,
            message_loss: msg_loss,
            node_isolation: isolated,
        }
    }
}

impl Default for ChaosSimulator {
    fn default() -> Self {
        Self::new()
    }
}

// ─── ChaosReport ─────────────────────────────────────────────────────────────

/// Tüm kaos senaryolarının raporu.
pub struct ChaosReport {
    pub node_count: usize,
    pub healthy: ChaosResult,
    pub split_brain: ChaosResult,
    pub byzantine_minority: ChaosResult,
    pub message_loss: ChaosResult,
    pub node_isolation: ChaosResult,
}

impl ChaosReport {
    /// Tüm senaryolarda güvenlik sağlandı mı?
    pub fn all_safe(&self) -> bool {
        self.healthy.safety_maintained
            && self.split_brain.safety_maintained
            && self.byzantine_minority.safety_maintained
            && self.message_loss.safety_maintained
            && self.node_isolation.safety_maintained
    }

    /// Kaç senaryo başarılı?
    pub fn safe_scenario_count(&self) -> usize {
        [
            &self.healthy,
            &self.split_brain,
            &self.byzantine_minority,
            &self.message_loss,
            &self.node_isolation,
        ]
        .iter()
        .filter(|r| r.safety_maintained)
        .count()
    }
}

// ─── Yardımcı fonksiyonlar ────────────────────────────────────────────────────

fn make_block_hash(seed: u8) -> BlockHash {
    let mut h = blake3::Hasher::new();
    h.update(b"nova-sim-block");
    h.update(&[seed]);
    BlockHash::from_bytes(*h.finalize().as_bytes())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_healthy_network_3_nodes() {
        let mut chaos = ChaosSimulator::new();
        let result = chaos.run_healthy_network(3, 5);
        assert!(result.safety_maintained, "Sağlıklı ağda güvenlik ihlali!");
    }

    #[test]
    fn test_healthy_network_7_nodes() {
        let mut chaos = ChaosSimulator::new();
        let result = chaos.run_healthy_network(7, 5);
        assert!(result.safety_maintained);
    }

    #[test]
    fn test_split_brain_safety_maintained() {
        let mut chaos = ChaosSimulator::new();
        // Split brain güvenliği tehdit etmemeli (quorum sağlanamasa da)
        let result = chaos.run_split_brain(6);
        assert!(result.safety_maintained,
            "Split-brain güvenlik ihlali: {:?}", result.notes);
    }

    #[test]
    fn test_byzantine_minority_safe() {
        let mut chaos = ChaosSimulator::new();
        // f=1 Byzantine, n=4: 4 > 3*1 → güvenli
        let result = chaos.run_byzantine_minority(4);
        assert!(result.safety_maintained,
            "Byzantine azınlık güvenlik ihlali: {:?}", result.notes);
    }

    #[test]
    fn test_byzantine_minority_n7_f2_safe() {
        let mut chaos = ChaosSimulator::new();
        // n=7, f=2: 7 > 6 → güvenli
        let result = chaos.run_byzantine_minority(7);
        assert!(result.safety_maintained);
    }

    #[test]
    fn test_message_loss_10_percent_safety() {
        let mut chaos = ChaosSimulator::new();
        let result = chaos.run_message_loss(5, 1_000); // %10
        assert!(result.safety_maintained, "Mesaj kaybında güvenlik ihlali");
    }

    #[test]
    fn test_isolated_node_others_continue() {
        let mut chaos = ChaosSimulator::new();
        // n=5, node 0 izole → kalan 4 node çalışmaya devam eder
        let result = chaos.run_isolated_node(5);
        assert!(result.safety_maintained,
            "İzole node senaryosunda güvenlik ihlali");
    }

    #[test]
    fn test_all_chaos_scenarios_safe() {
        let mut chaos = ChaosSimulator::new();
        let report = chaos.run_all(5);
        assert!(
            report.all_safe(),
            "{}/5 senaryo güvenli. Başarısızlar: {}",
            report.safe_scenario_count(),
            [&report.healthy, &report.split_brain, &report.byzantine_minority,
             &report.message_loss, &report.node_isolation]
                .iter()
                .filter(|r| !r.safety_maintained)
                .map(|r| r.scenario_name)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    #[test]
    fn test_chaos_report_5_of_5_safe() {
        let mut chaos = ChaosSimulator::new();
        let report = chaos.run_all(5);
        assert_eq!(report.safe_scenario_count(), 5,
            "Tüm 5 senaryo güvenli olmalı");
    }
}
