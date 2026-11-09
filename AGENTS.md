# AGENTS.md - assetcraft

Fixed assets, Austrian AfA, licences and expiries. Read first:
`mcp-central-docs/standards/RUST_ARTCRAFT_STANDARD.md`,
`mcp-central-docs/projects/craft-fleet-expansion/BACKOFFICE_CRAFT_SUITE.md` (section 4.4).

## Layout

- `crates/assetcraft-store` (L1): assets, categories, maintenance, licences, seats, deadlines, AfA postings on SQLite WAL.
- `crates/assetcraft-core` (L1): `Command` bus, `dispatch`, MCP catalog, `afa.rs` (pure AfA engine), `label.rs` (QR SVG), licence audit, expiries, CSV import, demo seed.
- `crates/assetcraft-mcp` (L2): `assetcraft` binary. CLI verbs map onto the same tool calls as MCP.
- No L3 (egui) yet. Planned: register table, asset detail with schedule chart, expiry board.
- Shared kernel: `../craft-biz` (money, bus, MCP loop, confinement).

## Invariants (never weaken)

- Money is i64 cents. Amount arguments are DE strings; floats are refused.
- AfA schedules always sum to the cost; rounding remainder goes to the final year.
- Useful life never below the category minimum (cars 8 years).
- After AfA is posted for an asset, its basis (cost, in-service date, life, GWG) is frozen: Rust check plus SQLite trigger.
- Disposed assets are read-only (trigger).
- `asset.depreciation_due` is sent once per asset and year (`afa_postings` primary key). Payload must match buchcraft's `depreciation_draft`: asset_tag, year, amount_cents, debit, credit (integers), date.
- Licence keys are never stored.

## Rules

- Zero-panic: unwrap/expect/panic/todo/unimplemented denied outside tests.
- MCP mode: stdout is JSON-RPC only; diagnostics to stderr.
- File tools (`label`, `import_csv`) go through `Confinement::check`.
- Demo data only via `demo-seed`, only on an empty register, labelled DEMO. Mock names only.
- Every bug fix ships with a regression test.

## Repo hygiene

- LF line endings only (`.gitattributes`). CRLF breaks commit hooks.
- Single trunk `main`. No agent branches or leftover WIP branches.
- No em dash characters in code, docs or commit messages.
- No `*.bak`, `*.db`, `*.csv`, `*.svg` outputs, `target/`, binaries in git.

## Gates

1. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
2. `cargo test --workspace`
3. `cargo fmt --all --check`
4. MCP probe: `initialize` -> `tools/list` -> `tools/call`, stdout pure JSON
5. `Cargo.lock` committed, clean `git status`
6. `CHANGELOG.md` line
