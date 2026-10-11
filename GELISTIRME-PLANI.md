# Nova Shield Chain — Geliştirme Planı
## Sıfırdan 200.000+ Satır Koda: Eksiksiz Teknik Yol Haritası

**Tarih:** 2026-07-17
**Temel:** WHITEPAPER.md (1607 satır) + 10 Repo Analizi (5.3M satır rakip kod)
**Hedef:** Mainnet 2027 Q4

---

## İçindekiler

1. Teknoloji Seçimleri — Kesinleşenler
2. Proje Dizin Yapısı — Tam Ağaç
3. Crate Listesi ve Satır Tahminleri
4. Bileşen-Whitepaper Eşleştirmesi
5. Faz 1: Kriptografik Temel (2026 Q3–Q4)
6. Faz 2: Çekirdek Konsensüs (2026 Q4–2027 Q1)
7. Faz 3: Şifreli Mempool + Auction (2027 Q1)
8. Faz 4: MPC Hesap Modeli (2027 Q1–Q2)
9. Faz 5: Miras + Freeze Protokolleri (2027 Q2)
10. Faz 6: Nova VM + Akıllı Kontrat (2027 Q2–Q3)
11. Faz 7: ZK-Shield (2027 Q3)
12. Faz 8: Guardian Marketplace (2027 Q3)
13. Faz 9: Cross-Chain Guard (2027 Q4)
14. Faz 10: Devnet → Testnet → Mainnet
15. Test Stratejisi
16. Formal Doğrulama Planı
17. Satır Sayısı Tablosu

---

## 1. Teknoloji Seçimleri — Kesinleşenler

### Programlama Dili
| Bileşen | Dil | Neden |
|---------|-----|-------|
| Çekirdek | Rust | Bellek güvenliği, performans, no_std, 10 repo de Rust |
| Akıllı Kontrat | Rust → WASM | Nova VM hedefi |
| EVM Katmanı | Rust (revm) | Ethereum uyumluluğu |
| CLI Araçları | Rust | Tutarlılık |
| Formal Doğrulama | TLA+, Coq | Matematiksel kanıt |

### Kritik Kütüphaneler

| İşlev | Kütüphane | Kaynak (analiz) | Whitepaper referansı |
|-------|-----------|-----------------|---------------------|
| MPC/TSS | `cggmp21` (ZenGo Rust) | multi-party-ecdsa | §5.1.3, §6.1.1 |
| Threshold Şifreleme | `shcrypto` (BLS12-381 IBE) | rolling-shutter | §5.2.2, §6.1.4 |
| Pedersen DKG | `frost-secp256k1` + özel | rolling-shutter | §6.1.4 |
| BLS İmzası | `blst` (Supranational) | solana, polkadot | §5.11, §5.10 |
| ZK Kanıt | `arkworks` (Groth16/PLONK) | penumbra | §5.7.3 |
| Hash | `blake3` | — | §6.1.3 |
| Post-Quantum | `pqcrypto-dilithium` (Dilithium3) | — | §6.1.2 |
| P2P | `libp2p` + gossipsub | rolling-shutter, cosmos | §5.12 |
| Kriptografi Primitifleri | `k256`, `p256`, `curve25519-dalek` | multi-party-ecdsa | §6.1 |
| Depolama | `rocksdb` (Rust binding) | go-ethereum, cosmos | State DB |
| Serialization | `borsh` (birincil) + `parity-scale-codec` | solana, polkadot | Protocol |
| WASM Runtime | `wasmtime` | — | §9.1 Nova VM |
| EVM Runtime | `revm` | cow-protocol | §9.1 EVM uyumluluk |
| Async Runtime | `tokio` | — | Ağ katmanı |
| Shard-safe HashMap | `dashmap` | solana | Paralel state |
| Rayon | `rayon` | solana | Paralel execution |

---

## 2. Proje Dizin Yapısı — Tam Ağaç

