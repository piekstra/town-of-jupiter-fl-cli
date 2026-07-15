//! Data types returned by the portal, normalized for programmatic use.
//!
//! Money is represented as [`Money`] (a decimal count of cents) rather than a
//! float so JSON output is exact. Volumes and dates keep their portal units.

use serde::{Deserialize, Serialize};

/// A monetary amount, stored as an integer number of cents to avoid float drift.
///
/// The portal renders dollars like `$123.45` and `($12.00)` (parentheses for
/// credits); [`Money::parse`] understands both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Money {
    /// Signed number of cents. Negative values are credits.
    pub cents: i64,
}

impl Money {
    pub const ZERO: Money = Money { cents: 0 };

    pub fn from_cents(cents: i64) -> Self {
        Money { cents }
    }

    /// Parse a portal-rendered money string such as `"$1,234.56"`, `"12.00"`,
    /// or `"($45.00)"` (a credit). Returns `None` if no number is present.
    pub fn parse(raw: &str) -> Option<Money> {
        let t = raw.trim();
        if t.is_empty() {
            return None;
        }
        let negative = t.starts_with('(') && t.ends_with(')') || t.starts_with('-');
        let mut digits = String::new();
        for ch in t.chars() {
            if ch.is_ascii_digit() || ch == '.' {
                digits.push(ch);
            }
        }
        if digits.is_empty() {
            return None;
        }
        let value: f64 = digits.parse().ok()?;
        let cents = (value * 100.0).round() as i64;
        Some(Money {
            cents: if negative { -cents } else { cents },
        })
    }

    pub fn dollars(&self) -> f64 {
        self.cents as f64 / 100.0
    }
}

impl std::fmt::Display for Money {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let neg = self.cents < 0;
        let abs = self.cents.unsigned_abs();
        let dollars = abs / 100;
        let cents = abs % 100;
        // Group thousands in the dollar portion.
        let ds = dollars.to_string();
        let mut grouped = String::new();
        for (i, ch) in ds.chars().enumerate() {
            if i > 0 && (ds.len() - i) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(ch);
        }
        if neg {
            write!(f, "(${grouped}.{cents:02})")
        } else {
            write!(f, "${grouped}.{cents:02}")
        }
    }
}

/// A utility account tied to the logged-in login.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Account {
    /// 7-digit customer number (the person/login).
    pub customer_number: String,
    /// 6-digit account number (the specific service/premise).
    pub account_number: String,
    /// Human name on the account, if the portal exposes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Service address for the account, if exposed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_address: Option<String>,
    /// Current balance due.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<Money>,
    /// Bill due date as rendered by the portal (kept as text; formats vary).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    /// Whether this account is enrolled in paperless/eBill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paperless: Option<bool>,
    /// Whether this account is enrolled in autopay/bank draft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopay: Option<bool>,
}

/// One account linked to the current login, as listed on `ListAccounts.aspx`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinkedAccount {
    pub account_number: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub past_due: Option<Money>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<Money>,
}

/// One bill/statement in the billing history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bill {
    /// Statement/bill date as text.
    pub date: String,
    /// Amount billed on this statement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<Money>,
    /// Balance after this statement, if shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<Money>,
    /// Due date for this statement, if shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    /// Bill/document identifier used to fetch the PDF, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    /// Absolute URL of the statement PDF (the grid's "Web Bill" / eBill link).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_url: Option<String>,
    /// Any additional labeled columns from the row, preserved verbatim.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty", default)]
    pub extra: std::collections::BTreeMap<String, String>,
}

/// One period of metered consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Billing/read period label as text (e.g. "Jun 2026").
    pub period: String,
    /// Consumption quantity as a number, if parseable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
    /// Unit of measure (e.g. "gallons", "kgal", "CCF").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Number of days in the period, if shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<u32>,
    /// Average per day, if shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_per_day: Option<f64>,
    /// Any additional labeled columns, preserved verbatim.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty", default)]
    pub extra: std::collections::BTreeMap<String, String>,
}

/// One period comparing your consumption to a group average (street/region/city).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageComparison {
    /// Read period label as text.
    pub period: String,
    /// Your consumption for the period, if parseable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumption: Option<f64>,
    /// The group's average consumption for the period, if parseable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average: Option<f64>,
    /// Unit of measure, if shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// A financial transaction (charge, payment, adjustment) on the ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Transaction date as text.
    pub date: String,
    /// Free-text description/type as shown by the portal.
    pub description: String,
    /// Signed amount (payments/credits negative, charges positive) when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<Money>,
    /// Running balance after the transaction, if shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<Money>,
    /// Any additional labeled columns, preserved verbatim.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty", default)]
    pub extra: std::collections::BTreeMap<String, String>,
}

/// The account holder's profile as exposed by the DNN user profile page.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Additional labeled profile fields, preserved verbatim.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty", default)]
    pub extra: std::collections::BTreeMap<String, String>,
}

/// Result of a one-time-payment account lookup (the public, pre-login flow).
///
/// The portal validates the customer+account pair and returns the amount due
/// before handing off to a hosted card-entry page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentQuote {
    pub customer_number: String,
    pub account_number: String,
    /// Amount the portal reports as due, if it could be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_due: Option<Money>,
    /// Name/address shown on the lookup, if any (helps confirm the right acct).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_name: Option<String>,
    /// The URL of the hosted payment page the portal would open, if we can
    /// determine it. Card entry happens there, off this CLI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hosted_payment_url: Option<String>,
    /// Whether the lookup was accepted by the portal.
    pub valid: bool,
    /// Any message the portal returned (e.g. "account not found").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Static contact / service information (no network call needed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub department: String,
    pub phone: String,
    pub portal: String,
    pub utilities_home: String,
    pub rates_url: String,
    pub bank_draft_form_url: String,
}

impl Default for Contact {
    fn default() -> Self {
        Contact {
            department: "Town of Jupiter Utilities".to_string(),
            phone: "(561) 741-2300".to_string(),
            portal: "https://utilitybill.jupiter.fl.us".to_string(),
            utilities_home: "https://www.jupiter.fl.us/water".to_string(),
            rates_url: "https://www.jupiter.fl.us/water".to_string(),
            bank_draft_form_url: "https://utilitybill.jupiter.fl.us".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_parses_plain() {
        assert_eq!(Money::parse("$123.45"), Some(Money::from_cents(12345)));
        assert_eq!(Money::parse("12.00"), Some(Money::from_cents(1200)));
        assert_eq!(Money::parse("$1,234.56"), Some(Money::from_cents(123456)));
    }

    #[test]
    fn money_parses_credit() {
        assert_eq!(Money::parse("($45.00)"), Some(Money::from_cents(-4500)));
        assert_eq!(Money::parse("-$5.00"), Some(Money::from_cents(-500)));
    }

    #[test]
    fn money_parses_none() {
        assert_eq!(Money::parse(""), None);
        assert_eq!(Money::parse("N/A"), None);
    }

    #[test]
    fn money_display_roundish() {
        assert_eq!(Money::from_cents(12345).to_string(), "$123.45");
        assert_eq!(Money::from_cents(123456).to_string(), "$1,234.56");
        assert_eq!(Money::from_cents(-4500).to_string(), "($45.00)");
    }
}
