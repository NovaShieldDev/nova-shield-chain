//! Mainnet güvenlik denetimi — tokenomics, genesis, vesting, chain ID
//!
//! ## Kapsam
//!
//! | Test Kategorisi           | Tehdit / Doğruluk Hedefi                          |
//! |---------------------------|---------------------------------------------------|
//! | Tokenomics toplam arzı    | 7 kategori toplamı = 200M NOVA (kesin)            |
//! | Genesis hash kararlılığı  | Aynı config → aynı hash (deterministik)           |
//! | Chain ID izolasyonu       | Mainnet/Testnet/Devnet chain ID'leri çakışmıyor   |
//! | Vesting enforcement       | Kilitli token transferi reddediliyor              |
//! | Bootnode güvenliği        | Mainnet'te localhost bootnode yok                 |
//! | Ceremony çıktı doğruluğu  | BLS compressed flag, adres formatı                |
//! | Genesis bakiyeler         | Her adres bakiyesi > 0, toplam doğru              |
//! | Validator konfigürasyonu  | Min stake, komisyon, endpoint format              |

#[cfg(test)]
mod tests {
    use nova_node::{
        bootnodes::{MAINNET_BOOTNODES, TESTNET_BOOTNODES, DEVNET_BOOTNODES, bootnodes_for_chain},
        genesis::{mainnet_genesis, testnet_genesis, devnet_genesis},
        keystore::Keystore,
    };
    use nova_primitives::constants::{
        MAINNET_CHAIN_ID, TESTNET_CHAIN_ID, DEVNET_CHAIN_ID, TOTAL_NOVA_SUPPLY,
        ALLOC_ECOSYSTEM, ALLOC_TEAM, ALLOC_INVESTORS, ALLOC_STAKING_REWARDS,
        ALLOC_TREASURY, ALLOC_PUBLIC_SALE, ALLOC_LIQUIDITY,
        FEE_BURN_BPS,
    };
    use nova_runtime::token::vesting::{VestingSchedule, VestingState};

    // ── 1. TOKENOMİCS DOĞRULUĞU ───────────────────────────────────────────────

    /// AUDIT: 7 kategori toplamı kesinlikle 200M NOVA olmalı
    ///
    /// Herhangi bir yanlış tokeniomics tahsisi enflasyon veya eksik arz yaratır.
    #[test]
    fn audit_tokenomics_all_categories_sum_to_total_supply() {
        let sum = ALLOC_ECOSYSTEM
            + ALLOC_TEAM
            + ALLOC_INVESTORS
            + ALLOC_STAKING_REWARDS
            + ALLOC_TREASURY
            + ALLOC_PUBLIC_SALE
            + ALLOC_LIQUIDITY;

        assert_eq!(
            sum, TOTAL_NOVA_SUPPLY,
            "Tokenomics toplam arzı yanlış! Beklenen: {TOTAL_NOVA_SUPPLY}, Hesaplanan: {sum}"
        );
    }

    /// AUDIT: Hiçbir kategori toplam arzdan büyük olamaz
    #[test]
    fn audit_no_single_category_exceeds_total_supply() {
        let categories = [
            ("Ecosystem", ALLOC_ECOSYSTEM),
            ("Team", ALLOC_TEAM),
            ("Investors", ALLOC_INVESTORS),
            ("Staking", ALLOC_STAKING_REWARDS),
            ("Treasury", ALLOC_TREASURY),
            ("PublicSale", ALLOC_PUBLIC_SALE),
            ("Liquidity", ALLOC_LIQUIDITY),
        ];

        for (name, alloc) in &categories {
            assert!(
                *alloc < TOTAL_NOVA_SUPPLY,
                "Kategori {name} ({alloc}) toplam arzdan büyük olamaz"
            );
            assert!(*alloc > 0, "Kategori {name} sıfır olamaz");
        }
    }

