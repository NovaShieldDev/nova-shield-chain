# Penumbra Agent-12: Kriptografi — decaf377-fmd (Fuzzy Message Detection) + eddy (Eşik Şifrelemesi)

**Kapsam:** `crates/crypto/decaf377-fmd/` ve `crates/crypto/eddy/`
**Toplam dosya:** 7 (fmd) + 14 (eddy) = 21 dosya
**Satır:** ~1200 satır
**Tarih:** 2026-07-22

---

## 1. decaf377-fmd — Fuzzy Message Detection (Bulanık Mesaj Tespiti)

### 1.1 Genel Mimari

FMD, kullanıcıların mesaj tarama işlemini üçüncü tarafa (detection server) devredebileceği, fakat tüm mesajlarını ifşa etmeyeceği bir gizlilik protokolüdür. Temel fikir: her mesaj için bir "ipucu" (clue) üretilir; detection server bu ipucunu kontrol ederek mesajın ilgili kişiye ait olup olmadığını _probabilistik_ olarak belirler.

**Yanlış pozitif oranı:** `2^(-precision_bits)` — yani precision_bits arttıkça tespit hassasiyeti artar, yanlış pozitif azalır.

### 1.2 Yapılar

```
lib.rs exports:
  - Clue           : 68 byte ipucu yapısı
  - ClueKey        : Kompakt (32 byte) ipucu oluşturma anahtarı
  - ExpandedClueKey: Genişletilmiş form (alt-anahtarlar önbelleğe alınmış)
  - DetectionKey   : Tespit anahtarı (gizli)
  - Precision      : u8 sarmalayıcı (MAX_PRECISION = 24)
  - Error          : Hata türleri
```

### 1.3 Anahtar Türetme Hiyerarşisi (HKD)

**Dosya:** `hkd.rs`

```
// Public key türetimi:
x = BLAKE2b("decaf377-fmd.hkd", root_pub_enc || [index]) mod Fr
X_i = root_pub + x * G    // alt açık anahtar

// Private key türetimi:
x_i = root_priv + x       // Fr alanında toplama
```

Özellikler:
- `root_pub_enc`: root public key'in 32 byte sıkıştırılmış kodlaması
- Index 0'dan MAX_PRECISION-1'e kadar: her bir bit pozisyonu için ayrı alt-anahtar
- `proptest: public_private_derivation_match` — türetme tutarlılığını doğrular

### 1.4 Hash Fonksiyonları

**Dosya:** `hash.rs`

```rust
// Bir bit üretmek için:
to_bit(a: Element, b: Element, c: Element) -> u8
  = BLAKE2b("decaf377-fmd.bit", a_bytes || b_bytes || c_bytes)[0] & 1

// Skalara dönüştürmek için:
to_scalar(point: Element, n: u8, bits: &[u8]) -> Fr
  = BLAKE2b("decaf377-fmd.sca", point_bytes || [n] || bits_bytes) mod Fr
```

### 1.5 Clue Oluşturma (ClueKey → Clue)

**Dosya:** `clue_key.rs`
**Fonksiyon:** `create_clue_deterministic(rseed, precision_bits)`

```
Girdi:
  root_pub      = DetectionKey * G           (açık anahtar)
  root_pub_enc  = root_pub.compress()        (32 byte)
  rseed         = [u8; 32]                   (rastgele tohum)
  n             = precision_bits             (u8, <= MAX_PRECISION)

Adımlar:
  1. r = BLAKE2b("decaf377-fmd.rdv", root_pub_enc || rseed) mod Fr
  2. z = BLAKE2b("decaf377-fmd.zdv", root_pub_enc || rseed) mod Fr
  3. P = r * G
  4. Q = z * G
  5. For i in 0..n:
       X_i = derive_public(root_pub, root_pub_enc, i)  // HKD
       rX_i = r * X_i                                  // ElGamal-benzeri paylaşım
       key_i = BLAKE2b("decaf377-fmd.bit", P, rX_i, Q)[0] & 1
       ctxt_i = key_i XOR 1                            // Dogru mesaj icin ctxt_i=0
  6. m = BLAKE2b("decaf377-fmd.sca", P, n, ctxts) mod Fr
  7. y = (z - m) * r^{-1}

Cikti: Clue = P(32 byte) || y(32 byte) || n(1 byte) || ctxts(3 byte) = 68 byte
```

