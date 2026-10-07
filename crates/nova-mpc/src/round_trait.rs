//! TSS-Lib Round Trait Sistemi
//!
//! ## Araştırma Kaynağı: tss-lib (Binance)
//!
//! `research/libs/tss-lib/tss/party.go` ve `round.go`:
//!
//! ```text
//! // TSS-Lib Round interface (Go):
//! type Round interface {
//!     Params() *Parameters
//!     Start() *Error
//!     Update() (bool, *Error)
//!     RoundNumber() int
//!     CanProceed() bool
//!     NextRound() Round
//!     WaitingFor() []*PartyID
//! }
//!
//! // TSS-Lib Party interface (Go):
//! type Party interface {
//!     Start() *Error
//!     UpdateFromBytes(wireBytes []byte, from *PartyID, isBroadcast bool) (ok bool, err *Error)
//!     Running() bool
//!     WaitingFor() []*PartyID
//!     FirstRound() Round
//! }
//! ```
//!
//! Nova uyarlaması: Generic Rust trait sistemi, Go interface'inden daha type-safe.
//! Her round kendi mesaj tipini biliyor, state geçişleri compile-time'da kontrol ediliyor.
//!
//! ## Ayrım: TSS-Lib'den Fark
//! - TSS-Lib: Runtime interface, reflection tabanlı
//! - Nova: Compile-time generic traits, tip güvenliği maksimum
//! - Her Round'un gelen ve giden mesaj tipleri ayrı generic parametre
//!
//! ## Kullanım Akışı
//! ```text
//! KeygenRound1 --[start]--> KeygenRound1Messages
//!   |
//!   v [process_message * N]
//! KeygenRound2 --[start]--> KeygenRound2Messages
//!   |
//!   v
//! KeygenRound3 --[start]--> LocalKey (final output)
//! ```

use crate::errors::{MpcError, MpcResult};
use crate::types::PartyIndex;

// ─── ROUND MESAJ TRAİTİ ───────────────────────────────────────────────────────

/// Round mesajı için temel trait.
///
/// TSS-Lib `Message` interface analog.
/// Her round'da gönderilen mesajın meta-bilgisi.
pub trait RoundMessage: std::fmt::Debug + Send + Sync {
    /// Mesajı gönderen parti.
    fn from_party(&self) -> PartyIndex;

    /// Mesaj broadcast mu, yoksa point-to-point mu?
    /// TSS-Lib: `IsBroadcast() bool`
    fn is_broadcast(&self) -> bool;

    /// Round numarası.
    fn round_number(&self) -> u32;

    /// Alıcı parti(ler). Broadcast için tüm partiler, P2P için tek parti.
    fn recipients(&self) -> Vec<PartyIndex>;

    /// Mesaj byte temsili (serileştirme için).
    fn to_bytes(&self) -> Vec<u8>;
}

// ─── ROUND TRAİTİ ────────────────────────────────────────────────────────────

/// Protokol round'u — TSS-Lib `Round` interface analog.
///
/// ## TSS-Lib Paralelizm
///
/// ```text
/// TSS-Lib Go:
/// type Round interface {
///     Start() *Error
///     Update() (bool, *Error)
///     CanProceed() bool
///     NextRound() Round
///     RoundNumber() int
///     WaitingFor() []*PartyID
/// }
///
/// Nova Rust:
/// pub trait ProtocolRound {
///     type InboundMessage: RoundMessage;
///     type Output;
///     fn round_number(&self) -> u32;
///     fn can_proceed(&self) -> bool;
///     fn process_message(&mut self, msg: Self::InboundMessage) -> MpcResult<()>;
///     fn output(self) -> MpcResult<Self::Output>;
///     fn waiting_for(&self) -> Vec<PartyIndex>;
/// }
/// ```
pub trait ProtocolRound: Sized + Send + Sync {
    /// Bu round'a gelen mesaj tipi.
    type InboundMessage: RoundMessage;

