# Nova Shield Chain — Master Context Belgesi
# Araştırma Agentleri İçin Proje Tanımı
**Tarih:** 2026-07-22

---

## 1. PROJENİN ÖZÜ

Nova Shield Chain, kripto varlıkları için tasarlanmış bir **Layer 1 güvenlik blockchain'i**dir.
Ana misyon: "Kayıp yok, hack yok, miras kayıpsız."

Hedef kullanıcı: Kripto varlığı olan ama seed phrase kaybetme, hack, ölüm gibi riskleri
yönetemeyen herkes.

Temel değer önerisi:
- Özel anahtarını kaybetsen bile varlığına erişebilirsin (Guardian MPC)
- Hesabın hacklense bile dondurabilirsin (Freeze koruması)
- Ölürsen varlıkların otomatik mirasçılara geçer (Deadman switch)
- İşlemlerini gizleyebilirsin (ZK Shield)
- Varlıklarını takas edebilirsin (DEX + Batch Auction)
- Bitcoin, ETH, Solana, Cosmos varlıklarını bu zincirden yönetebilirsin (Cross-chain)

---

## 2. NATIVE TOKEN: NSC (Nova Shield Coin)

### Mevcut Kod Değerleri (nova-runtime/src/token/)
```
Toplam Arz:    1,000,000,000 NSC (1 Milyar)
Ondalık:       10^18 (Ethereum standartı)
Slot süresi:   400 ms
1 Yıl:         78,840,000 slot (365 gün * 24 * 3600 / 0.4)
1 Epoch:       216,000 slot (24 saat)
```

### Token Dağılımı (nsc.rs)
```
Ekosistem:    %30
Staking:      %25
Hazine:       %20
Yatırımcı:   %15
Ekip:         %10
```

### Emisyon Modeli (emission.rs)
```
Formül: rate_bps = max(800 - year * 50, 200)
Yıl 0:  %8.0
Yıl 1:  %7.5
Yıl 2:  %7.0
...
Yıl 12+: %2.0 (sabit)
```

### Fee Modeli (burn.rs + fee_dist.rs)
```
Base fee:     %50 burn + %50 validator
Protocol fee: %0.25 DEX/Auction

Protocol fee dağılımı:
  Validator: %30
  Staker:    %30
  Ekosistem: %25
  Hazine:    %15
```

### Staking Parametreleri
```
Min validator stake: 100,000 NSC
Min delegasyon:      100 NSC
Min gov deposit:     1,000 NSC
Max aktif validator: 200
İdeal stake oranı:   %50
İdeal APY:           %5
Unbonding süresi:    21 gün (4,536,000 slot)
```

---

## 3. ANA ÖZELLİKLER

### 3.1 Guardian MPC Sistemi
Kullanıcı, hesabını yönetmek için 3-5 "guardian" (koruyucu) atar.
Bu guardianlar arasında FROST-style Schnorr threshold imzası kullanılır.
- Default: 3/5 threshold
- Guardian olunabilmek için NSC stake edilmeli
- Guardian kategorileri: Personal, CertifiedIndividual(1000 NSC min), Institutional(10k NSC), DAO(5k NSC)
- Guardian trust score: stake*30 + years*25 + success*20 + slash*15 + response*10

### 3.2 Hesap Kurtarma
Seed phrase yerine, guardian threshold imzasıyla hesap kurtarma.
- Kullanıcı özel anahtarı kaybeder → guardianlar oylama yapar → yeni anahtar üretilir
- GuardianCategory: Personal, Institutional, Device, HardwareKey, CloudBackup

### 3.3 ZK Shield (Gizlilik)
Zcash/Penumbra tarzı shielded transaction sistemi.
- 3 mod: FullShield (gas: 200,000), SemiShield (gas: 80,000), Standard (gas: 21,000)
- Not (note) = value + asset_id + randomness + transmission_key
- Nullifier: PRF(nullifier_key, commitment) → çift harcama koruması
- Key hierarchy: SpendingKey → FullViewingKey → IncomingViewingKey / OutgoingViewingKey
- FMD (Fuzzy Message Detection): precision 1..=24, default=4
- ZK kanıt sistemi: Groth16 + Plonk (henüz mock, arkworks entegrasyonu gerekli)
- Note commitment tree: MAX_TREE_SIZE = 2^20 (şu an), Penumbra 2^48 kullanıyor