**Onemli:** `ctxts` 3 byte, MAX_PRECISION=24 bit — her byte 8 bit, tam 3 byte = 24 bit.

### 1.6 Clue Tespiti (DetectionKey → bool)

**Dosya:** `detection.rs`
**Fonksiyon:** `examine(clue) -> bool`

```
1. Temel gecerlilik kontrolleri:
   - P != identity noktasi
   - y != 0

2. Q'yu yeniden hesapla:
   Q = y * P + m * G
   (m = to_scalar(P, precision_bits, ctxts))

3. Her bit i in 0..precision_bits icin:
   key_i = BLAKE2b("decaf377-fmd.bit", P, P * x_i, Q)[0] & 1
   msg_i = ctxt_i XOR key_i
   if msg_i == 0: return false  // hizli ret (short-circuit)

4. return true  // tum bitleri gecti
```

**Kisa devre optimizasyonu:** ilk basarisiz bit aninda `false` doner. Yanlis pozitifler `2^(-n)` olasilikla tum bitleri gecer.

**DetectionKey olusturma (from_field):**
```rust
root_pub = dtk * G
xs[i] = derive_private(dtk, root_pub_enc, i)  // HKD ile gizli alt-anahtarlar
```

### 1.7 Clue Yapisi

**Dosya:** `clue.rs`

```
Clue([u8; 68]):
  byte  0..31: P (decaf377 grup elemani, sikistirilmis)
  byte 32..63: y (skaler, little-endian)
  byte 64:     precision_bits (n)
  byte 65..67: ctxts (n adet bit, 3 byte'a paketlenmis)

Clue::precision() = self.0[64]
```

### 1.8 Precision

**Dosya:** `precision.rs`

```rust
Precision(u8)
MAX_PRECISION = 24

TryFrom<u8/u32/u64/i32> -> Precision
  - 0..=MAX_PRECISION -> Ok
  - > MAX_PRECISION -> Err("precision too high")
```

### 1.9 Guvenlik Analizi

| Ozellik | Deger |
|---------|-------|
| Yanlis pozitif orani | `2^(-n)` (n = precision_bits) |
| Maksimum hassasiyet | 24 bit → `1/16_777_216` |
| Grup | decaf377 (BLS12-377 egrisinde ristretto benzeri) |
| Hash | BLAKE2b (alan disi ciktilar icin `mod Fr`) |
| Clue boyutu | 68 byte |
| Detection complexity | O(n) skaler carpma |

**Gizlilik:** Detection server, kimin mesajina baktigini ogrenemez — sadece olasiliksal kararlar verebilir.
**Inkar edilebilirlik:** Yanlis pozitifler nedeniyle mesajin gercekten o kullaniciya ait oldugu kanitlanamaz.

### 1.10 Matematiksel Dogrulama

Clue olusturma ve tespitteki Q esitligini dogrulayin:

```
Olusturma: Q = z * G
Tespit:    Q = y * P + m * G
               = ((z - m) * r^{-1}) * (r * G) + m * G
               = (z - m) * G + m * G
               = z * G     // ESIT!
```

Bu esitlik sayesinde detection key sahibi Q'yu bagimaz sekilde yeniden hesaplayabilir, r ve z degerlerini bilmeden.

---

## 2. eddy — Esik (Threshold) Akis Sifrelemesi

### 2.1 Genel Mimari

Eddy, DEX batch swap miktarlarini sifrelemek icin tasarlanmis bir esik sifreleme kutuphanesidir. Temel prensipler:

1. **Homoforik sifreleme:** `Enc(a) + Enc(b) = Enc(a + b)` — sifreli metinler toplanabilir
2. **Esik cozme:** `t/n` katilimci gerektirir (DKG henuz WIP)
3. **64-bit → 4 x 16-bit:** Tasma onleme icin degerler limb'lere bolunur
4. **Baby-step Giant-step:** Cozme icin ayrik logaritma tablosu kullanir

**Durum:** Sifreleme + cozme + cozme kaniti TAMAMLANDI; DKG + serileştirme + sifreleme kaniti TODO.

### 2.2 Deger Kodlamasi (Value → Limbs)

**Dosya:** `value.rs`

