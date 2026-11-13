//! assetcraft-core: the command bus shared by CLI, MCP and the desktop app.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod afa;
pub mod label;

use assetcraft_store::{Asset, AssetError, AssetPatch, NewAsset, NewLicense, Registry};
use chrono::{Datelike, Local, NaiveDate};
use craft_biz::at::parse_date;
use craft_biz::bus::Bus;
pub use craft_biz::confine::Confinement;
use craft_biz::mcp::tool;
use craft_biz::Money;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const APP: &str = "assetcraft";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// EKR expense accounts (craft-biz seed): planned AfA and GWG write-off.
pub const ACCOUNT_AFA: i64 = 7000;
pub const ACCOUNT_GWG_EXPENSE: i64 = 7010;
/// EKR asset account for GWG.
pub const ACCOUNT_GWG_ASSET: i64 = 680;
const MAX_IMPORT_BYTES: u64 = 2 * 1024 * 1024;

pub struct Ctx {
    pub reg: Registry,
    pub confinement: Confinement,
    pub bus: Bus,
}

#[derive(Debug, Clone)]
pub enum Command {
    Status,
    Add(NewAsset),
    List {
        category: Option<String>,
        include_disposed: bool,
        text: Option<String>,
    },
    Show {
        tag: String,
        as_of: Option<NaiveDate>,
    },
    Update {
        tag: String,
        patch: AssetPatch,
    },
    Dispose {
        tag: String,
        on: NaiveDate,
        proceeds_cents: i64,
        note: String,
    },
    AfaSchedule {
        tag: String,
    },
    AfaYear {
        year: i32,
        emit: bool,
    },
    MaintenanceAdd {
        tag: String,
        date: NaiveDate,
        kind: String,
        cost_cents: i64,
        note: String,
    },
    Categories,
    CategorySet {
        name: String,
        life_years: i64,
        min_years: i64,
        account: i64,
    },
    GwgLimit {
        set_cents: Option<i64>,
    },
    LicenseAdd(NewLicense),
    LicenseList,
    LicenseAssign {
        license: String,
        assignee: String,
        force: bool,
    },
    LicenseUnassign {
        license: String,
        assignee: String,
    },
    LicenseSeats {
        license: String,
        seats: i64,
    },
    LicenseAudit {
        as_of: Option<NaiveDate>,
    },
    DeadlineAdd {
        name: String,
        kind: String,
        due_on: NaiveDate,
        tag: Option<String>,
        notes: String,
    },
    DeadlineDone {
        id: i64,
    },
    Expiries {
        within_days: i64,
        as_of: Option<NaiveDate>,
    },
    Label {
        tag: String,
        path: String,
    },
    ImportCsv {
        path: String,
        dry_run: bool,
    },
    DrainInbox,
    DemoSeed,
}

// ---------- argument helpers ----------

fn s<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string argument {k:?}"))
}
fn opt_s(a: &Value, k: &str) -> Option<String> {
    a.get(k)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|v| !v.trim().is_empty())
}
fn e(err: AssetError) -> String {
    err.to_string()
}
fn date(v: &str) -> Result<NaiveDate, String> {
    parse_date(v).map_err(|x| x.to_string())
}
fn opt_date(a: &Value, k: &str) -> Result<Option<NaiveDate>, String> {
    opt_s(a, k).map(|d| date(&d)).transpose()
}
fn int(a: &Value, k: &str) -> Result<Option<i64>, String> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_i64()
            .map(Some)
            .ok_or_else(|| format!("{k} must be an integer")),
    }
}
fn flag(a: &Value, k: &str) -> bool {
    a.get(k).and_then(Value::as_bool).unwrap_or(false)
}
fn today() -> NaiveDate {
    Local::now().date_naive()
}
/// Amounts are DE strings ("1.249,00"); floats are refused.
fn money(v: &Value, k: &str) -> Result<i64, String> {
    match v {
        Value::String(t) => Money::parse_de(t)
            .map(Money::cents)
            .map_err(|x| format!("{k}: {x}")),
        _ => Err(format!(
            "{k} must be a DE amount string like \"1.249,00\" (no floats)"
        )),
    }
}
fn opt_money(a: &Value, k: &str) -> Result<Option<i64>, String> {
    a.get(k)
        .filter(|v| !v.is_null())
        .map(|v| money(v, k))
        .transpose()
}
fn eur(c: i64) -> String {
    Money::from_cents(c).format_de()
}
/// Absent -> None; null -> Some(None); value -> Some(Some(v)).
fn tri<T>(
    a: &Value,
    k: &str,
    f: impl Fn(&Value) -> Result<T, String>,
) -> Result<Option<Option<T>>, String> {
    match a.get(k) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => f(v).map(|x| Some(Some(x))),
    }
}
fn str_of(v: &Value) -> Result<String, String> {
    v.as_str()
        .map(str::to_string)
        .ok_or_else(|| "expected a string".into())
}

