//! assetcraft-store: fixed assets, software licences and deadlines.
//!
//! Invariants:
//! - Money is i64 cents; acquisition cost is the AfA base (enter net if VAT is deductible).
//! - Useful life is never below the category minimum (e.g. 8 years for cars, EStG section 8(6)).
//! - Once AfA for an asset has been posted to buchcraft, its AfA basis (cost, in-service
//!   date, life, GWG flag) is frozen. Enforced in Rust and by a SQLite trigger.
//! - A disposed asset is read-only (trigger).
//! - Licence keys are never stored; only where the key lives (`key_location`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use chrono::{NaiveDate, Utc};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 1;
/// GWG limit since 2023 (EStG section 13): 1.000,00 EUR.
pub const DEFAULT_GWG_LIMIT_CENTS: i64 = 100_000;

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("database: {0}")]
    Db(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("locked: {0}")]
    Locked(String),
    #[error("conflict: {0}")]
    Conflict(String),
}

impl From<rusqlite::Error> for AssetError {
    fn from(e: rusqlite::Error) -> Self {
        let s = e.to_string();
        if s.contains("frozen") || s.contains("disposed") {
            AssetError::Locked(s)
        } else {
            AssetError::Db(s)
        }
    }
}

pub type Result<T> = std::result::Result<T, AssetError>;

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[derive(Debug, Clone, Serialize)]
pub struct Category {
    pub name: String,
    pub life_years: i64,
    pub min_years: i64,
    /// EKR asset account the AfA is credited to.
    pub account: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Asset {
    pub id: i64,
    pub tag: String,
    pub name: String,
    pub category: String,
    pub serial: Option<String>,
    pub location: Option<String>,
    pub holder: Option<String>,
    pub purchase_date: Option<NaiveDate>,
    pub in_service: NaiveDate,
    pub cost_cents: i64,
    pub life_years: i64,
    pub gwg: bool,
    pub supplier: Option<String>,
    pub invoice_ref: Option<String>,
    pub warranty_until: Option<NaiveDate>,
    pub disposed_on: Option<NaiveDate>,
    pub disposal_proceeds_cents: Option<i64>,
    pub disposal_note: Option<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Default)]
pub struct NewAsset {
    pub tag: String,
    pub name: String,
    pub category: String,
    pub serial: Option<String>,
    pub location: Option<String>,
    pub holder: Option<String>,
    pub purchase_date: Option<NaiveDate>,
    pub in_service: NaiveDate,
    pub cost_cents: i64,
    pub life_years: Option<i64>,
    /// None = automatic (cost at or below the GWG limit).
    pub gwg: Option<bool>,
    pub supplier: Option<String>,
    pub invoice_ref: Option<String>,
    pub warranty_until: Option<NaiveDate>,
    pub notes: String,
}

