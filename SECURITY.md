# Security Policy

## What this tool is (and is not)

`hl-monitor` is a **read-only account monitor** for Hyperliquid.

**It can:**
- Read public account data (positions, orders, fills, funding) for the address
  you put in `USER_ADDRESS`
- Display that data in a terminal dashboard
- Play a sound and send a desktop notification when a position opens or closes

**It can never:**
- Place, modify, cancel or trigger any order — there is no code path that signs
  or sends a trading action
- Withdraw, transfer or touch funds
- Send your configuration values to anyone other than Hyperliquid's official
  public API endpoints

## Keeping yourself safe

1. **Do not put your real trading private key in `.env`.**
   The monitor does not need a real key. The sample key shipped in
   `.env.example` is a public test key that controls no funds — keeping it is
   the safest choice.
2. **Never commit or share your `.env` file.**
   It is listed in `.gitignore`, but double-check before pushing anything.
3. **The `USER_ADDRESS` is public information** — anyone can read any account's
   public data from Hyperliquid. It is not a secret.

## Reporting a vulnerability

Found something that could expose user data, sign transactions, or otherwise
be abused? Please do **not** open a public issue.

Use GitHub's **"Report a vulnerability"** button on the repository's
*Security* tab to file a private report. Include:

- What the problem is and how it could be abused
- The file/line and, if possible, a minimal reproduction
- Any environment details that matter

You will get a response within a few days. Fixed issues are credited
anonymously by default — tell us if you want to be named.

## Scope

Only code in this repository is in scope. The Hyperliquid exchange itself and
third-party dependencies are out of scope — report those to their own
maintainers.
