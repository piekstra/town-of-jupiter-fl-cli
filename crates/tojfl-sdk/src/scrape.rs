//! HTML scraping of eCARE data pages into typed models.
//!
//! The eCARE modules render data as HTML `<table>`s whose exact column set and
//! DNN control ids drift between deployments and releases. Rather than pin to
//! brittle selectors, we extract every table generically, then map columns to
//! fields by matching header text against keyword sets. Columns we don't
//! recognize are preserved verbatim in each model's `extra` map, so no data is
//! silently dropped.

use crate::model::{Account, Bill, Money, Profile, Transaction, UsageRecord};
use scraper::{ElementRef, Html, Selector};
use std::collections::BTreeMap;

/// One parsed table row: its cell texts plus any per-row statement-PDF link.
///
/// Bundling the link with the cells (rather than a parallel `Vec`) makes the
/// "link belongs to this row" invariant structural — the compiler keeps them
/// together through any future filter/sort, so a link can never bind to the
/// wrong bill.
#[derive(Debug, Clone)]
pub struct TableRow {
    pub cells: Vec<String>,
    /// The row's statement-PDF (`ctl=VieweBill`) link, if it has one. Only some
    /// bills expose an eBill.
    pub pdf_link: Option<String>,
}

/// A simple parsed HTML table.
#[derive(Debug, Clone)]
pub struct Table {
    /// The table's `id` attribute, if any (used to find eCARE data grids).
    pub id: Option<String>,
    pub headers: Vec<String>,
    pub rows: Vec<TableRow>,
}

impl Table {
    /// Index of the first header containing any of `keywords` (case-insensitive).
    fn col(&self, keywords: &[&str]) -> Option<usize> {
        self.headers.iter().position(|h| {
            let hl = h.to_lowercase();
            keywords.iter().any(|k| hl.contains(k))
        })
    }

    /// Whether this looks like an eCARE data grid (an ASP.NET GridView) rather
    /// than a layout/navigation table. DNN renders its menus as nested tables,
    /// so id-based detection is far more reliable than "biggest table wins".
    fn is_data_grid(&self) -> bool {
        self.id
            .as_deref()
            .map(|id| {
                let l = id.to_lowercase();
                l.contains("gridview") || l.contains("grdview") || l.contains("grid")
            })
            .unwrap_or(false)
    }

    /// Detect layout/menu tables to exclude: their "cells" carry script or the
    /// DNN SolPartMenu payload rather than tabular data.
    fn is_noise(&self) -> bool {
        let joined = self.headers.join(" ");
        joined.len() > 400
            || self.headers.iter().any(|h| {
                let l = h.to_lowercase();
                l.contains("function ")
                    || l.contains("preloadimages")
                    || l.contains("ddr.menu")
                    || l.contains("mm_")
                    || h.len() > 120
            })
    }
}

