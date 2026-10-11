# Nova Verified Launchpad — Proje Sahiplerine Mesaj

---

## Kısa Versiyon (Twitter/X, Telegram duyuruları için)

> Every chain lets anyone launch anything. We don't.
>
> Nova Shield Launchpad is invite-only by merit. Pass our audit. Lock your liquidity. Prove your project is real.
>
> In return: your token launches on the only chain where bots physically cannot front-run your community. No sniper. No sandwich. No insider advantage. Every buyer — from the first second — gets the same price.
>
> This is not a feature. This is the protocol.
>
> **Nova Verified. The last launchpad your project will ever need.**

---

## Uzun Versiyon (Landing page, Medium, başvuru sayfası için)

---

### You've been robbed. Every single launch.

You spent months building. You wrote the whitepaper. You built the community. You stayed up until 3am debugging contracts.

Then launch day came.

In the first 30 seconds, before a single real person could buy, bots had already taken 40–60% of your token supply. Your community bought at the top. Price dumped. They blamed you. Some left. Some called it a rug. You watched something you built get destroyed by automated theft you had no way to stop.

This happens on every chain. Ethereum. Solana. BSC. Every single one.

**Not here.**

---

### Nova Shield Chain was built differently.

Our encrypted mempool means no bot, no validator, no insider can see a single transaction before it executes. When your token launches, every buy order enters encrypted. The slot closes. Every order is revealed and executed simultaneously at a single clearing price.

There is no "first buyer."
There is no front-run.
There is no sandwich.
There is no sniper.

Your community gets in at the same price as everyone else. Not because we ask bots to be nice. Because the mathematics of our protocol make it impossible for them to be anything else.

---

### But we don't let everyone in.

Nova Shield Launchpad is not a token factory. We don't list 500 projects a day. We list 2 or 3 — and every single one passes our verification process first.

**What we require:**

- KYC from at least one founder (kept private, never published)
- A real whitepaper — minimum 10 pages, real utility, real architecture
- Liquidity locked on-chain for a minimum of 12 months — no exceptions, no early exits
- Team tokens with a 12-month cliff and linear vesting
- A code audit — by our team or an approved independent firm

**What we score you on:**

- Working demo or MVP (+30 points)
- Independent security audit report (+25 points)
- Open source repository (+20 points)
- Credible advisors (+15 points)
- Clear and measurable roadmap (+10 points)

Score 70 or above: **Accepted. No listing fee.**
Score 50–69: **Conditional. Minor requirements, 500 NSC fee.**
Below 50: **Not yet. Come back when it's real.**

---

### What you get in return.

This is not a one-way relationship.

**Technical:**
- Free devnet environment for testing before mainnet
- Smart contract templates and integration guides
- Full Nova SDK support from our team

**Visibility:**
- The Nova Verified badge — displayed on our explorer, our launchpad, and every official Nova channel
- A dedicated launch announcement across all Nova platforms
- Featured placement on the launchpad homepage for 30 days after launch

**The unfair advantage:**
- Your project lives on the only chain where your community cannot be front-run
- Your dev wallet is protected by MPC — no seed phrase means no private key theft, no "dev got hacked" story
- Every transaction on Nova is post-quantum secured from day one

---

### The math is simple.

On every other chain, your launch day is your most vulnerable day.

On Nova Shield Chain, your launch day is the first day your community actually has a fair chance.

If you've built something real, we want you here.

**Apply for Nova Verified Launchpad.**

---

*Nova Shield Chain — Built for humans, not bots.*

---

## Başvuru Sayfası Tagline Seçenekleri

1. **"The only chain where your community comes first. Mathematically."**
2. **"We don't let everyone in. That's the point."**
3. **"Launch fair. Or don't launch here."**
4. **"Your community deserves the same price as the first buyer. Now they can have it."**
5. **"Nova Verified: Audited. Locked. Fair."**

---

---

# X RAKIP ANALIZI VE ENGAGEMENT STRATEJI PROMPTU

> Bu bölüm başka bir AI'ya verilecek hazır prompt metnidir.
> Kopyala → yapıştır → çalıştır.

---

## PROMPT (Başka Bir AI İçin — Tam Metin)

---