```rust
// 64-bit v → 4 x 16-bit limb
limb0 = v & 0x0000_ffff           // bit  0..15
limb1 = (v >> 16) & 0x0000_ffff   // bit 16..31
limb2 = (v >> 32) & 0x0000_ffff   // bit 32..47
limb3 = (v >> 48) & 0x0000_ffff   // bit 48..63

// Yeniden yapilandirma:
v = limb0 + limb1 << 16 + limb2 << 32 + limb3 << 48
```

`Value` (ciphertext seviyesinde) `u64` tutar.
`limb::Value` `u32` tutar (birden fazla sifreli metin toplanandiginda 16-bit asilabilir).

**Test:** `limb_value_roundtrip`, `encrypt_verify_roundtrip` (proptest)

### 2.3 ElGamal Sifrelemesi (Limb Seviyesi)

**Dosya:** `limb/value.rs`, `limb/ciphertext.rs`

```
Anahtar: (sk, PK = sk * G)   // PrivateKeyShare, EncryptionKey

Sifreleme:
  r <- Fr (rastgele blinding)
  c1 = r * G
  c2 = r * PK + v * G        // v = limb degeri (u16 ya da u32)

Cozme (tek anahtar):
  m = c2 - sk * c1 = v * G   // ayrik logaritma alinacak

Toplama (homoforik):
  (c1_a + c1_b, c2_a + c2_b) = Enc(a) + Enc(b) = Enc(a + b)
```

**ElGamal Ciphertext yapisi:**
```rust
limb::Ciphertext {
    c1: decaf377::Element,  // r * G
    c2: decaf377::Element,  // r * PK + v * G
}
```

`Add` / `AddAssign` trait'leri uygulanmis — batch isleme icin limb-by-limb toplama.

**Varsayilan (Default):** c1 = c2 = GENERATOR (nol sifreleme identity degil; bu tasarim karari not edilmeli).

### 2.4 Esik Cozme

**Dosya:** `limb/decryption_share.rs`

```
Katilimci i'nin cozme payi:
  D_i = key_share_i * c1    // gizli anahtar payi ile c1'i carp

Lagrange interpolasyonu (limb/ciphertext.rs):
  lambda_i = prod_{j != i} (j / (j - i))    // Lagrange katsayisi (Fr'de)
  D = sum_i (lambda_i * D_i)

Sonuc:
  m = c2 - D = v * G          // ayrik logaritma tablosunda ara

Schnorr benzeri Sigma kaniti (Fiat-Shamir via Merlin):
  k <- Fr (Merlin transcript RNG'den, witness'a bagli)
  kB  = k * G
  kC1 = k * c1
  c = transcript.challenge_scalar("c")
  r = k - key_share_i * c

Dogrulama:
  kB'  = r * G + PK_i * c
  kC1' = r * c1 + D_i * c
  gecerli <=> kB' == kB AND kC1' == kC1
```

**Tip guvenliği:** `DecryptionShare<Unverified>` → `verify()` → `DecryptionShare<Verified>`. Dogrulanmamis paylari `Ciphertext::decrypt()`'e gecirmek derleme hatasidir.

### 2.5 Ciphertext (Ust Seviye)

**Dosya:** `ciphertext.rs`

```rust
Ciphertext {
    c0: limb::Ciphertext,   // limb 0 (bit 0..15)
    c1: limb::Ciphertext,   // limb 1 (bit 16..31)
    c2: limb::Ciphertext,   // limb 2 (bit 32..47)
    c3: limb::Ciphertext,   // limb 3 (bit 48..63)
}
```

**Cozme akisi:**
```
decrypt(shares: Vec<DecryptionShare<Verified>>, table: &dyn DecryptionTable):
  1. Her limb icin ayri paylari topla
  2. Her limb ciphertext'i Lagrange ile coz → decaf377::Element
  3. Table'da ayrik logaritma ara → u32
  4. from_limbs(v0, v1, v2, v3) → Value
```

**Homoforik toplama:**
```rust
// Batch'leme: sifreli metinler dogrudan toplanabilir
c_total += c_new;  // Her limb icin ayri c1+c1, c2+c2
```

### 2.6 Ayrik Logaritma Tablosu

**Dosya:** `decryption_table.rs`

