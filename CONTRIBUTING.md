# Contributing

Thanks for wanting to help. This guide keeps it simple.

## Ground rules

- **It stays read-only.** Any change that lets the monitor sign or send
  trading actions will be rejected. This is a monitor, by design.
- **It stays single-panel.** Everything the user needs must fit one screen —
  no tab labyrinths, no extra windows.
- **Data comes only from Hyperliquid's own API.** No third-party data feeds.

## Getting started

```bash
git clone https://github.com/servetkarckay/hl-monitor.git
cd hl-monitor
cp .env.example .env   # the sample values work for development
cargo build --release
cargo test --workspace
cargo clippy --workspace
```

All three commands must be clean before you open a pull request:
tests pass, clippy reports zero warnings, release build succeeds.

## How to propose a change

1. Open an issue first (unless it's a trivial typo fix) and describe the
   problem, not just the solution.
2. Fork, create a branch, make your change.
3. Add or update tests for anything you touch — regressions get caught by
   tests, not by eyes.
4. Run the three checks above and paste the summary in your PR.
5. Keep PRs small: one idea per PR is much easier to review.

## Code notes for newcomers

- `src/` — the terminal app: `data.rs` (parsing + polling loops),
  `app.rs` (state + event loop + alerts), `ui/` (ratatui rendering).
- `api/` — a small vendored Hyperliquid client crate (`hyperliquid_api`):
  config, info/exchange endpoints, signing, WebSocket.
- Price rounding for any order-style math is centralized in
  `perp_price(x, sz_decimals)` — do not invent ad-hoc rounding.
- Tests: `cargo test --workspace`. UI layout is tested with ratatui's
  `TestBackend` — see `src/ui/overview.rs::layout_tests`.