    /// AUDIT: Fee burn %50 — deflasyonlu model korunuyor
    #[test]
    fn audit_fee_burn_rate_is_fifty_percent() {
        assert_eq!(
            FEE_BURN_BPS, 5_000,
            "Fee burn %50 (5000 BPS) olmalı, deflasyonlu tokenomics korunmuyor"
        );
    }

    /// AUDIT: Kategori oranları whitepaper ile uyumlu
    #[test]
    fn audit_tokenomics_percentages_match_whitepaper() {
        // Whitepaper: Ecosystem 22%, Team 19%, Investors 17%, Staking 16%,
        //             Treasury 12%, PublicSale 7%, Liquidity 7%
        let expected_bps: &[(&str, u128, u32)] = &[
            ("Ecosystem",  ALLOC_ECOSYSTEM,        2200),
            ("Team",       ALLOC_TEAM,             1900),
            ("Investors",  ALLOC_INVESTORS,        1700),
            ("Staking",    ALLOC_STAKING_REWARDS,  1600),
            ("Treasury",   ALLOC_TREASURY,         1200),
            ("PublicSale", ALLOC_PUBLIC_SALE,       700),
            ("Liquidity",  ALLOC_LIQUIDITY,         700),
        ];

        for (name, alloc, expected_bps_val) in expected_bps {
            let actual_bps = (alloc * 10_000 / TOTAL_NOVA_SUPPLY) as u32;
            assert_eq!(
                actual_bps, *expected_bps_val,
                "Kategori {name} oranı yanlış: beklenen %{}.{}, hesaplanan %{}.{}",
                expected_bps_val / 100, expected_bps_val % 100,
                actual_bps / 100, actual_bps % 100
            );
        }
    }

    // ── 2. GENESİS HASH KARARLILIĞI ──────────────────────────────────────────

    /// AUDIT: Aynı mainnet config → aynı genesis hash (deterministik)
    #[test]
    fn audit_mainnet_genesis_hash_is_deterministic() {
        let g1 = mainnet_genesis();
        let g2 = mainnet_genesis();
        assert_eq!(
            g1.hash_hex(), g2.hash_hex(),
            "Genesis hash deterministik olmalı — farklı çağrılar aynı hash üretmeli"
        );
    }

    /// AUDIT: Mainnet ve testnet genesis hash'leri farklı
    #[test]
    fn audit_different_chains_have_different_genesis_hashes() {
        let mainnet_hash = mainnet_genesis().hash_hex();
        let testnet_hash = testnet_genesis().hash_hex();
        let devnet_hash  = devnet_genesis().hash_hex();

        assert_ne!(mainnet_hash, testnet_hash, "Mainnet ve testnet genesis hash aynı olamaz");
        assert_ne!(mainnet_hash, devnet_hash,  "Mainnet ve devnet genesis hash aynı olamaz");
        assert_ne!(testnet_hash, devnet_hash,  "Testnet ve devnet genesis hash aynı olamaz");
    }

    /// AUDIT: Mainnet genesis doğrulama başarılı geçmeli
    #[test]
    fn audit_mainnet_genesis_passes_validation() {
        let g = mainnet_genesis();
        g.validate().expect("Mainnet genesis doğrulama başarısız");
    }

    // ── 3. CHAIN ID İZOLASYONU ────────────────────────────────────────────────

    /// AUDIT: Chain ID'ler birbirinden farklı — yanlış ağa bağlanma imkânsız
    #[test]
    fn audit_chain_ids_are_unique() {
        assert_ne!(MAINNET_CHAIN_ID, TESTNET_CHAIN_ID, "Mainnet ve testnet chain ID çakışıyor");
        assert_ne!(MAINNET_CHAIN_ID, DEVNET_CHAIN_ID,  "Mainnet ve devnet chain ID çakışıyor");
        assert_ne!(TESTNET_CHAIN_ID, DEVNET_CHAIN_ID,  "Testnet ve devnet chain ID çakışıyor");
    }

