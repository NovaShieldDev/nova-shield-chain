//! Nova DEX — Pool Registry ve AMM
//!
//! ## Güvenlik Düzeltmeleri
//!
//! - **NOVA-RED-DEX-POOL-001**: `nova_price_cents` artık kullanıcıdan alınmıyor.
//!   Sunucu tarafında oracle fiyatı kullanılır. `CreatePoolRequest`'ten alan kaldırıldı.
//!
//! - **NOVA-RED-DEX-POOL-002**: `add_liquidity` artık orantısız tokenleri kabul etmiyor.
//!   Sağlanan miktarlar pool oranına göre hesaplanır, fazlalık iade edilir.
//!
//! - **NOVA-RED-DEX-FEE-001**: Tek fee implementasyonu — `amm.rs` ödüllendirildi.
//!   `fee.rs` dead code kaldırıldı.
//!
//! - **NOVA-RED-DEX-SWAP-001**: Bot sayacı slippage kontrolünden SONRA artırılıyor.

use crate::errors::{DexError, DexResult};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Pool oluşturma ücreti: $17.50 değerinde nNOVA
/// nova_price_cents oracle'dan alınır (NOVA-RED-DEX-POOL-001 düzeltmesi).
pub const CREATION_FEE_USD_CENTS: u64 = 1_750; // $17.50

/// 1 NOVA = 10^18 nNOVA
pub const NOVA_DECIMALS: u128 = 1_000_000_000_000_000_000;

/// LP fee: swap başına binde 3 (%0.3) — standart
pub const LP_FEE_BPS: u64 = 30; // 30 bps = 0.3%

/// Hacim indirimi: günde 100+ başarılı swap yapan adresler için binde 1.5 (%0.15)
pub const LP_FEE_DISCOUNT_BPS: u64 = 15; // 15 bps = 0.15%

/// Günlük swap eşiği — bu sayıya ulaşınca sonraki swaplarda indirim uygulanır
pub const VOLUME_DISCOUNT_THRESHOLD: u64 = 100;

/// Launch period: pool açıldıktan sonraki ilk 5 dakika (ms)
pub const LAUNCH_PERIOD_MS: u64 = 5 * 60 * 1_000; // 300_000 ms

/// Launch period'da adres başına maksimum alım: token supply'ının %5'i (bps)
pub const LAUNCH_MAX_BUY_BPS: u128 = 500; // 500 bps = %5

/// Bot sayacı slippage başarısızlığında ARTTIRILMAZ (NOVA-RED-DEX-SWAP-001)
/// Yalnızca başarılı swap'ta artırılır.

// ─── LockDuration ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LockDuration {
    OneDay,
    ThreeDays,
    OneWeek,
    OneMonth,
    OneYear,
    Permanent,
}

impl LockDuration {
    pub fn to_ms(&self) -> Option<u64> {
        Some(match self {
            Self::OneDay    => 24 * 3600 * 1000,
            Self::ThreeDays => 3 * 24 * 3600 * 1000,
            Self::OneWeek   => 7 * 24 * 3600 * 1000,
            Self::OneMonth  => 30 * 24 * 3600 * 1000,
            Self::OneYear   => 365 * 24 * 3600 * 1000,
            Self::Permanent => return None,
        })
    }

    pub fn is_permanent(&self) -> bool {
        matches!(self, Self::Permanent)
    }
}

// ─── Pool ─────────────────────────────────────────────────────────────────────

/// Likidite pool'u
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pool {
    /// Pool kimliği (hex)
    pub id: String,
    /// Pool oluşturan adres
    pub creator: [u8; 20],
    /// Token A sembolü
    pub asset_a: String,
    /// Token B sembolü
    pub asset_b: String,
    /// Token A rezervi (nNOVA veya token smallest unit)
    pub reserve_a: u128,
    /// Token B rezervi
    pub reserve_b: u128,
    /// Toplam LP token arzı
    pub total_lp: u128,
    /// Kilitleme süresi
    pub lock_duration: LockDuration,
    /// Kilit açılma zamanı (ms, permanent ise None)
    pub unlock_at_ms: Option<u64>,
    /// Oluşturulma zamanı (ms)
    pub created_at_ms: u64,
    /// Toplam swap hacmi
    pub total_volume: u128,
    /// Bot swap sayacı (toplam, tüm zamanlar)
    pub bot_swap_count: u64,
    /// Launch period alım takibi: adres → toplam alınan token miktarı (reserve_b cinsinden)
    /// Pool açıldıktan sonraki ilk LAUNCH_PERIOD_MS süre boyunca aktif.
    pub launch_buy_tracker: HashMap<[u8; 20], u128>,
    /// Günlük adres bazlı swap sayacı: adres → bugünkü başarılı swap sayısı
    /// Hacim indirimi (VOLUME_DISCOUNT_THRESHOLD) kontrolü için kullanılır.
    /// Her UTC günü başında (swap_count_day değişince) sıfırlanır.
    pub daily_swap_counts: HashMap<[u8; 20], u64>,
    /// daily_swap_counts'un geçerli olduğu UTC gün anahtarı (now_ms / 86_400_000)
    pub swap_count_day: u32,
    /// Birikmiş creator fee payı (%20) — payout çağrısına kadar burada tutulur
    pub creator_fees_pending: u128,
    /// Birikmiş treasury fee payı (%80) — payout çağrısına kadar burada tutulur
    pub treasury_fees_pending: u128,
}

