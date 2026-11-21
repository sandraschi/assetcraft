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

## Docs (keep current in the same commit)

- `README.md`: what it does, quick start, MCP config, build.
- `HELP.md`: task-oriented how-to; update when a verb or workflow changes.
- `docs/PRD.md`: purpose, boundaries, honest gaps, next steps; update "Today" and "Gaps" with every release.
- `docs/TOOLS.md`: generated. After any tool change run `just docs`; the `tools_doc_is_current` test fails otherwise.
- `CHANGELOG.md`: one entry per change set.
- CLI help: `about`, argument doc comments and `AFTER_HELP` in the mcp crate's `main.rs`.

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
6. `CHANGELOG.md` entry; README / HELP / PRD updated if behaviour changed


## Documentation preservation and hooks

On 2026-11-20 a README "standardization" sweep across the fleet deleted project descriptions,
status sections and user warnings in nine repos. Full rules:
`mcp-central-docs/standards/CONTENT_PRESERVATION_STANDARD.md`.

- Applying any standard or template restructures; it never deletes. Move content (within the file
  or to `docs/` with a link), do not drop it. Never remove what the project is, what it is based on,
  its status (what works, what does not, what is not built) or warnings to users.
- Remove content only when Sandra explicitly asks for that removal, in this repo.
- Never add claims you have not verified (release downloads, versions, test counts, features).
- Only standards with `status: active` may be applied across repos. A change across several repos is
  done on one repo first and approved by Sandra; its commits name the tool and carry a `Sweep-Id:` line.
- `.githooks/commit-msg` blocks a commit that deletes a Markdown file, removes 20% of its lines
  (15 or more) or removes 3 or more headings. A removal Sandra asked for passes with
  `Docs-Removal-Approved: <who and why>` in the message. Enable once per clone:
  `git config core.hooksPath .githooks`. Never use `--no-verify`; if a hook blocks you, stop and ask.