impl Command {
    pub fn from_tool(name: &str, a: &Value) -> Result<Command, String> {
        let name = craft_biz::mcp::normalize_tool_name(name);
        Ok(match name.as_str() {
            "asset_status" => Command::Status,
            "asset_add" => Command::Add(NewAsset {
                tag: s(a, "tag")?.into(),
                name: s(a, "name")?.into(),
                category: opt_s(a, "category").unwrap_or_else(|| "other".into()),
                serial: opt_s(a, "serial"),
                location: opt_s(a, "location"),
                holder: opt_s(a, "holder"),
                purchase_date: opt_date(a, "purchase_date")?,
                in_service: date(s(a, "in_service")?)?,
                cost_cents: money(a.get("cost").ok_or("missing cost")?, "cost")?,
                life_years: int(a, "life_years")?,
                gwg: a.get("gwg").and_then(Value::as_bool),
                supplier: opt_s(a, "supplier"),
                invoice_ref: opt_s(a, "invoice_ref"),
                warranty_until: opt_date(a, "warranty_until")?,
                notes: opt_s(a, "notes").unwrap_or_default(),
            }),
            "asset_list" => Command::List {
                category: opt_s(a, "category"),
                include_disposed: flag(a, "include_disposed"),
                text: opt_s(a, "text"),
            },
            "asset_show" => Command::Show {
                tag: s(a, "tag")?.into(),
                as_of: opt_date(a, "as_of")?,
            },
            "asset_update" => Command::Update {
                tag: s(a, "tag")?.into(),
                patch: AssetPatch {
                    name: opt_s(a, "name"),
                    serial: tri(a, "serial", str_of)?,
                    location: tri(a, "location", str_of)?,
                    holder: tri(a, "holder", str_of)?,
                    warranty_until: tri(a, "warranty_until", |v| str_of(v).and_then(|t| date(&t)))?,
                    notes: a.get("notes").and_then(Value::as_str).map(str::to_string),
                    cost_cents: opt_money(a, "cost")?,
                    in_service: opt_date(a, "in_service")?,
                    life_years: int(a, "life_years")?,
                    gwg: a.get("gwg").and_then(Value::as_bool),
                },
            },
            "asset_dispose" => Command::Dispose {
                tag: s(a, "tag")?.into(),
                on: date(s(a, "date")?)?,
                proceeds_cents: opt_money(a, "proceeds")?.unwrap_or(0),
                note: opt_s(a, "note").unwrap_or_default(),
            },
            "asset_afa_schedule" => Command::AfaSchedule {
                tag: s(a, "tag")?.into(),
            },
            "asset_afa_year" => Command::AfaYear {
                year: int(a, "year")?
                    .map(|y| i32::try_from(y).map_err(|_| "year out of range".to_string()))
                    .transpose()?
                    .unwrap_or_else(|| today().year()),
                emit: flag(a, "emit"),
            },
            "asset_maintenance_add" => Command::MaintenanceAdd {
                tag: s(a, "tag")?.into(),
                date: opt_date(a, "date")?.unwrap_or_else(today),
                kind: s(a, "kind")?.into(),
                cost_cents: opt_money(a, "cost")?.unwrap_or(0),
                note: opt_s(a, "note").unwrap_or_default(),
            },
            "asset_category_list" => Command::Categories,
            "asset_category_set" => Command::CategorySet {
                name: s(a, "name")?.into(),
                life_years: int(a, "life_years")?.ok_or("missing life_years")?,
                min_years: int(a, "min_years")?.unwrap_or(0),
                account: int(a, "account")?.ok_or("missing account")?,
            },
            "asset_gwg_limit" => Command::GwgLimit {
                set_cents: opt_money(a, "set")?,
            },
            "asset_license_add" => Command::LicenseAdd(NewLicense {
                name: s(a, "name")?.into(),
                vendor: opt_s(a, "vendor"),
                seats: int(a, "seats")?.ok_or("missing seats")?,
                annual_cost_cents: opt_money(a, "annual_cost")?,
                renews_on: opt_date(a, "renews_on")?,
                key_location: opt_s(a, "key_location"),
                notes: opt_s(a, "notes").unwrap_or_default(),
            }),
            "asset_license_list" => Command::LicenseList,
            "asset_license_assign" => Command::LicenseAssign {
                license: s(a, "license")?.into(),
                assignee: s(a, "assignee")?.into(),
                force: flag(a, "force"),
            },
            "asset_license_unassign" => Command::LicenseUnassign {
                license: s(a, "license")?.into(),
                assignee: s(a, "assignee")?.into(),
            },
            "asset_license_seats" => Command::LicenseSeats {
                license: s(a, "license")?.into(),
                seats: int(a, "seats")?.ok_or("missing seats")?,
            },
            "asset_license_audit" => Command::LicenseAudit {
                as_of: opt_date(a, "as_of")?,
            },
            "asset_deadline_add" => Command::DeadlineAdd {
                name: s(a, "name")?.into(),
                kind: opt_s(a, "kind").unwrap_or_else(|| "other".into()),
                due_on: date(s(a, "due_on")?)?,
                tag: opt_s(a, "tag"),
                notes: opt_s(a, "notes").unwrap_or_default(),
            },
            "asset_deadline_done" => Command::DeadlineDone {
                id: int(a, "id")?.ok_or("missing id")?,
            },
            "asset_expiries" => Command::Expiries {
                within_days: int(a, "within_days")?.unwrap_or(60).clamp(0, 3650),
                as_of: opt_date(a, "as_of")?,
            },
            "asset_label" => Command::Label {
                tag: s(a, "tag")?.into(),
                path: s(a, "path")?.into(),
            },
            "asset_import_csv" => Command::ImportCsv {
                path: s(a, "path")?.into(),
                dry_run: flag(a, "dry_run"),
            },
            "asset_drain_inbox" => Command::DrainInbox,
            "asset_demo_seed" => Command::DemoSeed,
            other => return Err(format!("unknown tool {other}")),
        })
    }
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|x| x.to_string())
}

