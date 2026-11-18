//! `assetcraft-app`: Native desktop workstation for hardware/software asset registry and AfA depreciation.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assetcraft_core::afa::{self, Basis, Schedule};
use assetcraft_core::demo_seed;
use assetcraft_core::label;
use assetcraft_core::{afa_year, Ctx};
use assetcraft_store::{
    Asset, Category, Deadline, License, Maintenance, NewAsset, NewLicense, Registry, DEADLINE_KINDS,
};
use chrono::Local;
use clap::Parser;
use craft_biz::confine::Confinement;
use craft_biz::Money;
use craft_biz_ui::bookend::{self, Bar, Spec};
use craft_biz_ui::menu::Item;
use craft_biz_ui::theme::{self, tokens};
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "assetcraft-app",
    version,
    about = "Desktop workstation for Asset Registry & AfA Depreciation"
)]
struct Cli {
    /// Database file path.
    #[arg(long)]
    db: Option<PathBuf>,
    /// Smoke test: open DB, verify schemas, and exit immediately.
    #[arg(long)]
    smoke: bool,
}

fn resolve_bus() -> Option<craft_biz::bus::Bus> {
    if let Ok(dir) = std::env::var("CRAFT_BUS_DIR") {
        return Some(craft_biz::bus::Bus::new(std::path::PathBuf::from(dir)));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return Some(craft_biz::bus::Bus::new(
            std::path::PathBuf::from(local).join("craft-biz-bus"),
        ));
    }
    None
}

fn default_db() -> PathBuf {
    if let Ok(p) = std::env::var("ASSETCRAFT_DB") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("assetcraft").join("assetcraft.db")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    Assets,
    AfaDepreciation,
    Licenses,
    Labels,
    Expiries,
}

fn render_badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.colored_label(color, format!("[{text}]"));
}

struct AppState {
    reg: Registry,
    current_tab: ActiveTab,
    status_msg: Option<String>,

    // Cached data
    assets: Vec<Asset>,
    selected_asset: Option<Asset>,
    selected_afa: Option<Schedule>,
    selected_maint: Vec<Maintenance>,
    licenses: Vec<License>,
    categories: Vec<Category>,

    // Filters
    search_query: String,
    filter_category: Option<String>,
    include_disposed: bool,

    // License assignment input
    assignee_input: String,
    bar: Bar,
    dark: bool,
    zoom: f32,
    lang_de: bool,
    new_tag: String,
    new_name: String,
    new_category: String,
    new_cost: String,
    deadlines: Vec<Deadline>,
    lic_name: String,
    lic_vendor: String,
    lic_seats: String,
    lic_where: String,
    dl_name: String,
    dl_kind: String,
    dl_due: String,
    dl_tag: String,
    maint_kind: String,
    maint_cost: String,
    afa_year: String,
}

impl AppState {
    fn new(reg: Registry) -> Self {
        let mut app = Self {
            reg,
            current_tab: ActiveTab::Assets,
            status_msg: None,
            assets: Vec::new(),
            selected_asset: None,
            selected_afa: None,
            selected_maint: Vec::new(),
            licenses: Vec::new(),
            categories: Vec::new(),
            search_query: String::new(),
            filter_category: None,
            include_disposed: false,
            assignee_input: String::new(),
            bar: Bar::new(),
            dark: true,
            zoom: 1.0,
            lang_de: false,
            new_tag: String::new(),
            new_name: String::new(),
            new_category: "it".into(),
            new_cost: String::new(),
            deadlines: Vec::new(),
            lic_name: String::new(),
            lic_vendor: String::new(),
            lic_seats: "1".into(),
            lic_where: String::new(),
            dl_name: String::new(),
            dl_kind: "certificate".into(),
            dl_due: String::new(),
            dl_tag: String::new(),
            maint_kind: "service".into(),
            maint_cost: String::new(),
            afa_year: Local::now().format("%Y").to_string(),
        };
        app.reload_data();
        app
    }

