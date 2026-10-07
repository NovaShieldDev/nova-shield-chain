//! Nova Shield Chain — Temel Trait'ler
//!
//! Tüm protocol bileşenlerinin implement etmesi gereken arayüzler.
//! Bu trait'ler `nova-primitives` katmanında tanımlanır çünkü
//! birden fazla crate tarafından kullanılırlar.

use crate::{
    crypto::{Hash, PublicKey, Signature},
    errors::Result,
    types::{Address, Balance, BlockNumber, Nonce, SlotId, Timestamp},
};
use std::fmt;

// ─── HASH TRAİTİ ──────────────────────────────────────────────────────────────

/// BLAKE3 ile hashlenebilen tipler.
pub trait Hashable {
    /// Bu değerin kanonik BLAKE3 hash'ini hesapla.
    /// Implementasyon deterministik olmalı: aynı değer → aynı hash.
    fn hash(&self) -> Hash;
}

// ─── SERİLEŞTİRME TRAİTİ ─────────────────────────────────────────────────────

/// Borsh ile encode/decode edilebilen tipler.
pub trait BorshCodable: borsh::BorshSerialize + borsh::BorshDeserialize {
    /// Borsh encode → byte vec.
    fn encode_borsh(&self) -> Vec<u8>
    where
        Self: Sized,
    {
        borsh::to_vec(self).expect("Borsh encode her zaman başarılı olmalı")
    }

    /// Borsh decode ← byte slice.
    fn decode_borsh(data: &[u8]) -> Result<Self>
    where
        Self: Sized,
    {
        borsh::from_slice(data).map_err(|e| crate::errors::NovaError::Serialization {
            context: e.to_string(),
        })
    }
}

// Blanket implementasyon: BorshSerialize + BorshDeserialize olan her tip BorshCodable'dır.
impl<T: borsh::BorshSerialize + borsh::BorshDeserialize> BorshCodable for T {}

// ─── HESAP TRAİTİ ─────────────────────────────────────────────────────────────

/// Nova Shield hesabının asgari arayüzü.
pub trait Account: fmt::Debug {
    /// Hesap adresi.
    fn address(&self) -> Address;

    /// MPC public key (threshold imzalama için).
    fn public_key(&self) -> &PublicKey;

    /// NSC bakiyesi.
    fn balance(&self) -> Balance;

    /// İşlem nonce'u (replay koruması).
    fn nonce(&self) -> Nonce;

    /// Hesap aktif mi?
    fn is_active(&self) -> bool;

    /// Hesap dondurulmuş mu?
    fn is_frozen(&self) -> bool;

    /// Miras süreci başlatılmış mı?
    fn is_in_inheritance_process(&self) -> bool;
}

// ─── İMZALANABİLİR TRAİTİ ───────────────────────────────────────────────────

/// MPC threshold imzası alınabilen mesajlar.
pub trait Signable: Hashable {
    /// İmzalanacak mesaj byte'ları (hash üzerinden).
    fn signing_bytes(&self) -> Vec<u8> {
        self.hash().as_bytes().to_vec()
    }
}

// ─── İMZA DOĞRULAMA TRAİTİ ───────────────────────────────────────────────────

/// İmza doğrulayıcı.
pub trait SignatureVerifier {
    /// İmzayı verify et.
    /// `message`: imzalanan mesaj hash'i
    /// `signature`: ECDSA veya hibrit imza
    /// `public_key`: imzalayan tarafın public key'i
    fn verify(
        &self,
        message: &Hash,
        signature: &Signature,
        public_key: &PublicKey,
    ) -> Result<()>;
}

// ─── TX TRAİTİ ────────────────────────────────────────────────────────────────

/// Nova Shield transaction'larının asgari arayüzü.
pub trait Transaction: Hashable + Signable + fmt::Debug {
    /// TX hash'i (içeriğin BLAKE3'ü).
    fn tx_hash(&self) -> Hash {
        self.hash()
    }

    /// Gönderen adres.
    fn sender(&self) -> Address;

    /// Alıcı adres (varsa).
    fn recipient(&self) -> Option<Address>;

    /// Transfer miktarı.
    fn amount(&self) -> Balance;

    /// Gönderenin nonce'u (sıra/tekrar koruması).
    fn nonce(&self) -> Nonce;

    /// Hedef slot numarası (§5.2.3 replay koruması).
    fn target_slot(&self) -> SlotId;

    /// TX imzası.
    fn signature(&self) -> &Signature;

    /// Gas limiti.
    fn gas_limit(&self) -> u64;

    /// Gas fiyatı (NSC baz birim).
    fn gas_price(&self) -> u128;

    /// TX geçerliliğini temel düzeyde kontrol et (imza, nonce vs).
    fn validate_basic(&self) -> Result<()>;
}

// ─── BLOK TRAİTİ ──────────────────────────────────────────────────────────────

/// Blok başlığı için asgari arayüz.
pub trait BlockHeader: Hashable + fmt::Debug {
    /// Blok hash'i.
    fn block_hash(&self) -> Hash {
        self.hash()
    }