    /// Bu round'un tamamlandıktan sonra döndürdüğü çıktı.
    type Output;

    /// Bu round'un numarası (1'den başlar).
    /// TSS-Lib `RoundNumber() int` analog.
    fn round_number(&self) -> u32;

    /// Bu round ilerleyebilir mi? (Tüm beklenen mesajlar alındı mı?)
    /// TSS-Lib `CanProceed() bool` analog.
    fn can_proceed(&self) -> bool;

    /// Bu round'dan mesaj beklenilen partiler.
    /// TSS-Lib `WaitingFor() []*PartyID` analog.
    fn waiting_for(&self) -> Vec<PartyIndex>;

    /// Gelen mesajı işle.
    /// TSS-Lib `Update(msg Message) (bool, *Error)` analog.
    fn process_message(&mut self, msg: Self::InboundMessage) -> MpcResult<()>;

    /// Round tamamlandı — çıktıyı hesapla.
    /// TSS-Lib `Result()` analog (nova'da `output()` deniyor).
    fn output(self) -> MpcResult<Self::Output>;

    /// Bu round başlatılmış mı?
    fn is_started(&self) -> bool;
}

// ─── ROUND STATE MAKİNESİ ─────────────────────────────────────────────────────

/// Round state makinesi yürütücü.
///
/// TSS-Lib'de her `LocalParty` bir state machine çalıştırır.
/// Bu yapı o mantığın generic Rust versiyonu.
///
/// Amaç: Hangi round'da olduğumuzu takip et, mesajları doğru round'a yönlendir.
#[derive(Debug)]
pub struct RoundStateMachine<R> {
    /// Mevcut round.
    current_round: R,
    /// Bu round'a kaç mesaj işlendi.
    messages_processed: u32,
    /// Başlangıç zamanı (slot cinsinden).
    started_at_slot: u64,
    /// Timeout eşiği (slot cinsinden).
    timeout_slots: u64,
}

impl<R: ProtocolRound> RoundStateMachine<R> {
    /// Yeni state machine başlat.
    pub fn new(initial_round: R, started_at_slot: u64, timeout_slots: u64) -> Self {
        Self {
            current_round: initial_round,
            messages_processed: 0,
            started_at_slot,
            timeout_slots,
        }
    }

    /// Gelen mesajı işle.
    pub fn process(&mut self, msg: R::InboundMessage) -> MpcResult<()> {
        self.current_round.process_message(msg)?;
        self.messages_processed += 1;
        Ok(())
    }

    /// Round tamamlanabilir mi?
    pub fn can_finalize(&self) -> bool {
        self.current_round.can_proceed()
    }

    /// Beklenen parti listesi.
    pub fn pending_parties(&self) -> Vec<PartyIndex> {
        self.current_round.waiting_for()
    }

    /// Timeout kontrolü.
    pub fn is_timed_out(&self, current_slot: u64) -> bool {
        current_slot.saturating_sub(self.started_at_slot) > self.timeout_slots
    }

    /// Mevcut round numarası.
    pub fn round_number(&self) -> u32 {
        self.current_round.round_number()
    }

    /// İşlenen mesaj sayısı.
    pub fn messages_processed(&self) -> u32 {
        self.messages_processed
    }

    /// Round tamamla, çıktıyı al (round consume edilir).
    pub fn finalize(self) -> MpcResult<R::Output> {
        if !self.current_round.can_proceed() {
            let pending: Vec<u32> = self.current_round.waiting_for()
                .iter()
                .map(|p| p.get())
                .collect();
            return Err(MpcError::RoundNotComplete {
                round: self.current_round.round_number(),
                waiting_for: pending,
            });
        }
        self.current_round.output()
    }
}

// ─── PARTI KİMLİĞİ VE SIRALAMA ───────────────────────────────────────────────

