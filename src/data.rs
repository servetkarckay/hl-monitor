use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Overview {
    pub account_value: f64,
    pub withdrawable: f64,
    pub total_ntl_pos: f64,
    pub total_margin_used: f64,
    pub cross_maintenance: f64,
}

#[derive(Debug, Clone)]
pub struct Position {
    pub coin: String,
    pub side: String,
    pub size: f64,
    pub entry_px: f64,
    pub mark_px: Option<f64>,
    pub unrealized_pnl: f64,
    pub liq_px: Option<f64>,
    pub leverage: f64,
    pub notional: f64,
}

fn num(v: &Value, key: &str) -> f64 {
    v.get(key)
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| v.get(key).and_then(|x| x.as_f64()))
        .unwrap_or(0.0)
}

pub fn parse_clearinghouse(v: &Value) -> (Overview, Vec<Position>) {
    let ms = v.get("marginSummary").cloned().unwrap_or(Value::Null);
    let ov = Overview {
        account_value: num(&ms, "accountValue"),
        withdrawable: num(v, "withdrawable"),
        total_ntl_pos: num(&ms, "totalNtlPos"),
        total_margin_used: num(&ms, "totalMarginUsed"),
        cross_maintenance: num(v, "crossMaintenanceMarginUsed"),
    };
    let mut positions = Vec::new();
    if let Some(arr) = v.get("assetPositions").and_then(|x| x.as_array()) {
        for ap in arr {
            let p = ap.get("position").cloned().unwrap_or(Value::Null);
            let szi: f64 = num(&p, "szi");
            let side = if szi >= 0.0 { "Long" } else { "Short" };
            let lev = p
                .get("leverage")
                .and_then(|l| l.get("value"))
                .map(num_wrap)
                .unwrap_or(0.0);
            positions.push(Position {
                coin: p.get("coin").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                side: side.to_string(),
                size: szi.abs(),
                entry_px: num(&p, "entryPx"),
                mark_px: None,
                unrealized_pnl: num(&p, "unrealizedPnl"),
                liq_px: p.get("liquidationPx").and_then(|x| {
                    if let Some(s) = x.as_str() { s.parse().ok() } else { x.as_f64() }
                }),
                leverage: lev,
                notional: num(&p, "positionValue"),
            });
        }
    }
    (ov, positions)
}

fn num_wrap(v: &Value) -> f64 {
    if let Some(s) = v.as_str() { s.parse().unwrap_or(0.0) } else { v.as_f64().unwrap_or(0.0) }
}

/// Zaman aralığı sayfalama — Hyperliquid kuralı: son dönen kaydın
/// timestamp'i SONRAKİ isteğin startTime'ıdır (KAPSAYICI). Çakışan
/// kayıtlar çağıranın dedup'ı ile atılır; böylece sayfa sınırındaki
/// aynı-zamanlı kayıtlar kaybolmaz.
/// Dönüş: (kayıtlar, truncated) — true = sayfalama hata/ilerleme-yok
/// nedeniyle ERKEN BİTTİ, veri eksik olabilir.
pub async fn paginate_time_range<F, Fut>(
    start_ms: i64,
    end_ms: i64,
    page_size: usize,
    mut fetch: F,
) -> (Vec<serde_json::Value>, bool)
where
    F: FnMut(i64, i64) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<serde_json::Value>, String>>,
{
    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut cursor = start_ms;
    loop {
        let arr = match fetch(cursor, end_ms).await {
            Ok(a) => a,
            Err(_) => return (out, true),
        };
        let n = arr.len();
        let page_max = arr.iter().map(|f| f.get("time").map(as_i64).unwrap_or(0)).max().unwrap_or(0);
        out.extend(arr);
        if n < page_size { return (out, false); } // sayfa dolmadı = aralık bitti
        if page_max >= end_ms { return (out, false); }
        if page_max <= cursor { return (out, true); } // ilerleme yok -> güvenli dur
        cursor = page_max; // KAPSAYICI: aynı timestamp tekrar gelir, dedup atar
    }
}

pub fn parse_spot(v: &Value) -> Vec<(String, f64, f64)> {
    let mut out = Vec::new();
    if let Some(bals) = v.get("balances").and_then(|x| x.as_array()) {
        for b in bals {
            out.push((
                b.get("coin").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                num(b, "total"),
                num(b, "hold"),
            ));
        }
    }
    out
}

