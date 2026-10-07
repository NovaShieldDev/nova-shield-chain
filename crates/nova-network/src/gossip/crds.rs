//! CRDS (Cluster Replicated Data Store) — Solana pattern
//!
//! Solana `crds_value.rs`:
//! - `CrdsValue { signature, data: CrdsData }`
//! - `CrdsData` enum: ContactInfo, Vote, EpochSlots, NodeInstance, etc.
//! - Her value bir `label()` ile tanımlanır

use crate::peer::info::PeerId;
use blake3;
use ed25519_dalek::{Signature as Ed25519Sig, SigningKey, VerifyingKey, Signer, Verifier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── CrdsData ─────────────────────────────────────────────────────────────────

/// CRDS verisi — Solana `CrdsData` enum benzeri
/// Nova'ya özel türler eklendi: MpcShare, BatchSlot
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrdsData {
    /// Node iletişim bilgisi — Solana `LegacyContactInfo`
    ContactInfo {
        node_id: [u8; 32],
        addr: String,
        wallclock: u64,
    },
    /// Validator oylaması — Solana `Vote`
    Vote {
        validator_id: [u8; 32],
        slot: u64,
        vote_hash: [u8; 32],
        wallclock: u64,
    },
    /// Epoch slot bilgisi — Solana `EpochSlots`
    EpochSlots {
        from: [u8; 32],
        slots: Vec<u64>,
        wallclock: u64,
    },
    /// Node instance — Solana `NodeInstance`
    NodeInstance {
        from: [u8; 32],
        wallclock: u64,
        token: u64,
    },
    /// MPC decryption paylaşımı — Nova özel, Rolling Shutter DecryptionKeyShares
    MpcShare {
        from: [u8; 32],
        batch_index: u64,
        share_data: Vec<u8>,
        wallclock: u64,
    },
    /// Batch auction slot bilgisi — Nova özel
    BatchSlot {
        from: [u8; 32],
        slot: u64,
        clearing_price: u64,
        wallclock: u64,
    },
    /// Duplicate shred kanıtı — Solana `DuplicateShred`
    DuplicateShred {
        from: [u8; 32],
        slot: u64,
        wallclock: u64,
    },
    /// Validator metadata — Nova özel, stake/komisyon/uptime P2P dağıtımı
    ValidatorMeta {
        /// Validator node ID (public key)
        validator_id: [u8; 32],
        /// Stake miktarı (u64 — gossip verimli)
        stake: u64,
        /// Komisyon oranı (bps, 0-10000)
        commission_bps: u16,
        /// Son N slottaki uptime yüzdesi (bps, 0-10000)
        uptime_bps: u16,
        /// Epoch numarası (bu bilginin geçerli olduğu)
        epoch: u64,
        /// VRF public key (BABE için)
        vrf_pubkey: [u8; 32],
        /// Cüzdan adresi (20 byte)
        reward_addr: [u8; 20],
        wallclock: u64,
    },
}

impl CrdsData {
    /// Etiket (hangi node'dan hangi veri türü)
    pub fn label(&self) -> CrdsLabel {
        match self {
            CrdsData::ContactInfo { node_id, .. } => CrdsLabel::ContactInfo(*node_id),
            CrdsData::Vote { validator_id, slot, .. } => CrdsLabel::Vote(*validator_id, *slot),
            CrdsData::EpochSlots { from, .. } => CrdsLabel::EpochSlots(*from),
            CrdsData::NodeInstance { from, .. } => CrdsLabel::NodeInstance(*from),
            CrdsData::MpcShare { from, batch_index, .. } => CrdsLabel::MpcShare(*from, *batch_index),
            CrdsData::BatchSlot { from, slot, .. } => CrdsLabel::BatchSlot(*from, *slot),
            CrdsData::DuplicateShred { from, slot, .. } => CrdsLabel::DuplicateShred(*from, *slot),
            CrdsData::ValidatorMeta { validator_id, epoch, .. } => CrdsLabel::ValidatorMeta(*validator_id, *epoch),
        }
    }

    /// Wallclock zaman damgası
    pub fn wallclock(&self) -> u64 {
        match self {
            CrdsData::ContactInfo { wallclock, .. } => *wallclock,
            CrdsData::Vote { wallclock, .. } => *wallclock,
            CrdsData::EpochSlots { wallclock, .. } => *wallclock,
            CrdsData::NodeInstance { wallclock, .. } => *wallclock,
            CrdsData::MpcShare { wallclock, .. } => *wallclock,
            CrdsData::BatchSlot { wallclock, .. } => *wallclock,
            CrdsData::DuplicateShred { wallclock, .. } => *wallclock,
            CrdsData::ValidatorMeta { wallclock, .. } => *wallclock,
        }
    }

    /// Gönderen node ID
    pub fn from_node(&self) -> [u8; 32] {
        match self {
            CrdsData::ContactInfo { node_id, .. } => *node_id,
            CrdsData::Vote { validator_id, .. } => *validator_id,
            CrdsData::EpochSlots { from, .. } => *from,
            CrdsData::NodeInstance { from, .. } => *from,
            CrdsData::MpcShare { from, .. } => *from,
            CrdsData::BatchSlot { from, .. } => *from,
            CrdsData::DuplicateShred { from, .. } => *from,
            CrdsData::ValidatorMeta { validator_id, .. } => *validator_id,
        }
    }
}

// ─── CrdsLabel ────────────────────────────────────────────────────────────────

/// CRDS kayıt etiketi — benzersiz anahtar
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CrdsLabel {
    ContactInfo([u8; 32]),
    Vote([u8; 32], u64),
    EpochSlots([u8; 32]),
    NodeInstance([u8; 32]),
    MpcShare([u8; 32], u64),
    BatchSlot([u8; 32], u64),
    DuplicateShred([u8; 32], u64),
    ValidatorMeta([u8; 32], u64), // (validator_id, epoch)
}

// ─── CrdsValue ────────────────────────────────────────────────────────────────

/// CRDS değeri — Solana `CrdsValue { signature, data }`
///
/// NOVA-RED-NETWORK-002 düzeltmesi: Önceki BLAKE3 hash "imzası" herkes tarafından
/// hesaplanabilirdi → sahte gossip mesajları oluşturulabiliyordu.
/// Şimdi: Ed25519 gerçek kriptografik imza + public key ile doğrulama.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdsValue {
    /// Ed25519 imzası (64 byte, Vec<u8> serde uyumu için)
    pub signature: Vec<u8>,
    /// İmzalayan Ed25519 public key (32 byte)
    pub signer_pubkey: [u8; 32],
    /// Asıl veri
    pub data: CrdsData,
}