    fn reload_data(&mut self) {
        let text = if self.search_query.trim().is_empty() {
            None
        } else {
            Some(self.search_query.trim())
        };

        if let Ok(cats) = self.reg.categories() {
            self.categories = cats;
        }

        if let Ok(items) =
            self.reg
                .assets(self.filter_category.as_deref(), self.include_disposed, text)
        {
            if self.selected_asset.is_none() {
                if let Some(first) = items.first() {
                    self.select_asset(first.clone());
                }
            } else if let Some(ref sel) = self.selected_asset {
                if let Some(fresh) = items.iter().find(|a| a.tag == sel.tag) {
                    self.selected_asset = Some(fresh.clone());
                }
            }
            self.assets = items;
        }

        if let Ok(lics) = self.reg.licenses() {
            self.licenses = lics;
        }
        if let Ok(rows) = self.reg.deadlines(false) {
            self.deadlines = rows;
        }
    }

    fn select_asset(&mut self, a: Asset) {
        let basis = Basis {
            cost_cents: a.cost_cents,
            in_service: a.in_service,
            life_years: a.life_years,
            gwg: a.gwg,
            disposed_on: a.disposed_on,
        };
        self.selected_afa = afa::schedule(&basis).ok();
        self.selected_maint = self.reg.maintenance(&a.tag).unwrap_or_default();
        self.selected_asset = Some(a);
    }

    fn assign_license(&mut self, lic_name: &str) {
        if self.assignee_input.trim().is_empty() {
            self.status_msg = Some("Enter employee code or name to assign seat".to_string());
            return;
        }
        let today = Local::now().date_naive();
        match self
            .reg
            .assign(lic_name, self.assignee_input.trim(), today, false)
        {
            Ok(_) => {
                self.status_msg = Some(format!(
                    "Assigned seat on {lic_name} to {}",
                    self.assignee_input.trim()
                ));
                self.assignee_input.clear();
                self.reload_data();
            }
            Err(e) => {
                self.status_msg = Some(format!("License assignment failed: {e}"));
            }
        }
    }

    fn register_asset(&mut self) {
        let tag = self.new_tag.trim().to_string();
        let name = self.new_name.trim().to_string();
        let category = self.new_category.trim().to_string();
        if tag.is_empty() || name.is_empty() || category.is_empty() {
            self.status_msg = Some("Tag, name, and category are required.".into());
            return;
        }
        let cents = match Money::parse_de(&self.new_cost) {
            Ok(m) if m.cents() > 0 => m.cents(),
            _ => {
                self.status_msg = Some("Cost must be a positive amount such as 1.499,00.".into());
                return;
            }
        };
        let today = Local::now().date_naive();
        match self.reg.add_asset(&NewAsset {
            tag,
            name,
            category,
            in_service: today,
            purchase_date: Some(today),
            cost_cents: cents,
            notes: "Registered in the window.".into(),
            ..Default::default()
        }) {
            Ok(a) => {
                self.status_msg = Some(format!("Registered {}.", a.tag));
                self.new_tag.clear();
                self.new_name.clear();
                self.new_cost.clear();
                self.selected_asset = None;
                self.reload_data();
            }
            Err(e) => self.status_msg = Some(e.to_string()),
        }
    }

    fn load_demo(&mut self) {
        match demo_seed(&self.reg) {
            Ok(_) => {
                self.status_msg =
                    Some("Loaded the labelled DEMO inventory. Names say DEMO.".into());
                self.selected_asset = None;
                self.reload_data();
            }
            Err(e) => self.status_msg = Some(e),
        }
    }

    fn unassign_license(&mut self, lic_name: &str, assignee: &str) {
        match self.reg.unassign(lic_name, assignee) {
            Ok(_) => {
                self.status_msg = Some(format!("Unassigned {assignee} from {lic_name}"));
                self.reload_data();
            }
            Err(e) => {
                self.status_msg = Some(format!("License unassign failed: {e}"));
            }
        }
    }
}

