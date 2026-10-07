//! nova-guard — Nova Shield Chain Cross-chain Asset Protection (§5.6)
//!
//! ## Mimari
//!
//! ### Bitcoin (MPC tabanlı)
//! - `derive_mpc_btc_address(threshold)` → P2WSH adres
//! - `BitcoinTxBuilder`: UTXO coin selection, segwit tx
//!
//! ### Ethereum (go-ethereum DynamicFeeTx pattern)
//! - `EthTx`: EIP-1559 (ChainID, GasTipCap, GasFeeCap, Gas, To, Value, Data, V/R/S)
//! - `BeefyLightClient`: Polkadot BEEFY bridge pallet pattern (MMR root verification)
//!
//! ### Solana
//! - `SolanaTxBuilder`: system_transfer + spl_transfer_checked
//! - Ed25519 mock imzalama
//!
//! ### Cosmos (IBC)
//! - `IbcModule`: Channel state machine, FungibleTokenPacketData (ICS-20)
//! - `IbcTransferMsg`: MsgTransfer pattern (Cosmos SDK)
//!
//! ### Koordinasyon
//! - `CrossChainCoordinator`: vault oluştur + MPC imza + broadcast
//! - `CrossChainInheritanceManager`: §5.6.4 cross-chain deadman switch
//! - `ProtocolFeeCalculator`: §5.6.5 yıllık %0.05 fee

pub mod bitcoin;
pub mod cosmos;
pub mod economics;
pub mod errors;
pub mod ethereum;
pub mod inheritance;
pub mod solana;
pub mod coordinator;
pub mod types;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use errors::{GuardError, GuardResult};
pub use types::{
    AssetAddress, ChainId, CrossChainAsset, CrossChainTxReceipt, CrossChainTxRequest,
    GuardedVault, MpcThreshold, blake3_hash, blake3_hash_many,
};

pub use bitcoin::{
    BitcoinTx, BitcoinTxBuilder, BitcoinTxInput, BitcoinTxOutput, BitcoinUtxo,
    build_multisig_redeem_script, build_p2wsh_script, derive_mpc_btc_address,
};

pub use ethereum::{
    BeefyAuthoritySet, BeefyCommitment, BeefyLightClient, BeefyPayload, BeefySignature,
    BeefySignedCommitment, EthAddress, EthChainId, EthTx, ImportedCommitment, MmrLeaf,
    MAX_COMMITMENTS, MAX_REQUESTS,
};

pub use solana::{
    SolanaInstruction, SolanaMessage, SolanaPubkey, SolanaTx, SolanaTxBuilder,
};

pub use cosmos::{
    FungibleTokenPacketData, IbcChannel, IbcChannelState, IbcModule,
    IbcOrdering, IbcPacket, IbcTransferMsg,
};

pub use coordinator::{BroadcastResult, CrossChainCoordinator, MpcSignSession};

pub use inheritance::{
    CrossChainHeirAllocation, CrossChainInheritanceManager, CrossChainInheritancePlan,
};

pub use economics::{
    FeeCalculation, FeeDistribution, ProtocolFeeCalculator,
    ANNUAL_FEE_BPS, GUARDIAN_SHARE_BPS, SLOTS_PER_YEAR, STAKER_SHARE_BPS,
};