pub fn basis(a: &Asset) -> afa::Basis {
    afa::Basis {
        cost_cents: a.cost_cents,
        in_service: a.in_service,
        life_years: a.life_years,
        gwg: a.gwg,
        disposed_on: a.disposed_on,
    }
}

fn asset_json(reg: &Registry, a: &Asset, as_of: NaiveDate) -> Result<Value, String> {
    let book = afa::book_value_end_of(&basis(a), as_of.year() - 1)?;
    let mut v = to_json(a)?;
    v["cost"] = json!(eur(a.cost_cents));
    v["book_value_start_of_year"] = json!(eur(book));
    v["account"] = json!(accounts(reg, a)?.1);
    Ok(v)
}

/// (debit expense account, credit asset account) for the AfA entry.
pub fn accounts(reg: &Registry, a: &Asset) -> Result<(i64, i64), String> {
    if a.gwg {
        return Ok((ACCOUNT_GWG_EXPENSE, ACCOUNT_GWG_ASSET));
    }
    Ok((ACCOUNT_AFA, reg.category(&a.category).map_err(e)?.account))
}

fn schedule_json(s: &afa::Schedule) -> Value {
    let rows: Vec<Value> = s
        .rows
        .iter()
        .map(|r| json!({ "year": r.year, "kind": r.kind, "opening": eur(r.opening_cents), "amount": eur(r.amount_cents), "closing": eur(r.closing_cents), "amount_cents": r.amount_cents }))
        .collect();
    json!({ "rows": rows, "residual_at_disposal": s.residual_at_disposal_cents.map(eur) })
}