impl AppState {
    fn paint_menu(&mut self, ui: &mut egui::Ui) {
        let docs = bookend::repo_docs(env!("CARGO_MANIFEST_DIR"));
        let spec = Spec {
            name: "AssetCraft",
            version: env!("CARGO_PKG_VERSION"),
            description: "Assets, depreciation, and licenses.",
            repository: "https://github.com/sandraschi/assetcraft",
            docs: &docs,
            dark: self.dark,
            zoom: self.zoom,
            lang_de: self.lang_de,
            file: vec![
                Item::action(
                    bookend::t(self.lang_de, "Demo-Inventar laden", "Load demo inventory"),
                    "app.demo",
                ),
                Item::action(bookend::t(self.lang_de, "Aktualisieren", "Refresh"), "app.refresh"),
            ],
            view: vec![
                Item::action(bookend::t(self.lang_de, "Anlagen", "Assets"), "view.assets")
                    .checked(self.current_tab == ActiveTab::Assets),
                Item::action(bookend::t(self.lang_de, "Abschreibung", "Depreciation"), "view.afa")
                    .checked(self.current_tab == ActiveTab::AfaDepreciation),
                Item::action(bookend::t(self.lang_de, "Lizenzen", "Licenses"), "view.licenses")
                    .checked(self.current_tab == ActiveTab::Licenses),
                Item::action(bookend::t(self.lang_de, "Etiketten", "Labels"), "view.labels")
                    .checked(self.current_tab == ActiveTab::Labels),
                Item::action(bookend::t(self.lang_de, "Fristen", "Expiries"), "view.expiries")
                    .checked(self.current_tab == ActiveTab::Expiries),
            ],
            middle: Vec::new(),
        };
        let change = self.bar.show(ui, &spec);
        if let Some(dark) = change.dark {
            self.dark = dark;
        }
        if let Some(zoom) = change.zoom {
            self.zoom = zoom;
        }
        if let Some(lang_de) = change.lang_de {
            self.lang_de = lang_de;
        }
        match change.action.as_deref() {
            Some("app.refresh") => self.reload_data(),
            Some("app.demo") => self.load_demo(),
            Some("view.assets") => self.current_tab = ActiveTab::Assets,
            Some("view.afa") => self.current_tab = ActiveTab::AfaDepreciation,
            Some("view.licenses") => self.current_tab = ActiveTab::Licenses,
            Some("view.labels") => self.current_tab = ActiveTab::Labels,
            Some("view.expiries") => self.current_tab = ActiveTab::Expiries,
            _ => {}
        }
    }
}

impl eframe::App for AppState {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        theme::apply(ui.ctx(), self.dark);
        self.paint_menu(ui);

        // Header Bar
        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("📦 AssetCraft")
                    .color(Color32::from_rgb(56, 189, 248))
                    .strong(),
            );
            ui.label(
                RichText::new("• Hardware & Software Inventory, AfA Depreciation, and QR Tags")
                    .italics(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let theme_label = if self.dark {
                    "Light theme"
                } else {
                    "Dark theme"
                };
                if ui.button(theme_label).clicked() {
                    self.dark = !self.dark;
                    theme::apply(ui.ctx(), self.dark);
                }
                if ui.button("⟳ Refresh").clicked() {
                    self.reload_data();
                }
            });
        });

        ui.separator();

        // High-level Stats Bar
        let total_assets = self.assets.len();
        let total_cost_cents: i64 = self.assets.iter().map(|a| a.cost_cents).sum();
        let total_licenses = self.licenses.len();
        let total_seats_assigned: usize = self.licenses.iter().map(|l| l.assigned.len()).sum();

        ui.horizontal(|ui| {
            ui.group(|ui| {
                ui.label(RichText::new("Total Assets:").color(Color32::GRAY));
                ui.label(RichText::new(format!("{total_assets}")).strong());
            });
            ui.group(|ui| {
                ui.label(RichText::new("Acquisition Cost:").color(Color32::GRAY));
                ui.label(
                    RichText::new(format!("€{:.2}", total_cost_cents as f64 / 100.0)).strong(),
                );
            });
            ui.group(|ui| {
                ui.label(RichText::new("Software Licenses:").color(Color32::GRAY));
                ui.label(RichText::new(format!("{total_licenses}")).strong());
            });
            ui.group(|ui| {
                ui.label(RichText::new("Assigned Seats:").color(Color32::GRAY));
                ui.label(RichText::new(format!("{total_seats_assigned}")).strong());
            });
        });

        if let Some(ref msg) = self.status_msg {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                render_badge(ui, "Status", Color32::from_rgb(56, 189, 248));
                ui.label(msg);
            });
        }

        ui.add_space(8.0);

        // Tab Navigation
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.current_tab == ActiveTab::Assets, "💻 Asset Registry")
                .clicked()
            {
                self.current_tab = ActiveTab::Assets;
            }
            if ui
                .selectable_label(
                    self.current_tab == ActiveTab::AfaDepreciation,
                    "📉 AfA Depreciation Cockpit",
                )
                .clicked()
            {
                self.current_tab = ActiveTab::AfaDepreciation;
            }
            if ui
                .selectable_label(
                    self.current_tab == ActiveTab::Licenses,
                    "🔑 Software Licenses & Seats",
                )
                .clicked()
            {
                self.current_tab = ActiveTab::Licenses;
            }
            if ui
                .selectable_label(self.current_tab == ActiveTab::Expiries, "Expiries")
                .clicked()
            {
                self.current_tab = ActiveTab::Expiries;
            }
            if ui
                .selectable_label(
                    self.current_tab == ActiveTab::Labels,
                    "🏷 QR Labels & Barcodes",
                )
                .clicked()
            {
                self.current_tab = ActiveTab::Labels;
            }
        });

        ui.separator();

        match self.current_tab {
            ActiveTab::Assets => self.render_assets(ui),
            ActiveTab::AfaDepreciation => self.render_afa(ui),
            ActiveTab::Licenses => self.render_licenses(ui),
            ActiveTab::Labels => self.render_labels(ui),
            ActiveTab::Expiries => self.render_expiries(ui),
        }
    }
}