fn cell_text(el: ElementRef) -> String {
    el.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extract every table on the page that has at least one header and one body row.
pub fn extract_tables(html: &str) -> Vec<Table> {
    let doc = Html::parse_document(html);
    let table_sel = Selector::parse("table").unwrap();
    let tr_sel = Selector::parse("tr").unwrap();
    let th_sel = Selector::parse("th").unwrap();
    let td_sel = Selector::parse("td").unwrap();
    let ebill_sel = Selector::parse(r#"a[href*="VieweBill"]"#).unwrap();

    let mut tables = Vec::new();
    for table in doc.select(&table_sel) {
        let id = table.value().attr("id").map(|s| s.to_string());
        let rows: Vec<ElementRef> = table.select(&tr_sel).collect();
        if rows.is_empty() {
            continue;
        }

        // Header row: prefer explicit <th>; else treat the first row's <td> as
        // headers if the remaining rows look like data.
        let mut headers: Vec<String> = Vec::new();
        let mut body_start = 0;
        let first_th: Vec<ElementRef> = rows[0].select(&th_sel).collect();
        if !first_th.is_empty() {
            headers = first_th.iter().map(|e| cell_text(*e)).collect();
            body_start = 1;
        } else if rows.len() > 1 {
            let first_td: Vec<ElementRef> = rows[0].select(&td_sel).collect();
            headers = first_td.iter().map(|e| cell_text(*e)).collect();
            body_start = 1;
        }
        if headers.is_empty() {
            continue;
        }

        let mut body = Vec::new();
        for row in &rows[body_start..] {
            let cells: Vec<String> = row.select(&td_sel).map(|e| cell_text(e)).collect();
            if cells.is_empty() || cells.iter().all(|c| c.is_empty()) {
                continue;
            }
            // Capture this row's statement-PDF link (if any) alongside its cells,
            // scoped to the row so a link can never bind to the wrong bill.
            let pdf_link = row
                .select(&ebill_sel)
                .next()
                .and_then(|a| a.value().attr("href"))
                .map(|h| h.replace("&amp;", "&"));
            body.push(TableRow { cells, pdf_link });
        }
        if body.is_empty() {
            continue;
        }
        let t = Table {
            id,
            headers,
            rows: body,
        };
        // Drop DNN menu/layout/script tables so they can't masquerade as data.
        if !t.is_noise() {
            tables.push(t);
        }
    }
    tables
}

/// Pick the table most likely to hold rows relevant to `keywords` in its headers.
///
/// Prefers a real eCARE data grid (an ASP.NET GridView, detected by id) whose
/// headers match. Only if no grid matches does it fall back to the largest
/// keyword-matching table — never blindly to "the biggest table on the page",
/// which on a DNN site is usually the navigation menu.
fn best_table<'a>(tables: &'a [Table], keywords: &[&str]) -> Option<&'a Table> {
    tables
        .iter()
        .filter(|t| t.is_data_grid() && t.col(keywords).is_some())
        .max_by_key(|t| t.rows.len())
        .or_else(|| {
            tables
                .iter()
                .filter(|t| t.is_data_grid())
                .max_by_key(|t| t.rows.len())
        })
        .or_else(|| {
            tables
                .iter()
                .filter(|t| t.col(keywords).is_some())
                .max_by_key(|t| t.rows.len())
        })
}

/// Like [`best_table`] but only considers real data grids (or, for tests, a
/// keyword-matching table when no ids are present). Used where matching a
/// non-grid layout fragment would produce garbage (e.g. the usage page).
fn best_grid<'a>(tables: &'a [Table], keywords: &[&str]) -> Option<&'a Table> {
    // Prefer a keyword-matching grid; else, if any grid ids exist at all, the
    // largest grid; else (no ids on the page, i.e. a unit-test fixture) fall
    // back to a keyword match so parser logic stays testable.
    let any_ids = tables.iter().any(|t| t.id.is_some());
    tables
        .iter()
        .filter(|t| t.is_data_grid() && t.col(keywords).is_some())
        .max_by_key(|t| t.rows.len())
        .or_else(|| {
            if any_ids {
                None
            } else {
                tables
                    .iter()
                    .filter(|t| t.col(keywords).is_some())
                    .max_by_key(|t| t.rows.len())
            }
        })
}

fn build_extra(
    headers: &[String],
    row: &[String],
    used: &[Option<usize>],
) -> BTreeMap<String, String> {
    let used: std::collections::HashSet<usize> = used.iter().flatten().copied().collect();
    let mut extra = BTreeMap::new();
    for (i, h) in headers.iter().enumerate() {
        if used.contains(&i) {
            continue;
        }
        if let Some(v) = row.get(i) {
            if !v.is_empty() && !h.is_empty() {
                extra.insert(h.clone(), v.clone());
            }
        }
    }
    extra
}

