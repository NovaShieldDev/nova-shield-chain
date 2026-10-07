//! GÖREV 85: BLS12-381 Light Client Kanıtları (BEEFY/Ethereum Bridge pattern)
//!
//! ## Mimari
//!
//! ### Ethereum Beacon Chain pattern (Polkadot BEEFY'den ilham)
//! - `SyncCommittee`: 512 validator, BLS pubkey'ler, aggregate pubkey
//! - `LightClientHeader`: Beacon block header (slot, proposer_index, root'lar)
//! - `LightClientUpdate`: sync committee imzalı blok güncellemesi
//! - `BLS12381LightClient`: güncelleme doğrulama + sync committee geçişi
//!
//! ### BEEFY (Bridge Efficiency Enabling Finality Yielder — Polkadot)
//! - Her epoch'ta yeni sync committee belirlenir (SYNC_COMMITTEE_SIZE=512)
//! - 2/3 quorum imzası → finality kanıtı
//! - Light node tüm zinciri indirmeden sadece header'ı doğrular

use crate::errors::{NetworkError, NetworkResult};
use std::collections::BTreeMap;

// Gerçek BLS12-381 (blst crate — Supranational/Ethereum Foundation)
use blst::{
    min_pk::{
        AggregatePublicKey, AggregateSignature as BlstAggSig,
        PublicKey as BlstPk, SecretKey as BlstSk, Signature as BlstSig,
    },
    BLST_ERROR,
};

/// Beacon chain domain separation tag (Ethereum Altair uyumlu)
const BLS_DST: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Ethereum Beacon Chain sync committee boyutu
pub const SYNC_COMMITTEE_SIZE: usize = 512;

/// Finality için gereken minimum imza (2/3 quorum)
pub const MIN_SYNC_COMMITTEE_PARTICIPANTS: usize = (SYNC_COMMITTEE_SIZE * 2) / 3; // 341

/// Epoch başına slot sayısı (Ethereum: 32, Nova: 2400 session slot)
pub const SLOTS_PER_EPOCH: u64 = 2400;

/// Sync committee period (epoch başına)
pub const EPOCHS_PER_SYNC_COMMITTEE_PERIOD: u64 = 256;

pub const SLOTS_PER_SYNC_COMMITTEE_PERIOD: u64 =
    SLOTS_PER_EPOCH * EPOCHS_PER_SYNC_COMMITTEE_PERIOD;

// ─── BLS Pubkey ───────────────────────────────────────────────────────────────

/// BLS12-381 G1 public key (48 byte compressed)
/// Üretimde: blst::PublicKey — burada mock (hash tabanlı)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlsPubkey(pub [u8; 48]);

impl BlsPubkey {
    pub fn from_validator_id(id: u64) -> Self {
        let mut b = [0u8; 48];
        b[..8].copy_from_slice(&id.to_le_bytes());
        b[8] = 0x80; // compressed G1 point prefix
        Self(b)
    }

    /// Aggregate pubkey: validator seti imzalarının toplamı (mock: XOR + hash)
    pub fn aggregate(keys: &[BlsPubkey]) -> Self {
        let mut acc = [0u8; 48];
        for key in keys {
            for (a, b) in acc.iter_mut().zip(key.0.iter()) {
                *a ^= b;
            }
        }
        // blake3 ile karıştır
        let hash = blake3::hash(&acc);
        let mut result = [0u8; 48];
        result[..32].copy_from_slice(hash.as_bytes());
        result[32..].copy_from_slice(&acc[32..]);
        result[32] |= 0x80; // compressed flag
        Self(result)
    }

    /// Gerçek BLS keypair üret — deterministik seed'den.
    /// Döndürür: (gizli anahtar, genel anahtar)
    pub fn gen_real(seed: &[u8; 32]) -> (BlsSecretKey, BlsPubkey) {
        let sk = BlstSk::key_gen(seed, &[]).expect("geçerli 32-byte IKM");
        let pk_bytes = sk.sk_to_pk().to_bytes();
        let mut arr = [0u8; 48];
        arr.copy_from_slice(&pk_bytes);
        (BlsSecretKey(sk), BlsPubkey(arr))
    }
}