pub fn dispatch(ctx: &mut Ctx, cmd: Command) -> Result<Value, String> {
    let r = &ctx.reg;
    match cmd {
        Command::Status => Ok(
            json!({ "app": APP, "version": VERSION, "stats": to_json(&r.stats().map_err(e)?)?, "gwg_limit": eur(r.gwg_limit().map_err(e)?) }),
        ),
        Command::Add(n) => {
            let a = r.add_asset(&n).map_err(e)?;
            let mut v = asset_json(r, &a, today())?;
            v["schedule"] = schedule_json(&afa::schedule(&basis(&a))?);
            Ok(v)
        }
        Command::List {
            category,
            include_disposed,
            text,
        } => {
            let t = today();
            let list = r
                .assets(category.as_deref(), include_disposed, text.as_deref())
                .map_err(e)?;
            let rows = list
                .iter()
                .map(|a| asset_json(r, a, t))
                .collect::<Result<Vec<_>, _>>()?;
            let total: i64 = list.iter().map(|a| a.cost_cents).sum();
            Ok(json!({ "count": rows.len(), "total_cost": eur(total), "assets": rows }))
        }
        Command::Show { tag, as_of } => {
            let a = r.asset(&tag).map_err(e)?;
            let mut v = asset_json(r, &a, as_of.unwrap_or_else(today))?;
            v["category_detail"] = to_json(&r.category(&a.category).map_err(e)?)?;
            v["schedule"] = schedule_json(&afa::schedule(&basis(&a))?);
            v["postings"] = to_json(&r.postings(&a.tag).map_err(e)?)?;
            v["maintenance"] = to_json(&r.maintenance(&a.tag).map_err(e)?)?;
            v["deadlines"] = to_json(
                &r.deadlines(true)
                    .map_err(e)?
                    .into_iter()
                    .filter(|d| d.tag.as_deref() == Some(a.tag.as_str()))
                    .collect::<Vec<_>>(),
            )?;
            Ok(v)
        }
        Command::Update { tag, patch } => {
            let a = r.update_asset(&tag, &patch).map_err(e)?;
            asset_json(r, &a, today())
        }
        Command::Dispose {
            tag,
            on,
            proceeds_cents,
            note,
        } => {
            let a = r.dispose(&tag, on, proceeds_cents, &note).map_err(e)?;
            let sched = afa::schedule(&basis(&a))?;
            let residual = sched.residual_at_disposal_cents.unwrap_or(0);
            let disposal_year_afa = sched
                .rows
                .iter()
                .find(|x| x.year == on.year())
                .map_or(0, |x| x.amount_cents);
            Ok(json!({
                "tag": a.tag, "disposed_on": on, "proceeds": eur(proceeds_cents),
                "afa_in_disposal_year": eur(disposal_year_afa),
                "book_value_at_disposal": eur(residual),
                "gain_or_loss": eur(proceeds_cents - residual),
                "hint": "AfA of the disposal year comes through asset_afa_year; book the residual value (Anlagenabgang) and the proceeds in buchcraft.",
            }))
        }
        Command::AfaSchedule { tag } => {
            let a = r.asset(&tag).map_err(e)?;
            let mut v = schedule_json(&afa::schedule(&basis(&a))?);
            v["tag"] = json!(a.tag);
            v["cost"] = json!(eur(a.cost_cents));
            v["life_years"] = json!(a.life_years);
            v["gwg"] = json!(a.gwg);
            v["in_service"] = json!(a.in_service);
            Ok(v)
        }
        Command::AfaYear { year, emit } => afa_year(ctx, year, emit, today()),
        Command::MaintenanceAdd {
            tag,
            date,
            kind,
            cost_cents,
            note,
        } => to_json(
            &r.add_maintenance(&tag, date, &kind, cost_cents, &note)
                .map_err(e)?,
        ),
        Command::Categories => Ok(json!({ "categories": to_json(&r.categories().map_err(e)?)? })),
        Command::CategorySet {
            name,
            life_years,
            min_years,
            account,
        } => to_json(
            &r.set_category(&name, life_years, min_years, account)
                .map_err(e)?,
        ),
        Command::GwgLimit { set_cents } => {
            if let Some(c) = set_cents {
                r.set_gwg_limit(c).map_err(e)?;
            }
            Ok(
                json!({ "gwg_limit": eur(r.gwg_limit().map_err(e)?), "note": "applies to assets added from now on" }),
            )
        }
        Command::LicenseAdd(n) => to_json(&r.add_license(&n).map_err(e)?),
        Command::LicenseList => Ok(json!({ "licenses": to_json(&r.licenses().map_err(e)?)? })),
        Command::LicenseAssign {
            license,
            assignee,
            force,
        } => to_json(&r.assign(&license, &assignee, today(), force).map_err(e)?),
        Command::LicenseUnassign { license, assignee } => {
            to_json(&r.unassign(&license, &assignee).map_err(e)?)
        }
        Command::LicenseSeats { license, seats } => {
            to_json(&r.set_seats(&license, seats).map_err(e)?)
        }
        Command::LicenseAudit { as_of } => license_audit(r, as_of.unwrap_or_else(today)),
        Command::DeadlineAdd {
            name,
            kind,
            due_on,
            tag,
            notes,
        } => to_json(
            &r.add_deadline(&name, &kind, due_on, tag.as_deref(), &notes)
                .map_err(e)?,
        ),
        Command::DeadlineDone { id } => to_json(&r.complete_deadline(id).map_err(e)?),
        Command::Expiries { within_days, as_of } => {
            expiries(r, within_days, as_of.unwrap_or_else(today))
        }
        Command::Label { tag, path } => {
            let a = r.asset(&tag).map_err(e)?;
            let file = ctx.confinement.check(&path, true)?;
            let svg = label::svg(&a)?;
            std::fs::write(&file, &svg).map_err(|x| format!("{}: {x}", file.display()))?;
            Ok(
                json!({ "tag": a.tag, "written": file.display().to_string(), "payload": label::payload(&a), "bytes": svg.len() }),
            )
        }
        Command::ImportCsv { path, dry_run } => import_csv(ctx, &path, dry_run),
        Command::DrainInbox => drain_inbox(ctx),
        Command::DemoSeed => demo_seed(&ctx.reg),
    }
}

/// AfA for one year across all assets. With `emit`, every not-yet-posted line is sent
/// to buchcraft as `asset.depreciation_due` (a draft there) and recorded here, so a
/// second run never sends it twice.
pub fn afa_year(ctx: &mut Ctx, year: i32, emit: bool, today: NaiveDate) -> Result<Value, String> {
    if emit && year > today.year() {
        return Err(format!(
            "{year} has not started; AfA is posted for the current or a past year"
        ));
    }
    let mut lines = Vec::new();
    let mut by_account: BTreeMap<(i64, i64), i64> = BTreeMap::new();
    let (mut total, mut emitted, mut already) = (0i64, 0usize, 0usize);
    for a in ctx.reg.assets(None, true, None).map_err(e)? {
        let (amount, kind) = afa::amount_for_year(&basis(&a), year)?;
        if amount == 0 {
            continue;
        }
        let (debit, credit) = accounts(&ctx.reg, &a)?;
        total += amount;
        *by_account.entry((debit, credit)).or_default() += amount;
        let mut posted = ctx.reg.posted(a.id, year).map_err(e)?.map(|p| p.event_id);
        if posted.is_some() {
            already += 1;
        } else if emit {
            let date = a
                .disposed_on
                .filter(|d| d.year() == year)
                .unwrap_or(NaiveDate::from_ymd_opt(year, 12, 31).ok_or("bad year")?);
            let payload = json!({ "asset_tag": a.tag, "year": year, "amount_cents": amount, "debit": debit, "credit": credit, "date": date.to_string(), "kind": kind, "name": a.name });
            let ev = ctx
                .bus
                .emit(APP, "buchcraft", "asset.depreciation_due", payload)
                .map_err(|x| x.to_string())?;
            ctx.reg
                .record_posting(a.id, year, amount, &ev.id)
                .map_err(e)?;
            emitted += 1;
            posted = Some(ev.id);
        }
        lines.push(json!({ "tag": a.tag, "name": a.name, "category": a.category, "kind": kind, "amount": eur(amount), "amount_cents": amount, "debit": debit, "credit": credit, "posted": posted }));
    }
    let totals: Vec<Value> = by_account
        .iter()
        .map(|((d, c), amt)| json!({ "debit": d, "credit": c, "amount": eur(*amt) }))
        .collect();
    Ok(
        json!({ "year": year, "total": eur(total), "lines": lines.len(), "emitted": emitted, "already_posted": already, "by_account": totals, "rows": lines }),
    )
}

