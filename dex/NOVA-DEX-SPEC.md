# Nova DEX — Tam Teknik ve Ürün Spesifikasyonu

Yazar: Nova Shield Chain
Tarih: 2026-10-06
Durum: Tasarım aşaması — implementasyon bekliyor

---

## 1. GENEL BAKIŞ

Nova DEX, Nova Shield Chain üzerinde çalışan native bir merkeziyetsiz borsadır.
Ayrı bir DEX tokeni yoktur. Tüm işlemler NOVA token ile yapılır.
EVM uyumu aranmaz — ekosistem tamamen izoledir ve kasıtlı olarak böyle tasarlanmıştır.

### Temel Felsefe

Mevcut DEX'lerin üç temel sorunu vardır:

1. Rug pull — creator istediği an likiditeyi çekebilir
2. MEV — frontrunning ve sandwich attack ile kullanıcılar soyulur
3. Şeffaflık eksikliği — kullanıcı pool'un durumunu bilemez

Nova DEX bu üç sorunu zincir seviyesinde çözer. Kod değil, protokol garantiler.

---

## 2. POOL OLUŞTURMA

### Ücret

- Sabit: $15-20 değerinde NOVA
- Günlük NOVA/USD fiyatına göre hesaplanır
- Öngörülebilir, gas'a bağlı değil

### Rakip Karşılaştırması