    /// Üst blok hash'i.
    fn parent_hash(&self) -> Hash;

    /// Blok yüksekliği.
    fn block_number(&self) -> BlockNumber;

    /// Slot numarası.
    fn slot(&self) -> SlotId;

    /// State root (tüm hesap durumunun Merkle root'u).
    fn state_root(&self) -> Hash;

    /// TX root (blok TX'lerinin Merkle root'u).
    fn transactions_root(&self) -> Hash;

    /// Receipts root.
    fn receipts_root(&self) -> Hash;

    /// Blok üretim zamanı (Unix ms).
    fn timestamp(&self) -> Timestamp;

    /// Blok üreticisi (validator adresi).
    fn proposer(&self) -> Address;

    /// Validator aggregate BLS imzası (finality kanıtı).
    fn validator_signature(&self) -> &Signature;
}

// ─── STATE TRAİTİ ─────────────────────────────────────────────────────────────

/// Blok chain state'inin asgari okuma arayüzü.
pub trait StateReader {
    /// Hesap bakiyesini oku.
    fn get_balance(&self, address: &Address) -> Result<Balance>;

    /// Hesap nonce'unu oku.
    fn get_nonce(&self, address: &Address) -> Result<Nonce>;

    /// Hesap aktif mi?
    fn account_exists(&self, address: &Address) -> bool;

    /// State root hash'i.
    fn state_root(&self) -> Hash;
}

/// State'i değiştiren işlemler.
pub trait StateWriter: StateReader {
    /// Bakiye transferi.
    fn transfer(&mut self, from: &Address, to: &Address, amount: Balance) -> Result<()>;

    /// Nonce artır.
    fn increment_nonce(&mut self, address: &Address) -> Result<()>;

    /// State root'u güncelle.
    fn commit(&mut self) -> Result<Hash>;
}

// ─── KONSENSüS TRAİTİ ────────────────────────────────────────────────────────

/// Konsensüs motoru arayüzü.
pub trait ConsensusEngine: fmt::Debug + Send + Sync {
    /// Slot liderini belirle.
    fn slot_leader(&self, slot: SlotId) -> Result<Address>;

    /// Blok öner.
    fn propose_block(&mut self, slot: SlotId) -> Result<()>;

    /// Gelen bloğu doğrula.
    fn validate_block(&self, block_hash: &Hash) -> Result<()>;

    /// Blok finalize et.
    fn finalize_block(&mut self, block_hash: &Hash) -> Result<()>;

    /// Son finalize edilen blok.
    fn last_finalized(&self) -> Option<Hash>;
}

// ─── SLASHING TRAİTİ ─────────────────────────────────────────────────────────

/// Slashing delili.
pub trait SlashingEvidence: Hashable + fmt::Debug {
    /// Ceza uygulanacak validator adresi.
    fn validator_address(&self) -> Address;

    /// Slashing oranı (BPS cinsinden).
    fn slash_rate_bps(&self) -> u64;

    /// Delil geçerliliğini doğrula.
    fn verify(&self) -> Result<()>;
}

// ─── GUARDIAN TRAİTİ ─────────────────────────────────────────────────────────

/// Guardian'ların uygulayabileceği eylemler.
pub trait GuardianAction: fmt::Debug {
    /// Bu eylemi onaylayan guardian adresi.
    fn guardian_address(&self) -> Address;

    /// Eylemin hedef hesabı.
    fn target_account(&self) -> Address;

    /// Eylem timestamp'i.
    fn timestamp(&self) -> Timestamp;

    /// Guardian imzası.
    fn signature(&self) -> &Signature;
}

// ─── NOVA MODÜL TRAİTİ (COSMOS-SDK AppModule ANALOG) ─────────────────────────

/// Slot başlangıcı context — Cosmos-SDK `BeginBlockRequest` analog.
///
/// Araştırma kaynağı: `research/chains/cosmos-sdk/baseapp/abci.go`
/// BeginBlock(ctx, req) → her blok başında tüm modüllere çağrılır.
/// Nova'da blok yerine slot kavramı kullanılır.
#[derive(Debug, Clone)]
pub struct BeginSlotCtx {
    /// Mevcut slot numarası.
    pub slot: SlotId,
    /// Slot timestamp (Unix milisaniye).
    pub timestamp: Timestamp,
    /// Önceki slot'un blok hash'i.
    pub parent_hash: Hash,
}

/// Slot sonu context — Cosmos-SDK `EndBlockRequest` analog.
///
/// Araştırma kaynağı: `research/chains/cosmos-sdk/baseapp/abci.go`
/// EndBlock(ctx) → her blok sonunda validator güncellemeleri için çağrılır.
#[derive(Debug, Clone)]
pub struct EndSlotCtx {
    /// Mevcut slot numarası.
    pub slot: SlotId,
    /// Slot timestamp.
    pub timestamp: Timestamp,
    /// Bu slot'ta işlenen TX sayısı.
    pub tx_count: u32,
    /// Toplam gas kullanımı.
    pub gas_used: u64,
}

