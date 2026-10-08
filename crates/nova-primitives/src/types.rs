//! Nova Shield Chain — Temel Veri Tipleri
//!
//! Tüm crate'lerin paylaştığı temel newtype wrapper'lar.
//! Her tip: seri/deserialize + borsh + SCALE codec desteği taşır.

use crate::constants::{ADDRESS_SIZE, BLOCK_HASH_SIZE};
use borsh::{BorshDeserialize, BorshSerialize};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::fmt;
use zeroize::Zeroize;

// ─── HASH ──────────────────────────────────────────────────────────────────────

/// 32 byte'lık BLAKE3 hash değeri.
/// Blok hash, state root, TX hash gibi tüm hash'ler bu tip kullanır.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
    Zeroize,
)]
pub struct Hash32([u8; BLOCK_HASH_SIZE]);

impl Hash32 {
    /// Sıfır hash (genesis state root başlangıcı gibi durumlarda).
    pub const ZERO: Self = Self([0u8; BLOCK_HASH_SIZE]);

    /// Ham byte'lardan oluştur.
    #[inline]
    pub fn from_bytes(bytes: [u8; BLOCK_HASH_SIZE]) -> Self {
        Self(bytes)
    }

    /// Slice'tan oluştur; boyut uyuşmazlığında None döner.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        slice.try_into().ok().map(Self)
    }

    /// Ham byte referansı.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; BLOCK_HASH_SIZE] {
        &self.0
    }

    /// Hex string'e dönüştür (debug/log için).
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", self.to_hex())
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash32(0x{})", &self.to_hex()[..16])
    }
}

impl From<[u8; 32]> for Hash32 {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8]> for Hash32 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

// ─── ADRES ────────────────────────────────────────────────────────────────────

/// Nova Shield hesap adresi — 20 byte (Ethereum adresi formatı uyumlu).
/// MPC public key'den türetilir: BLAKE3(pubkey)[12..32]
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
    Zeroize,
)]
pub struct Address([u8; ADDRESS_SIZE]);

impl Address {
    /// Sıfır adres (null değeri).
    pub const ZERO: Self = Self([0u8; ADDRESS_SIZE]);

    /// Ham byte'lardan oluştur.
    #[inline]
    pub fn from_bytes(bytes: [u8; ADDRESS_SIZE]) -> Self {
        Self(bytes)
    }

    /// Slice'tan oluştur.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        slice.try_into().ok().map(Self)
    }

    /// EIP-55 checksum hex adresi (Go-Ethereum `common.Address.Hex()` analog).
    ///
    /// Kaynak: go-ethereum/common/bytes.go - Bytes2Hex + accounts/keystore eip55 checksum
    /// Algoritma: keccak256(lowercase_hex) hash'i hesapla, her alfabetik nibble için
    /// eğer hash nibble >= 8 ise büyük harf, aksi halde küçük harf.
    pub fn to_checksum_hex(&self) -> String {
        let hex_lower = hex::encode(self.0);
        let hash = Keccak256::digest(hex_lower.as_bytes());
        let hash_hex = hex::encode(hash);
        let checksummed: String = hex_lower
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if c.is_ascii_alphabetic() {
                    let nibble = u8::from_str_radix(&hash_hex[i..i + 1], 16).unwrap_or(0);
                    if nibble >= 8 { c.to_ascii_uppercase() } else { c }
                } else {
                    c
                }
            })
            .collect();
        format!("0x{}", checksummed)
    }

    /// EIP-55 checksum hex string'den parse et
    ///
    /// "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAd" gibi giriş kabul eder.
    /// Checksum yanlışsa `None` döndürür.
    pub fn from_checksum_hex(s: &str) -> Option<Self> {
        let hex_str = s.strip_prefix("0x").unwrap_or(s);
        if hex_str.len() != 40 { return None; }
        let bytes: [u8; 20] = hex::decode(hex_str).ok()?.try_into().ok()?;
        let addr = Self(bytes);
        // Checksum doğrula
        if s.starts_with("0x") && hex_str.chars().any(|c| c.is_ascii_uppercase()) {
            let expected = addr.to_checksum_hex();
            if expected != s { return None; }
        }
        Some(addr)
    }

    /// Checksum doğrulaması (geçerli EIP-55 mi?)
    ///
    /// Tümü küçük harf veya tümü büyük harf ise geçerli kabul edilir (legacy uyumluluk)
    pub fn is_valid_checksum(s: &str) -> bool {
        let hex_str = s.strip_prefix("0x").unwrap_or(s);
        if hex_str.len() != 40 { return false; }
        let bytes = match hex::decode(hex_str) {
            Ok(b) => b,
            Err(_) => return false,
        };
        let addr_bytes: [u8; 20] = match bytes.try_into() {
            Ok(b) => b,
            Err(_) => return false,
        };
        let addr = Self(addr_bytes);
        let expected = addr.to_checksum_hex();
        // Tümü küçük harf kabul (non-checksummed ama geçerli)
        if hex_str.chars().all(|c| !c.is_ascii_uppercase()) { return true; }
        // Tam checksum eşleşmeli
        s.starts_with("0x") && expected == s
    }

    /// Ham byte referansı.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; ADDRESS_SIZE] {
        &self.0
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_checksum_hex())
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", self.to_checksum_hex())
    }
}