/// BLS12-381 gizli anahtar (test ve validator signing için)
pub struct BlsSecretKey(BlstSk);

// ─── BLS Signature ────────────────────────────────────────────────────────────

/// BLS12-381 G2 aggregate signature (96 byte)
/// Üretimde: blst::Signature — burada mock (blake3 tabanlı)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlsSignature(pub [u8; 96]);

impl BlsSignature {
    /// Mock imza: blake3(message || signers_aggregate)
    pub fn mock_sign(message: &[u8], signers: &[BlsPubkey]) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(message);
        for key in signers {
            hasher.update(&key.0);
        }
        let hash = hasher.finalize();
        let mut sig = [0u8; 96];
        sig[..32].copy_from_slice(hash.as_bytes());
        // İkinci 32 byte: imzacı sayısı
        sig[32..40].copy_from_slice(&(signers.len() as u64).to_le_bytes());
        Self(sig)
    }

    /// Mock doğrulama
    pub fn mock_verify(&self, message: &[u8], signers: &[BlsPubkey]) -> bool {
        let expected = Self::mock_sign(message, signers);
        self.0 == expected.0
    }

    /// Gerçek BLS12-381 imza üret (tek imzacı).
    pub fn sign_real(message: &[u8], sk: &BlsSecretKey) -> Self {
        let sig = sk.0.sign(message, BLS_DST, &[]);
        let bytes = sig.to_bytes();
        Self(bytes)
    }

    /// Gerçek BLS12-381 aggregate imza üret: n imzacı aynı mesajı imzalar.
    ///
    /// Her imzacı mesajı ayrı ayrı imzalar, ardından imzalar aggregate edilir.
    /// Sonuç: tek bir 96-byte G2 noktası.
    pub fn aggregate_sign_real(message: &[u8], secret_keys: &[&BlsSecretKey]) -> Self {
        if secret_keys.is_empty() {
            return Self([0u8; 96]);
        }
        let sigs: Vec<BlstSig> = secret_keys
            .iter()
            .map(|sk| sk.0.sign(message, BLS_DST, &[]))
            .collect();
        let sigs_refs: Vec<&BlstSig> = sigs.iter().collect();
        let agg = BlstAggSig::aggregate(&sigs_refs, false)
            .expect("geçerli imzalar aggregate edilebilir");
        let bytes = agg.to_signature().to_bytes();
        Self(bytes)
    }

    /// Gerçek BLS12-381 aggregate imzayı doğrula.
    ///
    /// `fast_aggregate_verify`: tüm imzacıların aggregate pubkey'i ile
    /// imzanın aynı mesajı imzaladığını doğrular.
    pub fn verify_real(&self, message: &[u8], pubkeys: &[BlsPubkey]) -> bool {
        if pubkeys.is_empty() {
            return false;
        }

        // Signature deserialize
        let sig = match BlstSig::from_bytes(&self.0) {
            Ok(s) => s,
            Err(_) => return false,
        };

        // Public key'leri deserialize et
        let pks: Vec<BlstPk> = pubkeys
            .iter()
            .filter_map(|pk| BlstPk::from_bytes(&pk.0).ok())
            .collect();

        if pks.is_empty() {
            return false;
        }

        let pks_refs: Vec<&BlstPk> = pks.iter().collect();

        // fast_aggregate_verify: n pubkey'in aggregate'ini kullanır
        let result = sig.fast_aggregate_verify(true, message, BLS_DST, &pks_refs);
        result == BLST_ERROR::BLST_SUCCESS
    }
}

// ─── SyncCommittee ────────────────────────────────────────────────────────────

