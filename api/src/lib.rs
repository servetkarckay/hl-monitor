//! API — Hyperliquid veri ve emir erişim katmanı (façade).
//!
//! ROL: borsa I/O'sunu tipli, test edilebilir yüzeylere indirmek; bot
//! mantığı İÇERMEZ (sinyal üretimi strategy'de, pozisyon/karar engine'de).
//! Alt modüller:
//! `config`            — .env yükleme/doğrulama (`Config`)
//! `signing`           — EIP-712 phantom-agent imzası + HTTP istemci
//! `exchange_endpoint` — emir/iptal iletimi + fiyat/lot yuvarlama
//! `info_endpoint`     — market/hesap okuma metotları
//! `error_responses`   — `ApiError` hata taksonomisi
//! `types`             — info endpoint wire şemaları
//! `websocket`         — canlı akış (ping + otomatik yeniden bağlanma)
//! `supervisor`        — arka plan görevi keep-alive döngüsü
//!
//! İNVARİANT: imzalı /exchange yolu 5xx/timeout'ta asla retry etmez —
//! `ExchangeOutcome::Unknown` döner ve reconciliation çağırana aittir;
//! bu crate'e yeni emir yolu eklerken aynı kurala bağlan.
//!
//! KOMŞULAR: data (`ingestor` WsClient/Info kullanır), engine (Exchange/Info
//! ile emir ve reconciliation). GENİŞLETME: yeni borsa metodu ilgili endpoint
//! modülüne (yazma → exchange_endpoint, okuma → info_endpoint), yeni wire
//! şeması `types`'a girer.
pub mod config;
pub mod error_responses;
pub mod exchange_endpoint;
pub mod info_endpoint;
pub mod signing;
pub mod supervisor;
pub mod types;
pub mod websocket;

pub use config::Config;
pub use exchange_endpoint::{
    perp_price, perp_price_verified, round_5sig, round_px_for, round_px_wire, round_sz_up,
    Exchange, OrderStatuses, Tif,
};
pub use info_endpoint::Info;
pub use signing::{now_ms, HyperliquidClient, L1Signature};
pub use websocket::WsClient;
