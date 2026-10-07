//! Penumbra Tarzı Hiyerarşik Anahtar Sistemi
//!
//! §5.7.1 — ZK-Shield gizlilik koruması için hiyerarşik görüntüleme anahtarları.
//!
//! ## Araştırma Kaynağı: Penumbra Protocol
//!
//! Penumbra (`research/libs/penumbra/crates/core/keys/src/`):
//! ```text
//! SpendKey             <- Ana gizli anahtar (32 byte)
//!   +-- FullViewingKey <- Tüm TX'leri görür, harcama yapamaz
//!         +-- IncomingViewingKey <- Sadece gelen TX'leri görür
//!         +-- OutgoingViewingKey <- Sadece giden TX'leri görür
//!         +-- Address            <- Çeşitleyici ile çoklu adres üretimi
//! ```
//!
//! Penumbra implementasyonu (`crates/core/keys/src/keys/spend.rs`):
//! - `SpendKey { ask: SpendAuthKey, fvk: FullViewingKey }`
//! - `FullViewingKey { ak: VerificationKey, nk: NullifierKey, ovk: OutgoingViewingKey }`
//! - `IncomingViewingKey = prf_nk(nk, ak)` — Poseidon PRF
//! - `Address = diversifier_generator * ivk_scalar + transmission_key`
//!
//! Nova uyarlaması: Poseidon yerine BLAKE3 PRF kullanılır (nova-native hash).
//! Konsept aynı: Her SpendKey → sonsuz farklı adres üretebilir (diversifier).
//!
//! ## Nova Kullanım Senaryoları
//! - ZK-Shield şifreli bakiyeler için gizli adresler
//! - Miras planında heir'in sadece gelen TX'leri görmesi (IVK paylaşımı)
//! - Guardian'lara OutgoingViewingKey verilerek harcamaların izlenmesi
//!
//! ## Güvenlik Notları
//! - SpendKey asla ağa iletilmez, cihazda şifreli saklanır
//! - FVK paylaşılabilir (tam şeffaflık, harcama yetkisi yok)
//! - IVK paylaşılabilir (gelen işlem şeffaflığı)
//! - OVK paylaşılabilir (giden işlem şeffaflığı)

use blake3::Hasher;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::errors::{CryptoError, CryptoResult};

// ─── ALAN DOMAİN AYIRICILAR ─────────────────────────────────────────────────
//
// Penumbra'da her key türetme işlemi farklı domain separator kullanır.
// Referans: penumbra/crates/core/keys/src/keys/diversifier.rs

const DOMAIN_SPEND_AUTH: &[u8] = b"nova-shield.keys.spend_auth_v1";
const DOMAIN_NULLIFIER_KEY: &[u8] = b"nova-shield.keys.nullifier_key_v1";
const DOMAIN_OUTGOING_VK: &[u8] = b"nova-shield.keys.outgoing_viewing_v1";
const DOMAIN_INCOMING_VK: &[u8] = b"nova-shield.keys.incoming_viewing_v1";
const DOMAIN_DIVERSIFIER: &[u8] = b"nova-shield.keys.diversifier_v1";
const DOMAIN_ADDRESS: &[u8] = b"nova-shield.keys.address_v1";

// ─── SPEND AUTH KEY ──────────────────────────────────────────────────────────

/// Harcama yetki anahtarı — Penumbra `SpendAuthKey` analog.
///
/// Penumbra referans: `crates/core/keys/src/keys/spend.rs`
/// `SpendAuthKey` → schnorrkel imzalama anahtarı (Nova'da BLAKE3 tabanlı).
///
/// Bu anahtar cihazda şifreli tutulur; ağa hiç iletilmez.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SpendAuthKey([u8; 32]);

impl SpendAuthKey {
    /// Rastgele yeni SpendAuthKey.
    pub fn generate() -> CryptoResult<Self> {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Ok(Self(bytes))
    }