```
Tablo boyutu: 2^{16 + lg(N)} girdi
  - N = batch icindeki sifreli metin sayisi
  - 64 sifreli metin → 2^{16+6} = 2^22 = 4M girdi

Baslatma:
  initialize(k):
    x = 0, xB = G
    while x < 2^k:
      store(xB.compress(), x)
      x += 1; xB += G

Sorgu:
  lookup(point: [u8; 32]) → Option<u32>

Mock implementasyon: BTreeMap<[u8;32], u32>
Onerilen: RocksDB backing (nbits || key, value) formati
```

**Test:** `build_16` ve `build_21` testleri `#[ignore]` (yavas) — elle calistirilmali.

### 2.7 DecryptionShare (Ust Seviye)

**Dosya:** `decryption_share.rs`

```rust
DecryptionShare<S: VerificationStatus> {
    participant_index: u32,
    share0..share3: limb::DecryptionShare<S>,
}

// Type-level durum makinesi:
Unverified -> verify() -> Verified
// Dogrulanmamis payi kullanmak derleme hatasi!
```

### 2.8 Transcript Protokolu

**Dosya:** `transcript.rs`

Merlin transcript kullanimi (Fiat-Shamir donusumu icin):

```
Domain separator'lar:
  begin_decryption():      "eddy-decaf377-decrypt"
  begin_limb_decryption(): "begin-limb"

append_public_key_share:   index || pub_key_share_compressed
append_limb_ciphertext:    c1_compressed || c2_compressed
challenge_scalar:          64 byte → Fr::from_le_bytes_mod_order
```

**RNG bagimlilik:** Merlin `build_rng()` + gizli anahtar payi ile yeniden anahtarlama → kanittaki k deterministik ama dis gozlemciye gizli.

### 2.9 Sifreleme Kaniti (WIP)

**Dosya:** `proofs.rs`

```rust
// UYARI: Bu kanit degeri ifsa ediyor!
pub struct TransparentEncryptionProof {
    value: u64,
    blindings: [decaf377::Fr; 4],
}
```

Mevcut `TransparentEncryptionProof` sadece test amacli — gercek ZK-SNARK kaniti TODO. `verify()` her limb icin ElGamal sifreleme esitligini plaintext ile kontrol eder.

### 2.10 DKG (Dagitik Anahtar Uretimi)

**Dosya:** `dkg.rs`

```rust
pub struct Committee {
    pub shares: Vec<PublicKeyShare>,
    pub threshold: u32,
}
```

Sadece iskelet — gercek DKG protokolu henuz implemente edilmemis. Key share'ler icin cggmp21 veya benzeri dis protokol kullanilmali.

---

## 3. FMD + Eddy Entegrasyonu

FMD ve Eddy, Penumbra'nin iki ayri gizlilik katmanini olusturur:

| Sistem | Amac | Mekanizma |
|--------|------|-----------|
| FMD | Mesaj tespiti | ZK-benzeri probabilistik ipuclari |
| Eddy | Batch swap miktari | Esik ElGamal sifreleme |

**DEX baglantisi:**
- Swap miktarlari Eddy ile sifreler → validators sifreli miktarlari toplayabilir
- Epoch sonunda esik cozme ile gercek miktarlar ortaya cikar
- FMD, kullanicilarin islemlerini detection server'a delege etmesine izin verir

---

## 4. DEX Uygulama Notu

### FMD (Nova Shield DEX Icin)

**Dogrudan kullanilabilir:**
- Penumbra'nin FMD protokolu son derece oz ve test edilmis
- 68 byte clue boyutu makul (block boyutunu fazla sismirmez)
- decaf377 yerine ristretto255 veya baska bir grup kullanilabilir

**Uygulama adimlari:**
1. `decaf377-fmd` crate'ini bagimlilik olarak ekle
2. Her islem icin `ClueKey`'e karsilik `Clue` olustur ve transaction'a ekle
3. Detection server: `DetectionKey` ile `examine(clue)` cagir
4. Kullanici: `precision_bits` ile tespit hassasiyetini kontrol eder

**Dikkat:** MAX_PRECISION=24 bit → 3 byte ctxts yeterli. Daha buyuk precision icin format degismeli.

### Eddy (Nova Shield DEX Icin)

**Mevcut durum degerlendirmesi:**
- Sifreleme + Lagrange cozme + Schnorr kanit: HAZIR
- DKG + serializasyon + ZK sifreleme kaniti: TODO