/// Sync Committee — Ethereum Beacon Chain §light-client-sync
///
/// Her SLOTS_PER_SYNC_COMMITTEE_PERIOD slot'ta bir yeni committee seçilir.
/// Validators: BLS pubkey listesi (SYNC_COMMITTEE_SIZE=512)
/// Aggregate pubkey: tüm validator pubkey'lerinin BLS toplamı
#[derive(Debug, Clone)]
pub struct SyncCommittee {
    /// Validator BLS public key'leri (512 adet)
    pub pubkeys: Vec<BlsPubkey>,
    /// Aggregate pubkey: tüm pubkey'lerin BLS toplamı
    pub aggregate_pubkey: BlsPubkey,
    /// Bu committee'nin geçerli olduğu period
    pub period: u64,
}

impl SyncCommittee {
    pub fn new(pubkeys: Vec<BlsPubkey>, period: u64) -> Self {
        let aggregate_pubkey = BlsPubkey::aggregate(&pubkeys);
        Self { pubkeys, aggregate_pubkey, period }
    }

    /// Validator sayısı
    pub fn size(&self) -> usize {
        self.pubkeys.len()
    }

    /// Quorum karşılandı mı? (2/3 imzacı)
    pub fn has_quorum(&self, participant_count: usize) -> bool {
        // Küçük test committee'leri için oransal kontrol
        if self.pubkeys.len() < SYNC_COMMITTEE_SIZE {
            let threshold = (self.pubkeys.len() * 2 + 2) / 3;
            participant_count >= threshold
        } else {
            participant_count >= MIN_SYNC_COMMITTEE_PARTICIPANTS
        }
    }
}

// ─── LightClientHeader ────────────────────────────────────────────────────────

/// Light client blok başlığı — Ethereum Beacon Chain pattern
///
/// Ethereum: `BeaconBlockHeader { slot, proposer_index, parent_root, state_root, body_root }`
/// Nova: aynı yapı, slot = Nova slot numarası
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightClientHeader {
    /// Blok slotu
    pub slot: u64,
    /// Öneren validator indeksi
    pub proposer_index: u64,
    /// Önceki bloğun kökü
    pub parent_root: [u8; 32],
    /// Durum ağacı kökü
    pub state_root: [u8; 32],
    /// Block body kökü
    pub body_root: [u8; 32],
}

impl LightClientHeader {
    pub fn signing_root(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.slot.to_le_bytes());
        hasher.update(&self.proposer_index.to_le_bytes());
        hasher.update(&self.parent_root);
        hasher.update(&self.state_root);
        hasher.update(&self.body_root);
        *hasher.finalize().as_bytes()
    }

    /// Sync committee period hesapla
    pub fn sync_committee_period(&self) -> u64 {
        self.slot / SLOTS_PER_SYNC_COMMITTEE_PERIOD
    }
}

// ─── SyncAggregate ────────────────────────────────────────────────────────────

/// Sync committee aggregate — hangi validatorlar imzaladı + aggregate imza
///
/// Ethereum: `SyncAggregate { sync_committee_bits, sync_committee_signature }`
/// - sync_committee_bits: 512-bit bitmask (hangi validatorlar imzaladı)
/// - sync_committee_signature: BLS aggregate G2 imzası
#[derive(Debug, Clone)]
pub struct SyncAggregate {
    /// Katılımcı bitmask (her bit = 1 validator, 512/8 = 64 byte)
    pub sync_committee_bits: Vec<u8>,
    /// BLS aggregate imzası
    pub sync_committee_signature: BlsSignature,
    /// Katılımcı sayısı (bitmask'tan hesaplanır)
    pub participant_count: usize,
}

impl SyncAggregate {
    pub fn new(
        participant_indices: &[usize],
        committee_size: usize,
        signature: BlsSignature,
    ) -> Self {
        let byte_count = (committee_size + 7) / 8;
        let mut bits = vec![0u8; byte_count];
        for &idx in participant_indices {
            if idx < committee_size {
                bits[idx / 8] |= 1 << (idx % 8);
            }
        }
        Self {
            sync_committee_bits: bits,
            sync_committee_signature: signature,
            participant_count: participant_indices.len(),
        }
    }