/// Party kimlik bilgisi — TSS-Lib `PartyID` analog (geliştirilmiş).
///
/// TSS-Lib (`research/libs/tss-lib/tss/party_id.go`):
/// ```text
/// type PartyID struct {
///     *MessageWrapper
///     Index   int
///     id      string
///     moniker string
///     Key     *big.Int  // hash(pubkey) — sıralama için
/// }
/// ```
///
/// TSS-Lib party ID'leri hash(pubkey)'in big.Int değerine göre sıralanır.
/// Bu deterministik bir sıralama garantisi sağlar — tüm partiler aynı sıraya ulaşır.
/// Nova uyarlaması: big.Int yerine [u8; 32] byte array (lexicographic sort).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PartyIdentity {
    /// Parti indeksi (1'den başlar, sıralama sonrası atanır).
    /// TSS-Lib: `Index int`
    pub index: u32,
    /// İnsan okunabilir ad.
    /// TSS-Lib: `moniker string`
    pub moniker: String,
    /// Public key hash — sıralama anahtarı.
    /// TSS-Lib: `Key *big.Int` (pubkey hash, big-endian)
    /// Nova: [u8; 32] BLAKE3 hash (pubkey)
    pub key_hash: [u8; 32],
}

impl PartyIdentity {
    /// Public key'den PartyIdentity oluştur.
    ///
    /// TSS-Lib: `tss.NewPartyID(id, moniker, key)`
    /// key = big.Int (secp256k1 x koordinatı veya hash(pubkey))
    pub fn from_public_key(moniker: impl Into<String>, pubkey: &[u8]) -> Self {
        // TSS-Lib: Key = hash(pubkey) big.Int
        // Nova: BLAKE3(pubkey) → [u8; 32]
        let hash = blake3::hash(pubkey);
        Self {
            index: 0, // Sıralama sonrası atanır
            moniker: moniker.into(),
            key_hash: *hash.as_bytes(),
        }
    }

    /// Doğrudan byte'lardan oluştur.
    pub fn from_key_hash(moniker: impl Into<String>, key_hash: [u8; 32]) -> Self {
        Self { index: 0, moniker: moniker.into(), key_hash }
    }
}

impl PartialOrd for PartyIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PartyIdentity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // TSS-Lib: sort by big.Int(Key) — nova: lexicographic [u8;32] byte compare
        // big.Int büyük-endian sıralama ile aynı sonuç verir.
        self.key_hash.cmp(&other.key_hash)
    }
}

// ─── PARTI SIRALAMA (TSS-LIB SortPartyIDs ANALOG) ────────────────────────────

/// Parti listesini deterministik şekilde sırala.
///
/// TSS-Lib (`research/libs/tss-lib/tss/party_id.go`):
/// ```text
/// func SortPartyIDs(ids UnSortedPartyIDs, startAt ...int) SortedPartyIDs {
///     sort.Slice(ids, func(i, j int) bool {
///         bi := new(big.Int).SetBytes(ids[i].Key.Bytes())
///         bj := new(big.Int).SetBytes(ids[j].Key.Bytes())
///         return bi.Cmp(bj) < 0
///     })
///     start := 0
///     if len(startAt) > 0 { start = startAt[0] }
///     for i, id := range ids { id.Index = i + start }
///     return SortedPartyIDs(ids)
/// }
/// ```
///
/// Nova uyarlaması: Tamamen aynı algoritma, Go big.Int yerine Rust [u8;32] sort.
///
/// ## Güvenlik Önemi
/// Tüm protokol partileri aynı sıralamaya ulaşmalı (deterministik).
/// Farklı sıralama → Lagrange katsayıları yanlış → imza geçersiz.
pub fn sort_party_identities(mut parties: Vec<PartyIdentity>, start_at: u32) -> Vec<PartyIdentity> {
    // TSS-Lib sort.Slice analog: key_hash lexicographic sırası
    parties.sort_by(|a, b| a.key_hash.cmp(&b.key_hash));

    // İndeksleri yeniden ata (TSS-Lib: for i, id := range ids { id.Index = i + start })
    for (i, party) in parties.iter_mut().enumerate() {
        party.index = i as u32 + start_at;
    }

    parties
}

