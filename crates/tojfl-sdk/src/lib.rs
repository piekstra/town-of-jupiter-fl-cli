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
pub mod dnn;
pub mod error;
pub mod model;
pub mod pages;
pub mod payment;
pub mod scrape;
pub mod session;
pub mod usage;

use std::time::Duration;

pub use error::{Error, Result};
pub use model::{
    Account, Bill, Contact, Enrollment, LinkedAccount, Money, PaymentQuote, Profile, ServiceInfo,
    Transaction, UsageComparison, UsageRecord,
};
pub use usage::CompareTarget;

use client::Client;
pub use config::Config;
use session::Session;

/// The ergonomic entry point: an (optionally authenticated) portal handle.
pub struct Portal {
    client: Client,
    base_url: String,
    username: Option<String>,
    /// Whether a saved session was loaded (cookies seeded) for this base URL.
    /// A cheap, reliable gate: with no session we are definitely logged out,
    /// so we can reject authenticated calls without a network round-trip (and
    /// without risking a scrape of a public page that merely looks empty).
    has_session: bool,
    /// Account to activate before account-scoped reads (from `--account` /
    /// config `default_account`). `None` uses whatever account is active.
    active_account: Option<String>,
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
            username,
            has_session,
            active_account: cfg.default_account.clone(),
        })
    }

    /// Base URL this handle talks to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The username associated with the current session/config, if any.
    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    // --- authentication ---------------------------------------------------

    /// Log in and persist the session. Returns the saved-session path.
    pub fn login(&mut self, username: &str, password: &str) -> Result<std::path::PathBuf> {
        auth::login(&self.client, username, password)?;
        self.username = Some(username.to_string());
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
            Ok(())
        } else {
            Err(Error::NotAuthenticated)
        }
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
        let html = self.client.get_text(pages::HOME)?;
        Ok(scrape::parse_account_summary(&html))
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

    /// Service snapshot for the active account (last read/bill/payment).
    pub fn service_info(&self) -> Result<ServiceInfo> {
        self.ready()?;
        let html = self.client.get_text(pages::SERVICE_INFORMATION)?;
        Ok(scrape::parse_service_info(&html))
    }

    /// Paperless (eBill) and autopay enrollment status for the active account.
    pub fn enrollment(&self) -> Result<Enrollment> {
        self.ready()?;
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
    pub fn payment_quote(
        &self,
        customer_number: &str,
        account_number: &str,
    ) -> Result<PaymentQuote> {
        payment::quote(&self.client, customer_number, account_number)
    }

    /// Static contact / service information (no network call).
    pub fn contact(&self) -> Contact {
        Contact::default()
    }
}