/// spotMeta -> token adı -> allMids pair anahtarı ("@<index>").
/// Yalnız USDC'ye kote pairler eşlenir (quote token = "USDC"): pair
/// mid fiyatı = base token'ın USDC fiyatı. Eşleşmeyen tokenlar
/// "unpriceable" olarak raporlanır (değeri 0 varsayılmaz).
pub fn parse_spot_keys(v: &Value) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let (Some(tokens), Some(universe)) = (
        v.get("tokens").and_then(|x| x.as_array()),
        v.get("universe").and_then(|x| x.as_array()),
    ) else { return out; };
    let name_of = |idx: i64| -> String {
        tokens
            .iter()
            .find(|t| t.get("index").map(as_i64) == Some(idx))
            .and_then(|t| t.get("name"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };
    for p in universe {
        let tks = p.get("tokens").and_then(|x| x.as_array()).cloned().unwrap_or_default();
        if tks.len() != 2 { continue; }
        let (base, quote) = (as_i64(&tks[0]), as_i64(&tks[1]));
        if name_of(quote) != "USDC" { continue; }
        let base_name = name_of(base);
        if base_name.is_empty() || base_name == "USDC" { continue; }
        // allMids anahtarı = PAIR ADININ KENDİSİ: non-canonical "@107",
        // canonical "PURR/USDC" ("@0" allMids'te YOK — canlı yanıtla
        // doğrulandı: PURR/USDC anahtarı var, @0 yok).
        let key = p.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if key.is_empty() { continue; }
        out.insert(base_name, key);
    }
    out
}

#[derive(Debug, Clone)]
pub struct Order {
    pub oid: i64,
    pub coin: String,
    pub side: String,
    pub order_type: String,
    pub price: f64,
    pub size: f64,
    pub orig_size: f64,
    pub reduce_only: bool,
    pub trigger: bool,
    pub trigger_px: f64,
    pub cloid: Option<String>,
    pub timestamp: i64,
}

fn as_i64(v: &Value) -> i64 {
    if let Some(n) = v.as_i64() { n }
    else if let Some(n) = v.as_u64() { n as i64 }
    else if let Some(s) = v.as_str() { s.parse().unwrap_or(0) }
    else { 0 }
}

pub fn parse_orders(v: &Value) -> Vec<Order> {
    let arr = if let Some(a) = v.as_array() { a.clone() }
        else if let Some(a) = v.get("frontendOpenOrders").and_then(|x| x.as_array()) { a.clone() }
        else if let Some(a) = v.get("orders").and_then(|x| x.as_array()) { a.clone() }
        else if let Some(a) = v.get("data").and_then(|x| x.as_array()) { a.clone() }
        else { Vec::new() };
    let mut out = Vec::new();
    for o in &arr {
        let side_raw = o.get("side").and_then(|x| x.as_str()).unwrap_or("");
        let side = match side_raw { "B" => "Buy", "A" => "Sell", other => other }.to_string();
        let price = { let a = num(o, "limitPx"); if a == 0.0 && o.get("limitPx").is_none() { num(o, "limit_px") } else { a } };
        let size = { let a = num(o, "sz"); if a == 0.0 && o.get("sz").is_none() { num(o, "size") } else { a } };
        let orig_size_raw = o.get("origSz").or_else(|| o.get("orig_sz")).or_else(|| o.get("origSize"));
        let orig_size = match orig_size_raw {
            Some(raw) if raw.is_null() => size,
            Some(raw) => if let Some(s) = raw.as_str() { s.parse().unwrap_or(size) } else { raw.as_f64().unwrap_or(size) },
            None => size,
        };
        let reduce_only = o.get("reduceOnly").or_else(|| o.get("reduce_only")).and_then(|x| x.as_bool()).unwrap_or(false);
        let trigger = o.get("isTrigger").or_else(|| o.get("is_trigger")).and_then(|x| x.as_bool()).unwrap_or(false);
        let trigger_px = { let a = num(o, "triggerPx"); if a == 0.0 && o.get("triggerPx").is_none() { num(o, "trigger_px") } else { a } };
        let cloid = o.get("cloid").and_then(|x| if x.is_null() { None } else { x.as_str().map(|s| s.to_string()).or_else(|| Some(x.to_string())) });
        let timestamp = o.get("timestamp").map(as_i64).unwrap_or(0);
        let oid = o.get("oid").map(as_i64).unwrap_or(0);
        let order_type = o.get("orderType").or_else(|| o.get("order_type")).or_else(|| o.get("type")).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let coin = o.get("coin").and_then(|x| x.as_str()).unwrap_or("").to_string();
        out.push(Order { oid, coin, side, order_type, price, size, orig_size, reduce_only, trigger, trigger_px, cloid, timestamp });
    }
    out
}

#[derive(Debug, Clone)]
pub struct Fill {
    pub time: i64,
    pub coin: String,
    pub side: String,
    pub dir: String,
    pub px: f64,
    pub sz: f64,
    pub closed_pnl: f64,
    pub fee: f64,
    pub hash: String,
    pub oid: i64,
}

pub fn parse_fills(v: &Value) -> Vec<Fill> {
    let arr = if let Some(a) = v.as_array() { a.clone() }
        else if let Some(a) = v.get("userFills").and_then(|x| x.as_array()) { a.clone() }
        else if let Some(a) = v.get("fills").and_then(|x| x.as_array()) { a.clone() }
        else if let Some(a) = v.get("data").and_then(|x| x.as_array()) { a.clone() }
        else { Vec::new() };
    let mut out = Vec::new();
    for f in &arr {
        out.push(Fill {
            time: f.get("time").map(as_i64).unwrap_or(0),
            coin: f.get("coin").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            side: f.get("side").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            dir: f.get("dir").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            px: num(f, "px"),
            sz: num(f, "sz"),
            closed_pnl: num(f, "closedPnl"),
            fee: num(f, "fee"),
            hash: f.get("hash").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            oid: f.get("oid").map(as_i64).unwrap_or(0),
        });
    }
    out
}

#[derive(Debug, Clone)]
pub struct Funding {
    pub time: i64,
    pub coin: String,
    pub funding_rate: f64,
    pub usdc: f64,
    pub szi: f64,
    pub n_samples: i64,
}

pub fn parse_funding(v: &Value) -> Vec<Funding> {
    let arr = if let Some(a) = v.as_array() { a.clone() }
        else if let Some(a) = v.get("userFunding").and_then(|x| x.as_array()) { a.clone() }
        else if let Some(a) = v.get("data").and_then(|x| x.as_array()) { a.clone() }
        else { Vec::new() };
    let mut out = Vec::new();
    for item in &arr {
        let time = item.get("time").map(as_i64).unwrap_or(0);
        let delta = item.get("delta").cloned().unwrap_or_else(|| item.clone());
        let coin = delta.get("coin").and_then(|x| x.as_str()).or_else(|| item.get("coin").and_then(|x| x.as_str())).unwrap_or("").to_string();
        if coin.is_empty() { continue; }
        out.push(Funding {
            time,
            coin,
            funding_rate: { let a = num(&delta, "fundingRate"); if a == 0.0 && delta.get("fundingRate").is_none() { num(&delta, "funding_rate") } else { a } },
            usdc: { let a = num(&delta, "usdc"); if a == 0.0 && delta.get("usdc").is_none() { num(item, "usdc") } else { a } },
            szi: { let a = num(&delta, "szi"); if a == 0.0 && delta.get("szi").is_none() { num(item, "szi") } else { a } },
            n_samples: delta.get("nSamples").or_else(|| delta.get("n_samples")).map(as_i64).unwrap_or(0),
        });
    }
    out
}

#[derive(Debug, Clone)]
pub struct Fees {
    pub cross_rate: f64,
    pub add_rate: f64,
    pub referral_discount: f64,
}

pub fn parse_fees(v: &Value) -> Fees {
    Fees {
        cross_rate: num(v, "userCrossRate"),
        add_rate: num(v, "userAddRate"),
        referral_discount: num(v, "activeReferralDiscount"),
    }
}

#[derive(Debug, Clone)]
pub struct Portfolio {
    pub periods: Vec<(String, f64, f64, f64)>,
}

pub fn parse_portfolio(v: &Value) -> Portfolio {
    let arr = if let Some(a) = v.as_array() { a.clone() }
        else if let Some(a) = v.get("portfolio").and_then(|x| x.as_array()) { a.clone() }
        else { Vec::new() };
    let mut periods = Vec::new();
    for item in &arr {
        let (period, obj) = if let Some(a) = item.as_array() {
            (a.first().and_then(|x| x.as_str()).unwrap_or("").to_string(), a.get(1).cloned().unwrap_or(Value::Null))
        } else {
            (item.get("period").and_then(|x| x.as_str()).unwrap_or("").to_string(), item.clone())
        };
        let last_av = if let Some(h) = obj.get("accountValueHistory").and_then(|x| x.as_array()) {
            h.last().and_then(|e| if let Some(a) = e.as_array() { a.get(1).map(num_wrap) } else { e.get(1).map(num_wrap).or_else(|| e.as_f64()) }).unwrap_or(0.0)
        } else { num(&obj, "accountValue") };
        let last_pnl = if let Some(h) = obj.get("pnlHistory").and_then(|x| x.as_array()) {
            h.last().and_then(|e| if let Some(a) = e.as_array() { a.get(1).map(num_wrap) } else { e.get(1).map(num_wrap).or_else(|| e.as_f64()) }).unwrap_or(0.0)
        } else { num(&obj, "pnl") };
        let volume = num(&obj, "vlm");
        if period.is_empty() && obj.is_null() { continue; }
        periods.push((period, last_av, last_pnl, volume));
    }
    Portfolio { periods }
}

pub fn parse_mids(v: &Value) -> HashMap<String, f64> {
    let obj = if v.get("mids").and_then(|x| x.as_object()).is_some() { v.get("mids").cloned().unwrap_or(Value::Null) }
        else if v.get("allMids").and_then(|x| x.as_object()).is_some() { v.get("allMids").cloned().unwrap_or(Value::Null) }
        else if v.as_object().is_some() { v.clone() }
        else { Value::Null };
    let mut out = HashMap::new();
    if let Some(map) = obj.as_object() {
        for (k, val) in map {
            if let Ok(f) = if let Some(s) = val.as_str() { s.parse() } else { val.as_f64().map(|f| f.to_string()).unwrap_or_default().parse() } {
                out.insert(k.clone(), f);
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct CtxLite {
    pub funding: f64,
    pub open_interest: f64,
    pub mark_px: f64,
    pub oracle_px: f64,
    pub day_ntl_vlm: f64,
}

pub fn parse_ctx(v: &Value) -> HashMap<String, CtxLite> {
    let (meta, ctxs) = if let Some(arr) = v.as_array() {
        if arr.len() >= 2 { (arr[0].clone(), arr[1].clone()) } else { (Value::Null, Value::Null) }
    } else if let (Some(m), Some(c)) = (v.get("meta"), v.get("ctxs")) { (m.clone(), c.clone()) }
    else if let (Some(m), Some(c)) = (v.get("meta"), v.get("assetCtxs")) { (m.clone(), c.clone()) }
    else { (Value::Null, Value::Null) };
    let universe = meta.get("universe").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let ctx_arr = ctxs.as_array().cloned().unwrap_or_default();
    let mut out = HashMap::new();
    for (i, m) in universe.iter().enumerate() {
        let name = m.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if name.is_empty() { continue; }
        if let Some(c) = ctx_arr.get(i) {
            out.insert(name, CtxLite {
                funding: num(c, "funding"),
                open_interest: num(c, "openInterest"),
                mark_px: num(c, "markPx"),
                oracle_px: num(c, "oraclePx"),
                day_ntl_vlm: num(c, "dayNtlVlm"),
            });
        }
    }
    out
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum DataMsg {
    Snapshot {
        overview: Overview,
        positions: Vec<Position>,
        spot: Vec<(String, f64, f64)>,
        orders: Vec<Order>,
        fills: Vec<Fill>,
        funding: Vec<Funding>,
        fees: Fees,
        portfolio: Portfolio,
        mids: HashMap<String, f64>,
        ctx: HashMap<String, CtxLite>,
        latency_ms: f64,
        ob: Option<hyperliquid_api::types::L2Book>,
        rate_limit: (u64, u64),
        fills_truncated: bool,
        funding_truncated: bool,
        spot_px: HashMap<String, f64>,
    },
    Error(String),
}

#[derive(Debug, Clone)]
pub enum TapeMsg {
    Trade { time: i64, coin: String, side: String, px: f64, sz: f64 },
    UserEvent(Value),
    Connected,
    Disconnected,
}

pub async fn tape_loop(cfg: hyperliquid_api::Config, coins: Vec<String>, tx: tokio::sync::mpsc::Sender<TapeMsg>) {
    let coins = if coins.is_empty() {
        match hyperliquid_api::Info::new(cfg.clone()) {
            Ok(info) => match info.meta().await {
                Ok(meta) => meta.universe.iter().map(|m| m.name.clone()).collect(),
                Err(_) => vec!["BTC".into(), "ETH".into(), "SOL".into(), "HYPE".into()],
            },
            Err(_) => vec!["BTC".into(), "ETH".into(), "SOL".into(), "HYPE".into()],
        }
    } else { coins };
    let user = std::env::var("USER_ADDRESS").unwrap_or_default();
    let client = hyperliquid_api::WsClient::new(cfg);
    let mut subs: Vec<serde_json::Value> = Vec::new();
    for coin in &coins { subs.push(serde_json::json!({"type":"trades","coin":coin})); }
    if !user.is_empty() { subs.push(serde_json::json!({"type":"userEvents","user": user.to_ascii_lowercase()})); }
    let (ws_tx, mut ws_rx) = tokio::sync::mpsc::channel::<hyperliquid_api::websocket::WsEvent>(256);
    let _ = tx.send(TapeMsg::Connected).await;
    let run_handle = tokio::spawn(async move { let _ = client.run(subs, ws_tx).await; });
    while let Some(ev) = ws_rx.recv().await {
        match ev.channel.as_str() {
            "trades" => {
                let items = if let Some(arr) = ev.data.as_array() { arr.clone() } else { vec![ev.data.clone()] };
                for t in items {
                    let coin = t.get("coin").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let side = t.get("side").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let px = t.get("px").map(num_wrap).unwrap_or_else(|| t.get("price").map(num_wrap).unwrap_or(0.0));
                    let sz = t.get("sz").map(num_wrap).or_else(|| t.get("size").map(num_wrap)).unwrap_or(0.0);
                    let time = t.get("time").map(as_i64).unwrap_or(0);
                    let _ = tx.send(TapeMsg::Trade { time, coin, side, px, sz }).await;
                }
            }
            "userEvents" | "user" => { let _ = tx.send(TapeMsg::UserEvent(ev.data)).await; }
            _ => {}
        }
    }
    let _ = tx.send(TapeMsg::Disconnected).await;
    let _ = run_handle.await;
}

pub async fn poll_loop(cfg: hyperliquid_api::Config, user: String, interval: std::time::Duration, funding_days: i64, coins: Vec<String>, tx: tokio::sync::mpsc::Sender<DataMsg>) {
    let info = match hyperliquid_api::Info::new(cfg) { Ok(i) => i, Err(e) => { let _ = tx.send(DataMsg::Error(format!("{e}"))).await; return; } };
    let user_lc = user.to_ascii_lowercase();
    let _ = coins;
    // spotMeta eşlemesi: token adı ("HYPE") -> allMids pair anahtarı ("@107").
    // allMids spot fiyatları "@<pair index>" anahtarlarıyla döner;
    // spotClearinghouseState bakiyeleri ise TOKEN adıyla. Eşleme olmadan
    // spot tokenlar 0 değerlenir ya da aynı adlı perp fiyatıyla değerlenir.
    let mut spot_keys: HashMap<String, String> = HashMap::new();
    loop {
        let loop_start = std::time::Instant::now();
        if spot_keys.is_empty() {
            if let Ok(v) = info.c.info(serde_json::json!({"type":"spotMeta"})).await {
                spot_keys = parse_spot_keys(&v);
            }
        }
        let ch_res = info.clearinghouse_state(&user).await;
        let ob_res = info.l2_book("HYPE").await;
        let spot_res = info.c.info(serde_json::json!({"type":"spotClearinghouseState","user": user_lc})).await;
        let orders_res = info.frontend_open_orders(&user).await;
        let now_ms = hyperliquid_api::now_ms() as i64;
        let start_ms = now_ms - funding_days * 86400 * 1000;
        // Paginated fills: userFills (last 2000) + userFillsByTime for older.
        // fills_truncated YALNIZ gerçek eksiklikte true: ilk çağrı hatası,
        // sayfalama hatası, veya 10.000 kayıt tavanına değmek.
        // Tam sayfalama başarılıysa (geçmişe ulaşıldı) bayrak temizlenir.
        let mut fills_all: Vec<serde_json::Value> = Vec::new();
        let mut fills_err = false;
        match info.user_fills(&user).await {
            Ok(v) => {
                if let Some(arr) = v.as_array() {
                    fills_all.extend(arr.clone());
                }
            }
            // Sessiz yutma yok: ilk çağrı başarısızsa geçmiş EKSİKTİR,
            // kullanıcıya uyarı gösterilir.
            Err(_) => { fills_err = true; }
        }
        // userFills sayfa tavanını doldurduysa daha eski kayıtlar için
        // userFillsByTime: [start_ms, ilk sayfanın en eski zamanı] KAPSAYICI
        // (aynı zamanlı sınır kayıtları kaçmasın) — dedup by tid çakışmayı atar.
        if !fills_err && fills_all.len() >= 2000 {
            let first_min = fills_all.iter().map(|f| f.get("time").map(as_i64).unwrap_or(0)).min().unwrap_or(now_ms);
            if first_min >= start_ms {
                let client = info.c.clone();
                let user2 = user_lc.clone();
                let (older, trunc) = paginate_time_range(start_ms, first_min, 2000, move |s, e| {
                    let client = client.clone();
                    let user2 = user2.clone();
                    async move {
                        match client.info(serde_json::json!({"type":"userFillsByTime","user": user2, "startTime": s, "endTime": e})).await {
                            Ok(v) => Ok(v.as_array().cloned().unwrap_or_default()),
                            Err(e) => Err(format!("{e}")),
                        }
                    }
                }).await;
                fills_all.extend(older);
                if trunc { fills_err = true; }
            }
        }
        let fills_truncated = fills_err || fills_all.len() >= 10000;
        // Dedup fills by tid (API benzersiz trade id); tid yoksa (time, hash)
        {
            let mut seen = std::collections::HashSet::new();
            fills_all.retain(|f| {
                let key = match f.get("tid") {
                    Some(t) => format!("tid:{}", t),
                    None => format!("{}-{}", f.get("time").map(as_i64).unwrap_or(0), f.get("hash").and_then(|x| x.as_str()).unwrap_or("")),
                };
                seen.insert(key)
            });
        }
        // Paginated funding: 30 günlük pencereler + pencere İÇİNDE forward
        // cursor sayfalama (Hyperliquid kuralı: son dönen timestamp SONRAKİ
        // startTime — kapsayıcı; çakışan kayıtlar dedup'la atılır).
        // userFunding sayfa tavanı ~500: pencereyi tek çağrıyla kapatmak
        // kayıt kaybettirir.
        let mut funding_all: Vec<serde_json::Value> = Vec::new();
        let mut funding_truncated = false;
        let mut window_end = now_ms;
        let mut window_start = (now_ms - 30 * 86400 * 1000).max(start_ms);
        while window_start <= window_end && window_start <= now_ms {
            let client = info.c.clone();
            let user2 = user_lc.clone();
            let (mut page_recs, trunc) = paginate_time_range(window_start, window_end, 500, move |s, e| {
                let client = client.clone();
                let user2 = user2.clone();
                async move {
                    match client.info(serde_json::json!({"type":"userFunding","user": user2, "startTime": s, "endTime": e})).await {
                        Ok(v) => {
                            let arr = if let Some(a) = v.as_array() { a.clone() }
                                else if let Some(a) = v.get("userFunding").and_then(|x| x.as_array()) { a.clone() }
                                else { Vec::new() };
                            Ok(arr)
                        }
                        Err(e) => Err(format!("{e}")),
                    }
                }
            }).await;
            funding_all.append(&mut page_recs);
            if trunc { funding_truncated = true; }
            if window_start <= start_ms { break; }
            window_end = window_start - 1;
            window_start = (window_end - 30 * 86400 * 1000).max(start_ms);
        }
        // Dedup funding by (time, hash) — aynı saniyede aynı coin'e birden
        // çok funding kaydı gelebilir; (time, coin) bunları kaybettirir.
        {
            let mut seen = std::collections::HashSet::new();
            funding_all.retain(|f| {
                let t = f.get("time").map(as_i64).unwrap_or(0);
                let hash = f.get("hash").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let delta = f.get("delta").cloned().unwrap_or_else(|| f.clone());
                let coin = delta.get("coin").and_then(|x| x.as_str()).or_else(|| f.get("coin").and_then(|x| x.as_str())).unwrap_or("").to_string();
                seen.insert(format!("{}-{}-{}", t, hash, coin))
            });
        }
        let fees_res = info.user_fees(&user).await;
        let portfolio_res = info.c.info(serde_json::json!({"type":"portfolio","user": user_lc})).await;
        let mids_res = info.all_mids().await;
        let meta_ctx_res = info.meta_and_asset_ctxs().await;

        let mut err_msg = None;
        let mut ch_val = None; let mut spot_val = None; let mut orders_val = None;
        let mut fees_val = None; let mut portfolio_val = None;
        let mut mids_val = None; let mut meta_ctx_val = None;
        let mut rl_val: Option<(u64, u64)> = None;

        macro_rules! collect {
            ($res:expr, $slot:ident) => {
                match $res {
                    Ok(v) => $slot = Some(v),
                    Err(e) => { if err_msg.is_none() { err_msg = Some(format!("{e}")); } }
                }
            };
        }
        collect!(ch_res, ch_val); collect!(spot_res, spot_val); collect!(orders_res, orders_val);
        collect!(fees_res, fees_val);
        collect!(portfolio_res, portfolio_val); collect!(mids_res, mids_val); collect!(meta_ctx_res, meta_ctx_val);
        // rate limit
        if let Ok(v) = info.c.info(serde_json::json!({"type":"userRateLimit","user": user_lc})).await {
            let used = v.get("nRequestsUsed").and_then(|x| x.as_u64()).unwrap_or(0);
            let cap = v.get("nRequestsCap").and_then(|x| x.as_u64()).unwrap_or(0);
            rl_val = Some((used, cap));
        }

        if let Some(msg) = err_msg {
            if tx.send(DataMsg::Error(msg)).await.is_err() { return; }
            tokio::time::sleep(interval).await;
            continue;
        }
        let fills_val = serde_json::Value::Array(fills_all.clone());
        let funding_val = serde_json::Value::Array(funding_all.clone());
        let (overview, positions) = parse_clearinghouse(&ch_val.unwrap());
        let spot = parse_spot(&spot_val.unwrap());
        let orders = parse_orders(&orders_val.unwrap());
        let fills = parse_fills(&fills_val);
        let funding = parse_funding(&funding_val);
        let fees = parse_fees(&fees_val.unwrap());
        let portfolio = parse_portfolio(&portfolio_val.unwrap());
        let mids_raw = mids_val.unwrap();
        let mids: HashMap<String, f64> = mids_raw.into_iter().filter_map(|(k, v)| v.parse().ok().map(|f| (k, f))).collect();
        // Spot token fiyatları: allMids'ten pair anahtarıyla ("@107") okunur.
        let mut spot_px: HashMap<String, f64> = HashMap::new();
        for (tok, key) in &spot_keys {
            if let Some(px) = mids.get(key) {
                spot_px.insert(tok.clone(), *px);
            }
        }
        let (meta, ctxs) = meta_ctx_val.unwrap();
        let mut ctx_map: HashMap<String, CtxLite> = HashMap::new();
        for (i, m) in meta.universe.iter().enumerate() {
            if let Some(c) = ctxs.get(i) {
                ctx_map.insert(m.name.clone(), CtxLite {
                    funding: c.funding.parse().unwrap_or(0.0),
                    open_interest: c.openInterest.parse().unwrap_or(0.0),
                    mark_px: c.markPx.parse().unwrap_or(0.0),
                    oracle_px: c.oraclePx.parse().unwrap_or(0.0),
                    day_ntl_vlm: c.dayNtlVlm.parse().unwrap_or(0.0),
                });
            }
        }
        let latency_ms = loop_start.elapsed().as_secs_f64() * 1000.0;
        let ob = ob_res.ok();
        let rate_limit = rl_val.unwrap_or((0, 0));
        let msg = DataMsg::Snapshot { overview, positions, spot, orders, fills, funding, fees, portfolio, mids, ctx: ctx_map, latency_ms, ob, rate_limit, fills_truncated, funding_truncated, spot_px };
        if tx.send(msg).await.is_err() { return; }
        tokio::time::sleep(interval).await;
    }
}
#[cfg(test)]
mod spot_keys_tests {
    use super::*;
    use serde_json::json;

    /// REGRESYON: spot bakiyeleri allMids'ten TOKEN adıyla aranıyordu
    /// ama allMids spot fiyatları "@<pair index>" anahtarlıdır.
    /// parse_spot_keys token adını doğru pair anahtarına eşlemeli.
    #[test]
    fn parse_spot_keys_usdc_koteli() {
        let v = json!({
            "tokens": [
                {"index": 0, "name": "USDC"},
                {"index": 150, "name": "HYPE"},
                {"index": 2, "name": "PURR"}
            ],
            "universe": [
                {"name": "@107", "index": 107, "tokens": [150, 0]},
                {"name": "PURR/USDC", "index": 0, "tokens": [2, 0]}
            ]
        });
        let m = parse_spot_keys(&v);
        assert_eq!(m.get("HYPE"), Some(&"@107".to_string()));
        // Canonical pair allMids'te AD ile anahtarlanır ("@0" yoktur).
        assert_eq!(m.get("PURR"), Some(&"PURR/USDC".to_string()));
        assert!(!m.contains_key("USDC"), "USDC quote token eşlenmemeli");
    }

    #[test]
    fn parse_spot_keys_usdc_disi_kote_etme() {
        let v = json!({
            "tokens": [
                {"index": 0, "name": "USDC"},
                {"index": 5, "name": "FOO"}
            ],
            "universe": [
                {"name": "@1", "index": 1, "tokens": [5, 5]}
            ]
        });
        let m = parse_spot_keys(&v);
        assert!(!m.contains_key("FOO"), "USDC olmayan quote eşlenmemeli");
    }

    #[test]
    fn parse_spot_keys_kotu_girdi() {
        assert!(parse_spot_keys(&json!({})).is_empty());
        assert!(parse_spot_keys(&json!(null)).is_empty());
    }
}

#[cfg(test)]
mod paginate_tests {
    use super::*;
    use serde_json::json;

    fn rec(t: i64) -> serde_json::Value { json!({"time": t}) }

    /// REGRESYON: eski geriye-dogru sayfalama endTime'i new_min-1 yapip
    /// ayni timestamp'li sinir kayitlarini ATLIYORDU. Yeni kural: son
    /// dönen timestamp SONRAKİ startTime (kapsayici) — sinir kaydi
    /// tekrar gelir (dedup atar), hiçbir kayıt kaçmaz.
    #[tokio::test]
    async fn sinir_kaydi_kaybolmaz() {
        // page_size=2, aralik [0, 1000]
        // p1: 100,200 (dolu) -> cursor=200
        // p2: 200,300 (200 TEKRAR gelir — kapsayici) -> cursor=300
        // p3: 400 (sayfa dolmadi -> bitti)
        let pages: Vec<Vec<serde_json::Value>> = vec![
            vec![rec(100), rec(200)],
            vec![rec(200), rec(300)],
            vec![rec(400)],
        ];
        let mut idx = 0usize;
        let (out, trunc) = paginate_time_range(0, 1000, 2, move |_s, _e| {
            let page = pages.get(idx).cloned().unwrap_or_default();
            idx += 1;
            async move { Ok::<Vec<serde_json::Value>, String>(page) }
        }).await;
        assert!(!trunc, "tam sayfalama truncated OLMAMALI");
        let times: Vec<i64> = out.iter().map(|f| f.get("time").unwrap().as_i64().unwrap()).collect();
        assert_eq!(times, vec![100, 200, 200, 300, 400], "sinir kaydi (200) iki kez gelmeli — dedup caller'da");
    }

    /// REGRESYON: fills 2000 kayıt döndü diye erken truncated açılmamalı —
    /// paginate_time_range tam sayfalamada false döner, caller bayrağı
    /// buna bağlar.
    #[tokio::test]
    async fn tam_sayfalama_truncated_degil() {
        let pages: Vec<Vec<serde_json::Value>> = vec![
            vec![rec(100), rec(200), rec(300)],
            vec![rec(400)], // kısa sayfa = aralık bitti
        ];
        let mut idx = 0usize;
        let (out, trunc) = paginate_time_range(0, 10_000, 3, move |_s, _e| {
            let page = pages.get(idx).cloned().unwrap_or_default();
            idx += 1;
            async move { Ok::<Vec<serde_json::Value>, String>(page) }
        }).await;
        assert!(!trunc);
        assert_eq!(out.len(), 4);
    }

    /// İlerleme yok (hep aynı timestamp'te dolu sayfa) -> truncated=true,
    /// SONSUZ DÖNGÜ YOK.
    #[tokio::test]
    async fn ilerleme_yok_truncated() {
        let page = vec![rec(100), rec(100)];
        let (out, trunc) = paginate_time_range(0, 10_000, 2, move |_s, _e| {
            let page = page.clone();
            async move { Ok::<Vec<serde_json::Value>, String>(page) }
        }).await;
        assert!(trunc, "ilerleme yok truncated OLMALI");
        assert_eq!(out.len(), 4, "2 sayfa alıp durmalı (sonsuz döngü yok)");
    }

    /// Fetch hatası -> truncated=true (sessiz kayıp yok).
    #[tokio::test]
    async fn hata_truncated() {
        let (out, trunc) = paginate_time_range(0, 10_000, 2, |_s, _e| {
            async move { Err::<Vec<serde_json::Value>, String>("fail".into()) }
        }).await;
        assert!(trunc);
        assert!(out.is_empty());
    }

    /// page_max >= end_ms -> pencere sonu, truncated=false.
    #[tokio::test]
    async fn pencere_sonuna_ulasildi() {
        let page = vec![rec(200), rec(300)];
        let (out, trunc) = paginate_time_range(0, 300, 2, move |_s, _e| {
            let page = page.clone();
            async move { Ok::<Vec<serde_json::Value>, String>(page) }
        }).await;
        assert!(!trunc);
        assert_eq!(out.len(), 2);
    }
}