```
nova-shield-chain/
├── Cargo.toml                    # Workspace root
├── Cargo.lock
├── rust-toolchain.toml           # Rust versiyonu sabitlenir
├── .cargo/config.toml            # Linker, target ayarları
│
├── crates/
│   │
│   ├── # ─── KRİPTOGRAFİK TEMEL ───────────────────────────────────
│   │
│   ├── nova-crypto/              # Tüm kriptografik primitifler
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── hash.rs           # BLAKE3 wrapper + Merkle tree
│   │   │   ├── commitment.rs     # Pedersen commitment şeması
│   │   │   ├── signatures/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── ecdsa.rs      # secp256k1 ECDSA (CGGMP21 uyumlu)
│   │   │   │   ├── dilithium.rs  # Dilithium3 post-quantum imza
│   │   │   │   ├── hybrid.rs     # ECDSA + Dilithium hibrit (§6.1.2)
│   │   │   │   └── bls.rs        # BLS12-381 imza (blst)
│   │   │   ├── vrf.rs            # VRF (leader seçimi için)
│   │   │   ├── zk/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── groth16.rs    # Groth16 proof sistemi (arkworks)
│   │   │   │   ├── plonk.rs      # PLONK proof sistemi
│   │   │   │   ├── circuits/
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   ├── transfer.rs    # Gizli transfer devresi
│   │   │   │   │   ├── nullifier.rs   # Nullifier devresi
│   │   │   │   │   └── balance.rs     # Balance devresi
│   │   │   │   ├── commitment.rs  # ZK commitment şeması
│   │   │   │   └── nullifier.rs   # Nullifier üretimi ve doğrulama
│   │   │   ├── threshold/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── elgamal.rs    # ElGamal threshold şifreleme
│   │   │   │   ├── dkg.rs        # Pedersen DKG
│   │   │   │   └── decryption.rs # Threshold decryption
│   │   │   └── constant_time.rs  # Constant-time operasyonlar
│   │   └── tests/
│   │       ├── hash_tests.rs
│   │       ├── signature_tests.rs
│   │       ├── zk_tests.rs
│   │       └── threshold_tests.rs
│   │
│   ├── nova-mpc/                 # MPC / TSS çekirdeği (§5.1)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── keygen/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── round1.rs     # DKG round 1: commitment
│   │   │   │   ├── round2.rs     # DKG round 2: shares
│   │   │   │   ├── round3.rs     # DKG round 3: verification
│   │   │   │   └── round4.rs     # DKG round 4: key finalization
│   │   │   ├── signing/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── presign.rs    # CGGMP21 non-interactive pre-signing (§5.9.2)
│   │   │   │   ├── round1.rs
│   │   │   │   ├── round2.rs
│   │   │   │   ├── round3.rs
│   │   │   │   ├── round4.rs
│   │   │   │   └── finalize.rs
│   │   │   ├── resharing/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── round1.rs     # Key share yeniden dağıtımı
│   │   │   │   ├── round2.rs     # (Proactive secret sharing - §6.2.3)
│   │   │   │   └── round3.rs
│   │   │   ├── account.rs        # MPC hesap yapısı (§5.1.2)
│   │   │   ├── guardian.rs       # Guardian yönetimi (§5.1.5)
│   │   │   ├── recovery.rs       # Sosyal kurtarma (§5.1.4)
│   │   │   ├── threshold.rs      # Threshold parametreleri (2/3 - 10/15)
│   │   │   ├── identifiable_abort.rs # CGGMP21 abort detection
│   │   │   └── error.rs
│   │   ├── benches/
│   │   │   ├── keygen_bench.rs
│   │   │   └── signing_bench.rs
│   │   └── tests/
│   │       ├── keygen_tests.rs
│   │       ├── signing_tests.rs
│   │       ├── resharing_tests.rs
│   │       └── recovery_tests.rs
│   │
│   ├── # ─── KONSENSÜS ──────────────────────────────────────────────
│   │
│   ├── nova-consensus/           # Simplex BFT + DPoS (§5.10)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── types.rs          # Block, Slot, Epoch, ValidatorId
│   │   │   ├── bft/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── simplex.rs    # Simplex BFT (Avalanche'tan: 2-round finality)
│   │   │   │   ├── vote.rs       # BLS vote struct + aggregation
│   │   │   │   ├── round.rs      # Round yönetimi
│   │   │   │   ├── quorum.rs     # 2/3 quorum hesabı
│   │   │   │   └── finality.rs   # Single-slot finality (400ms)
│   │   │   ├── dpos/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── validator_set.rs  # Validator seti (200 → 1000)
│   │   │   │   ├── election.rs       # Epoch validator seçimi
│   │   │   │   ├── stake.rs          # Stake ağırlığı hesabı
│   │   │   │   └── phragmen.rs       # Phragmén adil dağılım (polkadot'tan)
│   │   │   ├── block/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── header.rs     # Block header (parent_hash, state_root, ...)
│   │   │   │   ├── body.rs       # Block body (encrypted commits, decrypted txs)
│   │   │   │   ├── producer.rs   # Blok üretici (slot leader)
│   │   │   │   └── verifier.rs   # Blok doğrulama
│   │   │   ├── leader/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── schedule.rs   # Leader schedule (epoch-bazlı)
│   │   │   │   ├── vrf.rs        # VRF tabanlı slot ataması
│   │   │   │   └── rotation.rs   # Round-robin + VRF fallback
│   │   │   ├── epoch/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── manager.rs    # 24 saatlik epoch yönetimi
│   │   │   │   ├── transition.rs # Epoch geçiş protokolü
│   │   │   │   └── rewards.rs    # Epoch ödül hesabı (polkadot formülü)
│   │   │   ├── slashing/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── double_sign.rs    # Çift imza tespiti → %100 slash
│   │   │   │   ├── mempool_leak.rs   # Mempool sızıntısı → %50 slash
│   │   │   │   ├── offline.rs        # Offline → günlük %0.1 slash
│   │   │   │   ├── wrong_batch.rs    # Yanlış batch price → %25 slash
│   │   │   │   ├── graduated.rs      # 3 kademe slashing sistemi
│   │   │   │   └── evidence.rs       # Delil toplama (100 blok pencere)
│   │   │   ├── checkpoint.rs     # Weak subjectivity checkpoint (§6.5.1)
│   │   │   └── diversity.rs      # Coğrafi çeşitlilik kuralları (§6.5.3)
│   │   ├── benches/
│   │   └── tests/
│   │
│   ├── # ─── ŞİFRELİ MEMPOOL ────────────────────────────────────────
│   │
│   ├── nova-mempool/             # Encrypted mempool (§5.2)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── commitment/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── builder.rs    # Commitment oluşturma: H = BLAKE3(TX || nonce || slot)
│   │   │   │   ├── verifier.rs   # Commitment doğrulama (içerik görmeden)
│   │   │   │   └── types.rs      # CommitmentHash, EncryptedTx
│   │   │   ├── encryption/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── elgamal.rs    # ElGamal ile TX şifreleme
│   │   │   │   ├── padding.rs    # Sabit 512-byte padding (§6.4.2)
│   │   │   │   └── ibe.rs        # IBE (Identity-Based Encryption, rolling-shutter'dan)
│   │   │   ├── decryption/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── threshold.rs  # Threshold decryption (2n/3+1)
│   │   │   │   ├── coordinator.rs # Decryption koordinasyonu
│   │   │   │   └── reveal.rs     # Slot kapanınca toplu açma
│   │   │   ├── pool/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── storage.rs    # Şifreli TX deposu
│   │   │   │   ├── validator.rs  # TX doğrulama (şifreli durumdayken)
│   │   │   │   ├── slot_manager.rs # 300ms commit + 100ms reveal
│   │   │   │   └── buffer.rs     # Slot buffer (700K TX kapasitesi benzeri)
│   │   │   ├── timing/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── random_delay.rs # 0-200ms random gecikme (§6.4.1)
│   │   │   │   └── slot_clock.rs   # 400ms slot saati
│   │   │   └── replay_protection.rs # Slot numaralı replay koruması (§5.2.3)
│   │   └── tests/
│   │
│   ├── nova-auction/             # Sealed-bid batch auction (§5.3)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── order/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── types.rs      # BuyOrder, SellOrder, Direction
│   │   │   │   ├── validation.rs # Emir doğrulama
│   │   │   │   └── partial.rs    # Kısmi doldurma (§5.3.4)
│   │   │   ├── clearing/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── price.rs      # Uniform price auction (CoW Protocol'den)
│   │   │   │   ├── matching.rs   # Emir eşleştirme algoritması
│   │   │   │   ├── curve.rs      # Arz/talep eğrisi hesabı
│   │   │   │   └── settlement.rs # Clearing price'a göre settlement
│   │   │   ├── batch/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── collector.rs  # 300ms boyunca emir toplama
│   │   │   │   ├── executor.rs   # Slot açılınca batch execute
│   │   │   │   └── result.rs     # Batch sonucu: fills, refunds
│   │   │   ├── slippage.rs       # Max slippage tolerance kontrolü
│   │   │   └── proof.rs          # Yanlış clearing price delil kanıtı (§6.5.2)
│   │   └── tests/
│   │
│   ├── # ─── MPC HESAP MODELİ ────────────────────────────────────────
│   │
│   ├── nova-account/             # MPC-native hesap modeli (§5.1)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── account/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── types.rs      # NovaAccount { threshold, guardians, state }
│   │   │   │   ├── creation.rs   # Hesap oluşturma (DKG tetikler)
│   │   │   │   ├── address.rs    # Adres türetme (MPC public key → adres)
│   │   │   │   └── state.rs      # Hesap durumu (active, frozen, inheritance)
│   │   │   ├── guardian/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── assignment.rs # Guardian atama TX (7 günlük bekleme)
│   │   │   │   ├── removal.rs    # Guardian kaldırma (§5.1.5)
│   │   │   │   ├── categories.rs # Individual/Certified/Institutional/DAO (§5.8.2)
│   │   │   │   └── change_window.rs # 7 günlük güvenlik penceresi
│   │   │   ├── recovery/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── initiate.rs   # Kurtarma başlatma
│   │   │   │   ├── approve.rs    # Guardian onayı (§5.1.4)
│   │   │   │   ├── window.rs     # 48 saat güvenlik penceresi
│   │   │   │   └── reshare.rs    # Key share yeniden dağıtımı
│   │   │   ├── device/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── registration.rs # Cihaz kayıt
│   │   │   │   ├── revocation.rs   # Uzaktan cihaz iptali (§6.3.3)
│   │   │   │   └── enclave.rs      # Secure Enclave arayüzü (§6.3.1)
│   │   │   └── heartbeat.rs      # Heartbeat mesajı (ücretsiz) (§5.4.9)
│   │   └── tests/
│   │
│   ├── # ─── MİRAS VE FREEZE PROTOKOLÜ ─────────────────────────────
│   │
│   ├── nova-inheritance/         # Deadman switch protokolü (§5.4)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── plan/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── types.rs      # InheritancePlan { triggers, heirs, conditions }
│   │   │   │   ├── builder.rs    # Plan oluşturma API'si
│   │   │   │   ├── validation.rs # Plan doğrulama
│   │   │   │   └── storage.rs    # Plan zincirde depolama
│   │   │   ├── trigger/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── inactivity.rs # Tetikleyici 1: İnaktivite (§5.4.2)
│   │   │   │   ├── scheduled.rs  # Tetikleyici 2: Belirli tarih/blok
│   │   │   │   ├── manual.rs     # Tetikleyici 3: Manuel aktivasyon
│   │   │   │   └── detector.rs   # Tetikleyici tespiti
│   │   │   ├── phases/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── phase1_warning.rs  # 30 gün öncesi uyarı
│   │   │   │   ├── phase2_final.rs    # 7 gün öncesi son uyarı
│   │   │   │   ├── phase3_grace.rs    # Grace period (7-365 gün)
│   │   │   │   └── phase4_transfer.rs # Kademeli devir başlar
│   │   │   ├── proof_of_life/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── standard.rs   # Zincir üstü imza (§5.4.4 Yöntem 1)
│   │   │   │   ├── duress.rs     # Duress kodu (baskı koruması) (Yöntem 2)
│   │   │   │   └── guardian.rs   # Guardian onayı ile süre uzatma (Yöntem 3)
│   │   │   ├── heirs/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── hierarchy.rs  # 3 öncelik seviyeli varis ağacı (§5.4.5)
│   │   │   │   ├── allocation.rs # Yüzde dağılımı (Varis A %60, B %40)
│   │   │   │   └── timeout.rs    # Varis yanıt timeout → bir sonraki seviye
│   │   │   ├── conditions/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── age.rs        # Yaş koşulu (§5.4.7)
│   │   │   │   ├── debt.rs       # Sıralı borç ödeme koşulu
│   │   │   │   ├── charity.rs    # Hayır kurumu koşulu
│   │   │   │   └── multisig.rs   # Çoklu imza varis koşulu
│   │   │   ├── staged_release/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── schedule.rs   # Zaman kilitli kademeli devir (§5.4.6)
│   │   │   │   └── executor.rs   # Kademeli devir uygulayıcısı
│   │   │   ├── guardian_abuse/
│   │   │   │   ├── mod.rs
│   │   │   │   └── protection.rs # Guardian suistimal koruması (§5.4.8)
│   │   │   └── inactivity_counter.rs # İnaktivite sayacı
│   │   └── tests/
│   │
│   ├── nova-freeze/              # Emergency freeze protokolü (§5.5)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── state.rs          # FreezeState { frozen, timestamp, initiator, reason }
│   │   │   ├── triggers/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── user.rs       # Tetikleyici 1: Kullanıcı kendi dondurur
│   │   │   │   ├── guardian.rs   # Tetikleyici 2: Guardian threshold freeze
│   │   │   │   └── anomaly.rs    # Tetikleyici 3: Otomatik anomali tespiti (§5.5.2)
│   │   │   ├── anomaly/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── detector.rs   # Davranış sapması tespiti
│   │   │   │   ├── scorer.rs     # Anomaly score hesabı
│   │   │   │   └── patterns.rs   # Referans davranış profili
│   │   │   ├── phases/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── frozen.rs     # Dondurma aşaması (çıkış engel, giriş serbest)
│   │   │   │   ├── investigation.rs # 72 saatlik soruşturma penceresi
│   │   │   │   ├── identity.rs   # Kullanıcı kimlik kanıtı (MPC threshold)
│   │   │   │   └── unfreeze.rs   # Hesap açma
│   │   │   ├── abuse_protection.rs # Yanlış freeze → %5 slash (§5.5.4)
│   │   │   └── reason.rs         # FreezeReason enum
│   │   └── tests/
│   │
│   ├── # ─── RUNTIME VE STATE ────────────────────────────────────────
│   │
│   ├── nova-runtime/             # Zincir runtime çekirdeği
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── state/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── db.rs         # RocksDB state backend
│   │   │   │   ├── trie.rs       # Merkle trie (state root)
│   │   │   │   ├── account.rs    # Hesap state storage
│   │   │   │   ├── cache.rs      # DashMap in-memory cache (Solana'dan)
│   │   │   │   └── snapshot.rs   # Epoch snapshot (full + incremental)
│   │   │   ├── executor/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── parallel.rs   # Paralel TX execution (Sealevel ilkesi)
│   │   │   │   ├── scheduler.rs  # Account-key bağımlılık grafiği
│   │   │   │   ├── batch.rs      # Batch auction sonrası yürütme
│   │   │   │   └── commit.rs     # State commit
│   │   │   ├── tx/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── types.rs      # Transaction, SignedTx, EncryptedTx
│   │   │   │   ├── validation.rs # TX doğrulama (imza + nonce + bakiye)
│   │   │   │   ├── fee.rs        # EIP-1559 benzeri dinamik fee (§8.1)
│   │   │   │   ├── burn.rs       # Base fee'nin %50'si yakılır
│   │   │   │   └── account_keys.rs # Okuma/yazma hesap listesi beyanı (Sealevel)
│   │   │   ├── block/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── lifecycle.rs  # open → freeze → root
│   │   │   │   ├── hash.rs       # Block hash hesabı (BLAKE3)
│   │   │   │   └── finalize.rs   # BLS threshold finalizasyon
│   │   │   ├── staking/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── validator.rs  # Validator { stake, commission, jailed, ... }
│   │   │   │   ├── delegator.rs  # Delegatör (NSC staker, oy kullanıcı)
│   │   │   │   ├── reward.rs     # Era payout (Polkadot formülü)
│   │   │   │   ├── unbonding.rs  # 21 günlük unbonding süreci
│   │   │   │   └── diversity.rs  # %5 stake cap + coğrafi çeşitlilik
│   │   │   ├── governance/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── proposal.rs   # Yönetim önerisi
│   │   │   │   ├── voting.rs     # Stake-ağırlıklı oy (Cosmos'tan)
│   │   │   │   ├── tally.rs      # Tally: quorum %33.4, threshold %50
│   │   │   │   └── execution.rs  # Kabul edilen öneri uygulama
│   │   │   ├── token/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── nsc.rs        # NSC token (1 milyar arz)
│   │   │   │   ├── emission.rs   # %8 → %2 emisyon planı (§7.3)
│   │   │   │   ├── fee_dist.rs   # Protocol fee dağıtımı 40/40/20 (§7.4)
│   │   │   │   └── burn.rs       # Fee yakma mekanizması
│   │   │   └── upgrade.rs        # Forkless runtime upgrade (Polkadot'tan)
│   │   └── tests/
│   │
│   ├── # ─── ZK-SHIELD ───────────────────────────────────────────────
│   │
│   ├── nova-zk/                  # ZK-Shield gizlilik katmanı (§5.7)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── utxo/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── note.rs       # Gizli not (value + randomness → commitment)
│   │   │   │   ├── nullifier.rs  # Nullifier = BLAKE3(spending_key || commitment)
│   │   │   │   ├── tree.rs       # Nullifier Merkle tree
│   │   │   │   └── pool.rs       # Shielded pool
│   │   │   ├── circuits/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── mint.rs       # Transparent → Shielded circuit
│   │   │   │   ├── transfer.rs   # Shielded → Shielded circuit
│   │   │   │   ├── burn.rs       # Shielded → Transparent circuit
│   │   │   │   └── multi_asset.rs # Çoklu varlık desteği
│   │   │   ├── proof/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── groth16.rs    # Groth16 prover/verifier
│   │   │   │   ├── plonk.rs      # PLONK prover/verifier
│   │   │   │   └── params.rs     # Trusted setup parametreleri
│   │   │   ├── keys/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── spending.rs   # Spending key
│   │   │   │   ├── viewing.rs    # Viewing key (denetim için)
│   │   │   │   └── diversified.rs # Çeşitlendirilmiş adresler
│   │   │   ├── modes/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── standard.rs   # Normal mod (mempool şifreli, zincir açık)
│   │   │   │   ├── full_shield.rs # Tam gizlilik modu (§5.7.5)
│   │   │   │   └── semi_shield.rs # Yarı-gizli (miktar gizli, adres açık)
│   │   │   └── integration.rs    # Şifreli mempool + ZK entegrasyonu (§5.7.4)
│   │   ├── benches/
│   │   │   └── proof_bench.rs
│   │   └── tests/
│   │
│   ├── # ─── GUARDIAN MARKETPLACESİ ─────────────────────────────────
│   │
│   ├── nova-guardian/            # Guardian marketplace ve itibar (§5.8)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── registry/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── registration.rs # Guardian kaydı + stake
│   │   │   │   ├── categories.rs   # 4 kategori (§5.8.2)
│   │   │   │   └── verification.rs # KYC/AML doğrulama (kurumsal)
│   │   │   ├── reputation/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── trust_score.rs  # Trust Score formülü (§5.8.3)
│   │   │   │   ├── history.rs      # Başarılı kurtarma geçmişi
│   │   │   │   ├── response_time.rs # Ortalama yanıt süresi
│   │   │   │   └── slash_record.rs # Slash geçmişi
│   │   │   ├── marketplace/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── listing.rs    # Guardian listeleme
│   │   │   │   ├── search.rs     # Filtre: kategori, trust score, ücret, SLA
│   │   │   │   ├── offer.rs      # Teklif gönderme + kabul
│   │   │   │   └── contract.rs   # On-chain hizmet sözleşmesi (SLA)
│   │   │   ├── fees/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── models.rs     # Yıllık / kurtarma başına / karma (§5.8.4)
│   │   │   │   └── auto_payment.rs # Otomatik ödeme
│   │   │   ├── insurance/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── fund.rs       # Sigorta fonu (§5.8.5)
│   │   │   │   ├── claim.rs      # Tazminat talebi
│   │   │   │   └── payout.rs     # Tazminat ödeme
│   │   │   └── economics.rs      # Guardian ekonomisi (§5.8.6)
│   │   └── tests/
│   │
│   ├── # ─── NOVA VM ─────────────────────────────────────────────────
│   │
│   ├── nova-vm/                  # Nova VM (§9.1)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── wasm/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── runtime.rs    # wasmtime runtime
│   │   │   │   ├── host_fns.rs   # Host fonksiyonları (MPC API, batch API)
│   │   │   │   ├── memory.rs     # Lineer bellek yönetimi
│   │   │   │   └── gas.rs        # WASM gas metrikleri
│   │   │   ├── evm/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── precompiles.rs # EVM precompile'lar
│   │   │   │   ├── bridge.rs     # WASM ↔ EVM köprüsü
│   │   │   │   └── compat.rs     # Solidity ABI uyumluluğu
│   │   │   ├── api/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── mpc_api.rs    # Kontrat → MPC hesap yönetimi
│   │   │   │   ├── batch_api.rs  # Kontrat → clearing price erişimi
│   │   │   │   ├── guardian_api.rs # Kontrat → guardian yönetimi
│   │   │   │   └── storage_api.rs  # Kontrat storage (key-value)
│   │   │   └── parallel.rs       # Paralel kontrat yürütme (hesap beyanı)
│   │   └── tests/
│   │
│   ├── # ─── P2P AĞ KATMANI ─────────────────────────────────────────
│   │
│   ├── nova-network/             # P2P ağ katmanı (§5.12)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── transport/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── noise.rs      # Noise XX handshake (§6.4.3)
│   │   │   │   ├── quic.rs       # QUIC transport (UDP üzerinde)
│   │   │   │   └── tcp.rs        # TCP fallback
│   │   │   ├── discovery/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── kademlia.rs   # Kademlia DHT (§5.12.1)
│   │   │   │   ├── mdns.rs       # Local discovery
│   │   │   │   └── bootstrap.rs  # Bootstrap node listesi
│   │   │   ├── gossip/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── gossipsub.rs  # GossipSub (rolling-shutter'dan)
│   │   │   │   ├── topics.rs     # Topic: txs, blocks, votes, guardian
│   │   │   │   └── scoring.rs    # Peer scoring (spam koruması)
│   │   │   ├── crds/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── types.rs      # CRDS değer tipleri (Solana'dan)
│   │   │   │   ├── store.rs      # CRDS veri deposu
│   │   │   │   └── gossip.rs     # PullRequest/PushMessage protokolü
│   │   │   ├── security/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── ip_anon.rs    # Validator IP anonimizasyonu (§6.2.2)
│   │   │   │   ├── relay.rs      # Relay node katmanı
│   │   │   │   ├── multi_as.rs   # Multi-AS peer zorunluluğu (§6.2.2)
│   │   │   │   ├── rate_limit.rs # Rate limiting (§6.4.4)
│   │   │   │   └── pow_challenge.rs # PoW DDoS koruması
│   │   │   ├── turbine/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── shred.rs      # Reed-Solomon shred (Solana Turbine'den)
│   │   │   │   ├── tree.rs       # Stake-weighted yayın ağacı
│   │   │   │   └── repair.rs     # Kayıp shred tamir
│   │   │   └── metrics.rs        # Ağ metrik toplama
│   │   └── tests/
│   │
│   ├── # ─── CROSS-CHAIN GUARD ───────────────────────────────────────
│   │
│   ├── nova-guard/               # Cross-chain asset protection (§5.6)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── bitcoin/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── address.rs    # MPC tabanlı Bitcoin adresi
│   │   │   │   ├── tx.rs         # Bitcoin TX oluşturma
│   │   │   │   ├── signer.rs     # MPC imza → Bitcoin broadcast
│   │   │   │   └── watcher.rs    # Bitcoin chain takibi
│   │   │   ├── ethereum/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── address.rs    # MPC tabanlı Ethereum adresi
│   │   │   │   ├── tx.rs         # ETH/ERC-20 TX
│   │   │   │   ├── signer.rs     # MPC imza → Ethereum broadcast
│   │   │   │   ├── beefy.rs      # BEEFY light client kanıtı (Polkadot'tan)
│   │   │   │   └── bridge.rs     # Nova → Ethereum köprüsü
│   │   │   ├── solana/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── address.rs    # MPC tabanlı Solana adresi
│   │   │   │   └── signer.rs     # MPC imza → Solana broadcast
│   │   │   ├── cosmos/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── ibc.rs        # IBC entegrasyonu (§9.3)
│   │   │   │   └── signer.rs
│   │   │   ├── coordinator.rs    # Cross-chain TX koordinasyonu
│   │   │   ├── inheritance.rs    # Cross-chain varlık miras entegrasyonu (§5.6.4)
│   │   │   └── economics.rs      # %0.05 yıllık fee (§5.6.5)
│   │   └── tests/
│   │
│   ├── # ─── SDK ─────────────────────────────────────────────────────
│   │
│   ├── nova-sdk/                 # Geliştirici SDK (§9.2)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── nova_mpc/
│   │   │   │   ├── mod.rs        # MPC hesap yönetimi API
│   │   │   │   ├── account.rs    # Hesap oluşturma, guardian atama
│   │   │   │   └── signing.rs    # TX imzalama
│   │   │   ├── nova_batch/
│   │   │   │   ├── mod.rs        # Batch auction entegrasyonu
│   │   │   │   ├── order.rs      # Emir gönderme
│   │   │   │   └── query.rs      # Clearing price sorgulama
│   │   │   ├── nova_deadman/
│   │   │   │   ├── mod.rs        # Miras planı yönetimi
│   │   │   │   ├── plan.rs       # Plan oluşturma/güncelleme
│   │   │   │   └── heartbeat.rs  # Heartbeat gönderme
│   │   │   ├── nova_guardian/
│   │   │   │   ├── mod.rs        # Guardian protokolü
│   │   │   │   ├── search.rs     # Marketplace arama
│   │   │   │   └── contract.rs   # Guardian sözleşmesi
│   │   │   └── rpc/
│   │   │       ├── mod.rs        # JSON-RPC client
│   │   │       └── types.rs
│   │   └── examples/
│   │       ├── create_account.rs
│   │       ├── send_tx.rs
│   │       ├── setup_inheritance.rs
│   │       └── guardian_setup.rs
│   │
│   ├── # ─── NODE ────────────────────────────────────────────────────
│   │
│   ├── nova-node/                # Validator node çalıştırıcı
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── config.rs         # Node konfigürasyonu
│   │   │   ├── services/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── consensus.rs  # Konsensüs servisi
│   │   │   │   ├── mempool.rs    # Mempool servisi
│   │   │   │   ├── network.rs    # P2P ağ servisi
│   │   │   │   ├── rpc.rs        # JSON-RPC + WebSocket sunucu
│   │   │   │   ├── guardian.rs   # Guardian servisi
│   │   │   │   └── metrics.rs    # Prometheus metrik sunucu
│   │   │   ├── sync/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── warp.rs       # Warp sync (GRANDPA benzeri)
│   │   │   │   ├── full.rs       # Tam senkronizasyon
│   │   │   │   └── light.rs      # Light client senkronizasyon
│   │   │   └── cli.rs            # Komut satırı arayüzü
│   │   └── Cargo.toml
│   │
│   ├── # ─── RPC VE API ─────────────────────────────────────────────
│   │
│   ├── nova-rpc/                 # JSON-RPC sunucu
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── endpoints/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── chain.rs      # chain_* metodları
│   │   │   │   ├── account.rs    # account_* metodları
│   │   │   │   ├── tx.rs         # tx_* metodları
│   │   │   │   ├── guardian.rs   # guardian_* metodları
│   │   │   │   ├── inheritance.rs # inheritance_* metodları
│   │   │   │   ├── freeze.rs     # freeze_* metodları
│   │   │   │   └── zk.rs         # zk_* metodları
│   │   │   ├── websocket.rs      # Subscription API (yeni blok, TX onayı)
│   │   │   └── middleware.rs     # Rate limiting, auth
│   │   └── tests/
│   │
│   ├── # ─── ARAÇLAR VE YARDIMCıLAR ─────────────────────────────────
│   │
│   ├── nova-primitives/          # Paylaşılan tipler (no_std)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── types.rs          # Hash, Address, Balance, Slot, Epoch
│   │   │   ├── block.rs          # BlockHeader, BlockBody
│   │   │   ├── tx.rs             # Transaction tipler
│   │   │   ├── error.rs          # Global hata tipleri
│   │   │   └── constants.rs      # Tüm sabitler
│   │   └── Cargo.toml
│   │
│   └── nova-testing/             # Test utilities
│       ├── src/
│       │   ├── lib.rs
│       │   ├── mock/
│       │   │   ├── mod.rs
│       │   │   ├── validator.rs  # Mock validator seti
│       │   │   ├── guardian.rs   # Mock guardian
│       │   │   └── network.rs    # Mock ağ
│       │   ├── fixtures/         # Test veri setleri
│       │   └── scenarios/        # Entegrasyon test senaryoları
│       └── Cargo.toml
│
├── bin/
│   ├── nova-node/                # Ana validator binary
│   ├── nova-cli/                 # Komut satırı aracı
│   ├── nova-keygen/              # Anahtar üretici
│   └── nova-guardian-cli/        # Guardian yönetim aracı
│
├── contracts/                    # Nova VM akıllı kontratlar
│   ├── nova-dex/                 # Örnek DEX kontratı
│   ├── nova-nft/                 # NFT standardı
│   ├── nova-multisig/            # Çok imzalı kontrat
│   └── nova-dao/                 # DAO yönetişim kontratı
│
├── formal/                       # Formal doğrulama
│   ├── tla/
│   │   ├── MpcSigning.tla        # MPC protokolü
│   │   ├── ThresholdDecryption.tla
│   │   ├── BatchAuction.tla
│   │   ├── DeadmanSwitch.tla
│   │   └── EmergencyFreeze.tla
│   └── coq/
│       ├── Crypto.v              # Kriptografik ilkeller
│       └── Economics.v           # Ekonomik model değişmezleri
│
├── docs/
│   ├── architecture/             # Mimari diyagramlar
│   ├── api/                      # RPC API referansı
│   └── tutorials/                # Geliştirici rehberleri
│
├── scripts/
│   ├── devnet.sh                 # Local devnet başlatma
│   ├── genesis.sh                # Genesis bloğu oluşturma
│   └── benchmark.sh              # Performans testleri
│
└── zombienet/                    # Ağ simülasyon testleri (Polkadot'tan)
    ├── devnet.toml
    └── testnet.toml
```