Sen bir kripto/blockchain X (Twitter) büyüme analistisin. Görevin aşağıda tanımlanan Nova Shield Chain projesini temsil ederek rakip X hesaplarını araştırmak, son postlarını analiz etmek ve yüksek engagement potansiyelli postlar için hazır yorum/repost stratejileri üretmek.

---

### BÖLÜM 1 — NOVA SHIELD CHAIN PROFİLİ (Karşılaştırma Referansı)

**Proje:** Nova Shield Chain
**Tür:** Layer 1 Blockchain — Native Rust
**Token:** NOVA (NSC)
**Chain ID:** 1984 (mainnet), 19840 (testnet)
**Blok Süresi:** 400ms
**Consensus:** BFT (Byzantine Fault Tolerant)
**Codebase:** ~100,000+ satır Rust, 23 crate, 1 geliştirici, 0 VC fonu
**Testler:** 3,588 test geçti, 0 başarısız
**Mainnet:** CANLI — 5 validator, chain_id=1984

**Kritik Teknik Özellikler (Rakiplerle Karşılaştırma Temeli):**

1. **MPC Native Account Model (FROST-style Schnorr Threshold Signing)**
   - Seed phrase YOK. Özel anahtar tek bir yerde SAKLANMAZ.
   - 3/5 threshold: 5 muhafızdan 3'ü onaylarsa işlem geçer.
   - "Dev cüzdanı hack'lendi" senaryosu matematiksel olarak imkânsız.
   - Rakiplerde: Ethereum, Solana, BSC, Avalanche, Sui — hepsi tek anahtar / seed phrase modeli.

2. **3 Katmanlı MEV Koruması**
   - Katman 1: Şifreli mempool (IBE — Identity-Based Encryption)
   - Katman 2: Sealed-bid batch auction (tüm emirler aynı temizleme fiyatında işlenir)
   - Katman 3: DEX commit-reveal scheme (BLAKE3 commitment hash, sender dahil)
   - Rakiplerde: Ethereum → Flashbots kısmen çözer ama validator tarafında hâlâ MEV var. Solana → Jito mevcut ama opsiyonel. Sui → object model yardımcı olur ama tam değil.

3. **Native Miras Sistemi (Deadman Switch)**
   - Blokchain protokol seviyesinde kalıtım: X gün heartbeat gelmezse varlıklar mirasçıya geçer.
   - Duress koruması: baskı altında girildiğinde sahte işlem gerçekleştirir, gerçek varlıklar korunur.
   - Rakiplerde: HİÇBİRİ protokol seviyesinde sunmuyor. Akıllı kontrat workaround'ları var ama L1 garantisi yok.

4. **DEX Likidite Kilidi (Protokol Seviyesi Rug Prevention)**
   - Likidite LockDuration enum ile kilitlenir: 1 gün / 7 gün / 30 gün / 1 yıl.
   - Kilit süresi dolmadan remove_liquidity çağrısı BLOKLANIR — kontrat değil, runtime seviyesi.
   - NOVA-DEX-LAUNCH-001: Pool oluşturulduktan sonra ilk 5 dakika her adres max %5 token alabilir (creator dahil).
   - Bu "snipe & dump" saldırısını matematiksel olarak engeller.
   - Rakiplerde: Uniswap, Raydium, PancakeSwap — likidite kilidi isteğe bağlı, protokol zorunluluğu yok.

5. **Gerçek ZK (Groth16 BLS12-381, arkworks)**
   - SpendCircuit, R1CS devreler, LightClient ZK doğrulaması.
   - Mock/dummy proof YOK — gerçek kriptografi.
   - Rakiplerde: zkSync, StarkNet ZK kullanır ama EVM uyumluluğu için ödün verir. Nova sıfırdan native.

6. **Post-Quantum Hazırlığı**
   - Noise XX (X25519 + HKDF-SHA256 + ChaCha20-Poly1305) P2P katmanında.
   - sr25519 VRF, KZG polynomial commitment.
   - Rakiplerde: Ethereum, Solana — ECDSA/secp256k1, kuantum tehdidine açık.

7. **Tokenomics (200M sabit arz, deflasyonlu)**
   - Yeni token ihracı YOK. Her işlemin %50'si yakılır.
   - Seed: $1.00/NOVA, $20M raise, $200M FDV.
   - Series A: $2.14/NOVA, $30M raise, $428M FDV.
   - Staking APY: %8–12 (yarılanma 10 yıl).
   - Rakiplerde: Ethereum (enflasyonlu model, EIP-1559 ile kısmen deflasyonlu), Solana (enflasyonlu), BSC (sabit arz ama burn yok).

