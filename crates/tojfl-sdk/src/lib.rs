//! `tojfl-sdk` — a client for the Town of Jupiter, FL utility billing portal.
//!
//! The portal is a Harris **eCARE** application hosted on **DotNetNuke (DNN)**,
//! an ASP.NET WebForms stack. There is no documented API, so this SDK drives
//! the same pages a browser would: it performs the DNN forms-authentication
//! postback to log in, persists the resulting session cookie, and scrapes the
//! eCARE data pages (billing history, usage, transactions, profile) into typed
//! models. It also automates the public one-time-payment lookup.
//!
//! ## Layers
//! - [`client::Client`] — cookie-aware HTTP with manual redirect handling.
//! - [`dnn`] — WebForms hidden-field extraction and postback assembly.
//! - [`auth`], [`scrape`], [`payment`] — flows built on the above.
//! - [`Portal`] — the ergonomic facade the CLI uses.
//!
//! ## Privacy
//! Credentials come from flags, env, the OS keychain, or a local config file —
//! never from anything in this repository. Sessions are stored under the OS
//! state dir with owner-only permissions.

pub mod accounts;
pub mod auth;
pub mod client;
pub mod config;
pub mod date;
pub mod dnn;
pub mod error;
pub mod model;
pub mod pages;
pub mod payment;
pub mod scrape;
pub mod session;
pub mod usage;

use std::cell::RefCell;
use std::time::Duration;

pub use error::{Error, Result};
pub use model::{
    Account, Bill, Contact, Enrollment, LinkedAccount, MeterRead, Money, PaymentQuote, Profile,
    ServiceInfo, Snapshot, Summary, Transaction, TransactionSummary, UsageComparison, UsageRecord,
    UsageStats,
};
pub use usage::CompareTarget;

use client::Client;
pub use config::Config;
use session::Session;

/// The ergonomic entry point: an (optionally authenticated) portal handle.
pub struct Portal {
    client: Client,
    base_url: String,
    /// Cached display username. Behind a `RefCell` so a silent `&self` session
    /// refresh can update it if it re-authenticates as a different user.
    username: RefCell<Option<String>>,
    /// Whether a saved session was loaded (cookies seeded) for this base URL.
    /// A cheap, reliable gate: with no session we are definitely logged out,
    /// so we can reject authenticated calls without a network round-trip (and
    /// without risking a scrape of a public page that merely looks empty).
    has_session: bool,
    /// Account to activate before account-scoped reads (from `--account` /
    /// config `default_account`). `None` uses whatever account is active.
    active_account: Option<String>,
    /// The config this handle was built from — kept so an expired session can
    /// be silently re-authenticated from stored credentials.
    cfg: Config,
}

/// Whether an expired/absent session should be silently refreshed: only when a
/// session previously existed (so a deliberate `logout` stays logged out) and
/// auto-login hasn't been disabled in config. Credential availability is checked
/// separately at refresh time.
fn should_refresh(has_session: bool, auto_login: Option<bool>) -> bool {
    has_session && auto_login.unwrap_or(true)
}

/// Whether a balance is owed *and* the due date has already passed as of
/// `today`. An unparseable/absent due date, or a zero/credit balance, is not
/// past due.
fn is_past_due(balance: Option<Money>, due_date: Option<&str>, today: date::Ymd) -> bool {
    let owed = balance.map(|b| b.cents > 0).unwrap_or(false);
    let overdue = due_date
        .and_then(date::parse)
        .map(|due| today > due)
        .unwrap_or(false);
    owed && overdue
}

impl Portal {
    /// Build a portal handle from config, seeding any saved session cookies so
    /// authenticated calls work immediately if a session is still valid.
    pub fn new(cfg: &Config) -> Result<Portal> {
        let base_url = cfg
            .base_url
            .clone()
            .unwrap_or_else(|| pages::BASE_URL.to_string());
        let timeout = Duration::from_secs(cfg.timeout_secs.unwrap_or(30));
        let client = Client::new(&base_url, timeout)?;

        let mut username = cfg.username.clone();
        let mut has_session = false;
        if let Some(sess) = Session::load()? {
            if sess.base_url == base_url && !sess.cookies.is_empty() {
                client.seed_cookies(&sess.cookies);
                username = username.or(sess.username);
                has_session = true;
            }
        }
        Ok(Portal {
            client,
            base_url,
            username: RefCell::new(username),
            has_session,
            active_account: cfg.default_account.clone(),
            cfg: cfg.clone(),
        })
    }