    /// Katılımcı pubkey'lerini çıkar
    pub fn participating_keys<'a>(&self, committee: &'a SyncCommittee) -> Vec<&'a BlsPubkey> {
        let mut keys = Vec::new();
        for (byte_idx, &byte) in self.sync_committee_bits.iter().enumerate() {
            for bit in 0..8 {
                if byte & (1 << bit) != 0 {
                    let validator_idx = byte_idx * 8 + bit;
                    if let Some(key) = committee.pubkeys.get(validator_idx) {
                        keys.push(key);
                    }
                }
            }
        }
        keys
    }
}

// ─── LightClientUpdate ────────────────────────────────────────────────────────

/// Light client güncelleme mesajı — sync committee geçişi
///
/// Ethereum: `LightClientUpdate { attested_header, next_sync_committee,
///            next_sync_committee_branch, finalized_header, sync_aggregate, ... }`
///
/// Nova: basitleştirilmiş versiyon — finality + committee geçişi
#[derive(Debug, Clone)]
pub struct LightClientUpdate {
    /// İmzalanan (attested) header
    pub attested_header: LightClientHeader,
    /// Kesinleşen header (finality)
    pub finalized_header: Option<LightClientHeader>,
    /// Bir sonraki sync committee (committee geçişi için)
    pub next_sync_committee: Option<SyncCommittee>,
    /// Sync aggregate imzası
    pub sync_aggregate: SyncAggregate,
    /// Signature slotu
    pub signature_slot: u64,
}

// ─── LightClientStore ─────────────────────────────────────────────────────────

/// Light client state — hangi header ve committee'ye güvenildiği
#[derive(Debug, Clone)]
pub struct LightClientStore {
    /// Şu anki güvenilir header
    pub current_header: LightClientHeader,
    /// Şu anki sync committee
    pub current_committee: SyncCommittee,
    /// Bir sonraki sync committee (önceden biliniyorsa)
    pub next_committee: Option<SyncCommittee>,
    /// En son finalized header
    pub finalized_header: LightClientHeader,
}

impl LightClientStore {
    pub fn new(genesis_header: LightClientHeader, genesis_committee: SyncCommittee) -> Self {
        let finalized_header = genesis_header.clone();
        Self {
            current_header: genesis_header,
            current_committee: genesis_committee,
            next_committee: None,
            finalized_header,
        }
    }

    /// Şu anki period
    pub fn current_period(&self) -> u64 {
        self.current_header.sync_committee_period()
    }
}

// ─── BLS12381LightClient ──────────────────────────────────────────────────────

/// BLS12-381 Light Client doğrulayıcı
///
/// BEEFY pattern: her güncelleme için BLS aggregate imza doğrulaması.
/// Gerçek implementasyonda `blst` kütüphanesi kullanılır.
/// Burada mock (BLAKE3 tabanlı) doğrulama.
#[derive(Debug)]
pub struct BLS12381LightClient {
    store: LightClientStore,
    /// Uygulanan güncelleme sayısı
    updates_applied: u64,
}

impl BLS12381LightClient {
    pub fn new(store: LightClientStore) -> Self {
        Self { store, updates_applied: 0 }
    }

