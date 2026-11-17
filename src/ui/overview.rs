use crate::app::AppState;
use ratatui::prelude::*;
use std::collections::VecDeque;
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table, Wrap};

/// Sabit izleme evreni: HYPE ana işlem/karar varlığı; BTC (genel yön),
/// SOL (likidite), JUP (DEX sektörü), PURR (HL ekosistemi) HYPE'a
/// sinyal öncülüğü ölçümü için izlenir.
pub const WATCHLIST: [&str; 5] = ["HYPE", "BTC", "SOL", "JUP", "PURR"];

/// Kompakt sayı: 1.2B / 3.4M / 5.6K
fn fmt_compact(v: f64) -> String {
    let a = v.abs();
    if a >= 1e9 { format!("{:.1}B", v / 1e9) }
    else if a >= 1e6 { format!("{:.1}M", v / 1e6) }
    else if a >= 1e3 { format!("{:.1}K", v / 1e3) }
    else { format!("{:.1}", v) }
}

/// Watchlist fiyat formatı: >=1000 → 2 hane (BTC: 92100.00),
/// aksi 4 hane (HYPE: 69.2625) — satır 34-38 kolona sığmalı.
fn fmt_px(v: f64) -> String {
    if v.abs() >= 1000.0 { format!("{:.2}", v) } else { format!("{:.4}", v) }
}