fn days_between(from: NaiveDate, to: NaiveDate) -> i64 {
    (to - from).num_days()
}

pub fn license_audit(r: &Registry, as_of: NaiveDate) -> Result<Value, String> {
    let mut rows = Vec::new();
    let (mut annual, mut flagged) = (0i64, 0usize);
    for l in r.licenses().map_err(e)? {
        let used = l.assigned.len() as i64;
        let mut flags = Vec::new();
        if used > l.seats {
            flags.push("over_allocated");
        }
        if used < l.seats {
            flags.push("unused_seats");
        }
        let days = l.renews_on.map(|d| days_between(as_of, d));
        match days {
            Some(x) if x < 0 => flags.push("renewal_overdue"),
            Some(x) if x <= 60 => flags.push("renewal_due"),
            _ => {}
        }
        if flags.iter().any(|f| *f != "unused_seats") {
            flagged += 1;
        }
        annual += l.annual_cost_cents.unwrap_or(0);
        rows.push(json!({
            "name": l.name, "vendor": l.vendor, "seats": l.seats, "used": used, "free": l.seats - used,
            "renews_on": l.renews_on, "days_to_renewal": days,
            "annual_cost": l.annual_cost_cents.map(eur),
            "cost_per_used_seat": l.annual_cost_cents.filter(|_| used > 0).map(|c| eur(c / used)),
            "flags": flags,
        }));
    }
    Ok(
        json!({ "as_of": as_of, "licenses": rows.len(), "needs_attention": flagged, "annual_cost": eur(annual), "rows": rows }),
    )
}

pub fn expiries(r: &Registry, within_days: i64, as_of: NaiveDate) -> Result<Value, String> {
    let mut items: Vec<(NaiveDate, Value)> = Vec::new();
    let mut push = |date: NaiveDate, kind: &str, name: String, reference: String| {
        let days = days_between(as_of, date);
        if days <= within_days {
            items.push((date, json!({ "date": date, "days_left": days, "overdue": days < 0, "kind": kind, "name": name, "ref": reference })));
        }
    };
    for a in r.assets(None, false, None).map_err(e)? {
        if let Some(w) = a.warranty_until {
            push(w, "warranty", a.name.clone(), a.tag.clone());
        }
    }
    for l in r.licenses().map_err(e)? {
        if let Some(d) = l.renews_on {
            push(
                d,
                "licence_renewal",
                l.name.clone(),
                format!("{} seats", l.seats),
            );
        }
    }
    for d in r.deadlines(false).map_err(e)? {
        push(
            d.due_on,
            &d.kind,
            d.name.clone(),
            d.tag
                .clone()
                .unwrap_or_else(|| format!("deadline {}", d.id)),
        );
    }
    items.sort_by_key(|(d, _)| *d);
    let overdue = items.iter().filter(|(d, _)| *d < as_of).count();
    Ok(
        json!({ "as_of": as_of, "within_days": within_days, "overdue": overdue, "rows": items.into_iter().map(|(_, v)| v).collect::<Vec<_>>() }),
    )
}

/// `tag;name;category;in_service;cost;serial;location;holder` (header optional, `;`, DE amounts).
pub fn parse_asset_csv(text: &str) -> Result<Vec<NewAsset>, String> {
    let mut out = Vec::new();
    for (i, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with('#')
            || (i == 0 && line.to_ascii_lowercase().starts_with("tag;"))
        {
            continue;
        }
        let f: Vec<String> = line
            .split(';')
            .map(|x| x.trim().trim_matches('"').trim().to_string())
            .collect();
        let at = |m: String| format!("line {}: {m}", i + 1);
        if f.len() < 5 || f.len() > 8 {
            return Err(at(format!("expected 5-8 fields tag;name;category;in_service;cost[;serial;location;holder], got {}", f.len())));
        }
        let opt = |k: usize| f.get(k).cloned().filter(|x| !x.is_empty());
        out.push(NewAsset {
            tag: f[0].clone(),
            name: f[1].clone(),
            category: if f[2].is_empty() {
                "other".into()
            } else {
                f[2].clone()
            },
            in_service: date(&f[3]).map_err(at)?,
            cost_cents: Money::parse_de(&f[4])
                .map(Money::cents)
                .map_err(|x| at(x.to_string()))?,
            serial: opt(5),
            location: opt(6),
            holder: opt(7),
            ..Default::default()
        });
    }
    if out.is_empty() {
        return Err("no asset rows found".into());
    }
    Ok(out)
}

