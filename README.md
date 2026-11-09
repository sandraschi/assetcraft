# assetcraft

Fixed assets with Austrian AfA, software licences and expiry dates for a small business. One SQLite file, one binary that is both CLI and MCP server. Part of the back-office craft suite (replaces the asset Excel, the licence spreadsheet and the calendar reminders for certificates and Pickerl).

Status: **v0.1.0, Tier 1** (store + CLI + MCP, tested). No desktop window yet. Synthetic demo data only; AfA rules not reviewed by a Steuerberater.

## What it does

- **Assets:** tag, category, in-service date, cost (the AfA base), holder, location, serial, warranty, supplier, invoice reference, maintenance log.
- **AfA (EStG section 7), in cents:** straight-line, calendar fiscal year, Halbjahres-AfA in the first and in the disposal year, GWG write-off at or below the limit (default 1.000,00 since 2023, configurable). The remainder cent goes to the final year, so a schedule always sums to the cost and never grows a stray year.
- **Categories:** default life and minimum life (cars: at least 8 years, EStG section 8(6)) and the EKR asset account (IT 0620, car 0630, BGA 0660, GWG 0680). Editable.
- **buchcraft link:** `afa 2026 --emit` sends one `asset.depreciation_due` per asset to buchcraft (7000 or 7010 against the asset account). buchcraft turns them into drafts a human approves. Each asset/year is sent once; after that the AfA basis is frozen (in code and by a SQLite trigger).
- **Disposal:** disposal-year AfA, book value, gain or loss; the asset becomes read-only.
- **Licences:** seats, assignments (refused when full unless forced), renewal dates, cost per used seat. Keys are never stored, only where they live; a pasted key is refused.
- **Expiries:** warranties, licence renewals and deadlines (certificates, domains, insurance, inspections) due within N days, overdue first.
- **Labels:** printable 60 x 25 mm SVG with a QR code (`ASSET:<tag>`).
- **CSV import:** `tag;name;category;in_service;cost[;serial;location;holder]`, all-or-nothing, with `--dry-run`. Gets a real inventory in on day one.

## Quick start

```powershell
assetcraft demo-seed
assetcraft list
assetcraft schedule NAS-001
assetcraft afa 2026                      # preview
assetcraft afa 2026 --emit               # drafts for buchcraft
buchcraft inbox-drain; buchcraft drafts list; buchcraft drafts approve 3
assetcraft expiries --days 60
assetcraft license audit
assetcraft add GOL-002 "Mini PC" --category it --in-service 09.11.2026 --cost "689,00"
assetcraft label GOL-001 D:\Temp\gol-001.svg
assetcraft import D:\Temp\inventory.csv --dry-run
```

Database: `--db PATH`, else `$ASSETCRAFT_DB`, else `%LOCALAPPDATA%\assetcraft\assetcraft.db`.

## MCP

```json
{ "mcpServers": { "assetcraft": { "command": "D:/Dev/cargo-target/release/assetcraft.exe", "args": ["mcp", "--allow-path", "D:/Dev/assetcraft-files"] } } }
```

24 tools, all `asset_*`.

## Build

```powershell
just ci
just build-release
```

Needs `../craft-biz` checked out next to this repo.

## Not done (honest list)

- No egui desktop app.
- Degressive AfA (section 7(1a)), Luxustangente for cars (40.000 EUR cap), investment allowance (IFB) and non-calendar fiscal years are not modelled.
- Disposal sends no event: book the residual value (Anlagenabgang) and proceeds in buchcraft yourself.
- No direct import from rust-devices-mcp or winops inventory yet; export those to CSV and use `import`.
- Label is SVG only (print from a browser); no label-printer driver.
