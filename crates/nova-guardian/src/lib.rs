//! nova-guardian — Guardian Marketplace crate (§5.8)
//!
//! Guardian: Seed phrase olmayan hesapların kurtarma yetkilisi.
//! Kişisel, sertifikalı bireysel, kurumsal ve DAO kategorilerinde.
//! Trust Score, marketplace listeleme, teklif/sözleşme akışı, sigorta fonu.

pub mod economics;
pub mod errors;
pub mod fees;
pub mod insurance;
pub mod marketplace;
pub mod phragmen;
pub mod registry;
pub mod reputation;
pub mod staking;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use economics::{EcosystemStats, GuardianEconomics};
pub use phragmen::{
    GuardianCandidate, PhragmenElection, PhragmenResult, PhragmenVote,
    DEFAULT_GUARDIAN_SET_SIZE, MIN_BACKING_BPS,
};
pub use errors::GuardianResult;
pub use fees::{AutoPaymentScheduler, FeeModel, PaymentStatus};
pub use insurance::{ClaimReason, ClaimStatus, ClaimStore, InsuranceClaim, InsuranceFund, InsurancePayout};
pub use marketplace::{
    ContractStore, GuardianListing, MarketplaceSearch, OfferStore, SearchFilter, ServiceContract,
    ServiceOffer,
};
pub use registry::{
    GuardianCategory, GuardianRegistry, GuardianStatus, KycStatus, KycVerifier, SlaTerms,
};
pub use reputation::{
    GuardianSlashRecord, RecoveryHistory, RecoveryOutcome, SlashReason, SlashRecordStore,
    TrustScoreCalculator, TrustScoreComponents,
};
pub use staking::{
    ExistentialDepositRestriction, FreezeReason, FreezeRestriction, GuardianStakeManager,
    GuardianStakingHook, HoldReason, MultiGuardianStakingHooks, SendRestriction,
    SendRestrictionChain, SendRestrictionError, StakingError, GUARDIAN_EXISTENTIAL_DEPOSIT,
};

