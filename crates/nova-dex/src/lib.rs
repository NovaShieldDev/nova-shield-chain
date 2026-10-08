//! nova-dex — Nova DEX core library
//!
//! ## Güvenlik Özeti (Nova Red Team Audit Düzeltmeleri)
//!
//! | Bulgu ID                     | Düzeltme                                          |
//! |------------------------------|---------------------------------------------------|
//! | NOVA-RED-DEX-AUTH-001        | `auth.rs`: ed25519 imza doğrulaması her işlemde   |
//! | NOVA-RED-DEX-POOL-001        | `pool.rs`: nova_price_cents oracle'dan, user değil|
//! | NOVA-RED-DEX-POOL-002        | `pool.rs`: orantısız likidite fazlalık iadesi      |
//! | NOVA-RED-DEX-MEV-001         | `mev.rs`: otomatik commit cleanup                 |
//! | NOVA-RED-DEX-MEV-002         | `mev.rs`: direkt swap kaldırıldı, sadece MEV akışı|
//! | NOVA-RED-DEX-MEV-005         | `mev.rs`: reveal artık swap'ı execute ediyor      |
//! | NOVA-RED-DEX-MEV-006         | `mev.rs`: commitment_hash sender adresini içeriyor|
//! | NOVA-RED-DEX-SWAP-001        | `pool.rs`: bot sayacı slippage sonrası artırılıyor|
//! | NOVA-RED-DEX-PAYOUT-001      | `pool.rs`: payout_authority kontrolü              |
//! | NOVA-RED-DEX-SNAPSHOT-001    | `snapshot.rs`: versiyon alanı restore'da kontrol  |
//! | NOVA-RED-DEX-DB-001          | `snapshot.rs`: panic yerine Result döndür         |
//! | NOVA-RED-DEX-FEE-001         | `pool.rs`: tek fee implementasyonu (LP_FEE_BPS)   |
//! | NOVA-RED-DEX-SERVER-003      | `nova-dex-server`: CORS kısıtlanacak (aşağıda)    |
//! | NOVA-RED-DEX-SERVER-004      | `nova-dex-server`: L1 client daima etkin           |
//! | NOVA-RED-DEX-TLS-001         | Deployment dokümantasyonu: HTTPS zorunlu           |
//! | NOVA-RED-DEX-HEALTH-001      | Server'da /health endpoint bilgi sınırlandırıldı  |

pub mod auth;
pub mod errors;
pub mod mev;
pub mod pool;
pub mod snapshot;

pub use auth::{
    SignedRequest, address_from_pubkey, address_to_hex,
    compute_request_hash, parse_hex_address, verify_signed_request,
};
pub use errors::{DexError, DexResult};
pub use mev::{MevProtectedMempool, SwapIntent, SLOT_DURATION_MS};
pub use pool::{
    LockDuration, Pool, PoolRegistry,
    CREATION_FEE_USD_CENTS, LP_FEE_BPS, NOVA_DECIMALS,
};
pub use snapshot::{RegistrySnapshot, SNAPSHOT_VERSION, open_db};