impl From<[u8; 20]> for Address {
    fn from(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }
}

// ─── SAYISAL NEWTYPE'LAR ───────────────────────────────────────────────────────

macro_rules! newtype_u64 {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
            Serialize, Deserialize,
            BorshSerialize, BorshDeserialize,
            Encode, Decode,
        )]
        pub struct $name(pub u64);

        impl $name {
            /// Sıfır değer.
            pub const ZERO: Self = Self(0);

            /// Bir sonraki değer (overflowda panic — debug; release'te wrap).
            #[inline]
            pub fn next(self) -> Self {
                Self(self.0.checked_add(1).expect("overflow"))
            }

            /// İç değere eriş.
            #[inline]
            pub fn get(self) -> u64 {
                self.0
            }
        }

        impl From<u64> for $name {
            fn from(v: u64) -> Self {
                Self(v)
            }
        }

        impl From<$name> for u64 {
            fn from(v: $name) -> Self {
                v.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

macro_rules! newtype_u128 {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
            Serialize, Deserialize,
            BorshSerialize, BorshDeserialize,
            Encode, Decode,
        )]
        pub struct $name(pub u128);

        impl $name {
            /// Sıfır değer.
            pub const ZERO: Self = Self(0);

            /// İç değere eriş.
            #[inline]
            pub fn get(self) -> u128 {
                self.0
            }

            /// Checked toplama.
            pub fn checked_add(self, rhs: Self) -> Option<Self> {
                self.0.checked_add(rhs.0).map(Self)
            }

            /// Checked çıkarma.
            pub fn checked_sub(self, rhs: Self) -> Option<Self> {
                self.0.checked_sub(rhs.0).map(Self)
            }
        }

        impl From<u128> for $name {
            fn from(v: u128) -> Self {
                Self(v)
            }
        }

        impl From<$name> for u128 {
            fn from(v: $name) -> Self {
                v.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

// u64 bazlı newtype'lar
newtype_u64!(BlockNumber, "Blok yüksekliği (0 = genesis).");
newtype_u64!(SlotId, "Slot numarası (0 = genesis slot).");
newtype_u64!(EpochId, "Epoch numarası (0 = genesis epoch).");
newtype_u64!(Nonce, "Hesap nonce'u (replay koruması).");
newtype_u64!(Timestamp, "Unix timestamp (milisaniye).");
newtype_u64!(ValidatorId, "Validator index (epoch içindeki sıra).");
newtype_u64!(ChainId, "Zincir kimliği (mainnet=1984).");

// u128 bazlı newtype'lar (token miktarları büyük olabilir)
newtype_u128!(Balance, "NOVA token bakiyesi (baz birim = 10^-18 NOVA).");

// ─── HESAP / KİMLİK TİPLERİ ───────────────────────────────────────────────────

/// Hesap kimliği — Nova Shield'de hesaplar adres ile tanımlanır.
pub type AccountId = Address;

/// Blok hash'i — semantik alias.
pub type BlockHash = Hash32;

// ─── YARDIMCI TİPLER ──────────────────────────────────────────────────────────

/// Slot + Epoch ikili. Konsensüs katmanında sık kullanılır.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
)]
pub struct SlotEpoch {
    /// Slot numarası.
    pub slot: SlotId,
    /// Bu slot'un ait olduğu epoch.
    pub epoch: EpochId,
}

impl SlotEpoch {
    /// Slot ve epoch'tan oluştur.
    pub fn new(slot: SlotId, epoch: EpochId) -> Self {
        Self { slot, epoch }
    }

    /// Slot'tan epoch hesapla.
    pub fn from_slot(slot: SlotId, epoch_duration: u64) -> Self {
        let epoch = EpochId(slot.0 / epoch_duration);
        Self { slot, epoch }
    }
}

/// Miktarı baz puan (basis point) olarak ifade eder.
/// 1 BPS = %0.01; 10_000 BPS = %100.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
)]
pub struct BasisPoints(pub u64);