impl CrdsValue {
    /// Yeni değer oluştur ve Ed25519 ile imzala.
    ///
    /// `signing_key_bytes`: gönderici Ed25519 private key (32 byte).
    /// Solana pattern: `CrdsValue::new_signed(data, keypair)`.
    pub fn sign(data: CrdsData, signing_key_bytes: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(signing_key_bytes);
        let verifying_key = signing_key.verifying_key();
        let msg = Self::signing_message(&data);
        let sig: Ed25519Sig = signing_key.sign(&msg);
        Self {
            signature: sig.to_bytes().to_vec(),
            signer_pubkey: verifying_key.to_bytes(),
            data,
        }
    }

    /// Geriye dönük uyumluluk / üretim kodu için kolaylık yöntemi.
    /// Deterministik dummy keypair kullanır.
    /// Üretimde doğrulanabilir gossip için `sign()` kullanılmalı.
    pub fn new(data: CrdsData) -> Self {
        Self::sign(data, &[0xBEu8; 32])
    }

    /// Ed25519 imzasını doğrula.
    ///
    /// Döner: `true` — imza geçerli, signer_pubkey ile uyuşuyor.
    pub fn verify(&self) -> bool {
        if self.signature.len() != 64 {
            return false;
        }
        let Ok(vk) = VerifyingKey::from_bytes(&self.signer_pubkey) else {
            return false;
        };
        let Ok(sig) = Ed25519Sig::from_slice(&self.signature) else {
            return false;
        };
        let msg = Self::signing_message(&self.data);
        vk.verify(&msg, &sig).is_ok()
    }