/// All-or-nothing import: one bad row and nothing is stored.
fn import_csv(ctx: &mut Ctx, path: &str, dry_run: bool) -> Result<Value, String> {
    let p = ctx.confinement.check(path, false)?;
    let size = std::fs::metadata(&p)
        .map_err(|x| format!("{}: {x}", p.display()))?
        .len();
    if size > MAX_IMPORT_BYTES {
        return Err("import file larger than 2 MB".into());
    }
    let text = std::fs::read_to_string(&p).map_err(|x| format!("{}: {x}", p.display()))?;
    let rows = parse_asset_csv(&text)?;
    let tx = ctx
        .reg
        .conn()
        .unchecked_transaction()
        .map_err(|x| x.to_string())?;
    let mut tags = Vec::new();
    for (n, row) in rows.iter().enumerate() {
        let a = ctx
            .reg
            .add_asset(row)
            .map_err(|x| format!("row {} ({}): {x}", n + 1, row.tag))?;
        tags.push(a.tag);
    }
    if dry_run {
        drop(tx);
    } else {
        tx.commit().map_err(|x| x.to_string())?;
    }
    Ok(
        json!({ "imported": if dry_run { 0 } else { tags.len() }, "valid_rows": tags.len(), "dry_run": dry_run, "tags": tags }),
    )
}

/// Labelled demo inventory. Only on an empty register.
pub fn demo_seed(r: &Registry) -> Result<Value, String> {
    if !r.assets(None, true, None).map_err(e)?.is_empty() || !r.licenses().map_err(e)?.is_empty() {
        return Err("demo seed only runs on an empty register".into());
    }
    let dd = |y: i32, m: u32, d: u32| {
        NaiveDate::from_ymd_opt(y, m, d).ok_or_else(|| "bad demo date".to_string())
    };
    r.set_meta("demo", "1").map_err(e)?;
    let assets = [
        (
            "GOL-001",
            "Workstation 24-core, RTX 4090 (DEMO)",
            "it",
            dd(2025, 2, 14)?,
            389_000,
            Some(dd(2028, 2, 14)?),
            Some("Joe Mocky"),
        ),
        (
            "NAS-001",
            "NAS 4-bay with 4x8 TB (DEMO)",
            "it",
            dd(2026, 8, 3)?,
            162_000,
            Some(dd(2029, 8, 3)?),
            None,
        ),
        (
            "LAP-002",
            "ThinkPad T14 (DEMO)",
            "it",
            dd(2026, 3, 2)?,
            149_900,
            Some(dd(2026, 12, 15)?),
            Some("Hannes Mockinger"),
        ),
        (
            "MON-001",
            "27in monitor (DEMO)",
            "it",
            dd(2026, 11, 2)?,
            34_900,
            None,
            Some("Hannes Mockinger"),
        ),
        (
            "CHR-001",
            "Office chair (DEMO)",
            "furniture",
            dd(2024, 1, 15)?,
            115_000,
            None,
            Some("Joe Mocky"),
        ),
        (
            "CAR-001",
            "Skoda Octavia Combi (DEMO)",
            "car",
            dd(2024, 5, 6)?,
            3_200_000,
            None,
            None,
        ),
        (
            "LAP-001",
            "Old laptop (DEMO)",
            "it",
            dd(2023, 9, 20)?,
            129_000,
            None,
            None,
        ),
    ];
    for (tag, name, cat, in_service, cost, warranty, holder) in assets {
        r.add_asset(&NewAsset {
            tag: tag.into(),
            name: name.into(),
            category: cat.into(),
            in_service,
            purchase_date: Some(in_service),
            cost_cents: cost,
            warranty_until: warranty,
            holder: holder.map(str::to_string),
            location: Some("Wien 9 office (DEMO)".into()),
            supplier: Some("Mock Elektronik GmbH".into()),
            ..Default::default()
        })
        .map_err(e)?;
    }
    r.dispose(
        "LAP-001",
        dd(2026, 4, 30)?,
        15_000,
        "sold to Anna Mockova (DEMO)",
    )
    .map_err(e)?;
    r.add_maintenance(
        "CAR-001",
        dd(2026, 5, 4)?,
        "service",
        38_400,
        "annual service (DEMO)",
    )
    .map_err(e)?;
    r.add_maintenance(
        "GOL-001",
        dd(2026, 7, 1)?,
        "repair",
        12_900,
        "PSU fan replaced (DEMO)",
    )
    .map_err(e)?;
    let lic = |name: &str, seats: i64, cost: i64, renews: NaiveDate, key: &str| NewLicense {
        name: name.into(),
        vendor: Some("Mock Software (DEMO)".into()),
        seats,
        annual_cost_cents: Some(cost),
        renews_on: Some(renews),
        key_location: Some(key.into()),
        notes: String::new(),
    };
    r.add_license(&lic(
        "Office suite (DEMO)",
        5,
        66_000,
        dd(2027, 3, 1)?,
        "Bitwarden: office admin",
    ))
    .map_err(e)?;
    r.add_license(&lic(
        "Design suite (DEMO)",
        2,
        79_000,
        dd(2026, 12, 10)?,
        "Bitwarden: design",
    ))
    .map_err(e)?;
    r.add_license(&lic(
        "IDE (DEMO)",
        1,
        29_900,
        dd(2026, 11, 20)?,
        "vendor portal",
    ))
    .map_err(e)?;
    for who in [
        "Joe Mocky",
        "Hannes Mockinger",
        "Anna Mockova",
        "Mia Mockerl",
    ] {
        r.assign("Office suite (DEMO)", who, dd(2026, 1, 5)?, false)
            .map_err(e)?;
    }
    r.assign("Design suite (DEMO)", "Joe Mocky", dd(2026, 1, 5)?, false)
        .map_err(e)?;
    r.assign(
        "Design suite (DEMO)",
        "Anna Mockova",
        dd(2026, 1, 5)?,
        false,
    )
    .map_err(e)?;
    r.assign(
        "Design suite (DEMO)",
        "Hannes Mockinger",
        dd(2026, 6, 1)?,
        true,
    )
    .map_err(e)?;
    r.add_deadline(
        "TLS certificate mockfirma.at (DEMO)",
        "certificate",
        dd(2026, 11, 28)?,
        None,
        "",
    )
    .map_err(e)?;
    r.add_deadline(
        "Domain mockfirma.at (DEMO)",
        "domain",
        dd(2027, 2, 1)?,
        None,
        "",
    )
    .map_err(e)?;
    r.add_deadline(
        "Pickerl (DEMO)",
        "inspection",
        dd(2026, 11, 5)?,
        Some("CAR-001"),
        "",
    )
    .map_err(e)?;
    r.add_deadline(
        "Car insurance (DEMO)",
        "insurance",
        dd(2027, 5, 6)?,
        Some("CAR-001"),
        "",
    )
    .map_err(e)?;
    Ok(json!({ "demo": true, "stats": to_json(&r.stats().map_err(e)?)? }))
}