/// Parse the billing-history table into [`Bill`]s.
pub fn parse_bills(html: &str) -> Vec<Bill> {
    let tables = extract_tables(html);
    let table = match best_table(&tables, &["bill", "statement", "date"]) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let c_date = table.col(&["date", "bill", "statement", "period"]);
    let c_amount = table.col(&["amount", "charge", "bill amount", "total"]);
    let c_balance = table.col(&["balance", "due"]);
    let c_due = table.col(&["due date"]);

    table
        .rows
        .iter()
        .map(|row| {
            let get = |i: Option<usize>| i.and_then(|i| row.cells.get(i)).cloned();
            Bill {
                date: get(c_date).unwrap_or_default(),
                amount: get(c_amount).as_deref().and_then(Money::parse),
                balance: get(c_balance).as_deref().and_then(Money::parse),
                due_date: get(c_due),
                document_id: None,
                // Per-row eBill link; only some bills expose a downloadable
                // statement. Bundled with the row, so it can't misalign.
                document_url: row.pdf_link.clone(),
                extra: build_extra(
                    &table.headers,
                    &row.cells,
                    &[c_date, c_amount, c_balance, c_due],
                ),
            }
        })
        .collect()
}

/// Parse the usage/consumption table into [`UsageRecord`]s.
///
/// `UsageHistory.aspx` is a form-first page: the consumption grid only renders
/// after a service type is selected and submitted. Until that postback flow is
/// implemented, the bare page has no data grid, so this returns empty rather
/// than latching onto the "Consumption Comparison" form fragment. Restricted to
/// real data grids for exactly that reason.
pub fn parse_usage(html: &str) -> Vec<UsageRecord> {
    let tables = extract_tables(html);
    let table = match best_grid(
        &tables,
        &["usage", "consumption", "gallons", "period", "reading"],
    ) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let c_period = table.col(&["period", "date", "month", "read"]);
    let c_qty = table.col(&["usage", "consumption", "gallons", "kgal", "ccf", "quantity"]);
    let c_days = table.col(&["days"]);
    let c_avg = table.col(&["average", "avg", "per day"]);
    // The portal's grid carries the unit of measure in its own "Units" column.
    let c_unit = table.col(&["units", "unit", "uom"]);

    // Unit falls back to sniffing the quantity column header if there's no
    // dedicated units column.
    let header_unit = c_qty
        .and_then(|i| table.headers.get(i))
        .and_then(|h| detect_unit(h));

    table
        .rows
        .iter()
        .map(|row| {
            let get = |i: Option<usize>| i.and_then(|i| row.cells.get(i)).cloned();
            let unit = get(c_unit)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| header_unit.clone());
            UsageRecord {
                period: get(c_period).unwrap_or_default(),
                quantity: get(c_qty).as_deref().and_then(parse_number),
                unit,
                days: get(c_days)
                    .as_deref()
                    .and_then(|s| parse_number(s).map(|n| n as u32)),
                average_per_day: get(c_avg).as_deref().and_then(parse_number),
                extra: build_extra(
                    &table.headers,
                    &row.cells,
                    &[c_period, c_qty, c_days, c_avg, c_unit],
                ),
            }
        })
        .collect()
}

/// Parse the transaction-history table into [`Transaction`]s.
pub fn parse_transactions(html: &str) -> Vec<Transaction> {
    let tables = extract_tables(html);
    let table = match best_table(&tables, &["transaction", "description", "date", "payment"]) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let c_date = table.col(&["date"]);
    let c_desc = table.col(&["description", "type", "transaction", "detail"]);
    let c_amount = table.col(&["amount", "payment", "charge"]);
    let c_balance = table.col(&["balance"]);

    table
        .rows
        .iter()
        .map(|row| {
            let get = |i: Option<usize>| i.and_then(|i| row.cells.get(i)).cloned();
            Transaction {
                date: get(c_date).unwrap_or_default(),
                description: get(c_desc).unwrap_or_default(),
                amount: get(c_amount).as_deref().and_then(Money::parse),
                balance: get(c_balance).as_deref().and_then(Money::parse),
                extra: build_extra(
                    &table.headers,
                    &row.cells,
                    &[c_date, c_desc, c_amount, c_balance],
                ),
            }
        })
        .collect()
}