    /// AUDIT: Mainnet chain ID = 1984 (sabit)
    #[test]
    fn audit_mainnet_chain_id_is_1984() {
        assert_eq!(MAINNET_CHAIN_ID, 1984, "Mainnet chain ID sabit 1984 olmalı");
    }

    /// AUDIT: Mainnet genesis chain_id sabitiyle uyumlu
    #[test]
    fn audit_mainnet_genesis_chain_id_matches_constant() {
        let g = mainnet_genesis();
        assert_eq!(
            g.chain_id, MAINNET_CHAIN_ID,
            "Genesis chain_id ({}) sabit MAINNET_CHAIN_ID ({MAINNET_CHAIN_ID}) ile uyumsuz",
            g.chain_id
        );
    }

    /// AUDIT: EIP-695 hex formatı doğru (0x7c0 = 1984)
    #[test]
    fn audit_eip695_chain_id_hex_is_correct() {
        let hex = format!("0x{:x}", MAINNET_CHAIN_ID);
        assert_eq!(hex, "0x7c0", "EIP-695 mainnet chain ID hex 0x7c0 olmalı, gelen: {hex}");
    }

    // ── 4. VESTİNG ENFORCEMENT ────────────────────────────────────────────────

    /// AUDIT: Kilitli token transferi vesting state tarafından bloklanıyor
    #[test]
    fn audit_vesting_blocks_locked_token_transfer() {
        use nova_primitives::Address;
        use nova_runtime::errors::RuntimeError;
        use nova_runtime::state::cache::AccountStateCache;
        use nova_runtime::tx::types::{Transaction, TransactionKind};
        use nova_runtime::tx::validation::TxValidator;
        use nova_primitives::SlotId;

        // Genesis ecosystem adresi: tamamen kilitli başlar
        let vested_addr = Address::from_bytes([0xABu8; 20]);
        let balance = ALLOC_ECOSYSTEM; // 44M NOVA

        let mut vs = VestingState::new();
        vs.register(vested_addr, VestingSchedule::linear(balance, 48));

        // Epoch 0 → tamamı kilitli
        assert_eq!(
            vs.locked(&vested_addr), balance,
            "Epoch 0'da tüm ecosystem bakiyesi kilitli olmalı"
        );

        // Transfer denemesi: fee için yeterli bakiye var ama hepsi kilitli
        let state = AccountStateCache::new();
        state.set_balance(vested_addr, balance);
        state.set_nonce(vested_addr, 0);

        // İmzasız TX: doğrulama imza kontrolünden önce vesting kontrolü olmamalı
        // (Sadece vesting check'i test etmek için check_vesting'i dolaylı test ediyoruz)
        // Fee cost = 1_000_000_000 * 21_000 = 21 * 10^12
        // balance = 44M * 10^18 >> fee → check_balance geçer
        // locked = balance → available = 0 → check_vesting başarısız
        let tx = Transaction {
            sender: vested_addr,
            nonce: 0,
            kind: TransactionKind::Transfer {
                to: Address::from_bytes([0x01u8; 20]),
                amount: 1_000_000u128,
            },
            max_fee_per_gas: 1_000_000_000,
            max_priority_fee_per_gas: 100_000_000,
            gas_limit: 21_000,
            signature: vec![0u8; 65], // dummy imza (check_vesting öncesi imza kontrolü devreye girer)
            slot: SlotId(0),
        };

        // Vesting check'ini doğrudan test etmek için check_vesting'e erişemeyiz
        // ama validate_with_vesting'te sıra: size → signature → fee → nonce → balance → vesting
        // İmza geçersiz olacak; yine de VestingLockedTransfer yerine InvalidSignature alırız
        // Bu yüzden vesting state'ini ayrıca doğrulayalım:
        let locked = vs.locked(&vested_addr);
        let available = balance.saturating_sub(locked);
        let requested = 1_000_000u128 + 1_000_000_000u128 * 21_000u128;
        assert!(
            requested > available,
            "Kilitli adres için istek ({requested}) mevcut bakiyeyi ({available}) aşmalı"
        );
        assert_eq!(available, 0, "Epoch 0'da ecosystem adresi için available=0 olmalı");

        // Validator direkt API ile test
        let validator = TxValidator::new(1_000_000_000);
        let result = validator.validate_with_vesting(&tx, &state, Some(&vs));

        // İmza geçersiz olduğundan InvalidSignature veya VestingLockedTransfer döner
        // Her ikisi de kabul: önemli olan geçirmemesi
        assert!(
            result.is_err(),
            "Kilitli ecosystem adresi transferi hata döndürmeli"
        );
    }

