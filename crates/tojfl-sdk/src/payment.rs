//! One-time payment flow (`OnlinePayment.aspx`).
//!
//! This is the public, pre-login "pay without an account" path. You enter a
//! customer number and account number; the portal validates them and reports
//! the amount due, then opens a hosted card-entry page in a pop-up. This module
//! automates the lookup and locates the hosted page; it deliberately does NOT
//! submit card details — that happens on the processor's own secure page.

use crate::client::Client;
use crate::dnn::{find_input_name_ending_with, find_name_by_id_ending_with, FormState};
use crate::error::{Error, Result};
use crate::model::{Money, PaymentQuote};
use crate::pages;
use scraper::{Html, Selector};

/// Validate a customer/account pair and read back the amount due.
pub fn quote(client: &Client, customer_number: &str, account_number: &str) -> Result<PaymentQuote> {
    validate_numbers(customer_number, account_number)?;

    let page = client.get_text(pages::ONLINE_PAYMENT)?;
    let mut form = FormState::from_html(&page);

    let cust_field = find_input_name_ending_with(&page, "txtCust")
        .ok_or_else(|| Error::MissingFormField("txtCust (customer number input)".into()))?;
    let acct_field = find_input_name_ending_with(&page, "txtAcct")
        .ok_or_else(|| Error::MissingFormField("txtAcct (account number input)".into()))?;

    form.set(&cust_field, customer_number);
    form.set(&acct_field, account_number);

    if let Some(go) = find_name_by_id_ending_with(&page, "GoButton")
        .or_else(|| find_input_name_ending_with(&page, "GoButton"))
    {
        // GoButton is an ASP.NET ImageButton: its click is fired by posting
        // `name.x`/`name.y` coordinate pairs, not a `name=value` field. Posting
        // it as a plain field silently no-ops the postback (the page just
        // re-renders unvalidated), so use the image-button convention.
        form.set(format!("{go}.x"), "5");
        form.set(format!("{go}.y"), "5");
    }

    let action = form
        .action
        .clone()
        .unwrap_or_else(|| pages::ONLINE_PAYMENT.to_string());
    let body = client.post_form_text(&action, &form.to_pairs())?;

    let hosted = find_hosted_payment_url(&body);
    // Prefer a labeled amount on the page; otherwise the hosted URL carries the
    // balance as an `Amount=` query parameter (the guest page doesn't render it
    // as a label on this deployment).
    let amount_due =
        read_amount_due(&body).or_else(|| hosted.as_deref().and_then(amount_from_hosted_url));
    let message = read_message(&body);
    // Heuristic validity: we found a balance or a hosted page, and no error msg.
    let valid = (amount_due.is_some() || hosted.is_some())
        && !message.as_deref().map(looks_like_error).unwrap_or(false);

    Ok(PaymentQuote {
        customer_number: customer_number.to_string(),
        account_number: account_number.to_string(),
        amount_due,
        account_name: None,
        hosted_payment_url: hosted,
        valid,
        message,
    })
}

fn validate_numbers(customer: &str, account: &str) -> Result<()> {
    if customer.is_empty() || !customer.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::invalid(
            "customer number must be all digits (7 digits, with leading zeros)",
        ));
    }
    if account.is_empty() || !account.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::invalid(
            "account number must be all digits (6 digits, with leading zeros)",
        ));
    }
    Ok(())
}

fn read_amount_due(html: &str) -> Option<Money> {
    let doc = Html::parse_document(html);
    // Look for a labeled amount-due value first.
    for sel in ["[id$=lblAmount]", "[id$=lblBalance]", "[id$=AmountDue]"] {
        if let Ok(s) = Selector::parse(sel) {
            if let Some(el) = doc.select(&s).next() {
                let t = el.text().collect::<String>();
                if let Some(m) = Money::parse(&t) {
                    return Some(m);
                }
            }
        }
    }
    None
}

fn read_message(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    for sel in [
        "[id$=lblMessage]",
        "[id$=lblError]",
        ".NormalRed",
        ".dnnFormMessage",
    ] {
        if let Ok(s) = Selector::parse(sel) {
            if let Some(el) = doc.select(&s).next() {
                let t = el.text().collect::<String>().trim().to_string();
                // The page always carries a static "turn off your pop-up
                // blocker" instruction; it is not a per-request message.
                if !t.is_empty() && !t.to_lowercase().contains("pop-up blocker") {
                    return Some(t);
                }
            }
        }
    }
    None
}

/// Read the balance from the hosted payment URL's `Amount=` query parameter
/// (e.g. `…&Amount=56.76`). The key match is case-insensitive.
fn amount_from_hosted_url(url: &str) -> Option<Money> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k.eq_ignore_ascii_case("amount") {
                return Money::parse(v);
            }
        }
    }
    None
}

fn find_hosted_payment_url(html: &str) -> Option<String> {
    // The portal opens the hosted page via window.open('...') or a redirect.
    if let Some(idx) = html.find("window.open(") {
        let rest = &html[idx + "window.open(".len()..];
        let trimmed = rest.trim_start_matches([' ', '\'', '"']);
        let end = trimmed.find(['\'', '"']).unwrap_or(0);
        if end > 0 {
            let url = &trimmed[..end];
            if url.starts_with("http") || url.starts_with('/') {
                return Some(url.to_string());
            }
        }
    }
    None
}

fn looks_like_error(msg: &str) -> bool {
    let m = msg.to_lowercase();
    [
        "invalid",
        "not found",
        "no account",
        "does not match",
        "error",
        "unable",
    ]
    .iter()
    .any(|k| m.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_digit_numbers() {
        assert!(validate_numbers("12a4567", "000000").is_err());
        assert!(validate_numbers("1234567", "00abcd").is_err());
        assert!(validate_numbers("1234567", "000000").is_ok());
    }

    #[test]
    fn extracts_window_open_url() {
        let html =
            r#"<script>window.open('https://pay.example.com/session/abc','_blank');</script>"#;
        assert_eq!(
            find_hosted_payment_url(html).as_deref(),
            Some("https://pay.example.com/session/abc")
        );
    }

    #[test]
    fn reads_amount_from_hosted_url() {
        let url = "https://paymentui.jupiter.fl.us/HP/Paymentus/ProcessPayment.aspx?PortalID=0&TextKey=TOJ123&UniqueID=abc&ExternalSource=10&Amount=56.76";
        assert_eq!(amount_from_hosted_url(url), Some(Money::from_cents(5676)));
        // Case-insensitive key; tolerates a param without a value.
        assert_eq!(
            amount_from_hosted_url("https://x/y?flag&amount=1,234.50"),
            Some(Money::from_cents(123450))
        );
        // No amount param, or no query string → None.
        assert_eq!(amount_from_hosted_url("https://x/y?PortalID=0"), None);
        assert_eq!(amount_from_hosted_url("https://x/y"), None);
    }

    #[test]
    fn error_message_detection() {
        assert!(looks_like_error("Account not found"));
        assert!(!looks_like_error("Amount due: $50.00"));
    }
}