/// Nova Shield Chain modül arayüzü — Cosmos-SDK `AppModule` analog.
///
/// Araştırma kaynağı: `research/chains/cosmos-sdk/types/module/module.go`
///
/// ```go
/// // Cosmos-SDK AppModule interface:
/// type AppModule interface {
///     AppModuleBasic
///     RegisterInvariants(sdk.InvariantRegistry)
///     InitGenesis(sdk.Context, codec.JSONCodec, json.RawMessage) []abci.ValidatorUpdate
///     ExportGenesis(sdk.Context, codec.JSONCodec) json.RawMessage
///     BeginBlock(sdk.Context, abci.RequestBeginBlock)
///     EndBlock(sdk.Context, abci.RequestEndBlock) []abci.ValidatorUpdate
/// }
/// ```
///
/// Nova'da adapt edilmiş hali:
pub trait NovaModule: fmt::Debug + Send + Sync {
    /// Modül adı (örn. "inheritance", "freeze", "auction").
    fn module_name(&self) -> &'static str;

    /// Slot başlangıcında çağrılır — inaktivite kontrolü, zamanlı işlemler.
    /// Cosmos-SDK BeginBlock analog.
    fn begin_slot(&mut self, ctx: &BeginSlotCtx) -> Result<()>;

    /// Slot sonunda çağrılır — state commit, validator güncellemeleri.
    /// Cosmos-SDK EndBlock analog.
    fn end_slot(&mut self, ctx: &EndSlotCtx) -> Result<()>;

    /// Genesis state'i yükle — Cosmos-SDK InitGenesis analog.
    fn init_genesis(&mut self) -> Result<()>;

    /// Modül sağlıklı mı? — Avalanchego Health interface analog.
    fn is_healthy(&self) -> bool;
}

// ─── SAĞLIK KONTROL TRAİTİ (AVALANCHEGo health.Checker ANALOG) ────────────────

/// Sağlık raporu — Avalanchego `health.Result` analog.
///
/// Araştırma kaynağı: `research/chains/avalanchego/health/`
/// Avalanchego'da her bileşen `HealthCheck() (interface{}, error)` uygular.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// Bileşen sağlıklı mı?
    pub healthy: bool,
    /// Sağlık mesajı (hata veya "OK").
    pub message: String,
    /// Son kontrol zamanı (Unix ms).
    pub checked_at_ms: u64,
}

impl HealthReport {
    /// Sağlıklı rapor.
    pub fn ok(checked_at_ms: u64) -> Self {
        Self { healthy: true, message: "OK".to_string(), checked_at_ms }
    }

    /// Hatalı rapor.
    pub fn error(reason: impl Into<String>, checked_at_ms: u64) -> Self {
        Self { healthy: false, message: reason.into(), checked_at_ms }
    }
}

/// Sağlık kontrol arayüzü — Avalanchego `health.Checker` analog.
///
/// Araştırma kaynağı: `research/chains/avalanchego/health/health.go`
/// ```go
/// type Checker interface {
///     HealthCheck(context.Context) (interface{}, error)
/// }
/// ```
pub trait HealthChecker: Send + Sync {
    /// Sağlık kontrolü yap — Avalanchego HealthCheck() analog.
    fn health_check(&self, current_ms: u64) -> HealthReport;
}

// ─── GENESIS TRAİTİ (COSMOS-SDK GenesisState ANALOG) ─────────────────────────

/// Genesis state yönetimi — Cosmos-SDK `GenesisState` analog.
///
/// Araştırma kaynağı: `research/chains/cosmos-sdk/types/genesis.go`
/// Her modülün genesis state'i JSON olarak export/import edilebilir.
pub trait GenesisHandler: fmt::Debug {
    /// Genesis state'i validate et.
    fn validate_genesis(&self) -> Result<()>;

    /// Default genesis state oluştur.
    fn default_genesis() -> Vec<u8> where Self: Sized;
}

// ─── SLASHING İNVARYANT TRAİTİ (COSMOS-SDK InvariantRegistry ANALOG) ──────────

/// Slashable ihlal — Cosmos-SDK `Invariant` analog.
///
/// Araştırma kaynağı: `research/chains/cosmos-sdk/x/staking/keeper/invariants.go`
/// Cosmos-SDK'da invariant'lar her blokta kontrol edilir; ihlal tespit edilirse
/// zincir durdurulur veya ceza uygulanır.
/// Nova'da slashable invariant'lar validator/guardian davranışını denetler.
pub trait SlashableInvariant: fmt::Debug {
    /// İnvaryant adı (tanımlama için).
    fn invariant_name(&self) -> &'static str;

    /// İnvaryant kontrolü.
    /// `Ok(())` → ihlal yok; `Err(...)` → ihlal var, ceza uygulanacak.
    fn check(&self) -> Result<()>;

    /// Ceza oranı (BPS cinsinden).
    fn slash_rate_bps(&self) -> u64;
}
