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

/// A simple parsed HTML table.
#[derive(Debug, Clone)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// Index of the first header containing any of `keywords` (case-insensitive).
    fn col(&self, keywords: &[&str]) -> Option<usize> {
        self.headers.iter().position(|h| {
            let hl = h.to_lowercase();
            keywords.iter().any(|k| hl.contains(k))
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

    let mut tables = Vec::new();
    for table in doc.select(&table_sel) {
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
            body.push(cells);
        }
        if body.is_empty() {
            continue;
        }
        tables.push(Table {
            headers,
            rows: body,
        });
    }
    tables
}

/// Pick the table most likely to hold rows relevant to `keywords` in its headers.
fn best_table<'a>(tables: &'a [Table], keywords: &[&str]) -> Option<&'a Table> {
    tables
        .iter()
        .filter(|t| t.col(keywords).is_some())
        .max_by_key(|t| t.rows.len())
        .or_else(|| tables.iter().max_by_key(|t| t.rows.len()))
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
            let get = |i: Option<usize>| i.and_then(|i| row.get(i)).cloned();
            Bill {
                date: get(c_date).unwrap_or_default(),
                amount: get(c_amount).as_deref().and_then(Money::parse),
                balance: get(c_balance).as_deref().and_then(Money::parse),
                due_date: get(c_due),
                document_id: None,
                extra: build_extra(&table.headers, row, &[c_date, c_amount, c_balance, c_due]),
            }
        })
        .collect()
}

/// Parse the usage/consumption table into [`UsageRecord`]s.
pub fn parse_usage(html: &str) -> Vec<UsageRecord> {
    let tables = extract_tables(html);
    let table = match best_table(&tables, &["usage", "consumption", "gallons", "period"]) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let c_period = table.col(&["period", "date", "month", "read"]);
    let c_qty = table.col(&["usage", "consumption", "gallons", "kgal", "ccf", "quantity"]);
    let c_days = table.col(&["days"]);
    let c_avg = table.col(&["average", "avg", "per day"]);

    let unit = c_qty
        .and_then(|i| table.headers.get(i))
        .map(|h| detect_unit(h))
        .unwrap_or(None);

    table
        .rows
        .iter()
        .map(|row| {
            let get = |i: Option<usize>| i.and_then(|i| row.get(i)).cloned();
            UsageRecord {
                period: get(c_period).unwrap_or_default(),
                quantity: get(c_qty).as_deref().and_then(parse_number),
                unit: unit.clone(),
                days: get(c_days)
                    .as_deref()
                    .and_then(|s| parse_number(s).map(|n| n as u32)),
                average_per_day: get(c_avg).as_deref().and_then(parse_number),
                extra: build_extra(&table.headers, row, &[c_period, c_qty, c_days, c_avg]),
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
            let get = |i: Option<usize>| i.and_then(|i| row.get(i)).cloned();
            Transaction {
                date: get(c_date).unwrap_or_default(),
                description: get(c_desc).unwrap_or_default(),
                amount: get(c_amount).as_deref().and_then(Money::parse),
                balance: get(c_balance).as_deref().and_then(Money::parse),
                extra: build_extra(&table.headers, row, &[c_date, c_desc, c_amount, c_balance]),
            }
        })
        .collect()
}

/// Parse the DNN user-profile page into a [`Profile`]. DNN renders profile
/// fields as label/value pairs; we scan common id suffixes and fall back to a
/// generic label→value sweep.
pub fn parse_profile(html: &str) -> Profile {
    let doc = Html::parse_document(html);
    let mut p = Profile {
        first_name: value_by_id_suffixes(&doc, &["FirstName", "txtFirstName", "First_Name"]),
        last_name: value_by_id_suffixes(&doc, &["LastName", "txtLastName", "Last_Name"]),
        email: value_by_id_suffixes(&doc, &["Email", "txtEmail", "Email_TextBox"]),
        username: value_by_id_suffixes(&doc, &["Username", "txtUsername", "DisplayName"]),
        ..Default::default()
    };

    // Sweep obvious label→value rows for anything else useful.
    if let Ok(sel) = Selector::parse("input[type=text], input:not([type])") {
        for el in doc.select(&sel) {
            if let (Some(id), Some(val)) = (el.value().attr("id"), el.value().attr("value")) {
                if val.trim().is_empty() {
                    continue;
                }
                let key = id.rsplit('_').next().unwrap_or(id).to_string();
                p.extra.entry(key).or_insert_with(|| val.trim().to_string());
            }
        }
    }
    p
}

/// Extract a best-effort account summary (balance, due date, account number)
/// from the post-login landing/home page.
pub fn parse_account_summary(html: &str) -> Account {
    let mut acct = Account::default();
    let doc = Html::parse_document(html);

    // Balance / amount due labels.
    acct.balance = find_labeled_money(
        &doc,
        &["balance", "amount due", "current balance", "total due"],
    );
    if let Some(due) = find_labeled_text(&doc, &["due date", "payment due"]) {
        acct.due_date = Some(due);
    }
    if let Some(addr) = find_labeled_text(&doc, &["service address", "address"]) {
        acct.service_address = Some(addr);
    }
    if let Some(name) = find_labeled_text(&doc, &["account name", "name on account"]) {
        acct.name = Some(name);
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