    /// Ham byte'lardan oluştur (deterministik test için).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Byte referansı.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ─── NULLIFIER KEY ──────────────────────────────────────────────────────────

/// Nullifier anahtarı — Penumbra `NullifierKey` analog.
///
/// ZK-UTXO'ların "harcanmış" flagını oluşturur.
/// Nullifier = PRF(nk, note_commitment) → zincire açıklanan değer.
/// Gerçek note content açıklanmadan "bu note harcandı" kanıtlanır.
///
/// Penumbra referans: `crates/core/keys/src/keys/nullifier.rs`
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct NullifierKey([u8; 32]);

impl NullifierKey {
    /// Ham byte dizisinden oluşturur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Ham byte dizisine referans döner.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Note commitment'ı nullify et: PRF_nk(note_commitment).
    ///
    /// Penumbra: `nk.derive_nullifier(note, position)` — Poseidon PRF
    /// Nova: BLAKE3 PRF (daha basit, aynı konsept)
    pub fn derive_nullifier(&self, note_commitment: &[u8]) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(DOMAIN_NULLIFIER_KEY);
        hasher.update(&self.0);
        hasher.update(note_commitment);
        *hasher.finalize().as_bytes()
    }
}

// ─── OUTGOING VIEWING KEY ─────────────────────────────────────────────────

/// Giden işlem görüntüleme anahtarı — Penumbra `OutgoingViewingKey` analog.
///
/// Bu anahtarla hesabın gönderdiği tüm TX'ler okunabilir.
/// Kullanım: Vergi uyumu, muhasebe, guardian denetimi.
///
/// Penumbra referans: `crates/core/keys/src/keys/outgoing.rs`
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct OutgoingViewingKey([u8; 32]);

impl OutgoingViewingKey {
    /// Ham byte dizisinden oluşturur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Ham byte dizisine referans döner.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Giden işlem için şifreleme anahtarı türet.
    /// Penumbra: `ovk.derive_memo_key()` — güvenli memo şifrelemesi.
    pub fn derive_tx_key(&self, tx_id: &[u8]) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(DOMAIN_OUTGOING_VK);
        hasher.update(&self.0);
        hasher.update(tx_id);
        *hasher.finalize().as_bytes()
    }
}

// ─── FULL VIEWING KEY ────────────────────────────────────────────────────────

/// Tam görüntüleme anahtarı — Penumbra `FullViewingKey` analog.
///
/// Penumbra'da `FullViewingKey { ak: VerificationKey, nk: NullifierKey, ovk: OutgoingViewingKey }`.
/// Bu anahtarla hesabın tüm TX geçmişi okunabilir (gelen + giden).
/// Harcama yapılamaz (SpendAuthKey olmadan).
///
/// Penumbra referans: `crates/core/keys/src/keys/spend.rs:FullViewingKey`
///
/// ```text
/// // Penumbra orijinal:
/// pub struct FullViewingKey {
///     pub ak: VerificationKey<SpendAuth>,
///     pub nk: NullifierKey,
///     pub ovk: OutgoingViewingKey,
/// }
///
/// // Nova uyarlaması:
/// pub struct FullViewingKey {
///     pub ak: [u8; 32],  // Spend auth dogrulama key
///     pub nk: NullifierKey,
///     pub ovk: OutgoingViewingKey,
/// }
/// ```
pub struct FullViewingKey {
    /// Harcama yetki doğrulama anahtarı (public).
    pub ak: [u8; 32],
    /// Nullifier anahtarı.
    pub nk: NullifierKey,
    /// Giden TX görüntüleme anahtarı.
    pub ovk: OutgoingViewingKey,
}

