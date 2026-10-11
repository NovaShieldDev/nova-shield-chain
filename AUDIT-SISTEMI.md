# Nova Shield Chain — Güvenlik Audit Sistemi

Tarih: 2026-07-22
Durum: PLANLANMIŞ — Araştırma fazı tamamlanınca uygulamaya geçilecek

---

## Audit Sistemi Tokenizasyon Modeli

**ÖNEMLI NOT:** Bu model, audit sistemi tamamen geliştirilip piyasaya sürüldükten sonra
ve $NOVA token lansman yapıldıktan sonra uygulamaya girecektir.
Audit sistemi çalışmadan, kanıtlanmadan bu model BAŞLATILMAZ.

### Audit Ücreti Modeli

Audit yaptırmak isteyen her proje:

```
Toplam Ücret: 100.000$ değerinde $NOVA token

Dağılım:
  %50 → 50.000$ — Nova Shield hazinesine direkt transfer
  %50 → 50.000$ — Proje elinde kalır (kilit takvimli)

Kilit Takvimi (%50'lik kısım için):
  0-3. ay  → %0 erişim (tamamen kilitli — cliff dönemi)
  4. ay    → %20 açılır
  5. ay    → %20 açılır
  6. ay    → %20 açılır
  7. ay    → %20 açılır
  8. ay    → %20 açılır (son dilim)
```

### Bu Modelin Gücü

- Her audit = 100K$ $NOVA satın alma baskısı → token fiyatı artar
- Cliff + vesting = 8 ay boyunca satış baskısı minimize
- Proje 8 ay boyunca $NOVA holder kalır → ekosisteme bağlı
- Hazineye gelen %50 = ekibin sürdürülebilir geliri
- Trail of Bits, Certora gibi geleneksel audit firmalarından fark:
  ödenen para ekosisteme geri dönüyor, kaybolmuyor

### Uygulama Önkoşulları (Bu Model İçin)

1. Audit sistemi tam çalışır durumda ve piyasaya sürülmüş olmalı
2. En az 1-2 başarılı responsible disclosure ile itibar kanıtlanmış olmalı
3. $NOVA token lansman yapılmış olmalı
4. Smart contract / protokol seviyesinde kilit mekanizması hazır olmalı

---

## Genel Strateji

İki paralel görev:

1. **Mod A — Kendi Kodumuz (self):** Nova Shield Chain'i sürekli, acımasız şekilde tara.
   Her commit, her yeni modül, her değişiklik anında taranır.

2. **Mod B — Rakip L1'ler (competitor):** Solana, Ethereum, Cosmos, Avalanche, Polkadot,
   Sui, Aptos ve diğer L1'leri deneyimli hacker gözüyle tara.

### Kritik Döngü (Mod B için)

```
Rakip L1'de kritik güvenlik açığı bul
           ↓
"Bizde de aynı açık var mı?" → HEMEN kendi kodumuzda kontrol et
           ↓
Varsa → Nova Shield'de ÖNCE düzelt
           ↓
Rakip projeye ücretsiz responsible disclosure yap
Tek şart: Fix duyurularında Nova Shield Chain'i zikretmeleri
           ↓
"[Solana/Cosmos/Avalanche] ekibi Nova Shield Chain güvenlik
 ekibine kritik güvenlik açığını sorumlu bir şekilde bildirdiği için
 teşekkür eder"
           ↓
Ekosistemde itibar + ücretsiz PR + mainnet öncesi güven inşası
```

Bu döngü aynı anda 4 şeyi başarır:
- Kendi kodumuz rakiplerin açıklarından arınmış olur
- Etik hacker + iyi niyetli L1 imajı kazanırız
- Ecosystem'de "ciddi güvenlik odaklı ekip" olarak bilinir hale geliriz
- Rakiplerin zayıf noktalarını öğrenir, biz daha güçlü inşa ederiz

---

## Sistem Mimarisi

