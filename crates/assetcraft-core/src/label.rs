//! Printable SVG asset labels: QR code (`ASSET:<tag>`) plus tag, name and
//! in-service date. QR modules are drawn as merged horizontal runs so the
//! file stays small and prints crisp at any size.

use assetcraft_store::Asset;
use qrcode::{Color, EcLevel, QrCode};

/// What the QR code encodes. Short and stable: scanners resolve it via `asset_show`.
pub fn payload(a: &Asset) -> String {
    format!("ASSET:{}", a.tag)
}

fn esc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            c if c.is_control() => " ".to_string(),
            c => c.to_string(),
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!(
            "{}...",
            s.chars().take(max.saturating_sub(3)).collect::<String>()
        )
    }
}

pub fn svg(a: &Asset) -> Result<String, String> {
    let code = QrCode::with_error_correction_level(payload(a).as_bytes(), EcLevel::M)
        .map_err(|e| format!("QR: {e}"))?;
    let n = code.width();
    let colors = code.to_colors();
    let quiet = 4usize;
    let size = n + 2 * quiet;
    let mut rects = String::new();
    for y in 0..n {
        let mut x = 0;
        while x < n {
            if colors.get(y * n + x) == Some(&Color::Dark) {
                let start = x;
                while x < n && colors.get(y * n + x) == Some(&Color::Dark) {
                    x += 1;
                }
                rects.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"1\"/>",
                    start + quiet,
                    y + quiet,
                    x - start
                ));
            } else {
                x += 1;
            }
        }
    }
    // Label: 60 x 25 mm at 1 unit = 1 QR module scaled into a 25 mm square.
    let tag = esc(&a.tag);
    let name = esc(&truncate(&a.name, 34));
    let since = a.in_service.format("%d.%m.%Y");
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"60mm\" height=\"25mm\" viewBox=\"0 0 600 250\">\n\
         <rect width=\"600\" height=\"250\" fill=\"#fff\"/>\n\
         <svg x=\"0\" y=\"0\" width=\"250\" height=\"250\" viewBox=\"0 0 {size} {size}\" shape-rendering=\"crispEdges\"><g fill=\"#000\">{rects}</g></svg>\n\
         <text x=\"260\" y=\"80\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"56\" font-weight=\"700\">{tag}</text>\n\
         <text x=\"260\" y=\"140\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"26\">{name}</text>\n\
         <text x=\"260\" y=\"190\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"22\" fill=\"#444\">in Betrieb seit {since}</text>\n\
         </svg>\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn asset(name: &str) -> Asset {
        Asset {
            id: 1,
            tag: "GOL-001".into(),
            name: name.into(),
            category: "it".into(),
            serial: None,
            location: None,
            holder: None,
            purchase_date: None,
            in_service: NaiveDate::from_ymd_opt(2025, 2, 14).unwrap(),
            cost_cents: 1,
            life_years: 3,
            gwg: false,
            supplier: None,
            invoice_ref: None,
            warranty_until: None,
            disposed_on: None,
            disposal_proceeds_cents: None,
            disposal_note: None,
            notes: String::new(),
        }
    }

    #[test]
    fn label_is_well_formed_and_escaped() {
        let s = svg(&asset(
            "Rack <A&B> \"Goliath\" with a very long name that will be cut",
        ))
        .unwrap();
        assert!(s.starts_with("<svg") && s.trim_end().ends_with("</svg>"));
        assert!(s.contains("GOL-001") && s.contains("14.02.2025"));
        assert!(s.contains("&lt;A&amp;B&gt;") && !s.contains("<A&B>"));
        assert!(s.contains("..."));
        assert!(s.matches("<rect").count() > 20, "QR modules drawn");
        assert_eq!(payload(&asset("x")), "ASSET:GOL-001");
    }
}
