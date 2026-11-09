//! Austrian straight-line depreciation (AfA, EStG section 7) in cents.
//!
//! - Fiscal year = calendar year.
//! - Halbjahres-AfA: an asset used for no more than six months in a year gets
//!   half the annual amount for that year. Applies to the first year (in service
//!   from 1 July) and to the disposal year (disposed within six months of the
//!   start of that year's use).
//! - GWG (cost at or below the limit, flag set): full write-off in the first year.
//! - Rounding: the annual amount is floored to whole cents; the final year takes
//!   the remainder, so the schedule always sums to the cost and never grows a
//!   stray extra year.
//! - Degressive AfA (section 7(1a)) and the car cost cap (Luxustangente) are not
//!   modelled.

use chrono::{Datelike, Months, NaiveDate};
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct Basis {
    pub cost_cents: i64,
    pub in_service: NaiveDate,
    pub life_years: i64,
    pub gwg: bool,
    pub disposed_on: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    pub year: i32,
    pub opening_cents: i64,
    pub amount_cents: i64,
    pub closing_cents: i64,
    /// "gwg", "half", "full", "final", "disposal_half", "disposal_full"
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Schedule {
    pub rows: Vec<Row>,
    /// Book value left at disposal (written off as Anlagenabgang, not AfA).
    pub residual_at_disposal_cents: Option<i64>,
}

fn used_at_most_six_months(start: NaiveDate, end: NaiveDate) -> bool {
    start
        .checked_add_months(Months::new(6))
        .is_some_and(|limit| end <= limit)
}

/// Planned schedule, truncated at the disposal year if disposed.
pub fn schedule(b: &Basis) -> Result<Schedule, String> {
    if b.cost_cents <= 0 {
        return Err("cost must be positive".into());
    }
    if !(1..=50).contains(&b.life_years) {
        return Err("life must be 1-50 years".into());
    }
    let first = b.in_service.year();
    let mut planned: Vec<(i32, i64, &'static str)> = Vec::new();
    if b.gwg {
        planned.push((first, b.cost_cents, "gwg"));
    } else {
        let annual = b.cost_cents / b.life_years;
        let half_start = b.in_service.month() >= 7;
        let mut year = first;
        let mut spent = 0i64;
        let full_years = b.life_years - 1;
        if half_start {
            planned.push((year, annual / 2, "half"));
            spent += annual / 2;
            year += 1;
        }
        for _ in 0..full_years {
            planned.push((year, annual, "full"));
            spent += annual;
            year += 1;
        }
        planned.push((year, b.cost_cents - spent, "final"));
        // A zero final row can only happen for tiny costs; drop it rather than book 0,00.
        planned.retain(|(_, amt, _)| *amt > 0);
    }

    let mut rows = Vec::new();
    let mut book = b.cost_cents;
    let mut residual = None;
    for (year, amount, kind) in planned {
        if let Some(out) = b.disposed_on {
            if year > out.year() {
                break;
            }
            if year == out.year() {
                // Disposal year: half if used no more than six months this year, capped by book value.
                let start = if year == first {
                    b.in_service
                } else {
                    NaiveDate::from_ymd_opt(year, 1, 1).ok_or("bad year")?
                };
                let annual_full = if b.gwg {
                    amount
                } else {
                    b.cost_cents / b.life_years
                };
                let (amt, k) = if b.gwg {
                    (amount, "gwg")
                } else if used_at_most_six_months(start, out) {
                    ((annual_full / 2).min(book), "disposal_half")
                } else {
                    (annual_full.min(book), "disposal_full")
                };
                // In the first year a half-year start already halves; never exceed the planned amount.
                let amt = if year == first && kind == "half" {
                    amt.min(amount)
                } else {
                    amt
                };
                rows.push(Row {
                    year,
                    opening_cents: book,
                    amount_cents: amt,
                    closing_cents: book - amt,
                    kind: k,
                });
                book -= amt;
                residual = Some(book);
                break;
            }
        }
        rows.push(Row {
            year,
            opening_cents: book,
            amount_cents: amount,
            closing_cents: book - amount,
            kind,
        });
        book -= amount;
    }
    if b.disposed_on.is_some() && residual.is_none() {
        // Disposed after the schedule ended: whatever is left (0 once fully written off).
        residual = Some(book);
    }
    Ok(Schedule {
        rows,
        residual_at_disposal_cents: residual,
    })
}

/// AfA amount for one year (0 if none).
pub fn amount_for_year(b: &Basis, year: i32) -> Result<(i64, &'static str), String> {
    Ok(schedule(b)?
        .rows
        .into_iter()
        .find(|r| r.year == year)
        .map_or((0, "none"), |r| (r.amount_cents, r.kind)))
}

/// Book value at the end of `year` (the full cost for years before it was in service).
pub fn book_value_end_of(b: &Basis, year: i32) -> Result<i64, String> {
    if year < b.in_service.year() {
        return Ok(b.cost_cents);
    }
    let s = schedule(b)?;
    let spent: i64 = s
        .rows
        .iter()
        .filter(|r| r.year <= year)
        .map(|r| r.amount_cents)
        .sum();
    Ok(b.cost_cents - spent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }
    fn basis(cost: i64, start: NaiveDate, life: i64) -> Basis {
        Basis {
            cost_cents: cost,
            in_service: start,
            life_years: life,
            gwg: false,
            disposed_on: None,
        }
    }
    fn amounts(s: &Schedule) -> Vec<(i32, i64)> {
        s.rows.iter().map(|r| (r.year, r.amount_cents)).collect()
    }

    #[test]
    fn first_half_full_years() {
        let s = schedule(&basis(150_000, d(2026, 6, 30), 3)).unwrap();
        assert_eq!(
            amounts(&s),
            vec![(2026, 50_000), (2027, 50_000), (2028, 50_000)]
        );
        assert_eq!(s.rows.last().unwrap().closing_cents, 0);
    }

    #[test]
    fn second_half_spans_four_calendar_years() {
        let s = schedule(&basis(150_000, d(2026, 7, 1), 3)).unwrap();
        assert_eq!(
            amounts(&s),
            vec![
                (2026, 25_000),
                (2027, 50_000),
                (2028, 50_000),
                (2029, 25_000)
            ]
        );
    }

    #[test]
    fn remainder_goes_to_final_year_not_a_new_one() {
        let s = schedule(&basis(100_001, d(2026, 2, 1), 3)).unwrap();
        assert_eq!(
            amounts(&s),
            vec![(2026, 33_333), (2027, 33_333), (2028, 33_335)]
        );
        let odd = schedule(&basis(100_001, d(2026, 8, 1), 3)).unwrap();
        assert_eq!(
            amounts(&odd),
            vec![
                (2026, 16_666),
                (2027, 33_333),
                (2028, 33_333),
                (2029, 16_669)
            ]
        );
        assert_eq!(
            odd.rows.iter().map(|r| r.amount_cents).sum::<i64>(),
            100_001
        );
        let one = schedule(&basis(1, d(2026, 2, 1), 3)).unwrap();
        assert_eq!(amounts(&one), vec![(2028, 1)], "tiny cost: no zero rows");
    }

    #[test]
    fn gwg_is_written_off_at_once() {
        let s = schedule(&Basis {
            gwg: true,
            ..basis(89_900, d(2026, 11, 20), 3)
        })
        .unwrap();
        assert_eq!(amounts(&s), vec![(2026, 89_900)]);
        assert_eq!(s.rows[0].kind, "gwg");
    }

    #[test]
    fn disposal_year_half_or_full_and_residual() {
        let b = basis(150_000, d(2026, 3, 10), 3);
        // Disposed 2027-05-01: used Jan-Apr in 2027 -> half (25.000), residual 75.000.
        let s = schedule(&Basis {
            disposed_on: Some(d(2027, 5, 1)),
            ..b
        })
        .unwrap();
        assert_eq!(amounts(&s), vec![(2026, 50_000), (2027, 25_000)]);
        assert_eq!(s.residual_at_disposal_cents, Some(75_000));
        // Disposed 2027-09-15: more than six months -> full year.
        let s = schedule(&Basis {
            disposed_on: Some(d(2027, 9, 15)),
            ..b
        })
        .unwrap();
        assert_eq!(amounts(&s), vec![(2026, 50_000), (2027, 50_000)]);
        assert_eq!(s.residual_at_disposal_cents, Some(50_000));
        // Disposed long after the schedule ended: nothing left.
        let s = schedule(&Basis {
            disposed_on: Some(d(2031, 1, 10)),
            ..b
        })
        .unwrap();
        assert_eq!(s.rows.len(), 3);
        assert_eq!(s.residual_at_disposal_cents, Some(0));
        // Same-year in and out within six months: half of the year.
        let s = schedule(&Basis {
            disposed_on: Some(d(2026, 8, 1)),
            ..b
        })
        .unwrap();
        assert_eq!(amounts(&s), vec![(2026, 25_000)]);
    }

    #[test]
    fn helpers() {
        let b = basis(150_000, d(2026, 7, 1), 3);
        assert_eq!(amount_for_year(&b, 2029).unwrap(), (25_000, "final"));
        assert_eq!(amount_for_year(&b, 2030).unwrap().0, 0);
        assert_eq!(book_value_end_of(&b, 2027).unwrap(), 75_000);
        assert_eq!(book_value_end_of(&b, 2025).unwrap(), 150_000);
        assert!(schedule(&basis(0, d(2026, 1, 1), 3)).is_err());
    }
}
