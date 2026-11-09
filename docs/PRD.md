# assetcraft PRD

## What it is

Fixed assets with Austrian AfA, software licences and expiry dates for a small business. Pure Rust, ArtCraft pattern. Suite plan: `BACKOFFICE_CRAFT_SUITE.md`, section 4.4.

## Users

- The owner who has to know what the business owns, what it is worth and what expires.
- Agents over MCP: register purchases, run the year-end AfA, watch expiries and licence seats.
- buchcraft (via the bus) and, through it, the Steuerberater.

## Jobs

1. Register assets once and get a correct AfA plan.
2. Post year-end AfA to the books without retyping.
3. Not overpay or under-license software.
4. Never miss a warranty, certificate, domain, insurance or inspection date.

## Today (v0.1.0)

- Assets with categories (default and minimum life, EKR account), maintenance log, disposal with gain/loss.
- AfA engine in cents: Halbjahres-AfA, GWG with configurable limit, remainder to the final year.
- `afa --emit` -> buchcraft drafts -> approved -> booked (verified end to end); posted AfA basis frozen by trigger.
- Licences with seats, assignment guard, audit; keys never stored.
- Expiries across warranties, renewals and deadlines; QR SVG labels; all-or-nothing CSV import.
- 24 MCP tools ([TOOLS.md](TOOLS.md), generated and test-checked).

## Boundaries

- Calendar fiscal year, straight-line AfA only.
- No direct booking: buchcraft receives drafts that a human approves.
- Not tax advice; categories and lives are defaults to check with the Steuerberater.

## Gaps (honest)

- No egui window.
- Degressive AfA, Luxustangente (car cost cap), IFB and non-calendar fiscal years not modelled.
- Disposal sends no event (residual value booked by hand).
- No direct import from rust-devices-mcp or winops inventory (CSV only).
- SVG labels only, no label-printer driver; synthetic data only.

## Next

1. egui app: register table, asset detail with schedule chart, expiry board, label print.
2. Steuerberater review; degressive AfA as an option.
3. Real inventory import from the fleet (Goliath, robots, plugs).