    /// Base URL this handle talks to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The username associated with the current session/config, if any.
    pub fn username(&self) -> Option<String> {
        self.username.borrow().clone()
    }

    // --- authentication ---------------------------------------------------

    /// Log in and persist the session. Returns the saved-session path.
    pub fn login(&mut self, username: &str, password: &str) -> Result<std::path::PathBuf> {
        auth::login(&self.client, username, password)?;
        *self.username.borrow_mut() = Some(username.to_string());
        self.has_session = true;
        let sess = Session {
            cookies: self.client.snapshot_cookies(),
            base_url: self.base_url.clone(),
            username: Some(username.to_string()),
            saved_at: Session::now(),
        };
        sess.save()
    }

    /// Whether the current (persisted) session is still authenticated.
    ///
    /// Short-circuits to `false` with no network call when no session is loaded;
    /// otherwise confirms with the portal that the cookies are still good.
    pub fn is_authenticated(&self) -> Result<bool> {
        if !self.has_session {
            return Ok(false);
        }
        auth::verify(&self.client)
    }

    /// Clear the saved session (local logout).
    pub fn logout(&self) -> Result<()> {
        Session::clear()
    }

    fn ensure_authenticated(&self) -> Result<()> {
        if self.is_authenticated()? {
            return Ok(());
        }
        // Session absent or expired: try a silent re-login from stored
        // credentials so long-lived callers (dashboards, cron) keep working
        // across the portal's short session timeout.
        if self.refresh_session()? {
            return Ok(());
        }
        Err(Error::NotAuthenticated)
    }

    /// Attempt to silently re-establish a session using stored credentials.
    /// Returns `Ok(true)` if a fresh session is now in place. Does nothing (and
    /// returns `Ok(false)`) when auto-login is disabled, no prior session
    /// existed, or no credentials are available to authenticate with.
    fn refresh_session(&self) -> Result<bool> {
        if !should_refresh(self.has_session, self.cfg.auto_login) {
            return Ok(false);
        }
        let creds = match config::credentials(&self.cfg, None, None) {
            Ok(c) => c,
            Err(_) => return Ok(false), // nothing stored to re-authenticate with
        };
        auth::login(&self.client, &creds.username, &creds.password)?;
        // Reflect the refreshed identity so `username()` doesn't go stale.
        *self.username.borrow_mut() = Some(creds.username.clone());
        // Persist the refreshed cookies so the next process starts authenticated.
        let sess = Session {
            cookies: self.client.snapshot_cookies(),
            base_url: self.base_url.clone(),
            username: Some(creds.username),
            saved_at: Session::now(),
        };
        let _ = sess.save();
        Ok(true)
    }

    /// Gate for account-scoped reads: require a session, then activate the
    /// requested account (if any) so the following page reports its data.
    fn ready(&self) -> Result<()> {
        self.ensure_authenticated()?;
        if let Some(acct) = &self.active_account {
            if !accounts::select(&self.client, acct)? {
                return Err(accounts::not_linked(
                    acct,
                    &accounts::numbers(&self.client)?,
                ));
            }
        }
        Ok(())
    }

    // --- linked accounts --------------------------------------------------

    /// List the accounts linked to the current login.
    pub fn list_accounts(&self) -> Result<Vec<LinkedAccount>> {
        self.ensure_authenticated()?;
        accounts::list(&self.client)
    }

    /// Activate a specific linked account for the session.
    pub fn select_account(&self, account_number: &str) -> Result<()> {
        self.ensure_authenticated()?;
        if accounts::select(&self.client, account_number)? {
            Ok(())
        } else {
            Err(accounts::not_linked(
                account_number,
                &accounts::numbers(&self.client)?,
            ))
        }
    }

    // --- authenticated data ----------------------------------------------

    /// Account summary from the post-login landing page (balance, due date, ...).
    pub fn account_summary(&self) -> Result<Account> {
        self.ready()?;
        self.fetch_account()
    }