impl FullViewingKey {
    /// SpendAuthKey'den FullViewingKey türet.
    ///
    /// Penumbra'da BLAKE2b tabanlı derivation kullanılır.
    /// Nova'da BLAKE3 ile:
    /// - ak = BLAKE3(domain_ak || spend_auth_bytes)
    /// - nk = BLAKE3(domain_nk || spend_auth_bytes)
    /// - ovk = BLAKE3(domain_ovk || spend_auth_bytes)
    pub fn derive_from(spend_auth_key: &SpendAuthKey) -> Self {
        let sak = spend_auth_key.as_bytes();

        // ak: harcama yetki doğrulama anahtarı
        let mut h = Hasher::new();
        h.update(DOMAIN_SPEND_AUTH);
        h.update(sak);
        let ak: [u8; 32] = *h.finalize().as_bytes();

        // nk: nullifier anahtarı
        let mut h = Hasher::new();
        h.update(DOMAIN_NULLIFIER_KEY);
        h.update(sak);
        let nk_bytes: [u8; 32] = *h.finalize().as_bytes();

        // ovk: giden TX görüntüleme anahtarı
        let mut h = Hasher::new();
        h.update(DOMAIN_OUTGOING_VK);
        h.update(sak);
        let ovk_bytes: [u8; 32] = *h.finalize().as_bytes();

        Self {
            ak,
            nk: NullifierKey::from_bytes(nk_bytes),
            ovk: OutgoingViewingKey::from_bytes(ovk_bytes),
        }
    }

    /// IncomingViewingKey türet.
    ///
    /// Penumbra: `ivk = prf_nk(nk, ak)` — Poseidon PRF
    /// Nova: ivk = BLAKE3(domain_ivk || nk || ak)
    pub fn incoming_viewing_key(&self) -> IncomingViewingKey {
        let mut h = Hasher::new();
        h.update(DOMAIN_INCOMING_VK);
        h.update(self.nk.as_bytes());
        h.update(&self.ak);
        let ivk_bytes: [u8; 32] = *h.finalize().as_bytes();
        IncomingViewingKey::from_bytes(ivk_bytes)
    }

    /// OutgoingViewingKey referansı.
    pub fn outgoing_viewing_key(&self) -> &OutgoingViewingKey {
        &self.ovk
    }

    /// NullifierKey referansı.
    pub fn nullifier_key(&self) -> &NullifierKey {
        &self.nk
    }
}

// ─── INCOMING VIEWING KEY ────────────────────────────────────────────────────

/// Gelen işlem görüntüleme anahtarı — Penumbra `IncomingViewingKey` analog.
///
/// Kullanım:
/// - Hesaba gelen tüm transferleri çözümle
/// - Miras planında heir'e bu anahtar verilirse varlıkları talep edebilir
/// - Guardian'lara bu anahtar verilirse sadece gelen işlemleri görebilir
///
/// Penumbra referans: `crates/core/keys/src/keys/incoming.rs`
///
/// ```text
/// // Penumbra orijinal:
/// pub struct IncomingViewingKey { ivk: Scalar }
/// // Scalar: ed25519 skaleri, adres üretiminde kullanilir
///
/// // Nova uyarlamasi: 32-byte key, BLAKE3 PRF ile adres üretimi
/// ```
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct IncomingViewingKey([u8; 32]);

impl IncomingViewingKey {
    /// Ham byte dizisinden oluşturur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Ham byte dizisine referans döner.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Çeşitleyici indeksinden Nova Shield adresi türet.
    ///
    /// Penumbra'da: `Address = diversified_generator(d) + transmission_key(ivk, d)`
    /// Bu yaklaşım tek IVK'dan sonsuz farklı adres üretir (gizlilik için).
    ///
    /// Nova uyarlaması:
    /// `address_bytes = BLAKE3(domain || ivk || diversifier_index)[0..20]`
    ///
    /// Kullanım senaryosu:
    /// - `diversifier=0` → birincil adres
    /// - `diversifier=1` → ikincil adres (farklı borsa için)
    /// - `diversifier=2` → miras planında heir için özel adres
    pub fn derive_address(&self, diversifier_index: u64) -> [u8; 20] {
        let mut h = Hasher::new();
        h.update(DOMAIN_ADDRESS);
        h.update(&self.0);
        h.update(DOMAIN_DIVERSIFIER);
        h.update(&diversifier_index.to_le_bytes());
        let hash = h.finalize();
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash.as_bytes()[0..20]);
        addr
    }

    /// Gelen note şifresini çöz.
    /// Penumbra: `ivk.decrypt_incoming(ciphertext)` — Chacha20Poly1305
    pub fn derive_note_key(&self, sender_ephemeral: &[u8]) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(DOMAIN_INCOMING_VK);
        h.update(&self.0);
        h.update(sender_ephemeral);
        *h.finalize().as_bytes()
    }
}