    /// Güncellemeyi doğrula ve uygula
    ///
    /// Kontroller (Ethereum light client spec'ten):
    /// 1. Sync aggregate katılımı quorum eşiğini geçiyor mu?
    /// 2. İmza slotu attested header slot'undan sonra mı?
    /// 3. BLS aggregate imzası geçerli mi?
    /// 4. Committee period uyumlu mu?
    pub fn apply_update(&mut self, update: LightClientUpdate) -> NetworkResult<LightClientUpdateResult> {
        // 1. Katılım quorum kontrolü
        let committee = self.active_committee(&update);
        if !committee.has_quorum(update.sync_aggregate.participant_count) {
            return Err(NetworkError::HandshakeFailed {
                reason: format!(
                    "yetersiz katılım: {}/{} (quorum: {})",
                    update.sync_aggregate.participant_count,
                    committee.size(),
                    (committee.size() * 2 + 2) / 3
                ),
            });
        }

        // 2. İmza slotu kontrolü
        if update.signature_slot <= update.attested_header.slot {
            return Err(NetworkError::HandshakeFailed {
                reason: "signature_slot attested_header.slot'tan önce olamaz".into(),
            });
        }

        // 3. BLS aggregate imza doğrulama (gerçek BLS12-381)
        let signing_root = update.attested_header.signing_root();
        let participant_keys: Vec<BlsPubkey> = update
            .sync_aggregate
            .participating_keys(committee)
            .into_iter()
            .cloned()
            .collect();

        let sig_valid = update.sync_aggregate.sync_committee_signature.verify_real(
            &signing_root,
            &participant_keys,
        );

        if !sig_valid {
            return Err(NetworkError::HandshakeFailed {
                reason: "BLS aggregate imzası geçersiz".into(),
            });
        }

        // 4. Period kontrolü
        let update_period = update.attested_header.sync_committee_period();
        let store_period = self.store.current_period();
        if update_period > store_period + 1 {
            return Err(NetworkError::HandshakeFailed {
                reason: format!(
                    "period atlandı: store={} update={}",
                    store_period, update_period
                ),
            });
        }

        let mut result = LightClientUpdateResult {
            new_finalized_slot: None,
            committee_updated: false,
            participants: update.sync_aggregate.participant_count,
        };

        // Finality güncelle
        if let Some(ref finalized) = update.finalized_header {
            if finalized.slot > self.store.finalized_header.slot {
                self.store.finalized_header = finalized.clone();
                result.new_finalized_slot = Some(finalized.slot);
            }
        }

        // Current header güncelle
        if update.attested_header.slot > self.store.current_header.slot {
            self.store.current_header = update.attested_header;
        }

        // Committee geçişi
        if let Some(next_committee) = update.next_sync_committee {
            if update_period == store_period {
                // Şimdiki period'un sonraki committee'si
                self.store.next_committee = Some(next_committee);
                result.committee_updated = true;
            } else if update_period == store_period + 1 {
                // Yeni period'a geçiş
                if let Some(next) = self.store.next_committee.take() {
                    self.store.current_committee = next;
                }
                self.store.next_committee = Some(next_committee);
                result.committee_updated = true;
            }
        }

        self.updates_applied += 1;
        Ok(result)
    }

    /// Şu anki period için aktif committee
    fn active_committee<'a>(&'a self, update: &LightClientUpdate) -> &'a SyncCommittee {
        let update_period = update.attested_header.sync_committee_period();
        let store_period = self.store.current_period();
        if update_period == store_period + 1 {
            self.store.next_committee.as_ref().unwrap_or(&self.store.current_committee)
        } else {
            &self.store.current_committee
        }
    }

    pub fn finalized_header(&self) -> &LightClientHeader {
        &self.store.finalized_header
    }

    pub fn current_header(&self) -> &LightClientHeader {
        &self.store.current_header
    }

    pub fn updates_applied(&self) -> u64 {
        self.updates_applied
    }

    pub fn current_period(&self) -> u64 {
        self.store.current_period()
    }
}

/// Güncelleme sonucu
#[derive(Debug, Clone)]
pub struct LightClientUpdateResult {
    /// Yeni finalized slot (varsa)
    pub new_finalized_slot: Option<u64>,
    /// Sync committee güncellendi mi?
    pub committee_updated: bool,
    /// Katılımcı sayısı
    pub participants: usize,
}

// ─── Yardımcı: SyncCommittee oluşturma ───────────────────────────────────────

