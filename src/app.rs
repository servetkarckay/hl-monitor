use std::collections::{HashMap, VecDeque};
use std::io;
use std::time::{Duration, Instant};

use crate::data;
use crate::ui;

use crossterm::cursor::Show as ShowCursor;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab { Overview, Positions, Orders, Fills, Funding, Portfolio, Tape }

impl Tab {
    pub fn name(&self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Positions => "Positions",
            Tab::Orders => "Orders",
            Tab::Fills => "Fills",
            Tab::Funding => "Funding",
            Tab::Portfolio => "Portfolio",
            Tab::Tape => "Tape",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Tab::Overview => Tab::Positions, Tab::Positions => Tab::Orders, Tab::Orders => Tab::Fills,
            Tab::Fills => Tab::Funding, Tab::Funding => Tab::Portfolio, Tab::Portfolio => Tab::Tape,
            Tab::Tape => Tab::Overview,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Tab::Overview => Tab::Tape, Tab::Positions => Tab::Overview, Tab::Orders => Tab::Positions,
            Tab::Fills => Tab::Orders, Tab::Funding => Tab::Fills, Tab::Portfolio => Tab::Funding,
            Tab::Tape => Tab::Portfolio,
        }
    }
    pub fn from_digit(n: usize) -> Option<Self> {
        match n { 1=>Some(Tab::Overview),2=>Some(Tab::Positions),3=>Some(Tab::Orders),4=>Some(Tab::Fills),5=>Some(Tab::Funding),6=>Some(Tab::Portfolio),7=>Some(Tab::Tape),_=>None }
    }
}

impl std::fmt::Display for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.name()) }
}

#[derive(Debug, Clone)]
pub enum TapeEvent {
    Trade { time: i64, coin: String, side: String, px: f64, sz: f64 },
    UserEvent(serde_json::Value),
    Connected,
    Disconnected,
}

#[derive(Debug)]
pub struct AppState {
    pub user: String,
    pub tab: Tab,
    pub paused: bool,
    pub last_refresh: Option<Instant>,
    pub refresh_interval: Duration,
    pub error: Option<String>,
    pub overview: Option<data::Overview>,
    pub positions: Vec<data::Position>,
    pub spot: Vec<(String, f64, f64)>,
    pub orders: Vec<data::Order>,
    pub fills: Vec<data::Fill>,
    pub funding: Vec<data::Funding>,
    pub fees: Option<data::Fees>,
    pub portfolio: Option<data::Portfolio>,
    pub mids: HashMap<String, f64>,
    pub ctx: HashMap<String, data::CtxLite>,
    pub tape: VecDeque<TapeEvent>,
    pub scroll: usize,
    pub filter_coin: Option<String>,
    pub input_mode: bool,
    pub filter_buffer: String,
    pub show_help: bool,
    pub selected: usize,
    pub coins: Vec<String>,
    pub equity_history: VecDeque<f64>,
    pub oi_history: VecDeque<(i64, f64)>,
    pub prev_positions: Vec<data::Position>,
    pub has_baseline: bool,
    pub connection_ok: bool,
    pub connection_msg: Option<String>,
    pub latency_ms: f64,
    pub ob: Option<hyperliquid_api::types::L2Book>,
    pub rate_limit: (u64, u64),
    pub fills_truncated: bool,
    pub funding_truncated: bool,
    pub spot_px: HashMap<String, f64>,
}

impl AppState {
    pub fn new(user: String, interval: Duration, coins: Vec<String>) -> Self {
        Self {
            user, tab: Tab::Overview, paused: false, last_refresh: None, refresh_interval: interval,
            error: None, overview: None, positions: Vec::new(), spot: Vec::new(), orders: Vec::new(),
            fills: Vec::new(), funding: Vec::new(), fees: None, portfolio: None, mids: HashMap::new(),
            ctx: HashMap::new(), tape: VecDeque::new(), scroll: 0, filter_coin: None, input_mode: false,
            filter_buffer: String::new(), show_help: false, selected: 0, coins, equity_history: VecDeque::new(), oi_history: VecDeque::new(), prev_positions: Vec::new(), has_baseline: false,
            connection_ok: true, connection_msg: None, latency_ms: 0.0, ob: None, rate_limit: (0, 0), fills_truncated: false, funding_truncated: false, spot_px: HashMap::new(),
        }
    }
    pub fn next_tab(&mut self) { self.tab = self.tab.next(); }
    pub fn prev_tab(&mut self) { self.tab = self.tab.prev(); }
}

