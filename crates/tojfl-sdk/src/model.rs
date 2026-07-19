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

/// An at-a-glance overview of the active account, composed from several pages.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Summary {
    /// Account summary (account #, balance, due date).
    pub account: Account,
    /// Service snapshot (last read/bill/payment).
    pub service: ServiceInfo,
    /// Paperless + autopay enrollment.
    pub enrollment: Enrollment,
}

/// A compact, machine-readable snapshot of the active account — everything a
/// dashboard needs in one payload (balance, due/past-due, last payment, usage
/// stats, and ledger totals). Built by `Portal::snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Active account number, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Account holder name (from the linked-accounts list), if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Service address / premise (from the linked-accounts list), if known —
    /// identifies *which* account this is, not just its number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_address: Option<String>,
    /// Current balance due.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<Money>,
    /// Bill due date as text (formats vary; keep the portal's rendering).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    /// Whether a balance is owed and the due date has already passed.
    pub past_due: bool,
    /// Total of payments still marked "Pending" in the ledger (positive
    /// magnitude), if any. The balance doesn't reflect these until they clear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_payments: Option<Money>,
    /// Balance minus pending payments — what you'll owe once they clear. Only
    /// set when there are pending payments (otherwise it equals `balance`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_balance: Option<Money>,
    /// Most recent payment amount, if the portal shows one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_payment_amount: Option<Money>,
    /// Most recent payment date, if shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_payment_date: Option<String>,
    /// Consumption stats over the usage history (`None` if no numeric periods).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageStats>,
    /// Ledger totals (charges, payments/credits, net).
    pub ledger: TransactionSummary,
}

/// Paperless (eBill) and autopay enrollment for the active account.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Enrollment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_number: Option<String>,
    /// Enrolled in paperless / eBill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paperless: Option<bool>,
    /// The eBill notification email, if enrolled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ebill_email: Option<String>,
    /// Enrolled in autopay / bank draft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopay: Option<bool>,
    /// Autopay plan description (e.g. "Autopay - Credit Card").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopay_plan: Option<String>,
    /// Autopay draw day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopay_draw_day: Option<String>,
    /// Autopay draw amount.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopay_draw_amount: Option<Money>,
}

/// Service snapshot from `ServiceInformation.aspx` for the active account.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_bill_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_bill_amount: Option<Money>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_payment_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_payment_amount: Option<Money>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_payment_date: Option<String>,
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
    /// Bill total for this statement (balance forward + current charges).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<Money>,
    /// New charges billed this period (the grid's "Current Bill" column).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_charges: Option<Money>,
    /// Balance carried forward from the prior statement (the grid's "Balance
    /// Forward" column) — NOT a running balance after this statement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_forward: Option<Money>,
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

/// Summary statistics over a set of [`UsageRecord`]s (the periods that carry a
/// parseable quantity). `None` from [`UsageStats::from_records`] means no period
/// had a usable number.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    /// How many periods contributed a quantity.
    pub periods: usize,
    /// Unit of measure, taken from the records (e.g. "100 Gallons"), if shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Sum of consumption across the periods.
    pub total: f64,
    /// Mean consumption per period.
    pub average: f64,
    /// Lowest single-period consumption, and the period it occurred in.
    pub min: f64,
    pub min_period: String,
    /// Highest single-period consumption, and the period it occurred in.
    pub max: f64,
    pub max_period: String,
}

impl UsageStats {
    /// Compute stats over the records that carry a quantity. Returns `None` when
    /// none do (so callers can report "no data" rather than a bogus zero-row).
    pub fn from_records(records: &[UsageRecord]) -> Option<UsageStats> {
        let mut periods = 0usize;
        let mut total = 0.0;
        let mut unit = None;
        let mut min = (f64::INFINITY, String::new());
        let mut max = (f64::NEG_INFINITY, String::new());
        for r in records {
            let Some(q) = r.quantity else { continue };
            periods += 1;
            total += q;
            if unit.is_none() {
                unit = r.unit.clone();
            }
            if q < min.0 {
                min = (q, r.period.clone());
            }
            if q > max.0 {
                max = (q, r.period.clone());
            }
        }
        if periods == 0 {
            return None;
        }
        Some(UsageStats {
            periods,
            unit,
            total,
            average: total / periods as f64,
            min: min.0,
            min_period: min.1,
            max: max.0,
            max_period: max.1,
        })
    }
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

/// One meter read from `MeterReadingHistory.aspx`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeterRead {
    /// Read date as text.
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_read: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_read: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<u32>,
    /// e.g. "Actual Read" / "Estimated".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumption: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average: Option<f64>,
    /// Any additional labeled columns, preserved verbatim.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty", default)]
    pub extra: std::collections::BTreeMap<String, String>,
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

impl Transaction {
    /// Whether this ledger entry is still marked "Pending" by the portal
    /// (e.g. a just-submitted payment: `"Payment - Thank You (Pending)"`).
    pub fn is_pending(&self) -> bool {
        self.description.to_lowercase().contains("pending")
    }
}

/// Total of ledger entries that are still-pending **payments** (a pending
/// credit — negative amount), as a positive magnitude. Zero when there are none.
/// The account balance doesn't reflect these until they clear.
pub fn pending_payment_total(txns: &[Transaction]) -> Money {
    let cents: i64 = txns
        .iter()
        .filter(|t| t.is_pending())
        .filter_map(|t| t.amount)
        .map(|m| m.cents)
        .filter(|c| *c < 0)
        .sum();
    Money::from_cents(-cents)
}

/// Totals over a set of ledger [`Transaction`]s. The portal signs amounts with
/// charges/debits positive and payments/credits negative, so we split on sign.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionSummary {
    /// Transactions counted (those carrying an amount).
    pub count: usize,
    /// Sum of positive amounts (bills, fees).
    pub charges: Money,
    /// Sum of the negative amounts as a positive magnitude (payments + credits).
    pub payments: Money,
    /// Net change over the set (`charges − payments`); positive means you were
    /// billed more than was paid/credited in the window.
    pub net: Money,
}