```
nova-shield-chain/
└── audit/
    ├── self/               # Kendi kodumuzu tarayan modüller
    │   ├── runner.sh       # Ana tarama betiği
    │   ├── rust/           # Rust-spesifik araçlar
    │   ├── consensus/      # Konsensüs doğruluk kontrolleri
    │   ├── crypto/         # MPC/ZK implementasyon kontrolleri
    │   ├── dex/            # DEX güvenlik kontrolleri
    │   └── reports/        # Çıktı raporları
    │
    ├── competitor/         # Rakip L1 tarama modülleri
    │   ├── runner.sh       # Rakip tarama betiği
    │   ├── targets/        # Her rakip için config
    │   ├── findings/       # Bulunan açıklar
    │   └── disclosure/     # Responsible disclosure şablonları
    │
    ├── engine/             # Ortak analiz motoru
    │   ├── claude_pipeline.py      # AI-destekli kod analizi
    │   ├── rust_analyzer.py        # Rust-spesifik statik analiz
    │   ├── consensus_checker.py    # Konsensüs bug checker
    │   ├── crypto_auditor.py       # MPC/ZK/BLS güvenlik denetimi
    │   ├── dex_auditor.py          # DEX/AMM güvenlik denetimi
    │   ├── economic_sim.py         # Ekonomik saldırı simülasyonu
    │   ├── poc_generator.py        # PoC üretici
    │   ├── report_writer.py        # Rapor yazıcı
    │   └── disclosure_helper.py    # Responsible disclosure yardımcısı
    │
    └── data/
        ├── rust_vulns.yaml         # Bilinen Rust güvenlik açıkları
        ├── l1_attack_patterns.yaml # L1 saldırı örüntüleri
        ├── dex_exploits.yaml       # DEX exploit veritabanı
        ├── crypto_gotchas.yaml     # Kriptografi tuzakları
        └── consensus_bugs.yaml     # Geçmiş konsensüs hataları
```

---

## Mod A — Kendi Kodumuz İçin Araçlar

### Rust-Spesifik Statik Analiz

| Araç | Amaç | Kritiklik |
|------|------|-----------|
| `cargo audit` | Bağımlılıklardaki CVE'leri bul | YÜKSEK |
| `cargo clippy` | Lint + güvenlik uyarıları | YÜKSEK |
| `cargo geiger` | unsafe blok sayım ve takibi | YÜKSEK |
| `rudra` | Rust güvenlik linter (panic safety, Send/Sync variance) | YÜKSEK |
| `cargo +nightly miri` | Tanımsız davranış tespiti | YÜKSEK |
| `lockbud` | Deadlock tespiti | ORTA |
| `cargo fuzz` | libFuzzer tabanlı fuzzing | YÜKSEK |
| `cargo-semver-checks` | API kırılma tespiti | ORTA |

### Nova Shield-Spesifik Kontroller

#### 1. Konsensüs Doğruluk Kontrolleri
- Tower BFT implementasyonumuzda lockout hesaplama doğruluğu
- Fork choice algoritması edge case'leri
- Validator set değişikliklerinde konsensüs sürekliliği
- Uzun menzilli saldırı (long-range attack) koruması
- Nothing-at-stake koruması
- Equivocation (çift oy) tespiti ve cezalandırma
- SUPERMINORITY_THRESHOLD hesaplama doğruluğu

#### 2. MPC / Threshold İmza (cggmp21)
- Threshold şema parametrelerinin doğruluğu
- DKG (Distributed Key Generation) protokol güvenliği
- Signing seremonyasında cheating tespiti
- Rogue key attack koruması
- Key resharing güvenliği
- Timing saldırısı direnci (sabit zamanlı operasyonlar)
- Nonce yeniden kullanımı (felony) tespiti

#### 3. ZK Devre Güvenliği (arkworks / Groth16 / PLONK)
- Underconstrained sinyal tespiti
- Prover-verifier ayrımı doğruluğu
- Witness manipulation path analizi
- Soundness açığı kontrolü
- Transcript hash manipülasyonu
- BLS12-381 implementasyon doğruluğu
- Poseidon hash sabit zaman analizi

#### 4. DEX / AMM Güvenliği
- AMM formülü matematiksel doğruluk (x*y=k, konsantre likidite)
- Fiyat manipülasyonu direnci
- Flash attack (flash loan eşdeğeri) koruması
- Sandwich saldırısı koruması (MEV)
- Reentrancy-eşdeğeri cross-program invocation güvenliği
- Slippage parametrelerinin doğru uygulanması
- Fee hesaplama doğruluğu (rounding saldırıları)
- Liquidity provision/removal edge case'leri
- Oracle manipülasyon direnci
- TWAP saldırı maliyeti analizi

#### 5. P2P Ağ Güvenliği (libp2p + gossipsub)
- DoS amplifikasyon vektörleri
- Eclipse saldırısı direnci
- Sybil saldırısı koruması
- Gossip mesaj validasyonu
- Peer scoring manipülasyonu
- Bandwidth exhaustion vektörleri
- QUIC implementasyon güvenliği

#### 6. Bellek Güvenliği (Rust unsafe)
- Her `unsafe` blok için manuel doğrulama
- `transmute` kullanımları — lifetime erasure analizi
- Raw pointer dereference güvenliği
- FFI boundary güvenliği
- `set_len()` güvensiz kullanım tespiti
- `from_raw_parts()` hizalama kontrolü

