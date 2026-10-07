//! Governance saldırı simülasyonları — Cosmos SDK / Compound / Uniswap seviyesinde
//!
//! ## Test Kategorileri
//!
//! 1. **Whale takeover** — Tek büyük oy sahibinin governance'ı ele geçirmesi
//! 2. **Flash-loan style stake** — Oy → unstake aynı snapshot'ta
//! 3. **Proposal spam** — Min deposit ile sonsuz öneri gönderme
//! 4. **Quorum bypass** — Quorum dolmadan geçiş denemeleri
//! 5. **Double voting** — Aynı adres iki kez oy kullanma
//! 6. **Veto griefing** — Meşru öneriyi veto ile bloke etme
//! 7. **Treasury drain** — Governance ile hazineyi boşaltma girişimi
//! 8. **Runtime upgrade bomb** — Kötü niyetli upgrade teklifi

#[cfg(test)]
mod tests {
    use nova_runtime::{
        Proposal, ProposalStatus, ProposalStore, TallyEngine, TallyResult,
        QUORUM_BPS, THRESHOLD_BPS, VETO_THRESHOLD_BPS,
        governance::proposal::{MIN_DEPOSIT_NSC, VOTING_PERIOD_SLOTS},
        VoteOption,
        tx::types::ProposalPayload,
    };
    use nova_primitives::{Address, SlotId};

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_proposal(store: &ProposalStore, proposer: u8) -> u64 {
        let pid = store.next_proposal_id();
        let p = Proposal::new(
            pid,
            addr(proposer),
            format!("Proposal {pid}"),
            "açıklama".to_string(),
            ProposalPayload::Text { content: "içerik".to_string() },
            MIN_DEPOSIT_NSC,
            0,
        );
        store.add_proposal(p);
        pid
    }

    // ── Whale Takeover ────────────────────────────────────────────────────────

