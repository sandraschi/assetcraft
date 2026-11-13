//! `assetcraft`: CLI and MCP stdio server in one binary.
//!
//! Every CLI verb builds the same tool-argument JSON an MCP client would send
//! and goes through `Command::from_tool` + `dispatch`, so CLI and MCP share one
//! code path. In `mcp` mode stdout carries only JSON-RPC; logs go to stderr.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assetcraft_core::{dispatch, tools, Command, Confinement, Ctx, APP, VERSION};
use assetcraft_store::Registry;
use clap::{Parser, Subcommand};
use craft_biz::bus::Bus;
use craft_biz::mcp::{serve_stdio, ToolHost};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::process::ExitCode;

const AFTER_HELP: &str = "Typical year:
  assetcraft add GOL-002 \"Mini PC\" --category it --in-service 09.11.2026 --cost \"689,00\"
  assetcraft schedule GOL-002
  assetcraft afa 2026                          # preview; --emit sends drafts to buchcraft
  assetcraft expiries --days 60
  assetcraft license audit
  assetcraft dispose LAP-001 --date 30.04.2026 --proceeds \"150,00\"
  assetcraft import D:\\Temp\\inventory.csv --dry-run
See HELP.md for workflows and docs/TOOLS.md for every MCP tool.";

#[derive(Parser)]
#[command(after_help = AFTER_HELP,
    name = "assetcraft",
    version,
    about = "Fixed assets with Austrian AfA, licences and expiry dates. CLI + MCP."
)]
struct Cli {
    /// Database. Default: $ASSETCRAFT_DB or %LOCALAPPDATA%\assetcraft\assetcraft.db
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    /// Print machine-readable JSON only.
    #[arg(long, global = true)]
    json: bool,
    /// Restrict file access to these directories (repeatable). Required for file tools in `mcp` mode.
    #[arg(long = "allow-path", global = true)]
    allow_path: Vec<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Counts and totals.
    Status,
    /// Register an asset. Amounts in DE format ("1.249,00").
    Add {
        tag: String,
        name: String,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        in_service: String,
        #[arg(long)]
        cost: String,
        #[arg(long)]
        life: Option<i64>,
        /// Force normal AfA for an item under the GWG limit.
        #[arg(long)]
        no_gwg: bool,
        #[arg(long)]
        serial: Option<String>,
        #[arg(long)]
        location: Option<String>,
        #[arg(long)]
        holder: Option<String>,
        #[arg(long)]
        warranty: Option<String>,
        #[arg(long)]
        supplier: Option<String>,
        #[arg(long)]
        invoice: Option<String>,
    },
    /// List assets.
    List {
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        text: Option<String>,
    },
    /// One asset with schedule, postings, maintenance.
    Show { tag: String },
    /// Change holder, location, serial, warranty, notes.
    Update {
        tag: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        holder: Option<String>,
        #[arg(long)]
        location: Option<String>,
        #[arg(long)]
        serial: Option<String>,
        #[arg(long)]
        warranty: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// Retire an asset.
    Dispose {
        tag: String,
        #[arg(long)]
        date: String,
        #[arg(long)]
        proceeds: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// AfA plan of one asset.
    Schedule { tag: String },
    /// AfA of all assets for a year; --emit sends drafts to buchcraft.
    Afa {
        year: Option<i32>,
        #[arg(long)]
        emit: bool,
    },
    /// Log maintenance.
    Maintenance {
        tag: String,
        kind: String,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        cost: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Categories (life, minimum life, EKR account).
    Categories,
    /// Create or change a category.
    Category {
        name: String,
        #[arg(long)]
        life: i64,
        #[arg(long, default_value_t = 0)]
        min: i64,
        #[arg(long)]
        account: i64,
    },
    /// Show or set the GWG limit.
    Gwg {
        #[arg(long)]
        set: Option<String>,
    },
    /// Software licences.
    License {
        #[command(subcommand)]
        op: LicenseOp,
    },
    /// Certificates, domains, insurance, inspections.
    Deadline {
        #[command(subcommand)]
        op: DeadlineOp,
    },
    /// Everything due within N days (default 60).
    Expiries {
        #[arg(long, default_value_t = 60)]
        days: i64,
    },
    /// Write a QR label SVG.
    Label { tag: String, path: String },
    /// Import assets from CSV.
    Import {
        path: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Record an equipment reminder from each hirecraft candidate.hired event.
    DrainInbox,
    /// Fill an empty register with the labelled demo inventory.
    DemoSeed,
    /// Print the MCP tool catalog (JSON, or markdown for docs/TOOLS.md).
    Tools {
        #[arg(long)]
        markdown: bool,
    },
    /// Run the MCP stdio server.
    Mcp,
}

#[derive(Subcommand)]
enum LicenseOp {
    List,
    Add {
        name: String,
        #[arg(long)]
        seats: i64,
        #[arg(long)]
        vendor: Option<String>,
        #[arg(long)]
        cost: Option<String>,
        #[arg(long)]
        renews: Option<String>,
        /// Where the key is stored, never the key.
        #[arg(long)]
        key_location: Option<String>,
    },
    Assign {
        license: String,
        assignee: String,
        #[arg(long)]
        force: bool,
    },
    Unassign {
        license: String,
        assignee: String,
    },
    Seats {
        license: String,
        seats: i64,
    },
    Audit,
}

#[derive(Subcommand)]
enum DeadlineOp {
    Add {
        name: String,
        #[arg(long)]
        due: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        tag: Option<String>,
    },
    Done {
        id: i64,
    },
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

fn obj(pairs: Vec<(&str, Option<Value>)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        if let Some(v) = v {
            m.insert(k.to_string(), v);
        }
    }
    Value::Object(m)
}
fn sv(s: &str) -> Option<Value> {
    Some(Value::String(s.to_string()))
}
fn os(s: Option<String>) -> Option<Value> {
    s.map(Value::String)
}

fn to_tool(cmd: Cmd) -> Result<(&'static str, Value), String> {
    Ok(match cmd {
        Cmd::Status => ("asset_status", json!({})),
        Cmd::Add {
            tag,
            name,
            category,
            in_service,
            cost,
            life,
            no_gwg,
            serial,
            location,
            holder,
            warranty,
            supplier,
            invoice,
        } => (
            "asset_add",
            obj(vec![
                ("tag", sv(&tag)),
                ("name", sv(&name)),
                ("category", os(category)),
                ("in_service", sv(&in_service)),
                ("cost", sv(&cost)),
                ("life_years", life.map(|l| json!(l))),
                ("gwg", no_gwg.then_some(json!(false))),
                ("serial", os(serial)),
                ("location", os(location)),
                ("holder", os(holder)),
                ("warranty_until", os(warranty)),
                ("supplier", os(supplier)),
                ("invoice_ref", os(invoice)),
            ]),
        ),
        Cmd::List {
            category,
            all,
            text,
        } => (
            "asset_list",
            obj(vec![
                ("category", os(category)),
                ("include_disposed", Some(json!(all))),
                ("text", os(text)),
            ]),
        ),
        Cmd::Show { tag } => ("asset_show", json!({ "tag": tag })),
        Cmd::Update {
            tag,
            name,
            holder,
            location,
            serial,
            warranty,
            notes,
        } => (
            "asset_update",
            obj(vec![
                ("tag", sv(&tag)),
                ("name", os(name)),
                ("holder", os(holder)),
                ("location", os(location)),
                ("serial", os(serial)),
                ("warranty_until", os(warranty)),
                ("notes", os(notes)),
            ]),
        ),
        Cmd::Dispose {
            tag,
            date,
            proceeds,
            note,
        } => (
            "asset_dispose",
            obj(vec![
                ("tag", sv(&tag)),
                ("date", sv(&date)),
                ("proceeds", os(proceeds)),
                ("note", os(note)),
            ]),
        ),
        Cmd::Schedule { tag } => ("asset_afa_schedule", json!({ "tag": tag })),
        Cmd::Afa { year, emit } => (
            "asset_afa_year",
            obj(vec![
                ("year", year.map(|y| json!(y))),
                ("emit", Some(json!(emit))),
            ]),
        ),
        Cmd::Maintenance {
            tag,
            kind,
            date,
            cost,
            note,
        } => (
            "asset_maintenance_add",
            obj(vec![
                ("tag", sv(&tag)),
                ("kind", sv(&kind)),
                ("date", os(date)),
                ("cost", os(cost)),
                ("note", os(note)),
            ]),
        ),
        Cmd::Categories => ("asset_category_list", json!({})),
        Cmd::Category {
            name,
            life,
            min,
            account,
        } => (
            "asset_category_set",
            json!({ "name": name, "life_years": life, "min_years": min, "account": account }),
        ),
        Cmd::Gwg { set } => ("asset_gwg_limit", obj(vec![("set", os(set))])),
        Cmd::License {
            op: LicenseOp::List,
        } => ("asset_license_list", json!({})),
        Cmd::License {
            op:
                LicenseOp::Add {
                    name,
                    seats,
                    vendor,
                    cost,
                    renews,
                    key_location,
                },
        } => (
            "asset_license_add",
            obj(vec![
                ("name", sv(&name)),
                ("seats", Some(json!(seats))),
                ("vendor", os(vendor)),
                ("annual_cost", os(cost)),
                ("renews_on", os(renews)),
                ("key_location", os(key_location)),
            ]),
        ),
        Cmd::License {
            op:
                LicenseOp::Assign {
                    license,
                    assignee,
                    force,
                },
        } => (
            "asset_license_assign",
            json!({ "license": license, "assignee": assignee, "force": force }),
        ),
        Cmd::License {
            op: LicenseOp::Unassign { license, assignee },
        } => (
            "asset_license_unassign",
            json!({ "license": license, "assignee": assignee }),
        ),
        Cmd::License {
            op: LicenseOp::Seats { license, seats },
        } => (
            "asset_license_seats",
            json!({ "license": license, "seats": seats }),
        ),
        Cmd::License {
            op: LicenseOp::Audit,
        } => ("asset_license_audit", json!({})),
        Cmd::Deadline {
            op:
                DeadlineOp::Add {
                    name,
                    due,
                    kind,
                    tag,
                },
        } => (
            "asset_deadline_add",
            obj(vec![
                ("name", sv(&name)),
                ("due_on", sv(&due)),
                ("kind", os(kind)),
                ("tag", os(tag)),
            ]),
        ),
        Cmd::Deadline {
            op: DeadlineOp::Done { id },
        } => ("asset_deadline_done", json!({ "id": id })),
        Cmd::Expiries { days } => ("asset_expiries", json!({ "within_days": days })),
        Cmd::Label { tag, path } => ("asset_label", json!({ "tag": tag, "path": path })),
        Cmd::Import { path, dry_run } => (
            "asset_import_csv",
            json!({ "path": path, "dry_run": dry_run }),
        ),
        Cmd::DrainInbox => ("asset_drain_inbox", json!({})),
        Cmd::DemoSeed => ("asset_demo_seed", json!({})),
        Cmd::Tools { .. } | Cmd::Mcp => return Err("not a tool verb".into()),
    })
}

struct Host {
    ctx: Ctx,
}

impl ToolHost for Host {
    fn server_name(&self) -> &str {
        APP
    }
    fn server_version(&self) -> &str {
        VERSION
    }
    fn tools(&self) -> Vec<Value> {
        tools()
    }
    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let cmd = Command::from_tool(name, args)?;
        dispatch(&mut self.ctx, cmd)
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "-".into(),
        Value::Array(a) => a.iter().map(scalar).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

const COLUMN_ORDER: &[&str] = &[
    "id",
    "tag",
    "date",
    "due_on",
    "year",
    "kind",
    "name",
    "category",
    "in_service",
    "cost",
    "book_value_start_of_year",
    "holder",
    "location",
    "opening",
    "amount",
    "closing",
    "debit",
    "credit",
    "posted",
    "seats",
    "used",
    "free",
    "renews_on",
    "days_to_renewal",
    "days_left",
    "overdue",
    "annual_cost",
    "cost_per_used_seat",
    "flags",
    "ref",
    "life_years",
    "min_years",
    "account",
    "assigned",
    "vendor",
];
const HIDDEN: &[&str] = &[
    "id_db",
    "purchase_date",
    "supplier",
    "invoice_ref",
    "notes",
    "serial",
    "disposal_note",
    "disposal_proceeds_cents",
    "disposed_on",
    "warranty_until",
    "key_location",
    "gwg",
];

fn columns(rows: &[Value]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for r in rows.iter().filter_map(Value::as_object) {
        for (k, v) in r {
            if !v.is_object()
                && !k.ends_with("_cents")
                && !HIDDEN.contains(&k.as_str())
                && !keys.contains(k)
            {
                keys.push(k.clone());
            }
        }
    }
    keys.sort_by_key(|k| {
        (
            COLUMN_ORDER
                .iter()
                .position(|c| c == k)
                .unwrap_or(usize::MAX),
            k.clone(),
        )
    });
    keys
}

/// Human rendering: header scalars, then an aligned table of the first list found.
fn render(v: &Value) -> String {
    const LISTS: [&str; 6] = [
        "rows",
        "assets",
        "licenses",
        "categories",
        "maintenance",
        "postings",
    ];
    let Some(obj) = v.as_object() else {
        return scalar(v);
    };
    let Some(key) = LISTS
        .iter()
        .find(|k| obj.get(**k).is_some_and(Value::is_array))
    else {
        return serde_json::to_string_pretty(v).unwrap_or_else(|e| e.to_string());
    };
    let mut out = String::new();
    for (k, val) in obj {
        if !val.is_array() && !val.is_object() && !k.ends_with("_cents") {
            out.push_str(&format!("{k}: {}\n", scalar(val)));
        }
    }
    let rows = obj
        .get(*key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if rows.is_empty() {
        out.push_str("(none)\n");
        return out;
    }
    let cols = columns(&rows);
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            cols.iter()
                .map(|c| r.get(c).map(scalar).unwrap_or_else(|| "-".into()))
                .collect()
        })
        .collect();
    let widths: Vec<usize> = cols
        .iter()
        .enumerate()
        .map(|(i, c)| {
            table
                .iter()
                .map(|r| r[i].chars().count())
                .chain([c.chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: &[String]| -> String {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(c, w)| format!("{c}{}", " ".repeat(w.saturating_sub(c.chars().count()))))
            .collect();
        format!("{}\n", padded.join("  ").trim_end())
    };
    out.push_str(&line(&cols));
    for r in &table {
        out.push_str(&line(r));
    }
    out
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Cmd::Tools { markdown } = cli.cmd {
        if markdown {
            print!("{}", craft_biz::mcp::tools_markdown(APP, &tools()));
            return ExitCode::SUCCESS;
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&tools()).unwrap_or_default()
        );
        return ExitCode::SUCCESS;
    }
    let db = cli.db.clone().unwrap_or_else(default_db);
    let reg = match Registry::open(&db) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("assetcraft: cannot open {}: {e}", db.display());
            return ExitCode::FAILURE;
        }
    };
    let is_mcp = matches!(cli.cmd, Cmd::Mcp);
    let confinement = if is_mcp || !cli.allow_path.is_empty() {
        match Confinement::roots(&cli.allow_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("assetcraft: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        Confinement(None)
    };
    let ctx = Ctx {
        reg,
        confinement,
        bus: Bus::new(Bus::default_root()),
    };
    if is_mcp {
        eprintln!("[assetcraft] db {}", db.display());
        let mut host = Host { ctx };
        return match serve_stdio(&mut host) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("assetcraft mcp: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let json_out = cli.json;
    let mut ctx = ctx;
    let result = to_tool(cli.cmd)
        .and_then(|(name, args)| Command::from_tool(name, &args))
        .and_then(|cmd| dispatch(&mut ctx, cmd));
    match result {
        Ok(v) => {
            if json_out {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                print!("{}", render(&v));
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            if json_out {
                println!("{}", json!({ "error": e }));
            } else {
                eprintln!("assetcraft: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_biz::mcp::handle_line;

    #[test]
    fn tools_doc_is_current() {
        let committed = include_str!("../../../docs/TOOLS.md");
        assert!(
            craft_biz::mcp::docs_in_sync(committed, &craft_biz::mcp::tools_markdown(APP, &tools())),
            "docs/TOOLS.md is stale: run `just docs`"
        );
    }

    fn host() -> Host {
        let reg = Registry::open_in_memory().unwrap();
        let bus = Bus::new(
            std::env::temp_dir().join(format!("assetcraft-mcp-test-{}", std::process::id())),
        );
        Host {
            ctx: Ctx {
                reg,
                confinement: Confinement(Some(Vec::new())),
                bus,
            },
        }
    }

    fn text(v: &Value) -> String {
        v["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn mcp_probe_initialize_list_call() {
        let mut h = host();
        let init = handle_line(
            &mut h,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        )
        .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "assetcraft");
        let list =
            handle_line(&mut h, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        assert_eq!(list["result"]["tools"].as_array().map(Vec::len), Some(25));
        let seed = handle_line(&mut h, r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"asset_demo_seed","arguments":{}}}"#).unwrap();
        assert_eq!(seed["result"]["isError"], false, "{seed}");
        let sched = handle_line(&mut h, r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"asset.afa_schedule","arguments":{"tag":"NAS-001"}}}"#).unwrap();
        assert!(text(&sched).contains("\"half\""));
        let label = handle_line(&mut h, r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"asset_label","arguments":{"tag":"NAS-001","path":"C:/Windows/x.svg"}}}"#).unwrap();
        assert_eq!(label["result"]["isError"], true, "confined");
        let key = handle_line(&mut h, r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"asset_license_add","arguments":{"name":"Win","seats":1,"key_location":"VK7JG-NPHTM-C97JM-9MPGT-3V66T"}}}"#).unwrap();
        assert_eq!(key["result"]["isError"], true, "no keys in the register");
    }

    #[test]
    fn cli_verbs_map_to_known_tools() {
        let verbs = vec![
            Cmd::Status,
            Cmd::List {
                category: None,
                all: false,
                text: None,
            },
            Cmd::Afa {
                year: Some(2026),
                emit: false,
            },
            Cmd::License {
                op: LicenseOp::Audit,
            },
            Cmd::Deadline {
                op: DeadlineOp::Done { id: 1 },
            },
            Cmd::Expiries { days: 30 },
            Cmd::Gwg { set: None },
            Cmd::DrainInbox,
            Cmd::DemoSeed,
        ];
        let names: Vec<String> = tools()
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect();
        for v in verbs {
            let (name, _) = to_tool(v).unwrap();
            assert!(names.iter().any(|n| n == name), "{name}");
        }
    }

    #[test]
    fn render_orders_columns() {
        let v = json!({ "year": 2026, "rows": [ { "amount": "1,00", "tag": "A-1", "debit": 7000, "amount_cents": 100 } ] });
        let s = render(&v);
        let header = s.lines().find(|l| l.starts_with("tag")).unwrap();
        assert!(
            header.find("amount") < header.find("debit") && !header.contains("_cents"),
            "{s}"
        );
        assert!(render(&json!({ "assets": [] })).contains("(none)"));
    }
}