#### 7. Ekonomik Güvenlik
- Token enflasyon/deflasyon mekanizması doğruluğu
- Stake ödülü hesaplama doğruluğu
- Slashing koşulları ve miktarı
- Validator incentive alignment
- DEX fee geliri sürdürülebilirliği
- Likidite teşvik mekanizması güvenliği

---

## Mod B — Rakip L1 Tarama Hedefleri

### Öncelik Sırası

| Rakip | Neden Öncelikli | Ortak Bileşen |
|-------|-----------------|---------------|
| Solana | En yakın mimari (Rust L1, PoH, Tower BFT) | gossip, turbine, banking stage |
| Cosmos SDK | IBC bridge mantığı | cross-chain messaging |
| Polkadot | Parachain konsensüs, DEX palletleri | XCMP, pallet-asset-conversion |
| Avalanche | Subnet konsensüs, P-chain/C-chain | validator set yönetimi |
| Aptos/Sui | Move VM güvenliği, parallel execution | BFT konsensüs |
| Ethereum (Geth) | EVM runtime, state trie | MPT implementasyonu |
| Penumbra | ZK DEX, concentrated liquidity | ZK circuit güvenliği |

### Rakip Tarama Metodolojisi

Her rakip L1 için şu adımlar:

```
1. KEŞIF
   - Repo klonla (araştırma dizininde zaten var)
   - Kritik modülleri tespit et (consensus, crypto, dex, p2p)
   - Son 90 günde hangi dosyalar değişti? (git log)
   - Audit tarihi var mı? Varsa ne zaman, kapsamı ne?

2. OTOMATİK TARAMA
   - cargo audit → bağımlılık CVE'leri
   - cargo clippy → güvenlik uyarıları
   - cargo geiger → unsafe blok haritası
   - Özel pattern'lar için grep (integer overflow, unchecked math, etc.)

3. AI ANALİZİ (Claude Pipeline)
   - Her kritik modül → Claude'a yolla
   - Prompt: "Deneyimli bir güvenlik araştırmacısı olarak..."
   - Focus: mantık hataları, ekonomik saldırılar, konsensüs bug'ları

4. EKONOMİK SİMÜLASYON
   - DEX varsa: flash attack path'larını enumerate et
   - Oracle kullanıyorsa: manipülasyon maliyeti hesapla
   - Validator ekonomisi: slashing/reward game theory analizi

5. DOĞRULAMA
   - Potansiyel bug bulununca: test yaz ve doğrula
   - Çalışan PoC üret (Rust test / local validator)
   - Impact hesapla (kaç kullanıcı, ne kadar fon etkilenir)

6. KENDİMİZDE KONTROL
   - "Nova Shield'de bu açık var mı?" sorusunu sor
   - Varsa HEMEN kendi kodumuzda düzelt ve kayıt altına al

7. RESPONSIBLE DISCLOSURE
   - Rakibe özel disclosure şablonu doldur
   - 90 günlük disclosure window belirle
   - İletişim kanalını bul (security@, Discord, GitHub security advisory)
   - Fix sonrası koordineli yayın planı

8. BASIN AÇIKLAMASI
   - Rakibin fix duyurusu + Nova Shield zikri
   - Nova Shield blog: "Ekosistemi Güçlendiriyoruz" serisi
   - Twitter/X: teknik thread
```

---

## Responsible Disclosure Şablonu

```
Konu: [GÜVENLİK AÇIĞI] [Rakip Proje] — [Kısa Başlık]

Merhaba [Proje] Güvenlik Ekibi,

Nova Shield Chain güvenlik araştırma ekibi olarak, [Proje]'nin
[bileşen] modülünde kritik bir güvenlik açığı tespit ettik.

Açığın Özeti:
[1-2 cümle]

Etkilenen Bileşen:
[dosya:satır]

Potansiyel Etki:
[ne olabilir]

Kanıt:
[PoC veya teknik detay — tam detayı sonraki adımda paylaşırız]

Bu açığı Nova Shield Chain'in kendi güvenlik araştırması sürecinde
keşfettik. Ekosisteme katkı sağlamak amacıyla ücretsiz olarak
bildiriyoruz.

Tek ricamız: Fix duyurularınızda Nova Shield Chain'i zikretmeniz.

90 gün içinde fix bekliyoruz. Koordineli açıklama için hazırız.

Nova Shield Chain Güvenlik Ekibi
https://nova-shield-chain.io
```

---

## Öncelik Metrikleri

### Kendi Kodumuz İçin Kritiklik Skoru