impl AppState {
    fn render_assets(&mut self, ui: &mut egui::Ui) {
        // Search & Filter Bar
        ui.horizontal(|ui| {
            ui.label("Search:");
            if ui.text_edit_singleline(&mut self.search_query).changed() {
                self.reload_data();
            }

            ui.separator();
            ui.label("Category:");
            let mut cat_switch = None;
            egui::ComboBox::from_id_salt("cat_filter")
                .selected_text(self.filter_category.as_deref().unwrap_or("All Categories"))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.filter_category.is_none(), "All Categories")
                        .clicked()
                    {
                        cat_switch = Some(None);
                    }
                    for c in &self.categories {
                        if ui
                            .selectable_label(
                                self.filter_category.as_deref() == Some(&c.name),
                                &c.name,
                            )
                            .clicked()
                        {
                            cat_switch = Some(Some(c.name.clone()));
                        }
                    }
                });
            if let Some(target) = cat_switch {
                self.filter_category = target;
                self.reload_data();
            }

            if ui
                .checkbox(&mut self.include_disposed, "Include Disposed")
                .changed()
            {
                self.reload_data();
            }
        });

        ui.add_space(8.0);

        let mut next_selected = None;
        let mut maint_tag = None;

        ui.columns(2, |cols| {
            // Left Column: Asset Registry List
            cols[0].group(|ui| {
                ui.heading(bookend::t(self.lang_de, "Anlagen und Inventar", "Hardware & Asset Inventory"));
                ui.horizontal(|ui| {
                    ui.label("Tag");
                    ui.add(egui::TextEdit::singleline(&mut self.new_tag).desired_width(70.0));
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut self.new_name).desired_width(140.0));
                    ui.label("Category");
                    ui.add(egui::TextEdit::singleline(&mut self.new_category).desired_width(70.0));
                    ui.label("Cost");
                    ui.add(egui::TextEdit::singleline(&mut self.new_cost).desired_width(80.0));
                    if ui.button("Register").clicked() {
                        self.register_asset();
                    }
                    if ui.button("Load demo").clicked() {
                        self.load_demo();
                    }
                });
                ui.label(RichText::new("Categories include it, car, office, furniture, machine, other. Cost is Austrian: 1.499,00. Demo names say DEMO.").color(tokens(self.dark).text_weak));
                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .id_salt("asset_scroll")
                    .show(ui, |ui| {
                        egui::Grid::new("asset_grid")
                            .striped(true)
                            .min_col_width(80.0)
                            .show(ui, |ui| {
                                ui.strong("Tag");
                                ui.strong("Name");
                                ui.strong("Category");
                                ui.strong("Holder");
                                ui.strong("Cost");
                                ui.strong("Status");
                                ui.end_row();

                                for a in &self.assets {
                                    let is_sel = self.selected_asset.as_ref().map(|s| &s.tag)
                                        == Some(&a.tag);
                                    if ui.selectable_label(is_sel, &a.tag).clicked() {
                                        next_selected = Some(a.clone());
                                    }
                                    ui.label(&a.name);
                                    ui.label(&a.category);
                                    ui.label(a.holder.as_deref().unwrap_or("-"));
                                    ui.label(format!("€{:.2}", a.cost_cents as f64 / 100.0));
                                    if a.disposed_on.is_some() {
                                        render_badge(ui, "Disposed", Color32::GRAY);
                                    } else if a.gwg {
                                        render_badge(ui, "GWG", Color32::from_rgb(245, 158, 11));
                                    } else {
                                        render_badge(ui, "Active", Color32::from_rgb(34, 197, 94));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });

            // Right Column: Asset Details & Maintenance Drawer
            cols[1].group(|ui| {
                ui.heading(bookend::t(
                    self.lang_de,
                    "Anlage und Wartung",
                    "Asset Details & Maintenance",
                ));
                ui.add_space(4.0);

                if let Some(ref a) = self.selected_asset {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&a.tag)
                                .color(Color32::from_rgb(56, 189, 248))
                                .size(18.0)
                                .strong(),
                        );
                        ui.label(RichText::new(&a.name).size(16.0));
                    });
                    ui.label(format!(
                        "Category: {} | Serial: {}",
                        a.category,
                        a.serial.as_deref().unwrap_or("-")
                    ));
                    ui.label(format!(
                        "Location: {} | Holder: {}",
                        a.location.as_deref().unwrap_or("-"),
                        a.holder.as_deref().unwrap_or("-")
                    ));
                    ui.label(format!(
                        "In-Service: {} | Useful Life: {} years",
                        a.in_service, a.life_years
                    ));
                    ui.label(format!(
                        "Acquisition Cost: €{:.2}",
                        a.cost_cents as f64 / 100.0
                    ));
                    if let Some(w) = a.warranty_until {
                        ui.label(format!("Warranty Until: {w}"));
                    }
                    if !a.notes.is_empty() {
                        ui.label(format!("Notes: {}", a.notes));
                    }

                    ui.separator();
                    ui.heading(bookend::t(self.lang_de, "Wartungsverlauf", "Maintenance History"));
                    ui.horizontal(|ui| {
                        ui.label("Kind");
                        ui.add(egui::TextEdit::singleline(&mut self.maint_kind).desired_width(80.0));
                        ui.label("Cost");
                        ui.add(egui::TextEdit::singleline(&mut self.maint_cost).desired_width(70.0));
                        if ui.button("Add maintenance").clicked() {
                            maint_tag = Some(a.tag.clone());
                        }
                    });
                    if self.selected_maint.is_empty() {
                        ui.label(
                            RichText::new("No maintenance recorded for this asset.").italics(),
                        );
                    } else {
                        egui::Grid::new("maint_grid").striped(true).show(ui, |ui| {
                            ui.strong("Date");
                            ui.strong("Kind");
                            ui.strong("Cost");
                            ui.strong("Note");
                            ui.end_row();

                            for m in &self.selected_maint {
                                ui.label(m.date.format("%Y-%m-%d").to_string());
                                ui.label(&m.kind);
                                ui.label(format!("€{:.2}", m.cost_cents as f64 / 100.0));
                                ui.label(&m.note);
                                ui.end_row();
                            }
                        });
                    }
                } else {
                    ui.label(
                        RichText::new("Select an asset to view details and maintenance.").italics(),
                    );
                }
            });
        });

        if let Some(asset) = next_selected {
            self.select_asset(asset);
        }
        if let Some(tag) = maint_tag {
            self.add_maintenance(&tag);
        }
    }

    fn add_maintenance(&mut self, tag: &str) {
        let cents = match Money::parse_de(&self.maint_cost) {
            Ok(m) if m.cents() >= 0 => m.cents(),
            _ => {
                self.status_msg = Some("Maintenance cost is an amount such as 38,40.".into());
                return;
            }
        };
        let today = Local::now().date_naive();
        let kind = self.maint_kind.clone();
        match self.reg.add_maintenance(tag, today, &kind, cents, "") {
            Ok(_) => {
                self.status_msg = Some(format!("Maintenance recorded for {tag}."));
                self.maint_cost.clear();
                if let Ok(a) = self.reg.asset(tag) {
                    self.select_asset(a);
                }
            }
            Err(e) => self.status_msg = Some(e.to_string()),
        }
    }

    fn add_licence(&mut self) {
        let name = self.lic_name.trim().to_string();
        if name.is_empty() {
            self.status_msg = Some("A licence needs a name.".into());
            return;
        }
        let seats: i64 = match self.lic_seats.trim().parse() {
            Ok(n) if n > 0 => n,
            _ => {
                self.status_msg = Some("Seats must be a positive whole number.".into());
                return;
            }
        };
        let vendor = self.lic_vendor.trim().to_string();
        let where_key = self.lic_where.trim().to_string();
        match self.reg.add_license(&NewLicense {
            name,
            vendor: if vendor.is_empty() {
                None
            } else {
                Some(vendor)
            },
            seats,
            key_location: if where_key.is_empty() {
                None
            } else {
                Some(where_key)
            },
            ..Default::default()
        }) {
            Ok(lic) => {
                self.status_msg = Some(format!("Licence {} added.", lic.name));
                self.lic_name.clear();
                self.reload_data();
            }
            Err(e) => self.status_msg = Some(e.to_string()),
        }
    }

    fn add_deadline(&mut self) {
        let name = self.dl_name.trim().to_string();
        if name.is_empty() {
            self.status_msg = Some("An expiry needs a name.".into());
            return;
        }
        let due = match chrono::NaiveDate::parse_from_str(self.dl_due.trim(), "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => {
                self.status_msg = Some("Due date is YYYY-MM-DD.".into());
                return;
            }
        };
        let tag = self.dl_tag.trim().to_string();
        let kind = self.dl_kind.clone();
        match self.reg.add_deadline(
            &name,
            &kind,
            due,
            if tag.is_empty() {
                None
            } else {
                Some(tag.as_str())
            },
            "",
        ) {
            Ok(_) => {
                self.status_msg = Some(format!("Expiry {name} saved."));
                self.dl_name.clear();
                self.reload_data();
            }
            Err(e) => self.status_msg = Some(e.to_string()),
        }
    }

    fn post_afa(&mut self) {
        let year: i32 = match self.afa_year.trim().parse() {
            Ok(y) => y,
            Err(_) => {
                self.status_msg = Some("AfA year must be a number.".into());
                return;
            }
        };
        let Some(bus) = resolve_bus() else {
            self.status_msg =
                Some("No local bus directory. Set LOCALAPPDATA or CRAFT_BUS_DIR.".into());
            return;
        };
        let hold = match Registry::open_in_memory() {
            Ok(r) => r,
            Err(e) => {
                self.status_msg = Some(e.to_string());
                return;
            }
        };
        let reg = std::mem::replace(&mut self.reg, hold);
        let mut ctx = Ctx {
            reg,
            confinement: Confinement(None),
            bus,
        };
        let today = Local::now().date_naive();
        let result = afa_year(&mut ctx, year, true, today);
        self.reg = ctx.reg;
        self.selected_asset = None;
        self.reload_data();
        match result {
            Ok(v) => {
                let emitted = v.get("emitted").and_then(|x| x.as_u64()).unwrap_or(0);
                let total = v.get("total").and_then(|x| x.as_str()).unwrap_or("");
                self.status_msg = Some(format!(
                    "AfA {year}: {total}. Sent {emitted} drafts to BuchCraft. A human still approves them."
                ));
            }
            Err(e) => self.status_msg = Some(e),
        }
    }

    fn render_expiries(&mut self, ui: &mut egui::Ui) {
        ui.heading(bookend::t(
            self.lang_de,
            "Garantien, Zertifikate, Domains, Versicherungen und Prüfungen",
            "Warranties, certificates, domains, insurance, and inspections",
        ));
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut self.dl_name).desired_width(180.0));
            ui.label("Kind");
            egui::ComboBox::from_id_salt("deadline_kind")
                .selected_text(&self.dl_kind)
                .show_ui(ui, |ui| {
                    for kind in DEADLINE_KINDS {
                        ui.selectable_value(&mut self.dl_kind, (*kind).to_string(), *kind);
                    }
                });
            ui.label("Due");
            ui.add(egui::TextEdit::singleline(&mut self.dl_due).desired_width(90.0));
            ui.label("Asset tag");
            ui.add(egui::TextEdit::singleline(&mut self.dl_tag).desired_width(70.0));
            if ui.button("Add expiry").clicked() {
                self.add_deadline();
            }
        });
        ui.label("Kind is certificate, domain, insurance, inspection, contract, or other. Due is YYYY-MM-DD. Tag is optional.");
        if self.deadlines.is_empty() {
            ui.label("No open expiries. Load the demo inventory, or add one.");
        }
        egui::Grid::new("expiry_grid").striped(true).show(ui, |ui| {
            ui.strong("Due");
            ui.strong("Kind");
            ui.strong("Name");
            ui.strong("Asset");
            ui.end_row();
            for d in &self.deadlines {
                ui.label(d.due_on.to_string());
                ui.label(&d.kind);
                ui.label(&d.name);
                ui.label(d.tag.as_deref().unwrap_or("-"));
                ui.end_row();
            }
        });
    }

    fn render_afa(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(bookend::t(
            self.lang_de,
            "AfA-Cockpit (lineare Absetzung für Abnutzung)",
            "Austrian AfA (Absetzung für Abnutzung) Linear Depreciation Cockpit",
        ));
            ui.separator();
            if let Some(ref a) = self.selected_asset {
                ui.label(format!("Viewing Asset: {} - {}", a.tag, a.name));
            }
        });

        ui.label(
            "Straight-line depreciation according to EStG §7 with Austrian Halbjahres-AfA (assets in service \
            after July 1 get 50% first-year depreciation) and GWG immediate write-off.",
        );

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Year");
            ui.add(egui::TextEdit::singleline(&mut self.afa_year).desired_width(60.0));
            if ui.button("Post this year's AfA to BuchCraft").clicked() {
                self.post_afa();
            }
        });
        ui.label("Posts asset.depreciation_due drafts. BuchCraft still has to approve them. A year is sent once.");

        if let Some(ref sched) = self.selected_afa {
            egui::ScrollArea::vertical()
                .id_salt("afa_scroll")
                .show(ui, |ui| {
                    egui::Grid::new("afa_grid")
                        .striped(true)
                        .min_col_width(120.0)
                        .show(ui, |ui| {
                            ui.strong("Fiscal Year");
                            ui.strong("Opening Book Value");
                            ui.strong("Annual AfA Amount");
                            ui.strong("Closing Book Value");
                            ui.strong("Depreciation Rule");
                            ui.end_row();

                            for row in &sched.rows {
                                ui.label(format!("{}", row.year));
                                ui.label(format!("€{:.2}", row.opening_cents as f64 / 100.0));
                                ui.label(
                                    RichText::new(format!(
                                        "€{:.2}",
                                        row.amount_cents as f64 / 100.0
                                    ))
                                    .strong(),
                                );
                                ui.label(format!("€{:.2}", row.closing_cents as f64 / 100.0));

                                let (tone_text, tone_color) = match row.kind {
                                    "gwg" => {
                                        ("GWG Instant Write-off", Color32::from_rgb(245, 158, 11))
                                    }
                                    "half" => {
                                        ("Halbjahres-AfA (50%)", Color32::from_rgb(56, 189, 248))
                                    }
                                    "full" => ("Full Year AfA", Color32::from_rgb(34, 197, 94)),
                                    "final" => {
                                        ("Final Residual Year", Color32::from_rgb(168, 85, 247))
                                    }
                                    _ => (row.kind, Color32::GRAY),
                                };
                                render_badge(ui, tone_text, tone_color);
                                ui.end_row();
                            }
                        });
                });

            if let Some(res) = sched.residual_at_disposal_cents {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    render_badge(ui, "Anlagenabgang", Color32::from_rgb(239, 68, 68));
                    ui.label(format!(
                        "Residual book value at disposal: €{:.2}",
                        res as f64 / 100.0
                    ));
                });
            }
        } else {
            ui.label(
                RichText::new("Select an active asset to calculate and view its AfA schedule.")
                    .italics(),
            );
        }
    }

    fn render_licenses(&mut self, ui: &mut egui::Ui) {
        ui.heading(bookend::t(
            self.lang_de,
            "Softwarelizenzen, Abos und Sitzverteilung",
            "Software Licenses, Subscriptions & Seat Allocation",
        ));
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut self.lic_name).desired_width(140.0));
            ui.label("Vendor");
            ui.add(egui::TextEdit::singleline(&mut self.lic_vendor).desired_width(120.0));
            ui.label("Seats");
            ui.add(egui::TextEdit::singleline(&mut self.lic_seats).desired_width(40.0));
            ui.label("Key lives in");
            ui.add(egui::TextEdit::singleline(&mut self.lic_where).desired_width(140.0));
            if ui.button("Add licence").clicked() {
                self.add_licence();
            }
        });
        ui.label("Store where the key lives, such as Bitwarden: Office. Do not paste the key.");
        ui.add_space(8.0);

        let mut assign_target = None;
        let mut unassign_target = None;

        egui::ScrollArea::vertical()
            .id_salt("lic_scroll")
            .show(ui, |ui| {
                egui::Grid::new("license_grid")
                    .striped(true)
                    .min_col_width(100.0)
                    .show(ui, |ui| {
                        ui.strong("License / App");
                        ui.strong("Vendor");
                        ui.strong("Seats Total");
                        ui.strong("Assigned Count");
                        ui.strong("Utilization");
                        ui.strong("Renewal Date");
                        ui.strong("Assignees");
                        ui.strong("Quick Assign");
                        ui.end_row();

                        for lic in &self.licenses {
                            ui.label(RichText::new(&lic.name).strong());
                            ui.label(lic.vendor.as_deref().unwrap_or("-"));
                            ui.label(format!("{}", lic.seats));
                            ui.label(format!("{}", lic.assigned.len()));

                            let pct = if lic.seats > 0 {
                                (lic.assigned.len() as f64 / lic.seats as f64) * 100.0
                            } else {
                                0.0
                            };
                            let col = if lic.assigned.len() as i64 > lic.seats {
                                Color32::from_rgb(239, 68, 68) // Over-allocated
                            } else if pct >= 90.0 {
                                Color32::from_rgb(245, 158, 11) // Near capacity
                            } else {
                                Color32::from_rgb(34, 197, 94)
                            };
                            render_badge(ui, &format!("{pct:.0}%"), col);

                            ui.label(
                                lic.renews_on
                                    .map(|d| d.format("%Y-%m-%d").to_string())
                                    .unwrap_or_else(|| "-".into()),
                            );

                            ui.horizontal_wrapped(|ui| {
                                for user in &lic.assigned {
                                    ui.label(user);
                                    if ui.small_button("×").clicked() {
                                        unassign_target = Some((lic.name.clone(), user.clone()));
                                    }
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.assignee_input);
                                if ui.small_button("+ Assign").clicked() {
                                    assign_target = Some(lic.name.clone());
                                }
                            });

                            ui.end_row();
                        }
                    });
            });

        if let Some(lic) = assign_target {
            self.assign_license(&lic);
        }
        if let Some((lic, user)) = unassign_target {
            self.unassign_license(&lic, &user);
        }
    }

    fn render_labels(&mut self, ui: &mut egui::Ui) {
        ui.heading(bookend::t(
            self.lang_de,
            "Druckbare Anlagenschilder und Barcode-Vorschau",
            "Printable Asset Tags & Barcode Preview",
        ));
        ui.label("Generates QR codes compliant with `ASSET:<tag>` format for mobile scan intake and audit checks.");

        ui.add_space(8.0);

        if let Some(ref a) = self.selected_asset {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Selected Asset:");
                    ui.label(RichText::new(format!("{} - {}", a.tag, a.name)).strong());
                });

                let payload = label::payload(a);
                ui.label(format!("QR Payload: {payload}"));

                if let Ok(svg_data) = label::svg(a) {
                    ui.separator();
                    ui.label("Generated Label SVG (Sample Text Preview):");
                    ui.text_edit_multiline(&mut svg_data.as_str());
                }
            });
        } else {
            ui.label(RichText::new("Select an asset to view its QR label preview.").italics());
        }
    }
}

fn main() -> eframe::Result<()> {
    let cli = Cli::parse();

    if cli.smoke {
        let db_path = cli.db.unwrap_or_else(default_db);
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match Registry::open(&db_path) {
            Ok(reg) => {
                let _ = reg.assets(None, true, None);
                if let Err(e) = bookend::smoke("AssetCraft", env!("CARGO_PKG_VERSION"), "assets") {
                    eprintln!("smoke: menu: {e}");
                    std::process::exit(1);
                }
                println!("smoke: ok (assetcraft-app)");
                return Ok(());
            }
            Err(e) => {
                eprintln!("smoke: failed: {e}");
                std::process::exit(1);
            }
        }
    }

    let db_path = cli.db.unwrap_or_else(default_db);
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let reg = match Registry::open(&db_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("assetcraft: cannot open db {}: {e}", db_path.display());
            std::process::exit(1);
        }
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("assetcraft - Asset Inventory & AfA Depreciation Workstation")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([880.0, 560.0]),
        ..Default::default()
    };

    let state = AppState::new(reg);

    eframe::run_native(
        "assetcraft-app",
        native_options,
        Box::new(|_cc| Ok(Box::new(state))),
    )
}