/// Records one open equipment reminder from each `candidate.hired` event.
///
/// No asset is created. A purchase price is not in the event, and inventing
/// one would enter the depreciation book. A second event for the same
/// candidate is acknowledged and does not add another reminder.
pub fn drain_inbox(ctx: &mut Ctx) -> Result<Value, String> {
    let events = ctx.bus.pending(APP).map_err(|err| err.to_string())?;
    let processed = events.len();
    let mut created = Vec::new();
    let mut rejected = 0usize;
    for ev in events {
        let outcome = if ev.kind == "candidate.hired" {
            apply_hired(ctx, &ev.payload)
        } else {
            Ok(None)
        };
        match outcome {
            Ok(Some(id)) => {
                created.push(id);
                ctx.bus.ack(APP, &ev.id).map_err(|err| err.to_string())?;
            }
            Ok(None) => ctx.bus.ack(APP, &ev.id).map_err(|err| err.to_string())?,
            Err(err) => {
                rejected += 1;
                ctx.bus
                    .reject(APP, &ev.id, &err)
                    .map_err(|bus_err| bus_err.to_string())?;
            }
        }
    }
    Ok(json!({
        "processed": processed,
        "created": created,
        "rejected": rejected,
    }))
}

fn apply_hired(ctx: &mut Ctx, payload: &Value) -> Result<Option<i64>, String> {
    let candidate_id = required_text(payload, "candidate_id")?;
    let name = required_text(payload, "name")?;
    let marker = format!("hire:{candidate_id}");
    let existing = ctx.reg.deadlines(true).map_err(e)?;
    if existing
        .iter()
        .any(|deadline| deadline.notes.contains(&marker))
    {
        return Ok(None);
    }
    let deadline = ctx
        .reg
        .add_deadline(
            &format!("Equipment for {name}"),
            "other",
            today(),
            None,
            &format!("{marker}; no asset chosen; cost unknown"),
        )
        .map_err(e)?;
    Ok(Some(deadline.id))
}

fn required_text<'a>(payload: &'a Value, key: &str) -> Result<&'a str, String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