impl TransactionSummary {
    /// Sum a set of transactions into charge/payment/net totals. Transactions
    /// without an amount are ignored; an empty set yields all-zero totals
    /// (sums have a natural zero, unlike [`UsageStats`]'s min/max).
    pub fn from_transactions(txns: &[Transaction]) -> TransactionSummary {
        let mut count = 0usize;
        let mut charge_cents = 0i64;
        let mut credit_cents = 0i64; // running sum of the negative amounts (<= 0)
        for t in txns {
            let Some(amt) = t.amount else { continue };
            count += 1;
            if amt.cents >= 0 {
                charge_cents += amt.cents;
            } else {
                credit_cents += amt.cents;
            }
        }
        TransactionSummary {
            count,
            charges: Money::from_cents(charge_cents),
            payments: Money::from_cents(-credit_cents),
            net: Money::from_cents(charge_cents + credit_cents),
        }
    }
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
    pub email: String,
    pub hours: String,
    pub address: String,
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
            email: "winfo@jupiter.fl.us".to_string(),
            hours: "Mon–Thu 7:30am–5:30pm, Fri 8:00am–5:00pm".to_string(),
            address: "210 Military Trail, Jupiter, FL 33458".to_string(),
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

    fn usage(period: &str, quantity: Option<f64>, unit: Option<&str>) -> UsageRecord {
        UsageRecord {
            period: period.into(),
            quantity,
            unit: unit.map(Into::into),
            days: None,
            average_per_day: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn usage_stats_aggregates_periods() {
        let records = vec![
            usage("Apr 2026", Some(6.4), Some("100 Gallons")),
            usage("May 2026", Some(11.8), Some("100 Gallons")),
            usage("Jun 2026", Some(9.9), Some("100 Gallons")),
        ];
        let s = UsageStats::from_records(&records).expect("has data");
        assert_eq!(s.periods, 3);
        assert_eq!(s.unit.as_deref(), Some("100 Gallons"));
        assert!((s.total - 28.1).abs() < 1e-9);
        assert!((s.average - 28.1 / 3.0).abs() < 1e-9);
        assert_eq!((s.min, s.min_period.as_str()), (6.4, "Apr 2026"));
        assert_eq!((s.max, s.max_period.as_str()), (11.8, "May 2026"));
    }

    #[test]
    fn usage_stats_skips_periods_without_a_quantity() {
        // Rows lacking a parseable quantity don't count toward periods/total.
        let records = vec![
            usage("Apr 2026", None, None),
            usage("May 2026", Some(5.0), Some("kgal")),
        ];
        let s = UsageStats::from_records(&records).expect("has one data row");
        assert_eq!(s.periods, 1);
        assert_eq!(s.total, 5.0);
        assert_eq!(s.unit.as_deref(), Some("kgal"));
    }

    #[test]
    fn usage_stats_is_none_when_no_quantities() {
        let records = vec![usage("Apr 2026", None, None)];
        assert!(UsageStats::from_records(&records).is_none());
        assert!(UsageStats::from_records(&[]).is_none());
    }

    fn txn(desc: &str, cents: Option<i64>) -> Transaction {
        Transaction {
            date: "Jun 16, 2026".into(),
            description: desc.into(),
            amount: cents.map(Money::from_cents),
            balance: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn transaction_summary_splits_charges_and_payments() {
        // A bill, a payment (credit), a late fee, and its reversal.
        let txns = vec![
            txn("Cycle Bill", Some(5676)),
            txn("Payment - Thank You", Some(-5649)),
            txn("Late Notice", Some(500)),
            txn("Late Notice", Some(-500)),
            txn("Pending", None), // no amount → ignored
        ];
        let s = TransactionSummary::from_transactions(&txns);
        assert_eq!(s.count, 4, "the amount-less row isn't counted");
        assert_eq!(s.charges, Money::from_cents(6176)); // 5676 + 500
        assert_eq!(s.payments, Money::from_cents(6149)); // |−5649| + |−500|, positive
        assert_eq!(s.net, Money::from_cents(27)); // 6176 − 6149
    }

    #[test]
    fn pending_payment_total_sums_pending_credits_only() {
        let txns = vec![
            txn("Payment - Thank You (Pending)", Some(-5676)), // pending payment
            txn("Payment - Thank You", Some(-5649)),           // cleared payment, ignored
            txn("Cycle Bill (Pending)", Some(6000)),           // a pending charge, not a payment
            txn("Pending Adjustment", None),                   // no amount, ignored
        ];
        // Only the pending credit counts, as a positive magnitude.
        assert_eq!(pending_payment_total(&txns), Money::from_cents(5676));
        // No pending rows → zero.
        assert_eq!(
            pending_payment_total(&[txn("Payment - Thank You", Some(-100))]),
            Money::ZERO
        );
    }

    #[test]
    fn transaction_summary_empty_is_zero() {
        let s = TransactionSummary::from_transactions(&[]);
        assert_eq!(s.count, 0);
        assert_eq!(s.charges, Money::ZERO);
        assert_eq!(s.payments, Money::ZERO);
        assert_eq!(s.net, Money::ZERO);
    }

    #[test]
    fn contact_default_carries_verified_details() {
        let c = Contact::default();
        assert_eq!(c.email, "winfo@jupiter.fl.us");
        assert!(c.hours.contains("Mon") && c.hours.contains("Fri"));
        assert!(c.address.contains("Military Trail") && c.address.contains("33458"));
    }

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