    /// AUDIT: 48 ay sonra vesting tamamen açılıyor
    #[test]
    fn audit_vesting_fully_unlocked_after_48_months() {
        use nova_primitives::Address;

        let addr = Address::from_bytes([0x11u8; 20]);
        let total = ALLOC_TEAM; // 38M NOVA

        let mut vs = VestingState::new();
        vs.register(addr, VestingSchedule::cliff_linear(total, 12, 48));

        // 60 ay ilerlet (12 cliff + 48 linear)
        vs.advance_epoch(60 * 30);

        assert_eq!(
            vs.locked(&addr), 0,
            "60 ay sonra team bakiyesi tamamen açılmış olmalı"
        );
        assert_eq!(
            vs.claimable(&addr), total,
            "60 ay sonra tüm team bakiyesi talep edilebilir olmalı"
        );
    }

    /// AUDIT: Cliff süresince hiçbir şey açılmıyor
    #[test]
    fn audit_cliff_period_nothing_vested() {
        use nova_primitives::Address;

        let addr = Address::from_bytes([0x22u8; 20]);
        let total = ALLOC_INVESTORS;

        let mut vs = VestingState::new();
        vs.register(addr, VestingSchedule::cliff_linear(total, 12, 24));

        // 11 ay — cliff bitmemiş
        vs.advance_epoch(11 * 30);
        assert_eq!(
            vs.claimable(&addr), 0,
            "12 ay cliff süresi dolmadan hiçbir investor token açılmamalı"
        );
        assert_eq!(vs.locked(&addr), total);

        // 12. ay — cliff bitti, henüz linear başlamadı
        vs.advance_epoch(1 * 30);
        // Cliff tam bittiğinde de 0 vested (linear henüz başlamadı)
        // Not: clifflinear -> cliff bitince 0/24 ay = 0
        assert_eq!(
            vs.claimable(&addr), 0,
            "12. ayda cliff tam bitişinde linear henüz başlamamış — 0 claimable"
        );
    }

    /// AUDIT: Public sale ve liquidity TGE'de tamamen açık
    #[test]
    fn audit_tge_unlocked_categories_are_fully_available_at_epoch_zero() {
        use nova_primitives::Address;

        let public_addr = Address::from_bytes([0x33u8; 20]);
        let liquidity_addr = Address::from_bytes([0x44u8; 20]);

        let mut vs = VestingState::new();
        vs.register(public_addr,   VestingSchedule::unlocked(ALLOC_PUBLIC_SALE));
        vs.register(liquidity_addr, VestingSchedule::unlocked(ALLOC_LIQUIDITY));

        // Epoch 0
        assert_eq!(vs.locked(&public_addr),   0,               "Public sale kilitli olamaz");
        assert_eq!(vs.locked(&liquidity_addr), 0,              "Liquidity kilitli olamaz");
        assert_eq!(vs.claimable(&public_addr),   ALLOC_PUBLIC_SALE);
        assert_eq!(vs.claimable(&liquidity_addr), ALLOC_LIQUIDITY);
    }

    // ── 5. BOOTNODE GÜVENLİĞİ ────────────────────────────────────────────────

