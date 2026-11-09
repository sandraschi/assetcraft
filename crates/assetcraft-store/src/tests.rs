use super::*;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn laptop(r: &Registry, tag: &str, cost: i64) -> Result<Asset> {
    r.add_asset(&NewAsset {
        tag: tag.into(),
        name: "ThinkPad".into(),
        category: "it".into(),
        in_service: d(2026, 3, 10),
        cost_cents: cost,
        ..Default::default()
    })
}

#[test]
fn add_validates_and_defaults() {
    let r = Registry::open_in_memory().unwrap();
    let a = laptop(&r, "gol-001", 150_000).unwrap();
    assert_eq!((a.tag.as_str(), a.life_years, a.gwg), ("GOL-001", 3, false));
    assert!(laptop(&r, "GOL-001", 150_000).is_err(), "duplicate tag");
    assert!(laptop(&r, "bad tag!", 150_000).is_err());
    assert!(laptop(&r, "X1", 0).is_err());
    // GWG: up to and including the limit.
    assert!(laptop(&r, "GWG-1", 100_000).unwrap().gwg);
    assert!(!laptop(&r, "GWG-2", 100_001).unwrap().gwg);
    assert!(r
        .add_asset(&NewAsset {
            tag: "GWG-3".into(),
            name: "x".into(),
            category: "it".into(),
            in_service: d(2026, 1, 1),
            cost_cents: 200_000,
            gwg: Some(true),
            ..Default::default()
        })
        .is_err());
    // Car: at least 8 years.
    let car = NewAsset {
        tag: "CAR-1".into(),
        name: "Skoda".into(),
        category: "car".into(),
        in_service: d(2026, 1, 1),
        cost_cents: 3_000_000,
        life_years: Some(5),
        ..Default::default()
    };
    assert!(r
        .add_asset(&car)
        .unwrap_err()
        .to_string()
        .contains("8 years"));
    assert_eq!(
        r.add_asset(&NewAsset {
            life_years: None,
            ..car
        })
        .unwrap()
        .life_years,
        8
    );
    assert!(r
        .add_asset(&NewAsset {
            tag: "Z".into(),
            name: "x".into(),
            category: "spaceship".into(),
            in_service: d(2026, 1, 1),
            cost_cents: 1,
            ..Default::default()
        })
        .is_err());
}

#[test]
fn gwg_limit_is_configurable() {
    let r = Registry::open_in_memory().unwrap();
    r.set_gwg_limit(80_000).unwrap();
    assert!(!laptop(&r, "A", 90_000).unwrap().gwg);
    assert!(r.set_gwg_limit(0).is_err());
}

#[test]
fn posted_basis_is_frozen_twice() {
    let r = Registry::open_in_memory().unwrap();
    let a = laptop(&r, "GOL-001", 150_000).unwrap();
    r.update_asset(
        "GOL-001",
        &AssetPatch {
            cost_cents: Some(160_000),
            ..Default::default()
        },
    )
    .unwrap();
    r.record_posting(a.id, 2026, 53_333, "ev1").unwrap();
    assert!(
        r.record_posting(a.id, 2026, 53_333, "ev2").is_err(),
        "once per year"
    );
    assert!(matches!(
        r.update_asset(
            "GOL-001",
            &AssetPatch {
                life_years: Some(4),
                ..Default::default()
            }
        ),
        Err(AssetError::Locked(_))
    ));
    // Non-basis fields stay editable.
    let u = r
        .update_asset(
            "GOL-001",
            &AssetPatch {
                holder: Some(Some("Joe Mocky".into())),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(u.holder.as_deref(), Some("Joe Mocky"));
    // The trigger holds when Rust is bypassed.
    let raw = r.conn().execute(
        "UPDATE assets SET cost_cents = 1 WHERE id = ?1",
        params![a.id],
    );
    assert!(raw.unwrap_err().to_string().contains("frozen"));
}

#[test]
fn disposed_assets_are_read_only() {
    let r = Registry::open_in_memory().unwrap();
    let a = laptop(&r, "GOL-001", 150_000).unwrap();
    assert!(
        r.dispose("GOL-001", d(2026, 1, 1), 0, "").is_err(),
        "before in service"
    );
    r.dispose("GOL-001", d(2027, 5, 1), 20_000, "sold to Hannes")
        .unwrap();
    assert!(r.dispose("GOL-001", d(2027, 6, 1), 0, "").is_err());
    assert!(matches!(
        r.update_asset(
            "GOL-001",
            &AssetPatch {
                notes: Some("x".into()),
                ..Default::default()
            }
        ),
        Err(AssetError::Locked(_))
    ));
    assert!(r
        .conn()
        .execute("UPDATE assets SET notes = 'x' WHERE id = ?1", params![a.id])
        .is_err());
    assert_eq!(r.assets(None, false, None).unwrap().len(), 0);
    assert_eq!(r.assets(None, true, Some("thinkpad")).unwrap().len(), 1);
}

#[test]
fn licence_seats_and_key_guard() {
    let r = Registry::open_in_memory().unwrap();
    r.add_license(&NewLicense {
        name: "Office 365".into(),
        seats: 2,
        key_location: Some("Bitwarden: M365 admin".into()),
        ..Default::default()
    })
    .unwrap();
    assert!(r
        .add_license(&NewLicense {
            name: "Win".into(),
            seats: 1,
            key_location: Some("VK7JG-NPHTM-C97JM-9MPGT-3V66T".into()),
            ..Default::default()
        })
        .unwrap_err()
        .to_string()
        .contains("licence key"));
    r.assign("office 365", "Joe Mocky", d(2026, 1, 1), false)
        .unwrap();
    assert!(
        r.assign("Office 365", "joe mocky", d(2026, 1, 1), false)
            .is_err(),
        "no double seat"
    );
    r.assign("Office 365", "Hannes Mockinger", d(2026, 1, 1), false)
        .unwrap();
    assert!(
        r.assign("Office 365", "Anna Mockova", d(2026, 1, 1), false)
            .is_err(),
        "full"
    );
    let l = r
        .assign("Office 365", "Anna Mockova", d(2026, 1, 1), true)
        .unwrap();
    assert_eq!(l.assigned.len(), 3);
    assert_eq!(
        r.unassign("Office 365", "ANNA MOCKOVA")
            .unwrap()
            .assigned
            .len(),
        2
    );
    assert!(r.unassign("Office 365", "nobody").is_err());
}

#[test]
fn deadlines_and_maintenance() {
    let r = Registry::open_in_memory().unwrap();
    laptop(&r, "GOL-001", 150_000).unwrap();
    let dl = r
        .add_deadline("TLS mockfirma.at", "certificate", d(2026, 12, 1), None, "")
        .unwrap();
    assert!(r
        .add_deadline("x", "birthday", d(2026, 12, 1), None, "")
        .is_err());
    r.add_deadline("Pickerl", "inspection", d(2027, 3, 1), Some("gol-001"), "")
        .unwrap();
    r.complete_deadline(dl.id).unwrap();
    assert!(r.complete_deadline(dl.id).is_err());
    assert_eq!(r.deadlines(false).unwrap().len(), 1);
    r.add_maintenance("GOL-001", d(2026, 6, 1), "repair", 8_900, "keyboard")
        .unwrap();
    assert!(r
        .add_maintenance("GOL-001", d(2026, 6, 1), " ", 0, "")
        .is_err());
    assert_eq!(r.maintenance("GOL-001").unwrap()[0].cost_cents, 8_900);
}