// ─── ADDRESS DIVERSIFIER ─────────────────────────────────────────────────────

/// Çeşitleyici adres: diversifier index + adres byte'ları
///
/// Penumbra: `Address { diversifier: Diversifier, transmission_key: ka::Public, clue_key }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiversifiedAddress {
    /// Hangi diversifier ile üretildi
    pub index: u64,
    /// 20 byte adres
    pub address: [u8; 20],
    /// Gelen TX şifre çözme için transmission key
    pub transmission_key: [u8; 32],
}

impl DiversifiedAddress {
    /// Adresi `0x` ön ekli hex string olarak döndürür.
    pub fn address_hex(&self) -> String {
        let mut s = String::with_capacity(42);
        s.push_str("0x");
        for b in &self.address {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }
}

/// IVK'dan çoklu adres üretici
///
/// Penumbra: `IncomingViewingKey::diversified_generator(diversifier)`
/// Tek IVK'dan farklı use-case'ler için sonsuz adres üretilebilir:
/// - index=0 → birincil adres (cüzdan)
/// - index=1 → borsa adresi (izole)
/// - index=2 → miras heir adresi
/// - index=N → her alacaklı için ayrı adres (gizlilik)
pub struct AddressDiversifier<'a> {
    ivk: &'a IncomingViewingKey,
}

impl<'a> AddressDiversifier<'a> {
    /// Verilen `IncomingViewingKey` ile yeni bir `AddressDiversifier` oluşturur.
    pub fn new(ivk: &'a IncomingViewingKey) -> Self {
        Self { ivk }
    }

    /// Belirli bir index için adres üret
    pub fn address_at(&self, index: u64) -> DiversifiedAddress {
        let mut h = Hasher::new();
        h.update(DOMAIN_DIVERSIFIER);
        h.update(self.ivk.as_bytes());
        h.update(&index.to_le_bytes());
        let hash = *h.finalize().as_bytes();

        let mut address = [0u8; 20];
        address.copy_from_slice(&hash[0..20]);

        // Transmission key: farklı domain
        let mut h2 = Hasher::new();
        h2.update(b"nova-shield.keys.transmission_key_v1");
        h2.update(self.ivk.as_bytes());
        h2.update(&index.to_le_bytes());
        let transmission_key = *h2.finalize().as_bytes();

        DiversifiedAddress { index, address, transmission_key }
    }

    /// İlk N adresi üret
    pub fn addresses(&self, count: u64) -> Vec<DiversifiedAddress> {
        (0..count).map(|i| self.address_at(i)).collect()
    }

    /// Bir adresin bu IVK'ya ait olup olmadığını doğrula
    pub fn owns_address(&self, addr: &[u8; 20], index: u64) -> bool {
        let expected = self.address_at(index);
        &expected.address == addr
    }

    /// Adresin hangi index'e ait olduğunu bul (brute force, max_search_depth'e kadar)
    pub fn find_index(&self, addr: &[u8; 20], max_search_depth: u64) -> Option<u64> {
        for i in 0..max_search_depth {
            if self.owns_address(addr, i) {
                return Some(i);
            }
        }
        None
    }
}

// ─── SPEND KEY (ANA MASTER KEY) ──────────────────────────────────────────────

/// Ana gizli anahtar (master key) — Penumbra `SpendKey` analog.
///
/// Penumbra'da: `SpendKey { ask: SpendAuthKey, fvk: FullViewingKey }`
/// Nova'da: SpendKey hem spend_auth'u hem türetilmiş tüm key'leri tutar.
///
/// Bu yapı bir "cüzdan kimliği"dir — seed phrase yerine geçer.
/// Nova'da birden fazla cihaza MPC ile dağıtılmış key share'ler olarak saklanır.
///
/// Penumbra referans: `crates/core/keys/src/keys/spend.rs:SpendKey`
///
/// ```text
/// // Penumbra orijinal:
/// pub struct SpendKey {
///     ask: SpendAuthKey,
///     fvk: FullViewingKey,
/// }
/// impl SpendKey {
///     pub fn generate() -> Self { ... }
///     pub fn full_viewing_key(&self) -> &FullViewingKey { &self.fvk }
///     pub fn spend_auth_key(&self) -> &SpendAuthKey { &self.ask }
/// }
/// ```
pub struct SpendKey {
    spend_auth: SpendAuthKey,
    fvk: FullViewingKey,
}