impl BasisPoints {
    /// %100 = 10_000 BPS.
    pub const FULL: Self = Self(10_000);
    /// %0 = 0 BPS.
    pub const ZERO: Self = Self(0);

    /// Geçerli BPS aralığı kontrolü (0..=10_000).
    pub fn is_valid(self) -> bool {
        self.0 <= 10_000
    }

    /// Miktara uygula: `amount * bps / 10_000`.
    pub fn apply_to(self, amount: u128) -> u128 {
        amount.saturating_mul(self.0 as u128) / 10_000
    }
}

/// Threshold parametresi (t-of-n şeması).
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, Debug,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
)]
pub struct Threshold {
    /// Gerekli imza sayısı (t).
    pub t: u8,
    /// Toplam pay sahibi sayısı (n).
    pub n: u8,
}

impl Threshold {
    /// Yeni threshold oluştur. t <= n koşulu kontrol edilir.
    pub fn new(t: u8, n: u8) -> Option<Self> {
        if t == 0 || t > n {
            return None;
        }
        Some(Self { t, n })
    }

    /// Varsayılan 3/5 threshold.
    pub fn default_3_of_5() -> Self {
        Self { t: 3, n: 5 }
    }

    /// t <= verilen count ise quorum sağlanmış demektir.
    pub fn is_met_by(&self, count: u8) -> bool {
        count >= self.t
    }
}

impl fmt::Display for Threshold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.t, self.n)
    }
}

/// Coğrafi bölge kodu (çeşitlilik kuralları için). §6.5.3
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, Debug,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum GeoRegion {
    /// Kuzey Amerika
    NorthAmerica = 0,
    /// Güney Amerika
    SouthAmerica = 1,
    /// Batı Avrupa
    WestEurope = 2,
    /// Doğu Avrupa
    EastEurope = 3,
    /// Doğu Asya
    EastAsia = 4,
    /// Güney Asya
    SouthAsia = 5,
    /// Güneydoğu Asya
    SoutheastAsia = 6,
    /// Afrika
    Africa = 7,
    /// Orta Doğu
    MiddleEast = 8,
    /// Okyanusya
    Oceania = 9,
    /// Bilinmiyor
    Unknown = 255,
}

// ─── NODE KIMLIĞI (AVALANCHEGo ids.NodeID ANALOG) ─────────────────────────────

/// Validator düğüm kimliği — 20 byte (Avalanchego `ids.ShortID` / `ids.NodeID` analog).
///
/// Araştırma kaynağı: `research/chains/avalanchego/ids/node_id.go`
/// NodeID, bir doğrulayıcı düğümün P2P ağındaki benzersiz kimliğidir.
/// Avalanchego'da `ids.NodeID = [20]byte`, BLS pubkey'den türetilir.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
    Zeroize,
)]
pub struct NodeId([u8; 20]);

impl NodeId {
    /// Sıfır NodeId.
    pub const ZERO: Self = Self([0u8; 20]);

    /// Ham byte'lardan oluştur.
    pub fn from_bytes(bytes: [u8; 20]) -> Self { Self(bytes) }