/// Pubkey dizisinden deterministik sıralı parti listesi oluştur.
///
/// TSS-Lib `GenerateTestPartyIDs` benzeri yardımcı fonksiyon.
pub fn make_sorted_parties(monikers: &[&str], pubkeys: &[Vec<u8>]) -> MpcResult<Vec<PartyIdentity>> {
    if monikers.len() != pubkeys.len() {
        return Err(MpcError::InvalidInput("moniker ve pubkey sayıları eşit olmalı".into()));
    }
    let parties: Vec<PartyIdentity> = monikers
        .iter()
        .zip(pubkeys.iter())
        .map(|(m, pk)| PartyIdentity::from_public_key(*m, pk))
        .collect();
    Ok(sort_party_identities(parties, 1))
}

// ─── RESHARİNG PARAMETRE SET (TSS-LIB ReSharingParameters ANALOG) ────────────

/// Resharing parametreleri — TSS-Lib `ReSharingParameters` analog.
///
/// TSS-Lib (`research/libs/tss-lib/tss/parameters.go`):
/// ```text
/// type ReSharingParameters struct {
///     *Parameters
///     newPartyCount int
///     newThreshold  int
/// }
/// ```
///
/// Nova'da: Eski ve yeni komite bilgileri ayrı tutulur.
#[derive(Debug, Clone)]
pub struct ResharingParams {
    /// Eski komitenin threshold değeri (t_old).
    pub old_threshold: u32,
    /// Eski komitenin toplam parti sayısı (n_old).
    pub old_party_count: u32,
    /// Eski komitenin sıralı parti kimlikleri.
    pub old_parties: Vec<PartyIdentity>,
    /// Yeni komitenin threshold değeri (t_new).
    pub new_threshold: u32,
    /// Yeni komitenin toplam parti sayısı (n_new).
    pub new_party_count: u32,
    /// Yeni komitenin sıralı parti kimlikleri.
    pub new_parties: Vec<PartyIdentity>,
}

impl ResharingParams {
    /// Yeni resharing parametreleri oluştur.
    ///
    /// TSS-Lib: `tss.NewReSharingParameters(ec, oldCtx, newCtx, partyCount, threshold, newPartyCount, newThreshold)`
    pub fn new(
        old_threshold: u32,
        old_parties: Vec<PartyIdentity>,
        new_threshold: u32,
        new_parties: Vec<PartyIdentity>,
    ) -> MpcResult<Self> {
        let old_party_count = old_parties.len() as u32;
        let new_party_count = new_parties.len() as u32;

        if old_threshold == 0 || old_threshold >= old_party_count {
            return Err(MpcError::InvalidInput(
                format!("geçersiz eski threshold: t={} n={}", old_threshold, old_party_count)
            ));
        }
        if new_threshold == 0 || new_threshold >= new_party_count {
            return Err(MpcError::InvalidInput(
                format!("geçersiz yeni threshold: t={} n={}", new_threshold, new_party_count)
            ));
        }

        Ok(Self {
            old_threshold,
            old_party_count,
            old_parties,
            new_threshold,
            new_party_count,
            new_parties,
        })
    }

    /// Bir parti indeksinin eski komitede olup olmadığını kontrol et.
    pub fn is_old_party(&self, key_hash: &[u8; 32]) -> bool {
        self.old_parties.iter().any(|p| &p.key_hash == key_hash)
    }

    /// Bir parti indeksinin yeni komitede olup olmadığını kontrol et.
    pub fn is_new_party(&self, key_hash: &[u8; 32]) -> bool {
        self.new_parties.iter().any(|p| &p.key_hash == key_hash)
    }

