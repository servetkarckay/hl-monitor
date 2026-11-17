pub mod app;
pub mod data;
pub mod ui;

use anyhow::Result;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Args {
    pub once: bool,
    pub interval: u64,
    pub coins: Vec<String>,
    pub fills_limit: usize,
    pub funding_days: i64,
}

pub async fn run(args: Args) -> Result<()> {
    if args.once {
        run_once(args).await
    } else {
        crate::app::run_tui(args).await
    }
}

async fn run_once(args: Args) -> Result<()> {
    let cfg = hyperliquid_api::Config::from_env()?;
    let user = std::env::var("USER_ADDRESS")
        .map_err(|_| anyhow::anyhow!("USER_ADDRESS environment variable is not set"))?;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::data::DataMsg>(1);
    let _handle = tokio::spawn(crate::data::poll_loop(
        cfg,
        user.clone(),
        Duration::from_secs(args.interval.max(2)),
        args.funding_days,
        args.coins.clone(),
        tx,
    ));

    let msg = tokio::time::timeout(Duration::from_secs(30), rx.recv())
        .await
        .map_err(|_| anyhow::anyhow!("timeout waiting for snapshot"))?
        .ok_or_else(|| anyhow::anyhow!("poll loop exited without snapshot"))?;

    match msg {
        crate::data::DataMsg::Snapshot {
            overview,
            positions,
            spot,
            orders,
            fills,
            funding,
            fees,
            portfolio,
            mids,
            ctx,
            latency_ms: _,
            ob: _,
            rate_limit: _,
            fills_truncated: _,
            funding_truncated: _,
            spot_px: _,
        } => {
            println!("Hyperliquid snapshot for {}", user);
            println!(
                "Perp: accountValue={:.2} withdrawable={:.2} totalNtlPos={:.2} totalMarginUsed={:.2}",
                overview.account_value,
                overview.withdrawable,
                overview.total_ntl_pos,
                overview.total_margin_used
            );
            println!("Positions: {}", positions.len());
            for p in &positions {
                println!(
                    "  {} {} size={:.4} entry={:.4} uPnL={:.2} lev={:.1}x notional={:.2}",
                    p.coin, p.side, p.size, p.entry_px, p.unrealized_pnl, p.leverage, p.notional
                );
            }
            println!("Spot balances: {}", spot.len());
            for (coin, total, hold) in &spot {
                println!("  {} total={:.4} hold={:.4}", coin, total, hold);
            }
            println!("Open orders: {}", orders.len());
            println!(
                "Fills: {} (closedPnL sum={:.2}, fees={:.4})",
                fills.len(),
                fills.iter().map(|f| f.closed_pnl).sum::<f64>(),
                fills.iter().map(|f| f.fee).sum::<f64>()
            );
            println!(
                "Funding payments: {} (net={:.4})",
                funding.len(),
                funding.iter().map(|f| f.usdc).sum::<f64>()
            );
            println!(
                "Fees: cross={:.6} add={:.6} referral={:.6}",
                fees.cross_rate, fees.add_rate, fees.referral_discount
            );
            println!("Portfolio periods:");
            for (period, acct, pnl, vol) in &portfolio.periods {
                println!(
                    "  {} acct={:.2} pnl={:.2} volume={:.2}",
                    period, acct, pnl, vol
                );
            }
            println!("Mids: {}", mids.len());
            println!("Context: {} coins", ctx.len());
            Ok(())
        }
        crate::data::DataMsg::Error(e) => Err(anyhow::anyhow!(e)),
    }
}