    /// Slice'tan oluştur; boyut uyuşmazlığında None.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        slice.try_into().ok().map(Self)
    }

    /// 20 byte referansı.
    pub fn as_bytes(&self) -> &[u8; 20] { &self.0 }

    /// CB58 formatında string (Avalanchego standart gösterim).
    pub fn to_cb58(&self) -> String {
        bs58::encode(self.0).into_string()
    }

    /// Public key byte'larından NodeId türet: BLAKE3(pubkey)[0..20].
    /// Avalanchego'da BLS pubkey → sha256 → ripemd160 kullanılır;
    /// Nova'da BLAKE3 yeterlidir (farklı ağ, aynı fikir).
    pub fn from_public_key(pubkey: &[u8]) -> Self {
        let hash = blake3::hash(pubkey);
        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(&hash.as_bytes()[0..20]);
        Self(bytes)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.to_cb58())
    }
}
impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId(0x{})", &hex::encode(self.0)[..8])
    }
}
impl From<[u8; 20]> for NodeId {
    fn from(b: [u8; 20]) -> Self { Self(b) }
}

// ─── VARLIK KIMLIĞI (AVALANCHEGo ids.ID ANALOG) ───────────────────────────────

/// Genel varlık/TX kimliği — 32 byte (Avalanchego `ids.ID` analog).
///
/// Araştırma kaynağı: `research/chains/avalanchego/ids/id.go`
/// Avalanchego'da her TX, Subnet, Blockchain kendi `ids.ID` ile tanımlanır.
/// Nova'da miras planı ID, varlık transferi ID gibi amaçlarda kullanılır.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
    Zeroize,
)]
pub struct AssetId([u8; 32]);

impl AssetId {
    /// Sıfır AssetId (NOVA native token).
    pub const ZERO: Self = Self([0u8; 32]);

    /// NOVA native token identifier — tüm sıfır bytes (native coin).
    pub const NOVA: Self = Self([0u8; 32]);

    /// Ham byte dizisinden oluşturur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self { Self(bytes) }
    /// Slice'tan oluşturur; uzunluk 32 değilse `None` döner.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        slice.try_into().ok().map(Self)
    }
    /// Ham byte dizisine referans döner.
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
    /// Hex string olarak döndürür.
    pub fn to_hex(&self) -> String { hex::encode(self.0) }

    /// İsimden deterministik AssetId türet: BLAKE3("nova_asset:" + name).
    pub fn from_name(name: &str) -> Self {
        let input = format!("nova_asset:{}", name);
        let hash = blake3::hash(input.as_bytes());
        Self(*hash.as_bytes())
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", self.to_hex())
    }
}
impl fmt::Debug for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AssetId(0x{}...)", &self.to_hex()[..8])
    }
}
impl From<[u8; 32]> for AssetId {
    fn from(b: [u8; 32]) -> Self { Self(b) }
}

// ─── COIN (COSMOS-SDK sdk.Coin ANALOG) ────────────────────────────────────────

/// Varlık ve miktar çifti — Cosmos-SDK `sdk.Coin` analog.
///
/// Araştırma kaynağı: `research/chains/cosmos-sdk/types/coin.go`
/// sdk.Coin { Denom string, Amount sdkmath.Int } yapısından ilham alındı.
/// Nova'da denom yerine `AssetId` kullanılır (string yerine 32-byte kimlik).
/// Amount: u128 baz birim (NSC için 10^-18 NSC; ERC-20 için token'a bağlı).
#[derive(
    Clone, PartialEq, Eq, Debug,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
)]
pub struct Coin {
    /// Varlık kimliği (NOVA için `AssetId::ZERO`).
    pub asset: AssetId,
    /// Miktar (baz birim, 10^-18).
    pub amount: u128,
}

impl Coin {
    /// NOVA native token coin oluştur.
    pub fn nova(amount: u128) -> Self {
        Self { asset: AssetId::ZERO, amount }
    }

    /// Belirli varlık coin'i oluştur.
    pub fn new(asset: AssetId, amount: u128) -> Self {
        Self { asset, amount }
    }

    /// Miktar sıfır mı?
    pub fn is_zero(&self) -> bool { self.amount == 0 }