    /// An at-a-glance overview (account summary + service snapshot + enrollment),
    /// activating the account and authenticating once for the whole set.
    pub fn summary(&self) -> Result<Summary> {
        self.ready()?;
        Ok(Summary {
            account: self.fetch_account()?,
            service: self.fetch_service()?,
            enrollment: self.fetch_enrollment()?,
        })
    }

    /// A compact machine-readable snapshot for dashboards: balance, due/past-due,
    /// last payment, usage stats, and ledger totals — one authenticated call.
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.ready()?;
        self.build_snapshot()
    }

    /// A snapshot for every linked account. Authenticates once, then activates
    /// and snapshots each account in turn. Ignores `--account`/`default_account`
    /// (the point is all of them); a single-account login yields one entry.
    pub fn snapshot_all(&self) -> Result<Vec<Snapshot>> {
        self.ensure_authenticated()?;
        let accounts = accounts::list(&self.client)?;
        let mut out = Vec::with_capacity(accounts.len());
        for a in &accounts {
            if !accounts::select(&self.client, &a.account_number)? {
                return Err(accounts::not_linked(
                    &a.account_number,
                    &accounts::numbers(&self.client)?,
                ));
            }
            out.push(self.build_snapshot()?);
        }
        Ok(out)
    }

    /// Build a snapshot of the currently-active account. Assumes the caller has
    /// already authenticated and activated the desired account.
    fn build_snapshot(&self) -> Result<Snapshot> {
        let account = self.fetch_account()?;
        let service = self.fetch_service()?;
        let usage_records = usage::fetch(&self.client)?;
        let txns = scrape::parse_transactions(&self.client.get_text(pages::TRANSACTION_HISTORY)?);
        Ok(Snapshot {
            account: (!account.account_number.is_empty()).then(|| account.account_number.clone()),
            past_due: is_past_due(account.balance, account.due_date.as_deref(), date::today()),
            balance: account.balance,
            due_date: account.due_date,
            last_payment_amount: service.last_payment_amount,
            last_payment_date: service.last_payment_date,
            usage: UsageStats::from_records(&usage_records),
            ledger: TransactionSummary::from_transactions(&txns),
        })
    }

    // Fetch helpers assume the caller already ran `ready()`, so `summary()` can
    // compose several without repeating the auth/account-activation round-trips.
    fn fetch_account(&self) -> Result<Account> {
        let html = self.client.get_text(pages::HOME)?;
        Ok(scrape::parse_account_summary(&html))
    }

    fn fetch_service(&self) -> Result<ServiceInfo> {
        let html = self.client.get_text(pages::SERVICE_INFORMATION)?;
        Ok(scrape::parse_service_info(&html))
    }

    fn fetch_enrollment(&self) -> Result<Enrollment> {
        let ebill_html = self.client.get_text(pages::EBILL_REGISTRATION)?;
        let account_number = scrape::account_number_from_page(&ebill_html);
        let (paperless, ebill_email) = match &account_number {
            Some(a) => scrape::parse_ebill(&ebill_html, a),
            None => (None, None),
        };
        let autopay_html = self.client.get_text(pages::AUTOPAY)?;
        let (autopay_plan, autopay_draw_day, autopay_draw_amount) =
            scrape::parse_autopay(&autopay_html);
        Ok(Enrollment {
            account_number,
            paperless,
            ebill_email,
            autopay: Some(autopay_plan.is_some()),
            autopay_plan,
            autopay_draw_day,
            autopay_draw_amount,
        })
    }

    /// Billing history (statements).
    pub fn bills(&self) -> Result<Vec<Bill>> {
        self.ready()?;
        let html = self.client.get_text(pages::BILLING_HISTORY)?;
        Ok(scrape::parse_bills(&html))
    }

    /// Metered usage / consumption history (submits the service form if needed).
    pub fn usage(&self) -> Result<Vec<UsageRecord>> {
        self.ready()?;
        usage::fetch(&self.client)
    }

    /// Compare your consumption to a street/region/city average.
    pub fn usage_compare(&self, target: CompareTarget) -> Result<Vec<UsageComparison>> {
        self.ready()?;
        usage::compare(&self.client, target)
    }

    /// Meter-reading history (date, meter #, prev/current read, consumption).
    pub fn meter_reads(&self) -> Result<Vec<MeterRead>> {
        self.ready()?;
        usage::fetch_meter_reads(&self.client)
    }

    /// Service snapshot for the active account (last read/bill/payment).
    pub fn service_info(&self) -> Result<ServiceInfo> {
        self.ready()?;
        self.fetch_service()
    }

    /// Paperless (eBill) and autopay enrollment status for the active account.
    pub fn enrollment(&self) -> Result<Enrollment> {
        self.ready()?;
        self.fetch_enrollment()
    }

    /// Download a bill's statement PDF bytes. Errors if the bill carries no
    /// eBill link, or if the portal returns something that isn't a PDF (e.g. an
    /// expired session bouncing to a login page).
    pub fn download_bill(&self, bill: &Bill) -> Result<Vec<u8>> {
        self.ready()?;
        let url = bill.document_url.as_deref().ok_or_else(|| {
            Error::invalid("this statement has no downloadable PDF (no Web Bill link)")
        })?;
        let bytes = self.client.get_bytes(url)?;
        if !bytes.starts_with(b"%PDF") {
            return Err(Error::Portal(
                "expected a PDF but got something else (session may have expired)".into(),
            ));
        }
        Ok(bytes)
    }

    /// Ledger transaction history (charges, payments, adjustments).
    pub fn transactions(&self) -> Result<Vec<Transaction>> {
        self.ready()?;
        let html = self.client.get_text(pages::TRANSACTION_HISTORY)?;
        Ok(scrape::parse_transactions(&html))
    }

    /// Account holder profile (from the DNN ManageUsers "Change Profile" page).
    /// Profile is per-login, so it isn't account-scoped.
    pub fn profile(&self) -> Result<Profile> {
        self.ensure_authenticated()?;
        let html = self.client.get_text(pages::CHANGE_PROFILE)?;
        Ok(scrape::parse_profile(&html))
    }

    /// Convenience: current balance due, if the summary exposes it.
    pub fn balance(&self) -> Result<Option<Money>> {
        Ok(self.account_summary()?.balance)
    }

    // --- public payment flow ---------------------------------------------

    /// Validate a customer/account pair and read the amount due (no login).
    ///
    /// This runs through a fresh **guest** client with no session cookies:
    /// `OnlinePayment.aspx` 302-redirects an authenticated session to an
    /// "Access Denied" page (the public one-time-payment form is for guests),
    /// so reusing the logged-in client would break the lookup.
    pub fn payment_quote(
        &self,
        customer_number: &str,
        account_number: &str,
    ) -> Result<PaymentQuote> {
        payment::quote(&self.guest_client()?, customer_number, account_number)
    }

    /// A fresh client with no seeded session cookies, for public/guest flows.
    fn guest_client(&self) -> Result<Client> {
        let timeout = Duration::from_secs(self.cfg.timeout_secs.unwrap_or(30));
        Client::new(&self.base_url, timeout)
    }

    /// Static contact / service information (no network call).
    pub fn contact(&self) -> Contact {
        Contact::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{is_past_due, should_refresh};
    use crate::model::Money;

    #[test]
    fn past_due_needs_both_a_balance_and_a_passed_due_date() {
        let today = (2026, 7, 17);
        let owed = Some(Money::from_cents(5676));
        // Owed + due date already passed → past due.
        assert!(is_past_due(owed, Some("6/1/2026"), today));
        // Owed but due date is still in the future → not past due.
        assert!(!is_past_due(owed, Some("8/5/2026"), today));
        // Nothing owed → never past due, even with an old due date.
        assert!(!is_past_due(Some(Money::ZERO), Some("6/1/2026"), today));
        assert!(!is_past_due(
            Some(Money::from_cents(-100)),
            Some("6/1/2026"),
            today
        ));
        // Missing/unparseable due date → not past due.
        assert!(!is_past_due(owed, None, today));
        assert!(!is_past_due(owed, Some("n/a"), today));
    }

    #[test]
    fn refresh_gate_respects_session_and_toggle() {
        // Default (None) is enabled, but only with a prior session.
        assert!(
            should_refresh(true, None),
            "expired session refreshes by default"
        );
        assert!(
            !should_refresh(false, None),
            "no prior session (e.g. after logout) must not auto-login"
        );
        // Explicit opt-out wins even with a session present.
        assert!(!should_refresh(true, Some(false)));
        assert!(should_refresh(true, Some(true)));
        assert!(!should_refresh(false, Some(true)));
    }
}
