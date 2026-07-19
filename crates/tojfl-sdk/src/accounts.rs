//! Linked-account listing and switching (`ListAccounts.aspx`).
//!
//! A login can have several utility accounts linked to it. The page renders
//! them in a GridView; each row's "Select" LinkButton fires
//! `__doPostBack('…$GridView1', 'Select$<rowIndex>')`, which activates that
//! account server-side for the session so every later page reports its data.

use crate::client::Client;
use crate::dnn::FormState;
use crate::error::{Error, Result};
use crate::model::{LinkedAccount, Money};
use crate::pages;
use scraper::{Html, Selector};

/// A parsed grid row: the account plus the postback that selects it.
struct Row {
    account: LinkedAccount,
    event_target: String,
    event_argument: String,
}

/// List the accounts linked to the current login.
pub fn list(client: &Client) -> Result<Vec<LinkedAccount>> {
    let html = client.get_text(pages::LIST_ACCOUNTS)?;
    Ok(parse_rows(&html).into_iter().map(|r| r.account).collect())
}

/// Activate `account_number` for the session so subsequent pages report it.
/// Returns `Ok(false)` if that account isn't linked to this login.
pub fn select(client: &Client, account_number: &str) -> Result<bool> {
    let html = client.get_text(pages::LIST_ACCOUNTS)?;
    let rows = parse_rows(&html);
    let row = match rows
        .iter()
        .find(|r| r.account.account_number == account_number)
    {
        Some(r) => r,
        None => return Ok(false),
    };
    let mut form = FormState::from_html(&html);
    form.with_event(&row.event_target, &row.event_argument);
    let action = form
        .action
        .clone()
        .unwrap_or_else(|| pages::LIST_ACCOUNTS.to_string());
    client.post_form_text(&action, &form.to_pairs())?;
    Ok(true)
}

/// The account numbers linked to this login (convenience for validation).
pub fn numbers(client: &Client) -> Result<Vec<String>> {
    Ok(list(client)?
        .into_iter()
        .map(|a| a.account_number)
        .collect())
}

fn parse_rows(html: &str) -> Vec<Row> {
    let doc = Html::parse_document(html);
    // The linked-accounts GridView.
    let table = match Selector::parse(
        "table[id*='ListAccounts'][id*='GridView'], table[id*='ListAccounts_GridView']",
    )
    .ok()
    .and_then(|s| doc.select(&s).next())
    {
        Some(t) => t,
        None => return Vec::new(),
    };

    let tr = Selector::parse("tr").unwrap();
    let th = Selector::parse("th").unwrap();
    let td = Selector::parse("td").unwrap();
    let link = Selector::parse("a[href*='__doPostBack']").unwrap();

    // Header row → column indices by keyword.
    let mut rows_iter = table.select(&tr);
    let header = rows_iter.next();
    let headers: Vec<String> = header
        .map(|h| h.select(&th).map(|c| cell(c)).collect())
        .unwrap_or_default();
    let col = |kw: &[&str]| -> Option<usize> {
        headers.iter().position(|h| {
            let l = h.to_lowercase();
            kw.iter().any(|k| l.contains(k))
        })
    };
    let c_acct = col(&["account"]);
    let c_name = col(&["name"]);
    let c_addr = col(&["service address", "address"]);
    let c_due = col(&["past due"]);
    let c_bal = col(&["balance"]);

    let mut out = Vec::new();
    for row in rows_iter {
        let cells: Vec<_> = row.select(&td).collect();
        if cells.is_empty() {
            continue;
        }
        let text = |i: Option<usize>| i.and_then(|i| cells.get(i)).map(|c| cell(*c));
        let account_number = match text(c_acct).map(|s| s.trim().to_string()) {
            Some(a) if !a.is_empty() => a,
            _ => continue,
        };
        // The Select LinkButton's __doPostBack('target','arg') for this row.
        let (event_target, event_argument) = match row
            .select(&link)
            .find_map(|a| a.value().attr("href").and_then(parse_dopostback))
        {
            Some(pair) => pair,
            None => continue, // no way to select this row; skip
        };

        out.push(Row {
            account: LinkedAccount {
                account_number,
                name: text(c_name)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                service_address: text(c_addr)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                past_due: text(c_due).as_deref().and_then(Money::parse),
                balance: text(c_bal).as_deref().and_then(Money::parse),
            },
            event_target,
            event_argument,
        });
    }
    out
}

fn cell(el: scraper::ElementRef) -> String {
    el.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pull `(target, argument)` out of a `__doPostBack('target','argument')` call.
fn parse_dopostback(href: &str) -> Option<(String, String)> {
    let re = regex::Regex::new(r"__doPostBack\('([^']*)',\s*'([^']*)'\)").ok()?;
    let caps = re.captures(href)?;
    Some((
        caps.get(1)?.as_str().to_string(),
        caps.get(2)?.as_str().to_string(),
    ))
}

/// Turn a "not linked" `select` result into an error listing valid accounts.
pub(crate) fn not_linked(account: &str, available: &[String]) -> Error {
    Error::NotFound(format!(
        "account {account} is not linked to this login (linked: {})",
        if available.is_empty() {
            "none found".to_string()
        } else {
            available.join(", ")
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: &str = r#"
      <table id="ListAccounts_GridView1">
        <tr><th>Account #</th><th>&nbsp;</th><th>Name</th><th>Service Address</th><th>Past Due Amount</th><th>Balance</th></tr>
        <tr>
          <td>111111</td>
          <td><a href="javascript:__doPostBack('ListAccounts$GridView1','Select$0')">Select</a></td>
          <td>JANE DOE</td><td>1 MAIN ST</td><td>$0.00</td><td>$84.21</td>
        </tr>
        <tr>
          <td>222222</td>
          <td><a href="javascript:__doPostBack('ListAccounts$GridView1','Select$1')">Select</a></td>
          <td>JANE DOE</td><td>2 OAK AVE</td><td>$12.00</td><td>$12.00</td>
        </tr>
      </table>"#;

    #[test]
    fn parses_two_accounts_with_select_postbacks() {
        let rows = parse_rows(GRID);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].account.account_number, "111111");
        assert_eq!(rows[0].account.balance, Some(Money::from_cents(8421)));
        assert_eq!(rows[0].event_target, "ListAccounts$GridView1");
        assert_eq!(rows[0].event_argument, "Select$0");
        assert_eq!(rows[1].account.account_number, "222222");
        assert_eq!(rows[1].event_argument, "Select$1");
        assert_eq!(rows[1].account.past_due, Some(Money::from_cents(1200)));
    }

    #[test]
    fn dopostback_parse() {
        assert_eq!(
            parse_dopostback("javascript:__doPostBack('grid','Select$3')"),
            Some(("grid".to_string(), "Select$3".to_string()))
        );
    }
}