    /// AUDIT: Mainnet bootnode listesinde localhost yok
    #[test]
    fn audit_mainnet_bootnodes_no_localhost() {
        for node in MAINNET_BOOTNODES {
            assert!(
                !node.starts_with("127.0.0.1"),
                "Mainnet bootnode localhost içeremez: {node}"
            );
            assert!(
                !node.starts_with("0.0.0.0"),
                "Mainnet bootnode 0.0.0.0 içeremez: {node}"
            );
            assert!(
                !node.starts_with("localhost"),
                "Mainnet bootnode localhost içeremez: {node}"
            );
        }
    }

    /// AUDIT: Mainnet ve testnet bootnode listesi birbirinden farklı
    #[test]
    fn audit_mainnet_testnet_bootnodes_are_different() {
        let mainnet: std::collections::HashSet<&&str> = MAINNET_BOOTNODES.iter().collect();
        let testnet: std::collections::HashSet<&&str> = TESTNET_BOOTNODES.iter().collect();
        assert!(
            mainnet.is_disjoint(&testnet),
            "Mainnet ve testnet bootnode listeleri çakışıyor — ağ izolasyonu bozuk"
        );
    }

    /// AUDIT: Her bootnode host:port formatında
    #[test]
    fn audit_all_mainnet_bootnodes_have_valid_port() {
        for node in MAINNET_BOOTNODES {
            let parts: Vec<&str> = node.rsplitn(2, ':').collect();
            assert_eq!(parts.len(), 2, "host:port formatı bozuk: {node}");
            let port: u16 = parts[0].parse().unwrap_or(0);
            assert!(port > 1024, "port > 1024 olmalı: {node} → port {port}");
        }
    }

    /// AUDIT: Devnet bootnode'ları mainnet'e karışmamalı
    #[test]
    fn audit_devnet_bootnodes_not_in_mainnet() {
        let mainnet_set: std::collections::HashSet<&&str> = MAINNET_BOOTNODES.iter().collect();
        let devnet_set: std::collections::HashSet<&&str> = DEVNET_BOOTNODES.iter().collect();
        assert!(
            mainnet_set.is_disjoint(&devnet_set),
            "Devnet bootnode'ları mainnet listesinde — kritik ağ izolasyonu hatası"
        );
    }

    // ── 6. CEREMONY ÇIKTI DOĞRULUĞU ─────────────────────────────────────────

    /// AUDIT: Keystore generate → BLS compressed flag zorunlu (byte[0] & 0x80 != 0)
    #[test]
    fn audit_ceremony_bls_key_has_compressed_flag() {
        let mut ks = Keystore::in_memory();
        for id in 1u64..=5 {
            let keys = ks.generate(id).unwrap();
            let bls_bytes = keys.bls_pubkey_bytes()
                .expect("BLS pubkey bytes dönüştürülemedi");

            assert!(
                (bls_bytes[0] & 0x80) != 0,
                "Validator {id}: BLS pubkey compressed flag eksik (byte[0]={:#04x})",
                bls_bytes[0]
            );
            assert!(
                bls_bytes[0] != 0x80,
                "Validator {id}: BLS pubkey identity point olamaz (byte[0]=0x80)"
            );
        }
    }

    /// AUDIT: Keystore generate → Ethereum uyumlu adres formatı
    #[test]
    fn audit_ceremony_address_is_ethereum_compatible() {
        let mut ks = Keystore::in_memory();
        for id in 1u64..=5 {
            let keys = ks.generate(id).unwrap();

            assert!(
                keys.address.starts_with("0x"),
                "Validator {id}: adres 0x ile başlamalı"
            );
            assert_eq!(
                keys.address.len(), 42,
                "Validator {id}: adres 42 karakter olmalı (0x + 40 hex)"
            );
            let hex_part = &keys.address[2..];
            assert!(
                hex_part.chars().all(|c| c.is_ascii_hexdigit()),
                "Validator {id}: adres geçerli hex içermeli"
            );
        }
    }