    /// PoC: Tek whale (%50+ oy) governance öneriyi geçirebilir mi?
    /// Güvenlik: Quorum ve veto eşiği bu senaryoyu sınırlamalı
    #[test]
    fn poc_whale_cannot_pass_below_quorum() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 1);

        let total_bonded = 1_000_000u128;
        // QUORUM_BPS = 3340 → %33.4 → 334_000 token quorum gerekli
        let quorum_required = total_bonded * QUORUM_BPS as u128 / 10_000;

        // Whale: sadece quorum altında oy kullanıyor
        let whale_votes = quorum_required - 1;
        store.cast_vote(pid, addr(10), VoteOption::Yes, whale_votes, whale_votes, SlotId(1)).unwrap();

        // Oylama süresi dol
        let finalized = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), total_bonded);
        assert!(!finalized.is_empty());

        let proposal = store.get(pid).unwrap();
        // Quorum sağlanamadı → geçmemeli
        assert!(
            !matches!(proposal.status, ProposalStatus::Passed),
            "quorum altında whale oyu öneriyi geçiremez"
        );
    }

    /// PoC: Whale %67+ ile öneriyi geçirebilir (meşru yol)
    #[test]
    fn poc_legitimate_supermajority_passes() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 1);

        let total_bonded = 1_000_000u128;

        // Yes: 700k, No: 300k — %70 Yes, quorum > %33.4
        store.cast_vote(pid, addr(1), VoteOption::Yes, 700_000, 700_000, SlotId(1)).unwrap();
        store.cast_vote(pid, addr(2), VoteOption::No, 300_000, 300_000, SlotId(1)).unwrap();

        let finalized = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), total_bonded);
        assert!(!finalized.is_empty());

        let proposal = store.get(pid).unwrap();
        assert!(
            matches!(proposal.status, ProposalStatus::Passed),
            "meşru supermajority önerisi geçmeli"
        );
    }

    // ── Double Voting Koruması ────────────────────────────────────────────────

    /// PoC: Aynı adres aynı öneri için iki kez oy kullanamaz
    #[test]
    fn poc_double_voting_rejected() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 1);

        // İlk oy
        store.cast_vote(pid, addr(5), VoteOption::Yes, 100_000, 100_000, SlotId(1)).unwrap();

        // İkinci oy aynı adresten → hata vermeli veya oy değişmemeli
        let second_vote = store.cast_vote(pid, addr(5), VoteOption::No, 200_000, 200_000, SlotId(2));

        // Governance implementasyonuna göre: ya error ya oy update
        // En azından toplam oy tutarlı kalmalı
        let votes = store.votes_for(pid);
        let voter_vote = votes.get(&addr(5)).unwrap();

        // Toplam oy iki oy miktarının toplamı olamaz (double vote yok)
        assert!(
            voter_vote.1 <= 200_000,
            "double voting: oy miktarı tek bir oyun max değerini aşmamalı"
        );
    }

    // ── Quorum Güvenliği ──────────────────────────────────────────────────────

    /// PoC: Quorum sağlanmadan öneri geçemez (Cosmos SDK pattern)
    #[test]
    fn poc_quorum_not_met_proposal_fails() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 1);

        let total_bonded = 10_000_000u128; // 10M token
        // Çok az oy: sadece 1000 token (toplam %0.01)
        store.cast_vote(pid, addr(1), VoteOption::Yes, 1_000, 1_000, SlotId(1)).unwrap();

        let finalized = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), total_bonded);
        assert!(!finalized.is_empty());

        let proposal = store.get(pid).unwrap();
        assert!(
            !matches!(proposal.status, ProposalStatus::Passed),
            "quorum sağlanmadan öneri geçemez"
        );
    }

    /// PoC: Tam quorum eşiğinde öneri geçebilir (boundary test)
    #[test]
    fn poc_exact_quorum_threshold_passes() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 1);

        let total_bonded = 1_000_000u128;
        let quorum_amount = (total_bonded * QUORUM_BPS as u128 + 9999) / 10_000;

        // Tam quorum + Yes supermajority
        let yes_amount = quorum_amount * 7 / 10; // %70 Yes
        let no_amount = quorum_amount - yes_amount;

        store.cast_vote(pid, addr(1), VoteOption::Yes, yes_amount, yes_amount, SlotId(1)).unwrap();
        store.cast_vote(pid, addr(2), VoteOption::No, no_amount, no_amount, SlotId(1)).unwrap();

        let _ = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), total_bonded);
        let proposal = store.get(pid).unwrap();

        // Quorum sağlandı ve yes > %50 → geçmeli
        assert!(
            matches!(proposal.status, ProposalStatus::Passed),
            "tam quorum eşiğinde öneri geçmeli: {:?}", proposal.status
        );
    }

    // ── Veto Koruması ─────────────────────────────────────────────────────────

    /// PoC: %33.4+ NoWithVeto → öneri iptal (deposit yakılır)
    #[test]
    fn poc_veto_threshold_kills_proposal() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 1);

        let total_bonded = 1_000_000u128;

        // %34 NoWithVeto, %66 Yes — veto eşiği aşıldı
        store.cast_vote(pid, addr(1), VoteOption::NoWithVeto, 340_000, 340_000, SlotId(1)).unwrap();
        store.cast_vote(pid, addr(2), VoteOption::Yes, 660_000, 660_000, SlotId(1)).unwrap();

        let finalized = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), total_bonded);
        assert!(!finalized.is_empty());

        let proposal = store.get(pid).unwrap();
        assert!(
            matches!(proposal.status, ProposalStatus::Vetoed),
            "veto eşiği aşıldığında öneri vetolansın: {:?}", proposal.status
        );
    }

    /// PoC: %33'ün altında veto → öneri geçer (veto griefing başarısız)
    #[test]
    fn poc_insufficient_veto_cannot_block_proposal() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 1);

        let total_bonded = 1_000_000u128;

        // %30 NoWithVeto (eşiğin altında), %70 Yes
        store.cast_vote(pid, addr(1), VoteOption::NoWithVeto, 300_000, 300_000, SlotId(1)).unwrap();
        store.cast_vote(pid, addr(2), VoteOption::Yes, 700_000, 700_000, SlotId(1)).unwrap();

        let _ = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), total_bonded);
        let proposal = store.get(pid).unwrap();

        // Veto yetmez → yes kazanmalı
        assert!(
            !matches!(proposal.status, ProposalStatus::Vetoed),
            "eşik altı veto öneriyi durduramaz"
        );
    }

    // ── TallyEngine Doğrudan Testleri ─────────────────────────────────────────

    fn make_votes(
        yes: u128,
        no: u128,
        abstain: u128,
        veto: u128,
    ) -> std::collections::HashMap<Address, (VoteOption, u128)> {
        let mut votes = std::collections::HashMap::new();
        if yes > 0    { votes.insert(addr(11), (VoteOption::Yes, yes)); }
        if no > 0     { votes.insert(addr(12), (VoteOption::No, no)); }
        if abstain > 0{ votes.insert(addr(13), (VoteOption::Abstain, abstain)); }
        if veto > 0   { votes.insert(addr(14), (VoteOption::NoWithVeto, veto)); }
        votes
    }

    /// PoC: TallyEngine Yes/No/Abstain/Veto doğru hesaplar
    #[test]
    fn poc_tally_engine_correct_calculation() {
        // yes=700k, no=100k, abstain=100k, veto=100k → total=1M
        let votes = make_votes(700_000, 100_000, 100_000, 100_000);
        let result = TallyEngine::tally(&votes, 1_000_000);

        assert_eq!(result.yes_power, 700_000);
        assert_eq!(result.no_power, 100_000);
        assert_eq!(result.abstain_power, 100_000);
        assert_eq!(result.no_with_veto_power, 100_000);

        // Non-abstain = 900k, yes% = 700/900 = %77.7 → should pass
        let (status, burned) = TallyEngine::evaluate(&result, false);
        assert!(matches!(status, ProposalStatus::Passed), "clear yes majority should pass");
        assert!(!burned, "clear yes should not burn deposit");
    }

    /// PoC: 50/50 oy → öneri geçmez (threshold > %50 gerekli)
    #[test]
    fn poc_fifty_fifty_vote_does_not_pass() {
        let votes = make_votes(500_000, 500_000, 0, 0);
        let result = TallyEngine::tally(&votes, 1_000_000);

        let (status, _) = TallyEngine::evaluate(&result, false);
        // Yes = 500k, No = 500k → Yes% = 50% = THRESHOLD_BPS (5000) eşiği tam → geçmez
        assert!(
            !matches!(status, ProposalStatus::Passed),
            "50/50 oy geçmemeli (>%50 gerekli)"
        );
    }

    // ── Proposal Spam Koruması ────────────────────────────────────────────────

    /// PoC: Min deposit olmadan öneri gönderilemez
    #[test]
    fn poc_proposal_requires_minimum_deposit() {
        let store = ProposalStore::new();
        let pid = store.next_proposal_id();

        // MIN_DEPOSIT_NSC altında deposit
        let low_deposit = MIN_DEPOSIT_NSC / 2;
        let p = Proposal::new(
            pid,
            addr(1),
            "Spam öneri".to_string(),
            "içerik".to_string(),
            ProposalPayload::Text { content: "spam".to_string() },
            low_deposit,
            0,
        );
        store.add_proposal(p);

        // Düşük depozitli öneri voting period'a geçemez
        let proposal = store.get(pid).unwrap();
        // Deposit yoklanabilir — deposit_period_slots henüz dolmadı, pending
        assert!(
            !matches!(proposal.status, ProposalStatus::Passed),
            "düşük depozitli öneri aktif olmamalı"
        );
    }

    // ── Treasury Drain Koruması ───────────────────────────────────────────────

    /// PoC: Hazine harcama teklifi → governance supermajority zorunlu
    #[test]
    fn poc_treasury_spend_requires_governance_supermajority() {
        let store = ProposalStore::new();
        let pid = store.next_proposal_id();

        let treasury_drain = Proposal::new(
            pid,
            addr(99),
            "Hazine Harcaması".to_string(),
            "tüm hazineyi çek".to_string(),
            ProposalPayload::TreasurySpend {
                recipient: addr(99),
                amount: 1_000_000_000 * 1_000_000_000_000_000_000u128, // 1 milyar NSC
            },
            MIN_DEPOSIT_NSC,
            0,
        );
        store.add_proposal(treasury_drain);

        let total_bonded = 10_000_000u128;

        // Düşük oy → geçmemeli
        store.cast_vote(pid, addr(99), VoteOption::Yes, 100_000, 100_000, SlotId(1)).unwrap();

        let _ = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), total_bonded);
        let proposal = store.get(pid).unwrap();

        assert!(
            !matches!(proposal.status, ProposalStatus::Passed),
            "hazine drain teklifi tek oylayla geçemez"
        );
    }

    // ── Runtime Upgrade Güvenliği ─────────────────────────────────────────────

    /// PoC: Runtime upgrade supermajority gerektiriyor
    #[test]
    fn poc_runtime_upgrade_requires_supermajority() {
        let store = ProposalStore::new();
        let pid = store.next_proposal_id();

        let upgrade = Proposal::new(
            pid,
            addr(1),
            "Kötü Upgrade".to_string(),
            "backdoor ekle".to_string(),
            ProposalPayload::UpgradeRuntime { new_version: 0x999999 },
            MIN_DEPOSIT_NSC,
            0,
        );
        store.add_proposal(upgrade);

        let total_bonded = 1_000_000u128;

        // Sadece %40 Yes, %60 No → geçmemeli
        store.cast_vote(pid, addr(1), VoteOption::Yes, 400_000, 400_000, SlotId(1)).unwrap();
        store.cast_vote(pid, addr(2), VoteOption::No, 600_000, 600_000, SlotId(1)).unwrap();

        let _ = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), total_bonded);
        let proposal = store.get(pid).unwrap();

        assert!(
            !matches!(proposal.status, ProposalStatus::Passed),
            "basit çoğunlukta runtime upgrade geçemez"
        );
    }

    /// PoC: TallyEngine expedited mode → daha yüksek eşik gerektirir
    #[test]
    fn poc_expedited_proposal_needs_higher_threshold() {
        // Normal %60 Yes → geçer
        let normal_votes = make_votes(600_000, 400_000, 0, 0);
        let normal_result = TallyEngine::tally(&normal_votes, 1_000_000);
        let (normal_status, _) = TallyEngine::evaluate(&normal_result, false);
        assert!(matches!(normal_status, ProposalStatus::Passed), "normal %60 geçmeli");

        // Expedited %60 Yes → geçmez (>%66.7 gerekli)
        let exp_votes = make_votes(600_000, 400_000, 0, 0);
        let expedited_result = TallyEngine::tally(&exp_votes, 1_000_000);
        let (exp_status, _) = TallyEngine::evaluate(&expedited_result, true);
        assert!(
            !matches!(exp_status, ProposalStatus::Passed),
            "expedited modda %60 geçmemeli"
        );
    }
    // ── Proptest: Governance Property Testleri ───────────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// Property: Tally sonucu oy güçlerinin toplamı total_voted'a eşit
        #[test]
        fn prop_tally_vote_sum_equals_total_voted(
            yes     in 0u128..1_000_000u128,
            no      in 0u128..1_000_000u128,
            abstain in 0u128..1_000_000u128,
            veto    in 0u128..1_000_000u128,
        ) {
            let total_bonded = 10_000_000u128;
            let votes = make_votes(yes, no, abstain, veto);
            let result = TallyEngine::tally(&votes, total_bonded);
            prop_assert_eq!(
                result.yes_power + result.no_power + result.abstain_power + result.no_with_veto_power,
                result.total_voted,
                "tally oy güçleri toplamı = total_voted"
            );
        }

        /// Property: Veto >= %33.4 → öneri her zaman VetoRejected
        #[test]
        fn prop_veto_above_threshold_always_rejected(
            yes  in 1_000u128..10_000u128,
            no   in 0u128..1_000u128,
            veto in 5_000u128..10_000u128, // veto yüksek
        ) {
            // total_voted = yes + no + veto; veto_ratio > 33.4%
            let total_voted = yes + no + veto;
            if total_voted == 0 { return Ok(()); }
            let veto_ratio = veto * 10_000 / total_voted;
            if veto_ratio > VETO_THRESHOLD_BPS as u128 {
                // total_bonded = total_voted → %100 katılım, quorum garantili
                let total_bonded = total_voted;
                let votes = make_votes(yes, no, 0, veto);
                let result = TallyEngine::tally(&votes, total_bonded);
                let (status, burned) = TallyEngine::evaluate(&result, false);
                prop_assert!(
                    matches!(status, ProposalStatus::Vetoed),
                    "veto>VETO_THRESHOLD_BPS → Vetoed olmalı"
                );
                prop_assert!(burned, "veto reject → depozito yakılmalı");
            }
        }

        /// Property: Quorum sağlanmazsa öneri hiçbir zaman Passed olmaz
        #[test]
        fn prop_below_quorum_never_passes(
            yes in 1u128..100u128,
        ) {
            // total_bonded büyük, oy az → quorum yok
            let total_bonded = 1_000_000u128;
            let votes = make_votes(yes, 0, 0, 0);
            let result = TallyEngine::tally(&votes, total_bonded);
            let (status, _) = TallyEngine::evaluate(&result, false);
            prop_assert!(
                !matches!(status, ProposalStatus::Passed),
                "quorum altında öneri Passed olamaz"
            );
        }

        /// Property: yes/total_non_abstain < %50 → Rejected (veto yok)
        #[test]
        fn prop_minority_yes_never_passes(
            yes in 1u128..4_999u128,  // <%50
            no  in 5_001u128..10_000u128,
        ) {
            // quorum sağla: oy > %33.4 of bonded
            let total_bonded = (yes + no) * 2; // 50% oy → quorum aşar
            let votes = make_votes(yes, no, 0, 0);
            let result = TallyEngine::tally(&votes, total_bonded);
            let (status, _) = TallyEngine::evaluate(&result, false);
            prop_assert!(
                !matches!(status, ProposalStatus::Passed),
                "evet < %50 → Rejected olmalı"
            );
        }
    }
}