/// Parse the DNN user-profile page into a [`Profile`]. DNN renders profile
/// fields as label/value pairs; we scan common id suffixes and fall back to a
/// generic label→value sweep.
pub fn parse_profile(html: &str) -> Profile {
    let doc = Html::parse_document(html);
    // Only pull explicitly-recognized profile properties. We deliberately do NOT
    // sweep every text input: the ManageUsers page also carries password and
    // security-question fields, which must never surface in `profile` output.
    Profile {
        first_name: value_by_id_suffixes(&doc, &["FirstName", "txtFirstName", "First_Name"]),
        last_name: value_by_id_suffixes(&doc, &["LastName", "txtLastName", "Last_Name"]),
        email: value_by_id_suffixes(&doc, &["Email", "txtEmail", "Email_TextBox"]),
        username: value_by_id_suffixes(&doc, &["Username", "txtUsername", "DisplayName"]),
        ..Default::default()
    }
}

/// Extract the account summary from the post-login home page. Its `acctInfo`
/// panel renders `Label : Value` colon-separated spans — `Customer/Account #:
/// <customer> - <account>`, `Balance : $X.XX`, `Due Date : …`, `Account Status
/// : …` — which reflect the currently-selected account.
pub fn parse_account_summary(html: &str) -> Account {
    let doc = Html::parse_document(html);
    // Whole-page visible text with runs collapsed, so the colon-separated
    // label/value spans read as one string for pattern matching.
    let text: String = doc
        .root_element()
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let cap = |re: &str, n: usize| -> Option<String> {
        regex::Regex::new(re)
            .ok()?
            .captures(&text)?
            .get(n)
            .map(|m| m.as_str().trim().to_string())
    };

    // "Customer/Account #: 1234567 - 123456" → customer / account.
    let (customer_number, account_number) =
        match regex::Regex::new(r"Customer/Account\s*#:\s*(\d+)\s*-\s*(\d+)")
            .ok()
            .and_then(|re| {
                re.captures(&text)
                    .map(|c| (c[1].to_string(), c[2].to_string()))
            }) {
            Some((c, a)) => (c, a),
            None => (String::new(), String::new()),
        };

    let mut acct = Account {
        customer_number,
        account_number,
        balance: cap(r"Balance\s*:\s*(\(?\$?[\d,]+\.\d{2}\)?)", 1)
            .as_deref()
            .and_then(Money::parse),
        due_date: cap(r"Due Date\s*:\s*([\d/]+)", 1),
        ..Default::default()
    };

    // Fallbacks: labeled cells (older layouts) then the embedded billing grid.
    if acct.balance.is_none() {
        acct.balance = find_labeled_money(&doc, &["balance", "amount due", "total due"]);
    }
    if acct.balance.is_none() {
        if let Some(latest) = parse_bills(html).into_iter().next() {
            acct.balance = latest.amount.or(latest.balance);
        }
    }
    if acct.due_date.is_none() {
        acct.due_date = find_labeled_text(&doc, &["due date", "payment due"]);
    }
    acct
}

// --- small parsing helpers -------------------------------------------------

fn detect_unit(header: &str) -> Option<String> {
    let h = header.to_lowercase();
    for u in ["gallons", "kgal", "ccf", "cubic feet", "therms", "kwh"] {
        if h.contains(u) {
            return Some(u.to_string());
        }
    }
    None
}