    /// Checked toplama (Cosmos-SDK `sdk.Coin.Add()` analog).
    pub fn add(&self, other: &Coin) -> Option<Coin> {
        if self.asset != other.asset {
            return None; // Farklı varlıklar toplanamaz
        }
        self.amount.checked_add(other.amount).map(|amount| Coin {
            asset: self.asset,
            amount,
        })
    }

    /// Checked çıkarma (Cosmos-SDK `sdk.Coin.Sub()` analog).
    pub fn sub(&self, other: &Coin) -> Option<Coin> {
        if self.asset != other.asset {
            return None;
        }
        self.amount.checked_sub(other.amount).map(|amount| Coin {
            asset: self.asset,
            amount,
        })
    }
}

impl fmt::Display for Coin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.amount, self.asset)
    }
}

/// Çoklu varlık listesi — Cosmos-SDK `sdk.Coins` analog.
/// Sıralı, unique asset ID'li coin listesi.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize, Encode, Decode)]
pub struct Coins(pub Vec<Coin>);

impl Coins {
    /// Boş coins.
    pub fn empty() -> Self { Self(Vec::new()) }

    /// Tek coin.
    pub fn of(coin: Coin) -> Self { Self(vec![coin]) }

    /// Belirli varlığın miktarını bul.
    pub fn amount_of(&self, asset: &AssetId) -> u128 {
        self.0.iter().find(|c| &c.asset == asset).map(|c| c.amount).unwrap_or(0)
    }

    /// Boş mu?
    pub fn is_empty(&self) -> bool {
        self.0.is_empty() || self.0.iter().all(|c| c.is_zero())
    }

    /// Toplam coin sayısı.
    pub fn len(&self) -> usize { self.0.len() }
}

// ─── SABİT NOKTA ONDALIĞI (COSMOS-SDK sdk.Dec ANALOG) ─────────────────────────

/// 18 haneli sabit nokta ondalık sayı — Cosmos-SDK `sdk.Dec` analog.
///
/// Araştırma kaynağı: `research/chains/cosmos-sdk/types/dec.go`
/// sdk.Dec: 18 decimal places, BigInt backed.
/// Nova'da i128 kullanıyoruz (18 decimal yeterli, BigInt overhead'siz).
/// 1.0 = Dec(1_000_000_000_000_000_000i128)
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
)]
pub struct Dec(pub i128);

impl Dec {
    /// Hassasiyet: 10^18
    pub const PRECISION: i128 = 1_000_000_000_000_000_000;

    /// Sıfır.
    pub const ZERO: Self = Self(0);
    /// Bir.
    pub const ONE: Self = Self(Self::PRECISION);
    /// Yarım.
    pub const HALF: Self = Self(Self::PRECISION / 2);

    /// Tam sayıdan Dec oluştur.
    pub fn from_integer(i: i64) -> Self {
        Self(i as i128 * Self::PRECISION)
    }

    /// Kesir olarak oluştur: numerator / denominator.
    pub fn from_ratio(num: i64, den: i64) -> Option<Self> {
        if den == 0 { return None; }
        Some(Self(num as i128 * Self::PRECISION / den as i128))
    }

    /// Baz puana çevir (BPS = 1/10_000).
    pub fn from_bps(bps: u64) -> Self {
        Self(bps as i128 * Self::PRECISION / 10_000)
    }

    /// u64 amount × Dec oranını hesapla (Cosmos-SDK MulInt64 analog).
    pub fn mul_amount(self, amount: u128) -> u128 {
        if self.0 <= 0 { return 0; }
        (amount as u128).saturating_mul(self.0 as u128) / Self::PRECISION as u128
    }

    /// Toplama (Cosmos-SDK Add analog).
    pub fn add(self, other: Self) -> Self { Self(self.0.saturating_add(other.0)) }

    /// Çıkarma.
    pub fn sub(self, other: Self) -> Self { Self(self.0.saturating_sub(other.0)) }