pub fn render(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if state.overview.is_none() {
        let p = Paragraph::new("Loading…").block(Block::default().title("Hyperliquid Terminal").borders(Borders::ALL));
        f.render_widget(p, area);
        return;
    }
    let ov = state.overview.as_ref().unwrap();
    let spot_usd_total: f64 = state.spot.iter().map(|(coin, total, _)| {
        if coin == "USDC" { *total }
        else { state.spot_px.get(coin).copied().unwrap_or(0.0) * *total }
    }).sum();
    let unpriceable: Vec<String> = state.spot.iter().filter(|(coin, _, _)| coin != "USDC" && !state.spot_px.contains_key(coin)).map(|(c, _, _)| c.clone()).collect();
    let current_eq = ov.account_value + spot_usd_total;

    let margin_usage_pct = if ov.account_value > 0.0 { ov.total_margin_used / ov.account_value * 100.0 } else { 0.0 };
    let equity_peak = state.equity_history.iter().cloned().fold(0.0_f64, f64::max);
    let drawdown_pct = if equity_peak > 0.0 { (equity_peak - current_eq) / equity_peak * 100.0 } else { 0.0 };

    let closed: Vec<&crate::data::Fill> = state.fills.iter().filter(|x| x.closed_pnl != 0.0).collect();
    let closed_count = closed.len();
    let win_count = closed.iter().filter(|x| x.closed_pnl > 0.0).count();
    let loss_count = closed.iter().filter(|x| x.closed_pnl < 0.0).count();
    let win_rate = if closed_count > 0 { win_count as f64 / closed_count as f64 * 100.0 } else { 0.0 };
    let total_closed_pnl: f64 = state.fills.iter().map(|x| x.closed_pnl).sum();
    let total_fee: f64 = state.fills.iter().map(|x| x.fee).sum();
    let gross_profit: f64 = closed.iter().filter(|x| x.closed_pnl > 0.0).map(|x| x.closed_pnl).sum();
    let gross_loss: f64 = closed.iter().filter(|x| x.closed_pnl < 0.0).map(|x| x.closed_pnl).sum::<f64>().abs();
    let profit_factor = if gross_loss > 0.0 { gross_profit / gross_loss } else { 0.0 };
    let funding_net: f64 = state.funding.iter().map(|f| f.usdc).sum();

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(6), Constraint::Length(6), Constraint::Length(6), Constraint::Length(2)])
        .split(area);

    // Header
    let header_line = Line::from(vec![
        Span::styled(" HYPERLIQUID TERMINAL ", Style::default().fg(Color::Black).bg(Color::Cyan).bold()),
        Span::raw(format!("  {}  ", state.user)),
        Span::styled(if state.connection_ok { "● LIVE" } else { "● DOWN" }, Style::default().fg(if state.connection_ok { Color::Green } else { Color::Red }).bold()),
        Span::raw(format!("   Perp: {:.2}  Spot: {:.2}  Total: {:.2}   Orders:{}  Fills:{}  OI1h: {:+.2}%  OI24h: {:+.2}%  ping: {:.0}ms  rl: {}/{}  upd: {}",
            ov.account_value, spot_usd_total, current_eq, state.orders.len(), state.fills.len(),
            oi_change(&state.oi_history, 3_600_000),
            oi_change(&state.oi_history, 86_400_000),
            state.latency_ms,
            state.rate_limit.0, state.rate_limit.1,
            state.last_refresh.map(|i| format!("{:.1}s", i.elapsed().as_secs_f64())).unwrap_or_else(|| "—".into()))),
    ]);
    let header = Paragraph::new(header_line).block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, root[0]);

    // Main: 3 cols (sağ kolon geniş: Watchlist 5 satır sığmalı)
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(27), Constraint::Percentage(39), Constraint::Percentage(34)])
        .split(root[1]);

    // Left: Account + Risk + Fees
    {
        let chunks = Layout::default().direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Length(7), Constraint::Min(4)])
            .split(cols[0]);
        let acct_lines = vec![
            Line::from(format!("Perp balance: {:.2}", ov.account_value)),
            Line::from(format!("Withdrawable: {:.2}", ov.withdrawable)),
            Line::from(format!("TotalNtlPos: {:.2}", ov.total_ntl_pos)),
            Line::from(format!("MarginUsed: {:.2}", ov.total_margin_used)),
            Line::from(format!("Spot USD value: {:.2}", spot_usd_total)),
            Line::from(format!("Unpriceable spot: {}", if unpriceable.is_empty() { "none".to_string() } else { unpriceable.join(", ") })),
            Line::from(format!("Total equity: {:.2}", current_eq)),
        ];
        let acct_border = if margin_usage_pct > 70.0 { Color::Red } else { Color::White };
        let acct = Paragraph::new(acct_lines).block(Block::default().title("Account").borders(Borders::ALL).border_style(Style::default().fg(acct_border))).wrap(Wrap { trim: true });
        f.render_widget(acct, chunks[0]);

        let risk_lines = vec![
            Line::from(format!("Margin usage: {:.1}%", margin_usage_pct)),
            Line::from(format!("Drawdown: {:.2}%", drawdown_pct)),
            Line::from(format!("Eq peak: {:.2}", equity_peak)),
        ];
        let risk_area = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(4), Constraint::Length(3)]).split(chunks[1]);
        let risk_border_color = if margin_usage_pct > 70.0 { Color::Red } else { Color::White };
        let risk = Paragraph::new(risk_lines).block(Block::default().title("Risk").borders(Borders::ALL).border_style(Style::default().fg(risk_border_color))).wrap(Wrap { trim: true });
        f.render_widget(risk, risk_area[0]);
        let gauge = Gauge::default().block(Block::default().title("Margin").borders(Borders::ALL)).gauge_style(Style::default().fg(Color::Red)).ratio((margin_usage_pct / 100.0).clamp(0.0, 1.0));
        f.render_widget(gauge, risk_area[1]);

        let fee_lines = if let Some(fees) = &state.fees {
            vec![
                Line::from(format!("cross: {:.6}", fees.cross_rate)),
                Line::from(format!("add: {:.6}", fees.add_rate)),
                Line::from(format!("referral: {:.6}", fees.referral_discount)),
            ]
        } else { vec![Line::from("loading…")] };
        let fees = Paragraph::new(fee_lines).block(Block::default().title("Fee tier").borders(Borders::ALL)).wrap(Wrap { trim: true });
        f.render_widget(fees, chunks[2]);
    }

    // Center: Positions + Orders
    {
        let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Percentage(55), Constraint::Percentage(45)]).split(cols[1]);
        if state.positions.is_empty() {
            let p = Paragraph::new("No open perp positions").block(Block::default().title("Positions").borders(Borders::ALL));
            f.render_widget(p, chunks[0]);
        } else {
            let header = Row::new(vec![Cell::from("Coin"), Cell::from("Side"), Cell::from("Size"), Cell::from("Entry"), Cell::from("Mark"), Cell::from("uPnL"), Cell::from("Lev"), Cell::from("LiqDist"), Cell::from("Notional")]).style(Style::default().bold());
            let rows: Vec<Row> = state.positions.iter().map(|p| {
                let mark = state.mids.get(&p.coin).copied().or(p.mark_px).unwrap_or(0.0);
                let pnl = p.unrealized_pnl;
                let color = if pnl >= 0.0 { Color::Green } else { Color::Red };
                let side_color = if p.side == "Long" { Color::Green } else { Color::Red };
                let liq_dist = p.liq_px.map(|l| if mark > 0.0 { (mark - l).abs() / mark * 100.0 } else { 0.0 });
                let liq_str = liq_dist.map(|d| {
                    let bar_len = (d / 10.0 * 8.0).min(8.0) as usize;
                    let bar: String = "█".repeat(bar_len);
                    format!("{:.1}% {}", d, bar)
                }).unwrap_or_else(|| "—".into());
                let liq_color = liq_dist.map(|d| if d < 2.0 { Color::Red } else if d < 5.0 { Color::Yellow } else { Color::Green }).unwrap_or(Color::White);
                Row::new(vec![
                    Cell::from(p.coin.clone()),
                    Cell::from(p.side.clone()).style(Style::default().fg(side_color)),
                    Cell::from(format!("{:.4}", p.size)),
                    Cell::from(format!("{:.4}", p.entry_px)),
                    Cell::from(format!("{:.4}", mark)),
                    Cell::from(format!("{:.2}", pnl)).style(Style::default().fg(color)),
                    Cell::from(format!("{:.1}x", p.leverage)),
                    Cell::from(liq_str).style(Style::default().fg(liq_color)),
                    Cell::from(format!("{:.2}", p.notional)),
                ])
            }).collect();
            let widths = [Constraint::Length(7), Constraint::Length(6), Constraint::Length(9), Constraint::Length(9), Constraint::Length(9), Constraint::Length(9), Constraint::Length(5), Constraint::Length(8), Constraint::Length(10)];
            let table = Table::new(rows, widths).header(header).block(Block::default().title("Positions").borders(Borders::ALL)).column_spacing(1);
            f.render_widget(table, chunks[0]);
        }
        if state.orders.is_empty() {
            let p = Paragraph::new("No open orders").block(Block::default().title("Open orders").borders(Borders::ALL));
            f.render_widget(p, chunks[1]);
        } else {
            let header = Row::new(vec![Cell::from("Coin"), Cell::from("Side"), Cell::from("Type"), Cell::from("Price"), Cell::from("Size"), Cell::from("Orig")]).style(Style::default().bold());
            let rows: Vec<Row> = state.orders.iter().take(10).map(|o| {
                let side_color = if o.side == "Buy" { Color::Green } else { Color::Red };
                Row::new(vec![
                    Cell::from(o.coin.clone()),
                    Cell::from(o.side.clone()).style(Style::default().fg(side_color)),
                    Cell::from(o.order_type.clone()),
                    Cell::from(format!("{:.4}", o.price)),
                    Cell::from(format!("{:.4}", o.size)),
                    Cell::from(format!("{:.4}", o.orig_size)),
                ])
            }).collect();
            let more = if state.orders.len() > 10 { format!(" ({} total)", state.orders.len()) } else { String::new() };
            let widths = [Constraint::Length(7), Constraint::Length(6), Constraint::Length(9), Constraint::Length(9), Constraint::Length(9), Constraint::Length(9)];
            let table = Table::new(rows, widths).header(header).block(Block::default().title(format!("Open orders{}", more)).borders(Borders::ALL)).column_spacing(1);
            f.render_widget(table, chunks[1]);
        }
    }

    // Right: PnL + Funding + Watchlist + Win/Loss
    {
        let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(8), Constraint::Length(8), Constraint::Length(7), Constraint::Min(3)]).split(cols[2]);
        let mut pnl_lines = vec![
            Line::from(format!("Fills: {} (W{} L{})", state.fills.len(), win_count, loss_count)),
            Line::from(format!("Win rate: {:.1}%", win_rate)),
            Line::from(format!("Closed PnL: {:.2}", total_closed_pnl)),
            Line::from(format!("Profit factor: {:.2}", profit_factor)),
            Line::from(format!("Funding net: {:.4}", funding_net)),
            Line::from(format!("Total fee: {:.2}", total_fee)),
        ];
        if state.fills_truncated {
            pnl_lines.push(Line::from(Span::styled("⚠ fills truncated (last 10000)", Style::default().fg(Color::Yellow))));
        }
        if state.funding_truncated {
            pnl_lines.push(Line::from(Span::styled("⚠ funding truncated", Style::default().fg(Color::Yellow))));
        }
        let pnl = Paragraph::new(pnl_lines).block(Block::default().title("PnL").borders(Borders::ALL)).wrap(Wrap { trim: true });
        f.render_widget(pnl, chunks[0]);

        // Funding payments
        let mut funding_lines = vec![Line::from("coin  time  usdc  szi")];
        for fd in state.funding.iter().rev().take(6) {
            funding_lines.push(Line::from(format!("{:<5} {}  {:.4}  {:.4}", fd.coin, fd.time % 100000, fd.usdc, fd.szi)));
        }
        if state.funding.is_empty() { funding_lines.push(Line::from("No funding")); }
        let fund = Paragraph::new(funding_lines).block(Block::default().title("Funding payments").borders(Borders::ALL)).wrap(Wrap { trim: true });
        f.render_widget(fund, chunks[1]);

        // Watchlist: sabit 5-coinlik izleme evreni (HYPE ana varlık;
        // BTC/SOL/JUP/PURR HYPE sinyal öncülüğü ölçümü için).
        {
            let mut wl_lines: Vec<Line> = Vec::new();
            for coin in WATCHLIST {
                let c = state.ctx.get(coin);
                let (px, vlm, fund_rate, oi) = match c {
                    Some(c) => (c.mark_px, c.day_ntl_vlm, c.funding, c.open_interest),
                    None => (0.0, 0.0, 0.0, 0.0),
                };
                let apr = fund_rate * 24.0 * 365.0 * 100.0;
                let mut spans = vec![
                    Span::styled(format!("{:<5}", coin), Style::default().bold()),
                    Span::raw(format!("{} ", fmt_px(px))),
                    Span::raw(format!("V{} ", fmt_compact(vlm))),
                    Span::raw(format!("F{:+.1}% ", apr)),
                    Span::raw(format!("OI{}", fmt_compact(oi))),
                ];
                if c.is_none() {
                    spans.push(Span::styled(" (no ctx)", Style::default().fg(Color::DarkGray)));
                }
                wl_lines.push(Line::from(spans));
            }
            let wl = Paragraph::new(wl_lines).block(Block::default().title("Watchlist").borders(Borders::ALL)).wrap(Wrap { trim: true });
            f.render_widget(wl, chunks[2]);
        }

        // win/loss bar
        let mut bar_lines = vec![Line::from(format!("Win: {}  Loss: {}", win_count, loss_count))];
        let total = (win_count + loss_count).max(1);
        let win_bar = "█".repeat(win_count * 20 / total);
        let loss_bar = "█".repeat(loss_count * 20 / total);
        bar_lines.push(Line::from(Span::styled(format!("W {}", win_bar), Style::default().fg(Color::Green))));
        bar_lines.push(Line::from(Span::styled(format!("L {}", loss_bar), Style::default().fg(Color::Red))));
        let bar_p = Paragraph::new(bar_lines).block(Block::default().title("Win/Loss").borders(Borders::ALL)).wrap(Wrap { trim: true });
        f.render_widget(bar_p, chunks[3]);
    }

    // Equity curve (full width)
    {
        let spark_data: Vec<u64> = state.equity_history.iter().map(|x| (x * 100.0).max(0.0) as u64).collect();
        let spark = Sparkline::default().block(Block::default().title("Equity curve (full width)").borders(Borders::ALL)).data(&spark_data).style(Style::default().fg(Color::Cyan));
        f.render_widget(spark, root[2]);
    }

    // Bottom: fills + tape + portfolio + order book depth
    let bottom = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(20), Constraint::Percentage(20), Constraint::Percentage(20), Constraint::Percentage(20), Constraint::Percentage(20)]).split(root[3]);
    // Recent fills
    {
        let mut filtered: Vec<&crate::data::Fill> = state.fills.iter().collect();
        filtered.sort_by_key(|x| std::cmp::Reverse(x.time));
        let rows: Vec<Row> = filtered.iter().take(8).map(|f| {
            let color = if f.closed_pnl >= 0.0 { Color::Green } else { Color::Red };
            Row::new(vec![Cell::from(f.coin.clone()), Cell::from(f.side.clone()), Cell::from(format!("{:.4}", f.px)), Cell::from(format!("{:.2}", f.closed_pnl)).style(Style::default().fg(color))])
        }).collect();
        let widths = [Constraint::Length(7), Constraint::Length(5), Constraint::Length(10), Constraint::Length(9)];
        let table = Table::new(rows, widths).block(Block::default().title("Recent fills").borders(Borders::ALL)).column_spacing(1);
        f.render_widget(table, bottom[0]);
    }
    // Live tape
    {
        let mut lines: Vec<Line> = Vec::new();
        let mut count = 0;
        for ev in state.tape.iter().rev() {
            if count >= 8 { break; }
            if let crate::app::TapeEvent::Trade { coin, side, px, sz, .. } = ev {
                let val = px * sz;
                let is_whale = val >= 50000.0;
                let color = if side == "B" || side == "Buy" { Color::Green } else { Color::Red };
                let marker = if is_whale { "🐋" } else { "  " };
                let style = if is_whale { Style::default().fg(color).bold() } else { Style::default().fg(color) };
                lines.push(Line::from(vec![
                    Span::raw(marker.to_string()),
                    Span::raw(format!("{:<6}", coin)),
                    Span::styled(format!("{:<4}", side), style),
                    Span::raw(format!("{:>10.4}  {:>8.4}  ${:>8.0}", px, sz, val)),
                ]));
                count += 1;
            }
        }
        if lines.is_empty() { lines.push(Line::from("No trades yet")); }
        let p = Paragraph::new(lines).block(Block::default().title("Live tape").borders(Borders::ALL)).wrap(Wrap { trim: true });
        f.render_widget(p, bottom[1]);
    }
    // Portfolio
    {
        let periods = state.portfolio.as_ref().map(|p| &p.periods);
        let mut lines = vec![Line::from("period   pnl   volume")];
        for name in ["day", "week", "month", "allTime"] {
            lines.push(Line::from(format!("{:<7} {}  {}", name, fmt_opt(period_value(periods, name)), fmt_opt(period_volume(periods, name)))));
        }
        let p = Paragraph::new(lines).block(Block::default().title("Portfolio").borders(Borders::ALL)).wrap(Wrap { trim: true });
        f.render_widget(p, bottom[2]);

    // Funding heatmap: top 5 highest positive & negative APR
    {
        let mut coins: Vec<(&String, &crate::data::CtxLite)> = state.ctx.iter().collect();
        coins.sort_by(|a, b| b.1.funding.partial_cmp(&a.1.funding).unwrap_or(std::cmp::Ordering::Equal));
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled("Top 5 positive (APR):", Style::default().fg(Color::Green).bold())));
        for (coin, c) in coins.iter().take(5) {
            let apr = c.funding * 24.0 * 365.0 * 100.0;
            lines.push(Line::from(format!("{:<6} fund: {:+.5}  APR: {:+.1}%", coin, c.funding, apr)));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Top 5 negative (APR):", Style::default().fg(Color::Red).bold())));
        // most negative: sort ascending
        coins.sort_by(|a, b| a.1.funding.partial_cmp(&b.1.funding).unwrap_or(std::cmp::Ordering::Equal));
        for (coin, c) in coins.iter().take(5) {
            let apr = c.funding * 24.0 * 365.0 * 100.0;
            lines.push(Line::from(format!("{:<6} fund: {:+.5}  APR: {:+.1}%", coin, c.funding, apr)));
        }
        let p = Paragraph::new(lines).block(Block::default().title("Funding heatmap (APR)").borders(Borders::ALL)).wrap(Wrap { trim: true });
        f.render_widget(p, bottom[3]);
    }

    // Depth chart (cumulative bid/ask)
    {
        if let Some(ob) = &state.ob {
            let bids = ob.bids();
            let asks = ob.asks();
            let mut cum_bid = 0.0;
            let mut cum_ask = 0.0;
            let max_cum = {
                let mut cb = 0.0; let mut ca = 0.0;
                for b in bids { cb += b.sz.parse::<f64>().unwrap_or(0.0); }
                for a in asks { ca += a.sz.parse::<f64>().unwrap_or(0.0); }
                cb.max(ca).max(1.0)
            };
            let mut lines: Vec<Line> = Vec::new();
            // asks (top, descending price)
            for a in asks.iter().rev().take(6) {
                cum_ask += a.sz.parse::<f64>().unwrap_or(0.0);
                let bar = "█".repeat((cum_ask / max_cum * 16.0) as usize);
                lines.push(Line::from(Span::styled(format!("{:>8} │{} {:.1}", a.px, bar, cum_ask), Style::default().fg(Color::Red))));
            }
            lines.push(Line::from(Span::styled("──── mid ────", Style::default().fg(Color::Yellow))));
            // bids (descending price)
            for b in bids.iter().take(6) {
                cum_bid += b.sz.parse::<f64>().unwrap_or(0.0);
                let bar = "█".repeat((cum_bid / max_cum * 16.0) as usize);
                lines.push(Line::from(Span::styled(format!("{:>8} │{} {:.1}", b.px, bar, cum_bid), Style::default().fg(Color::Green))));
            }
            let p = Paragraph::new(lines).block(Block::default().title("Depth chart").borders(Borders::ALL)).wrap(Wrap { trim: true });
            f.render_widget(p, bottom[4]);
        } else {
            let p = Paragraph::new("No order book").block(Block::default().title("Depth chart").borders(Borders::ALL));
            f.render_widget(p, bottom[4]);
        }
    }
    }

    // Status bar (handled by mod.rs, but footer here)
    let mut footer_lines = vec![Line::from(format!("Last refresh: {}  |  Tabs: 1 Overview 2 Positions 3 Orders 4 Fills 5 Funding 6 Portfolio 7 Tape",
        state.last_refresh.map(|i| format!("{:.1}s ago", i.elapsed().as_secs_f64())).unwrap_or_else(|| "—".to_string())))];
    if let Some(err) = &state.error {
        footer_lines.push(Line::from(Span::styled(format!("Error: {}", err), Style::default().fg(Color::Red))));
    }
    let footer = Paragraph::new(footer_lines).wrap(Wrap { trim: true });
    f.render_widget(footer, root[4]);
}