impl SpendKey {
    /// Rastgele yeni SpendKey üret.
    pub fn generate() -> CryptoResult<Self> {
        let spend_auth = SpendAuthKey::generate()?;
        let fvk = FullViewingKey::derive_from(&spend_auth);
        Ok(Self { spend_auth, fvk })
    }

    /// Deterministik SpendKey (test/recovery için).
    pub fn from_seed(seed: &[u8]) -> Self {
        let mut h = Hasher::new();
        h.update(b"nova-shield.spend_key.from_seed.v1");
        h.update(seed);
        let bytes: [u8; 32] = *h.finalize().as_bytes();
        let spend_auth = SpendAuthKey::from_bytes(bytes);
        let fvk = FullViewingKey::derive_from(&spend_auth);
        Self { spend_auth, fvk }
    }

    /// FullViewingKey referansı.
    pub fn full_viewing_key(&self) -> &FullViewingKey {
        &self.fvk
    }

    /// IncomingViewingKey türet.
    pub fn incoming_viewing_key(&self) -> IncomingViewingKey {
        self.fvk.incoming_viewing_key()
    }

    /// Birincil Nova Shield adresi (diversifier=0).
    pub fn primary_address(&self) -> [u8; 20] {
        self.fvk.incoming_viewing_key().derive_address(0)
    }

    /// Özel amaçlı adres üret (miras için heir adres, borsa adresi, vb.).
    pub fn derive_address(&self, diversifier_index: u64) -> [u8; 20] {
        self.fvk.incoming_viewing_key().derive_address(diversifier_index)
    }