8. **Verified Launchpad**
   - KYC zorunlu, gerçek whitepaper şartı, 12 ay likidite kilidi zorunlu, kod audit şartı.
   - Skor 70+ → kabul, liste ücreti yok. 50-69 → koşullu, 500 NSC ücret. 50 altı → ret.
   - Rakiplerde: PinkSale, DxSale — ücret al, liste, sorumluluk alma modeli.

**X Hesabı:** @NovaShieldChainOfficial
**Website:** novashieldchain.io
**Slogan:** "Built for humans, not bots."

---

### BÖLÜM 2 — ARAŞTIRILACAK TOP 20 RAKİP X HESABI

Aşağıdaki projelerin resmi X hesaplarını bul (varsa). Her biri için:
- Resmi X handle'ını doğrula
- Son 20 postunu çek (retweet hariç, kendi orijinal postları)
- Tarih, içerik özeti, tahmini etkileşim seviyesi (like/repost/yorum sayısı) kaydet

**Araştırılacak projeler:**

1. Ethereum (Ethereum Foundation)
2. Solana
3. Avalanche (Avax)
4. Polkadot
5. Cosmos (ATOM)
6. Sui (Sui Network)
7. Aptos
8. Near Protocol
9. Arbitrum
10. Optimism
11. zkSync
12. StarkNet
13. Cardano
14. Algorand
15. Fantom (Sonic)
16. Sei Network
17. Monad
18. Berachain
19. Movement Labs
20. Hyperliquid

---

### BÖLÜM 3 — HER POST İÇİN ANALİZ ÇERÇEVESI

Her post için şu soruları yanıtla:

**A. İçerik Kategorisi:**
- Teknik duyuru (yeni özellik, upgrade, testnet/mainnet)
- Topluluk/marketing (milestone, kullanıcı sayısı, TVL)
- Ekosistem (ortaklık, entegrasyon, grant)
- FUD/eleştiri yanıtı
- Genel kripto haberi yorumu
- Eğitim içeriği

**B. Nova Shield Chain Bağlantı Puanı (1-10):**
- 9-10: Doğrudan bizimle karşılaştırılabilir, rakip zayıflık açıkça görünüyor
- 7-8: Nova'nın çözdüğü bir problemi dile getiriyor
- 5-6: Genel kripto konusu, Nova ile ilişkilendirilebilir
- 3-4: Uzak bağlantı, zorla çekilebilir
- 1-2: İlgisiz

**C. Yorum Stratejisi (Puan 7+ için):**
Şu formatlardan birini seç:
- **"Biz çözdük"** → Nova'nın teknik çözümünü kısaca belirt
- **"Aynı fikirdeyiz + daha ileri gittik"** → Rakibin iyi noktasını onayla, Nova'yı ekle
- **"Soru sor"** → Topluluğu düşündürecek, Nova'yı ima eden soru
- **"Veri ile konuş"** → Somut Nova rakamları (3,588 test, 400ms blok, 0 seed phrase)
- **"Destekle ve tag at"** → Genel destek + @NovaShieldChainOfficial etiket

**D. Hazır Yorum Metni:**
Her seçilen post için 2 farklı versiyon:
- **Kısa (max 280 karakter):** X'te direkt yorum olarak kullanılabilir
- **Uzun (max 560 karakter, thread için):** Daha detaylı, 2 tweet'lik thread başlangıcı

**E. Zamanlama Önerisi:**
- Postin yayınlanmasından sonra kaç saat içinde yorum atılmalı (ilk 2 saat en yüksek görünürlük)
- Haftanın hangi günü/saatinde paylaşılmalı (UTC)

---

### BÖLÜM 4 — X ALGORİTMASI OPTİMİZASYONU KURALLARI

Tüm üretilen metinlerde şu kurallara uy:

1. **Hashtag:** Yorum başına max 2 hashtag. Thread sonuna max 3. #NovaShieldChain #NSC #DeFi #Web3 #Blockchain arasından seç. Hepsini birden KULLANMA.
2. **Mention:** Yorum içinde karşı hesabı @mention ET — algoritmik amplifikasyon için kritik.
3. **Soru ile bitir:** En az %40 yorumda soru işaretiyle biten cümle ekle — yorum çekme için.
4. **Rakam kullan:** Soyut iddia yerine somut sayı. "Çok hızlı" değil "400ms block time". "Güvenli" değil "3,588 tests, 0 failed".
5. **İngilizce yaz:** Tüm X yorumları İngilizce. Türkçe yalnızca Türk topluluklarına özel hesaplarda.
6. **Emoji:** Maksimum 1-2 emoji. Terminal/kod ekranı emoji kullanma. ✅ ⚡ 🔐 🧱 tercih.
7. **İlk 2 kelime güçlü olsun:** "Actually," / "Hot take:" / "This is why" / "Real question:" / "We solved this." gibi başlangıçlar.
8. **Link ekleme:** Yorumlarda link eklemek algoritmayı düşürür. Sadece thread son tweetine ekle.

---

### BÖLÜM 5 — ÇIKTI FORMATI

Her rakip proje için şu yapıyı kullan:

```
## [Proje Adı] — @[handle]

### Post #[1-20]
**Tarih:** [YYYY-MM-DD]
**İçerik Özeti:** [1-2 cümle]
**Etkileşim (tahmini):** [like/repost/yorum]
**Kategori:** [A'dan kategori]
**Nova Bağlantı Puanı:** [1-10]

[Puan 7+ ise:]
**Yorum Stratejisi:** [strateji tipi]
**Kısa Yorum (≤280):**
> [metin]

**Uzun Thread Başlangıcı (≤560):**
> Tweet 1: [metin]
> Tweet 2: [metin]

**Zamanlama:** Post yayınından [X] saat içinde, [gün] [saat] UTC tercih.
```

---

### BÖLÜM 6 — 7 GÜNLÜK ENGAGEMENT TAKVİMİ

Analiz tamamlandıktan sonra şu formatta haftalık plan oluştur:

| Gün | Saat (UTC) | Hedef Hesap | Post Özeti | Aksiyon | Hazır Metin |
|-----|-----------|-------------|------------|---------|-------------|
| Pzt | 09:00 | @ethereum | ... | Yorum | "..." |
| Pzt | 18:00 | @solana | ... | Repost + Yorum | "..." |
| Sal | ... | ... | ... | ... | ... |
... (7 gün dolana kadar devam)

**Günde max 5 etkileşim** (X spam filtresi için).
**Her gün en az 1 farklı proje hedefle** (çeşitlilik algoritma için olumlu).
**En güçlü engagement fırsatlarını Salı-Perşembe 14:00-20:00 UTC'ye denk getir** (kripto kitlesi en aktif saat).

---

### BÖLÜM 7 — ÖZEL NOTLAR

- Nova Shield Chain hiçbir zaman rakiplere saldırgan ton kullanmaz. Ton: güvenli, teknik, saygılı.
- "X daha kötü" değil, "biz bunu farklı çözdük" çerçevesi.
- Sıfır VC, sıfır fon, 1 geliştirici gerçeği — ama bunu pazarlama aracı olarak SADECE genel post'larda kullan, yorum thread'lerinde değil. Oraya teknik gücü taşı.
- Eğer bir post Nova ile hiç ilgisizse ve puan 6 altındaysa → atlat, zaman harcama.
- Repost stratejisi: Sadece gerçekten değerli teknik içerikleri repost et + kısa yorum ekle. Boş repost yapma.

---

### BAŞLA

Yukarıdaki çerçeveyi kullanarak:
1. Her projenin resmi X handle'ını doğrula.
2. Her projenin son 20 orijinal postunu çek.
3. Her post için analiz çerçevesini uygula.
4. Puan 7+ olan tüm postlar için hazır yorum metinleri üret.
5. 7 günlük engagement takvimini oluştur.
6. Sonuçları Bölüm 5'teki formatta sun.

Başlamadan önce hangi projenin hangi X handle'ı kullandığını listele ve onay iste — handle'larda hata varsa tüm analiz yanlış olur.

---

*Bu prompt Nova Shield Chain X büyüme stratejisi için hazırlanmıştır. Son güncelleme: 2026-10-10.*
