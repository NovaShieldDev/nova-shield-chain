# Penumbra — Kapsamlı Kaynak Kodu Analizi

**Repo:** `research/chains/penumbra`
**Dil:** Rust
**Versiyon:** 2.1.1
**Analiz Tarihi:** 2026-07-17
**Analizi Yapan:** Claude (bizzat, agentsiz, satır satır)

---

## İçindekiler

1. [Genel Mimari ve Crate Yapısı](#1-genel-mimari-ve-crate-yapısı)
2. [Elliptic Curve: decaf377](#2-elliptic-curve-decaf377)
3. [Anahtar Hiyerarşisi](#3-anahtar-hiyerarşisi)
4. [SpendKey — Temel Harcama Anahtarı](#4-spendkey--temel-harcama-anahtarı)
5. [FullViewingKey — Tam Görüntüleme Anahtarı](#5-fullviewingkey--tam-görüntüleme-anahtarı)
6. [NullifierKey — Çift Harcama Önleme](#6-nullifierkey--çift-harcama-önleme)
7. [IncomingViewingKey — Gelen İşlemleri Görme](#7-incomingviewingkey--gelen-işlemleri-görme)
8. [Diversifier — Sonsuz Adres Çeşitlendirme](#8-diversifier--sonsuz-adres-çeşitlendirme)
9. [Adres Yapısı](#9-adres-yapısı)
10. [Fuzzy Message Detection (FMD)](#10-fuzzy-message-detection-fmd)
11. [Key Agreement (decaf377-ka)](#11-key-agreement-decaf377-ka)
12. [Note Modeli — Gizli UTXO](#12-note-modeli--gizli-utxo)
13. [Note Şifreleme ve Şifre Çözme](#13-note-şifreleme-ve-şifre-çözme)
14. [Tiered Commitment Tree (TCT)](#14-tiered-commitment-tree-tct)
15. [Nullifier Türetimi](#15-nullifier-türetimi)
16. [SpendProof — Groth16 ZK Kanıtı](#16-spendproof--groth16-zk-kanıtı)
17. [OutputProof — Çıktı ZK Kanıtı](#17-outputproof--çıktı-zk-kanıtı)
18. [Staking Sistemi](#18-staking-sistemi)
19. [DEX: Zswap ve Konsantre Likidite](#19-dex-zswap-ve-konsantre-likidite)
20. [Batch Swap Mekanizması](#20-batch-swap-mekanizması)
21. [Güvenlik Özellikleri ve Notlar](#21-güvenlik-özellikleri-ve-notlar)
22. [Nova Shield için Çıkarımlar](#22-nova-shield-için-çıkarımlar)
23. [Özet](#23-özet)

---

## 1. Genel Mimari ve Crate Yapısı

### Workspace Yapısı (`Cargo.toml`)

Penumbra monorepo olarak organize edilmiş, 50+ crate içeriyor:

```
crates/
├── crypto/
│   ├── tct/              -- Tiered Commitment Tree (Merkle ağacı)
│   ├── decaf377-fmd/     -- Fuzzy Message Detection
│   ├── decaf377-ka/      -- Key Agreement (ECDH analogu)
│   └── decaf377-rdsa/    -- Randomizable Digital Signature Algorithm
├── core/
│   ├── keys/             -- Anahtar hiyerarşisi (SpendKey → Address)
│   ├── component/
│   │   ├── shielded-pool/  -- Note, SpendProof, OutputProof
│   │   ├── sct/            -- State Commitment Tree + Nullifier
│   │   ├── stake/          -- Validator staking, delegasyon
│   │   ├── dex/            -- DEX (AMM + batch swap)
│   │   ├── governance/     -- On-chain yönetişim
│   │   └── fee/            -- Ücret yönetimi
│   └── app/              -- Ana uygulama state machine
└── proto/                -- Protobuf tanımları
```

### Kritik Bağımlılıklar

| Kütüphane | Versiyon | Kullanım |
|-----------|----------|---------|
| `ark-groth16` | — | ZK-SNARK ispatlama sistemi |
| `ark-r1cs-std` | — | R1CS constraint sistemi |
| `decaf377` | — | Özel elliptic curve |
| `poseidon377` | — | ZK-dostu hash fonksiyonu |
| `decaf377-fmd` | — | Fuzzy Message Detection |
| `cnidarium` | — | JMT tabanlı state store |
| `tendermint` | v0.37 | ABCI/consensus entegrasyon |
| `ark-bls12-377` | — | Pairing tabanlı ZK eğrisi |

### Çalışma Prensibi (Genel)

Penumbra, Zcash'in Sapling protokolünden ilham alır, ancak kendi eğrisi (decaf377), kendi hash fonksiyonu (poseidon377) ve kendi commitment tree (TCT) kullanır. Her işlem, hem fonksiyonellik hem de gizlilik için ZK-SNARK kanıtları içerir. Consensus için Tendermint kullanılır.

---

## 2. Elliptic Curve: decaf377

### Nedir?

decaf377, BLS12-377 eğrisinin scalar field (Fq) üzerinde inşa edilmiş Ristretto-benzeri bir prime-order elliptic curve grubudur. "decaf" ailesinin bir üyesidir, cofactor=8 olan bir Montgomery eğrisini cofactor=1'e indirger.

### Neden önemli?

- **Cofactor = 1**: Küçük alt gruplardan kaynaklanan güvenlik açıklarını tamamen ortadan kaldırır
- **Fq = BLS12-377'nin scalar field'ı**: Bu sayede ZK devreleri içinde native olarak temsil edilebilir — her field elemanı aynı zamanda bir ZK constraint'i olabilir
- **Tek codec**: Vartime ve constant-time sıkıştırma/açma (`vartime_compress`, `vartime_decompress`, `Encoding`)

### Kullanılan Tipler

```rust
// Scalar field (Fr) — imza, randomizer, blinding factor
decaf377::Fr

// Base field (Fq) — hash çıktıları, commitment'lar, nullifier'lar
decaf377::Fq

// Curve noktası
decaf377::Element

// Sıkıştırılmış nokta (32 byte)
decaf377::Encoding([u8; 32])
```

---

## 3. Anahtar Hiyerarşisi

Penumbra'nın anahtar hiyerarşisi:

```
SeedPhrase (BIP39 mnemonic)
    │
    │  PBKDF2-HMAC-SHA512 (BIP39 KDF)
    ▼
SpendKeyBytes ([u8; 32])
    │
    │  PRF expand (BLAKE2b/domain sep)
    ├──── ask (SigningKey<SpendAuth>)   -- Harcama imzalama
    └──── nk  (NullifierKey)           -- Nullifier türetimi
              │
              │  PRF expand
              ├──── ovk (OutgoingViewingKey)   -- Giden işlemleri izleme
              ├──── dk  (DiversifierKey)       -- Adres çeşitlendirme
              └──── ivk (IncomingViewingKey)   -- Gelen işlemleri izleme
                        │
                        │  Diversifier + ivk
                        ▼
                    Address (80 byte)
                    {diversifier, g_d, pk_d, ck_d}
```

---

## 4. SpendKey — Temel Harcama Anahtarı

**Dosya:** `crates/core/keys/src/keys/spend.rs`

### Struct Tanımı

```rust
pub struct SpendKey {
    seed: SpendKeyBytes,          // Ham 32-byte seed
    ask: SigningKey<SpendAuth>,   // Harcama imzalama anahtarı
    fvk: FullViewingKey,          // Tam görüntüleme anahtarı
}
```

### SpendKeyBytes → SpendKey Türetimi

```rust
impl From<SpendKeyBytes> for SpendKey {
    fn from(seed: SpendKeyBytes) -> Self {
        // PRF expand: BLAKE2b ile domain separation
        let ask = SigningKey::new_from_field(
            prf::expand_ff(b"Penumbra_ExpndSd", &seed.0, &[0; 1])
        );
        let nk = NullifierKey(
            prf::expand_ff(b"Penumbra_ExpndSd", &seed.0, &[1; 1])
        );
        let fvk = FullViewingKey::from_components(ask.into(), nk);
        Self { seed, ask, fvk }
    }
}
```

**Matematiksel açıklama:**
- `ask = BLAKE2b("Penumbra_ExpndSd" || seed || [0])` → Fr field elemanı
- `nk = BLAKE2b("Penumbra_ExpndSd" || seed || [1])` → Fq field elemanı

### BIP39 Türetimi

```rust
pub fn from_seed_phrase_bip39(seed_phrase: SeedPhrase, index: u64) -> Self {
    // BIP39 standart KDF
    pbkdf2::<Hmac<sha2::Sha512>>(
        password.as_bytes(),    // mnemonic kelimeleri
        salt.as_bytes(),        // "mnemonic{index}"
        NUM_PBKDF2_ROUNDS,     // 2048 round
        &mut spend_seed_bytes,  // 32 byte çıktı
    );
    SpendKeyBytes(spend_seed_bytes).into()
}
```

### BIP44 Türetimi (Ledger Uyumlu)

```rust
pub fn from_seed_phrase_bip44(seed_phrase: SeedPhrase, path: &Bip44Path) -> Self {
    // 1. BIP39: 64 byte seed
    pbkdf2::<Hmac<sha2::Sha512>>(password, "mnemonic", 2048, &mut seed_bytes);
    // 2. BIP44: child key türetimi
    let child_key = XPrv::derive_from_path(&seed_bytes, &path.path())
    let child_key_bytes = child_key.to_bytes(); // 32 byte
    SpendKeyBytes(child_key_bytes).into()
}
```

---

## 5. FullViewingKey — Tam Görüntüleme Anahtarı

**Dosya:** `crates/core/keys/src/keys/fvk.rs`

### Struct Tanımı

```rust
pub struct FullViewingKey {
    ak: VerificationKey<SpendAuth>,  // Harcama doğrulama (ask'ın public key'i)
    nk: NullifierKey,                // Nullifier key
    ovk: OutgoingViewingKey,         // Giden işlemleri izleme
    ivk: IncomingViewingKey,         // Gelen işlemleri izleme
}
```

### from_components(ak, nk) — Tam Türetim

```rust
pub fn from_components(ak: VerificationKey<SpendAuth>, nk: NullifierKey) -> Self {
    // OVK: İlk 32 byte
    let ovk_bytes = prf::expand(b"Penumbra_DeriOVK", nk.0.to_bytes(), ak_bytes)[0..32];
    let ovk = OutgoingViewingKey(ovk_bytes);

    // DK (DiversifierKey): İlk 16 byte
    let dk_bytes = prf::expand(b"Penumbra_DerivDK", nk.0.to_bytes(), ak_bytes)[0..16];
    let dk = DiversifierKey(dk_bytes);

    // IVK: poseidon377 hash, mod r indirgemesi
    let ivk_scalar = poseidon377::hash_2(&IVK_DOMAIN_SEP, (nk.0, ak_s)) % Fr::r();
    let ivk = IncomingViewingKey {
        ivk: ka::Secret::new_from_field(ivk_scalar),
        dk,
    };

    FullViewingKey { ak, nk, ovk, ivk }
}
```

**IVK domain separator:** `poseidon377::hash_2("Penumbra_FVKeyIVK", (nk, ak_s))`

### wallet_id()

```rust
pub fn wallet_id(&self) -> WalletId {
    // FVK'nın benzersiz tanımlayıcısı
    let hash = poseidon377::hash_2("Penumbra_HashFVK", (nk.0, ak_s));
    WalletId(hash.to_bytes())
}
```

---

## 6. NullifierKey — Çift Harcama Önleme

**Dosya:** `crates/core/keys/src/keys/nullifier.rs`

```rust
pub struct NullifierKey(pub Fq);   // decaf377 base field elemanı (32 byte)
```

NullifierKey, bir note'un harcandığında benzersiz bir nullifier türetmek için kullanılır. Nullifier = `poseidon377::hash_3(nk, commitment, position)`. Aynı note iki kez harcanmaya çalışılırsa nullifier zaten mevcut olacaktır ve zincir reddeder.

### R1CS Witness (ZK devre içinde)

```rust
pub struct NullifierKeyVar {
    pub inner: FqVar,  // ZK devresinde witness olarak temsil edilir
}
```

AllocVar implementasyonu: NullifierKey sadece `AllocationMode::Witness` olarak tahsis edilir (public input değil, özel bilgi).

---

## 7. IncomingViewingKey — Gelen İşlemleri Görme

**Dosya:** `crates/core/keys/src/keys/ivk.rs`

### Struct Tanımı

```rust
pub struct IncomingViewingKey {
    ivk: ka::Secret,      // ECDH secret key (Fr)
    dk: DiversifierKey,   // AES-128 key
}
```

### payment_address(index) — Adres Üretimi

```rust
pub fn payment_address(&self, index: AddressIndex) -> (Address, DetectionKey) {
    // 1. Diversifier: AES-128 encrypt(index)
    let d = self.dk.diversifier_for_index(&index);

    // 2. Diversified generator: blake2b → decaf377 point
    let g_d = d.diversified_generator();

    // 3. Transmission key: ivk * g_d
    let pk_d = self.ivk.diversified_public(&g_d);

    // 4. FMD detection key ve clue key
    let dtk_d = fmd::DetectionKey::from_field(
        prf::expand_ff(b"Penumbra_Fmd_Sk", nk_bytes, d_bytes)
    );
    let ck_d = dtk_d.clue_key();

    let address = Address { d, g_d, pk_d, ck_d };
    (address, dtk_d)
}
```

### views_address(address) — Adres Sahipliği Doğrulama

```rust
pub fn views_address(&self, address: &Address) -> bool {
    // ivk * address.g_d == address.pk_d
    self.ivk.diversified_public(address.diversified_generator())
        == *address.transmission_key()
}
```

Bu, merkezi bir sunucu gerektirmeden hangi adresler için işlem görüntüleneceğini tespit eder.

### ZK Devresinde IVK Türetimi

```rust
impl IncomingViewingKeyVar {
    pub fn derive(nk_var: &NullifierKeyVar, ak_var: &ElementVar) -> Result<Self, SynthesisError> {
        // Poseidon hash ZK içinde
        let ivk_mod_q = poseidon377::r1cs::hash_2(cs, &domain_sep, (nk, ak_s))?;

        // ivk_mod_q'yu mod r'ye indirge (R1CS constraint'leri ile)
        // a*(a-1)*(a-2)*(a-3)*(a-4) = 0 şeklinde "a ∈ {0,1,2,3,4}" kontrol
        // 5 constraint gerekiyor
        let ivk_mod_r = mod_r_reduce(ivk_mod_q)?;
        ...
    }
}
```

---

## 8. Diversifier — Sonsuz Adres Çeşitlendirme

**Dosya:** `crates/core/keys/src/keys/diversifier.rs`

### Struct'lar

```rust
// 16 byte gizli adres bileşeni
pub struct Diversifier(pub [u8; 16]);

// AES-128 key (DK = Diversifier Key)
pub struct DiversifierKey(pub(super) [u8; 16]);

// Adres indeksi: hesap numarası + rastgele veri
pub struct AddressIndex {
    pub account: u32,      // 4 byte: hesap numarası
    pub randomizer: [u8; 12],  // 12 byte: efemeral adresler için
}
```

### Diversifier Üretimi

```rust
pub fn diversifier_for_index(&self, index: &AddressIndex) -> Diversifier {
    // AES-128 ECB encryption
    let key = GenericArray::from(self.0);
    let plaintext = GenericArray::from(index.to_bytes()); // 16 byte

    let cipher = Aes128::new(&key);
    cipher.encrypt_block(&mut block);

    Diversifier(block.into())
}
```

**Özellik:** AES-128 PRF kullanıldığından:
- Her `AddressIndex` değeri için deterministik ve benzersiz bir `Diversifier` üretilir
- `DiversifierKey` olmadan `AddressIndex`'i tersine çevirmek mümkün değil
- Tek bir `SpendKey`'den 2^32 * 2^96 = 2^128 farklı adres türetilebilir

### Diversified Generator

```rust
impl Diversifier {
    pub fn diversified_generator(&self) -> decaf377::Element {
        // blake2b("Penumbra_Divrsfy" || diversifier_bytes) → Fq
        let hash = blake2b_simd::Params::new()
            .personal(b"Penumbra_Divrsfy")
            .hash(&self.0);

        // Fq elemanını curve noktasına encode et
        decaf377::Element::encode_to_curve(&Fq::from_le_bytes_mod_order(hash.as_bytes()))
    }
}
```

Her farklı diversifier için farklı bir curve basepoint üretilir. Bu, farklı adresler arasındaki bağlantıyı kırar.

---

## 9. Adres Yapısı

**Dosya:** `crates/core/keys/src/address.rs`

### Struct Tanımı

```rust
pub struct Address {
    d: Diversifier,                   // 16 byte: AES-şifreli adres indeksi
    g_d: OnceLock<Element>,          // 32 byte (lazy computed): çeşitlendirilmiş basepoint
    pk_d: ka::Public,                 // 32 byte: iletim anahtarı (ivk * g_d)
    transmission_key_s: Fq,          // 32 byte: pk_d'nin Fq projeksiyonu
    ck_d: fmd::ClueKey,              // 32 byte: FMD detection key
}
// Toplam: 80 byte (kodlanmamış)
```

### Serializasyon

- **Kodlama:** f4jumble (32 byte karma permütasyonu) + Bech32m
- **Prefix:** `penumbra1...` (ana ağ), `penumbratest1...` (test ağı)
- **Şeffaf adres:** `tpenumbra1...` (diversifier = all-zero, clue key = zero)

---

## 10. Fuzzy Message Detection (FMD)

**Dosya:** `crates/crypto/decaf377-fmd/src/`

### Amaç

Alıcı, hangi blok zinciri işlemlerinin kendisine ait olduğunu gizli bir anahtar kullanarak tespit edebilir. Üçüncü taraflara (dedektörler) tespit izni verilebilir; bu dedektörler belirli bir hata oranıyla filtreleme yapar, ancak tam olarak hangi işlemlerin o kişiye ait olduğunu bilemezler.

### Bileşenler

```
DetectionKey (dtk ∈ Fr)
    │
    │  dtk * G
    ▼
ClueKey ([u8; 32])  =  decaf377 Encoding
    │
    │  Alıcı adresine dahil edilir (ck_d)
    ▼
Clue ([u8; 68])
    │  P (32b) + y (32b) + precision (1b) + ciphertexts (3b)
```

### Clue Oluşturma (Gönderen)

**Dosya:** `clue_key.rs` — `create_clue_deterministic()`

```rust
// Rastgele r, z ∈ Fr
let r = Fr::from_le_bytes_mod_order(blake2b("decaf377-fmd.rdv" || ck || rseed));
let z = Fr::from_le_bytes_mod_order(blake2b("decaf377-fmd.zdv" || ck || rseed));

let P = r * G;    // G: generator
let Q = z * G;

// Her precision biti için şifrele
for i in 0..precision_bits {
    let X_i = hkd::derive_public(&root_pub, &root_pub_enc, i); // Alt anahtar
    let rX_i = (r * X_i).compress();
    let key_i = hash::to_bit(P, rX_i, Q);  // 1-bit key
    let ctxt_i = key_i ^ 1;                 // Şifreli bit (plaintext=1, yani "için")
    ctxts.set(i, ctxt_i);
}

// Sahtelik kanıtı (dummy detections önlemek için)
let m = hash::to_scalar(P, precision_bits, ctxts);
let y = (z - m) * r.inverse();

// Clue = P (32) || y (32) || precision (1) || ctxts (3)
```

### Clue Doğrulama (Dedektör)

**Dosya:** `detection.rs` — `DetectionKey::examine()`

```rust
pub fn examine(&self, clue: &Clue) -> bool {
    // P ve y'yi parse et
    let P = decompress(clue[0..32]);
    let y = Fr::from_bytes(clue[32..64]);

    // Sıfır kontrolü (güvenlik)
    if P.is_identity() || y == Fr::ZERO { return false; }

    // Q'yu yeniden türet
    let m = hash::to_scalar(P, precision_bits, ctxts);
    let Q_bytes = ((y * P) + (m * G)).compress();

    // Her bit için kontrol et
    for i in 0..precision_bits {
        let Px_i = (P * self.xs[i]).compress(); // xs[i]: türetilmiş alt anahtar
        let key_i = hash::to_bit(P, Px_i, Q_bytes);
        let msg_i = ctxts[i] ^ key_i;

        if msg_i == 0 { return false; } // Erken çıkış
    }
    true // Tüm bitler 1 → olası eşleşme
}
```

**Özellik:** False positive oranı `2^{-precision}`. Örneğin precision=4 → %6.25 yanlış pozitif.

### Hiyerarşik Anahtar Türetimi (HKD)

**Dosya:** `hkd.rs`

Alt anahtarlar (`X_i`) root public key'den BLAKE2b ile türetilir, özel anahtar gerektirmez. Bu sayede dedektöre sadece `DetectionKey` verilir, `SpendKey` verilmez.

---

## 11. Key Agreement (decaf377-ka)

**Dosya:** `crates/crypto/decaf377-ka/src/lib.rs`

### Struct'lar

```rust
pub struct Public(pub [u8; 32]);    // Sıkıştırılmış decaf377 noktası
pub struct Secret(decaf377::Fr);   // Scalar değer
pub struct SharedSecret(pub [u8; 32]); // ECDH sonucu
```

### Temel Operasyonlar

```rust
impl Secret {
    // public = secret * G
    pub fn public(&self) -> Public {
        Public((self.0 * Element::GENERATOR).vartime_compress().into())
    }

    // Çeşitlendirilmiş public key: secret * diversified_generator
    pub fn diversified_public(&self, g_d: &Element) -> Public {
        Public((self.0 * g_d).vartime_compress().into())
    }

    // ECDH: shared_secret = secret * peer_public
    pub fn key_agreement_with(&self, other: &Public) -> Result<SharedSecret> {
        let pk = Encoding(other.0).vartime_decompress()?;
        Ok(SharedSecret((self.0 * pk).vartime_compress().into()))
    }
}
```

**Not:** Çeşitlendirilmiş public key sayesinde tek bir `ivk`'dan farklı diversified_generator'larla farklı (bağlantısız) public key'ler türetilebilir. Bu, Note şifrelemesinde kullanılır.

---

## 12. Note Modeli — Gizli UTXO

**Dosya:** `crates/core/component/shielded-pool/src/note.rs`

### Penumbra Note Nedir?

Zcash'in shielded note'una benzer, Penumbra'da UTXO'nun karşılığıdır. Her gizli işlem mevcut bir note'u harcar (SpendProof ile) ve yeni note'lar oluşturur (OutputProof ile).

### Struct Tanımı

```rust
pub struct Note {
    value: Value,                  // {amount: u128, asset_id: Fq}
    rseed: Rseed,                  // [u8; 32] — rastgele tohum
    address: Address,             // 80 byte — alıcı adresi
    transmission_key_s: Fq,       // address.pk_d'nin Fq projeksiyonu
}
```

### Boyutlar

| Bileşen | Boyut |
|---------|-------|
| address | 80 byte |
| amount | 16 byte (u128) |
| asset_id | 32 byte |
| rseed | 32 byte |
| **Toplam plaintext** | **160 byte** |
| ChaCha20Poly1305 overhead | +16 byte (auth tag) |
| **Toplam ciphertext** | **176 byte** |

### Note Commitment

```rust
pub fn commit(&self) -> StateCommitment {
    let blinding = self.rseed.derive_note_blinding(); // poseidon377 türetimi

    StateCommitment(poseidon377::hash_6(
        &NOTECOMMIT_DOMAIN_SEP,
        (
            blinding,           // gizlilik için blinding factor
            self.value.amount,  // miktar
            self.value.asset_id, // varlık tipi
            g_d_compressed,     // diversified_generator (Fq)
            pk_d_s,            // transmission_key_s
            ck_d_fq,           // clue_key (Fq'ya dönüştürülmüş)
        )
    ))
}
```

**Matematiksel özellikler:**
- Commitment gizlidir (rseed bilinmeden tersine çevrilmez)
- Bağlayıcıdır (farklı note içeriği farklı commitment üretir)
- ZK devresinde doğrulanabilir (poseidon377 R1CS implementasyonuna sahip)

---

## 13. Note Şifreleme ve Şifre Çözme

**Dosya:** `note.rs` — `encrypt()`, `decrypt()`, `decrypt_outgoing()`

### Şifreleme (Gönderen)

```rust
pub fn encrypt(&self) -> (NotePayload, PayloadKey) {
    // 1. Efemeral anahtar üret
    let esk = self.rseed.derive_esk();           // Fr scalar
    let epk = esk.diversified_public(&g_d);      // Efemeral public key

    // 2. ECDH: shared_secret = esk * pk_d
    let shared_secret = esk.key_agreement_with(&self.address.transmission_key())?;

    // 3. Payload key türet
    let payload_key = PayloadKey::derive(&shared_secret, &epk);

    // 4. ChaCha20Poly1305 ile şifrele
    let ciphertext = payload_key.encrypt(note_plaintext_bytes, OvkWrappedKey::NOTE);

    (NotePayload { note_commitment, ephemeral_key: epk, encrypted_note: ciphertext }, payload_key)
}
```

### Şifre Çözme (Alıcı — IVK ile)

```rust
pub fn decrypt(ciphertext: &[u8], ivk: &IncomingViewingKey, epk: &ka::Public) -> Result<Note> {
    // 1. ECDH: shared_secret = ivk * epk
    let shared_secret = ivk.key_agreement_with(epk)?;

    // 2. Payload key türet
    let payload_key = PayloadKey::derive(&shared_secret, epk);

    // 3. Şifreyi çöz
    let plaintext = payload_key.decrypt(ciphertext)?;
    let note = Note::try_from(plaintext)?;

    // 4. ZIP-212: Efemeral public key bütünlüğü kontrolü
    let esk = note.rseed.derive_esk();
    let expected_epk = esk.diversified_public(&note.diversified_generator());
    if expected_epk != *epk {
        anyhow::bail!("ephemeral public key mismatch");
    }

    Ok(note)
}
```

**ZIP-212 Önemi:** Bu kontrol, epk'nin rseed'den deterministik olarak türetildiğini doğrular. Böylece bir saldırgan note içeriğini manipüle edemez.

### Şifre Çözme (Gönderen — OVK ile)

```rust
pub fn decrypt_outgoing(ciphertext, wrapped_ovk, note_commitment, epk, cv, ovk) -> Result<Note> {
    // OVK ile sarılmış anahtar çözülür
    let key_bytes = prf::expand("Penumbra_OutVNF", ovk.0, cv || note_commitment || epk);
    // Bu key ile encrypted_note çözülür
}
```

---

## 14. Tiered Commitment Tree (TCT)

**Dosya:** `crates/crypto/tct/src/`

### Mimari

```
Eternity Tree (Anchor = Global Root)
    │  (8 seviye, 4^8 = 65536 epoch kapasitesi)
    │
    └── Epoch Root  [her epoch bir yaprak]
            │  (8 seviye, 65536 blok kapasitesi)
            │
            └── Block Root  [her blok bir yaprak]
                    │  (8 seviye, 65536 commitment kapasitesi)
                    │
                    └── Note Commitment  [yaprak]
```

**Toplam kapasite:** 65536 × 65536 × 65536 = 2^48 commitment

### Tree Struct

```rust
pub struct Tree {
    // StateCommitment → pozisyon indeksi (witness takibi için)
    index: HashedMap<StateCommitment, index::within::Tree>,

    // Frontier tabanlı iç temsil
    inner: Arc<frontier::Top<frontier::Tier<frontier::Tier<frontier::Item>>>>,
}
```

### Hash Fonksiyonu

```rust
// Yaprak hash'i
Hash::of(commitment) = hash_1(&DOMAIN_SEPARATOR, commitment.0)

// İç düğüm hash'i (4 çocuk)
Hash::node(height, a, b, c, d) = hash_4(
    &(*DOMAIN_SEPARATOR + height),  // domain separation
    (a.0, b.0, c.0, d.0)
)
// Burada DOMAIN_SEPARATOR = blake2b("penumbra.tct")
```

**Poseidon hash kullanımı:** 4-ary tree, her iç düğüm 4 çocuk hash'i alır. `hash_4` ile Poseidon377 uygulanır.

**Optimizasyon:** Boş blok serileri için `PRECOMPUTED_HASH_PAIRS` statik array'i (heights 9-16 için önceden hesaplanmış hash'ler).

### Pozisyon Yapısı

```rust
pub struct Position(index::within::Tree);
// 48 bit: [epoch:16][block:16][commitment:16]
// u64'e cast edilebilir
```

### Witness ve Proof

```rust
// Commitment ekleme
tree.insert(Witness::Keep, commitment)?;  // Witness tutulur
tree.insert(Witness::Forget, commitment)?; // Sadece hash

// Proof üretme
tree.witness(commitment) -> Option<Proof>

pub struct Proof {
    position: index::within::Tree,
    auth_path: Node { ... },  // 24 seviye, her seviye 3 kardeş hash
    leaf: StateCommitment,
}
// auth_path: [[Hash; 3]; 24]  (24 seviye × 3 kardeş)
```

### ZK Devresinde Auth Path Doğrulaması

**Dosya:** `crates/crypto/tct/src/r1cs.rs`

```rust
pub struct MerkleAuthPathVar {
    inner: [[FqVar; 3]; 24],  // 24 seviye, her seviye 3 FqVar kardeş
}

impl MerkleAuthPathVar {
    pub fn verify(
        &self,
        cs: ConstraintSystemRef<Fq>,
        enforce: &Boolean<Fq>,       // Dummy spend için koşullu zorlama
        position_bits: &[Boolean<Fq>], // 48 bit konum
        anchor_var: FqVar,            // Public: Merkle root
        commitment_var: FqVar,        // Witness: note commitment
    ) -> Result<(), SynthesisError> {
        // Yaprak hash'i: poseidon hash_1
        let leaf_var = poseidon377::r1cs::hash_1(cs, &domain_sep, commitment_var)?;
        let mut prev = leaf_var;

        // Height 1'den 24'e kadar hash'le
        for height in 1..=24 {
            let height_var = FqVar::constant(Fq::from(height as u64));

            // WhichWay: position'ın 2-bit dilimini al
            let which_way = WhichWayVar::at(height, position_bits)?;
            let siblings = &self.inner[(24 - height) as usize];

            // Node'u doğru pozisyona yerleştir
            let [a, b, c, d] = which_way.insert(prev, siblings)?;

            // poseidon hash_4
            prev = hash_node(cs, height_var, a, b, c, d)?;
        }

        // Root'u doğrula (koşullu: dummy spend'de enforce=false)
        anchor_var.conditional_enforce_equal(&prev, enforce)?;
        Ok(())
    }
}
```

**WhichWayVar:** Pozisyonun her 2 bitlik dilimi (0-3), çocuk pozisyonunu belirler. `inner = bit_1 + 2 * bit_2` şeklinde constraint.

---

## 15. Nullifier Türetimi

**Dosya:** `crates/core/component/sct/src/nullifier.rs`

### Struct

```rust
pub struct Nullifier(pub Fq);   // decaf377 base field elemanı
```

### Türetim Formülü

```rust
pub fn derive(nk: &NullifierKey, pos: tct::Position, state_commitment: &StateCommitment) -> Nullifier {
    Nullifier(hash_3(
        &NULLIFIER_DOMAIN_SEP,      // blake2b("penumbra.nullifier")
        (
            nk.0,                   // NullifierKey: Fq
            state_commitment.0,     // Note commitment: Fq
            Fq::from(u64::from(pos)), // Pozisyon: u64 → Fq
        )
    ))
}
```

**Matematiksel:** `nf = poseidon377::hash_3(DOMAIN_SEP, nk, cm, pos)`

**Neden 3 girdi?**
- `nk`: Anahtar sahibini bağlar (başkası bu nullifier'ı üretemez)
- `cm`: Note'u bağlar (farklı note'lar farklı nullifier)
- `pos`: TCT pozisyonunu bağlar (aynı içerik farklı yerlere eklenirse bile nullifier farklı)

### ZK Devresinde Nullifier Türetimi

```rust
impl NullifierVar {
    pub fn derive(nk, position, state_commitment) -> Result<NullifierVar> {
        let domain_sep = FqVar::constant(*NULLIFIER_DOMAIN_SEP);
        let nullifier = poseidon377::r1cs::hash_3(
            cs,
            &domain_sep,
            (nk.inner, state_commitment.inner, position.position)
        )?;
        Ok(NullifierVar { inner: nullifier })
    }
}
```

---

## 16. SpendProof — Groth16 ZK Kanıtı

**Dosya:** `crates/core/component/shielded-pool/src/spend/proof.rs`

### Genel Bakış

SpendProof, bir note'un harcanabildiğini ve harcama yetkisinin bulunduğunu, gizli bilgileri açıklamadan kanıtlar. Groth16 protokolü, Bls12_377 eğrisi üzerinde kullanılır.

### Public ve Private Girdiler

```rust
pub struct SpendProofPublic {
    pub anchor: tct::Root,                    // TCT'nin güncel Merkle root'u
    pub balance_commitment: balance::Commitment, // Harcanan değerin gizli commitment'ı
    pub nullifier: Nullifier,                 // Çift harcamayı önlemek için nullifier
    pub rk: VerificationKey<SpendAuth>,       // Randomize edilmiş doğrulama anahtarı
}

pub struct SpendProofPrivate {
    pub state_commitment_proof: tct::Proof,   // Merkle yolu kanıtı
    pub note: Note,                           // Harcanacak note
    pub v_blinding: Fr,                       // Value commitment blinding
    pub spend_auth_randomizer: Fr,            // imza randomizer
    pub ak: VerificationKey<SpendAuth>,       // Harcama doğrulama anahtarı
    pub nk: NullifierKey,                     // Nullifier key
}
```

### Circuit Constraints (6 Grup)

```rust
impl ConstraintSynthesizer<Fq> for SpendCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fq>) -> Result<()> {
        // WITNESSES (gizli girdiler):
        let note_var = NoteVar::new_witness(cs, note)?;
        let claimed_note_commitment = StateCommitmentVar::new_input(cs, cm)?;
        let merkle_path_var = MerkleAuthPathVar::new_witness(cs, proof)?;
        let position_var = PositionVar::new_witness(cs, position)?;
        let nk_var = NullifierKeyVar::new_witness(cs, nk)?;
        let ak_element_var = ElementVar::new_witness(cs, ak)?;
        let v_blinding_vars = UInt8::new_witness_vec(cs, v_blinding)?;
        let spend_auth_randomizer_var = Fr::new_witness(cs, randomizer)?;

        // PUBLIC INPUTS:
        let anchor_var = FqVar::new_input(cs, anchor)?;
        let claimed_balance_commitment_var = BalanceCommitmentVar::new_input(cs, cv)?;
        let claimed_nullifier_var = NullifierVar::new_input(cs, nf)?;
        let rk_var = ElementVar::new_input(cs, rk)?;

        // CONSTRAINT 1: Note commitment integrity
        // note_var.commit() == claimed_note_commitment
        let commitment = note_var.commit()?;
        commitment.enforce_equal(&claimed_note_commitment)?;

        // CONSTRAINT 2: Nullifier integrity
        // poseidon3(nk, cm, pos) == claimed_nullifier
        let computed_nullifier = NullifierVar::derive(&nk_var, &position_var, &commitment_var)?;
        computed_nullifier.enforce_equal(&claimed_nullifier_var)?;

        // CONSTRAINT 3: Merkle auth path verification
        // TCT'de cm'nin merkle yolu anchor'a ulaşmalı
        // Dummy spend (amount=0) için atlanır
        let is_not_dummy = note_var.amount().is_eq(&FqVar::zero())?.not();
        merkle_path_var.verify(cs, &is_not_dummy, &position_bits, anchor_var, commitment)?;

        // CONSTRAINT 4: Randomized verification key integrity
        // ak.randomize(randomizer) == rk
        let computed_rk = ak_element_var.randomize(&spend_auth_randomizer_var)?;
        computed_rk.enforce_equal(&rk_var)?;

        // CONSTRAINT 5: Diversified address integrity
        // ivk.diversified_public(g_d) == pk_d
        let ivk = IncomingViewingKeyVar::derive(&nk_var, &ak_element_var)?;
        let computed_pk_d = ivk.diversified_public(&note_var.diversified_generator())?;
        computed_pk_d.enforce_equal(&note_var.transmission_key())?;

        // CONSTRAINT 6: Balance commitment integrity
        // value.commit(v_blinding) == balance_commitment
        let balance_commitment = note_var.value().commit(v_blinding_vars)?;
        balance_commitment.enforce_equal(&claimed_balance_commitment_var)?;

        Ok(())
    }
}
```

### Prove ve Verify

```rust
impl SpendProof {
    pub fn prove(blinding_r: Fq, blinding_s: Fq, pk: &ProvingKey<Bls12_377>, public, private) -> Result<Self> {
        let circuit = SpendCircuit { public, private };
        let proof = Groth16::<Bls12_377, LibsnarkReduction>::create_proof_with_reduction(
            circuit, pk, blinding_r, blinding_s,
        )?;
        // 192 byte proof (3 Bls12_377 affine noktası)
        let mut proof_bytes = [0u8; GROTH16_PROOF_LENGTH_BYTES];
        Proof::serialize_compressed(&proof, &mut proof_bytes[..])?;
        Ok(Self(proof_bytes))
    }

    pub fn verify(&self, vk: &PreparedVerifyingKey<Bls12_377>, public: SpendProofPublic) -> Result<()> {
        let proof = Proof::deserialize_compressed_unchecked(&self.0)?;

        // Public inputs flatten: anchor || balance_commitment || nullifier || rk
        let public_inputs = [anchor, balance_commitment, nullifier, element_rk]
            .into_iter()
            .flat_map(|x| x.to_field_elements())
            .collect::<Vec<_>>();

        Groth16::<Bls12_377, LibsnarkReduction>::verify_with_processed_vk(
            vk, &public_inputs, &proof
        )?
        .then_some(())
        .ok_or(VerificationError::InvalidProof)
    }
}
```

**Proof boyutu:** `GROTH16_PROOF_LENGTH_BYTES` — Bls12_377 üzerinde yaklaşık 192 byte (3 curve noktası: A, B, C).

---

## 17. OutputProof — Çıktı ZK Kanıtı

**Dosya:** `crates/core/component/shielded-pool/src/output/proof.rs`

### Amaç

Yeni bir note oluşturulurken, gizli bilgileri açıklamadan hem note commitment'ının doğru hesaplandığını hem de balance commitment'ının doğru olduğunu kanıtlar.

### Circuit

```rust
pub struct OutputCircuit {
    public: OutputProofPublic,   // balance_commitment, note_commitment
    private: OutputProofPrivate, // note, balance_blinding
}

impl ConstraintSynthesizer<Fq> for OutputCircuit {
    fn generate_constraints(self, cs) -> Result<()> {
        // Witnesses
        let note_var = NoteVar::new_witness(cs, note)?;
        let balance_blinding_vars = UInt8::new_witness_vec(cs, balance_blinding)?;

        // Public inputs
        let claimed_note_commitment = StateCommitmentVar::new_input(cs, note_commitment)?;
        let claimed_balance_commitment = BalanceCommitmentVar::new_input(cs, balance_commitment)?;

        // CONSTRAINT 1: Balance commitment integrity
        // (-note.value).commit(balance_blinding) == balance_commitment
        let balance_commitment = BalanceVar::from_negative_value_var(note_var.value())
            .commit(balance_blinding_vars)?;
        balance_commitment.enforce_equal(&claimed_balance_commitment)?;

        // CONSTRAINT 2: Note commitment integrity
        // note.commit() == note_commitment
        let note_commitment = note_var.commit()?;
        note_commitment.enforce_equal(&claimed_note_commitment)?;

        Ok(())
    }
}
```

**Not:** OutputCircuit'te address integrity constraint yok — `NoteVar::new_witness(...)` içinde `diversified_generator != identity` kontrolü yapılır.

---

## 18. Staking Sistemi

**Dosya:** `crates/core/component/stake/src/`

### Validator Struct

```rust
pub struct Validator {
    pub identity_key: IdentityKey,        // Ed25519/Decaf377 doğrulama anahtarı
    pub governance_key: GovernanceKey,    // Yönetişim oy anahtarı
    pub consensus_key: tendermint::PublicKey, // Tendermint consensus key
    pub name: String,                     // max 140 karakter
    pub website: String,                  // max 70 karakter
    pub description: String,              // max 280 karakter
    pub enabled: bool,                    // aktif/pasif
    pub funding_streams: FundingStreams,  // komisyon dağıtımı
    pub sequence_number: u32,             // replay attack önleme
}
```

### Validator State Machine (6 Durum)

```
Defined → Inactive → Active → Jailed → (reinstate) → Active
                              ↓
                           Tombstoned (kalıcı yasak)
Active ↔ Disabled (operatör kontrolü)
```

| Durum | Açıklama |
|-------|----------|
| `Defined` | Tanımlı ama min stake altında |
| `Inactive` | Min stake üstünde ama consensus dışında |
| `Active` | Aktif consensus katılımcısı |
| `Jailed` | Downtime slashing sonrası |
| `Tombstoned` | Byzantine slashing → kalıcı ban |
| `Disabled` | Operatör tarafından devre dışı |

### Delegation Token

Her validator için benzersiz bir `DelegationToken` oluşturulur:
- Token = `udelegation_{identity_key_bech32}`
- Kullanıcılar staking yaparken bu token'ı alır
- NSC değeriyle değişim oranı `RateData` ile hesaplanır

### RateData — Ödül ve Exchange Rate

```rust
pub struct RateData {
    pub identity_key: IdentityKey,
    pub validator_reward_rate: Amount,    // BPS²_SCALING_FACTOR ölçeklenmiş
    pub validator_exchange_rate: Amount,  // NSC/DelegationToken oranı
}
```

### Epoch Geçiş Hesabı (`next_epoch()`)

```rust
pub fn next_epoch(&self, next_base_rate: &BaseRateData, funding_streams: &[FundingStream], state: &State) -> RateData {
    if let State::Active = state {
        let validator_commission = sum_of_funding_streams / 10000;

        // Validator reward rate
        let commission_factor = 1 - validator_commission;
        let next_validator_reward_rate = next_base_reward_rate * commission_factor;

        // Validator exchange rate (compound)
        let reward_growth_factor = 1 + next_validator_reward_rate;
        let next_validator_exchange_rate = prev_exchange_rate * reward_growth_factor;

        // BPS² scaling factor ile rescale
        RateData { ..scaled values.. }
    } else {
        // Aktif olmayan validator'lar için sabit rate
        self.clone()
    }
}
```

**Matematiksel:** Ödüller bileşik (compound) büyür. Exchange rate her epoch güncellenir.

### Unbonding Süreci

1. Kullanıcı `Undelegate` işlemi gönderir
2. `DelegationToken` yakılır, `UnbondingToken` verilir
3. Unbonding süresi (epoch sayısı) geçer
4. `UndelegateClaimProof` (Groth16) ile NSC geri alınır

---

## 19. DEX: Zswap ve Konsantre Likidite

**Dosya:** `crates/core/component/dex/src/`

### Genel Mimari

Penumbra'nın DEX'i iki katmandan oluşur:
1. **Batch Swap:** Kullanıcılar swap'larını şifreli olarak gönderir, blok sonunda toplu işlenir
2. **Konsantre Likidite Pozisyonları:** Uniswap v3 benzeri, belirli fiyat aralıklarında likidite

### TradingFunction — CFMM Formülü

```rust
pub struct BareTradingFunction {
    pub fee: u32,   // Basis points (0-9999)
    pub p: Amount,  // Asset 1 değerlemesi
    pub q: Amount,  // Asset 2 değerlemesi
}
```

**CFMM Denklemi:** `φ(R) = p·R₁ + q·R₂ = sabit`

Bu, bir **constant-sum** market maker'dır (sabit fiyat). `p/q` oranı, asset1/asset2 fiyatını verir.

**Fee hesabı:**
```
gamma = (10000 - fee) / 10000
effective_price = p/q * (1/gamma)  [fee dahil]
```

### fill() — Trade Gerçekleştirme

```rust
pub fn fill(&self, input: Value, reserves: &Reserves) -> Result<(Value, Reserves, Value)> {
    // Asset 1 → Asset 2 swap:
    // output = gamma * (q/p) * input   (fee düşülerek)
    // new_r1 = r1 + input
    // new_r2 = r2 - output

    // Asset 2 → Asset 1: flip() ile aynı işlem
}
```

### Position Struct

```rust
pub struct Position {
    pub trading_function: TradingFunction,  // Fiyat ve fee
    pub reserves: Reserves,                  // {r1: Amount, r2: Amount}
    pub close_on_fill: bool,                 // Doldurulunca kapat
    pub phi: TradingFunction,               // Normalize edilmiş ticaret fonksiyonu
}
```

### fill_route() — Çoklu Hop Routing

**Dosya:** `component/router/fill_route.rs`

```rust
pub async fn fill_route(
    state: &mut S,
    input: Value,
    hops: &[asset::Id],          // Rota: [A, B, C] → A→B→C swap
    spill_price: Option<U128x128>, // Fiyat limiti
) -> Result<SwapExecution, FillError> {
    // StateDelta ile atomik işlem
    let mut this = StateDelta::new(state);

    // Her hop için:
    // 1. En iyi fiyatlı pozisyonu bul (price_index)
    // 2. fill() ile pozisyonu doldur
    // 3. Pozisyonu güncelle/kapat
    // 4. Çıktıyı bir sonraki hop'a gönder
}
```

**Özellik:** `ensure_progress=true` ile spill_price'a rağmen en az 1 pozisyon tüketilir (sonsuz döngü önleme).

---

## 20. Batch Swap Mekanizması

**Dosya:** `component/dex.rs`, `swap/plaintext.rs`, `batch_swap_output_data.rs`

### SwapPlaintext

```rust
pub struct SwapPlaintext {
    pub trading_pair: TradingPair,    // Hangi çift
    pub delta_1_i: Amount,           // Kullanıcı input 1
    pub delta_2_i: Amount,           // Kullanıcı input 2
    pub claim_fee: Fee,              // SwapClaim için ön ödenen fee
    pub claim_address: Address,      // Çıktıların gönderileceği adres
    pub rseed: Rseed,               // Çıktı note'ları için rastgelelik kaynağı
}
```

### Swap Commitment

```rust
pub fn swap_commitment(&self) -> StateCommitment {
    hash_7(
        &DOMAIN_SEPARATOR,
        (trading_pair, delta_1_i, delta_2_i, claim_fee_amount, claim_fee_asset, claim_address, rseed)
    )
}
```

### BatchSwapOutputData — Pro-Rata Hesabı

```rust
pub struct BatchSwapOutputData {
    pub delta_1: Amount,    // Toplam asset 1 girdi
    pub delta_2: Amount,    // Toplam asset 2 girdi
    pub lambda_1: Amount,   // Toplam asset 1 çıktı (2→1 swaplar)
    pub lambda_2: Amount,   // Toplam asset 2 çıktı (1→2 swaplar)
    pub unfilled_1: Amount, // Gerçekleşmemiş asset 1
    pub unfilled_2: Amount, // Gerçekleşmemiş asset 2
}

pub fn pro_rata_outputs(&self, (delta_1_i, delta_2_i): (Amount, Amount)) -> (Amount, Amount) {
    // Kullanıcının toplam içindeki payı
    let pro_rata_1 = delta_1_i / self.delta_1;
    let pro_rata_2 = delta_2_i / self.delta_2;

    // Pro-rata çıktılar
    let lambda_2_i = pro_rata_1 * self.lambda_2 + pro_rata_2 * self.unfilled_2;
    let lambda_1_i = pro_rata_1 * self.unfilled_1 + pro_rata_2 * self.lambda_1;
}
```

### DEX end_block() İşlem Akışı

**Dosya:** `component/dex.rs` — `Dex::end_block()`

```
F.0. Alternatif fee token'ları swap flow'a ekle
F.1. Tüm trading pair'lar için akışları hesapla
F.2. Her pair için route_and_fill() çalıştır
F.3. BatchSwapOutputData'yı kaydet
F.4. Arb: Kalan likiditeyle arbitraj
F.5. Eviction: Düşük performanslı pozisyonları çıkar
F.6. Chandelier: Mum grafik verilerini güncelle
```

### Swap Akışı (Kullanıcı Perspektifi)

1. Kullanıcı `SwapPlaintext` oluşturur ve şifreler (`SwapCiphertext`)
2. `Swap` işlemi zincire gönderilir (içerik gizli)
3. Blok kapanınca validator tüm swap'ları toplu işler
4. `BatchSwapOutputData` zincire yazılır
5. Kullanıcı `SwapClaim` işlemi ile çıktılarını alır (ZK kanıt ile)

---

## 21. Güvenlik Özellikleri ve Notlar

### Kriptografik Güvenlik

| Özellik | Implementasyon | Güvenlik Seviyesi |
|---------|---------------|-------------------|
| İmza | Randomizable Ed25519 (rdsa) | 128-bit |
| Commitment | poseidon377 (ZK-dostu) | 128-bit |
| Nullifier | poseidon377 hash_3 | 128-bit |
| Şifreleme | ChaCha20Poly1305 | 256-bit key |
| ZK-SNARK | Groth16 / Bls12_377 | 128-bit |
| KDF | PBKDF2-HMAC-SHA512 (2048 round) | BIP39 standart |
| Hash | BLAKE2b + poseidon377 | — |

### ZIP-212 Efemeral Key Bütünlüğü

Her note şifre çözümünde `esk = rseed.derive_esk()` ve `epk = esk * g_d` kontrol edilir. Bu, bir saldırganın sahte note içeriği enjekte edemeyeceğini garanti eder.

### Dummy Spend Desteği

`amount = 0` olan note'lar harcandığında Merkle path doğrulaması atlanır. Bu, transaction graph analizi saldırılarına karşı trafik analizi koruması sağlar.

### FMD Güvenliği

- `P = 0` veya `y = 0` clue'lar reddedilir (evrensel eşleşme açığını kapatır)
- Constant-time implementasyon: Uzun vadeli anahtar materyali (`xs[i]`) için timing channel yok
- Kısa devre: Ephemeral bit'e (`key_i`) bağlı olduğundan timing leak'i minimal

### Groth16 Güvenlik Notu

Groth16 provably secure ama **trusted setup** gerektirir. Penumbra bu setup'ı açık bir ceremony ile yapmıştır. Eğer ceremony kirlenirse gizlilik bozulabilir (ses birleşimi yeterli — en az bir katılımcı dürüst olursa güvenli).

---

## 22. Nova Shield için Çıkarımlar

### 1. MPC Hesap Modeli ↔ Spend Key Hiyerarşisi

Penumbra'nın `SpendKey → FVK → IVK → Address` hiyerarşisi, Nova Shield'ın MPC hesap modeliyle birleştirilebilir:

```
Nova Shield Fikri:
MPC_SpendKey (3/5 threshold) → FVK → IVK → Address
                   ↑
          Kimsenin tek başına erişimi yok
```

`SpendKey`'in kendisi guardian'lar arasında paylaşılır. `FVK`, `IVK`, `Address` hiyerarşisi aynen kullanılabilir çünkü bunlar public türetimdir.

### 2. Diversifier Mekanizması — Doğrudan Alınabilir

AES-128 tabanlı `DiversifierKey → Diversifier → Address` mekanizması Nova Shield için mükemmel:
- Tek MPC hesaptan sonsuz adres türetimi
- Adresler arasında bağlantı kurulamaz
- `DiversifierKey` özel, `Address` public

**Rust implementasyonu:** Direkt alınabilir (`aes` crate kullanılıyor, Rust ekosisteminde mevcut).

### 3. Fuzzy Message Detection — Opsiyonel Hafif Gizlilik

FMD, Nova Shield'e şu kullanım senaryosu için eklenebilir:
- Guardian'lara "bu hesabın işlemlerini izle" yetkisi verilir
- Guardian'lar belirli false positive oranıyla filtreleme yapar
- Ana hesap sahibi tam gizliliği korur

Precision=4 (FP rate=6.25%) guardian senaryosu için yeterli.

### 4. Note/UTXO Modeli — MEV Gizliliği İçin Referans

Penumbra'nın note modeli (160 byte, ChaCha20Poly1305, ZIP-212):
- Nova Shield'in şifreli mempool modeliyle birleştirilebilir
- Note commitment + batch auction = MEV'e karşı matematiksel koruma
- `swap_commitment` (poseidon hash_7) → şifreli batch swap

### 5. TCT — Merkle Tree Tasarımı

| Nova Shield için karar | Penumbra Yaklaşımı | Öneri |
|------------------------|-------------------|-------|
| State commitment tree | 3-tier, 4-ary, 2^48 capacity | Benzer yapı, epoch/block/slot |
| Hash fonksiyonu | poseidon377 (ZK-dostu) | poseidon2 (daha hızlı, daha yeni) |
| Auth path | [[Hash;3];24] (24 seviye) | Slot sayısına göre ayarla |
| Witness/Forget | İkili mod | Aynı pattern |

### 6. Nullifier Mekanizması — Çift Harcama Önleme

`poseidon3(nk, commitment, position)` formülü direkt alınabilir. 3 girdi kritik:
- Yalnızca anahtar sahibi nullifier üretebilir
- Aynı note farklı pozisyonda eklense bile farklı nullifier
- ZK devresinde doğrulanabilir

### 7. Groth16 + Bls12_377 Yerine Alternatif

Penumbra Groth16 kullanıyor (trusted setup gerektirir). Nova Shield için öneri:

| Sistem | Artı | Eksi |
|--------|------|------|
| Groth16 (Penumbra) | Küçük proof, hızlı doğrulama | Trusted setup |
| Plonk/UltraPlonk | Transparent setup | Daha büyük proof |
| Nova/Nexus (IVC) | Incremental doğrulama | Yeni, az test edilmiş |
| Halo2 | Transparent, production-ready | Daha yavaş prover |

**Tavsiye:** Proof-of-Stake sisteminde her işlem için ZK kanıt fazla overhead getirir. Sadece gizlilik gerektiren işlemler için (shielded transfer) ZK kullan.

### 8. DEX Batch Auction — MEV Çözümünün Referansı

Penumbra'nın `end_block()` sonunda batch swap yapması, Nova Shield'in MEV çözümüyle aynı prensibi paylaşıyor:

```
Penumbra:  Swap gönder (şifreli) → blok kapat → toplu işle → uniform rate
Nova Shield: İşlem gönder (şifreli) → slot kapat → toplu işle → uniform clear price
```

`BatchSwapOutputData.pro_rata_outputs()` formülü (pro-rata dağıtım) Nova Shield'in clearing price hesabı için referans alınabilir.

### 9. Validator State Machine — Staking Tasarımı

Penumbra'nın 6 durumlu validator state machine'i (Defined → Inactive → Active → Jailed → Tombstoned, Disabled) Nova Shield için model alınabilir. Özellikle:
- `sequence_number` replay attack önleme: doğrudan al
- `funding_streams` komisyon yapısı: validator ekonomisi için referans
- `BPS²_SCALING_FACTOR` compound rate hesabı: matematiksel doğruluk için al

### 10. Güvenlik Prensipleri

- **Domain separation:** Her hash için ayrı domain separator (`b"Penumbra_..."`) — Nova Shield'de de zorunlu
- **Zeroize:** Hassas veri (`Secret`, `SharedSecret`) drop'ta sıfırlanıyor — Nova Shield MPC key'leri için zorunlu
- **Constant-time:** FMD'de timing channel kapatılmış — threshold imza için de gerekli

---

## 23. Özet

### Ne İncelendi

| Bileşen | Dosya | Satır Sayısı |
|---------|-------|-------------|
| SpendKey türetimi | `keys/spend.rs` | 220 |
| FullViewingKey | `keys/fvk.rs` | ~300 |
| NullifierKey | `keys/nullifier.rs` | ~80 |
| IncomingViewingKey | `keys/ivk.rs` | ~300 |
| Diversifier | `keys/diversifier.rs` | 319 |
| Address | `keys/address.rs` | ~200 |
| Note modeli | `shielded-pool/note.rs` | ~400 |
| SpendProof (Groth16) | `shielded-pool/spend/proof.rs` | ~500 |
| OutputProof | `shielded-pool/output/proof.rs` | ~300 |
| TCT lib | `tct/src/lib.rs` | 152 |
| TCT tree | `tct/src/tree.rs` | ~500 |
| TCT proof | `tct/src/proof.rs` | 144 |
| TCT hash | `tct/src/internal/hash.rs` | 325 |
| TCT r1cs | `tct/src/r1cs.rs` | 335 |
| FMD lib | `decaf377-fmd/src/lib.rs` | 23 |
| FMD clue_key | `decaf377-fmd/src/clue_key.rs` | 200 |
| FMD detection | `decaf377-fmd/src/detection.rs` | 135 |
| Key Agreement | `decaf377-ka/src/lib.rs` | 161 |
| Nullifier (SCT) | `sct/src/nullifier.rs` | 167 |
| Validator | `stake/src/validator.rs` | ~100 |
| Validator state | `stake/src/validator/state.rs` | 79 |
| RateData | `stake/src/rate.rs` | ~200 |
| TradingFunction | `dex/src/lp/trading_function.rs` | ~270 |
| BatchSwapOutputData | `dex/src/batch_swap_output_data.rs` | ~100 |
| SwapPlaintext | `dex/src/swap/plaintext.rs` | ~100 |
| DEX component | `dex/src/component/dex.rs` | ~200 |
| fill_route | `dex/src/component/router/fill_route.rs` | ~200 |

### Penumbra'yı Özel Kılan Şeyler

1. **Kendi eğrisi (decaf377):** BLS12-377 scalar field üzerinde → ZK dostu, cofactor=1
2. **Kendi hash'i (poseidon377):** R1CS constraint'leri mevcut → ZK devresinde native kullanım
3. **Kendi Merkle tree'si (TCT):** 3-tier, 4-ary, 2^48 kapasite, sparse
4. **FMD:** Delegatable detection (gizlilik + seçici şeffaflık)
5. **Batch swap:** MEV'e karşı matematiksel koruma (blok sonunda uniform fiyat)
6. **Groth16 üretim kanıtları:** Her işlem tipi için ayrı circuit

### Nova Shield İçin En Kritik Çıkarımlar

1. `DiversifierKey → AES-128 → Address` mekanizması: **Doğrudan alınabilir**
2. `poseidon3(nk, cm, pos)` nullifier: **Doğrudan alınabilir**
3. Batch auction end_block mekanizması: **MEV çözümümüzün referansı**
4. Note şifreleme (ECDH + ChaCha20Poly1305): **Şifreli mempool için model**
5. Validator state machine (6 durum): **DPoS staking modeli için referans**
6. `BPS²_SCALING_FACTOR` compound rate: **Staking reward hesabı**