### 3.4 Dondurma (Freeze) Koruması
Hesap anomali tespit edildiğinde veya kullanıcı/guardian tarafından dondurulabilir.
- 5 anomali türü: UnusualTransferVolume, TransferToUnknownAddress, RapidMultipleWithdrawals, GuardianReport, GeographicAnomaly
- Anomali skoru: hacim*3500 + bilinmeyen_alıcı*2500 + yüksek_frekans*2500 + çok_alıcı*1500 bps
- Dondurma sonrası: Receive ve Unfreeze izin verilen tek işlemler
- Çözme: UserIdentityProven (threshold imza) / AttackConfirmed (eski+yeni guardian) / TimedOut(72h)→Miras

### 3.5 Deadman Switch (Miras)
Kullanıcı belirli süre aktif sinyal göndermezse miras planı tetiklenir.
- Min inaktivite: 30 gün
- Timeline: PreWarning (son 30g) → FinalWarning (son 7g) → GracePeriod (7g-365g) → StagedRelease
- Duress modu: Baskı altında özel heartbeat → guardian alarmı, dışarıya normal görünür
- Max 15 varis, 4 tier (Primary/Secondary/Tertiary/TreasuryBurn)
- Max 20 staged release adımı, max 10 koşullu kural

### 3.6 DEX — Batch Auction (CoW tarzı)
CoW Protocol'den ilham, Nova'ya özgü adaptasyon.
- Uniform Price Auction (UPA): aynı slottaki tüm emirler tek clearing price'ta kapanır
- MEV koruması: İBE şifreli emirler → slot kapanana kadar içerik gizli
- Solver rekabeti: VCG payment mekanizması
- OrderUid: 56-byte (CoW Protocol formatı)
- OrderClass: Market, Limit, LimitPartialFill
- Dutch auction: NSC/kripto satışı için lineer fiyat düşüşü
- EKSIK: Coincidence of Wants (çapraz çift netting) henüz implemente edilmedi

### 3.7 Cross-Chain Guard
Bitcoin, Ethereum, Solana, Cosmos varlıklarını Nova Shield'dan yönet.
- Her zincir için MPC vault adresi
- Guardian threshold imzasıyla cross-chain TX
- Yıllık koruma ücreti: %0.05 (5 bps)
  - BTC/ETH carpanı: %100
  - SOL carpanı: %80
  - Cosmos carpanı: %60
- Fee dağılımı: Staker %60, Guardian %40
- IBC ICS-20 (Cosmos), BEEFY light client (Ethereum/Polkadot)

### 3.8 Guardian Marketplace
Guardian'ların hizmet sunduğu pazar yeri.
- Listing: trust_score, SLA terms, available_slots
- Offer: 1 hafta geçerli
- Contract: 1 yıl
- Sigorta tazminat oranları:
  - RecoveryRefused: %100
  - WrongRecoveryAttempt: %75
  - GuardianUnresponsive: %50
  - SlaViolation: %25

---

## 4. KONSENSÜs MİMARİSİ

### Seçilen Motorlar (henüz entegre değil)
1. **Simplex BFT** (Avalanche ilhamlı): Ana blok üretim motoru
   - Quorum: (2n+2)/3, n=200 validator için 134
2. **GRANDPA** (Polkadot ilhamlı): Finality gadget
   - 2 round: Prevote + Precommit
3. **Snowball** (Avalanche): Çatal (fork) kararı
   - K=20, α=15, β_virtuous=15, β_rogue=20

### Validator Sistemi
- Max 200 aktif validator
- Seçim: Sequential Phragmén (NPoS)
- Slashing:
  - Double sign: %100 (kalıcı tombstone)
  - Mempool leak: %50
  - Wrong batch clearing: %25
  - Extended offline: %1

### MEV Koruması
- Emirler: IBE (Identity-Based Encryption) ile şifreli
- Sıralama: commitment_hash lexicographic → validator emirleri göremez
- Eon key: Epoch başında threshold key paylaşımı (Rolling Shutter pattern)

---

## 5. AĞ MİMARİSİ