**Nova Shield DEX icin oneri:**
1. Esik sifrelemesi DEX'te kritik degilse: simdilik sifresiz batch swap kullan
2. Privacy oncelikli ise: Eddy'nin mevcut kismini alip DKG icin cggmp21 entegre et
3. Ayrik logaritma tablosu boyutu dikkatli ayarlanmali: batch basina max N sifreli metin → tablo boyutu 2^(16+log2(N))
4. RocksDB backing store onerisi dogru — production'da BTreeMap yeterli degil

**ElGamal homoforik ozelligi kritik:** Validators her slot'ta batch'leri toplayip epoch'ta cozebilir.

### Kritik Tasarim Kararlari

1. **Limb ayrimi neden 16-bit?** N sifreli metin toplansa, her limb max `N * 2^16` degerine cikar. `N <= 2^k` icin tablo boyutu `2^{16+k}`. 16-bit limb → 64-bit icin 4 limb = optimal denge.

2. **Lagrange interpolasyonu:** `lambda_i = prod_{j!=i} (j / (j-i))` — klasik Shamir SS cozumu. t-of-n esik icin t pay yeterli.

3. **Transcript deterministik randomness:** `build_rng().rekey_with_witness_bytes("key_share", ...)` — ayni gizli anahtar + ayni ciphertext = ayni k. Bu tutarlilik icin iyi ama k'nin gizli anahtari sizdimamasina dikkat.

---

## 5. Dosya Ozeti

### decaf377-fmd (7 dosya)
| Dosya | Icerik |
|-------|--------|
| lib.rs | Module re-exports |
| clue_key.rs | ClueKey, ExpandedClueKey, clue olusturma |
| detection.rs | DetectionKey, examine(), HKD private |
| clue.rs | Clue([u8;68]), precision() |
| hkd.rs | derive_public/private, proptest |
| hash.rs | to_bit(), to_scalar() |
| precision.rs | Precision(u8), MAX_PRECISION=24 |

### eddy (14 dosya)
| Dosya | Icerik |
|-------|--------|
| lib.rs | Module re-exports, TODO listesi |
| encryption_key.rs | EncryptionKey(Element) |
| value.rs | Value(u64), to_limbs(), from_limbs(), transparent_encrypt() |
| ciphertext.rs | Ciphertext{c0..c3}, decrypt(), Add/AddAssign |
| key_share.rs | PrivateKeyShare, PublicKeyShare |
| decryption_share.rs | DecryptionShare<Verified/Unverified>, verify() |
| decryption_table.rs | DecryptionTable trait, MockDecryptionTable, initialize() |
| limb.rs | limb module re-exports |
| limb/value.rs | limb::Value(u32), transparent_encrypt() |
| limb/ciphertext.rs | limb::Ciphertext{c1,c2}, decrypt(), lagrange_coefficient() |
| limb/decryption_share.rs | limb::DecryptionShare, Schnorr kanit |
| transcript.rs | TranscriptProtocol trait, Merlin impl |
| dkg.rs | Committee{shares, threshold} (WIP) |
| proofs.rs | TransparentEncryptionProof (WIP, deger ifsa ediyor!) |

---

## 6. Kritik Bulgular

1. **FMD matematigi tutarli:** `y * P + m * G = Q` esitligi, clue olusturma ve tespitte birebir ortusiyor. `z * G - r^{-1} * (m) * r * G = (z-m) * G` dogrulanmis.

2. **Eddy'de DKG eksik:** Threshold sifrelemesi icin DKG olmadan gercek t/n guvenligi yok. Mevcut kod key share'leri disaridan aliyor.

3. **TransparentEncryptionProof guvensiz:** Adinin tam tersine "transparent" kelimesi kasitli — plaintext degeri ifsa ediyor. Uretimde ZK kanitle degistirilmeli.

4. **Tablo boyutu siniri:** `2^22 = 4M` girdi → `build_21` testi `#[ignore]` ile isaretlenmis (yavas). Uretim icin asenkron/lazy yukleme veya RocksDB sart.

5. **Type-state pattern:** `DecryptionShare<Verified/Unverified>` — dogrulanmamis payi decrypt'e vermek derleme hatasidir. Rust'in tip sistemi guvenliği garanti ediyor.

6. **limb::Ciphertext Default degeri:** `c1 = c2 = GENERATOR` — nol element degil. Toplamaya baslama noktasi olarak kullanilirken dikkat gerekiyor; `0 * G = identity` oldugu durumlarda yanlis toplama yapilabilir.