```
KRİTİK (hemen düzelt):
  - Consensus protokolünü bozan her şey
  - Validator fonlarına erişim sağlayan her şey
  - DEX'ten para çalınmasına izin veren her şey
  - ZK devrelerinde soundness açığı
  - MPC'de key recovery mümkün kılan her şey

YÜKSEK (1 sprint içinde):
  - DoS vektörleri (ağ durdurulabilir)
  - Yanlış hesaplama (ödül/slashing)
  - Oracle manipülasyonu
  - MEV saldırısına açık DEX tasarımı

ORTA (planlı):
  - Performans güvenlik açıkları
  - Teşvik uyumsuzlukları
  - Bağımlılık CVE'leri (düşük CVSS)

DÜŞÜK (backlog):
  - Bilgi ifşası
  - Loglama eksiklikleri
```

---

## Uygulama Takvimi

### Faz 1 — Araştırma Tamamlanınca (şimdiki görev)
Solana, Penumbra, Polkadot, CoW Protocol, multi-party-ecdsa analizleri biter.
Bu analizlerden çıkan bulgular doğrudan audit sisteminin `data/` dizinlerine aktarılır:
- Bulunan pattern'lar → `l1_attack_patterns.yaml`
- DEX exploit'ları → `dex_exploits.yaml`
- Kriptografi tuzakları → `crypto_gotchas.yaml`
- Konsensüs bug'ları → `consensus_bugs.yaml`

### Faz 2 — Temel Altyapı
- `audit/self/runner.sh` — cargo audit + clippy + geiger otomasyonu
- `audit/engine/claude_pipeline.py` — mevcut bug-bounty sisteminden uyarla, Rust L1'e özel
- `audit/engine/rust_analyzer.py` — Rust-spesifik güvenlik kontrolleri
- `audit/data/` — tüm YAML veritabanları

### Faz 3 — DEX ve Kripto Modülleri
- `audit/engine/dex_auditor.py` — AMM/DEX güvenlik motoru
- `audit/engine/crypto_auditor.py` — MPC/ZK denetim motoru
- `audit/engine/economic_sim.py` — ekonomik saldırı simülasyonu (CoW Protocol analizinden öğrendiklerimizle)

### Faz 4 — Rakip Tarama
- `audit/competitor/` dizini ve runner
- İlk hedef: Solana (en yakın mimari, araştırma zaten var)
- `audit/engine/disclosure_helper.py` — responsible disclosure otomasyonu

### Faz 5 — Sürekli İzleme
- Her commit'te otomatik Faz 1 taraması
- Haftalık rakip taraması (yeni commitler)
- CVE veritabanı günlük güncelleme

---

## Mevcut Bug-Bounty Sisteminden Alınacaklar

`/home/ali-haydar-karadas/bug-bounty/audit-system/` içindeki şu modüller
Nova Shield için doğrudan uyarlanacak:

| Mevcut Dosya | Nova Shield Uyarlaması |
|-------------|----------------------|
| `engine/claude_pipeline.py` | Rust L1 prompt'larıyla genişlet |
| `engine/economic_math_sim.py` | DEX AMM formülleri ekle |
| `engine/llm_code_reader.py` | Rust syntax awareness ekle |
| `engine/poc_generator.py` | Rust/Foundry PoC template'leri |
| `engine/report_writer.py` | Doğrudan kullan + L1 bug kategorileri ekle |
| `engine/knowledge_graph.py` | L1 bağımlılık grafiği için uyarla |
| `engine/zk_analyzer.py` | arkworks/Groth16/PLONK için genişlet |
| `engine/fork_detector.py` | L1 fork analizi için uyarla |
| `detect/` | Rust crate tipi tespiti ekle |

---

## Başarı Metrikleri

### Kendi Kodumuz İçin
- Her release öncesi: 0 kritik açık
- Her commit: otomatik tarama geçmeli
- unsafe blok sayısı: her sprint başında raporlanmalı
- Bağımlılık CVE: haftalık sıfır CVE hedefi

### Rakip Tarama İçin
- İlk 6 ay: en az 1 rakip L1'de anlamlı güvenlik açığı bul
- Her responsible disclosure: blog post + sosyal medya
- Ekosistemde tanınma: "Nova Shield Chain ekibi bildirdi" haberleri

---

## Notlar

- Mevcut araştırma analiz dosyaları (`research/analysis/`) bu sistemin
  ham veri kaynağıdır. 5 repo analizi bitince çıkan pattern'lar `audit/data/`'ya aktarılır.
- Bug-bounty audit sistemi EVM/Solidity odaklı; Nova Shield sistemi Rust L1 odaklı.
  İkisi birbirini tamamlar, birbirinin yerine geçmez.
- Responsible disclosure her zaman etik ve koordineli olacak.
  Rakipler fix yapmadan kamuya açıklamak kesinlikle yasak.
