# AssetCraft - Tiered Execution Plan

**Parent Standard:** [`RUST_ARTCRAFT_STANDARD.md`](../../mcp-central-docs/standards/RUST_ARTCRAFT_STANDARD.md)  
**Menu Bar Standard:** [`CRAFT_MENU_BAR_STANDARD.md`](../../mcp-central-docs/standards/rust-artcraft/CRAFT_MENU_BAR_STANDARD.md)  
**Architecture Model:** 4-Tier ArtCraft Incremental Delivery  
**Status:** Active  

---

## Architecture Flow

```
Tier 1: Core Engine ──> Tier 2: MCP / CLI ──> Tier 3: Desktop UI ──> Tier 4: Menu Bar, Help & Packaging
(assetcraft-core)       (assetcraft-mcp)       (assetcraft-app)       (Hardened Workstation)
```

---

## Tier Specifications

### Tier 1 - Core Asset & AfA Engine (`assetcraft-core`)
- [x] L1 `Command::dispatch` universal asset execution bus.
- [x] Austrian AfA engine (Halbjahres-AfA, GWG limits, residual value).
- [x] Category configurations with EKR account mapping.
- [x] Software licence seat allocation and audit guards.
- [x] Expiry and warranty tracker with QR SVG label generation.
- [x] Cross-craft bus event emission (`afa.posted` -> BuchCraft).
- [x] `#![forbid(unsafe_code)]` and zero-panic clippy lints.

### Tier 2 - Headless Driver & FastMCP (`assetcraft-mcp`)
- [x] Unified CLI `assetcraft-cli` with `--json` output.
- [x] FastMCP stdio server communicating over JSON-RPC 2.0 (24 tools).
- [x] CSV bulk import/export with transactional validation.
- [x] Headless `--smoke` verification without window initialization.

### Tier 3 - Desktop Workstation UI (`assetcraft-app`)
- [x] Immediate-mode `egui` 0.37 / `wgpu` workstation.
- [x] Asset table with status pills, category grouping, and sorting.
- [x] Asset detail page with interactive depreciation schedule chart.
- [x] Expiry board, check-out/check-in modal, and QR label sheet preview.

### Tier 4 - Craft Menu Bar Standard, Help Suite & Hardened Verification
- [ ] Universal 8-menu bar (`File`, `Edit`, `View`, `Insert`, `Modify`, `Commands`, `Window`, `Help`) with `switch_on_hover`.
- [ ] Mandatory 5-dialog Help suite (`Command Palette...`, `Keyboard Shortcuts...`, `Project Files & Docs...`, `Licenses & Notices...`, `About AssetCraft`).
- [ ] Fleet UI/UX Pattern 2 adherence: viewport-centric navigation, top button row mapped to View toggles, panel visibility managed via `View` / `Window`.
- [ ] Tofu Guard verification (100% font glyph coverage, zero tofu placeholder boxes).
- [ ] Headless `--smoke` parity validation.
- [ ] Standalone release binary packaging and NSIS installer.
