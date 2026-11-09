use super::*;

fn ctx(tag: &str) -> Ctx {
    let root = std::env::temp_dir().join(format!("assetcraft-bus-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    Ctx {
        reg: Registry::open_in_memory().unwrap(),
        confinement: Confinement(None),
        bus: Bus::new(root),
    }
}

fn call(c: &mut Ctx, name: &str, a: Value) -> Result<Value, String> {
    let cmd = Command::from_tool(name, &a)?;
    dispatch(c, cmd)
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[test]
fn catalog_tools_are_routable() {
    assert_eq!(tools().len(), 24);
    for t in tools() {
        let name = t["name"].as_str().unwrap();
        if let Err(err) = Command::from_tool(name, &json!({})) {
            assert!(!err.starts_with("unknown tool"), "{name}");
        }
    }
    assert!(Command::from_tool(
        "asset_add",
        &json!({ "tag": "A", "name": "x", "in_service": "2026-01-01", "cost": 1249.0 })
    )
    .unwrap_err()
    .contains("no floats"));
}

#[test]
fn afa_year_emits_once_with_buchcraft_payload() {
    let mut c = ctx("afa");
    call(&mut c, "asset_add", json!({ "tag": "GOL-001", "name": "Workstation", "category": "it", "in_service": "14.02.2025", "cost": "3.890,00" })).unwrap();
    call(&mut c, "asset_add", json!({ "tag": "MON-001", "name": "Monitor", "category": "it", "in_service": "02.11.2026", "cost": "349,00" })).unwrap();
    call(&mut c, "asset_add", json!({ "tag": "NAS-001", "name": "NAS", "category": "it", "in_service": "03.08.2026", "cost": "1.620,00" })).unwrap();
    let preview = afa_year(&mut c, 2026, false, d(2026, 12, 31)).unwrap();
    // 3.890 / 3 = 1.296,66; NAS half year 270,00; monitor GWG 349,00.
    assert_eq!(preview["total"], "1.915,66");
    assert_eq!(preview["emitted"], 0);
    assert!(
        afa_year(&mut c, 2027, true, d(2026, 12, 31)).is_err(),
        "future year"
    );
    let posted = afa_year(&mut c, 2026, true, d(2026, 12, 31)).unwrap();
    assert_eq!(posted["emitted"], 3);
    let again = afa_year(&mut c, 2026, true, d(2026, 12, 31)).unwrap();
    assert_eq!(
        (again["emitted"].as_u64(), again["already_posted"].as_u64()),
        (Some(0), Some(3))
    );
    let events = c.bus.pending("buchcraft").unwrap();
    assert_eq!(events.len(), 3);
    let gwg = events
        .iter()
        .find(|e| e.payload["asset_tag"] == "MON-001")
        .unwrap();
    assert_eq!(
        (
            gwg.kind.as_str(),
            &gwg.payload["debit"],
            &gwg.payload["credit"],
            &gwg.payload["amount_cents"]
        ),
        (
            "asset.depreciation_due",
            &json!(7010),
            &json!(680),
            &json!(34_900)
        )
    );
    let gol = events
        .iter()
        .find(|e| e.payload["asset_tag"] == "GOL-001")
        .unwrap();
    assert_eq!(
        (
            &gol.payload["debit"],
            &gol.payload["credit"],
            &gol.payload["date"]
        ),
        (&json!(7000), &json!(620), &json!("2026-12-31"))
    );
    // Basis is now frozen.
    assert!(call(
        &mut c,
        "asset_update",
        json!({ "tag": "GOL-001", "cost": "4.000,00" })
    )
    .unwrap_err()
    .contains("frozen"));
    let _ = std::fs::remove_dir_all(c.bus.root());
}

#[test]
fn dispose_reports_gain_and_freezes() {
    let mut c = ctx("dispose");
    call(&mut c, "asset_add", json!({ "tag": "LAP-001", "name": "Old laptop", "category": "it", "in_service": "2025-03-10", "cost": "1.500,00" })).unwrap();
    let r = call(
        &mut c,
        "asset_dispose",
        json!({ "tag": "LAP-001", "date": "2026-05-01", "proceeds": "800,00" }),
    )
    .unwrap();
    // 2025 full 500, 2026 half 250 -> book 750, gain 50.
    assert_eq!(r["afa_in_disposal_year"], "250,00");
    assert_eq!(r["book_value_at_disposal"], "750,00");
    assert_eq!(r["gain_or_loss"], "50,00");
    assert!(call(
        &mut c,
        "asset_update",
        json!({ "tag": "LAP-001", "notes": "x" })
    )
    .is_err());
    let y = afa_year(&mut c, 2026, false, d(2026, 12, 31)).unwrap();
    assert_eq!(y["total"], "250,00");
    assert_eq!(
        afa_year(&mut c, 2027, false, d(2027, 12, 31)).unwrap()["lines"],
        0
    );
}

#[test]
fn demo_audit_and_expiries() {
    let mut c = ctx("demo");
    call(&mut c, "asset_demo_seed", json!({})).unwrap();
    assert!(call(&mut c, "asset_demo_seed", json!({})).is_err());
    let audit = call(
        &mut c,
        "asset_license_audit",
        json!({ "as_of": "2026-11-09" }),
    )
    .unwrap();
    let rows = audit["rows"].as_array().unwrap();
    let design = rows
        .iter()
        .find(|r| r["name"] == "Design suite (DEMO)")
        .unwrap();
    assert!(design["flags"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f == "over_allocated"));
    let ide = rows.iter().find(|r| r["name"] == "IDE (DEMO)").unwrap();
    assert_eq!(ide["days_to_renewal"], 11);
    assert_eq!(audit["needs_attention"], 2);
    let exp = call(
        &mut c,
        "asset_expiries",
        json!({ "as_of": "2026-11-09", "within_days": 60 }),
    )
    .unwrap();
    let kinds: Vec<&str> = exp["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "inspection",
            "licence_renewal",
            "certificate",
            "licence_renewal",
            "warranty"
        ]
    );
    assert_eq!(exp["overdue"], 1);
    let list = call(&mut c, "asset_list", json!({})).unwrap();
    assert_eq!(list["count"], 6, "disposed laptop hidden");
    // In service this year: start-of-year book value is the full cost (regression).
    let lap = list["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["tag"] == "NAS-001")
        .unwrap();
    assert_eq!(lap["book_value_start_of_year"], "1.620,00");
    let show = call(&mut c, "asset_show", json!({ "tag": "car-001" })).unwrap();
    assert_eq!(show["schedule"]["rows"].as_array().map(Vec::len), Some(8));
    assert_eq!(show["maintenance"].as_array().map(Vec::len), Some(1));
    assert_eq!(show["deadlines"].as_array().map(Vec::len), Some(2));
}

#[test]
fn csv_import_is_all_or_nothing_and_label_is_confined() {
    let mut c = ctx("csv");
    let dir = std::env::temp_dir().join(format!("assetcraft-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    c.confinement = Confinement::roots(std::slice::from_ref(&dir)).unwrap();
    let good = dir.join("good.csv");
    std::fs::write(&good, "tag;name;category;in_service;cost;serial;location;holder\nRB-001;Raspbot V2;machine;01.03.2026;189,00;SN1;Lab;\nPLG-001;Smart plug;it;2026-04-02;19,90\n").unwrap();
    let dry = call(
        &mut c,
        "asset_import_csv",
        json!({ "path": good.display().to_string(), "dry_run": true }),
    )
    .unwrap();
    assert_eq!(
        (dry["valid_rows"].as_u64(), dry["imported"].as_u64()),
        (Some(2), Some(0))
    );
    assert_eq!(
        c.reg.assets(None, true, None).unwrap().len(),
        0,
        "dry run stores nothing"
    );
    let bad = dir.join("bad.csv");
    std::fs::write(
        &bad,
        "X-1;ok;it;01.01.2026;10,00\nX-2;broken;it;01.01.2026;abc\n",
    )
    .unwrap();
    assert!(call(
        &mut c,
        "asset_import_csv",
        json!({ "path": bad.display().to_string() })
    )
    .unwrap_err()
    .contains("line 2"));
    let dup = dir.join("dup.csv");
    std::fs::write(
        &dup,
        "X-1;ok;it;01.01.2026;10,00\nX-1;again;it;01.01.2026;10,00\n",
    )
    .unwrap();
    assert!(call(
        &mut c,
        "asset_import_csv",
        json!({ "path": dup.display().to_string() })
    )
    .is_err());
    assert_eq!(
        c.reg.assets(None, true, None).unwrap().len(),
        0,
        "failed import stores nothing"
    );
    call(
        &mut c,
        "asset_import_csv",
        json!({ "path": good.display().to_string() }),
    )
    .unwrap();
    assert_eq!(c.reg.asset("RB-001").unwrap().category, "machine");
    let out = dir.join("rb.svg");
    let l = call(
        &mut c,
        "asset_label",
        json!({ "tag": "rb-001", "path": out.display().to_string() }),
    )
    .unwrap();
    assert_eq!(l["payload"], "ASSET:RB-001");
    assert!(std::fs::read_to_string(&out)
        .unwrap()
        .contains("Raspbot V2"));
    assert!(call(
        &mut c,
        "asset_label",
        json!({ "tag": "RB-001", "path": "C:/Windows/x.svg" })
    )
    .is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
