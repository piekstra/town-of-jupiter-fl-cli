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
pub use model::{Account, Bill, Contact, Money, PaymentQuote, Profile, Transaction, UsageRecord};

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

    // --- authenticated data ----------------------------------------------

    /// Account summary from the post-login landing page (balance, due date, ...).
    pub fn account_summary(&self) -> Result<Account> {
        self.ensure_authenticated()?;
        let html = self.client.get_text(pages::HOME)?;
        Ok(scrape::parse_account_summary(&html))
    }

    /// Billing history (statements).
    pub fn bills(&self) -> Result<Vec<Bill>> {
        self.ensure_authenticated()?;
        let html = self.client.get_text(pages::BILLING_HISTORY)?;
        Ok(scrape::parse_bills(&html))
    }

    /// Metered usage / consumption history (submits the service form if needed).
    pub fn usage(&self) -> Result<Vec<UsageRecord>> {
        self.ensure_authenticated()?;
        usage::fetch(&self.client)
    }

    /// Ledger transaction history (charges, payments, adjustments).
    pub fn transactions(&self) -> Result<Vec<Transaction>> {
        self.ensure_authenticated()?;
        let html = self.client.get_text(pages::TRANSACTION_HISTORY)?;
        Ok(scrape::parse_transactions(&html))
    }

    /// Account holder profile (from the DNN ManageUsers "Change Profile" page).
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