/// MCP tool catalog.
pub fn tools() -> Vec<Value> {
    let tag = json!({ "type": "string", "description": "Asset tag, e.g. GOL-001" });
    let date = json!({ "type": "string", "description": "DD.MM.YYYY or YYYY-MM-DD" });
    let money = json!({ "type": "string", "description": "DE amount, e.g. \"1.249,00\"" });
    let nullable = json!({ "type": ["string", "null"] });
    vec![
        tool("asset_status", "Counts, cost of active assets, licence seats, open deadlines, GWG limit.", json!({}), &[]),
        tool("asset_add", "Register an asset. cost is the AfA base (net if VAT is deductible). GWG is automatic at or below the limit unless gwg=false. Life defaults from the category; cars need at least 8 years.", json!({ "tag": tag, "name": {"type":"string"}, "category": {"type":"string","description":"it, car, office, furniture, machine, other (or your own)"}, "in_service": date, "cost": money, "life_years": {"type":"integer"}, "gwg": {"type":"boolean"}, "purchase_date": date, "serial": {"type":"string"}, "location": {"type":"string"}, "holder": {"type":"string"}, "supplier": {"type":"string"}, "invoice_ref": {"type":"string"}, "warranty_until": date, "notes": {"type":"string"} }), &["tag", "name", "in_service", "cost"]),
        tool("asset_list", "List assets with book value at the start of this year.", json!({ "category": {"type":"string"}, "include_disposed": {"type":"boolean"}, "text": {"type":"string","description":"search name, tag, serial, holder, location"} }), &[]),
        tool("asset_show", "One asset with AfA schedule, postings to buchcraft, maintenance and deadlines.", json!({ "tag": tag, "as_of": date }), &["tag"]),
        tool("asset_update", "Change fields; null clears serial/location/holder/warranty_until. cost/in_service/life_years/gwg are frozen once AfA was posted.", json!({ "tag": tag, "name": {"type":"string"}, "serial": nullable, "location": nullable, "holder": nullable, "warranty_until": nullable, "notes": {"type":"string"}, "cost": money, "in_service": date, "life_years": {"type":"integer"}, "gwg": {"type":"boolean"} }), &["tag"]),
        tool("asset_dispose", "Retire an asset (sold, scrapped). Returns disposal-year AfA, book value and gain/loss. The asset becomes read-only.", json!({ "tag": tag, "date": date, "proceeds": money, "note": {"type":"string"} }), &["tag", "date"]),
        tool("asset_afa_schedule", "Straight-line AfA plan in cents with Halbjahres-AfA and GWG.", json!({ "tag": tag }), &["tag"]),
        tool("asset_afa_year", "AfA for all assets in a year with totals per account. emit=true sends asset.depreciation_due to buchcraft (drafts there, approved by a human); each asset/year is sent once.", json!({ "year": {"type":"integer"}, "emit": {"type":"boolean"} }), &[]),
        tool("asset_maintenance_add", "Log a repair, service or inspection with cost.", json!({ "tag": tag, "date": date, "kind": {"type":"string"}, "cost": money, "note": {"type":"string"} }), &["tag", "kind"]),
        tool("asset_category_list", "Categories with default life, minimum life and EKR asset account.", json!({}), &[]),
        tool("asset_category_set", "Create or change a category (life 1-50, minimum life, EKR account).", json!({ "name": {"type":"string"}, "life_years": {"type":"integer"}, "min_years": {"type":"integer"}, "account": {"type":"integer"} }), &["name", "life_years", "account"]),
        tool("asset_gwg_limit", "Show or set the GWG limit (default 1.000,00 since 2023).", json!({ "set": money }), &[]),
        tool("asset_license_add", "Register a software licence. Never pass the key itself: key_location says where it is stored.", json!({ "name": {"type":"string"}, "vendor": {"type":"string"}, "seats": {"type":"integer"}, "annual_cost": money, "renews_on": date, "key_location": {"type":"string"}, "notes": {"type":"string"} }), &["name", "seats"]),
        tool("asset_license_list", "Licences with assigned seats.", json!({}), &[]),
        tool("asset_license_assign", "Give a person a seat. Refused when full unless force=true.", json!({ "license": {"type":"string"}, "assignee": {"type":"string"}, "force": {"type":"boolean"} }), &["license", "assignee"]),
        tool("asset_license_unassign", "Free a seat.", json!({ "license": {"type":"string"}, "assignee": {"type":"string"} }), &["license", "assignee"]),
        tool("asset_license_seats", "Change the number of seats.", json!({ "license": {"type":"string"}, "seats": {"type":"integer"} }), &["license", "seats"]),
        tool("asset_license_audit", "Over-allocated and unused seats, renewals due within 60 days, cost per used seat.", json!({ "as_of": date }), &[]),
        tool("asset_deadline_add", "Track a certificate, domain, insurance, inspection or contract date, optionally tied to an asset.", json!({ "name": {"type":"string"}, "kind": {"type":"string","enum":["certificate","domain","insurance","inspection","contract","other"]}, "due_on": date, "tag": tag, "notes": {"type":"string"} }), &["name", "due_on"]),
        tool("asset_deadline_done", "Mark a deadline handled.", json!({ "id": {"type":"integer"} }), &["id"]),
        tool("asset_expiries", "Warranties, licence renewals and deadlines due within N days (default 60), overdue first.", json!({ "within_days": {"type":"integer"}, "as_of": date }), &[]),
        tool("asset_label", "Write a printable SVG label with a QR code (ASSET:<tag>) for an asset.", json!({ "tag": tag, "path": {"type":"string"} }), &["tag", "path"]),
        tool("asset_import_csv", "Import assets from CSV tag;name;category;in_service;cost[;serial;location;holder]. All-or-nothing; dry_run validates only.", json!({ "path": {"type":"string"}, "dry_run": {"type":"boolean"} }), &["path"]),
        tool("asset_drain_inbox", "Record one equipment reminder per hirecraft candidate.hired event. No asset and no cost are invented. A repeated candidate is skipped. A payload without candidate_id or name is rejected.", json!({}), &[]),
        tool("asset_demo_seed", "Fill an EMPTY register with a labelled demo inventory, licences and deadlines.", json!({}), &[]),
    ]
}

#[cfg(test)]
mod tests;