fn norm(s: &str) -> String { s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase() }
fn find_period<'a>(periods: Option<&'a Vec<(String, f64, f64, f64)>>, name: &str) -> Option<&'a (String, f64, f64, f64)> {
    let target = norm(name); periods?.iter().find(|p| norm(&p.0) == target)
}
fn period_value(periods: Option<&Vec<(String, f64, f64, f64)>>, name: &str) -> Option<f64> { find_period(periods, name).map(|p| p.2) }
fn period_volume(periods: Option<&Vec<(String, f64, f64, f64)>>, name: &str) -> Option<f64> { find_period(periods, name).map(|p| p.3) }
fn fmt_opt(v: Option<f64>) -> String { match v { Some(x) => format!("{:.2}", x), None => "—".to_string() } }
/// Zaman damgası tabanlı OI değişimi: hist (epoch_ms, oi) örnekleri.
/// window_ms öncesindeki son örneği (yoksa en eski örneği) baz alır —
/// örnek SAYISINA değil GEÇEN SÜREYE dayanır (pause/connection kopması
/// örnek akışını bozduğunda sayı-tabanlı hesap yanlış pencere ölçer).
fn oi_change(hist: &VecDeque<(i64, f64)>, window_ms: i64) -> f64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let cutoff = now - window_ms;
    let mut base: Option<f64> = None;
    for (t, v) in hist.iter() {
        if *t <= cutoff { base = Some(*v); } else { break; }
    }
    let base = match base.or_else(|| hist.front().map(|(_, v)| *v)) {
        Some(b) => b,
        None => return 0.0,
    };
    let last = hist.back().map(|(_, v)| *v).unwrap_or(0.0);
    if base > 0.0 { (last - base) / base * 100.0 } else { 0.0 }
}
#[cfg(test)]
mod oi_tests {
    use super::*;