### P2P (Henüz Mock)
- Planlanan: libp2p + Noise XX (X25519 + AES-256-GCM)
- Gossip: Solana CRDS pattern (PUSH_FANOUT=9)
- Block propagation: Turbine/Shred (SHRED_DATA_PAYLOAD_SIZE=1232 byte)
- Erasure coding: 32+32 (planlanan Reed-Solomon)
- Discovery: Kademlia DHT (K=20)

### XCM ve Köprüler
- XCM: MAX_INSTRUCTIONS=32, NOVA_PARACHAIN_ID=1000
- IBC: port="transfer", version="ics20-1"
- BEEFY: quorum=2/3+1, MAX_COMMITMENTS=256

---

## 6. MEVCUT KOD DURUMU (2026-07-22 İtibarıyla)

### Tamamen Çalışan
- nova-mpc: FROST threshold imza, 7534 satır, end-to-end testler geçiyor
- nova-account: Guardian sistemi, heartbeat, recovery
- nova-freeze: Anomali tespiti, unfreeze mekanizması
- nova-inheritance: Miras planı, staged release, conditional rules
- nova-mempool: Threshold ECDH MEV koruması, Gulf Stream

### Kısmen Çalışan
- nova-runtime: Token/staking/governance algoritmaları tam AMA executor bağlanmamış
- nova-consensus: 3 motor bağımsız çalışıyor AMA birbirine bağlanmamış
- nova-guardian: Trust score, marketplace AMA 4 kritik bug var
- nova-auction: Batch auction AMA CoW feature yok, 2 bug

### Tamamen Mock/Stub
- nova-zk: Hiç gerçek ZK kanıtı yok (BLAKE3 ile simüle)
- nova-vm: wasmtime/revm yok, EVM uyumsuz
- nova-network: libp2p yok, Noise XX mock, Reed-Solomon mock
- nova-node: BFT stub, P2P stub
- nova-rpc: HTTP/WS server yok

### Kritik Bug Listesi
1. Threshold encrypt/decrypt simetrisi bozuk
2. DilithiumPublicKey::SIZE: 1312 vs 1952 (gerçek: 1952)
3. VrfVerifier::verify stub (always Err)
4. TX imzası hiç doğrulanmıyor
5. TX hash içerik-bağımsız (to/amount dahil değil)
6. Konsensüs epoch 2400 slot vs 216000 slot çakışması
7. Staking modül adresi çakışması [0x53] vs [0x10]
8. Guardian otomatik ödeme stub (success=true hardcode)
9. Guardian sigorta toplu ödeme çalışmıyor
10. Guardian marketplace fee filtresi uygulanmamış
11. Guardian offer süresi yanlış (21600 slot = 2.4 saat, 1 hafta değil)
12. EVM function_selector BLAKE3 (keccak256 olmalı)
13. Reed-Solomon: XOR mock (gerçek RS değil)
14. ETH/SOL/Cosmos vault adresi guardian içermiyor
15. Dec::mul hassasiyet hatası
16. CoW feature (çapraz çift netting) yok
17. ZK Merkle tree iki şema uyumsuz
18. Miras distribute hesabı yanlış
19. eon_key secp256k1[33] vs IBE Ristretto255[32] uyumsuzluk

---

## 7. AJANLAR İÇİN GÖREV TANIMI

Sen bu projenin araştırmacısısın. Sana verilen zincirin TÜM analiz dosyalarını okuyacaksın.

Amacın: Nova Shield Chain'in her özelliği için bu zincirden ne öğrenebileceğimizi
sanki sıfırdan tasarlıyormuşuz gibi, son derece kapsamlı şekilde çıkarmak.

Her bölümde şunları belirt:
- Bu zincirden alınacak exact formül, parametre, değer
- Nova Shield'ın özelliğiyle nasıl birleştirilmeli
- Mevcut koddaki hangi eksiklikleri gideriyor
- Uygulama adımları (sıralı, somut)
- Hangi Rust crate'leri kullanılmalı
- Güvenlik açıkları ve dikkat edilecekler

UYARI: Bu belgeyi okuduktan sonra, verilen zincirin TÜM analiz dosyalarını
(`research/analysis/<zincir>/`) SATIR SATIR okuyacaksın. Hiçbir dosyayı atlama.
Her dosyadan Nova Shield için spesifik değer, formül veya ders çıkar.
