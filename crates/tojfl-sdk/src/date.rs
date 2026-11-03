//! Tolerant parsing of the calendar dates the eCARE grids render, plus a
//! date-range predicate for client-side history filtering.
//!
//! The portal returns no query API for date ranges, so `--since`/`--until`
//! filtering happens on the scraped rows. Dates arrive in two shapes —
//! `MM/DD/YYYY` (billing history) and `Mon DD, YYYY` (transactions, usage, meter
//! reads); user-supplied bounds additionally accept ISO `YYYY-MM-DD`. Everything
//! parses into a comparable `(year, month, day)` triple, so a range check is
//! plain tuple ordering and no date-library dependency is needed.

/// A calendar date as a comparable `(year, month, day)` triple.
pub type Ymd = (i32, u32, u32);

/// Parse a date in any shape the portal or a `--since`/`--until` flag may use:
/// ISO `YYYY-MM-DD`, `MM/DD/YYYY`, or `Mon DD, YYYY` (e.g. `Jul 15, 2026`, with
/// or without a `.` after the month). Returns `None` for anything unrecognized.
pub fn parse(s: &str) -> Option<Ymd> {
    let s = s.trim();
    parse_iso(s)
        .or_else(|| parse_slash(s))
        .or_else(|| parse_month_name(s))
}

/// A month the portal names with no day, such as "Jun 2026", as (year, month).
pub fn parse_month_year(s: &str) -> Option<(i32, u32)> {
    let mut parts = s.split_whitespace();
    let month = month_from_name(parts.next()?)?;
    let year: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1000..=9999).contains(&year) {
        return None;
    }
    Some((year, month))
}

fn parse_iso(s: &str) -> Option<Ymd> {
    let mut it = s.split('-');
    let y = it.next()?.trim().parse().ok()?;
    let m = it.next()?.trim().parse().ok()?;
    let d = it.next()?.trim().parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    valid(y, m, d)
}

fn parse_slash(s: &str) -> Option<Ymd> {
    let mut it = s.split('/');
    let m = it.next()?.trim().parse().ok()?;
    let d = it.next()?.trim().parse().ok()?;
    let y = it.next()?.trim().parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    valid(y, m, d)
}

fn parse_month_name(s: &str) -> Option<Ymd> {
    // "Jul 15, 2026" / "Jul. 15, 2026" — treat the comma as whitespace.
    let cleaned = s.replace(',', " ");
    let mut parts = cleaned.split_whitespace();
    let m = month_from_name(parts.next()?)?;
    let d = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    valid(y, m, d)
}

fn month_from_name(s: &str) -> Option<u32> {
    let key: String = s
        .trim_end_matches('.')
        .to_lowercase()
        .chars()
        .take(3)
        .collect();
    Some(match key.as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

fn valid(y: i32, m: u32, d: u32) -> Option<Ymd> {
    if (1000..=9999).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d) {
        Some((y, m, d))
    } else {
        None
    }
}

/// Today's date (UTC) as a comparable triple, from the system clock. Day
/// granularity is all the callers need (e.g. an is-it-past-the-due-date check),
/// so the UTC/local distinction — at most a day near midnight — doesn't matter.
pub fn today() -> Ymd {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    civil_from_days(secs.div_euclid(86_400))
}

/// Convert a count of days since the Unix epoch into a civil `(y, m, d)` date.
/// Howard Hinnant's well-known branch-free algorithm — no date-library needed.
fn civil_from_days(z: i64) -> Ymd {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    ((y + if m <= 2 { 1 } else { 0 }) as i32, m as u32, d)
}

/// Whether `date_str` falls within the inclusive `[since, until]` window (either
/// bound optional). Dates that don't parse are **kept**: a range filter must
/// never silently drop rows just because a deployment used an unfamiliar date
/// shape — better to show an extra row than to hide billing data.
pub fn in_range(date_str: &str, since: Option<Ymd>, until: Option<Ymd>) -> bool {
    match parse(date_str) {
        Some(d) => since.is_none_or(|s| d >= s) && until.is_none_or(|u| d <= u),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_month_and_a_year_parse_and_nothing_else_does() {
        assert_eq!(parse_month_year("Jun 2026"), Some((2026, 6)));
        assert_eq!(parse_month_year("June 2026"), Some((2026, 6)));
        assert_eq!(parse_month_year("Jun 26"), None, "a year is four digits");
        assert_eq!(parse_month_year("Jun 20261"), None, "a year is four digits");
        assert_eq!(parse_month_year("Jun 2026 extra"), None);
        assert_eq!(parse_month_year("Summer 2026"), None);
        assert_eq!(parse_month_year("Jun"), None);
        assert_eq!(parse_month_year("2026"), None);
        assert_eq!(parse_month_year(""), None);
    }

    #[test]
    fn parses_all_portal_and_input_shapes() {
        assert_eq!(parse("2026-07-15"), Some((2026, 7, 15)));
        assert_eq!(parse("07/15/2026"), Some((2026, 7, 15)));
        assert_eq!(parse("Jul 15, 2026"), Some((2026, 7, 15)));
        assert_eq!(parse("Jul. 15, 2026"), Some((2026, 7, 15)));
        assert_eq!(parse("  Jun 12, 2026 "), Some((2026, 6, 12)));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("not a date"), None);
        assert_eq!(parse("2026/07"), None);
        assert_eq!(parse("Foo 15, 2026"), None);
        assert_eq!(parse("13/40/2026"), None); // month/day out of range
    }

    #[test]
    fn tuple_order_matches_calendar_order() {
        // Same year: month then day drive ordering across the mixed formats.
        assert!(parse("Jun 12, 2026") < parse("07/15/2026"));
        assert!(parse("2025-12-31") < parse("Jan 1, 2026"));
    }

    #[test]
    fn in_range_is_inclusive_on_both_bounds() {
        let since = parse("2026-06-01");
        let until = parse("2026-06-30");
        assert!(
            in_range("06/01/2026", since, until),
            "lower bound inclusive"
        );
        assert!(
            in_range("06/30/2026", since, until),
            "upper bound inclusive"
        );
        assert!(in_range("Jun 15, 2026", since, until));
        assert!(!in_range("05/31/2026", since, until));
        assert!(!in_range("Jul 1, 2026", since, until));
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1)); // Unix epoch
        assert_eq!(civil_from_days(18262), (2020, 1, 1)); // a known leap year start
        assert_eq!(civil_from_days(-1), (1969, 12, 31)); // before the epoch
    }

    #[test]
    fn today_is_a_plausible_date() {
        let (y, m, d) = today();
        assert!((2025..2100).contains(&y), "year {y} out of range");
        assert!((1..=12).contains(&m));
        assert!((1..=31).contains(&d));
    }

    #[test]
    fn open_ended_bounds_and_unparseable_rows() {
        // Only a lower bound / only an upper bound.
        assert!(in_range("Jul 15, 2026", parse("2026-01-01"), None));
        assert!(!in_range("Jul 15, 2026", parse("2026-08-01"), None));
        assert!(in_range("Jul 15, 2026", None, parse("2026-12-31")));
        // No bounds at all keeps everything.
        assert!(in_range("anything", None, None));
        // Unparseable date + an active filter is kept, not dropped.
        assert!(in_range(
            "Q3 2026",
            parse("2026-01-01"),
            parse("2026-12-31")
        ));
    }
}