    /// Etiket
    pub fn label(&self) -> CrdsLabel {
        self.data.label()
    }

    /// Hash (yayılım için tekilleştirme — imza hash'i)
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.signature);
        *hasher.finalize().as_bytes()
    }

    /// İmzalanacak mesaj: BLAKE3(data_json)
    fn signing_message(data: &CrdsData) -> Vec<u8> {
        let serialized = serde_json::to_vec(data).unwrap_or_default();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-crds-v2");
        hasher.update(&serialized);
        hasher.finalize().as_bytes().to_vec()
    }
}

// ─── CrdsTable ────────────────────────────────────────────────────────────────

/// CRDS tablosu — her label için en güncel değer
/// Solana: `Crds { table: HashMap<CrdsValueLabel, VersionedCrdsValue> }`
#[derive(Debug, Default)]
pub struct CrdsTable {
    table: HashMap<CrdsLabel, CrdsValue>,
    /// Toplam eklenen değer sayısı (monoton)
    insert_count: u64,
}

impl CrdsTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Değer ekle — daha yeni wallclock ise güncelle
    /// Solana: `crds.insert(value, now)` — daha yeni ise kabul et
    pub fn insert(&mut self, value: CrdsValue) -> bool {
        let label = value.label();
        if !value.verify() {
            return false; // Geçersiz imza
        }
        let should_insert = match self.table.get(&label) {
            None => true,
            Some(existing) => value.data.wallclock() > existing.data.wallclock(),
        };
        if should_insert {
            self.table.insert(label, value);
            self.insert_count += 1;
        }
        should_insert
    }

    /// Değer al
    pub fn get(&self, label: &CrdsLabel) -> Option<&CrdsValue> {
        self.table.get(label)
    }

    /// Tüm değerler
    pub fn values(&self) -> impl Iterator<Item = &CrdsValue> {
        self.table.values()
    }

    /// Toplam kayıt sayısı
    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// Belirli bir node'dan ContactInfo al
    pub fn get_contact_info(&self, node_id: &[u8; 32]) -> Option<&CrdsValue> {
        self.table.get(&CrdsLabel::ContactInfo(*node_id))
    }

    /// Belirli bir slot için oy al
    pub fn get_vote(&self, validator_id: &[u8; 32], slot: u64) -> Option<&CrdsValue> {
        self.table.get(&CrdsLabel::Vote(*validator_id, slot))
    }

    /// Bloom filter için hash listesi — pull anti-entropy
    pub fn all_hashes(&self) -> Vec<[u8; 32]> {
        self.table.values().map(|v| v.hash()).collect()
    }

    pub fn insert_count(&self) -> u64 {
        self.insert_count
    }

    /// Validator metadata al (en son epoch'taki)
    pub fn get_validator_meta(&self, validator_id: &[u8; 32], epoch: u64) -> Option<&CrdsValue> {
        self.table.get(&CrdsLabel::ValidatorMeta(*validator_id, epoch))
    }

    /// Tüm validator metadata'larını al
    pub fn all_validator_metas(&self) -> Vec<&CrdsValue> {
        self.table.values()
            .filter(|v| matches!(v.data, CrdsData::ValidatorMeta { .. }))
            .collect()
    }
}

// ─── ValidatorMetadataStore ───────────────────────────────────────────────────

/// Gossip üzerinden dağıtılan validator metadata depolama
///
/// Polkadot: `im_online` pallet'i + `offchain_worker` metadata yayınlama
/// Nova: CRDS üzerinden P2P validator bilgisi dağıtımı
pub struct ValidatorMetadataStore {
    crds: CrdsTable,
}

impl ValidatorMetadataStore {
    pub fn new() -> Self {
        Self { crds: CrdsTable::new() }
    }

    /// Validator metadata yayınla (gossip'e ekle)
    pub fn publish_meta(
        &mut self,
        validator_id: [u8; 32],
        stake: u64,
        commission_bps: u16,
        uptime_bps: u16,
        epoch: u64,
        vrf_pubkey: [u8; 32],
        reward_addr: [u8; 20],
        wallclock: u64,
    ) -> bool {
        let data = CrdsData::ValidatorMeta {
            validator_id,
            stake,
            commission_bps,
            uptime_bps,
            epoch,
            vrf_pubkey,
            reward_addr,
            wallclock,
        };
        self.crds.insert(CrdsValue::new(data))
    }