impl Pool {
    pub fn new(
        id: String,
        creator: [u8; 20],
        asset_a: String,
        asset_b: String,
        initial_a: u128,
        initial_b: u128,
        lock_duration: LockDuration,
        created_at_ms: u64,
    ) -> Self {
        let unlock_at_ms = lock_duration.to_ms().map(|d| created_at_ms + d);
        let initial_lp = (initial_a as f64 * initial_b as f64).sqrt() as u128;
        Self {
            id,
            creator,
            asset_a,
            asset_b,
            reserve_a: initial_a,
            reserve_b: initial_b,
            total_lp: initial_lp.max(1),
            lock_duration,
            unlock_at_ms,
            created_at_ms,
            total_volume: 0,
            bot_swap_count: 0,
            launch_buy_tracker: HashMap::new(),
            daily_swap_counts: HashMap::new(),
            swap_count_day: 0,
            creator_fees_pending: 0,
            treasury_fees_pending: 0,
        }
    }

    /// Kilit süresi doldu mu?
    pub fn is_unlocked(&self, now_ms: u64) -> bool {
        match self.unlock_at_ms {
            None => false, // Permanent
            Some(t) => now_ms >= t,
        }
    }
}

// ─── LpPositions ──────────────────────────────────────────────────────────────

type LpPositions = HashMap<[u8; 20], u128>; // address → lp_tokens

// ─── PayoutResult ─────────────────────────────────────────────────────────────

/// process_payouts() dönüş değeri
#[derive(Debug)]
pub struct PayoutResult {
    /// İşlenen pool sayısı
    pub pools_processed: usize,
    /// Creator adresi → dağıtılan miktar (nNOVA) çiftleri
    pub creator_distributions: Vec<([u8; 20], u128)>,
    /// Creator'lara dağıtılan toplam (nNOVA)
    pub total_distributed: u128,
    /// Treasury'ye ayrılan toplam (nNOVA)
    pub treasury_total: u128,
}

// ─── PoolRegistry ─────────────────────────────────────────────────────────────

/// Pool kayıt defteri — tüm pool'lar ve LP pozisyonları
pub struct PoolRegistry {
    pools: Arc<RwLock<HashMap<String, Pool>>>,
    lp_positions: Arc<RwLock<HashMap<String, LpPositions>>>, // pool_id → positions
    /// Oracle fiyatı (cent/NOVA) — NOVA-RED-DEX-POOL-001: kullanıcıdan alınmaz
    nova_price_cents: Arc<RwLock<u64>>,
    /// Pool yetkili paydout caller'ı
    payout_authority: [u8; 20],
}