| DEX         | Pool Açma Ücreti         |
|-------------|--------------------------|
| Nova        | $15-20 sabit             |
| Uniswap v3  | $50-200 (gas'a göre)     |
| Raydium     | ~$35 (SOL karşılığı)     |
| PancakeSwap | <$5 (BNB gas)            |

Uniswap ve Raydium'da ücret tahmin edilemez. Nova'da her zaman sabittir.

### Akış

1. Token seç
2. Likidite kilitleme süresi seç
3. Onayla

Toplam 3 adım. Başka soru yok.

---

## 3. LİKİDİTE KİLİTLEME SİSTEMİ

### Süreler

| Seçenek   | Süre       |
|-----------|------------|
| 1 gün     | 24 saat    |
| 3 gün     | 72 saat    |
| 1 hafta   | 168 saat   |
| 1 ay      | 720 saat   |
| 12 ay     | 8.760 saat |
| Sınırsız  | Sonsuza kadar kilitli |

### Kurallar

- Seçilen süre zincire yazılır. Sonradan değiştirilemez.
- Süre uzatılabilir. Kısaltılamaz.
- Süre dolmadan likidite çekme işlemi zincir tarafından kesinlikle reddedilir.
- Sınırsız seçenler DEX arayüzünde en üstte listelenir.
- Sınırsız seçenler "Permanent Liquidity" rozeti alır.

### Pool Detay Sayfası

Her pool'da herkesin görebileceği bilgiler:

- Kilitleme süresi (seçilen)
- Kalan süre (canlı sayaç)
- Creator cüzdan adresi
- Toplam işlem hacmi
- Toplam fee kazancı
- Likidite miktarı

### Rakip Karşılaştırması

| DEX         | Liq Kilitleme                          |
|-------------|----------------------------------------|
| Nova        | Zorunlu, zincir garantili              |
| Uniswap v3  | Yok — harici araç gerekir (Unicrypt)   |
| Raydium     | Yok — creator istediği an çekebilir    |
| PancakeSwap | Opsiyonel — çoğu kimse yapmaz          |

Rakiplerde rug teknik olarak mümkündür ve her gün gerçekleşmektedir.
Nova'da zincir seviyesinde imkansızdır.

---

## 4. SWAP FEE MODELİ

### Sabit Fee

Nova DEX'te tek bir fee tier vardır: **%0.30**

Rakipler birden fazla tier sunar. Bu kullanıcıyı yanıltır ve yanlış pool seçimine yol açar.
Nova'da sürpriz yoktur.

### Gelir Dağılımı — Normal İşlem (Günlük 1-99 işlem)

```
Swap fee:   %0.30
Creator:    %0.06  (toplam fee'nin %20'si)
Kasa:       %0.24  (toplam fee'nin %80'i)
```

### Gelir Dağılımı — Yoğun İşlem (Günlük 100+ işlem)

```
Swap fee:   %0.15
Creator:    %0.03  (toplam fee'nin %20'si)
Kasa:       %0.12  (toplam fee'nin %80'i)
```

- Sayaç her gün 00:00 UTC'de sıfırlanır.
- 100. işlemden itibaren indirim otomatik başlar.
- Oranlar değişmez, sadece taban düşer.

### Rakip Karşılaştırması

| DEX         | Fee Yapısı                              |
|-------------|-----------------------------------------|
| Nova        | %0.30 sabit, tek tier                   |
| Uniswap v3  | %0.01 / %0.05 / %0.30 / %1 — karmaşık  |
| Raydium     | Pool tipine göre değişir                |
| PancakeSwap | %0.01 / %0.05 / %0.25 / %1             |

---

## 5. CREATOR FEE ÖDEME SİSTEMİ

- Her swap'ın %20'si creator'a aittir.
- Ödemeler 24 saatte 1 kez toplu olarak yapılır.
- Transfer ücreti creator'ın alacağından düşülür.
- Net miktar direkt creator cüzdanına gönderilir.
- Manuel claim gerekmez, otomatiktir.

### Rakip Karşılaştırması

| DEX         | Creator Geliri                        |
|-------------|---------------------------------------|
| Nova        | %20 otomatik, günlük net ödeme        |
| Uniswap v3  | Manuel claim, karmaşık pozisyon yönet.|
| Raydium     | Direkt otomatik ödeme yok             |
| PancakeSwap | Manuel, zincir güven sorunu mevcut    |

---

## 6. BOT VE YOĞUN İŞLEM TEŞVİKİ

Mevcut DEX'lerde en büyük hacim kaynağı arbitraj ve sniper botlarıdır.
Raydium günlük hacminin %60-70'i bot işlemidir.

Nova DEX bu kitleyi aktif olarak çeker:

### Fee İndirimi

- Günlük 100+ işlemde fee %0.30'dan %0.15'e düşer (%50 indirim)
- Hiçbir rakipte bu özellik yoktur
- Botlar aynı fee'yi normal kullanıcıyla ödemez

### Düşük Latency

- Nova blok süresi: 400ms
- BFT consensus — kesinlik anında
- Botlar için öngörülebilir ve hızlı ortam

### MEV Koruması (Bot Açısından Avantaj)

- Sandwich attack imkansız
- Tüm botlar eşit şartlarda rekabet eder
- Haksız avantaj yoktur — sadece hız ve strateji kazanır

### Rakip Karşılaştırması

| DEX         | Bot Teşviki                           |
|-------------|---------------------------------------|
| Nova        | 100+ işlemde %50 fee indirimi         |
| Uniswap v3  | Yok                                   |
| Raydium     | Yok                                   |
| PancakeSwap | Yok                                   |

---

## 7. RUG KORUMASI

### Nova'da Rug Nasıl İmkansız?

Pool açılırken seçilen kilitleme süresi zincire yazılır.
Bu süre dolmadan likidite çekme işlemi protokol tarafından reddedilir.
Creator'ın bu kısıtlamayı aşma imkanı yoktur.
Harici araç, multisig veya governance kararına gerek yoktur.
Zincirin kendisi garantör'dür.

### Rakip Karşılaştırması

| DEX         | Rug Koruması                           |
|-------------|----------------------------------------|
| Nova        | Zincir seviyesi, teknik olarak imkansız|
| Uniswap v3  | Yok — harici araç gerekir             |
| Raydium     | Yok — her gün rug yaşanıyor           |
| PancakeSwap | Yok — opsiyonel kilit, çoğu boş       |

---

## 8. MEV KORUMASI

### MEV Nedir?

Miner/validator'ların işlemleri yeniden sıralayarak kullanıcıdan değer çalması.
En yaygın türleri:
- Frontrunning: kullanıcının işleminden önce aynı işlemi yapma
- Sandwich attack: kullanıcının işleminin önüne ve arkasına işlem koyarak fiyatı manipüle etme

### Nova'da Durum

MEV zincir seviyesinde engellenmiştir.
Frontrunning ve sandwich attack protokol tarafından imkansız kılınmıştır.
Kullanıcı beklediği fiyattan işlem yapar.

### Rakip Karşılaştırması

| DEX         | MEV Koruması                                    |
|-------------|-------------------------------------------------|
| Nova        | Zincir seviyesinde tam koruma                   |
| Uniswap v3  | Yok — MEV milyarlarca dolarlık endüstri         |
| Raydium     | Yok — Solana MEV sorunu çözümsüz               |
| PancakeSwap | Yok — kullanıcı her gün soyuluyor               |

---

## 9. HEDEF KİTLE

| Kullanıcı Tipi  | Nova'yı Tercih Sebebi                              |
|-----------------|----------------------------------------------------|
| Normal trader   | Rug yok, MEV yok, şeffaf pool bilgisi              |
| Token creator   | Likidite güvende, %20 fee geliri otomatik          |
| LP sağlayıcı    | Kilitli likidite = güvenli pozisyon                |
| Arbitraj botu   | 100+ işlemde %50 fee indirimi                      |
| Sniper botu     | 400ms blok, MEV yok = eşit rekabet ortamı         |

---

## 10. GENEL KARŞILAŞTIRMA TABLOSU

| Özellik              | Nova       | Uniswap v3  | Raydium     | PancakeSwap |
|----------------------|------------|-------------|-------------|-------------|
| Pool açma ücreti     | $15-20 sabit | $50-200 değişken | ~$35  | <$5         |
| Fee tier             | %0.30 sabit | 4 farklı tier | Değişken  | 4 farklı tier|
| Liq kilitleme        | Zorunlu, zincir | Harici araç | Yok    | Opsiyonel   |
| Creator fee geliri   | %20 otomatik | Manuel     | Yok         | Manuel      |
| Bot fee indirimi     | 100+ islemde %50 | Yok   | Yok         | Yok         |
| Rug koruması         | Zincir seviyesi | Yok     | Yok         | Yok         |
| MEV koruması         | Zincir seviyesi | Yok     | Yok         | Yok         |
| Native token         | NOVA        | ETH + UNI  | SOL + RAY   | BNB + CAKE  |
| Ayrı DEX tokeni      | Yok         | Var (UNI)  | Var (RAY)   | Var (CAKE)  |

---

## 11. L1 KOD DURUMU — DEX İÇİN KONTROL

### Mevcut Durumda VAR Olanlar (L1'de çalışıyor)

**MEV Koruması: TAMAM ✅**
- `crates/nova-mempool/src/reveal.rs` — şifreli mempool (IBE + threshold decrypt)
- `crates/nova-mempool/src/ordering.rs` — commitment hash sıralı MEV-free ordering
- `crates/nova-testing/src/scenarios/mev_protection.rs` — test senaryoları
- Batch auction: tüm emirler uniform fiyattan, eşzamanlı işlenir
- Commit-reveal: slot kapanana kadar içerik şifreli
- Frontrunning: imkansız (içerik görünmez)
- Sandwich attack: imkansız (sıralama manipülasyonu yok)

**Freeze/Anomali Koruması: TAMAM ✅**
- `crates/nova-freeze/src/engine.rs` — hesap dondurma motoru
- Anomali tespiti, guardian threshold, audit log mevcut

### DEX İçin YAPILACAKLAR (Eksikler)

**1. Likidite Kilitleme Mekanizması: YOK ❌ — YAZILACAK**
- Pool açılırken süre seçimi (1g/3g/1h/1ay/12ay/sınırsız)
- Seçilen süre zincire yazılacak, değiştirilemez olacak
- Süre dolmadan liq çekme → protokol reddedecek
- Süre uzatma: izin var, kısaltma: kesinlikle yasak
- Zincir seviyesinde garanti — "rug teknik olarak imkansız" narrative'i buradan geliyor

**2. AMM Core (x*y=k): YOK ❌ — YAZILACAK**
- Constant product market maker
- Fiyat hesaplama, slippage, price impact
- %5 varsayılan slippage toleransı (mevcut test kodunda da var)

**3. Pool Factory: YOK ❌ — YAZILACAK**
- Pool oluşturma: $15-20 NOVA sabit ücret
- Pool detay sayfası: süre, sayaç, creator, hacim, fee
- Sınırsız seçenler → "Permanent Liquidity" rozeti + üste sabitlenir

**4. Fee Dağıtım Motoru: YOK ❌ — YAZILACAK**
- %0.30 sabit swap fee
- %20 → creator (24 saatte 1 toplu, net gönderim)
- %80 → kasa
- Bot modu (100+ işlem): %0.15'e düşer, aynı oranlar korunur
- Transfer ücreti creator alacağından düşülür

**5. Bot Sayaç Sistemi: YOK ❌ — YAZILACAK**
- Cüzdan başına günlük işlem sayacı
- 00:00 UTC'de otomatik sıfırlama
- 100. işlemden itibaren indirimli fee modu

**6. Creator Ödeme Motoru: YOK ❌ — YAZILACAK**
- 24 saatte 1 toplu ödeme
- Tüm pool'lardan biriken fee hesaplanır
- Net miktar creator cüzdanına otomatik gönderilir

**7. MEV Korumasının DEX'e Entegrasyonu: KONTROL EDİLECEK ⚠️**
- Mevcut IBE/batch auction sistemi DEX swap işlemleriyle entegre edilmeli
- `nova-mempool` → `nova-dex` bağlantısı kurulacak
- Swap işlemleri de şifreli mempool üzerinden geçmeli

**8. nova-dex Crate'i: YOK ❌ — OLUŞTURULACAK**
- `crates/nova-dex/` klasörü
- Cargo.toml bağımlılıkları: nova-primitives, nova-mempool, nova-runtime
- Tüm yukarıdaki modüller burada toplanacak

### Kalite Hedefi

- Sıfır derleme hatası
- Her modül için birim testleri
- Entegrasyon testleri (swap → fee → creator ödeme → liq kilit)
- `cargo test --workspace` → 0 FAILED
- Güvenlik audit: `nova-security-audit` genişletilecek (DEX modülü eklenecek)
- Seviye: A+++ — mainnet kalitesi

## 12. SIRADAKI ADIMLAR (Uygulama Sırası)

1. `crates/nova-dex/` crate'i oluştur, Cargo.toml yaz
2. AMM core (x*y=k) — fiyat, slippage, price impact
3. Pool factory — oluşturma, süre seçimi, ücret tahsili
4. Likidite kilitleme mekanizması — zincir seviyesi, kesin
5. Fee dağıtım motoru — %0.30, %20/%80 dağılım
6. Bot sayaç sistemi — günlük 100+ işlem indirimi
7. Creator ödeme motoru — 24 saatlik toplu gönderim
8. MEV entegrasyonu — swap'ları şifreli mempool'a bağla
9. `nova-security-audit` DEX modülü — rug/MEV/fee testleri
10. Testnet deploy ve tam entegrasyon testi