/// Outer Option = leave unchanged; inner None = clear.
#[derive(Debug, Clone, Default)]
pub struct AssetPatch {
    pub name: Option<String>,
    pub serial: Option<Option<String>>,
    pub location: Option<Option<String>>,
    pub holder: Option<Option<String>>,
    pub warranty_until: Option<Option<NaiveDate>>,
    pub notes: Option<String>,
    pub cost_cents: Option<i64>,
    pub in_service: Option<NaiveDate>,
    pub life_years: Option<i64>,
    pub gwg: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Maintenance {
    pub id: i64,
    pub tag: String,
    pub date: NaiveDate,
    pub kind: String,
    pub cost_cents: i64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Posting {
    pub tag: String,
    pub year: i32,
    pub amount_cents: i64,
    pub event_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct License {
    pub id: i64,
    pub name: String,
    pub vendor: Option<String>,
    pub seats: i64,
    pub annual_cost_cents: Option<i64>,
    pub renews_on: Option<NaiveDate>,
    pub key_location: Option<String>,
    pub notes: String,
    pub assigned: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NewLicense {
    pub name: String,
    pub vendor: Option<String>,
    pub seats: i64,
    pub annual_cost_cents: Option<i64>,
    pub renews_on: Option<NaiveDate>,
    pub key_location: Option<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Deadline {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub due_on: NaiveDate,
    pub tag: Option<String>,
    pub notes: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub assets_active: i64,
    pub assets_disposed: i64,
    pub cost_active_cents: i64,
    pub licenses: i64,
    pub seats_total: i64,
    pub seats_assigned: i64,
    pub deadlines_open: i64,
    pub demo: bool,
}

pub const DEADLINE_KINDS: &[&str] = &[
    "certificate",
    "domain",
    "insurance",
    "inspection",
    "contract",
    "other",
];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS categories (
    name TEXT PRIMARY KEY,
    life_years INTEGER NOT NULL CHECK (life_years BETWEEN 1 AND 50),
    min_years INTEGER NOT NULL CHECK (min_years BETWEEN 0 AND 50),
    account INTEGER NOT NULL CHECK (account BETWEEN 0 AND 9999),
    CHECK (life_years >= min_years)
);
CREATE TABLE IF NOT EXISTS assets (
    id INTEGER PRIMARY KEY,
    tag TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    category TEXT NOT NULL REFERENCES categories(name),
    serial TEXT,
    location TEXT,
    holder TEXT,
    purchase_date TEXT,
    in_service TEXT NOT NULL,
    cost_cents INTEGER NOT NULL CHECK (cost_cents > 0),
    life_years INTEGER NOT NULL CHECK (life_years BETWEEN 1 AND 50),
    gwg INTEGER NOT NULL DEFAULT 0,
    supplier TEXT,
    invoice_ref TEXT,
    warranty_until TEXT,
    disposed_on TEXT,
    disposal_proceeds_cents INTEGER CHECK (disposal_proceeds_cents IS NULL OR disposal_proceeds_cents >= 0),
    disposal_note TEXT,
    notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS afa_postings (
    asset_id INTEGER NOT NULL REFERENCES assets(id),
    year INTEGER NOT NULL,
    amount_cents INTEGER NOT NULL CHECK (amount_cents > 0),
    event_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (asset_id, year)
);
CREATE TRIGGER IF NOT EXISTS trg_asset_disposed BEFORE UPDATE ON assets
WHEN OLD.disposed_on IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'asset is disposed and read-only'); END;
CREATE TRIGGER IF NOT EXISTS trg_asset_basis_frozen BEFORE UPDATE OF cost_cents, in_service, life_years, gwg ON assets
WHEN EXISTS (SELECT 1 FROM afa_postings p WHERE p.asset_id = OLD.id)
 AND (NEW.cost_cents <> OLD.cost_cents OR NEW.in_service <> OLD.in_service OR NEW.life_years <> OLD.life_years OR NEW.gwg <> OLD.gwg)
BEGIN SELECT RAISE(ABORT, 'AfA basis is frozen: AfA was already posted'); END;
CREATE TABLE IF NOT EXISTS maintenance (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL REFERENCES assets(id),
    date TEXT NOT NULL,
    kind TEXT NOT NULL,
    cost_cents INTEGER NOT NULL DEFAULT 0 CHECK (cost_cents >= 0),
    note TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS licenses (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    vendor TEXT,
    seats INTEGER NOT NULL CHECK (seats > 0),
    annual_cost_cents INTEGER CHECK (annual_cost_cents IS NULL OR annual_cost_cents >= 0),
    renews_on TEXT,
    key_location TEXT,
    notes TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS license_seats (
    license_id INTEGER NOT NULL REFERENCES licenses(id),
    assignee TEXT NOT NULL,
    assigned_on TEXT NOT NULL,
    PRIMARY KEY (license_id, assignee)
);
CREATE TABLE IF NOT EXISTS deadlines (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    due_on TEXT NOT NULL,
    asset_id INTEGER REFERENCES assets(id),
    notes TEXT NOT NULL DEFAULT '',
    done INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO categories (name, life_years, min_years, account) VALUES
    ('it', 3, 0, 620),
    ('car', 8, 8, 630),
    ('office', 5, 0, 660),
    ('furniture', 10, 0, 660),
    ('machine', 5, 0, 660),
    ('other', 5, 0, 660);
";

const ASSET_COLS: &str = "a.id, a.tag, a.name, a.category, a.serial, a.location, a.holder, a.purchase_date, a.in_service, a.cost_cents, a.life_years, a.gwg,
    a.supplier, a.invoice_ref, a.warranty_until, a.disposed_on, a.disposal_proceeds_cents, a.disposal_note, a.notes";

fn asset_row(r: &Row) -> rusqlite::Result<Asset> {
    Ok(Asset {
        id: r.get(0)?,
        tag: r.get(1)?,
        name: r.get(2)?,
        category: r.get(3)?,
        serial: r.get(4)?,
        location: r.get(5)?,
        holder: r.get(6)?,
        purchase_date: r.get(7)?,
        in_service: r.get(8)?,
        cost_cents: r.get(9)?,
        life_years: r.get(10)?,
        gwg: r.get(11)?,
        supplier: r.get(12)?,
        invoice_ref: r.get(13)?,
        warranty_until: r.get(14)?,
        disposed_on: r.get(15)?,
        disposal_proceeds_cents: r.get(16)?,
        disposal_note: r.get(17)?,
        notes: r.get(18)?,
    })
}

fn clean(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Asset tags: 1-24 chars, A-Z 0-9 and '-', stored upper case.
pub fn validate_tag(tag: &str) -> Result<String> {
    let t = tag.trim().to_ascii_uppercase();
    if t.is_empty() || t.len() > 24 || !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(AssetError::Invalid(format!(
            "asset tag {tag:?} must be 1-24 letters, digits or '-'"
        )));
    }
    Ok(t)
}

fn validate_name(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 200 {
        return Err(AssetError::Invalid("name must be 1-200 characters".into()));
    }
    Ok(n.to_string())
}

pub struct Registry {
    conn: Connection,
}

impl Registry {
    pub fn open(path: &Path) -> Result<Registry> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| AssetError::Db(e.to_string()))?;
            }
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Registry> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Registry> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
        conn.execute_batch(SCHEMA)?;
        let v: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match v.as_deref() {
            None => {
                conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                    params![SCHEMA_VERSION.to_string()],
                )?;
            }
            Some(x) if x == SCHEMA_VERSION.to_string() => {}
            Some(x) => {
                return Err(AssetError::Db(format!(
                    "database schema {x} is newer than this build ({SCHEMA_VERSION})"
                )))
            }
        }
        Ok(Registry { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
        Ok(())
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn is_demo(&self) -> Result<bool> {
        Ok(self.meta("demo")?.as_deref() == Some("1"))
    }

    pub fn gwg_limit(&self) -> Result<i64> {
        Ok(self
            .meta("gwg_limit_cents")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_GWG_LIMIT_CENTS))
    }

    pub fn set_gwg_limit(&self, cents: i64) -> Result<()> {
        if !(1..=10_000_000).contains(&cents) {
            return Err(AssetError::Invalid(
                "GWG limit must be between 0,01 and 100.000,00".into(),
            ));
        }
        self.set_meta("gwg_limit_cents", &cents.to_string())
    }

    // ---------- categories ----------

    pub fn categories(&self) -> Result<Vec<Category>> {
        let mut st = self
            .conn
            .prepare("SELECT name, life_years, min_years, account FROM categories ORDER BY name")?;
        let rows = st
            .query_map([], |r| {
                Ok(Category {
                    name: r.get(0)?,
                    life_years: r.get(1)?,
                    min_years: r.get(2)?,
                    account: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn category(&self, name: &str) -> Result<Category> {
        let n = name.trim().to_ascii_lowercase();
        self.categories()?
            .into_iter()
            .find(|c| c.name == n)
            .ok_or_else(|| AssetError::NotFound(format!("category {name} (see categories)")))
    }

    pub fn set_category(
        &self,
        name: &str,
        life_years: i64,
        min_years: i64,
        account: i64,
    ) -> Result<Category> {
        let n = name.trim().to_ascii_lowercase();
        if n.is_empty() || n.len() > 40 || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(AssetError::Invalid(
                "category name: 1-40 of a-z, 0-9, _".into(),
            ));
        }
        if !(1..=50).contains(&life_years)
            || !(0..=life_years).contains(&min_years)
            || !(0..=9999).contains(&account)
        {
            return Err(AssetError::Invalid(
                "life 1-50 years, 0 <= min <= life, account 0-9999".into(),
            ));
        }
        self.conn.execute(
            "INSERT INTO categories (name, life_years, min_years, account) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(name) DO UPDATE SET life_years = excluded.life_years, min_years = excluded.min_years, account = excluded.account",
            params![n, life_years, min_years, account],
        )?;
        self.category(&n)
    }

    // ---------- assets ----------

    pub fn add_asset(&self, n: &NewAsset) -> Result<Asset> {
        let tag = validate_tag(&n.tag)?;
        let name = validate_name(&n.name)?;
        let cat = self.category(&n.category)?;
        if n.cost_cents <= 0 {
            return Err(AssetError::Invalid("cost must be positive".into()));
        }
        let limit = self.gwg_limit()?;
        let gwg = match n.gwg {
            Some(true) if n.cost_cents > limit => {
                return Err(AssetError::Invalid(format!(
                    "cost is above the GWG limit of {} cents",
                    limit
                )))
            }
            Some(g) => g,
            None => n.cost_cents <= limit,
        };
        let life = n.life_years.unwrap_or(cat.life_years);
        if !(1..=50).contains(&life) {
            return Err(AssetError::Invalid("life must be 1-50 years".into()));
        }
        if !gwg && life < cat.min_years {
            return Err(AssetError::Invalid(format!(
                "category {} needs at least {} years useful life",
                cat.name, cat.min_years
            )));
        }
        if let Some(p) = n.purchase_date {
            if p > n.in_service + chrono::Duration::days(366) {
                return Err(AssetError::Invalid(
                    "purchase date is more than a year after the in-service date".into(),
                ));
            }
        }
        if self.asset(&tag).is_ok() {
            return Err(AssetError::Conflict(format!("asset {tag} exists")));
        }
        self.conn.execute(
            "INSERT INTO assets (tag, name, category, serial, location, holder, purchase_date, in_service, cost_cents, life_years, gwg, supplier, invoice_ref, warranty_until, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                tag,
                name,
                cat.name,
                clean(n.serial.clone()),
                clean(n.location.clone()),
                clean(n.holder.clone()),
                n.purchase_date,
                n.in_service,
                n.cost_cents,
                life,
                gwg,
                clean(n.supplier.clone()),
                clean(n.invoice_ref.clone()),
                n.warranty_until,
                n.notes.trim(),
                now()
            ],
        )?;
        self.asset(&tag)
    }

    pub fn asset(&self, tag: &str) -> Result<Asset> {
        let t = tag.trim().to_ascii_uppercase();
        self.conn
            .query_row(
                &format!("SELECT {ASSET_COLS} FROM assets a WHERE a.tag = ?1"),
                params![t],
                asset_row,
            )
            .optional()?
            .ok_or_else(|| AssetError::NotFound(format!("asset {tag}")))
    }

    pub fn assets(
        &self,
        category: Option<&str>,
        include_disposed: bool,
        text: Option<&str>,
    ) -> Result<Vec<Asset>> {
        let mut sql = format!("SELECT {ASSET_COLS} FROM assets a WHERE 1 = 1");
        let mut args: Vec<SqlValue> = Vec::new();
        if let Some(c) = category {
            args.push(SqlValue::Text(c.trim().to_ascii_lowercase()));
            sql.push_str(&format!(" AND a.category = ?{}", args.len()));
        }
        if !include_disposed {
            sql.push_str(" AND a.disposed_on IS NULL");
        }
        if let Some(t) = text.map(str::trim).filter(|t| !t.is_empty()) {
            args.push(SqlValue::Text(format!("%{}%", t.to_lowercase())));
            let i = args.len();
            sql.push_str(&format!(" AND (lower(a.name) LIKE ?{i} OR lower(a.tag) LIKE ?{i} OR lower(COALESCE(a.serial, '')) LIKE ?{i} OR lower(COALESCE(a.holder, '')) LIKE ?{i} OR lower(COALESCE(a.location, '')) LIKE ?{i})"));
        }
        sql.push_str(" ORDER BY a.tag");
        let mut st = self.conn.prepare(&sql)?;
        let rows = st
            .query_map(params_from_iter(args.iter()), asset_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn update_asset(&self, tag: &str, p: &AssetPatch) -> Result<Asset> {
        let a = self.asset(tag)?;
        if a.disposed_on.is_some() {
            return Err(AssetError::Locked(format!(
                "asset {} is disposed and read-only",
                a.tag
            )));
        }
        let basis_change = p.cost_cents.is_some_and(|c| c != a.cost_cents)
            || p.in_service.is_some_and(|d| d != a.in_service)
            || p.life_years.is_some_and(|l| l != a.life_years)
            || p.gwg.is_some_and(|g| g != a.gwg);
        if basis_change && !self.postings(&a.tag)?.is_empty() {
            return Err(AssetError::Locked(format!(
                "AfA basis of {} is frozen: AfA was already posted",
                a.tag
            )));
        }
        let name = match &p.name {
            Some(n) => validate_name(n)?,
            None => a.name.clone(),
        };
        let cost = p.cost_cents.unwrap_or(a.cost_cents);
        if cost <= 0 {
            return Err(AssetError::Invalid("cost must be positive".into()));
        }
        let gwg = p.gwg.unwrap_or(a.gwg);
        if gwg && cost > self.gwg_limit()? {
            return Err(AssetError::Invalid("cost is above the GWG limit".into()));
        }
        let life = p.life_years.unwrap_or(a.life_years);
        let cat = self.category(&a.category)?;
        if !(1..=50).contains(&life) || (!gwg && life < cat.min_years) {
            return Err(AssetError::Invalid(format!(
                "life must be 1-50 years and at least {} for {}",
                cat.min_years, cat.name
            )));
        }
        self.conn.execute(
            "UPDATE assets SET name = ?2, serial = ?3, location = ?4, holder = ?5, warranty_until = ?6, notes = ?7, cost_cents = ?8, in_service = ?9, life_years = ?10, gwg = ?11 WHERE id = ?1",
            params![
                a.id,
                name,
                p.serial.clone().map(clean).unwrap_or(a.serial),
                p.location.clone().map(clean).unwrap_or(a.location),
                p.holder.clone().map(clean).unwrap_or(a.holder),
                p.warranty_until.unwrap_or(a.warranty_until),
                p.notes.as_deref().map(str::trim).unwrap_or(&a.notes),
                cost,
                p.in_service.unwrap_or(a.in_service),
                life,
                gwg
            ],
        )?;
        self.asset(&a.tag)
    }

    pub fn dispose(
        &self,
        tag: &str,
        on: NaiveDate,
        proceeds_cents: i64,
        note: &str,
    ) -> Result<Asset> {
        let a = self.asset(tag)?;
        if a.disposed_on.is_some() {
            return Err(AssetError::Conflict(format!(
                "asset {} is already disposed",
                a.tag
            )));
        }
        if on < a.in_service {
            return Err(AssetError::Invalid(
                "disposal date is before the in-service date".into(),
            ));
        }
        if proceeds_cents < 0 {
            return Err(AssetError::Invalid("proceeds cannot be negative".into()));
        }
        self.conn.execute(
            "UPDATE assets SET disposed_on = ?2, disposal_proceeds_cents = ?3, disposal_note = ?4 WHERE id = ?1",
            params![a.id, on, proceeds_cents, clean(Some(note.to_string()))],
        )?;
        self.asset(&a.tag)
    }

    // ---------- maintenance ----------

    pub fn add_maintenance(
        &self,
        tag: &str,
        date: NaiveDate,
        kind: &str,
        cost_cents: i64,
        note: &str,
    ) -> Result<Maintenance> {
        let a = self.asset(tag)?;
        let kind = kind.trim();
        if kind.is_empty() || kind.len() > 60 {
            return Err(AssetError::Invalid(
                "kind must be 1-60 characters (e.g. repair, service, inspection)".into(),
            ));
        }
        if cost_cents < 0 {
            return Err(AssetError::Invalid("cost cannot be negative".into()));
        }
        self.conn.execute("INSERT INTO maintenance (asset_id, date, kind, cost_cents, note) VALUES (?1, ?2, ?3, ?4, ?5)", params![a.id, date, kind, cost_cents, note.trim()])?;
        Ok(Maintenance {
            id: self.conn.last_insert_rowid(),
            tag: a.tag,
            date,
            kind: kind.into(),
            cost_cents,
            note: note.trim().into(),
        })
    }

    pub fn maintenance(&self, tag: &str) -> Result<Vec<Maintenance>> {
        let a = self.asset(tag)?;
        let mut st = self.conn.prepare("SELECT id, date, kind, cost_cents, note FROM maintenance WHERE asset_id = ?1 ORDER BY date, id")?;
        let rows = st
            .query_map(params![a.id], |r| {
                Ok(Maintenance {
                    id: r.get(0)?,
                    tag: a.tag.clone(),
                    date: r.get(1)?,
                    kind: r.get(2)?,
                    cost_cents: r.get(3)?,
                    note: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---------- AfA postings ----------

    pub fn postings(&self, tag: &str) -> Result<Vec<Posting>> {
        let a = self.asset(tag)?;
        let mut st = self.conn.prepare("SELECT year, amount_cents, event_id, created_at FROM afa_postings WHERE asset_id = ?1 ORDER BY year")?;
        let rows = st
            .query_map(params![a.id], |r| {
                Ok(Posting {
                    tag: a.tag.clone(),
                    year: r.get(0)?,
                    amount_cents: r.get(1)?,
                    event_id: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn posted(&self, asset_id: i64, year: i32) -> Result<Option<Posting>> {
        Ok(self
            .conn
            .query_row(
                "SELECT a.tag, p.year, p.amount_cents, p.event_id, p.created_at FROM afa_postings p JOIN assets a ON a.id = p.asset_id WHERE p.asset_id = ?1 AND p.year = ?2",
                params![asset_id, year],
                |r| Ok(Posting { tag: r.get(0)?, year: r.get(1)?, amount_cents: r.get(2)?, event_id: r.get(3)?, created_at: r.get(4)? }),
            )
            .optional()?)
    }

    pub fn record_posting(
        &self,
        asset_id: i64,
        year: i32,
        amount_cents: i64,
        event_id: &str,
    ) -> Result<()> {
        if self.posted(asset_id, year)?.is_some() {
            return Err(AssetError::Conflict(format!(
                "AfA {year} already posted for asset {asset_id}"
            )));
        }
        self.conn.execute("INSERT INTO afa_postings (asset_id, year, amount_cents, event_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![asset_id, year, amount_cents, event_id, now()])?;
        Ok(())
    }

    // ---------- licences ----------

    fn seats_of(&self, license_id: i64) -> Result<Vec<String>> {
        let mut st = self.conn.prepare(
            "SELECT assignee FROM license_seats WHERE license_id = ?1 ORDER BY assignee",
        )?;
        let rows = st
            .query_map(params![license_id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    pub fn add_license(&self, n: &NewLicense) -> Result<License> {
        let name = validate_name(&n.name)?;
        if n.seats <= 0 || n.seats > 100_000 {
            return Err(AssetError::Invalid("seats must be 1-100000".into()));
        }
        if n.annual_cost_cents.is_some_and(|c| c < 0) {
            return Err(AssetError::Invalid("cost cannot be negative".into()));
        }
        if let Some(k) = &n.key_location {
            // Heuristic: a licence key pasted where its location belongs.
            let compact: String = k.chars().filter(|c| !c.is_whitespace()).collect();
            if compact.len() >= 16
                && compact.chars().filter(|c| *c == '-').count() >= 3
                && !compact.contains(['/', '\\', ':'])
            {
                return Err(AssetError::Invalid("key_location looks like a licence key; store where the key lives (e.g. 'Bitwarden: Office 365'), not the key".into()));
            }
        }
        if self.license(&name).is_ok() {
            return Err(AssetError::Conflict(format!("licence {name} exists")));
        }
        self.conn.execute(
            "INSERT INTO licenses (name, vendor, seats, annual_cost_cents, renews_on, key_location, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![name, clean(n.vendor.clone()), n.seats, n.annual_cost_cents, n.renews_on, clean(n.key_location.clone()), n.notes.trim()],
        )?;
        self.license(&name)
    }

    pub fn license(&self, name: &str) -> Result<License> {
        let row = self
            .conn
            .query_row(
                "SELECT id, name, vendor, seats, annual_cost_cents, renews_on, key_location, notes FROM licenses WHERE lower(name) = lower(?1)",
                params![name.trim()],
                |r| {
                    Ok(License {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        vendor: r.get(2)?,
                        seats: r.get(3)?,
                        annual_cost_cents: r.get(4)?,
                        renews_on: r.get(5)?,
                        key_location: r.get(6)?,
                        notes: r.get(7)?,
                        assigned: Vec::new(),
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AssetError::NotFound(format!("licence {name}")))?;
        let assigned = self.seats_of(row.id)?;
        Ok(License { assigned, ..row })
    }

    pub fn licenses(&self) -> Result<Vec<License>> {
        let names: Vec<String> = {
            let mut st = self
                .conn
                .prepare("SELECT name FROM licenses ORDER BY name")?;
            let v = st
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            v
        };
        names.iter().map(|n| self.license(n)).collect()
    }

    pub fn set_seats(&self, name: &str, seats: i64) -> Result<License> {
        let l = self.license(name)?;
        if seats <= 0 || seats > 100_000 {
            return Err(AssetError::Invalid("seats must be 1-100000".into()));
        }
        self.conn.execute(
            "UPDATE licenses SET seats = ?2 WHERE id = ?1",
            params![l.id, seats],
        )?;
        self.license(&l.name)
    }

    /// Assign a seat. Refused when all seats are taken, unless `force` (the audit then reports it).
    pub fn assign(
        &self,
        name: &str,
        assignee: &str,
        on: NaiveDate,
        force: bool,
    ) -> Result<License> {
        let l = self.license(name)?;
        let who = assignee.trim();
        if who.is_empty() || who.len() > 120 {
            return Err(AssetError::Invalid(
                "assignee must be 1-120 characters".into(),
            ));
        }
        if l.assigned.iter().any(|a| a.eq_ignore_ascii_case(who)) {
            return Err(AssetError::Conflict(format!(
                "{who} already has a seat of {}",
                l.name
            )));
        }
        if l.assigned.len() as i64 >= l.seats && !force {
            return Err(AssetError::Conflict(format!(
                "all {} seats of {} are assigned (pass force to over-allocate)",
                l.seats, l.name
            )));
        }
        self.conn.execute(
            "INSERT INTO license_seats (license_id, assignee, assigned_on) VALUES (?1, ?2, ?3)",
            params![l.id, who, on],
        )?;
        self.license(&l.name)
    }

    pub fn unassign(&self, name: &str, assignee: &str) -> Result<License> {
        let l = self.license(name)?;
        let n = self.conn.execute(
            "DELETE FROM license_seats WHERE license_id = ?1 AND lower(assignee) = lower(?2)",
            params![l.id, assignee.trim()],
        )?;
        if n == 0 {
            return Err(AssetError::NotFound(format!(
                "{assignee} has no seat of {}",
                l.name
            )));
        }
        self.license(&l.name)
    }

    // ---------- deadlines ----------

    pub fn add_deadline(
        &self,
        name: &str,
        kind: &str,
        due_on: NaiveDate,
        tag: Option<&str>,
        notes: &str,
    ) -> Result<Deadline> {
        let name = validate_name(name)?;
        let kind = kind.trim().to_ascii_lowercase();
        if !DEADLINE_KINDS.contains(&kind.as_str()) {
            return Err(AssetError::Invalid(format!(
                "kind must be one of {}",
                DEADLINE_KINDS.join(", ")
            )));
        }
        let asset_id = tag.map(|t| self.asset(t).map(|a| a.id)).transpose()?;
        self.conn.execute("INSERT INTO deadlines (name, kind, due_on, asset_id, notes) VALUES (?1, ?2, ?3, ?4, ?5)", params![name, kind, due_on, asset_id, notes.trim()])?;
        self.deadline(self.conn.last_insert_rowid())
    }

    fn deadline(&self, id: i64) -> Result<Deadline> {
        self.conn
            .query_row(
                "SELECT d.id, d.name, d.kind, d.due_on, a.tag, d.notes, d.done FROM deadlines d LEFT JOIN assets a ON a.id = d.asset_id WHERE d.id = ?1",
                params![id],
                |r| Ok(Deadline { id: r.get(0)?, name: r.get(1)?, kind: r.get(2)?, due_on: r.get(3)?, tag: r.get(4)?, notes: r.get(5)?, done: r.get(6)? }),
            )
            .optional()?
            .ok_or_else(|| AssetError::NotFound(format!("deadline {id}")))
    }

    pub fn deadlines(&self, include_done: bool) -> Result<Vec<Deadline>> {
        let sql = format!(
            "SELECT d.id, d.name, d.kind, d.due_on, a.tag, d.notes, d.done FROM deadlines d LEFT JOIN assets a ON a.id = d.asset_id {} ORDER BY d.due_on",
            if include_done { "" } else { "WHERE d.done = 0" }
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st
            .query_map([], |r| {
                Ok(Deadline {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    due_on: r.get(3)?,
                    tag: r.get(4)?,
                    notes: r.get(5)?,
                    done: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn complete_deadline(&self, id: i64) -> Result<Deadline> {
        let d = self.deadline(id)?;
        if d.done {
            return Err(AssetError::Conflict(format!(
                "deadline {id} is already done"
            )));
        }
        self.conn
            .execute("UPDATE deadlines SET done = 1 WHERE id = ?1", params![id])?;
        self.deadline(id)
    }

    pub fn stats(&self) -> Result<Stats> {
        let one = |sql: &str| -> Result<i64> { Ok(self.conn.query_row(sql, [], |r| r.get(0))?) };
        Ok(Stats {
            assets_active: one("SELECT COUNT(*) FROM assets WHERE disposed_on IS NULL")?,
            assets_disposed: one("SELECT COUNT(*) FROM assets WHERE disposed_on IS NOT NULL")?,
            cost_active_cents: one(
                "SELECT COALESCE(SUM(cost_cents), 0) FROM assets WHERE disposed_on IS NULL",
            )?,
            licenses: one("SELECT COUNT(*) FROM licenses")?,
            seats_total: one("SELECT COALESCE(SUM(seats), 0) FROM licenses")?,
            seats_assigned: one("SELECT COUNT(*) FROM license_seats")?,
            deadlines_open: one("SELECT COUNT(*) FROM deadlines WHERE done = 0")?,
            demo: self.is_demo()?,
        })
    }
}

#[cfg(test)]
mod tests;