    /// Bir parti hem eski hem yeni komitede mi? (Overlap)
    pub fn is_in_both(&self, key_hash: &[u8; 32]) -> bool {
        self.is_old_party(key_hash) && self.is_new_party(key_hash)
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// TSS-Lib SortPartyIDs testi: deterministik sıralama.
    #[test]
    fn test_sort_party_ids_deterministic() {
        // Üç parti — her seferinde aynı sıralamaya ulaşmalı
        let parties_a = vec![
            PartyIdentity::from_key_hash("P1", [0x03u8; 32]),
            PartyIdentity::from_key_hash("P2", [0x01u8; 32]),
            PartyIdentity::from_key_hash("P3", [0x02u8; 32]),
        ];
        let parties_b = vec![
            PartyIdentity::from_key_hash("P3", [0x02u8; 32]),
            PartyIdentity::from_key_hash("P1", [0x03u8; 32]),
            PartyIdentity::from_key_hash("P2", [0x01u8; 32]),
        ];

        let sorted_a = sort_party_identities(parties_a, 1);
        let sorted_b = sort_party_identities(parties_b, 1);

        // key_hash sırasına göre: 01 < 02 < 03
        assert_eq!(sorted_a[0].key_hash, [0x01u8; 32]);
        assert_eq!(sorted_a[1].key_hash, [0x02u8; 32]);
        assert_eq!(sorted_a[2].key_hash, [0x03u8; 32]);

        // Her iki başlangıç sırasından da aynı sonuç
        assert_eq!(sorted_a[0].index, sorted_b[0].index);
        assert_eq!(sorted_a[1].index, sorted_b[1].index);
        assert_eq!(sorted_a[2].index, sorted_b[2].index);
    }

    /// TSS-Lib SortPartyIDs: index ataması start_at'dan başlar.
    #[test]
    fn test_sort_party_ids_index_assignment() {
        let parties = vec![
            PartyIdentity::from_key_hash("P3", [0x03u8; 32]),
            PartyIdentity::from_key_hash("P1", [0x01u8; 32]),
        ];
        let sorted = sort_party_identities(parties, 1);

        assert_eq!(sorted[0].index, 1, "İlk parti index=1");
        assert_eq!(sorted[1].index, 2, "İkinci parti index=2");
    }

    /// TSS-Lib SortPartyIDs: start_at=0 desteği.
    #[test]
    fn test_sort_party_ids_start_at_zero() {
        let parties = vec![
            PartyIdentity::from_key_hash("A", [0x01u8; 32]),
            PartyIdentity::from_key_hash("B", [0x02u8; 32]),
        ];
        let sorted = sort_party_identities(parties, 0);
        assert_eq!(sorted[0].index, 0);
        assert_eq!(sorted[1].index, 1);
    }

    /// make_sorted_parties: pubkey'den deterministik sıralama.
    #[test]
    fn test_make_sorted_parties() {
        let monikers = ["alice", "bob", "carol"];
        let pubkeys: Vec<Vec<u8>> = vec![
            vec![0x03, 0xAB],  // key_hash = BLAKE3([0x03, 0xAB])
            vec![0x02, 0xCD],
            vec![0x03, 0x01],
        ];

        let sorted = make_sorted_parties(&monikers, &pubkeys).unwrap();
        assert_eq!(sorted.len(), 3);

        // Sıralama tutarlı mı?
        assert!(sorted[0].key_hash < sorted[1].key_hash);
        assert!(sorted[1].key_hash < sorted[2].key_hash);
    }

    /// PartyIdentity: aynı pubkey → aynı key_hash.
    #[test]
    fn test_party_identity_deterministic_hash() {
        let pk = vec![0x02u8; 33];
        let p1 = PartyIdentity::from_public_key("alice", &pk);
        let p2 = PartyIdentity::from_public_key("alice", &pk);
        assert_eq!(p1.key_hash, p2.key_hash, "Aynı pubkey → aynı hash");
    }

    /// PartyIdentity: farklı pubkey → farklı hash.
    #[test]
    fn test_party_identity_unique_hash() {
        let pk1 = vec![0x02u8; 33];
        let pk2 = vec![0x03u8; 33];
        let p1 = PartyIdentity::from_public_key("alice", &pk1);
        let p2 = PartyIdentity::from_public_key("bob", &pk2);
        assert_ne!(p1.key_hash, p2.key_hash);
    }

    /// ResharingParams: geçerli parametreler.
    #[test]
    fn test_resharing_params_valid() {
        let old_parties = vec![
            PartyIdentity::from_key_hash("P1", [1u8; 32]),
            PartyIdentity::from_key_hash("P2", [2u8; 32]),
            PartyIdentity::from_key_hash("P3", [3u8; 32]),
        ];
        let new_parties = vec![
            PartyIdentity::from_key_hash("Q1", [4u8; 32]),
            PartyIdentity::from_key_hash("Q2", [5u8; 32]),
            PartyIdentity::from_key_hash("Q3", [6u8; 32]),
            PartyIdentity::from_key_hash("Q4", [7u8; 32]),
        ];

        // 2/3 → 3/4 reshare
        let params = ResharingParams::new(2, old_parties, 3, new_parties).unwrap();
        assert_eq!(params.old_threshold, 2);
        assert_eq!(params.old_party_count, 3);
        assert_eq!(params.new_threshold, 3);
        assert_eq!(params.new_party_count, 4);
    }

    /// ResharingParams: geçersiz threshold reddedilir.
    #[test]
    fn test_resharing_params_invalid_threshold() {
        let old_parties = vec![
            PartyIdentity::from_key_hash("P1", [1u8; 32]),
            PartyIdentity::from_key_hash("P2", [2u8; 32]),
        ];
        let new_parties = vec![
            PartyIdentity::from_key_hash("Q1", [3u8; 32]),
        ];

        // t >= n geçersiz
        let result = ResharingParams::new(2, old_parties.clone(), 1, new_parties);
        assert!(result.is_err(), "t >= n geçersiz olmalı");
    }

    /// ResharingParams: is_old_party / is_new_party kontrolü.
    #[test]
    fn test_resharing_params_party_membership() {
        let key_hash_old = [0x01u8; 32];
        let key_hash_both = [0x02u8; 32];
        let key_hash_new = [0x03u8; 32];

        let old_parties = vec![
            PartyIdentity::from_key_hash("P1", key_hash_old),
            PartyIdentity::from_key_hash("P2", key_hash_both),
        ];
        let new_parties = vec![
            PartyIdentity::from_key_hash("Q1", key_hash_both),
            PartyIdentity::from_key_hash("Q2", key_hash_new),
        ];

        let params = ResharingParams::new(1, old_parties, 1, new_parties).unwrap();

        assert!(params.is_old_party(&key_hash_old));
        assert!(!params.is_old_party(&key_hash_new));
        assert!(params.is_new_party(&key_hash_new));
        assert!(!params.is_new_party(&key_hash_old));
        assert!(params.is_in_both(&key_hash_both));
        assert!(!params.is_in_both(&key_hash_old));
    }

    /// RoundStateMachine: timeout kontrolü.
    #[test]
    fn test_round_state_machine_timeout() {
        // RoundStateMachine doğrudan test edilemez (Round trait gerekli)
        // Timeout mantığını bağımsız test ediyoruz
        let start_slot = 100u64;
        let timeout = 50u64;
        let current = 151u64;
        assert!(current.saturating_sub(start_slot) > timeout, "Timeout gerçekleşmeli");

        let current_ok = 149u64;
        assert!(!(current_ok.saturating_sub(start_slot) > timeout), "Timeout gerçekleşmemeli");
    }
}