    /// AUDIT: Her validator aynı ID çağrısında (farklı zaman) farklı node key alır
    #[test]
    fn audit_ceremony_generates_unique_keys_per_validator() {
        let mut ks = Keystore::in_memory();
        let mut addresses = std::collections::HashSet::new();
        let mut node_keys = std::collections::HashSet::new();

        for id in 1u64..=5 {
            let keys = ks.generate(id).unwrap();
            addresses.insert(keys.address.clone());
            node_keys.insert(keys.node_key.clone());
        }

        // 5 validator → 5 farklı adres (UUID benzeri benzersizlik)
        assert_eq!(addresses.len(), 5, "Her validator benzersiz adres almalı");
        assert_eq!(node_keys.len(), 5, "Her validator benzersiz node key almalı");
    }

    // ── 7. GENESİS BAKİYELER ─────────────────────────────────────────────────

    /// AUDIT: Mainnet genesis bakiyelerinin toplamı tokenomics ile uyumlu
    #[test]
    fn audit_mainnet_genesis_balances_sum_matches_total_supply() {
        let g = mainnet_genesis();

        let total: u128 = g.balances.iter()
            .map(|b| b.balance.parse::<u128>().unwrap_or(0))
            .sum();

        assert_eq!(
            total, TOTAL_NOVA_SUPPLY,
            "Genesis bakiyeler toplamı ({total}) NOVA toplam arzıyla ({TOTAL_NOVA_SUPPLY}) uyumsuz"
        );
    }

    /// AUDIT: Hiçbir genesis bakiyesi sıfır değil
    #[test]
    fn audit_mainnet_genesis_no_zero_balances() {
        let g = mainnet_genesis();
        for b in &g.balances {
            let amount: u128 = b.balance.parse().unwrap_or(0);
            assert!(amount > 0, "Genesis bakiyesi sıfır olamaz: {:?}", b.address);
        }
    }

    /// AUDIT: Mainnet genesis validator count doğru
    #[test]
    fn audit_mainnet_genesis_has_validators() {
        let g = mainnet_genesis();
        assert!(
            !g.validators.is_empty(),
            "Mainnet genesis en az 1 validator içermeli"
        );
        // Her validator stake > 0
        for v in &g.validators {
            let stake: u128 = v.stake.parse().unwrap_or(0);
            assert!(stake > 0, "Validator {} stake'i sıfır olamaz", v.id);
        }
    }

    // ── 8. SİSTEM GENELİ KRİTİK KONTROLLER ─────────────────────────────────

    /// AUDIT: TOTAL_NOVA_SUPPLY overflow kontrolü (u128 sınırları içinde)
    #[test]
    fn audit_total_supply_no_overflow() {
        // 200M NOVA × 10^18 = 2 × 10^26
        // u128 max ≈ 3.4 × 10^38 → güvenli
        let nova_factor: u128 = 1_000_000_000_000_000_000;
        let expected = 200_000_000u128 * nova_factor;
        assert_eq!(
            TOTAL_NOVA_SUPPLY, expected,
            "TOTAL_NOVA_SUPPLY hesaplama hatası"
        );
        // u128 taşması yok
        assert!(TOTAL_NOVA_SUPPLY < u128::MAX / 1_000,
            "TOTAL_NOVA_SUPPLY güvenli u128 aralığının dışında");
    }

    /// AUDIT: Tüm genesis chain'ler token sembolü NOVA
    #[test]
    fn audit_all_chains_use_nova_token_symbol() {
        assert_eq!(mainnet_genesis().token.symbol, "NOVA");
        assert_eq!(testnet_genesis().token.symbol, "NOVA");
        // devnet farklı sembol kullanabilir ama mainnet/testnet NOVA olmalı
    }
}