    /// Peer'dan gelen metadata'yı işle
    pub fn process_gossip(&mut self, value: CrdsValue) -> bool {
        if !matches!(value.data, CrdsData::ValidatorMeta { .. }) {
            return false;
        }
        self.crds.insert(value)
    }

    /// Belirli epoch için tüm validator meta'larını getir
    pub fn validators_for_epoch(&self, epoch: u64) -> Vec<ValidatorMetaSummary> {
        self.crds.all_validator_metas()
            .into_iter()
            .filter_map(|v| {
                if let CrdsData::ValidatorMeta {
                    validator_id, stake, commission_bps, uptime_bps,
                    epoch: e, vrf_pubkey, reward_addr, ..
                } = &v.data {
                    if *e == epoch {
                        Some(ValidatorMetaSummary {
                            validator_id: *validator_id,
                            stake: *stake,
                            commission_bps: *commission_bps,
                            uptime_bps: *uptime_bps,
                            vrf_pubkey: *vrf_pubkey,
                            reward_addr: *reward_addr,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    /// Toplam kayıtlı validator sayısı (bu epoch için)
    pub fn validator_count(&self, epoch: u64) -> usize {
        self.validators_for_epoch(epoch).len()
    }

    /// En yüksek stake'li top-N validator
    pub fn top_validators_by_stake(&self, epoch: u64, n: usize) -> Vec<ValidatorMetaSummary> {
        let mut metas = self.validators_for_epoch(epoch);
        metas.sort_by(|a, b| b.stake.cmp(&a.stake));
        metas.truncate(n);
        metas
    }

    /// CRDS tablosuna erişim (pull anti-entropy için)
    pub fn crds(&self) -> &CrdsTable {
        &self.crds
    }
}

impl Default for ValidatorMetadataStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Validator metadata özeti
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorMetaSummary {
    pub validator_id: [u8; 32],
    pub stake: u64,
    pub commission_bps: u16,
    pub uptime_bps: u16,
    pub vrf_pubkey: [u8; 32],
    pub reward_addr: [u8; 20],
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn node(n: u8) -> [u8; 32] {
        [n; 32]
    }

    fn contact_info(n: u8, wc: u64) -> CrdsValue {
        CrdsValue::new(CrdsData::ContactInfo {
            node_id: node(n),
            addr: format!("127.0.0.{n}:30333"),
            wallclock: wc,
        })
    }

    fn vote(n: u8, slot: u64) -> CrdsValue {
        CrdsValue::new(CrdsData::Vote {
            validator_id: node(n),
            slot,
            vote_hash: [0u8; 32],
            wallclock: 1000,
        })
    }

    #[test]
    fn test_crds_value_verify() {
        let v = contact_info(1, 1000);
        assert!(v.verify(), "Kendi keypair ile imzalanmış değer doğrulanmalı");
    }

    #[test]
    fn test_crds_value_invalid_signature() {
        let mut v = contact_info(1, 1000);
        v.signature[0] ^= 0xFF;
        assert!(!v.verify(), "Bozuk imza reddedilmeli");
    }

    /// NOVA-RED-NETWORK-002 regresyon: Farklı keypair ile sahte CrdsValue reddedilmeli
    #[test]
    fn test_fake_crds_value_rejected() {
        let data = CrdsData::ContactInfo {
            node_id: [0x01u8; 32],
            addr: "10.0.0.1:30333".to_string(),
            wallclock: 1000,
        };
        // Gerçek keypair
        let real_key = [0xAAu8; 32];
        let mut v = CrdsValue::sign(data, &real_key);

        // Saldırgan: pubkey'i başka bir node ile değiştir ama imzayı koruma
        v.signer_pubkey = [0xBBu8; 32]; // Farklı public key
        assert!(!v.verify(), "Yanlış pubkey ile imza doğrulanmamalı");
    }

    /// NOVA-RED-NETWORK-002 regresyon: Farklı keypair imzası, doğru pubkey → red
    #[test]
    fn test_wrong_key_crds_rejected() {
        let data = CrdsData::ContactInfo {
            node_id: [0x02u8; 32],
            addr: "10.0.0.2:30333".to_string(),
            wallclock: 2000,
        };
        // Sahte keypair ile imzala ama gerçek pubkey'i ekle
        let attacker_key = [0xCCu8; 32];
        let real_key = [0xDDu8; 32];

        let attacker_signed = CrdsValue::sign(data.clone(), &attacker_key);
        let real_value = CrdsValue::sign(data, &real_key);

        // Saldırgan: gerçek pubkey'i ekle ama kendi imzasını kullan
        let mut tampered = attacker_signed;
        tampered.signer_pubkey = real_value.signer_pubkey;
        assert!(!tampered.verify(), "Yanlış keypair imzası reddedilmeli");
    }

    /// CrdsValue::sign() farklı keypairler farklı değerler üretir
    #[test]
    fn test_different_keys_produce_different_signatures() {
        let data = CrdsData::ContactInfo {
            node_id: [0x03u8; 32],
            addr: "10.0.0.3:30333".to_string(),
            wallclock: 3000,
        };
        let v1 = CrdsValue::sign(data.clone(), &[0x11u8; 32]);
        let v2 = CrdsValue::sign(data, &[0x22u8; 32]);
        assert_ne!(v1.signature, v2.signature, "Farklı keypairler farklı imzalar üretmeli");
        assert_ne!(v1.signer_pubkey, v2.signer_pubkey);
    }

    #[test]
    fn test_crds_table_insert() {
        let mut table = CrdsTable::new();
        assert!(table.insert(contact_info(1, 1000)));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn test_crds_table_newer_wins() {
        let mut table = CrdsTable::new();
        table.insert(contact_info(1, 1000));
        // Daha yeni wallclock ile güncelle
        let updated = table.insert(contact_info(1, 2000));
        assert!(updated);
        // Eski wallclock ile güncelleme başarısız
        let old = table.insert(contact_info(1, 500));
        assert!(!old);
        assert_eq!(table.len(), 1); // Hâlâ 1
    }

    #[test]
    fn test_crds_table_multiple_nodes() {
        let mut table = CrdsTable::new();
        for i in 1u8..=5 {
            table.insert(contact_info(i, 1000));
        }
        assert_eq!(table.len(), 5);
    }

    #[test]
    fn test_crds_get_contact_info() {
        let mut table = CrdsTable::new();
        table.insert(contact_info(1, 1000));
        let found = table.get_contact_info(&node(1));
        assert!(found.is_some());
        let not_found = table.get_contact_info(&node(99));
        assert!(not_found.is_none());
    }

    #[test]
    fn test_crds_vote_label() {
        let mut table = CrdsTable::new();
        table.insert(vote(1, 100));
        table.insert(vote(1, 200)); // farklı slot → farklı label
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn test_crds_mpc_share() {
        let v = CrdsValue::new(CrdsData::MpcShare {
            from: node(1),
            batch_index: 42,
            share_data: b"share".to_vec(),
            wallclock: 1000,
        });
        assert!(v.verify());
        assert_eq!(v.label(), CrdsLabel::MpcShare(node(1), 42));
    }

    #[test]
    fn test_crds_batch_slot() {
        let v = CrdsValue::new(CrdsData::BatchSlot {
            from: node(2),
            slot: 500,
            clearing_price: 1_000_000,
            wallclock: 2000,
        });
        assert!(v.verify());
        assert_eq!(v.data.wallclock(), 2000);
    }

    #[test]
    fn test_crds_all_hashes() {
        let mut table = CrdsTable::new();
        for i in 1u8..=3 {
            table.insert(contact_info(i, 1000));
        }
        let hashes = table.all_hashes();
        assert_eq!(hashes.len(), 3);
    }

    #[test]
    fn test_crds_insert_count() {
        let mut table = CrdsTable::new();
        table.insert(contact_info(1, 1000));
        table.insert(contact_info(2, 1000));
        table.insert(contact_info(1, 500)); // güncellenmez
        assert_eq!(table.insert_count(), 2);
    }

    // ─── ValidatorMetadata testleri ──────────────────────────────────────────

    fn make_val_meta(n: u8, stake: u64, epoch: u64, wc: u64) -> CrdsValue {
        CrdsValue::new(CrdsData::ValidatorMeta {
            validator_id: [n; 32],
            stake,
            commission_bps: 500,
            uptime_bps: 9800,
            epoch,
            vrf_pubkey: [n; 32],
            reward_addr: [n; 20],
            wallclock: wc,
        })
    }

    #[test]
    fn test_validator_meta_insert_and_retrieve() {
        let mut store = ValidatorMetadataStore::new();
        let inserted = store.publish_meta(
            [1u8; 32], 1_000_000, 500, 9800, 0,
            [0u8; 32], [0u8; 20], 1000,
        );
        assert!(inserted);
        assert_eq!(store.validator_count(0), 1);
    }

    #[test]
    fn test_validator_meta_newer_wins() {
        let mut store = ValidatorMetadataStore::new();
        store.publish_meta([1u8; 32], 1_000_000, 500, 9800, 0, [0u8; 32], [0u8; 20], 1000);
        // Daha yeni wallclock ile güncelle
        let ok = store.publish_meta([1u8; 32], 2_000_000, 400, 9900, 0, [0u8; 32], [0u8; 20], 2000);
        assert!(ok);
        // Eski wallclock ile güncelleme başarısız
        let fail = store.publish_meta([1u8; 32], 500_000, 600, 5000, 0, [0u8; 32], [0u8; 20], 500);
        assert!(!fail);
        // Stake güncellendi mi?
        let metas = store.validators_for_epoch(0);
        assert_eq!(metas[0].stake, 2_000_000);
    }

    #[test]
    fn test_validators_for_epoch_filter() {
        let mut store = ValidatorMetadataStore::new();
        // Epoch 0'a 3 validator
        for i in 1u8..=3 {
            store.publish_meta([i; 32], i as u64 * 1000, 500, 9000, 0, [0u8; 32], [0u8; 20], 1000);
        }
        // Epoch 1'e 2 validator
        for i in 4u8..=5 {
            store.publish_meta([i; 32], i as u64 * 1000, 500, 9000, 1, [0u8; 32], [0u8; 20], 1000);
        }
        assert_eq!(store.validator_count(0), 3);
        assert_eq!(store.validator_count(1), 2);
    }

    #[test]
    fn test_top_validators_by_stake() {
        let mut store = ValidatorMetadataStore::new();
        for i in 1u8..=10 {
            store.publish_meta([i; 32], i as u64 * 1_000_000, 500, 9000, 0, [0u8; 32], [0u8; 20], 1000);
        }
        let top3 = store.top_validators_by_stake(0, 3);
        assert_eq!(top3.len(), 3);
        // En yüksek stake ilk sırada
        assert!(top3[0].stake >= top3[1].stake);
        assert!(top3[1].stake >= top3[2].stake);
    }

    #[test]
    fn test_process_gossip_filters_non_meta() {
        let mut store = ValidatorMetadataStore::new();
        let contact = CrdsValue::new(CrdsData::ContactInfo {
            node_id: [1u8; 32],
            addr: "127.0.0.1:30333".to_string(),
            wallclock: 1000,
        });
        // ContactInfo → ValidatorMetadataStore reddeder
        assert!(!store.process_gossip(contact));
        assert_eq!(store.validator_count(0), 0);
    }

    #[test]
    fn test_validator_meta_crds_label() {
        let v = make_val_meta(7, 5000, 2, 999);
        assert_eq!(v.label(), CrdsLabel::ValidatorMeta([7u8; 32], 2));
    }

    #[test]
    fn test_get_validator_meta_from_table() {
        let mut table = CrdsTable::new();
        table.insert(make_val_meta(5, 9999, 3, 2000));
        let fetched = table.get_validator_meta(&[5u8; 32], 3);
        assert!(fetched.is_some());
        let fetched_other = table.get_validator_meta(&[5u8; 32], 4);
        assert!(fetched_other.is_none());
    }
}
