//! KONFİG — .env yükleme ve doğrulama (borsa erişim kimliğinin tek kapısı).
//!
//! ROL: sürecin borsa bağlantı bilgilerini tek `Config` yapısında toplar.
//! 1. `dotenvy` ile .env'i süreç ENV'ine yükler (secret export gereksinimi yok).
//! 2. `Config::from_env`: NETWORK / HYPERLIQUID_API_URL / HYPERLIQUID_WS_URL /
//!    HYPERLIQUID_PRIVATE_KEY zorunlu; API_WALLET_ADDRESS opsiyonel.
//! 3. `validate`: https/wss ön ekleri ve 32 bayt hex-anahtar DOĞRULANIR —
//!    hatalı konfig süreci anında düşürür (fail-fast; yarım başlayan bot yok).
//! 4. `pk_hex`: imza anahtarını 0x'siz lowercase 32 byte hex olarak döner.
//!
//! ANA TİP: `Config` — tüm borsa istemcilerine (`HyperliquidClient::new` vb.)
//! klonlanarak geçilir.
//!
//! İNVARİANT: `private_key` asla log'a sızmaz — `Debug` derive YERİNE elle
//! maskelenmiş impl (`<REDACTED>`); yeni alan eklerken bunu elle koru,
//! derive'a geri dönme.
//!
//! YAPMAYANLAR: engine/strateji ENV'leri (ENGINE_*, TRAIL_*, LEARN_*) burada
//! DEĞİL — kendi modüllerinde okunur; API_WALLET_ADDRESS burada yalnız
//! taşınır, kullanım yerine signing/engine'dir.
//!
//! GENİŞLETME: yeni borsa ENV'i → alan + `from_env` + `validate` üçlüsü
//! birlikte güncellenir.
use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub struct Config {
    pub network: String,
    pub api_url: String,
    pub ws_url: String,
    /// API wallet (agent) private key, 0x'siz 32 byte hex
    pub private_key: String,
    pub agent_address: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // private_key ASLA log'a sızmamalı — Debug derive yerine maskelenmiş.
        f.debug_struct("Config")
            .field("network", &self.network)
            .field("api_url", &self.api_url)
            .field("ws_url", &self.ws_url)
            .field("private_key", &"<REDACTED>")
            .field("agent_address", &self.agent_address)
            .finish()
    }
}

impl Config {
    /// .env'den yukler; private key formatini dogrular.
    pub fn from_env() -> Result<Self> {
        // once calisma dizini, sonra workspace koku dene
        if dotenvy::dotenv().is_err() {
            let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../.env"));
        }

        let cfg = Self {
            network: std::env::var("NETWORK").context("NETWORK eksik")?,
            api_url: std::env::var("HYPERLIQUID_API_URL").context("HYPERLIQUID_API_URL eksik")?,
            ws_url: std::env::var("HYPERLIQUID_WS_URL").context("HYPERLIQUID_WS_URL eksik")?,
            private_key: std::env::var("HYPERLIQUID_PRIVATE_KEY")
                .context("HYPERLIQUID_PRIVATE_KEY eksik")?,
            agent_address: std::env::var("API_WALLET_ADDRESS").unwrap_or_default(),
        };

        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        let pk = self.private_key.trim().trim_start_matches("0x");
        if hex::decode(pk)
            .map_err(|e| anyhow::anyhow!("private key hex degil: {e}"))?
            .len()
            != 32
        {
            bail!("private key 32 byte olmali");
        }
        if !self.api_url.starts_with("https://") {
            bail!("api_url https olmali: {}", self.api_url);
        }
        if !self.ws_url.starts_with("wss://") {
            bail!("ws_url wss olmali: {}", self.ws_url);
        }
        Ok(())
    }

    /// 0x'siz, temiz 32 byte hex private key
    pub fn pk_hex(&self) -> String {
        self.private_key
            .trim()
            .trim_start_matches("0x")
            .to_ascii_lowercase()
    }
}