pub async fn run_tui(args: crate::Args) -> anyhow::Result<()> {
    let cfg = hyperliquid_api::Config::from_env()?;
    let user = std::env::var("USER_ADDRESS").map_err(|_| anyhow::anyhow!("USER_ADDRESS not set"))?;
    let mut state = AppState::new(user.clone(), Duration::from_secs(args.interval), args.coins.clone());
    if args.coins.len() == 1 { state.filter_coin = Some(args.coins[0].clone()); }

    let (data_tx, mut data_rx) = mpsc::channel::<crate::data::DataMsg>(4);
    let (tape_tx, mut tape_rx) = mpsc::channel::<crate::data::TapeMsg>(256);
    let tape_cfg = cfg.clone();
    let tape_coins = args.coins.clone();
    let poll_handle = tokio::spawn(crate::data::poll_loop(cfg, user.clone(), Duration::from_secs(args.interval), args.funding_days, args.coins.clone(), data_tx));
    let tape_handle = tokio::spawn(crate::data::tape_loop(tape_cfg, tape_coins, tape_tx));

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| { let _ = disable_raw_mode(); let _ = execute!(io::stdout(), LeaveAlternateScreen, ShowCursor); original_hook(info); }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, rx) = mpsc::channel::<Event>(256);
    let input_handle = tokio::spawn(async move {
        loop {
            match event::poll(Duration::from_millis(50)) {
                Ok(true) => { if let Ok(ev) = event::read() { if tx.send(ev).await.is_err() { break; } } }
                Ok(false) => { tokio::task::yield_now().await; }
                Err(_) => break,
            }
        }
    });

    let result = event_loop(&mut terminal, state, rx, &mut data_rx, &mut tape_rx).await;
    input_handle.abort(); poll_handle.abort(); tape_handle.abort();
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen, ShowCursor);
    let _ = terminal.show_cursor();
    result
}

/// OI zaman serisinin izlenecek sabit piyasası: önce kullanıcının
/// birincil coin'i (--coins ilk elemanı, örn. "HYPE"); ctx'te yoksa
/// 24s hacmi en yüksek coin (ad leksikografik tiebreak — deterministik).
fn select_oi_coin(coins: &[String], ctx: &HashMap<String, data::CtxLite>) -> Option<String> {
    if let Some(c) = coins.first() {
        if ctx.contains_key(c) { return Some(c.clone()); }
    }
    ctx.iter()
        .max_by(|a, b| {
            a.1.day_ntl_vlm.partial_cmp(&b.1.day_ntl_vlm)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.0.cmp(a.0))
        })
        .map(|(k, _)| k.clone())
}

/// Saf pozisyon olayları: (opened?, Position). Long->Short flip hem
/// close hem open üretir. (Test edilebilir; detect_position_changes
/// bunu ses/bildirim'e çevirir.)
fn position_events(old: &[data::Position], new: &[data::Position]) -> Vec<(bool, data::Position)> {
    let mut ev = Vec::new();
    for np in new {
        if !old.iter().any(|op| op.coin == np.coin && op.side == np.side) {
            ev.push((true, np.clone()));
        }
    }
    for op in old {
        if !new.iter().any(|np| np.coin == op.coin && np.side == op.side) {
            ev.push((false, op.clone()));
        }
    }
    ev
}

fn detect_position_changes(old: &[data::Position], new: &[data::Position]) {
    for (opened, p) in position_events(old, new) {
        let (sound, title, msg) = if opened {
            ("/tmp/opencode/beep_open.wav", "Position opened", format!("{} {} {:.4} @ {:.4}", p.coin, p.side, p.size, p.entry_px))
        } else {
            ("/tmp/opencode/beep_close.wav", "Position closed", format!("{} {} closed", p.coin, p.side))
        };
        let _ = std::process::Command::new("pw-play").arg("--target").arg("alsa_output.pci-0000_00_1b.0.analog-stereo").arg(sound).spawn();
        let _ = std::process::Command::new("notify-send").arg("-u").arg("critical").arg(title).arg(msg).spawn();
    }
}