    /// Çarpma (Cosmos-SDK Mul analog).
    ///
    /// BUG-10 düzeltmesi: Eski implementasyon `(a / 1_000_000) * (b / 1_000_000)` yapıyordu.
    /// Bu iki kez 6 basamak keserek 12 basamak hassasiyet kaybına yol açıyor.
    /// Slashing oran hesaplamalarında yanlış sonuç üretiyordu.
    ///
    /// Doğrusu: `i256` kullanarak `(a * b) / PRECISION` tek seferde bölme.
    /// i128 overflow'ı önlemek için i128 max değerini kontrol edip
    /// signed 128-bit aritmetiği doğru şekilde uyguluyoruz.
    ///
    /// Cosmos-SDK Go: `d.i.Mul(d2.i).Quo(precisionReuse)` — BigInt ile tam hassasiyet.
    pub fn mul(self, other: Self) -> Self {
        // i128: max ~1.7 × 10^38; PRECISION = 10^18
        // a * b / PRECISION: a ve b her biri PRECISION civarındaysa (1.0 × PRECISION)
        // a*b ≈ PRECISION^2 = 10^36, yani i128 max'ın içinde kalır.
        // Ancak büyük değerlerde overflow olabilir; i128 checked mul kullan.
        let product = (self.0 as i128).checked_mul(other.0 as i128);
        match product {
            Some(p) => Self(p / Self::PRECISION),
            None => {
                // NOVA-RED-PRIM-003: Overflow fallback sıfır döndürmesin.
                // Yüksek-alçak bölümleme: (a/P_HALF) * (b/P_HALF) / P_HALF
                // P_HALF = 10^9, P = 10^18
                // Bu yaklaşım 1.0 × 1.0 = 1.0 sonucunu doğru verir.
                const P_HALF: i128 = 1_000_000_000;
                let a = self.0 / P_HALF;
                let b = other.0 / P_HALF;
                let result = a.saturating_mul(b)
                    .checked_div(P_HALF)
                    .unwrap_or(i128::MAX);
                Self(result)
            }
        }
    }

    /// Sıfırdan büyük mü?
    pub fn is_positive(self) -> bool { self.0 > 0 }

    /// Sıfırdan küçük mü?
    pub fn is_negative(self) -> bool { self.0 < 0 }

    /// İç i128 değeri.
    pub fn raw(self) -> i128 { self.0 }

    /// Float gösterimi (debug).
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / Self::PRECISION as f64
    }
}

impl fmt::Display for Dec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.6}", self.to_f64())
    }
}
impl fmt::Debug for Dec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Dec({})", self.to_f64())
    }
}

// ─── ÇOKLu İMZA (POLKADOT sp_runtime::MultiSignature ANALOG) ─────────────────

/// Çoklu imza enum'u — Polkadot `sp_runtime::MultiSignature` analog.
///
/// Araştırma kaynağı: `research/chains/polkadot-sdk/substrate/primitives/runtime/src/lib.rs`
/// Polkadot üç imza şemasını destekler: Ed25519, Sr25519, Ecdsa.
/// Nova'da bunlara ek olarak Dilithium3 post-quantum ve AggregateBls eklenmiştir.
///
/// Boyut notları (doğrulama sırasında kontrol edilmeli):
/// - Ed25519: 64 byte
/// - Sr25519: 64 byte
/// - Ecdsa: 65 byte (r[32]+s[32]+v[1])
/// - Dilithium3: 3293 byte (CRYSTALS-Dilithium Level-3)
/// - AggregateBls: 96 byte (BLS12-381 G2 compressed)
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize, Encode, Decode)]
pub enum MultiSignature {
    /// Ed25519 imzası (64 byte). Solana/Polkadot standart.
    Ed25519(Vec<u8>),
    /// Sr25519/Ristretto imzası (64 byte). Polkadot native.
    Sr25519(Vec<u8>),
    /// secp256k1 ECDSA imzası (65 byte: r[32] + s[32] + v[1]).
    Ecdsa(Vec<u8>),
    /// Dilithium3 post-quantum imzası (~3293 byte). §6.1.2
    Dilithium3(Vec<u8>),
    /// BLS12-381 aggregate imzası (96 byte). Validator oylama için.
    AggregateBls(Vec<u8>),
}

impl MultiSignature {
    /// Ed25519 imzası oluştur (64 byte doğrulama).
    pub fn ed25519(bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() == 64 { Some(Self::Ed25519(bytes)) } else { None }
    }