    fn ms_ago(ms: i64) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0) - ms
    }

    /// REGRESYON: hesap örnek SAYISINA dayanıyordu; pause/connection
    /// kopmasında örnek akışı bozulunca pencere yanlış ölçülüyordu.
    /// Artık geçen SÜREYE (timestamp) dayanıyor.
    #[test]
    fn oi_change_timestamp_tabanli() {
        let mut h: VecDeque<(i64, f64)> = VecDeque::new();
        // 2 saat önce 100, 1 saat önce 110 (1h penceresi baz=110)
        h.push_back((ms_ago(7_200_000), 100.0));
        h.push_back((ms_ago(3_600_000), 110.0));
        h.push_back((ms_ago(1000), 120.0));
        let ch1 = oi_change(&h, 3_600_000);
        assert!((ch1 - (120.0 - 110.0) / 110.0 * 100.0).abs() < 0.5, "1h baz 110 olmalı, got {ch1}");
        // 24h penceresi baz = 100
        let ch24 = oi_change(&h, 86_400_000);
        assert!((ch24 - 20.0).abs() < 0.5, "24h baz 100 olmalı, got {ch24}");
    }

    /// Örnek arası büyük boşluk (pause): pencere içinde örnek yoksa
    /// en eski örnek baz alınır — hata değil, bilinen kısıt.
    #[test]
    fn oi_change_bosluk_en_eski_baz() {
        let mut h: VecDeque<(i64, f64)> = VecDeque::new();
        h.push_back((ms_ago(5_000), 100.0));
        h.push_back((ms_ago(1000), 150.0));
        // 1h penceresinde 1h-oncesi örnek yok -> baz = en eski (100)
        let ch = oi_change(&h, 3_600_000);
        assert!((ch - 50.0).abs() < 0.5, "baz en eski örnek olmalı, got {ch}");
    }

    #[test]
    fn oi_change_bos_ve_tek_ornek() {
        let h: VecDeque<(i64, f64)> = VecDeque::new();
        assert_eq!(oi_change(&h, 3_600_000), 0.0);
        let mut h1: VecDeque<(i64, f64)> = VecDeque::new();
        h1.push_back((ms_ago(1000), 100.0));
        assert_eq!(oi_change(&h1, 3_600_000), 0.0);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use crate::data::{CtxLite, Overview};

    fn sample_state() -> AppState {
        let mut state = AppState::new("t".into(), std::time::Duration::from_secs(2), WATCHLIST.iter().map(|c| c.to_string()).collect());
        state.overview = Some(Overview { account_value: 100.0, withdrawable: 50.0, total_ntl_pos: 0.0, total_margin_used: 0.0, cross_maintenance: 0.0 });
        for coin in WATCHLIST {
            state.ctx.insert(coin.into(), CtxLite { funding: 0.0001, open_interest: 1e6, mark_px: 1.0, oracle_px: 1.0, day_ntl_vlm: 1e7 });
        }
        state
    }

    /// REGRESYON: Watchlist ve Win/Loss İKİSİ DE chunks[2]'ye çiziliyordu;
    /// ikinci çizim birincini siliyordu, chunks[3] boş kalıyordu.
    /// TestBackend ile tam frame çizilir ve İKİ panelin de metni
    /// buffer'da bulunmalı (ezilme yok).
    #[test]
    fn watchlist_ve_winloss_ayri_panellerde() {
        let state = sample_state();
        let backend = ratatui::backend::TestBackend::new(120, 50);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, f.area(), &state)).unwrap();
        let text: String = terminal.backend().buffer().content.iter().flat_map(|c| c.symbol().chars()).collect();
        assert!(text.contains("Watchlist"), "Watchlist paneli görünmeli (ezilmemeli)");
        assert!(text.contains("Win/Loss"), "Win/Loss paneli görünmeli (ayrı chunk)");
        // 5 coin de satır olarak görünmeli
        for coin in WATCHLIST {
            assert!(text.contains(coin), "Watchlist'te {coin} satırı olmalı");
        }
    }

    /// Dar ekran (40x24): Length constraint'ler sıkışsa bile paneller
    /// birbirini EZMEMELİ — her panel kendi chunk'ında.
    #[test]
    fn dar_ekranda_ezilme_yok() {
        let state = sample_state();
        let backend = ratatui::backend::TestBackend::new(40, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, f.area(), &state)).unwrap();
        let text: String = terminal.backend().buffer().content.iter().flat_map(|c| c.symbol().chars()).collect();
        assert!(text.contains("Watchlist"), "dar ekranda Watchlist ezilmemeli");
        assert!(text.contains("Win/Loss"), "dar ekranda Win/Loss ezilmemeli");
    }
}