    /// SpendAuthKey referansı (imzalama için).
    pub fn spend_auth_key(&self) -> &SpendAuthKey {
        &self.spend_auth
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Penumbra key hierarchy: SpendKey → FVK → IVK → Address zinciri.
    #[test]
    fn test_spend_key_hierarchy() {
        let sk = SpendKey::generate().expect("SpendKey oluşturulmalı");

        // FullViewingKey türetildi mi?
        let fvk = sk.full_viewing_key();
        assert_ne!(fvk.ak, [0u8; 32], "ak sıfır olmamalı");

        // IncomingViewingKey türetildi mi?
        let ivk = sk.incoming_viewing_key();
        assert_ne!(ivk.as_bytes(), &[0u8; 32], "ivk sıfır olmamalı");
    }

    /// Penumbra diversifier: aynı key, farklı adresler.
    #[test]
    fn test_address_diversification() {
        let sk = SpendKey::from_seed(b"test-seed-for-diversification");

        let addr0 = sk.derive_address(0);
        let addr1 = sk.derive_address(1);
        let addr2 = sk.derive_address(2);
        let addr100 = sk.derive_address(100);

        // Tüm adresler farklı olmalı
        assert_ne!(addr0, addr1);
        assert_ne!(addr1, addr2);
        assert_ne!(addr0, addr100);
    }

    /// Penumbra determinism: aynı seed → aynı adres.
    #[test]
    fn test_deterministic_address_from_seed() {
        let sk1 = SpendKey::from_seed(b"my-seed-phrase-replacement");
        let sk2 = SpendKey::from_seed(b"my-seed-phrase-replacement");

        assert_eq!(sk1.primary_address(), sk2.primary_address(),
            "Aynı seed → aynı birincil adres");
    }

    /// Penumbra key separation: farklı key'ler farklı adres üretir.
    #[test]
    fn test_different_seeds_different_addresses() {
        let sk1 = SpendKey::from_seed(b"seed-one");
        let sk2 = SpendKey::from_seed(b"seed-two");

        assert_ne!(sk1.primary_address(), sk2.primary_address(),
            "Farklı seed → farklı birincil adres");
    }

    /// IVK'dan FullViewingKey olmadan adres türetme (heir senaryosu).
    #[test]
    fn test_ivk_standalone_address_derivation() {
        let sk = SpendKey::from_seed(b"inheritance-scenario");

        // Heir'e sadece IVK verilir (harcama yapamaz ama adresi bilebilir)
        let ivk = sk.incoming_viewing_key();
        let heir_addr = ivk.derive_address(0);

        // SpendKey'den türetilen primary adres ile eşleşmeli
        assert_eq!(heir_addr, sk.primary_address(),
            "IVK.address(0) == SpendKey.primary_address()");
    }

    /// Nullifier key: aynı note → aynı nullifier.
    #[test]
    fn test_nullifier_determinism() {
        let sk = SpendKey::from_seed(b"nullifier-test");
        let nk = sk.full_viewing_key().nullifier_key();

        let note_commitment = b"note-commitment-bytes-32-long---";
        let null1 = nk.derive_nullifier(note_commitment);
        let null2 = nk.derive_nullifier(note_commitment);

        assert_eq!(null1, null2, "Deterministic nullifier");
    }

    /// Nullifier key: farklı note → farklı nullifier.
    #[test]
    fn test_nullifier_uniqueness() {
        let sk = SpendKey::from_seed(b"nullifier-unique-test");
        let nk = sk.full_viewing_key().nullifier_key();

        let null1 = nk.derive_nullifier(b"note-commitment-A");
        let null2 = nk.derive_nullifier(b"note-commitment-B");

        assert_ne!(null1, null2, "Farklı note → farklı nullifier");
    }

    /// OVK: giden TX key türetimi.
    #[test]
    fn test_outgoing_viewing_key_tx_key() {
        let sk = SpendKey::from_seed(b"ovk-test");
        let ovk = sk.full_viewing_key().outgoing_viewing_key();

        let tx_id = b"tx-12345";
        let key = ovk.derive_tx_key(tx_id);
        assert_ne!(key, [0u8; 32], "TX key sıfır olmamalı");

        // Deterministik
        let key2 = ovk.derive_tx_key(tx_id);
        assert_eq!(key, key2);
    }

    /// IVK note key: gelen TX şifre çözme anahtarı.
    #[test]
    fn test_incoming_viewing_key_note_key() {
        let sk = SpendKey::from_seed(b"ivk-note-test");
        let ivk = sk.incoming_viewing_key();

        let ephemeral = b"sender-ephemeral-pubkey-32bytes-";
        let k1 = ivk.derive_note_key(ephemeral);
        let k2 = ivk.derive_note_key(ephemeral);
        assert_eq!(k1, k2, "Deterministik note key");
    }

    /// Penumbra key isolation: FVK'dan spend_auth key çıkartılamaz.
    /// (Bu sadece API düzeyinde — FVK'nın spend_auth erişimi yok)
    #[test]
    fn test_fvk_cannot_derive_spend_auth() {
        let sk = SpendKey::from_seed(b"isolation-test");
        let fvk = sk.full_viewing_key();

        // FVK sadece ak, nk, ovk içeriyor — spend_auth erişimi yok
        // Bu test sadece derleme zamanında kontrol edilir:
        // `fvk.spend_auth` → compile error (field yok)
        assert_ne!(fvk.ak, [0u8; 32]);
        // FVK'dan spend auth key türetilemez — test bunu confirm eder
        // gerçek kanıt: API'de metot yok
    }

    /// SpendKey generate: her seferinde farklı adres üretir.
    #[test]
    fn test_generate_random_produces_unique_keys() {
        let sk1 = SpendKey::generate().unwrap();
        let sk2 = SpendKey::generate().unwrap();

        // Olasılıksal — pratik olarak asla eşit olmaz
        assert_ne!(sk1.primary_address(), sk2.primary_address(),
            "Rastgele üretilen key'ler farklı adres vermeli");
    }

    // ─── Task 97: Address Diversification testleri ────────────────────────────

    /// Farklı index → farklı adres
    #[test]
    fn test_diversifier_different_indices() {
        let sk = SpendKey::from_seed(b"diversifier-test-seed");
        let ivk = sk.incoming_viewing_key();
        let div = AddressDiversifier::new(&ivk);

        let a0 = div.address_at(0);
        let a1 = div.address_at(1);
        let a2 = div.address_at(2);

        assert_ne!(a0.address, a1.address);
        assert_ne!(a1.address, a2.address);
        assert_ne!(a0.address, a2.address);
    }

    /// Aynı index → aynı adres (deterministik)
    #[test]
    fn test_diversifier_deterministic() {
        let sk = SpendKey::from_seed(b"det-seed");
        let ivk = sk.incoming_viewing_key();
        let div = AddressDiversifier::new(&ivk);

        let a1 = div.address_at(42);
        let a2 = div.address_at(42);
        assert_eq!(a1.address, a2.address);
        assert_eq!(a1.transmission_key, a2.transmission_key);
    }

    /// Farklı IVK → farklı adresler
    #[test]
    fn test_diversifier_different_ivks() {
        let sk1 = SpendKey::from_seed(b"ivk1");
        let sk2 = SpendKey::from_seed(b"ivk2");
        let ivk1 = sk1.incoming_viewing_key();
        let ivk2 = sk2.incoming_viewing_key();
        let div1 = AddressDiversifier::new(&ivk1);
        let div2 = AddressDiversifier::new(&ivk2);

        let a1 = div1.address_at(0).address;
        let a2 = div2.address_at(0).address;
        assert_ne!(a1, a2);
    }

    /// N adres üret
    #[test]
    fn test_diversifier_generate_multiple() {
        let sk = SpendKey::from_seed(b"multi-addr");
        let ivk = sk.incoming_viewing_key();
        let div = AddressDiversifier::new(&ivk);

        let addrs = div.addresses(10);
        assert_eq!(addrs.len(), 10);

        // Tüm adresler farklı
        let unique: std::collections::HashSet<[u8; 20]> = addrs.iter().map(|a| a.address).collect();
        assert_eq!(unique.len(), 10);
    }

    /// Adres sahipliği doğrulama
    #[test]
    fn test_diversifier_owns_address() {
        let sk = SpendKey::from_seed(b"owns-test");
        let ivk = sk.incoming_viewing_key();
        let div = AddressDiversifier::new(&ivk);

        let addr5 = div.address_at(5);
        assert!(div.owns_address(&addr5.address, 5));
        assert!(!div.owns_address(&addr5.address, 6)); // yanlış index
    }

    /// Adres indeksi bulma
    #[test]
    fn test_diversifier_find_index() {
        let sk = SpendKey::from_seed(b"find-test");
        let ivk = sk.incoming_viewing_key();
        let div = AddressDiversifier::new(&ivk);

        let addr3 = div.address_at(3).address;
        let found = div.find_index(&addr3, 10).unwrap();
        assert_eq!(found, 3);

        // Rastgele adres → bulunamaz
        let random_addr = [0xABu8; 20];
        assert!(div.find_index(&random_addr, 100).is_none());
    }

    /// Transmission key adresten bağımsız
    #[test]
    fn test_diversifier_transmission_key_unique() {
        let sk = SpendKey::from_seed(b"tk-test");
        let ivk = sk.incoming_viewing_key();
        let div = AddressDiversifier::new(&ivk);

        let a = div.address_at(0);
        let b = div.address_at(1);
        assert_ne!(a.transmission_key, b.transmission_key);
    }

    /// address_hex formatı
    #[test]
    fn test_diversified_address_hex() {
        let sk = SpendKey::from_seed(b"hex-test");
        let ivk = sk.incoming_viewing_key();
        let div = AddressDiversifier::new(&ivk);
        let addr = div.address_at(0);
        let hex = addr.address_hex();
        assert!(hex.starts_with("0x"));
        assert_eq!(hex.len(), 42); // "0x" + 40 hex chars
    }
}