/// Gerçek BLS anahtarlarıyla committee oluştur.
///
/// Her validator için deterministik seed: `seed_prefix || id.to_le_bytes()`.
/// Döndürür: (committee, gizli anahtarlar)
pub fn make_real_sync_committee(
    size: usize,
    period: u64,
    seed_prefix: u8,
) -> (SyncCommittee, Vec<BlsSecretKey>) {
    let mut pubkeys = Vec::with_capacity(size);
    let mut secret_keys = Vec::with_capacity(size);

    for i in 0..size {
        let mut seed = [0u8; 32];
        seed[0] = seed_prefix;
        seed[1..9].copy_from_slice(&(i as u64).to_le_bytes());
        seed[9] = 0xBF; // disambiguator
        let (sk, pk) = BlsPubkey::gen_real(&seed);
        pubkeys.push(pk);
        secret_keys.push(sk);
    }

    (SyncCommittee::new(pubkeys, period), secret_keys)
}

/// Test/devnet için committee oluştur (mock anahtarlar — sadece quorum/period testleri için)
pub fn make_sync_committee(validator_ids: &[u64], period: u64) -> SyncCommittee {
    let pubkeys: Vec<BlsPubkey> = validator_ids
        .iter()
        .map(|&id| BlsPubkey::from_validator_id(id))
        .collect();
    SyncCommittee::new(pubkeys, period)
}