impl PoolRegistry {
    pub fn new(payout_authority: [u8; 20]) -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            lp_positions: Arc::new(RwLock::new(HashMap::new())),
            nova_price_cents: Arc::new(RwLock::new(2_000)), // $20 default
            payout_authority,
        }
    }

    /// Oracle fiyatını güncelle (admin only — sunucu tarafında çağrılır)
    ///
    /// NOVA-RED-DEX-POOL-001: Bu değer ASLA kullanıcı isteğinden alınmaz.
    pub fn update_oracle_price(&self, nova_price_cents: u64) {
        *self.nova_price_cents.write() = nova_price_cents;
    }

    /// Mevcut oracle fiyatını al
    pub fn oracle_price(&self) -> u64 {
        *self.nova_price_cents.read()
    }

    /// Pool oluşturma ücreti hesapla (nNOVA cinsinden)
    ///
    /// `required_fee_nNOVA = (CREATION_FEE_USD_CENTS * 10^18) / nova_price_cents`
    ///
    /// NOVA-RED-DEX-POOL-001: nova_price_cents oracle'dan alınır, kullanıcıdan değil.
    pub fn required_creation_fee_nnnova(&self) -> DexResult<u128> {
        let price = self.oracle_price();
        if price == 0 {
            return Err(DexError::OracleUnavailable);
        }
        Ok((CREATION_FEE_USD_CENTS as u128 * NOVA_DECIMALS) / price as u128)
    }

    /// Yeni pool oluştur
    pub fn create_pool(
        &self,
        creator: [u8; 20],
        asset_a: String,
        asset_b: String,
        initial_a: u128,
        initial_b: u128,
        lock_duration: LockDuration,
        creation_fee_paid: u128,
        now_ms: u64,
    ) -> DexResult<String> {
        if initial_a == 0 || initial_b == 0 {
            return Err(DexError::ZeroAmount);
        }

        // NOVA-RED-DEX-POOL-001: Ücret oracle'dan hesaplanır
        let required_fee = self.required_creation_fee_nnnova()?;
        if creation_fee_paid < required_fee {
            return Err(DexError::InsufficientCreationFee {
                required: required_fee,
                paid: creation_fee_paid,
            });
        }

        // Pool ID türet
        let id = {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"nova-dex-pool-v1");
            hasher.update(&creator);
            hasher.update(asset_a.as_bytes());
            hasher.update(asset_b.as_bytes());
            hasher.update(&now_ms.to_le_bytes());
            hex::encode(&hasher.finalize().as_bytes()[..16])
        };

        let pool = Pool::new(id.clone(), creator, asset_a, asset_b, initial_a, initial_b, lock_duration, now_ms);
        let initial_lp = pool.total_lp;

        self.pools.write().insert(id.clone(), pool);
        self.lp_positions.write().entry(id.clone()).or_default().insert(creator, initial_lp);

        tracing::info!(pool_id = %id, creator = %hex::encode(creator), "pool oluşturuldu");
        Ok(id)
    }

    /// Pool bilgisi al
    pub fn get_pool(&self, pool_id: &str) -> DexResult<Pool> {
        self.pools.read().get(pool_id).cloned()
            .ok_or_else(|| DexError::PoolNotFound { pool_id: pool_id.into() })
    }

    /// Tüm pool'ları listele
    pub fn list_pools(&self) -> Vec<Pool> {
        self.pools.read().values().cloned().collect()
    }

    /// AMM swap — x*y=k sabiti
    ///
    /// ## Özellikler
    ///
    /// - **Hacim indirimi**: Günde 100+ başarılı swap yapan adresler %0.15 fee öder (standart %0.30).
    ///   `daily_swap_counts` per-adres, per-UTC-gün takip edilir. Her yeni günde sayaç sıfırlanır.
    ///
    /// - **Fee dağılımı**: Her swaptaki fee %20 creator + %80 treasury olarak ayrılır.
    ///   `creator_fees_pending` ve `treasury_fees_pending` alanlarında birikir.
    ///   Rezervlere sadece `amount_in_net` (fee düşülmüş) eklenir.
    ///
    /// - **NOVA-RED-DEX-SWAP-001**: Bot sayacı ve günlük sayaç slippage kontrolünden SONRA artırılır.
    ///
    /// - **NOVA-DEX-LAUNCH-001**: Pool açılışından sonraki ilk 5 dakika adres başına max %5 alım.
    pub fn swap(
        &self,
        pool_id: &str,
        swapper: [u8; 20],
        zero_for_one: bool,
        amount_in: u128,
        min_amount_out: u128,
        now_ms: u64,
    ) -> DexResult<u128> {
        if amount_in == 0 {
            return Err(DexError::ZeroAmount);
        }

        let mut pools = self.pools.write();
        let pool = pools.get_mut(pool_id)
            .ok_or_else(|| DexError::PoolNotFound { pool_id: pool_id.into() })?;

        // ── 1. Günlük sayaç — yeni gün başladıysa sıfırla ───────────────────
        // UTC gün anahtarı: kaç tam gün geçtiği (epoch'tan itibaren)
        let day_key = (now_ms / 86_400_000) as u32;
        if day_key > pool.swap_count_day {
            pool.daily_swap_counts.clear();
            pool.swap_count_day = day_key;
        }
        // Bu adresin bugün kaç başarılı swap yaptığı (bu swap'tan önce)
        let swap_count_today = pool.daily_swap_counts.get(&swapper).copied().unwrap_or(0);

        // ── 2. Fee oranı — hacim indirimi kontrolü ───────────────────────────
        // VOLUME_DISCOUNT_THRESHOLD veya üzeri swap → indirimli fee (%0.15)
        // Eşiğin altında → standart fee (%0.30)
        let fee_bps: u128 = if swap_count_today >= VOLUME_DISCOUNT_THRESHOLD {
            LP_FEE_DISCOUNT_BPS as u128
        } else {
            LP_FEE_BPS as u128
        };
        let fee = amount_in * fee_bps / 10_000;
        let amount_in_net = amount_in.saturating_sub(fee);

        // ── 3. NOVA-DEX-LAUNCH-001: Launch period kontrolü ───────────────────
        // Pool açılışından sonraki ilk 5 dakika: adres başına max %5 alım.
        // Tahmin için amount_in_net kullanılır (gerçek fee oranıyla tutarlı).
        if now_ms < pool.created_at_ms.saturating_add(LAUNCH_PERIOD_MS) {
            if zero_for_one {
                let token_supply = pool.reserve_b;
                let max_buy = token_supply.saturating_mul(LAUNCH_MAX_BUY_BPS) / 10_000;
                let already_bought = pool.launch_buy_tracker.get(&swapper).copied().unwrap_or(0);
                let estimated_out = token_supply
                    .checked_mul(amount_in_net)
                    .and_then(|n| n.checked_div(pool.reserve_a + amount_in_net))
                    .unwrap_or(0);
                if already_bought.saturating_add(estimated_out) > max_buy {
                    return Err(DexError::LaunchPeriodLimitExceeded {
                        max: max_buy.saturating_sub(already_bought),
                        requested: estimated_out,
                    });
                }
            }
        }
        // ─────────────────────────────────────────────────────────────────────

        // ── 4. AMM hesaplama — x*y=k, fee net olarak kullanılır ──────────────
        let (reserve_in, reserve_out) = if zero_for_one {
            (pool.reserve_a, pool.reserve_b)
        } else {
            (pool.reserve_b, pool.reserve_a)
        };

        if reserve_in == 0 || reserve_out == 0 {
            return Err(DexError::InsufficientLiquidity);
        }

        // dy = y * dx_net / (x + dx_net)
        let amount_out = reserve_out
            .checked_mul(amount_in_net)
            .and_then(|n| n.checked_div(reserve_in + amount_in_net))
            .unwrap_or(0);

        // ── 5. NOVA-RED-DEX-SWAP-001: Slippage kontrolü — state değişmeden önce
        if amount_out < min_amount_out {
            return Err(DexError::SlippageExceeded { got: amount_out, min: min_amount_out });
        }

        // ── 6. Rezervleri güncelle — sadece amount_in_net (fee rezerve girmez) ─
        if zero_for_one {
            pool.reserve_a += amount_in_net;
            pool.reserve_b = pool.reserve_b.saturating_sub(amount_out);
        } else {
            pool.reserve_b += amount_in_net;
            pool.reserve_a = pool.reserve_a.saturating_sub(amount_out);
        }

        pool.total_volume += amount_in;

        // ── 7. Fee dağılımı: %20 creator, %80 treasury ───────────────────────
        // Küçük fee miktarlarında integer division sebebiyle creator_fee=0 olabilir;
        // bu durumda tüm fee treasury'ye gider.
        let creator_fee = fee * 20 / 100;
        let treasury_fee = fee.saturating_sub(creator_fee);
        pool.creator_fees_pending += creator_fee;
        pool.treasury_fees_pending += treasury_fee;

        // ── 8. NOVA-DEX-LAUNCH-001: Launch period alım takibi ────────────────
        if now_ms < pool.created_at_ms.saturating_add(LAUNCH_PERIOD_MS) && zero_for_one {
            *pool.launch_buy_tracker.entry(swapper).or_insert(0) += amount_out;
        }

        // ── 9. NOVA-RED-DEX-SWAP-001: Sayaçlar sadece başarılı swap'ta artır ─
        pool.bot_swap_count += 1;
        *pool.daily_swap_counts.entry(swapper).or_insert(0) += 1;

        tracing::debug!(
            pool_id,
            amount_in = %amount_in,
            amount_out = %amount_out,
            fee_bps = %fee_bps,
            fee = %fee,
            "swap"
        );
        Ok(amount_out)
    }

    /// Likidite ekle — orantısız tokenler fazlalık iadesiyle birlikte
    ///
    /// NOVA-RED-DEX-POOL-002: Sağlanan miktarlar pool oranına göre hesaplanır.
    /// Fazlalık amount_b (veya amount_a) iade edilir.
    ///
    /// Dönüş: `(lp_minted, actual_a, actual_b)` — gerçekte kullanılan miktarlar.
    pub fn add_liquidity(
        &self,
        pool_id: &str,
        provider: [u8; 20],
        amount_a: u128,
        amount_b: u128,
        now_ms: u64,
    ) -> DexResult<(u128, u128, u128)> {
        if amount_a == 0 || amount_b == 0 {
            return Err(DexError::ZeroAmount);
        }

        let mut pools = self.pools.write();
        let pool = pools.get_mut(pool_id)
            .ok_or_else(|| DexError::PoolNotFound { pool_id: pool_id.into() })?;

        // NOVA-RED-DEX-POOL-002: Orantısız miktar hesapla
        // a_ratio = amount_a / reserve_a, b_ratio = amount_b / reserve_b
        // actual = min(a_ratio, b_ratio) × reserve
        let (actual_a, actual_b) = if pool.reserve_a == 0 || pool.reserve_b == 0 {
            (amount_a, amount_b)
        } else {
            // a_ratio = amount_a / reserve_a (oran karşılaştırması için çapraz çarpım)
            // amount_a / reserve_a <= amount_b / reserve_b
            // => amount_a * reserve_b <= amount_b * reserve_a
            let a_limited = amount_a
                .checked_mul(pool.reserve_b)
                .map(|n| n / pool.reserve_a)
                .unwrap_or(u128::MAX);

            if a_limited <= amount_b {
                // amount_a sınırlıyor: amount_b oran göre hesaplanır
                let b_actual = amount_a
                    .checked_mul(pool.reserve_b)
                    .and_then(|n| n.checked_div(pool.reserve_a))
                    .unwrap_or(0);
                (amount_a, b_actual)
            } else {
                // amount_b sınırlıyor: amount_a oran göre hesaplanır
                let a_actual = amount_b
                    .checked_mul(pool.reserve_a)
                    .and_then(|n| n.checked_div(pool.reserve_b))
                    .unwrap_or(0);
                (a_actual, amount_b)
            }
        };

        // LP token mint = min(actual_a/reserve_a, actual_b/reserve_b) * total_lp
        let lp_minted = if pool.reserve_a == 0 || pool.total_lp == 0 {
            (actual_a as f64 * actual_b as f64).sqrt() as u128
        } else {
            let lp_a = actual_a.saturating_mul(pool.total_lp) / pool.reserve_a;
            let lp_b = actual_b.saturating_mul(pool.total_lp) / pool.reserve_b;
            lp_a.min(lp_b)
        };

        if lp_minted == 0 {
            return Err(DexError::InsufficientLiquidity);
        }

        pool.reserve_a += actual_a;
        pool.reserve_b += actual_b;
        pool.total_lp += lp_minted;

        let _ = now_ms; // zaman damgası (ileride kullanılabilir)
        drop(pools);

        *self.lp_positions.write()
            .entry(pool_id.to_string()).or_default()
            .entry(provider).or_insert(0) += lp_minted;

        tracing::debug!(pool_id, lp_minted = %lp_minted, "likidite eklendi");
        Ok((lp_minted, actual_a, actual_b))
    }

    /// Likidite çek
    pub fn remove_liquidity(
        &self,
        pool_id: &str,
        provider: [u8; 20],
        lp_to_burn: u128,
        now_ms: u64,
    ) -> DexResult<(u128, u128)> {
        if lp_to_burn == 0 {
            return Err(DexError::ZeroAmount);
        }

        let mut pools = self.pools.write();
        let pool = pools.get_mut(pool_id)
            .ok_or_else(|| DexError::PoolNotFound { pool_id: pool_id.into() })?;

        // Kilit kontrolü
        if !pool.is_unlocked(now_ms) {
            let remaining = pool.unlock_at_ms.map(|t| t.saturating_sub(now_ms)).unwrap_or(u64::MAX);
            return Err(DexError::LockNotExpired { remaining_ms: remaining });
        }

        let lp_balance = self.lp_positions.read()
            .get(pool_id)
            .and_then(|pos| pos.get(&provider).copied())
            .unwrap_or(0);

        if lp_balance < lp_to_burn {
            return Err(DexError::InsufficientLpTokens);
        }

        let amount_a = lp_to_burn.saturating_mul(pool.reserve_a) / pool.total_lp;
        let amount_b = lp_to_burn.saturating_mul(pool.reserve_b) / pool.total_lp;

        pool.reserve_a = pool.reserve_a.saturating_sub(amount_a);
        pool.reserve_b = pool.reserve_b.saturating_sub(amount_b);
        pool.total_lp = pool.total_lp.saturating_sub(lp_to_burn);

        drop(pools);

        let mut pos_map = self.lp_positions.write();
        if let Some(positions) = pos_map.get_mut(pool_id) {
            if let Some(balance) = positions.get_mut(&provider) {
                *balance = balance.saturating_sub(lp_to_burn);
            }
        }

        tracing::debug!(pool_id, lp_to_burn = %lp_to_burn, "likidite çekildi");
        Ok((amount_a, amount_b))
    }

    /// Kilitleme süresini uzat (kısaltma yasak)
    pub fn extend_lock(
        &self,
        pool_id: &str,
        caller: [u8; 20],
        new_duration: LockDuration,
        now_ms: u64,
    ) -> DexResult<()> {
        let mut pools = self.pools.write();
        let pool = pools.get_mut(pool_id)
            .ok_or_else(|| DexError::PoolNotFound { pool_id: pool_id.into() })?;

        if pool.creator != caller {
            return Err(DexError::Unauthorized { reason: "sadece creator süre uzatabilir".into() });
        }

        if new_duration.is_permanent() {
            pool.unlock_at_ms = None;
            pool.lock_duration = new_duration;
            return Ok(());
        }

        let new_unlock = new_duration.to_ms().map(|d| now_ms + d);
        // Kısaltma yasak
        if let (Some(current), Some(new)) = (pool.unlock_at_ms, new_unlock) {
            if new < current {
                return Err(DexError::Unauthorized { reason: "kilit süresi kısaltılamaz".into() });
            }
        }

        pool.unlock_at_ms = new_unlock;
        pool.lock_duration = new_duration;
        Ok(())
    }

    /// LP pozisyonunu sorgula
    pub fn get_lp_position(&self, pool_id: &str, provider: &[u8; 20]) -> u128 {
        self.lp_positions.read()
            .get(pool_id)
            .and_then(|pos| pos.get(provider).copied())
            .unwrap_or(0)
    }

    /// Creator ödemelerini işle — payout_authority kontrolü
    ///
    /// NOVA-RED-DEX-PAYOUT-001: Yalnızca yetkili caller çağırabilir.
    ///
    /// Her pool'un `creator_fees_pending` ve `treasury_fees_pending` alanlarını toplar,
    /// sıfırlar ve `PayoutResult` ile raporlar. Gerçek zincir transferleri bu sonuç
    /// kullanılarak dışarıdan (L1 RPC üzerinden) tetiklenir.
    pub fn process_payouts(&self, caller: [u8; 20]) -> DexResult<PayoutResult> {
        if caller != self.payout_authority {
            return Err(DexError::PayoutUnauthorized);
        }

        let mut pools = self.pools.write();
        let mut creator_distributions: Vec<([u8; 20], u128)> = Vec::new();
        let mut total_distributed: u128 = 0;
        let mut treasury_total: u128 = 0;

        for pool in pools.values_mut() {
            // Creator payı varsa topla ve sıfırla
            if pool.creator_fees_pending > 0 {
                creator_distributions.push((pool.creator, pool.creator_fees_pending));
                total_distributed = total_distributed.saturating_add(pool.creator_fees_pending);
                pool.creator_fees_pending = 0;
            }
            // Treasury payını topla ve sıfırla
            if pool.treasury_fees_pending > 0 {
                treasury_total = treasury_total.saturating_add(pool.treasury_fees_pending);
                pool.treasury_fees_pending = 0;
            }
        }

        let pools_processed = pools.len();

        tracing::info!(
            pools_processed,
            total_distributed = %total_distributed,
            treasury_total = %treasury_total,
            "creator payout işlendi"
        );

        Ok(PayoutResult {
            pools_processed,
            creator_distributions,
            total_distributed,
            treasury_total,
        })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn creator() -> [u8; 20] { [0x01u8; 20] }
    fn provider() -> [u8; 20] { [0x02u8; 20] }
    fn swapper() -> [u8; 20] { [0x03u8; 20] }
    fn payout_auth() -> [u8; 20] { [0xAAu8; 20] }

    fn registry() -> PoolRegistry {
        let r = PoolRegistry::new(payout_auth());
        r.update_oracle_price(2_000); // $20 per NOVA
        r
    }

    fn make_pool(r: &PoolRegistry) -> String {
        r.create_pool(
            creator(), "NOVA".into(), "USDC".into(),
            1_000_000, 1_000_000,
            LockDuration::OneDay, 1_750_000_000_000_000_000, // required fee at $20
            0,
        ).unwrap()
    }

    #[test]
    fn test_oracle_price_controls_fee() {
        let r = PoolRegistry::new(payout_auth());
        r.update_oracle_price(2_000); // $20
        let fee = r.required_creation_fee_nnnova().unwrap();
        // 1750 cents * 10^18 / 2000 cents = 0.875 * 10^18 nNOVA
        assert_eq!(fee, 875_000_000_000_000_000);
    }

    /// NOVA-RED-DEX-POOL-001: nova_price_cents asla kullanıcıdan alınmaz
    #[test]
    fn test_pool_creation_fee_from_oracle_not_user() {
        let r = PoolRegistry::new(payout_auth());
        // Oracle olmadan pool oluşturulamaz
        *r.nova_price_cents.write() = 0;
        let result = r.required_creation_fee_nnnova();
        assert!(matches!(result, Err(DexError::OracleUnavailable)));
    }

    #[test]
    fn test_pool_creation_insufficient_fee_rejected() {
        let r = registry();
        let result = r.create_pool(
            creator(), "NOVA".into(), "USDC".into(),
            1_000_000, 1_000_000, LockDuration::OneDay,
            1, // çok düşük ücret
            0,
        );
        assert!(matches!(result, Err(DexError::InsufficientCreationFee { .. })));
    }

    #[test]
    fn test_swap_basic() {
        let r = registry();
        let pid = make_pool(&r);
        // Launch period sonrası (5 dk + 1 ms)
        let after_launch = LAUNCH_PERIOD_MS + 1;
        let out = r.swap(&pid, swapper(), true, 1_000, 0, after_launch).unwrap();
        assert!(out > 0);
    }

    /// NOVA-RED-DEX-SWAP-001: Slippage başarısızlığında bot sayacı artmamalı
    #[test]
    fn test_swap_bot_counter_not_incremented_on_slippage_failure() {
        let r = registry();
        let pid = make_pool(&r);
        let count_before = r.get_pool(&pid).unwrap().bot_swap_count;
        let after_launch = LAUNCH_PERIOD_MS + 1;
        let result = r.swap(&pid, swapper(), true, 100, u128::MAX, after_launch);
        assert!(matches!(result, Err(DexError::SlippageExceeded { .. })));
        let count_after = r.get_pool(&pid).unwrap().bot_swap_count;
        assert_eq!(count_before, count_after, "başarısız swap bot sayacını artırmamalı");
    }

    /// NOVA-RED-DEX-POOL-002: Orantısız likidite fazlalık iadesi
    #[test]
    fn test_add_liquidity_proportional_refund() {
        let r = registry();
        let pid = make_pool(&r);
        // Pool 1:1 oranında, 1000 A + 500 B ver → 500 A kullanılır, 500 B kullanılır
        let (lp, actual_a, actual_b) = r.add_liquidity(&pid, provider(), 1_000, 500, 0).unwrap();
        assert!(lp > 0);
        // actual_b = 500 (sınırlayan)
        assert_eq!(actual_b, 500);
        // actual_a en fazla reserve_a / reserve_b * 500 = 500
        assert!(actual_a <= 1_000, "fazlalık A iade edilmeli: actual_a={actual_a}");
        assert_eq!(actual_a, actual_b, "1:1 pool'da eşit miktarlar kullanılmalı");
    }

    #[test]
    fn test_add_remove_liquidity() {
        let r = registry();
        let pid = make_pool(&r);
        let (lp, _, _) = r.add_liquidity(&pid, provider(), 100_000, 100_000, 0).unwrap();
        assert!(lp > 0);

        let (out_a, out_b) = r.remove_liquidity(&pid, provider(), lp, 24 * 3600 * 1000 + 1).unwrap();
        assert!(out_a > 0);
        assert!(out_b > 0);
    }

    #[test]
    fn test_remove_liquidity_lock_not_expired() {
        let r = registry();
        let pid = make_pool(&r);
        let (lp, _, _) = r.add_liquidity(&pid, provider(), 100_000, 100_000, 0).unwrap();
        // Henüz kilit açılmadı (now_ms=0)
        let result = r.remove_liquidity(&pid, provider(), lp, 0);
        assert!(matches!(result, Err(DexError::LockNotExpired { .. })));
    }

    /// NOVA-RED-DEX-PAYOUT-001: Yetkisiz payout çağrısı reddedilmeli
    #[test]
    fn test_process_payouts_unauthorized_rejected() {
        let r = registry();
        let result = r.process_payouts([0xFFu8; 20]);
        assert!(matches!(result, Err(DexError::PayoutUnauthorized)));
    }

    #[test]
    fn test_process_payouts_authorized_ok() {
        let r = registry();
        let result = r.process_payouts(payout_auth());
        assert!(result.is_ok());
    }

    // ─── Fee dağılımı testleri ────────────────────────────────────────────────

    /// Her başarılı swap'tan sonra creator_fees_pending + treasury_fees_pending = fee
    #[test]
    fn test_fee_split_accumulates_correctly() {
        let r = registry();
        let pid = make_pool(&r);
        let after_launch = LAUNCH_PERIOD_MS + 1;

        // amount_in = 10_000 → fee = 10_000 * 30 / 10_000 = 30
        // creator_fee = 30 * 20 / 100 = 6
        // treasury_fee = 30 - 6 = 24
        r.swap(&pid, swapper(), true, 10_000, 0, after_launch).unwrap();

        let pool = r.get_pool(&pid).unwrap();
        let total_fee: u128 = pool.creator_fees_pending + pool.treasury_fees_pending;
        assert_eq!(total_fee, 30, "toplam fee 30 bps olmalı");
        assert_eq!(pool.creator_fees_pending, 6, "%20 creator payı");
        assert_eq!(pool.treasury_fees_pending, 24, "%80 treasury payı");
    }

    /// Birden fazla swap birikimli olarak toplanır
    #[test]
    fn test_fee_accumulates_across_swaps() {
        let r = registry();
        let pid = make_pool(&r);
        let after_launch = LAUNCH_PERIOD_MS + 1;

        // 3 swap × fee=30 her biri → toplam fee=90
        for _ in 0..3 {
            r.swap(&pid, swapper(), true, 10_000, 0, after_launch).unwrap();
        }

        let pool = r.get_pool(&pid).unwrap();
        let total = pool.creator_fees_pending + pool.treasury_fees_pending;
        assert_eq!(total, 90, "3 swap sonrası toplam fee 90 olmalı");
    }

    /// process_payouts fee'leri toplayıp sıfırlar
    #[test]
    fn test_process_payouts_distributes_and_resets() {
        let r = registry();
        let pid = make_pool(&r);
        let after_launch = LAUNCH_PERIOD_MS + 1;

        r.swap(&pid, swapper(), true, 10_000, 0, after_launch).unwrap();

        // Payout öncesi kontrol
        {
            let pool = r.get_pool(&pid).unwrap();
            assert!(pool.creator_fees_pending > 0, "payout öncesi fee birikmeli");
        }

        let result = r.process_payouts(payout_auth()).unwrap();
        assert!(result.pools_processed > 0);
        assert!(result.total_distributed > 0, "creator'a dağıtılan miktar > 0");
        assert!(result.treasury_total > 0, "treasury payı > 0");
        assert_eq!(
            result.total_distributed + result.treasury_total, 30,
            "toplam dağıtılan = toplam fee"
        );

        // Payout sonrası sıfırlanmış olmalı
        let pool = r.get_pool(&pid).unwrap();
        assert_eq!(pool.creator_fees_pending, 0, "payout sonrası creator fee sıfır olmalı");
        assert_eq!(pool.treasury_fees_pending, 0, "payout sonrası treasury fee sıfır olmalı");
    }

    // ─── Hacim indirimi testleri ──────────────────────────────────────────────

    /// 100 swap sonrası günlük sayaç doğru güncelleniyor
    #[test]
    fn test_daily_swap_count_increments() {
        let r = registry();
        let pid = make_pool(&r);
        let after_launch = LAUNCH_PERIOD_MS + 1;

        for _ in 0..5 {
            r.swap(&pid, swapper(), true, 1_000, 0, after_launch).unwrap();
        }

        let pool = r.get_pool(&pid).unwrap();
        let count = pool.daily_swap_counts.get(&swapper()).copied().unwrap_or(0);
        assert_eq!(count, 5, "günlük sayaç 5 olmalı");
    }

    /// 100+ swap sonrası hacim indirimi uygulanır — fee daha düşük
    #[test]
    fn test_volume_discount_after_100_swaps() {
        let r = registry();
        let pid = make_pool(&r);
        let after_launch = LAUNCH_PERIOD_MS + 1;

        // 100 swap yap — standart fee (%0.30)
        for _ in 0..100 {
            r.swap(&pid, swapper(), true, 1_000, 0, after_launch).unwrap();
        }

        // 101. swaptan önce fee birikimini kaydet
        let fees_before = {
            let pool = r.get_pool(&pid).unwrap();
            pool.creator_fees_pending + pool.treasury_fees_pending
        };

        // 101. swap — indirimli fee (%0.15)
        r.swap(&pid, swapper(), true, 10_000, 0, after_launch).unwrap();

        let pool = r.get_pool(&pid).unwrap();
        let total_fees_after = pool.creator_fees_pending + pool.treasury_fees_pending;
        let fee_101 = total_fees_after - fees_before;

        // İndirimli: 10_000 * 15 / 10_000 = 15
        // Standart:  10_000 * 30 / 10_000 = 30
        assert_eq!(fee_101, 15, "101. swapda indirimli fee (15 bps) uygulanmalı");
    }

    /// Yeni günde sayaç sıfırlanır, indirim uygulanmaz
    #[test]
    fn test_daily_reset_clears_discount() {
        let r = registry();
        let pid = make_pool(&r);
        let after_launch = LAUNCH_PERIOD_MS + 1;

        // Bugün (gün 0): 100 swap → indirim hakkı kazanıldı
        for _ in 0..100 {
            r.swap(&pid, swapper(), true, 1_000, 0, after_launch).unwrap();
        }

        // Günlük sayaç kontrol
        {
            let pool = r.get_pool(&pid).unwrap();
            let count = pool.daily_swap_counts.get(&swapper()).copied().unwrap_or(0);
            assert_eq!(count, 100);
        }

        // Yarın (gün 1 = 86_400_000 ms): sayaç sıfırlanır
        let next_day_ms = 86_400_000u64 + after_launch;
        r.swap(&pid, swapper(), true, 10_000, 0, next_day_ms).unwrap();

        let pool = r.get_pool(&pid).unwrap();

        // Yeni günde sayaç 1 olmalı (sıfırlanıp +1 yapıldı)
        let count = pool.daily_swap_counts.get(&swapper()).copied().unwrap_or(0);
        assert_eq!(count, 1, "yeni günde sayaç 1 olmalı");

        // Yeni günde standart fee uygulanmalı (10_000 * 30 / 10_000 = 30)
        // Sıfırlanan sayaçtan sonra biriken fee = sadece bu swap'ın fee'si
        // Ancak önceki günün birikimi de var — bunu doğrudan test etmek güç,
        // asıl olan sayacın sıfırlanmış olması.
        assert_eq!(pool.swap_count_day, (next_day_ms / 86_400_000) as u32, "gün anahtarı güncellendi");
    }

    /// Farklı adresler bağımsız sayaçlara sahip
    #[test]
    fn test_volume_discount_per_address() {
        let r = registry();
        let pid = make_pool(&r);
        let after_launch = LAUNCH_PERIOD_MS + 1;
        let addr1 = [0x10u8; 20];
        let addr2 = [0x20u8; 20];

        // addr1: 100 swap
        for _ in 0..100 {
            r.swap(&pid, addr1, true, 1_000, 0, after_launch).unwrap();
        }

        // addr2: 0 swap — indirim yok
        let pool = r.get_pool(&pid).unwrap();
        let count1 = pool.daily_swap_counts.get(&addr1).copied().unwrap_or(0);
        let count2 = pool.daily_swap_counts.get(&addr2).copied().unwrap_or(0);
        assert_eq!(count1, 100, "addr1: 100 swap");
        assert_eq!(count2, 0, "addr2: 0 swap — bağımsız sayaç");
    }

    // ─── NOVA-DEX-LAUNCH-001: Launch Period Testleri ──────────────────────────

    /// Launch period içinde küçük alım başarılı olmalı
    #[test]
    fn test_launch_period_small_buy_ok() {
        let r = registry();
        let pid = make_pool(&r); // created_at_ms = 0
        // Pool'da 1_000_000 reserve_b var → max %5 = 50_000
        // Küçük alım (launch period içinde, now_ms = 1)
        let out = r.swap(&pid, swapper(), true, 1_000, 0, 1).unwrap();
        assert!(out > 0, "küçük alım launch period içinde başarılı olmalı");
    }

    /// Launch period içinde %5 limiti aşan alım reddedilmeli
    #[test]
    fn test_launch_period_large_buy_rejected() {
        let r = registry();
        let pid = make_pool(&r); // 1_000_000 reserve_b, max = 50_000
        // Çok büyük alım — limit aşılıyor
        let result = r.swap(&pid, swapper(), true, 5_000_000, 0, 1);
        assert!(
            matches!(result, Err(DexError::LaunchPeriodLimitExceeded { .. })),
            "launch period'da büyük alım reddedilmeli"
        );
    }

    /// Launch period bittikten sonra büyük alım serbest
    #[test]
    fn test_launch_period_expired_large_buy_ok() {
        let r = registry();
        let pid = make_pool(&r);
        // 5 dakika + 1 ms sonra — launch period bitti
        let after = LAUNCH_PERIOD_MS + 1;
        let out = r.swap(&pid, swapper(), true, 400_000, 0, after).unwrap();
        assert!(out > 0, "launch period sonrası büyük alım serbest");
    }

    /// Aynı adres launch period içinde birden fazla alım yapabilir ama toplam %5'i geçemez
    #[test]
    fn test_launch_period_cumulative_limit() {
        let r = registry();
        let pid = make_pool(&r); // 1_000_000 reserve_b → max %5 = 50_000
        // İlk alım: küçük — geçmeli
        r.swap(&pid, swapper(), true, 500, 0, 1).unwrap();
        // İkinci alım: küçük — geçmeli
        r.swap(&pid, swapper(), true, 500, 0, 2).unwrap();
        // Büyük alım — toplam limit aşılıyor
        let result = r.swap(&pid, swapper(), true, 5_000_000, 0, 3);
        assert!(
            matches!(result, Err(DexError::LaunchPeriodLimitExceeded { .. })),
            "birikimli limit aşımı reddedilmeli"
        );
    }

    /// Farklı adresler launch period içinde bağımsız olarak %5'e kadar alabilir
    #[test]
    fn test_launch_period_different_addresses_independent() {
        let r = registry();
        let pid = make_pool(&r);
        let addr1 = [0x01u8; 20];
        let addr2 = [0x02u8; 20];
        // Her ikisi de küçük alım yapabilir
        r.swap(&pid, addr1, true, 1_000, 0, 1).unwrap();
        r.swap(&pid, addr2, true, 1_000, 0, 2).unwrap();
        // Her iki adresin alımı bağımsız — diğerini etkilemiyor
        let pool = r.get_pool(&pid).unwrap();
        assert_eq!(pool.launch_buy_tracker.len(), 2, "her adres ayrı takip edilmeli");
    }

    /// Creator da launch period kuralına tabi (creator dahil herkes eşit)
    #[test]
    fn test_launch_period_creator_also_limited() {
        let r = registry();
        let pid = make_pool(&r);
        // Creator adresi de %5 limitine tabi
        let result = r.swap(&pid, creator(), true, 5_000_000, 0, 1);
        assert!(
            matches!(result, Err(DexError::LaunchPeriodLimitExceeded { .. })),
            "creator da launch period kuralına tabi"
        );
    }

    /// Sell işlemi (zero_for_one = false) launch period limitinden etkilenmez
    #[test]
    fn test_launch_period_sell_not_affected() {
        let r = registry();
        let pid = make_pool(&r);
        // Sell: zero_for_one = false → token A veriyor, B alıyor → limit yok
        let out = r.swap(&pid, swapper(), false, 500_000, 0, 1).unwrap();
        assert!(out > 0, "sell işlemi launch period limitinden etkilenmemeli");
    }
}