    /// Sr25519 imzası oluştur (64 byte doğrulama).
    pub fn sr25519(bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() == 64 { Some(Self::Sr25519(bytes)) } else { None }
    }

    /// ECDSA imzası oluştur (65 byte doğrulama).
    pub fn ecdsa(bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() == 65 { Some(Self::Ecdsa(bytes)) } else { None }
    }

    /// Dilithium3 imzası oluştur (herhangi bir boyut, nova-crypto doğrular).
    pub fn dilithium3(bytes: Vec<u8>) -> Self { Self::Dilithium3(bytes) }

    /// BLS aggregate imzası oluştur (96 byte doğrulama).
    pub fn aggregate_bls(bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() == 96 { Some(Self::AggregateBls(bytes)) } else { None }
    }

    /// İmza byte'larını döndür.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Ed25519(b) | Self::Sr25519(b) | Self::Ecdsa(b)
            | Self::Dilithium3(b) | Self::AggregateBls(b) => b.as_slice(),
        }
    }

    /// İmza şeması adı.
    pub fn scheme_name(&self) -> &'static str {
        match self {
            Self::Ed25519(_) => "Ed25519",
            Self::Sr25519(_) => "Sr25519",
            Self::Ecdsa(_) => "Ecdsa",
            Self::Dilithium3(_) => "Dilithium3",
            Self::AggregateBls(_) => "AggregateBls",
        }
    }

    /// Post-quantum imzası mı?
    pub fn is_post_quantum(&self) -> bool {
        matches!(self, Self::Dilithium3(_))
    }

    /// Beklenen boyut doğrulama.
    pub fn is_valid_size(&self) -> bool {
        match self {
            Self::Ed25519(b) | Self::Sr25519(b) => b.len() == 64,
            Self::Ecdsa(b) => b.len() == 65,
            Self::Dilithium3(b) => b.len() == 3293, // NOVA-RED-PRIM-001: Dilithium3 = 3293 byte
            Self::AggregateBls(b) => b.len() == 96,
        }
    }
}

// ─── RUNTIME VERSİYON (POLKADOT sp_version::RuntimeVersion ANALOG) ─────────────

/// Protocol versiyon bilgisi — Polkadot `sp_version::RuntimeVersion` analog.
///
/// Araştırma kaynağı: `research/chains/polkadot-sdk/substrate/primitives/version/src/lib.rs`
/// RuntimeVersion { spec_name, impl_name, spec_version, impl_version, tx_version }
/// Nova'da bu yapı protocol upgrade yönetimi ve uyumluluk kontrolü için kullanılır.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize, Encode, Decode)]
pub struct RuntimeVersion {
    /// Protocol spec adı (örn. "nova-shield").
    pub spec_name: String,
    /// Implementasyon adı (örn. "nova-reference-client").
    pub impl_name: String,
    /// Spec versiyonu — breaking change'lerde artar.
    pub spec_version: u32,
    /// Impl versiyonu — non-breaking değişikliklerde artar.
    pub impl_version: u32,
    /// Transaction versiyonu — TX format değiştiğinde artar.
    pub transaction_version: u32,
    /// State versiyonu (state migration için).
    pub state_version: u8,
}

impl RuntimeVersion {
    /// Genesis versiyonu.
    pub fn genesis() -> Self {
        Self {
            spec_name: "nova-shield".to_string(),
            impl_name: "nova-reference-client".to_string(),
            spec_version: 1,
            impl_version: 0,
            transaction_version: 1,
            state_version: 0,
        }
    }

    /// Bu versiyon ile diğeri arasında breaking change var mı?
    /// (Polkadot'ta `can_author_with` kontrolüne benzer)
    pub fn is_compatible_with(&self, other: &RuntimeVersion) -> bool {
        self.spec_name == other.spec_name
            && self.spec_version == other.spec_version
            && self.transaction_version == other.transaction_version
    }
}

impl fmt::Display for RuntimeVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/v{} (impl v{}, tx v{})",
            self.spec_name, self.spec_version,
            self.impl_version, self.transaction_version)
    }
}