/// Parse the first numeric token from a string (handles commas).
pub fn parse_number(s: &str) -> Option<f64> {
    let mut digits = String::new();
    let mut seen_dot = false;
    let mut started = false;
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            started = true;
        } else if ch == '.' && !seen_dot && started {
            digits.push(ch);
            seen_dot = true;
        } else if ch == ',' {
            continue;
        } else if started {
            break;
        }
    }
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn value_by_id_suffixes(doc: &Html, suffixes: &[&str]) -> Option<String> {
    let sel = Selector::parse("input[id], span[id], td[id]").ok()?;
    for el in doc.select(&sel) {
        if let Some(id) = el.value().attr("id") {
            if suffixes.iter().any(|s| id.ends_with(s)) {
                let v = el
                    .value()
                    .attr("value")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| cell_text(el));
                let v = v.trim().to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Look for a money value adjacent to a label containing any keyword.
fn find_labeled_money(doc: &Html, keywords: &[&str]) -> Option<Money> {
    find_labeled_text(doc, keywords)
        .as_deref()
        .and_then(Money::parse)
}

/// Find text near a label containing any keyword. Scans table rows (label cell
/// then value cell) and label/value sibling spans.
fn find_labeled_text(doc: &Html, keywords: &[&str]) -> Option<String> {
    let matches = |s: &str| {
        let l = s.to_lowercase();
        keywords.iter().any(|k| l.contains(k))
    };

    // Two-cell table rows: <td>Label</td><td>Value</td>.
    if let Ok(tr) = Selector::parse("tr") {
        if let Ok(td) = Selector::parse("td, th") {
            for row in doc.select(&tr) {
                let cells: Vec<String> = row.select(&td).map(cell_text).collect();
                if cells.len() >= 2 && matches(&cells[0]) && !cells[1].trim().is_empty() {
                    return Some(cells[1].trim().to_string());
                }
            }
        }
    }

    // Adjacent label spans: <span>Label:</span><span>Value</span>.
    if let Ok(sp) = Selector::parse("span, label, div") {
        let els: Vec<ElementRef> = doc.select(&sp).collect();
        for i in 0..els.len() {
            let t = cell_text(els[i]);
            if matches(&t) {
                if let Some(next) = els.get(i + 1) {
                    let v = cell_text(*next);
                    if !v.trim().is_empty() && !matches(&v) {
                        return Some(v.trim().to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_number() {
        assert_eq!(parse_number("1,234 gallons"), Some(1234.0));
        assert_eq!(parse_number("12.5"), Some(12.5));
        assert_eq!(parse_number("n/a"), None);
    }

    #[test]
    fn extracts_a_table() {
        let html = r#"<table>
            <tr><th>Bill Date</th><th>Amount</th><th>Balance</th></tr>
            <tr><td>06/01/2026</td><td>$84.21</td><td>$0.00</td></tr>
            <tr><td>05/01/2026</td><td>$79.10</td><td>$0.00</td></tr>
        </table>"#;
        let tables = extract_tables(html);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].headers.len(), 3);
        assert_eq!(tables[0].rows.len(), 2);
    }

    #[test]
    fn parses_bills_table() {
        let html = r#"<table>
            <tr><th>Bill Date</th><th>Amount</th><th>Balance</th><th>Due Date</th></tr>
            <tr><td>06/01/2026</td><td>$84.21</td><td>$0.00</td><td>06/21/2026</td></tr>
        </table>"#;
        let bills = parse_bills(html);
        assert_eq!(bills.len(), 1);
        assert_eq!(bills[0].date, "06/01/2026");
        assert_eq!(bills[0].amount, Some(Money::from_cents(8421)));
        assert_eq!(bills[0].due_date.as_deref(), Some("06/21/2026"));
    }

    #[test]
    fn parses_usage_table() {
        let html = r#"<table>
            <tr><th>Period</th><th>Usage (gallons)</th><th>Days</th></tr>
            <tr><td>Jun 2026</td><td>3,120</td><td>30</td></tr>
        </table>"#;
        let usage = parse_usage(html);
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].period, "Jun 2026");
        assert_eq!(usage[0].quantity, Some(3120.0));
        assert_eq!(usage[0].unit.as_deref(), Some("gallons"));
        assert_eq!(usage[0].days, Some(30));
    }

    #[test]
    fn bills_capture_ebill_pdf_url() {
        let html = r#"<table id="BillingHistory_GridView1">
            <tr><th>Bill Date</th><th>Bill Total</th><th>Web Bill</th></tr>
            <tr>
              <td><a href="javascript:__doPostBack('x','')">06/16/2026</a></td>
              <td>$84.21</td>
              <td><a href="https://utilitybill.jupiter.fl.us/BillingHistory.aspx?mid=1&amp;ctl=VieweBill&amp;BH=abc">View</a></td>
            </tr>
        </table>"#;
        let bills = parse_bills(html);
        assert_eq!(bills.len(), 1);
        assert_eq!(bills[0].date, "06/16/2026");
        // The eBill URL is captured (entities decoded), the postback link ignored.
        assert_eq!(
            bills[0].document_url.as_deref(),
            Some(
                "https://utilitybill.jupiter.fl.us/BillingHistory.aspx?mid=1&ctl=VieweBill&BH=abc"
            )
        );
    }

    #[test]
    fn bills_without_ebill_link_have_no_document_url() {
        // Rows lacking a Web Bill link must yield `document_url: None` — this is
        // the precondition `Portal::download_bill` rejects. Guards the case where
        // (as on the live portal) only the most recent statement is downloadable.
        let html = r#"<table id="BillingHistory_GridView1">
            <tr><th>Bill Date</th><th>Bill Total</th><th>Web Bill</th></tr>
            <tr><td>06/16/2026</td><td>$84.21</td>
                <td><a href="https://x/BillingHistory.aspx?ctl=VieweBill&amp;BH=z">View</a></td></tr>
            <tr><td>05/14/2026</td><td>$79.10</td><td>&nbsp;</td></tr>
        </table>"#;
        let bills = parse_bills(html);
        assert_eq!(bills.len(), 2);
        assert!(
            bills[0].document_url.is_some(),
            "row with a link is downloadable"
        );
        assert!(
            bills[1].document_url.is_none(),
            "row without a link must not carry a (mis-aligned) URL"
        );
    }

    #[test]
    fn usage_unit_comes_from_units_column() {
        // Mirrors the live grid: a dedicated "Units" column, not a unit baked
        // into the quantity header.
        let html = r#"<table id="UsageHistory_GridView1">
            <tr><th>Read Date</th><th>Consumption</th><th>Units</th></tr>
            <tr><td>06/2026</td><td>3,120</td><td>100 Gallons</td></tr>
        </table>"#;
        let u = parse_usage(html);
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].quantity, Some(3120.0));
        assert_eq!(u[0].unit.as_deref(), Some("100 Gallons"));
        assert!(
            u[0].extra.is_empty(),
            "recognized columns should not leak to extra"
        );
    }

    #[test]
    fn parses_transactions_and_keeps_extra() {
        let html = r#"<table>
            <tr><th>Date</th><th>Description</th><th>Amount</th><th>Balance</th><th>Reference</th></tr>
            <tr><td>06/15/2026</td><td>Payment - Thank You</td><td>($84.21)</td><td>$0.00</td><td>WEB123</td></tr>
        </table>"#;
        let txns = parse_transactions(html);
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].description, "Payment - Thank You");
        assert_eq!(txns[0].amount, Some(Money::from_cents(-8421)));
        assert_eq!(
            txns[0].extra.get("Reference").map(String::as_str),
            Some("WEB123")
        );
    }

    #[test]
    fn finds_labeled_balance() {
        let html = r#"<table><tr><td>Current Balance</td><td>$123.45</td></tr>
            <tr><td>Due Date</td><td>07/20/2026</td></tr></table>"#;
        let acct = parse_account_summary(html);
        assert_eq!(acct.balance, Some(Money::from_cents(12345)));
        assert_eq!(acct.due_date.as_deref(), Some("07/20/2026"));
    }
}