---

## 3. Crate Listesi ve Satır Tahminleri

| Crate | Açıklama | Tahmini Satır | Whitepaper §§ |
|-------|----------|---------------|---------------|
| `nova-primitives` | Paylaşılan tipler, sabitler | 3,000 | Tüm |
| `nova-crypto` | Hash, imza, ZK altyapısı | 25,000 | §6.1 |
| `nova-mpc` | CGGMP21 keygen + signing + resharing | 35,000 | §5.1, §6.1.1 |
| `nova-mempool` | Şifreli mempool + threshold decrypt | 20,000 | §5.2 |
| `nova-auction` | Sealed-bid batch auction | 15,000 | §5.3 |
| `nova-account` | MPC hesap, guardian, recovery | 18,000 | §5.1 |
| `nova-inheritance` | Deadman switch protokolü | 22,000 | §5.4 |
| `nova-freeze` | Emergency freeze protokolü | 10,000 | §5.5 |
| `nova-consensus` | Simplex BFT + DPoS + slashing | 28,000 | §5.10 |
| `nova-runtime` | State, executor, staking, governance | 30,000 | §5.9, §7 |
| `nova-zk` | ZK-Shield, circuits, proof sistemleri | 28,000 | §5.7 |
| `nova-guardian` | Marketplace, itibar, sigorta | 15,000 | §5.8 |
| `nova-vm` | Nova VM (WASM + EVM uyumluluğu) | 25,000 | §9.1 |
| `nova-network` | libp2p, gossipsub, Turbine, güvenlik | 20,000 | §5.12, §6.4 |
| `nova-guard` | Cross-chain BTC/ETH/SOL/Cosmos | 18,000 | §5.6 |
| `nova-sdk` | Geliştirici SDK + örnekler | 12,000 | §9.2 |
| `nova-node` | Validator node, sync, CLI | 10,000 | §10 |
| `nova-rpc` | JSON-RPC + WebSocket API | 8,000 | §9 |
| `nova-testing` | Mock'lar, fixture'lar, senaryolar | 15,000 | — |
| Akıllı Kontratlar | DEX, NFT, DAO, MultiSig örnek | 12,000 | §9 |
| Test Dosyaları | Birim + entegrasyon + fuzz + bench | 30,000 | §6.7 |
| Formal Doğrulama | TLA+, Coq spec dosyaları | 8,000 | §6.7.1 |
| Belgeler + Scriptler | Docs, scripts, config | 5,000 | — |
| **TOPLAM** | | **~416,000** | |