async fn event_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, mut state: AppState, mut rx: mpsc::Receiver<Event>, data_rx: &mut mpsc::Receiver<crate::data::DataMsg>, tape_rx: &mut mpsc::Receiver<crate::data::TapeMsg>) -> anyhow::Result<()> {
    let mut tick = tokio::time::interval(Duration::from_millis(16));
    loop {
        while let Ok(msg) = data_rx.try_recv() {
            match msg {
                crate::data::DataMsg::Snapshot { overview, positions, spot, orders, fills, funding, fees, portfolio, mids, ctx, latency_ms, ob, rate_limit, fills_truncated, funding_truncated, spot_px } => {
                    state.error = None; state.connection_ok = true; state.connection_msg = None;
                    if !state.paused {
                        // Detect position open/close for alerts (skip first snapshot = baseline)
                        if state.has_baseline {
                            detect_position_changes(&state.prev_positions, &positions);
                        } else {
                            state.has_baseline = true;
                        }
                        state.prev_positions = positions.clone();
                        state.overview = Some(overview.clone());
                        state.positions = positions; state.spot = spot; state.orders = orders; state.fills = fills;
                        state.funding = funding; state.fees = Some(fees); state.portfolio = Some(portfolio);
                        state.mids = mids; state.ctx = ctx; state.latency_ms = latency_ms; state.ob = ob; state.rate_limit = rate_limit; state.fills_truncated = fills_truncated; state.funding_truncated = funding_truncated; state.spot_px = spot_px;
                        let spot_usd: f64 = state.spot.iter().map(|(coin, total, _)| {
                            if coin == "USDC" { *total }
                            else { state.spot_px.get(coin).copied().unwrap_or(0.0) * *total }
                        }).sum();
                        let eq = state.overview.as_ref().map(|o| o.account_value).unwrap_or(0.0) + spot_usd;
                        state.equity_history.push_back(eq);
                        while state.equity_history.len() > 200 { state.equity_history.pop_front(); }
                        // OI: SABIT piyasa — user'ın seçtiği birincil coin
                        // (--coins ilk elemanı); seçim yoksa day hacmi en
                        // yüksek coin (ad leksikografik tiebreak).
                        let oi = select_oi_coin(&state.coins, &state.ctx)
                            .and_then(|c| state.ctx.get(&c))
                            .map(|c| c.open_interest)
                            .unwrap_or(0.0);
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        state.oi_history.push_back((now_ms, oi));
                        while state.oi_history.len() > 43200 { state.oi_history.pop_front(); }
                    }
                    state.last_refresh = Some(Instant::now());
                }
                crate::data::DataMsg::Error(e) => { state.error = Some(e.clone()); state.connection_ok = false; state.connection_msg = Some(e); }
            }
        }
        while let Ok(msg) = tape_rx.try_recv() {
            let ev = match msg {
                crate::data::TapeMsg::Trade { time, coin, side, px, sz } => TapeEvent::Trade { time, coin, side, px, sz },
                crate::data::TapeMsg::UserEvent(v) => TapeEvent::UserEvent(v),
                crate::data::TapeMsg::Connected => TapeEvent::Connected,
                crate::data::TapeMsg::Disconnected => TapeEvent::Disconnected,
            };
            state.tape.push_back(ev);
            while state.tape.len() > 500 { state.tape.pop_front(); }
        }
        terminal.draw(|f| ui::draw(f, &state))?;
        tokio::select! {
            _ = tick.tick() => {}
            Some(ev) = rx.recv() => {
                if let Event::Key(key) = ev {
                    if key.kind == KeyEventKind::Press {
                        if state.show_help { state.show_help = false; continue; }
                        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                        if state.input_mode {
                            match key.code {
                                KeyCode::Esc => { state.input_mode = false; state.filter_buffer.clear(); }
                                KeyCode::Enter => { let buf = state.filter_buffer.trim().to_string(); state.filter_coin = if buf.is_empty() { None } else { Some(buf) }; state.input_mode = false; state.filter_buffer.clear(); state.scroll = 0; }
                                KeyCode::Backspace => { state.filter_buffer.pop(); }
                                KeyCode::Char(c) if !ctrl => { state.filter_buffer.push(c); }
                                _ => {}
                            }
                        } else {
                            match key.code {
                                KeyCode::Char('q') => return Ok(()),
                                KeyCode::Char('c') if ctrl => return Ok(()),
                                KeyCode::Char('\x03') => return Ok(()),
                                KeyCode::Tab => { state.next_tab(); state.scroll = 0; }
                                KeyCode::BackTab => { state.prev_tab(); state.scroll = 0; }
                                KeyCode::Right => { state.next_tab(); state.scroll = 0; }
                                KeyCode::Left => { state.prev_tab(); state.scroll = 0; }
                                KeyCode::Char('/') => { state.input_mode = true; state.filter_buffer = state.filter_coin.clone().unwrap_or_default(); }
                                KeyCode::Char('c') => { state.filter_coin = None; state.scroll = 0; }
                                KeyCode::Char('e') => {
                                    let path = std::env::temp_dir().join("hl_monitor_fills.csv");
                                    let path = path.to_string_lossy().to_string();
                                    let mut csv = String::from("time,coin,side,dir,px,sz,closedPnl,fee,hash,oid\n");
                                    for f in &state.fills { csv.push_str(&format!("{},{},{},{},{},{},{},{},{},{}\n", f.time, f.coin, f.side, f.dir, f.px, f.sz, f.closed_pnl, f.fee, f.hash, f.oid)); }
                                    let _ = std::fs::write(&path, csv);
                                    state.error = Some(format!("Exported {} fills to {}", state.fills.len(), path));
                                }
                                KeyCode::Char('?') => { state.show_help = true; }
                                KeyCode::Up | KeyCode::Char('k') => { state.scroll = state.scroll.saturating_sub(1); }
                                KeyCode::Down | KeyCode::Char('j') => { state.scroll = state.scroll.saturating_add(1); }
                                KeyCode::PageUp => { state.scroll = state.scroll.saturating_sub(10); }
                                KeyCode::PageDown => { state.scroll = state.scroll.saturating_add(10); }
                                KeyCode::Home | KeyCode::Char('g') => { state.scroll = 0; }
                                KeyCode::Char(c) if c.is_ascii_digit() => { if let Some(d) = c.to_digit(10) { if let Some(t) = Tab::from_digit(d as usize) { state.tab = t; state.scroll = 0; } } }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn tabs_cycle_forward_and_wrap() {
        let mut s = AppState::new("u".into(), Duration::from_secs(10), vec![]);
        assert_eq!(s.tab, Tab::Overview);
        let order = [Tab::Positions, Tab::Orders, Tab::Fills, Tab::Funding, Tab::Portfolio, Tab::Tape, Tab::Overview];
        for t in order { s.next_tab(); assert_eq!(s.tab, t); }
    }
    #[test]
    fn tabs_cycle_backward_and_wrap() {
        let mut s = AppState::new("u".into(), Duration::from_secs(10), vec![]);
        s.prev_tab(); assert_eq!(s.tab, Tab::Tape);
        s.prev_tab(); assert_eq!(s.tab, Tab::Portfolio);
    }
    #[test]
    fn empty_state() {
        let s = AppState::new("alice".into(), Duration::from_secs(5), vec!["BTC".into()]);
        assert_eq!(s.user, "alice");
        assert_eq!(s.refresh_interval, Duration::from_secs(5));
        assert!(!s.paused);
        assert!(s.last_refresh.is_none());
        assert!(s.error.is_none());
        assert!(s.overview.is_none());
        assert!(s.positions.is_empty());
        assert!(s.spot.is_empty());
        assert!(s.orders.is_empty());
        assert!(s.fills.is_empty());
        assert!(s.funding.is_empty());
        assert!(s.fees.is_none());
        assert!(s.portfolio.is_none());
        assert!(s.mids.is_empty());
        assert!(s.ctx.is_empty());
        assert!(s.tape.is_empty());
        assert_eq!(s.scroll, 0);
        assert!(s.filter_coin.is_none());
        assert!(!s.input_mode);
        assert!(s.filter_buffer.is_empty());
        assert!(!s.show_help);
        assert_eq!(s.selected, 0);
        assert_eq!(s.coins, vec!["BTC".to_string()]);
        assert!(s.equity_history.is_empty());
        assert!(s.oi_history.is_empty());
        assert!(s.prev_positions.is_empty());
        assert!(!s.has_baseline);
        assert!(s.ob.is_none());
        assert_eq!(s.rate_limit, (0, 0));
        assert!(!s.fills_truncated);
        assert!(!s.funding_truncated);
        assert!(s.connection_ok);
        assert!(s.connection_msg.is_none());
        assert_eq!(s.latency_ms, 0.0);
        assert!(s.spot_px.is_empty());
    }

    fn pos(coin: &str, side: &str, size: f64) -> data::Position {
        data::Position {
            coin: coin.into(), side: side.into(), size,
            entry_px: 1.0, mark_px: None, unrealized_pnl: 0.0,
            liq_px: None, leverage: 1.0, notional: size,
        }
    }

    /// REGRESYON: kapanış karşılaştırması `np.side == np.side` yazıyordu
    /// (her zaman true) — kapanış bildirimi üretilmiyordu.
    #[test]
    fn position_events_close_algilir() {
        let old = vec![pos("HYPE", "Long", 2.0)];
        let new: Vec<data::Position> = vec![];
        let ev = position_events(&old, &new);
        assert_eq!(ev.len(), 1);
        assert!(!ev[0].0, "HYPE kapanış event'i üretilmeli");
    }

    /// Long -> Short flip: close + open ikisi de üretilmeli.
    #[test]
    fn position_events_flip() {
        let old = vec![pos("HYPE", "Long", 2.0)];
        let new = vec![pos("HYPE", "Short", 3.0)];
        let ev = position_events(&old, &new);
        assert_eq!(ev.len(), 2, "flip = close(Long) + open(Short)");
        assert!(ev.iter().any(|(o, _)| !*o));
        assert!(ev.iter().any(|(o, _)| *o));
    }

    /// Değişim yoksa event yok; ilk snapshot'ta da (old boş, new dolu)
    /// event üretimi detect_position_changes ÇAĞRILMADAN baseline ile
    /// engellenir — position_events kendisi doğru çalışmalı.
    #[test]
    fn position_events_degisim_yok() {
        let old = vec![pos("HYPE", "Long", 2.0)];
        let new = vec![pos("HYPE", "Long", 2.0)];
        assert!(position_events(&old, &new).is_empty());
        assert!(position_events(&[], &[]).is_empty());
    }

    /// REGRESYON: `ctx.values().next()` rastgele HashMap elemanı alıyordu.
    /// Seçim sabit olmalı: önce --coins ilk elemanı, sonra hacim sırası.
    #[test]
    fn select_oi_coin_sabit_piyasa() {
        let mut ctx: HashMap<String, data::CtxLite> = HashMap::new();
        ctx.insert("BTC".into(), data::CtxLite { funding: 0.0, open_interest: 1.0, mark_px: 0.0, oracle_px: 0.0, day_ntl_vlm: 100.0 });
        ctx.insert("HYPE".into(), data::CtxLite { funding: 0.0, open_interest: 2.0, mark_px: 0.0, oracle_px: 0.0, day_ntl_vlm: 500.0 });
        ctx.insert("ETH".into(), data::CtxLite { funding: 0.0, open_interest: 3.0, mark_px: 0.0, oracle_px: 0.0, day_ntl_vlm: 500.0 });
        // Kullanıcı HYPE seçtiyse HYPE (hacimden bağımsız).
        assert_eq!(select_oi_coin(&["HYPE".into()], &ctx), Some("HYPE".into()));
        // Seçim yoksa: en yüksek hacim; eşitlikte ad leksikografik küçük.
        assert_eq!(select_oi_coin(&[], &ctx), Some("ETH".into()));
        // Seçilen coin ctx'te yoksa hacim sırasına düşer.
        assert_eq!(select_oi_coin(&["XYZ".into()], &ctx), Some("ETH".into()));
        // ctx boş -> None.
        assert_eq!(select_oi_coin(&["HYPE".into()], &HashMap::new()), None);
    }
}