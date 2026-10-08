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

/// LP fee: swap başına binde 3 (%0.3)
pub const LP_FEE_BPS: u64 = 30; // 30 bps = 0.3%

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
    /// Bot swap sayacı
    pub bot_swap_count: u64,
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
    /// NOVA-RED-DEX-SWAP-001: Bot sayacı başarılı swap'tan SONRA artırılır.
    pub fn swap(
        &self,
        pool_id: &str,
        swapper: [u8; 20],
        zero_for_one: bool,
        amount_in: u128,
        min_amount_out: u128,
    ) -> DexResult<u128> {
        if amount_in == 0 {
            return Err(DexError::ZeroAmount);
        }

        let mut pools = self.pools.write();
        let pool = pools.get_mut(pool_id)
            .ok_or_else(|| DexError::PoolNotFound { pool_id: pool_id.into() })?;

        let (reserve_in, reserve_out) = if zero_for_one {
            (pool.reserve_a, pool.reserve_b)
        } else {
            (pool.reserve_b, pool.reserve_a)
        };

        if reserve_in == 0 || reserve_out == 0 {
            return Err(DexError::InsufficientLiquidity);
        }

        // AMM: fee uygulanmış amount_in
        let fee = amount_in * LP_FEE_BPS as u128 / 10_000;
        let amount_in_with_fee = amount_in.saturating_sub(fee);

        // x * y = k → dy = y * dx / (x + dx)
        let amount_out = reserve_out
            .checked_mul(amount_in_with_fee)
            .and_then(|n| n.checked_div(reserve_in + amount_in_with_fee))
            .unwrap_or(0);

        // NOVA-RED-DEX-SWAP-001: Slippage kontrolü bot sayacından ÖNCE
        if amount_out < min_amount_out {
            return Err(DexError::SlippageExceeded { got: amount_out, min: min_amount_out });
        }

        // Rezervleri güncelle
        if zero_for_one {
            pool.reserve_a += amount_in;
            pool.reserve_b = pool.reserve_b.saturating_sub(amount_out);
        } else {
            pool.reserve_b += amount_in;
            pool.reserve_a = pool.reserve_a.saturating_sub(amount_out);
        }

        pool.total_volume += amount_in;

        // NOVA-RED-DEX-SWAP-001: Bot sayacı yalnızca başarılı swap'ta artır
        let _ = swapper; // swapper kaydı (gelecekte kullanılabilir)
        pool.bot_swap_count += 1;

        tracing::debug!(pool_id, amount_in = %amount_in, amount_out = %amount_out, "swap");
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
    pub fn process_payouts(&self, caller: [u8; 20]) -> DexResult<u64> {
        if caller != self.payout_authority {
            return Err(DexError::PayoutUnauthorized);
        }
        // Pool başına fee birikimi hesapla (basit implementasyon)
        let count = self.pools.read().len() as u64;
        tracing::info!("payout işlendi: {} pool", count);
        Ok(count)
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
        let out = r.swap(&pid, swapper(), true, 1_000, 0).unwrap();
        assert!(out > 0);
    }

    /// NOVA-RED-DEX-SWAP-001: Slippage başarısızlığında bot sayacı artmamalı
    #[test]
    fn test_swap_bot_counter_not_incremented_on_slippage_failure() {
        let r = registry();
        let pid = make_pool(&r);
        let count_before = r.get_pool(&pid).unwrap().bot_swap_count;
        let result = r.swap(&pid, swapper(), true, 100, u128::MAX);
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
}