/// Geçerli update oluştur — gerçek BLS anahtarlarıyla imzalar.
pub fn make_valid_update(
    committee: &SyncCommittee,
    secret_keys: &[BlsSecretKey],
    attested_slot: u64,
    finalized_slot: Option<u64>,
    next_committee: Option<SyncCommittee>,
) -> LightClientUpdate {
    let attested_header = LightClientHeader {
        slot: attested_slot,
        proposer_index: 1,
        parent_root: [0u8; 32],
        state_root: blake3::hash(&attested_slot.to_le_bytes()).into(),
        body_root: [0u8; 32],
    };

    let signing_root = attested_header.signing_root();
    let participant_indices: Vec<usize> = (0..committee.size()).collect();

    // Gerçek BLS aggregate imza: tüm committee üyeleri imzalar
    let sk_refs: Vec<&BlsSecretKey> = secret_keys.iter().collect();
    let signature = BlsSignature::aggregate_sign_real(&signing_root, &sk_refs);

    let sync_aggregate = SyncAggregate::new(&participant_indices, committee.size(), signature);

    let finalized_header = finalized_slot.map(|slot| LightClientHeader {
        slot,
        proposer_index: 1,
        parent_root: [0u8; 32],
        state_root: blake3::hash(&slot.to_le_bytes()).into(),
        body_root: [0u8; 32],
    });

    LightClientUpdate {
        attested_header,
        finalized_header,
        next_sync_committee: next_committee,
        sync_aggregate,
        signature_slot: attested_slot + 1,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Gerçek BLS anahtarlı committee + client üret.
    /// seed_prefix: period çakışmasını önlemek için her period farklı seed.
    fn make_real_committee_for_test(size: usize, period: u64) -> (SyncCommittee, Vec<BlsSecretKey>) {
        make_real_sync_committee(size, period, period as u8)
    }

    fn make_real_client(committee_size: usize) -> (BLS12381LightClient, Vec<BlsSecretKey>) {
        let genesis_header = LightClientHeader {
            slot: 0,
            proposer_index: 0,
            parent_root: [0u8; 32],
            state_root: [0u8; 32],
            body_root: [0u8; 32],
        };
        let (committee, sks) = make_real_committee_for_test(committee_size, 0);
        let store = LightClientStore::new(genesis_header, committee);
        (BLS12381LightClient::new(store), sks)
    }

    // 1. Geçerli güncelleme kabul edilir (gerçek BLS)
    #[test]
    fn valid_update_accepted() {
        let (mut client, sks) = make_real_client(10);
        let (committee, _) = make_real_committee_for_test(10, 0);
        let update = make_valid_update(&committee, &sks, 100, Some(80), None);
        let result = client.apply_update(update).unwrap();
        assert_eq!(result.new_finalized_slot, Some(80));
        assert_eq!(client.current_header().slot, 100);
        assert_eq!(client.finalized_header().slot, 80);
        assert_eq!(client.updates_applied(), 1);
    }

    // 2. Quorum yetersiz → hata (quorum kontrolü BLS'den önce)
    #[test]
    fn insufficient_quorum_rejected() {
        let (mut client, sks) = make_real_client(10);
        let (committee, _) = make_real_committee_for_test(10, 0);
        let mut update = make_valid_update(&committee, &sks, 100, None, None);

        // Sadece 1 katılımcı (quorum için 7 gerekli = ceil(2*10/3))
        // Quorum kontrolü BLS doğrulamadan önce → herhangi bir imza olabilir
        let dummy_sig = BlsSignature([0u8; 96]);
        update.sync_aggregate = SyncAggregate::new(&[0], 10, dummy_sig);

        let result = client.apply_update(update);
        assert!(result.is_err(), "yetersiz quorum kabul edilmemeli");
    }

    // 3. Geçersiz imza → hata (gerçek BLS reddeder)
    #[test]
    fn invalid_signature_rejected() {
        let (mut client, sks) = make_real_client(5);
        let (committee, _) = make_real_committee_for_test(5, 0);
        let mut update = make_valid_update(&committee, &sks, 50, None, None);

        // Sıfır imza: gerçek BLS doğrulamadan geçemez
        let wrong_sig = BlsSignature([0u8; 96]);
        let participant_indices: Vec<usize> = (0..5).collect();
        update.sync_aggregate = SyncAggregate::new(&participant_indices, 5, wrong_sig);

        let result = client.apply_update(update);
        assert!(result.is_err(), "geçersiz imza kabul edilmemeli");
    }

    // 4. Signature slot önceki attested slot → hata (slot kontrolü BLS'den önce)
    #[test]
    fn signature_slot_before_attested_rejected() {
        let (mut client, sks) = make_real_client(5);
        let (committee, _) = make_real_committee_for_test(5, 0);
        let mut update = make_valid_update(&committee, &sks, 100, None, None);
        update.signature_slot = 99; // attested=100, signature=99 → hata

        let result = client.apply_update(update);
        assert!(result.is_err(), "geçersiz slot sırası kabul edilmemeli");
    }

    // 5. Finality olmadan güncelleme (gerçek BLS)
    #[test]
    fn update_without_finality() {
        let (mut client, sks) = make_real_client(6);
        let (committee, _) = make_real_committee_for_test(6, 0);
        let update = make_valid_update(&committee, &sks, 200, None, None);
        let result = client.apply_update(update).unwrap();
        assert_eq!(result.new_finalized_slot, None);
        assert_eq!(client.current_header().slot, 200);
        assert_eq!(client.finalized_header().slot, 0); // hâlâ genesis
    }

    // 6. Sync committee geçişi (gerçek BLS)
    #[test]
    fn sync_committee_transition() {
        let (mut client, sks) = make_real_client(6);
        let (current_committee, _) = make_real_committee_for_test(6, 0);
        let (next_committee, _) = make_real_committee_for_test(6, 1);

        // Period 0 güncelleme — next committee'yi bildir
        let update = make_valid_update(
            &current_committee,
            &sks,
            SLOTS_PER_SYNC_COMMITTEE_PERIOD - 1,
            None,
            Some(next_committee),
        );
        let result = client.apply_update(update).unwrap();
        assert!(result.committee_updated, "committee güncellemesi bekleniyor");
        assert!(client.store.next_committee.is_some());
    }

    // 7. Period atlama → hata (BLS geçer, period kontrolü reddeder)
    #[test]
    fn period_skip_rejected() {
        let (mut client, sks) = make_real_client(6);
        let (next_committee, _) = make_real_committee_for_test(6, 2);

        // Period 0'dan period 2'ye atlama (SLOTS_PER_SYNC_COMMITTEE_PERIOD * 2 slot)
        let attested_slot = SLOTS_PER_SYNC_COMMITTEE_PERIOD * 2;
        let (current_committee, _) = make_real_committee_for_test(6, 0);
        let update = make_valid_update(&current_committee, &sks, attested_slot, None, Some(next_committee));

        let result = client.apply_update(update);
        assert!(result.is_err(), "period atlaması kabul edilmemeli");
    }

    // 8. Ardışık çok sayıda güncelleme (gerçek BLS)
    #[test]
    fn multiple_sequential_updates() {
        let (mut client, sks) = make_real_client(9);
        let (committee, _) = make_real_committee_for_test(9, 0);

        for i in 1u64..=10 {
            let update = make_valid_update(&committee, &sks, i * 100, Some(i * 100 - 20), None);
            client.apply_update(update).unwrap();
        }

        assert_eq!(client.updates_applied(), 10);
        assert_eq!(client.current_header().slot, 1000);
        assert_eq!(client.finalized_header().slot, 980);
    }

    // 9. SyncCommittee quorum hesabı doğru mu? (mock commit — quorum algoritmasını test eder)
    #[test]
    fn quorum_threshold_calculation() {
        let ids: Vec<u64> = (1..=3u64).collect();
        let c3 = make_sync_committee(&ids, 0);
        assert!(c3.has_quorum(2));
        assert!(!c3.has_quorum(1));

        let ids9: Vec<u64> = (1..=9u64).collect();
        let c9 = make_sync_committee(&ids9, 0);
        assert!(c9.has_quorum(6)); // ceil(2*9/3) = 6
        assert!(!c9.has_quorum(5));
    }

    // 10. Mock BLS aggregate imza round-trip (mock test)
    #[test]
    fn bls_mock_signature_round_trip() {
        let ids: Vec<u64> = (1..=5u64).collect();
        let committee = make_sync_committee(&ids, 0);
        let message = b"nova_shield_light_client_test";
        let sig = BlsSignature::mock_sign(message, &committee.pubkeys);
        assert!(sig.mock_verify(message, &committee.pubkeys));
        assert!(!sig.mock_verify(b"different_message", &committee.pubkeys));
    }

    // 10b. Gerçek BLS12-381 aggregate imza round-trip
    #[test]
    fn bls_real_signature_round_trip() {
        let (committee, sks) = make_real_committee_for_test(5, 0);
        let message = b"nova_shield_real_bls_test";
        let sk_refs: Vec<&BlsSecretKey> = sks.iter().collect();
        let sig = BlsSignature::aggregate_sign_real(message, &sk_refs);
        assert!(sig.verify_real(message, &committee.pubkeys));
        assert!(!sig.verify_real(b"different_message", &committee.pubkeys));
    }

    // 11. SyncAggregate katılımcı çıkarma (mock anahtarlar yeterli)
    #[test]
    fn sync_aggregate_participant_extraction() {
        let ids: Vec<u64> = (1..=8u64).collect();
        let committee = make_sync_committee(&ids, 0);
        let participants = [0, 2, 4, 6];
        let sig = BlsSignature::mock_sign(b"test", &committee.pubkeys);
        let agg = SyncAggregate::new(&participants, 8, sig);

        assert_eq!(agg.participant_count, 4);
        let keys = agg.participating_keys(&committee);
        assert_eq!(keys.len(), 4);
        // İndeks 0, 2, 4, 6 → pubkey'ler doğru
        assert_eq!(*keys[0], committee.pubkeys[0]);
        assert_eq!(*keys[1], committee.pubkeys[2]);
    }

    // 12. LightClientHeader signing_root deterministik
    #[test]
    fn signing_root_deterministic() {
        let header = LightClientHeader {
            slot: 12345,
            proposer_index: 7,
            parent_root: [0xABu8; 32],
            state_root: [0xCDu8; 32],
            body_root: [0xEFu8; 32],
        };
        let r1 = header.signing_root();
        let r2 = header.signing_root();
        assert_eq!(r1, r2);
    }
}