// ─── Integration Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fees::models::{AnnualFee, HybridFee};
    use crate::marketplace::listing::GuardianListing;
    use crate::registry::registration::Guardian;
    use crate::registry::categories::{MIN_STAKE_CERTIFIED_INDIVIDUAL, MIN_STAKE_INSTITUTIONAL};
    use nova_primitives::{Address, SlotId};

    const NSC: u128 = 1_000_000_000_000_000_000;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    // ─── Senaryo 1: Guardian kayıt ve Trust Score ────────────────────────────

    #[test]
    fn integration_register_and_score() {
        let registry = GuardianRegistry::new();

        let g = Guardian::new(
            addr(1),
            GuardianCategory::CertifiedIndividual,
            MIN_STAKE_CERTIFIED_INDIVIDUAL,
            0,
        )
        .unwrap();
        registry.register(g).unwrap();

        let guardian = registry.get(&addr(1)).unwrap();
        assert!(guardian.status.is_active());
        assert_eq!(guardian.category, GuardianCategory::CertifiedIndividual);

        // Trust Score hesapla
        let components = TrustScoreCalculator::compute(
            MIN_STAKE_CERTIFIED_INDIVIDUAL,
            365 * 2, // 2 yıl
            365,
            8, 10,  // %80 başarı
            0,      // slash yok
            3_600,  // hızlı yanıt
        );
        let score = components.compute();
        assert!(score > 0 && score <= 100);
    }

    // ─── Senaryo 2: Marketplace listing ve arama ────────────────────────────

    #[test]
    fn integration_marketplace_listing_and_search() {
        let listings = vec![
            GuardianListing::new(
                addr(1),
                GuardianCategory::CertifiedIndividual,
                85,
                FeeModel::Annual(AnnualFee { nsc_per_year: 50 * NSC }),
                10,
                "Bireysel guardian".to_string(),
                0,
            ),
            GuardianListing::new(
                addr(2),
                GuardianCategory::Institutional,
                92,
                FeeModel::Hybrid(HybridFee { annual_base: 100 * NSC, per_recovery_bonus: 10 * NSC }),
                50,
                "Kurumsal guardian".to_string(),
                0,
            )
            .with_sla(SlaTerms::new(216_000, 9_500, 200)),
            GuardianListing::new(
                addr(3),
                GuardianCategory::Personal,
                55,
                FeeModel::Free,
                5,
                "Kişisel guardian".to_string(),
                0,
            ),
        ];

        // Kurumsal + SLA filtresi
        let filter = SearchFilter::new()
            .with_category(GuardianCategory::Institutional)
            .with_sla();
        let results = MarketplaceSearch::search(&listings, &filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].guardian_id, addr(2));

        // Trust Score >= 80 filtresi → 2 sonuç
        let filter2 = SearchFilter::new().with_min_trust_score(80);
        let results2 = MarketplaceSearch::search(&listings, &filter2);
        assert_eq!(results2.len(), 2);
        // Trust Score'a göre sıralı (en yüksek önce)
        assert!(results2[0].trust_score >= results2[1].trust_score);
    }

    // ─── Senaryo 3: Teklif → Sözleşme akışı ─────────────────────────────────

    #[test]
    fn integration_offer_to_contract_flow() {
        let offer_store = OfferStore::new();
        let contract_store = ContractStore::new();

        // Kullanıcı teklif gönderir
        let offer_id = offer_store.create_offer(addr(10), addr(20), 50 * NSC, SlotId(0));
        let offer = offer_store.get(offer_id).unwrap();
        assert_eq!(offer.status, crate::marketplace::offer::OfferStatus::Pending);

        // Guardian teklifi kabul eder
        offer_store.accept(offer_id).unwrap();
        let accepted = offer_store.get(offer_id).unwrap();
        assert_eq!(accepted.status, crate::marketplace::offer::OfferStatus::Accepted);

        // Sözleşme oluşturulur
        let contract_id = contract_store.create(
            accepted.user,
            accepted.guardian,
            accepted.proposed_annual_fee,
            accepted.proposed_annual_fee, // prepaid = 1 yıl
            None,
            SlotId(0),
        );

        let contract = contract_store.get(contract_id).unwrap();
        assert_eq!(contract.status, crate::marketplace::contract::ContractStatus::Active);
        assert!(contract.is_active(SlotId(1_000)));
        assert!(!contract.is_active(SlotId(78_840_001)));
    }

    // ─── Senaryo 4: Slash kaydı ve otomatik jail ─────────────────────────────

    #[test]
    fn integration_slash_and_auto_jail() {
        let registry = GuardianRegistry::new();
        let slash_store = SlashRecordStore::new();

        let g = Guardian::new(addr(1), GuardianCategory::Personal, 0, 0).unwrap();
        registry.register(g).unwrap();

        // 3 yanlış dondurma → otomatik jail
        for i in 0u64..3 {
            slash_store.record_slash(
                addr(1),
                SlashReason::WrongFreezeAttempt,
                0, // penalty yok
                SlotId(i * 1000),
            );
        }

        let rec = slash_store.get_record(&addr(1));
        assert!(rec.should_auto_jail(3000));

        // Jail uygula
        registry.jail(&addr(1), 5_000).unwrap();
        assert!(registry.get(&addr(1)).unwrap().status.is_jailed());

        // Jail süresi dolunca serbest
        let released = registry.try_unjail(&addr(1), SlotId(5_000)).unwrap();
        assert!(released);
        assert!(registry.get(&addr(1)).unwrap().status.is_active());
    }

    // ─── Senaryo 5: Kurtarma geçmişi ve başarı oranı ─────────────────────────

    #[test]
    fn integration_recovery_history_and_trust_score() {
        let history_store = crate::reputation::history::RecoveryHistoryStore::new();

        for i in 0u64..8 {
            history_store.record(addr(1), addr(2), RecoveryOutcome::Success, 5_000, SlotId(i * 100));
        }
        for i in 0u64..2 {
            history_store.record(addr(1), addr(2), RecoveryOutcome::Timeout, 100_000, SlotId((8 + i) * 100));
        }

        let hist = history_store.history(&addr(1));
        assert_eq!(hist.successful_recoveries, 8);
        assert_eq!(hist.total_recoveries, 10);

        // Başarı oranını Trust Score'a yansıt
        let sr_score = TrustScoreCalculator::success_rate_score(
            hist.successful_recoveries,
            hist.total_recoveries,
        );
        assert_eq!(sr_score, 80); // %80
    }

    // ─── Senaryo 6: Sigorta fonu → talep → ödeme ────────────────────────────

    #[test]
    fn integration_insurance_fund_claim_payout() {
        let fund = InsuranceFund::new();
        let claim_store = ClaimStore::new();

        // Kurumsal guardian fona katkı yapar
        fund.deposit(500 * NSC).unwrap();
        assert_eq!(fund.balance(), 500 * NSC);

        // Kullanıcı başarısız kurtarma sonrası talep açar
        let claim_id = claim_store.submit_claim(
            addr(5),
            addr(10),
            ClaimReason::RecoveryRefused,
            100 * NSC,
            SlotId(1000),
        );

        // Talep onaylanır
        claim_store.approve(claim_id, 100 * NSC, SlotId(1100)).unwrap();

        // Tazminat ödenir
        let payout_mgr = InsurancePayout::new(&fund, &claim_store);
        let record = payout_mgr.execute_payout(claim_id, SlotId(1200)).unwrap();

        assert_eq!(record.amount, 100 * NSC);
        assert_eq!(record.recipient, addr(5));
        assert_eq!(fund.balance(), 400 * NSC);
        assert_eq!(fund.total_paid_out(), 100 * NSC);

        // Talep Paid durumunda
        assert_eq!(claim_store.get(claim_id).unwrap().status, ClaimStatus::Paid);
    }

    // ─── Senaryo 7: Otomatik ödeme planı ─────────────────────────────────────

    #[test]
    fn integration_auto_payment_schedule() {
        let scheduler = AutoPaymentScheduler::new();
        const PERIOD: u64 = 78_840_000;

        let id = scheduler.create_schedule(addr(5), addr(20), 50 * NSC, PERIOD, SlotId(0));

        // Henüz ödeme günü değil
        let processed = scheduler.process_slot(SlotId(1_000), |_, _, _| true);
        assert!(processed.is_empty());

        // Ödeme günü
        let processed = scheduler.process_slot(SlotId(PERIOD), |_, _, _| true);
        assert_eq!(processed.len(), 1);
        assert!(processed[0].success);

        // Bir sonraki dönem için yenile
        scheduler.renew_schedule(id).unwrap();
        let processed2 = scheduler.process_slot(SlotId(PERIOD * 2), |_, _, _| true);
        assert_eq!(processed2.len(), 1);

        // 2 ödeme geçmişte
        assert_eq!(scheduler.payment_history(id).len(), 2);
    }

    // ─── Senaryo 8: Ekosistem istatistikleri ─────────────────────────────────

    #[test]
    fn integration_ecosystem_stats() {
        let mut stats = EcosystemStats::new();
        stats.total_guardians = 50;
        stats.active_guardians = 45;
        stats.total_staked_nsc = 10_000 * NSC;
        stats.total_accounts_protected = 500;
        stats.total_successful_recoveries = 450;
        stats.total_failed_recoveries = 50;

        assert_eq!(stats.recovery_success_rate_bps(), 9_000); // %90
        assert_eq!(stats.avg_stake_per_protected_account(), 20 * NSC);
    }

    // ─── Senaryo 9: Guardian ekonomi hesapları ───────────────────────────────

    #[test]
    fn integration_guardian_economics() {
        use crate::economics::GuardianEconomics;
        use crate::fees::models::AnnualFee;
        use crate::registry::categories::MIN_STAKE_INSTITUTIONAL;

        let econ = GuardianEconomics::new(
            GuardianCategory::Institutional,
            MIN_STAKE_INSTITUTIONAL,
            100, // 100 hesabı koruyor
            FeeModel::Annual(AnnualFee { nsc_per_year: 100 * NSC }),
        );

        assert_eq!(econ.estimated_annual_revenue, 10_000 * NSC);
        assert!(econ.apy_bps > 0);
        assert_eq!(econ.monthly_revenue(), 10_000 * NSC / 12);
    }

    // ─── Senaryo 10: KYC Verifier akışı ─────────────────────────────────────

    #[test]
    fn integration_kyc_verification_flow() {
        let registry = GuardianRegistry::new();

        // Kurumsal guardian PendingVerification ile başlar
        let g = Guardian::new(
            addr(1),
            GuardianCategory::Institutional,
            MIN_STAKE_INSTITUTIONAL,
            0,
        )
        .unwrap();
        assert!(matches!(g.status, GuardianStatus::PendingVerification));
        registry.register(g).unwrap();

        // KYC onayı (statik metod)
        KycVerifier::approve(&registry, &addr(1)).unwrap();
        let updated = registry.get(&addr(1)).unwrap();
        assert_eq!(updated.kyc_status, KycStatus::Verified);
        assert!(updated.status.is_active());
    }

    // ─── Senaryo 11: SLA koşulları kontrolü ──────────────────────────────────

    #[test]
    fn integration_sla_terms() {
        let sla = SlaTerms::new(216_000, 9_500, 100); // 24h yanıt, %95 uptime, 100 NSC garanti
        // is_response_time_violated: true ise ihlal var
        assert!(!sla.is_response_time_violated(216_000)); // tam limitinde — ihlal yok
        assert!(sla.is_response_time_violated(216_001)); // limitin üstü — ihlal
        // uptime doğrudan alan üzerinden kontrol
        assert!(sla.min_uptime_bps <= 9_500);
        assert!(sla.max_response_slots == 216_000);
    }

    // ─── Senaryo 12: Çok kullanıcılı paralel kontratlar ─────────────────────

    #[test]
    fn integration_multiple_users_same_guardian() {
        let contract_store = ContractStore::new();

        // 5 farklı kullanıcı aynı guardian ile sözleşme
        for i in 1u8..=5 {
            contract_store.create(addr(i), addr(99), 50 * NSC, 50 * NSC, None, SlotId(0));
        }

        let active = contract_store.active_contracts_for_user(&addr(1), SlotId(1000));
        assert_eq!(active.len(), 1);

        let active3 = contract_store.active_contracts_for_user(&addr(3), SlotId(1000));
        assert_eq!(active3.len(), 1);
    }
}