**Hedef 200K satır Rust kodu fazlasıyla aşılıyor.**

---

## 4. Bileşen-Whitepaper Eşleştirmesi

Her whitepaper gereksinimi → hangi crate → hangi dosya → araştırma kaynağı:

| Whitepaper Özelliği | Crate | Ana Dosyalar | Kaynak Analiz |
|---------------------|-------|--------------|---------------|
| MPC 3/5 threshold (§5.1.2) | nova-mpc | keygen/, threshold.rs | multi-party-ecdsa, tss-lib |
| CGGMP21 protokolü (§6.1.1) | nova-mpc | signing/, presign.rs | multi-party-ecdsa |
| Pre-signing (§5.9.2) | nova-mpc | signing/presign.rs | multi-party-ecdsa |
| Sosyal kurtarma (§5.1.4) | nova-account | recovery/, window.rs | — |
| Guardian yönetimi (§5.1.5) | nova-account | guardian/ | — |
| Şifreli mempool (§5.2.2) | nova-mempool | encryption/, pool/ | rolling-shutter |
| ElGamal threshold (§6.1.4) | nova-crypto | threshold/elgamal.rs | rolling-shutter |
| Pedersen DKG (§6.1.4) | nova-crypto | threshold/dkg.rs | rolling-shutter |
| 512-byte padding (§6.4.2) | nova-mempool | encryption/padding.rs | — |
| Batch auction (§5.3.2) | nova-auction | clearing/price.rs | cow-protocol |
| Uniform price clearing (§5.3.2) | nova-auction | clearing/matching.rs | cow-protocol |
| 400ms slot (§5.3.3) | nova-mempool | timing/slot_clock.rs | solana |
| Deadman switch (§5.4) | nova-inheritance | trigger/, phases/ | — |
| 4-fazlı protokol (§5.4.3) | nova-inheritance | phases/ | — |
| Duress kodu (§5.4.4) | nova-inheritance | proof_of_life/duress.rs | — |
| Hiyerarşik varis (§5.4.5) | nova-inheritance | heirs/hierarchy.rs | — |
| Kademeli devir (§5.4.6) | nova-inheritance | staged_release/ | — |
| Koşullu miras (§5.4.7) | nova-inheritance | conditions/ | — |
| Emergency freeze (§5.5) | nova-freeze | triggers/, phases/ | — |
| Anomali tespiti (§5.5.2) | nova-freeze | anomaly/ | — |
| Cross-chain MPC (§5.6) | nova-guard | bitcoin/, ethereum/ | multi-party-ecdsa |
| ZK-Shield UTXO (§5.7.3) | nova-zk | utxo/, circuits/ | penumbra |
| Groth16/PLONK (§5.7.3) | nova-zk | proof/ | penumbra, arkworks |
| Nullifier (§5.7.3) | nova-zk | utxo/nullifier.rs | penumbra |
| Guardian marketplace (§5.8) | nova-guardian | marketplace/, registry/ | — |
| Trust Score (§5.8.3) | nova-guardian | reputation/trust_score.rs | — |
| Sigorta fonu (§5.8.5) | nova-guardian | insurance/ | — |
| Simplex BFT (§5.10) | nova-consensus | bft/simplex.rs | avalanchego |
| BLS threshold imzası (§5.10) | nova-crypto | signatures/bls.rs | polkadot-sdk |
| Single-slot finality (§5.10) | nova-consensus | bft/finality.rs | avalanchego |
| Slashing (§5.10.4) | nova-consensus | slashing/ | cosmos-sdk |
| Dilithium3 (§6.1.2) | nova-crypto | signatures/dilithium.rs | — |
| Hibrit imza (§6.1.2) | nova-crypto | signatures/hybrid.rs | — |
| BLAKE3 (§6.1.3) | nova-crypto | hash.rs | — |
| HSM zorunluluğu (§6.2.1) | nova-node | hsm.rs | — |
| IP anonimizasyon (§6.2.2) | nova-network | security/ip_anon.rs | — |
| Key rotation (§6.2.3) | nova-consensus | epoch/transition.rs | polkadot-sdk |
| Secure Enclave (§6.3.1) | nova-account | device/enclave.rs | — |
| Random gecikme (§6.4.1) | nova-mempool | timing/random_delay.rs | — |
| Noise XX protokolü (§6.4.3) | nova-network | transport/noise.rs | rolling-shutter |
| DDoS koruması (§6.4.4) | nova-network | security/rate_limit.rs | — |
| Checkpoint (§6.5.1) | nova-consensus | checkpoint.rs | ethereum, cosmos |
| Coğrafi çeşitlilik (§6.5.3) | nova-runtime | staking/diversity.rs | — |
| NSC token ekonomisi (§7) | nova-runtime | token/ | polkadot-sdk |
| EIP-1559 dinamik fee (§8.1) | nova-runtime | tx/fee.rs | go-ethereum |
| Fee yakma %50 (§8.1) | nova-runtime | token/burn.rs | — |
| Ücretsiz işlemler (§8.3) | nova-runtime | tx/fee.rs | — |
| Nova VM WASM (§9.1) | nova-vm | wasm/ | solana |
| EVM uyumluluk (§9.1) | nova-vm | evm/ | go-ethereum, revm |
| IBC entegrasyonu (§9.3) | nova-guard | cosmos/ibc.rs | cosmos-sdk |
| Phragmén (Polkadot'tan) | nova-consensus | dpos/phragmen.rs | polkadot-sdk |
| Paralel TX (Sealevel) | nova-runtime | executor/parallel.rs | solana |
| Turbine blok yayma | nova-network | turbine/ | solana |
| CRDS gossip metadata | nova-network | crds/ | solana |
| F1 fee dağıtımı | nova-runtime | token/fee_dist.rs | cosmos-sdk |
| Forkless upgrade | nova-runtime | upgrade.rs | polkadot-sdk |
| BEEFY bridge kanıtı | nova-guard | ethereum/beefy.rs | polkadot-sdk |

---

## 5. Faz 1: Kriptografik Temel (2026 Q3–Q4)

**Amaç:** Her şeyin üzerine inşa edeceğimiz kriptografik primitifler.

### 5.1 nova-primitives

```
Görev 1: Temel tipler
nova-primitives/src/types.rs
  - Hash = [u8; 32] (BLAKE3 çıktısı)
  - Address = [u8; 32] (MPC public key'den türetilir)
  - Balance = u128 (lamport/NSC en küçük birim)
  - Slot = u64
  - Epoch = u64
  - BlockHeight = u64
  - ValidatorId = PublicKey

nova-primitives/src/constants.rs
  - SLOT_DURATION_MS: u64 = 400
  - EPOCH_DURATION_SLOTS: u64 = 216_000  // 24 saat
  - MAX_VALIDATORS: u32 = 1_000
  - INITIAL_VALIDATORS: u32 = 200
  - MIN_VALIDATOR_STAKE: u128 = 100_000_000_000  // 100,000 NSC (8 decimal)
  - TOTAL_NSC_SUPPLY: u128 = 1_000_000_000_000_000_000  // 1B NSC
  - BFT_THRESHOLD_NUMERATOR: u64 = 2
  - BFT_THRESHOLD_DENOMINATOR: u64 = 3
  - UNBONDING_PERIOD_EPOCHS: u64 = 21
  - MAX_SLIPPAGE_SLOTS: u64 = 5
  - HEARTBEAT_FEE: u128 = 0  // Ücretsiz
  - GUARDIAN_MIN_STAKE: u128 = 10_000_000_000_000  // 10,000 NSC (kurumsal)
  - BASE_FEE_BURN_RATE: u64 = 50  // %50
  - DEFAULT_THRESHOLD: u32 = 3
  - DEFAULT_TOTAL_SHARES: u32 = 5
  - FREEZE_INVESTIGATION_WINDOW_SECS: u64 = 72 * 3600
  - RECOVERY_WINDOW_SECS: u64 = 48 * 3600
  - GUARDIAN_CHANGE_WINDOW_DAYS: u64 = 7
  - INHERITANCE_MIN_INACTIVITY_DAYS: u64 = 30
  - PROOF_OF_LIFE_GRACE_MAX_EXTENSIONS: u32 = 3
  - SLASH_DOUBLE_SIGN: u64 = 100  // %100
  - SLASH_MEMPOOL_LEAK: u64 = 50  // %50
  - SLASH_WRONG_BATCH: u64 = 25   // %25
  - SLASH_OFFLINE_DAILY: u64 = 1  // %0.1 (per 1000)
  - MAX_ENCRYPTION_PADDING_BYTES: usize = 512
```

### 5.2 nova-crypto

```
Görev 2: BLAKE3 Hash + Merkle Tree

Görev 3: CGGMP21 uyumlu ECDSA + secp256k1

Görev 4: Dilithium3 (pqcrypto-dilithium bağlaması)

Görev 5: Hibrit imza (ECDSA + Dilithium3 paralel)
  - SignedMessage { ecdsa_sig: [u8;64], dilithium_sig: Vec<u8>, payload: Vec<u8> }
  - verify() → ECDSA VEYA Dilithium3 geçerliyse kabul

Görev 6: BLS12-381 (blst)
  - BLS signature
  - BLS aggregate
  - BLS threshold (for single-slot finality)

Görev 7: VRF (leader seçimi)
  - ECVRF-SECP256K1-SHA256-TAI (IETF draft)
  - prove(secret_key, alpha) → (beta, pi)
  - verify(public_key, alpha, pi) → beta

Görev 8: Pedersen Commitment
  - commit(value, randomness) → C
  - verify(C, value, randomness) → bool

Görev 9: ElGamal Threshold Şifreleme
  - KeyGen(t, n) → (shares, public_key)
  - Encrypt(message, public_key) → ciphertext
  - DecryptShare(ciphertext, share) → partial
  - Combine(partials[t]) → message

Görev 10: Pedersen DKG
  - Round 1: Her participant commitment üretir
  - Round 2: Shares dağıtılır
  - Round 3: Verification + finalize

Görev 11: Constant-time operasyonlar
  - subtle crate kullanımı
  - Tüm karşılaştırma + seçim operasyonları
```

### 5.3 ZK Devreleri (nova-crypto/zk)

```
Görev 12: arkworks kurulumu
  - ark-bls12-381, ark-groth16, ark-snark
  - ark-std (no_std uyumlu)

Görev 13: Transfer devresi
  - Public: nullifier, commitment_new, amount_commitment
  - Private: value, randomness, spending_key
  - Constraint: balance korunumu

Görev 14: Nullifier devresi
  - nullifier = BLAKE3(spending_key || commitment)
  - Double-spend kontrolü

Görev 15: Trusted setup (PLONK için universal)
  - Yerel devnet için hızlı setup
  - Mainnet için seremonik setup planı

Tahmini süre: 8-10 hafta
Tahmini satır: 25,000
```

---

## 6. Faz 2: Çekirdek Konsensüs (2026 Q4–2027 Q1)

### 6.1 Simplex BFT Implementasyonu

```
Referans: avalanchego/consensus/simplex/ (Avalanche)
          polkadot-sdk GRANDPA (2-round mantığı)

Aşamalar:
  PREPARE phase:
    - Blok üretici ProposeBlock yayınlar
    - Blok içeriği: encrypted_commits + prev_block_hash + slot_number

  VOTE phase:
    - Her validator bloğu doğrular
    - BLS kısmi imzasını yayınlar

  FINALIZE phase:
    - 2/3 BLS imzası toplandığında
    - BLS.Aggregate() → tek BLS imzası
    - Block confirmed, state güncellenir

Yapılar:
  struct Block {
    header: BlockHeader,
    body: BlockBody,
    bls_agg_signature: BlsSignature,
  }

  struct BlockHeader {
    height: BlockHeight,
    slot: Slot,
    parent_hash: Hash,
    state_root: Hash,
    tx_root: Hash,
    batch_clearing_price: ClearingPrice,
    timestamp: u64,
    proposer: ValidatorId,
  }
```

### 6.2 DPoS Validator Seçimi

```
Epoch başı seçim süreci:
1. Tüm validator adayları stake'e göre sıralanır (Phragmén)
2. VRF ile rastgelelik eklenir (bias önleme)
3. 200 aktif validator belirlenir
4. Leader schedule oluşturulur (her slot için bir leader)

Phragmén uygulaması:
  - polkadot-sdk substrate/frame/staking'den alınan algoritma
  - Delegatör oylarını validator'lara adil dağıtır
  - Tek entity %5'ten fazla stake kontrolü yapamaz

VRF Slot Ataması:
  slot_proof = VRF(epoch_randomness || slot_number, validator_secret)
  if hash(slot_proof.output) < validator_weight_threshold:
    → Bu validator bu slot'un lideri
```

### 6.3 Slashing

```
Her slashing türü için ayrı delil yapısı:

DoubleSignEvidence {
  slot: Slot,
  block1: BlockHeader,
  sig1: BlsSignature,
  block2: BlockHeader,
  sig2: BlsSignature,
}

MempoolLeakEvidence {
  slot: Slot,
  tx_content: EncryptedTx,
  premature_reveal: TxContent,
  timestamp: u64,
}

Delil penceresi: 100 blok
Delil geçerliyse: otomatik slash + delil sahibine %10 ödül

Graduated slashing (Polkadot'tan alınan fikir):
  1. ihlal: uyarı + stake'in %0.1
  2. ihlal: stake'in %1
  3. ihlal: stake'in %10
  Koordineli saldırı (n > t): tam slash
```

### 6.4 Epoch Yönetimi

```
Epoch = 24 saat = 216,000 slot (400ms × 216,000 = 86,400 saniye)

Epoch sonu işlemleri (EndBlocker benzeri):
1. Validator set değerlendirmesi
2. Ödül dağıtımı (Polkadot PiecewiseLinear formülü)
3. Unbonding işlemleri
4. Key rotation (proactive re-sharing)
5. Yeni epoch başlatma (VRF + Phragmén)

Ödül formülü:
  staker_payout = yearly_inflation(staked_ratio) × total_supply / epochs_per_year
  Year 1: ~%8 APY
  Year 12: ~%2 APY (deflasyoner sınır)
```

---

## 7. Faz 3: Şifreli Mempool + Auction (2027 Q1)

### 7.1 Şifreli Mempool

```
TX Yaşam Döngüsü:

[KULLANICI]
  1. TX oluştur: { from, to, amount, data, nonce, slot_deadline }
  2. IBE ile şifrele: EncryptedTx = IBE.Encrypt(TX, epoch_pk)
  3. Commitment oluştur: C = BLAKE3(EncryptedTx || nonce || slot_number)
  4. Padding: EncryptedTx → fixed 512 bytes
  5. 0-200ms random gecikme ekle
  6. Ağa yayınla: { commitment: C, ciphertext: EncryptedTx }

[VALIDATOR NODE - COMMIT PHASE (0-300ms)]
  7. C'yi al, EncryptedTx hash'ini doğrula
  8. Duplicate kontrolü
  9. Buffer'a ekle (700K TX kapasitesi)

[SLOT LİDERİ - 300ms anı]
  10. Buffer'daki tüm TX'leri topla
  11. Threshold decryption başlat: DecryptRequest(slot)

[2/3 VALIDATOR - REVEAL PHASE (300-400ms)]
  12. Her validator IBE key share ile kısmi decrypt üretir
  13. Lider t kısmi decrypt'i birleştirir
  14. Tüm TX'ler açılır
  15. Batch auction motoru devreye girer

[BATCH EXECUTE]
  16. Swap emirleri: clearing price hesabı
  17. Normal TX'ler: paralel execution
  18. State root güncellenir
  19. BLS threshold imzası → blok kesinleşir
```

### 7.2 Sealed-Bid Batch Auction

```
Uniform Price Auction Algoritması (CoW Protocol'den):

Input: Vec<BuyOrder>, Vec<SellOrder>  (slot'taki tüm emirler)

1. Her emir fiyata göre sıralanır:
   - Alımlar: yüksekten düşüğe (en çok ödemek isteyen önce)
   - Satımlar: düşükten yükseğe (en az satmak isteyen önce)

2. Kümülatif talep eğrisi oluşturulur:
   price=210: 200 SOL alım var
   price=200: 350 SOL alım var  ← (100 + 200 + 50)
   price=195: 400 SOL alım var

3. Kümülatif arz eğrisi oluşturulur:
   price=198: 80 SOL satış var
   price=200: 275 SOL satış var
   price=205: 395 SOL satış var

4. Kesişme noktası = CLEARING PRICE
   Bu örnekte: $200

5. Settlement:
   - $200 ve üzeri alım emirleri EXECUTE → hepsi $200'dan
   - $200 ve altı satım emirleri EXECUTE → hepsi $200'dan
   - MEV = 0 (herkes aynı fiyat, sıralama yok)

6. Kısmi doldurma:
   Tam eşleşme bulunamazsa → kısmi fill
   Kalan miktar → bir sonraki slot (slippage kontrolü)
```

---

## 8. Faz 4: MPC Hesap Modeli (2027 Q1–Q2)

### 8.1 CGGMP21 Keygen

```
Referans: research/libs/multi-party-ecdsa (ZenGo)

4 round DKG:
Round 1: Her katılımcı (K1-K5):
  - secret_i (random scalar)
  - commitment_i = hash(secret_i || salt_i)
  - broadcast(commitment_i)

Round 2:
  - Feldman VSS: poly_i(x) ile shares üretilir
  - share_ji = poly_i(j)  → j'ye gönderilir
  - broadcast(poly_i commitments)

Round 3:
  - Her katılımcı aldığı share'leri doğrular
  - Açıklamalar eşleşmiyorsa → abort + tespit

Round 4:
  - aggregate_public_key = ΣKᵢ * G  (toplam public key)
  - share_i = Σshare_ji  (kişisel share)
  - MPC hesap adresi = BLAKE3(aggregate_public_key)

Hesap yapısı:
struct MpcAccount {
  address: Address,
  public_key: PublicKey,      // Toplam MPC public key
  threshold: u32,              // t (varsayılan 3)
  total_shares: u32,           // n (varsayılan 5)
  guardians: Vec<GuardianId>,  // K1-K5 sahipleri
  created_at: Slot,
  last_activity: Slot,         // Heartbeat/TX son zamanı
  state: AccountState,         // Active | Frozen | Inheritance
  inheritance_plan: Option<InheritancePlanId>,
  freeze_state: Option<FreezeState>,
}
```

### 8.2 CGGMP21 Pre-Signing

```
Whitepaper §5.9.2: "Guardian'ların işlem anında online olması gerekmez"

Pre-signing (offline, önceden):
  Guardian offline iken:
  1. Random pre-signature share üretir
  2. Şifreli olarak Nova Shield node'una yükler
  3. (Henüz bir TX'e bağlı değil)

İşlem anında (online değil):
  1. Kullanıcı TX oluşturur
  2. TX hash pre-signature share ile birleştirilir
  3. Tek round: ~10ms
  4. 400ms slot'a rahatlıkla sığar

Implementasyon notu:
  cggmp21 Rust kütüphanesi bu mekanizmayı native destekler.
  research/libs/multi-party-ecdsa/src/protocols/multi_party_ecdsa/gg_2020/
  referans implementasyon.
```

### 8.3 Key Resharing (Proactive Security)

```
Her epoch (24 saatte bir) key rotation:

1. Mevcut threshold {K1, K2, K3, K4, K5} → {K1', K2', K3', K4', K5'}
2. Eski share'ler imha edilir
3. Saldırgan t adet share toplamak için TÜM BİRİKTİRMEYİ 24 saat içinde yapmak zorunda
4. Bu pratik olarak imkansız

Resharing protokolü (tss-lib referans: research/libs/tss-lib/ecdsa/resharing/):
  - Eski komite shares'ini yeni komiteye güvenli aktarır
  - Ne eski ne yeni komite hiçbir zaman tam key'i görmez
```

---

## 9. Faz 5: Miras + Freeze Protokolleri (2027 Q2)

### 9.1 Deadman Switch State Machine

```
                ┌─────────────────────────────────┐
                │         ACTIVE (Normal)          │
                │  last_activity güncel            │
                └───────────────┬─────────────────┘
                                │ inactivity > trigger_period
                                ▼
                ┌─────────────────────────────────┐
                │     PHASE 1: WARNING (30 gün)   │
                │  Guardian'lara bildirim         │
                │  Kullanıcı heartbeat → ACTIVE   │
                └───────────────┬─────────────────┘
                                │ 30 gün doldu
                                ▼
                ┌─────────────────────────────────┐
                │     PHASE 2: FINAL (7 gün)      │
                │  Tüm taraflara son uyarı        │
                │  Kullanıcı heartbeat → ACTIVE   │
                └───────────────┬─────────────────┘
                                │ 7 gün doldu
                                ▼
                ┌─────────────────────────────────┐
                │   PHASE 3: GRACE PERIOD         │
                │  (7 - 365 gün, kullanıcı seçer) │
                │  Varisler erişemez              │
                │  Kullanıcı iptal edebilir       │
                └───────────────┬─────────────────┘
                                │ grace period doldu
                                ▼
                ┌─────────────────────────────────┐
                │   PHASE 4: STAGED TRANSFER      │
                │  Plan kurallarına göre devir    │
                │  Kademeli (zaman kilitli)        │
                └─────────────────────────────────┘

State machine: TLA+ ile formal olarak kanıtlanacak
```

### 9.2 Koşullu Miras Motoru

```rust
pub enum InheritanceCondition {
    AgeReached {
        address: Address,
        min_account_age_blocks: u64  // 18 yıl ≈ blok sayısı olarak
    },
    DebtPaidFirst {
        creditor: Address,
        amount: Balance
    },
    CharityFirst {
        charity: Address,
        percentage: u8  // %5
    },
    MultiSigRequired {
        required_signers: Vec<Address>,
        threshold: u32
    },
    TimeAfterTrigger {
        delay_seconds: u64  // Kademeli devir için
    },
}

pub struct HeirAllocation {
    pub address: Address,
    pub percentage: u8,          // 0-100
    pub conditions: Vec<InheritanceCondition>,
    pub response_timeout: u64,   // Bu seviyenin timeout süresi
    pub fallback_heir: Option<Box<HeirAllocation>>,
}

pub struct InheritancePlan {
    pub owner: Address,
    pub tiers: Vec<Vec<HeirAllocation>>,  // 3 öncelik seviyesi
    pub burn_address: Option<Address>,     // Son çare yakma
    pub treasury_address: Option<Address>,
}
```

### 9.3 Emergency Freeze — Anomali Dedektörü

```
Anomali tespiti için referans davranış profili oluşturulur:

Metrikler:
  - Ortalama TX miktarı (μ, σ)
  - Ortalama günlük TX sayısı
  - Coğrafi IP dağılımı (yaklaşık)
  - Alıcı adresleri (beyaz liste benzeri)
  - TX saatleri (timezone profili)

Anomali skoru:
  score = Σ(deviation_i × weight_i)

  Eşik aşıldıysa:
  - score > 0.7: Bildirim
  - score > 0.9: 10 dakika içinde onay yoksa dondur

Örnek tetikleyiciler:
  - Hiç dış adrese işlem yapmamış hesap → aniden 10 adrese transfer
  - 0 işlem geçmişi olan hesap → büyük miktarlı ilk transfer
  - Olağandışı saat (kullanıcının timezone'u dışı)
```

---

## 10. Faz 6: Nova VM + Akıllı Kontrat (2027 Q2–Q3)

### 10.1 Nova VM Mimarisi

```
Katmanlar:

L1: Rust WASM Kontratları (birincil)
  - wasmtime runtime
  - Host fonksiyonlar: MPC API, Batch API, Guardian API
  - Gas: compute unit sistemi (Solana modeli)

L2: EVM Uyumluluk Katmanı (ikincil)
  - revm entegrasyonu
  - Solidity ABI desteği
  - ETH kontratları az değişiklikle çalışır

Paralel Execution (Sealevel ilkesi):
  - Her kontrat çağrısı account_keys beyan eder
  - Çakışmayan çağrılar rayon ile paralel çalıştırılır
  - Batch açılınca bağımlılık grafiği çözülür
  - PrioGraphScheduler (Solana'dan)

Host Fonksiyon Örnekleri:
  mpc_get_account(address) → MpcAccount
  mpc_initiate_signing(tx_hash, required_guardians) → SigningSessionId
  batch_get_clearing_price(token_pair) → Price
  guardian_assign(guardian_id, share_index) → Result
  deadman_set_plan(plan: InheritancePlan) → PlanId
  deadman_heartbeat() → ()
  freeze_account() → ()
```

### 10.2 Nova SDK Kontrat Örnekleri

```rust
// nova-dex/src/lib.rs
// Batch auction ile entegre DEX

#[nova_contract]
pub struct NovaDex {
    order_book: HashMap<TokenPair, Vec<Order>>,
}

impl NovaDex {
    pub fn submit_order(&mut self, order: Order) {
        // Şifreli mempool'a gider, batch auction ile execute edilir
        // Kontrat clearing price'ı okuyabilir
        let price = batch_get_clearing_price(order.pair);
        self.order_book.entry(order.pair).push(order);
    }

    pub fn settle(&mut self, slot: Slot) {
        // Slot kapanınca settlement
        let clearing_price = batch_get_clearing_price(self.pair);
        // ...
    }
}
```

---

## 11. Faz 7: ZK-Shield (2027 Q3)

### 11.1 Trusted Setup (PLONK Universal)

```
Mainnet öncesi KZG seremonik:
  - KZG10 polynomial commitment scheme
  - Universal (tüm devreler için tek setup)
  - PLONK (Permutations over Lagrange-bases for Oecumenical Noninteractive arguments of Knowledge)

Alternatif: Groth16 (daha hızlı doğrulama, devre başına setup)
  Karar: Araştırma sürecinde netleşecek

Devnet için: Sahte trusted setup (test)
Testnet için: Küçük ölçekli seremonik
Mainnet için: Çok katılımcılı seremonik (güven dağıtımı)
```

### 11.2 ZK-Shield Akışı

```
Transparent → Shielded (Mint):
  1. 1000 NSC hesaptan çıkar
  2. ZK devresi: commit(1000, randomness) → C
  3. Nullifier_set'e C eklenir
  4. ZK proof: "1000 NSC'ye sahibim ve commitment'ı doğru ürettim"
  5. Zincire: { commitment: C, proof: π }

Shielded → Shielded (Transfer):
  1. Spending key ile nullifier üret: N = BLAKE3(sk || C_old)
  2. Yeni commitment: C_new = commit(900, r_new)
  3. ZK proof:
     - C_old listede var (nullifier kullanılmadı)
     - C_new doğru üretildi
     - 1000 = 900 + 100 (fee)
     - İmzalayan sk'ya sahip
  4. Zincire: { nullifier: N, commitment: C_new, proof: π }

Shielded → Transparent (Burn):
  1. Nullifier üret
  2. ZK proof: "Bu commitment benim ve harcamadım"
  3. Belirtilen adrese 1000 NSC transfer edilir
  4. Zincire: { nullifier: N, recipient: addr, proof: π }
```

---

## 12. Faz 8: Guardian Marketplace (2027 Q3)

### 12.1 On-Chain Trust Score

```rust
pub struct TrustScore {
    // Ağırlıklı faktörler:
    pub stake_weight: f64,          // 100_000 NSC = 1.0, logaritmik
    pub active_years: f64,          // Her yıl +0.1, max 1.0
    pub recovery_success_rate: f64, // Başarılı / toplam kurtarma
    pub slash_history: f64,         // Slash yoksa 1.0, her slash -0.3
    pub response_time_score: f64,   // <1h = 1.0, <24h = 0.7, >24h = 0.3
    pub accounts_guarding: f64,     // 1-10 hesap = 0.5, 10-100 = 0.8, 100+ = 1.0
}

pub fn compute_trust_score(g: &GuardianMetrics) -> f64 {
    0.20 * stake_weight(g.stake) +
    0.15 * active_years(g.created_epoch) +
    0.30 * g.recovery_success_rate +
    0.20 * slash_factor(g.slash_count) +
    0.10 * response_score(g.avg_response_ms) +
    0.05 * accounts_score(g.account_count)
}
```

### 12.2 Hizmet Sözleşmesi (On-Chain SLA)

```rust
pub struct GuardianContract {
    pub guardian_id: GuardianId,
    pub user_id: Address,
    pub start_epoch: Epoch,
    pub fee_model: FeeModel,     // Annual | PerRecovery | Hybrid
    pub fee_amount: Balance,
    pub sla_response_hours: u64, // Onay için maksimum bekleme
    pub auto_payment: bool,
    pub insurance_coverage: Balance, // Sigorta fonu kapsamı
}
```

---

## 13. Faz 9: Cross-Chain Guard (2027 Q4)

### 13.1 Bitcoin MPC Adresi

```
Bitcoin adresi MPC public key'den türetilir:
  aggregate_pubkey = CGGMP21.keygen(guardians)
  bitcoin_address = P2WPKH(aggregate_pubkey)  // Native SegWit

İşlem imzalama:
  1. Kullanıcı "BTC gönder" talebi oluşturur
  2. CGGMP21 signing protokolü başlar
  3. t/n guardian pre-signed share kullanır
  4. Tam Bitcoin TX imzası oluşturulur
  5. Bitcoin ağına broadcast edilir
  6. Nova Shield zincirinde log kaydı yapılır (koordinasyon)
```

### 13.2 Ethereum BEEFY Bridge

```
Polkadot-sdk BEEFY (polkadot-sdk analizi §15.J'den):

Nova → Ethereum bridge:
  1. Her epoch sonunda Nova validator'lar BLS12-381 imzalar
  2. BLS imzaları BLS.Aggregate() ile birleştirilir
  3. Ethereum'a tek BLS doğrulama gönderilir
  4. Ethereum kontratı: "Bu BLS public key ile bu mesaj imzalandı mı?"
  5. Düşük gas (tek imza doğrulama)

Nova Shield Ethereum kontratı:
  - Solidity ile yazılır
  - BLS12-381 precompile kullanır (EIP-2537)
  - Epoch'ta kesinleşmiş blokları doğrular
```

---

## 14. Faz 10: Devnet → Testnet → Mainnet

### Devnet (2027 Q2)

```
Ortam: 5 local validator (tek makine, Docker)
Amaç: Temel blok üretimi + konsensüs doğrulama

Test edilecekler:
  □ Blok üretimi (400ms slot)
  □ BLS threshold imzası
  □ Şifreli mempool (5 TX)
  □ Batch clearing price
  □ Basit transfer TX
  □ Guardian atama

Araçlar:
  nova-node --dev --alice  (validator 1)
  nova-node --dev --bob    (validator 2)
  ...
  nova-cli submit-tx ...
```

### Testnet (2027 Q3)

```
Ortam: 200 validator, gerçek ağ (incentivize testnet)
NSC ödülleri: Gerçek

Test edilecekler:
  □ Tüm MPC operasyonlar (keygen, signing, resharing)
  □ Sosyal kurtarma senaryoları
  □ Guardian marketplace
  □ Deadman switch (hızlandırılmış zaman: 1 gün = 1 ay)
  □ Emergency freeze
  □ ZK-Shield (tüm modlar)
  □ Cross-chain BTC/ETH
  □ Nova VM akıllı kontratlar
  □ Güvenlik testleri + bug bounty
  □ Audit firmaları (§6.7.2)

Stres testleri:
  □ 10,000 TPS yük testi
  □ 1/3 validator offline (fault tolerance)
  □ Network partition testi
  □ Disk I/O bottleneck testi
```

### Mainnet (2027 Q4)

```
Faz 1 başlangıç:
  □ 200 genesis validator (ön seçim yapıldı)
  □ Token genesis dağıtımı (§7.2 token dağılımı)
  □ Hazine likidite desteği aktif (§7.6 Çözüm 1)
  □ Kurumsal guardian'lar hazır (§7.6 Çözüm 4)
  □ 2+ CEX listing aktif (MEXC → Gate)
  □ Nova VM akıllı kontrat platformu açık

İlk 24 ay validator sübvansiyonu (§7.6 Çözüm 2):
  Validator ödülleri → doğrudan emisyondan
  Protocol fee → ikincil kaynak
```

---

## 15. Test Stratejisi

### 15.1 Birim Testleri (Unit Tests)

```
Her crate'de /tests/ dizini:
  - Her public fonksiyon için en az 1 test
  - Edge case testleri (0 değer, maks değer, overflow)
  - Hata yolları (err path coverage %80+)

Özel dikkat:
  - Constant-time operasyonlar: timing test
  - MPC protokolü: her round ayrı test
  - ZK devreler: test vector doğrulama
  - Slashing: her suç türü ayrı test
```

### 15.2 Entegrasyon Testleri

```
Senaryo Örnekleri:

Senaryo 1 — Normal TX Akışı:
  1. Hesap oluştur (DKG)
  2. Fon transfer et (pre-signing + batch auction)
  3. State doğrula

Senaryo 2 — Cihaz Kaybı Kurtarma:
  1. K1, K2 offline
  2. K3, K4, K5 online → kurtarma başlat
  3. 48 saat bekle (simüle edilmiş)
  4. Yeni K1, K2 kaydet
  5. İşlem yapılabilir mi? → Evet

Senaryo 3 — Deadman Switch:
  1. Miras planı kur (3 varis, 2 koşul)
  2. İnaktivite simüle et (hızlandırılmış)
  3. Her fazın doğru çalıştığını doğrula
  4. Varis 1 yanıt vermez → Varis 2'ye geç

Senaryo 4 — MEV Direnci:
  1. Bot TX'i önceden bilse de batch'te avantaj yok
  2. Clearing price her zaman uniform
  3. Sıralama değiştirilse de fark yok (kanıtla)

Senaryo 5 — 1/3 Validator Saldırısı:
  1. 66/200 validator offline
  2. Blok üretimi devam eder mi? → Evet (134/200 > 2/3)
  3. Kesinleşme süresi değişir mi? → Hayır
```

### 15.3 Fuzz Testleri

```
cargo-fuzz kullanımı:

Fuzz hedefleri:
  - TX parsing (malformed input)
  - MPC mesaj işleme
  - ZK proof doğrulama
  - Commitment doğrulama
  - Inheritance plan parsing
  - P2P mesaj parsing

AFL++ entegrasyonu devnet öncesi
```

### 15.4 Benchmark'lar

```
Kritik benchmark'lar:

nova-crypto:
  - BLAKE3: >6,000 MB/s beklenen
  - BLS sign: <1ms hedefi
  - BLS verify: <5ms hedefi
  - Groth16 proof: <500ms hedefi
  - Groth16 verify: <10ms hedefi
  - Dilithium3 sign: <1ms hedefi

nova-mpc:
  - Keygen (5 party): <2 sn hedefi
  - Pre-signing (offline): <100ms
  - Signing (online, pre-signed): <10ms

nova-auction:
  - 10,000 emir clearing: <50ms hedefi

nova-consensus:
  - BLS aggregate 200 imza: <5ms hedefi
  - Block validation: <20ms hedefi

Genel:
  - 25,000 TPS (Faz 1 hedefi)
  - 400ms slot içinde tüm işlemler
```

---

## 16. Formal Doğrulama Planı

### 16.1 TLA+ Spesifikasyonları

```
MpcSigning.tla:
  - CGGMP21 protokolü state machine
  - Kanıtlanacak: Hiçbir zaman tam key ifşa olmaz
  - Kanıtlanacak: t-1 corrupt guardian ile imza üretilemez

ThresholdDecryption.tla:
  - ElGamal threshold şifreleme
  - Kanıtlanacak: t-1 validator TX içeriğini öğrenemez
  - Kanıtlanacak: t validator birleşince şifre çözülür

BatchAuction.tla:
  - Uniform price auction state machine
  - Kanıtlanacak: Clearing price her zaman supply=demand kesişimi
  - Kanıtlanacak: Hiçbir katılımcı fiyatı tek başına manipüle edemez

DeadmanSwitch.tla:
  - 4 fazlı state machine
  - Kanıtlanacak: Guardian'lar suistimal edemez (threshold + time)
  - Kanıtlanacak: Kullanıcı her zaman iptal edebilir (grace period'da)
  - Kanıtlanacak: İnaktivite süresi dolmadan devir olmaz

EmergencyFreeze.tla:
  - Freeze state machine
  - Kanıtlanacak: Freeze sırasında çıkış TX'leri reddedilir
  - Kanıtlanacak: Kullanıcı 72 saat içinde iptal edebilir
```

### 16.2 Coq Kanıtları

```
Crypto.v:
  - BLAKE3 collision direnci (sıkıştırma fonksiyonu)
  - Commitment binding + hiding özellikleri
  - ZK proof soundness + completeness

Economics.v:
  - Token arzı korunumu (yakma dahil)
  - Staking ödül formülü monotonluğu
  - Fee dağıtımı doğruluğu (40+40+20=100)
```

---

## 17. Satır Sayısı Tablosu — Detaylı

| Crate | Kaynak | Test | Bench | Toplam |
|-------|--------|------|-------|--------|
| nova-primitives | 3,000 | 500 | 0 | 3,500 |
| nova-crypto | 18,000 | 5,000 | 2,000 | 25,000 |
| nova-mpc | 25,000 | 8,000 | 2,000 | 35,000 |
| nova-mempool | 14,000 | 4,500 | 1,500 | 20,000 |
| nova-auction | 10,000 | 4,000 | 1,000 | 15,000 |
| nova-account | 13,000 | 4,000 | 1,000 | 18,000 |
| nova-inheritance | 16,000 | 5,500 | 500 | 22,000 |
| nova-freeze | 7,000 | 2,500 | 500 | 10,000 |
| nova-consensus | 20,000 | 6,500 | 1,500 | 28,000 |
| nova-runtime | 22,000 | 6,500 | 1,500 | 30,000 |
| nova-zk | 20,000 | 6,000 | 2,000 | 28,000 |
| nova-guardian | 11,000 | 3,500 | 500 | 15,000 |
| nova-vm | 18,000 | 6,000 | 1,000 | 25,000 |
| nova-network | 15,000 | 4,500 | 500 | 20,000 |
| nova-guard | 13,000 | 4,000 | 1,000 | 18,000 |
| nova-sdk | 9,000 | 2,500 | 500 | 12,000 |
| nova-node | 8,000 | 1,500 | 500 | 10,000 |
| nova-rpc | 6,500 | 1,500 | 0 | 8,000 |
| nova-testing | 0 | 15,000 | 0 | 15,000 |
| Kontratlar | 10,000 | 2,000 | 0 | 12,000 |
| Formal (TLA+/Coq) | 8,000 | 0 | 0 | 8,000 |
| Scriptler/Docs | 5,000 | 0 | 0 | 5,000 |
| **TOPLAM** | **271,500** | **93,500** | **17,500** | **~382,500** |

**Rust kaynak kodu: ~271,500 satır**
**Test kodu: ~93,500 satır**
**Bench kodu: ~17,500 satır**
**GENEL TOPLAM: ~382,500 satır**

---

## Hızlı Başlangıç — İlk Komutlar

```bash
# 1. Proje kurulumu
cargo new --name nova-shield-chain .
cargo init --lib crates/nova-primitives
cargo init --lib crates/nova-crypto
# ... diğer crate'ler

# 2. Workspace Cargo.toml
[workspace]
members = [
  "crates/nova-primitives",
  "crates/nova-crypto",
  "crates/nova-mpc",
  # ... 18 crate
]

# 3. İlk build kontrolü
SKIP_ZK_BUILD=1 cargo check --workspace

# 4. İlk test
cargo test -p nova-primitives
cargo test -p nova-crypto

# 5. Devnet başlatma (Faz 1 sonunda)
./scripts/devnet.sh
```

---

## Geliştirme Sırası (Öncelik Listesi)

```
HAFTA 1-4:   nova-primitives + nova-crypto (BLAKE3, BLS, Dilithium3)
HAFTA 5-12:  nova-mpc (CGGMP21 keygen + signing + pre-signing)
HAFTA 13-18: nova-consensus (Simplex BFT + DPoS + slashing)
HAFTA 19-24: nova-mempool + nova-auction (şifreli mempool + batch)
HAFTA 25-30: nova-account + nova-runtime (MPC hesap + state)
HAFTA 31-36: nova-inheritance + nova-freeze (miras + freeze)
HAFTA 37-42: nova-network + nova-node (P2P + validator)
HAFTA 43-48: nova-vm + kontratlar (Nova VM + EVM)
HAFTA 49-54: nova-zk (ZK-Shield)
HAFTA 55-60: nova-guardian + nova-guard (marketplace + cross-chain)
HAFTA 61-64: nova-sdk + nova-rpc + entegrasyon
HAFTA 65-72: Testnet + audit + bug bounty
HAFTA 73-76: Mainnet hazırlığı + launch
```

---

*Bu plan, 10 rakip sistemin satır satır analizinden ve 1607 satırlık whitepaper'dan damıtıldı. Her karar araştırma temelli, her bileşen whitepaper'a bağlı.*