// ─── Integration Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> [u8; 20] {
        [0xAAu8; 20]
    }

    fn guardians(n: usize) -> Vec<[u8; 20]> {
        (0..n).map(|i| [i as u8; 20]).collect()
    }

    // ─── 1: Bitcoin MPC vault oluşturma ────────────────────────────────────

    #[test]
    fn integration_btc_mpc_vault_creation() {
        let coord = CrossChainCoordinator::new();
        let id = coord.create_vault(owner(), ChainId::Bitcoin, 3, guardians(5), 100).unwrap();

        let vault = coord.get_vault(id).unwrap();
        assert_eq!(vault.chain, ChainId::Bitcoin);
        assert!(vault.address.display.starts_with("bc1q"));
        assert_eq!(vault.threshold.threshold, 3);
        assert_eq!(vault.threshold.total, 5);
    }

    // ─── 2: Bitcoin UTXO transferi ─────────────────────────────────────────

    #[test]
    fn integration_btc_send() {
        let coord = CrossChainCoordinator::new();
        let id = coord.create_vault(owner(), ChainId::Bitcoin, 2, guardians(3), 100).unwrap();
        coord.deposit(id, 10_000_000).unwrap(); // 0.1 BTC

        let to = AssetAddress::bitcoin("bc1qrecipient");
        let receipt = coord.send_bitcoin(id, &to, 1_000_000, 5, 200).unwrap();

        assert!(receipt.success);
        assert_eq!(receipt.vault_id, id);
        let vault = coord.get_vault(id).unwrap();
        assert_eq!(vault.balance, 9_000_000);
    }

    // ─── 3: Ethereum EIP-1559 işlemi ───────────────────────────────────────

    #[test]
    fn integration_eth_send() {
        let coord = CrossChainCoordinator::new();
        let id = coord.create_vault(owner(), ChainId::Ethereum, 2, guardians(3), 100).unwrap();
        coord.deposit(id, 2_000_000_000_000_000_000u64).unwrap(); // 2 ETH

        let to = EthAddress([0xFFu8; 20]);
        let receipt = coord.send_ethereum(id, to, 1_000_000_000_000_000_000, 100).unwrap();

        assert!(receipt.success);
        assert_ne!(receipt.tx_hash, [0u8; 32]);
    }

    // ─── 4: Solana SOL transferi ────────────────────────────────────────────

    #[test]
    fn integration_solana_send() {
        let coord = CrossChainCoordinator::new();
        let id = coord.create_vault(owner(), ChainId::Solana, 2, guardians(3), 100).unwrap();
        coord.deposit(id, 5_000_000_000u64).unwrap(); // 5 SOL

        let to = SolanaPubkey([0xBBu8; 32]);
        let receipt = coord.send_solana(id, to, 1_000_000_000, 100).unwrap();

        assert!(receipt.success);
    }

    // ─── 5: Cosmos IBC ATOM gönderimi ──────────────────────────────────────

    #[test]
    fn integration_cosmos_ibc_send() {
        let coord = CrossChainCoordinator::new();
        let id = coord.create_vault(owner(), ChainId::Cosmos, 2, guardians(3), 100).unwrap();
        coord.deposit(id, 10_000_000).unwrap(); // 10 ATOM

        let to = AssetAddress::cosmos([0x99u8; 20]);
        let receipt = coord.send_cosmos_ibc(id, to, 5_000_000, "uatom", 100).unwrap();

        assert!(receipt.success);
        assert_eq!(coord.ibc().pending_count(), 1);
    }

    // ─── 6: BEEFY light client taahhüt doğrulama ───────────────────────────

    #[test]
    fn integration_beefy_light_client() {
        let validators: Vec<[u8; 32]> = (0..10).map(|i| [i as u8; 32]).collect();
        let set = BeefyAuthoritySet::new(1, validators.clone());
        let quorum = set.quorum() as usize; // 7
        let mut lc = BeefyLightClient::initialize(set.clone());

        let commitment = BeefyCommitment::new(1000, 1, [0xBBu8; 32]);
        let hash = commitment.commitment_hash();
        let mut signed = BeefySignedCommitment::new(commitment);

        for i in 0..quorum {
            let sig = BeefySignature::mock_sign(i as u32, &hash, &validators[i]);
            signed.add_signature(sig);
        }

        let imported = lc.submit_signed_commitment(signed).unwrap();
        assert_eq!(imported.block_number, 1000);
        assert!(lc.is_block_finalized(1000));
    }

    // ─── 7: MPC imza oturumu ───────────────────────────────────────────────

    #[test]
    fn integration_mpc_sign_session() {
        let coord = CrossChainCoordinator::new();
        let g = guardians(5);
        let id = coord.create_vault(owner(), ChainId::Bitcoin, 3, g.clone(), 100).unwrap();

        let msg = [0xFFu8; 32];
        let session_id = coord.initiate_sign(id, msg).unwrap();

        // 3 guardian imzalaması gerekiyor
        let c1 = coord.guardian_approve(session_id, g[0]).unwrap();
        assert!(!c1);
        let c2 = coord.guardian_approve(session_id, g[1]).unwrap();
        assert!(!c2);
        let c3 = coord.guardian_approve(session_id, g[2]).unwrap();
        assert!(c3); // eşik sağlandı

        let sig = coord.get_signature(session_id).unwrap();
        assert_ne!(sig, [0u8; 32]);
    }

    // ─── 8: Cross-chain miras planı ────────────────────────────────────────

    #[test]
    fn integration_cross_chain_inheritance() {
        let mgr = CrossChainInheritanceManager::new();

        let plan_id = mgr.create_plan(owner(), vec![1, 2, 3], 100);

        mgr.add_heir_to_plan(
            plan_id, "Alice",
            vec![
                AssetAddress::bitcoin("bc1qalice"),
                AssetAddress::ethereum([0xA1u8; 20]),
            ],
            6_000,
        ).unwrap();

        mgr.add_heir_to_plan(
            plan_id, "Bob",
            vec![
                AssetAddress::solana([0xB0u8; 32]),
                AssetAddress::cosmos([0xB1u8; 20]),
            ],
            4_000,
        ).unwrap();

        let plan = mgr.get_plan(plan_id).unwrap();
        plan.validate_shares().unwrap();

        // 4 zincir kapsanıyor
        let chains = plan.covered_chains();
        assert_eq!(chains.len(), 4);

        // Trigger
        mgr.trigger_plan(plan_id, 5000).unwrap();
        assert_eq!(mgr.triggered_plans().len(), 1);
    }

    // ─── 9: Protocol fee hesabı ────────────────────────────────────────────

    #[test]
    fn integration_protocol_fee() {
        let calc = ProtocolFeeCalculator::default_price();

        // $100,000 BTC vault
        let fee_btc = calc.calculate_for_chain(100_000 * 1_000_000, &ChainId::Bitcoin).unwrap();
        // $100,000 SOL vault
        let fee_sol = calc.calculate_for_chain(100_000 * 1_000_000, &ChainId::Solana).unwrap();

        // BTC fee yıllık $50 (100,000 * 0.05%)
        assert_eq!(fee_btc.annual_fee_usd, 50);

        // SOL 80% çarpanı → $40
        assert_eq!(fee_sol.annual_fee_usd, 40);

        // Dağıtım doğrulaması
        let dist = FeeDistribution::new(1000, 1_000_000);
        assert!(dist.verify_total());
    }

    // ─── 10: Çok zincirli vault portföyü ───────────────────────────────────

    #[test]
    fn integration_multi_chain_portfolio() {
        let coord = CrossChainCoordinator::new();
        let g = guardians(3);

        // 4 zincirde vault
        let btc_id = coord.create_vault(owner(), ChainId::Bitcoin, 2, g.clone(), 1).unwrap();
        let eth_id = coord.create_vault(owner(), ChainId::Ethereum, 2, g.clone(), 1).unwrap();
        let sol_id = coord.create_vault(owner(), ChainId::Solana, 2, g.clone(), 1).unwrap();
        let cos_id = coord.create_vault(owner(), ChainId::Cosmos, 2, g.clone(), 1).unwrap();

        coord.deposit(btc_id, 1_000_000).unwrap();
        coord.deposit(eth_id, 1_000_000_000_000_000_000u64).unwrap();
        coord.deposit(sol_id, 1_000_000_000).unwrap();
        coord.deposit(cos_id, 1_000_000).unwrap();

        assert_eq!(coord.vault_count(), 4);

        // Her vault'tan gönderim
        coord.send_bitcoin(btc_id, &AssetAddress::bitcoin("bc1q"), 100_000, 5, 1).unwrap();
        coord.send_ethereum(eth_id, EthAddress([0xFFu8; 20]), 100_000_000_000_000_000, 1).unwrap();
        coord.send_solana(sol_id, SolanaPubkey([0xBBu8; 32]), 100_000_000, 1).unwrap();
        coord.send_cosmos_ibc(cos_id, AssetAddress::cosmos([0x99u8; 20]), 100_000, "uatom", 1).unwrap();

        // Tüm vaultların bakiyeleri güncellendi
        assert_eq!(coord.get_vault(btc_id).unwrap().balance, 900_000);
    }

    // ─── 11: EthTx ERC-20 calldata ─────────────────────────────────────────

    #[test]
    fn integration_erc20_tx() {
        let to = EthAddress([0xEEu8; 20]);
        let contract = EthAddress([0xCCu8; 20]);
        let data = EthTx::erc20_transfer_data(to, 1_000_000_000_000_000_000);

        let mut tx = EthTx::new(
            EthChainId::MAINNET, 5,
            1_500_000_000, 20_000_000_000,
            65_000, Some(contract), 0, data,
        ).unwrap();

        tx.validate().unwrap();

        let from = EthAddress([0x01u8; 20]);
        let key = [0x42u8; 32];
        tx.sign(&from, &key);

        assert!(tx.from.is_some());
        assert_ne!(tx.hash(), [0u8; 32]);
    }

    // ─── 12: IBC kanal yönetimi ─────────────────────────────────────────────

    #[test]
    fn integration_ibc_channel_management() {
        let module = IbcModule::new();

        // Cosmos Hub ↔ Osmosis
        module.register_channel(IbcChannel::new_transfer("channel-0", "channel-1"));
        // Cosmos Hub ↔ Juno
        module.register_channel(IbcChannel::new_transfer("channel-207", "channel-42"));

        // İki farklı kanaldan transfer
        for channel in ["channel-0", "channel-207"] {
            let msg = IbcTransferMsg::new(
                channel, "uatom", 1_000_000,
                AssetAddress::cosmos([0x01u8; 20]),
                AssetAddress::cosmos([0x02u8; 20]),
                1000,
            );
            module.send_transfer(msg).unwrap();
        }

        assert_eq!(module.pending_count(), 2);
    }

    // ─── 13: Bitcoin multisig redeem script ────────────────────────────────

    #[test]
    fn integration_btc_multisig_script() {
        let g: Vec<[u8; 20]> = (0..5).map(|i| [i as u8; 20]).collect();
        let threshold = MpcThreshold::new(3, 5, g.clone()).unwrap();

        let addr = derive_mpc_btc_address(&threshold);
        let script = build_multisig_redeem_script(&threshold);
        let p2wsh = build_p2wsh_script(&addr.raw);

        // P2WSH: OP_0 + 32 byte
        assert_eq!(p2wsh.len(), 34);
        assert_eq!(p2wsh[0], 0x00);

        // Redeem script: OP_3 ... OP_5 OP_CHECKMULTISIG
        assert_eq!(script[0], 0x53); // OP_3
        assert_eq!(*script.last().unwrap(), 0xAE); // OP_CHECKMULTISIG
    }
}
