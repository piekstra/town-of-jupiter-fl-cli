//! `tojfl mcp`: the shared `utility/v1` tools (`pk_cli_utility::mcp`) over the
//! Town of Jupiter portal.
//!
//! This module supplies what is the town's own: [`Reads`], the SDK reads the
//! commands already make, and [`JupiterSource`], which maps them with the
//! commands' mappers. Tests put fixtures behind [`Reads`], so nothing here needs
//! a network or a login to test.
//!
//! The server signs in itself rather than through the SDK's silent refresh, which
//! would send a refused password again on every call. It reads a credential and
//! never writes one, and each call reads the config, so listing the tools touches
//! no file and a server left running sees the sign-in the user does next.
//!
//! The commands' mappers are best-effort, for a person who can see the page. A
//! tool's answer goes to a model as fact, so here an answer the portal did not
//! give (a balance the page does not state, rows none of which can be read) is an
//! upstream error, never 0.00 or an empty list. The scrapers return no rows for a
//! page they cannot recognize, so an empty list is checked against the dates the
//! service page states.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use pk_cli_core::CliError as AppError;
use pk_cli_mcp::Server;
use pk_cli_secrets::{CredentialStore, KeychainRead, Secret, SecretResolver, SecretSpec};
use pk_cli_utility::mcp::{UtilityAccount, UtilitySource, UtilityTools};
use pk_cli_utility::{Payment, Statement, UsagePeriod, UtilitySummary};
use tojfl_sdk::{
    config, Bill, Config, Error as SdkError, LinkedAccount, Money, Portal, ServiceInfo, Summary,
    Transaction, UsageRecord,
};

use crate::cli::GlobalOpts;
use crate::commands;

/// The password's environment variable, read before the keychain, so a Claude
/// Desktop extension can pass what its settings hold.
const PASSWORD_ENV: &str = "TOJFL_PASSWORD";

/// What a person runs when no password is stored or it has been refused.
const SIGN_IN: &str = "tojfl auth login --save";

/// The tools `tojfl mcp` serves.
pub fn tools(global: &GlobalOpts) -> UtilityTools<JupiterSource<LivePortal>> {
    let present = |value: &Option<String>| value.clone().filter(|v| !v.is_empty());
    let source = JupiterSource {
        portal: LivePortal {
            config_path: present(&global.config),
            username: present(&global.username),
            base_url: present(&global.base_url),
            account: present(&global.account),
            refused: Mutex::default(),
        },
    };
    with_source(source, std::env::var_os(PASSWORD_ENV).is_some())
}

fn with_source<R: Reads>(
    source: JupiterSource<R>,
    password_in_env: bool,
) -> UtilityTools<JupiterSource<R>> {
    let mut server = Server::new(
        "tojfl",
        env!("CARGO_PKG_VERSION"),
        "Town of Jupiter Utilities",
        "jupiter",
    )
    .sign_in_command(SIGN_IN)
    .instructions(
        "Town of Jupiter utility access here is read-only: no tool can make a payment or change \
         anything. Bills are statements, whose paid state is not reported. Payments come from \
         the portal's ledger and may be marked pending. Usage is by the periods the portal \
         reports, in its units.",
    );
    // The keychain is not read while the variable is set, and a Claude Desktop
    // extension bundles its binary, so `tojfl auth login` would fix nothing.
    if password_in_env {
        server = server.hosted(
            "through the open-source `tojfl` tool; the password stays on the user's machine, in \
             the settings of the program that started this server",
            "ask the user to update the portal password where it is set: for the Claude Desktop \
             extension, in its settings; otherwise $TOJFL_PASSWORD",
        );
    }
    UtilityTools::new(server, source).metering("water use")
}

/// The portal reads the tools need. `account` is the one to activate first;
/// `None` is the default the config names, or the session's active account.
pub trait Reads {
    fn accounts(&self) -> Result<Vec<LinkedAccount>, AppError>;
    fn summary(&self, account: Option<&str>) -> Result<Summary, AppError>;
    fn service(&self, account: Option<&str>) -> Result<ServiceInfo, AppError>;
    fn bills(&self, account: Option<&str>) -> Result<Vec<Bill>, AppError>;
    fn transactions(&self, account: Option<&str>) -> Result<Vec<Transaction>, AppError>;
    fn usage(&self, account: Option<&str>) -> Result<Vec<UsageRecord>, AppError>;
}

/// `Reads` over the portal. An SDK [`Portal`] holds a `RefCell` and is built per
/// call from the config as it stands.
pub struct LivePortal {
    /// `--config`.
    config_path: Option<String>,
    /// `--username` or `$TOJFL_USERNAME`, which win over the config.
    username: Option<String>,
    /// `--base-url`.
    base_url: Option<String>,
    /// `--account`.
    account: Option<String>,
    /// Held across every read, not just a sign-in: which account the portal
    /// reports is state on the session, so one call must not select an account
    /// while another is reading.
    refused: Mutex<Refused>,
}

/// The login the portal last refused.
#[derive(Default)]
struct Refused(Option<(String, Secret)>);

impl Refused {
    /// Sign in, unless this password was already refused for this username. A
    /// running server left with a stale password cannot lock the login by
    /// retrying it: the next call that finds a different one tries that. Only a
    /// refusal lands here; the portal being down does not.
    fn sign_in(
        &mut self,
        username: &str,
        password: Secret,
        login: impl FnOnce(&str, &str) -> Result<(), SdkError>,
    ) -> Result<(), AppError> {
        let refused = self.0.as_ref();
        if refused.is_some_and(|(u, p)| u == username && p.expose() == password.expose()) {
            return Err(AppError::Auth(
                "the portal refused this password on an earlier call, so it was not sent again"
                    .into(),
            ));
        }
        match login(username, password.expose()) {
            Ok(()) => {
                self.0 = None;
                Ok(())
            }
            Err(e) => {
                if matches!(e, SdkError::Auth(_)) {
                    self.0 = Some((username.to_string(), password));
                }
                Err(cli_error(e))
            }
        }
    }
}

/// Whether a server with no live session may sign in. A password in the variable
/// is the extension's way of signing in; a stored one signs in only where a
/// session existed, so a deliberate `tojfl auth logout` stays logged out, as it
/// does for the commands. `auto_login = false` holds either way.
fn may_sign_in(auto_login: Option<bool>, had_session: bool, password_in_env: bool) -> bool {
    auto_login.unwrap_or(true) && (had_session || password_in_env)
}

/// The family's secret precedence, the variable and then the keychain, and then
/// the config's `password`, which the commands also accept.
fn resolve_password(
    keychain: &impl KeychainRead,
    env: &str,
    in_config: Option<&str>,
) -> Result<Secret, AppError> {
    let spec = SecretSpec::new(config::KEYRING_USER)
        .env(env)
        .remedy(format!("`{SIGN_IN}`"));
    let found = SecretResolver::new(keychain)
        .resolve(&spec)
        // A variable set but empty is a usage error on a command line. Here no
        // tool argument can fix it, and the model would be told to try another.
        .map_err(|e| match e {
            AppError::Usage(message) => AppError::Auth(message),
            other => other,
        })?;
    match found {
        Some(resolved) => Ok(resolved.secret),
        None => in_config
            .filter(|p| !p.is_empty())
            .map(Secret::new)
            .ok_or_else(|| AppError::Auth("no portal password is stored".into())),
    }
}

/// The commands' mapping, but for a config error, which the commands call a
/// usage error: here no tool argument can fix it.
fn cli_error(e: SdkError) -> AppError {
    match e {
        SdkError::Config(message) => AppError::Other(message),
        other => crate::to_cli_error(&anyhow::Error::from(other)),
    }
}

impl LivePortal {
    fn config(&self, account: Option<&str>) -> Result<Config, SdkError> {
        let mut cfg = match &self.config_path {
            Some(path) => Config::load_from(Path::new(path))?,
            None => Config::load()?,
        };
        if self.username.is_some() {
            cfg.username.clone_from(&self.username);
        }
        if self.base_url.is_some() {
            cfg.base_url.clone_from(&self.base_url);
        }
        if let Some(account) = account.map(str::to_string).or_else(|| self.account.clone()) {
            cfg.default_account = Some(account);
        }
        // Commands treat a blank in the file as a value; here it would only be
        // a confusing error from the portal.
        for value in [
            &mut cfg.username,
            &mut cfg.base_url,
            &mut cfg.default_account,
        ] {
            value.take_if(|v| v.is_empty());
        }
        Ok(cfg)
    }

    fn read<T>(
        &self,
        account: Option<&str>,
        op: impl Fn(&Portal) -> Result<T, SdkError>,
    ) -> Result<T, AppError> {
        let mut refused = self.refused.lock().unwrap_or_else(PoisonError::into_inner);
        let mut cfg = self.config(account).map_err(cli_error)?;
        let auto_login = cfg.auto_login;
        // Off in the SDK, so the one sign-in is the one below, which remembers a
        // refusal.
        cfg.auto_login = Some(false);
        let mut portal = Portal::new(&cfg).map_err(cli_error)?;
        let had_session = portal.has_saved_session();
        // The username was changed since the session was saved, as the settings
        // of a Claude Desktop extension can be. Its cookies would still answer,
        // with the other login's accounts.
        if portal.session_is_another_login() {
            portal = Portal::without_session(&cfg).map_err(cli_error)?;
        }
        match op(&portal) {
            Err(SdkError::NotAuthenticated)
                if may_sign_in(
                    auto_login,
                    had_session,
                    std::env::var_os(PASSWORD_ENV).is_some(),
                ) =>
            {
                let username = portal
                    .username()
                    .filter(|u| !u.is_empty())
                    .ok_or_else(|| AppError::Auth("no portal username is configured".into()))?;
                let keychain = CredentialStore::new(config::KEYRING_SERVICE);
                let password = resolve_password(&keychain, PASSWORD_ENV, cfg.password.as_deref())?;
                refused.sign_in(&username, password, |u, p| {
                    portal.login(u, p).map(|_saved_to| ())
                })?;
                op(&portal).map_err(cli_error)
            }
            other => other.map_err(cli_error),
        }
    }
}

impl Reads for LivePortal {
    fn accounts(&self) -> Result<Vec<LinkedAccount>, AppError> {
        self.read(None, Portal::list_accounts)
    }

    fn summary(&self, account: Option<&str>) -> Result<Summary, AppError> {
        self.read(account, Portal::summary)
    }

    fn service(&self, account: Option<&str>) -> Result<ServiceInfo, AppError> {
        self.read(account, Portal::service_info)
    }

    fn bills(&self, account: Option<&str>) -> Result<Vec<Bill>, AppError> {
        self.read(account, Portal::bills)
    }

    fn transactions(&self, account: Option<&str>) -> Result<Vec<Transaction>, AppError> {
        self.read(account, Portal::transactions)
    }

    fn usage(&self, account: Option<&str>) -> Result<Vec<UsageRecord>, AppError> {
        self.read(account, Portal::usage)
    }
}

fn shape_error(what: &str) -> AppError {
    AppError::Upstream(format!(
        "the portal's {what} did not have the expected shape, so it is not reported here"
    ))
}

/// `date`, a date the service page states, means a list that came back empty did
/// not find what is there.
fn empty_is_an_error(date: Option<&String>, what: &str) -> Result<(), AppError> {
    match date {
        Some(d) if !d.trim().is_empty() => Err(shape_error(what)),
        _ => Ok(()),
    }
}

/// Whether `date` falls within the dates of these rows, so a ledger that goes back
/// that far should show what happened on it.
fn ledger_covers(rows: &[Transaction], date: Option<&str>) -> bool {
    let oldest = rows
        .iter()
        .filter_map(|t| tojfl_sdk::date::parse(&t.date))
        .min();
    date.and_then(tojfl_sdk::date::parse)
        .zip(oldest)
        .is_some_and(|(date, oldest)| date >= oldest)
}

/// A ledger row as a payment: a credit the portal calls a payment. Adjustments are
/// credits too, and a reversal is a charge.
fn payment_of(t: &Transaction) -> Option<Payment> {
    let cents = t.amount?.cents;
    if cents >= 0 || !t.description.to_lowercase().contains("payment") {
        return None;
    }
    Some(Payment {
        date: commands::iso_date(&t.date),
        amount: commands::pk_money(Money::from_cents(-cents)),
        method: None,
        status: t.is_pending().then(|| "pending".to_string()),
        confirmation: None,
    })
}

/// A usage row with its period in the form the shared date filter reads: a date,
/// or `YYYY-MM` for a month the portal names ("Jun 2026"). Anything else stays as
/// the portal wrote it.
fn usage_of(u: &UsageRecord) -> Option<UsagePeriod> {
    let mut period = commands::usage_of(u)?;
    let label = period.period.trim();
    if let Some((y, m, d)) = tojfl_sdk::date::parse(label) {
        period.period = format!("{y:04}-{m:02}-{d:02}");
    } else if let Some((y, m)) = tojfl_sdk::date::parse_month_year(label) {
        period.period = format!("{y:04}-{m:02}");
    }
    Some(period)
}

/// [`UtilitySource`] over [`Reads`].
pub struct JupiterSource<R> {
    portal: R,
}

impl<R: Reads> UtilitySource for JupiterSource<R> {
    async fn accounts(&self) -> Result<Vec<UtilityAccount>, AppError> {
        Ok(self
            .portal
            .accounts()?
            .into_iter()
            .map(|a| UtilityAccount {
                id: a.account_number,
                // The address, not the account holder's name.
                name: a.service_address.filter(|s| !s.trim().is_empty()),
            })
            .collect())
    }

    async fn summary(&self, account: Option<&str>) -> Result<UtilitySummary, AppError> {
        let summary = self.portal.summary(account)?;
        if summary.account.balance.is_none() {
            return Err(shape_error("account page"));
        }
        // Which account the page reports is state on the session, which another
        // `tojfl` can change between selecting and reading. Numbers differ only in
        // leading zeros between the portal's pages.
        let reported = summary.account.account_number.trim_start_matches('0');
        if let Some(named) = account {
            if !reported.is_empty() && reported != named.trim_start_matches('0') {
                return Err(AppError::Upstream(
                    "the portal reported a different account from the one asked for".into(),
                ));
            }
        }
        Ok(commands::summary_dto(&summary))
    }

    async fn statements(&self, account: Option<&str>) -> Result<Vec<Statement>, AppError> {
        let rows = self.portal.bills(account)?;
        if rows.is_empty() {
            let service = self.portal.service(account)?;
            empty_is_an_error(service.last_bill_date.as_ref(), "billing history")?;
            return Ok(Vec::new());
        }
        // `statement_of` shows a row with no amount as 0.00.
        let mut statements: Vec<Statement> = rows
            .iter()
            .enumerate()
            .filter(|(_, b)| b.amount.is_some() && !b.date.trim().is_empty())
            .map(|(i, b)| commands::statement_of(i + 1, b))
            .collect();
        if statements.is_empty() {
            return Err(shape_error("billing history"));
        }
        statements.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(statements)
    }

    async fn payments(
        &self,
        account: Option<&str>,
        _since: Option<&str>,
    ) -> Result<Vec<Payment>, AppError> {
        let rows = self.portal.transactions(account)?;
        if rows.is_empty() {
            let service = self.portal.service(account)?;
            empty_is_an_error(service.last_payment_date.as_ref(), "transaction history")?;
            return Ok(Vec::new());
        }
        // Rows that no longer say what they are would all read as "not a payment".
        let readable = |t: &Transaction| {
            t.amount.is_some() && !t.description.trim().is_empty() && !t.date.trim().is_empty()
        };
        if !rows.iter().any(readable) {
            return Err(shape_error("transaction history"));
        }
        let mut payments: Vec<Payment> = rows
            .iter()
            .filter(|t| readable(t))
            .filter_map(payment_of)
            .collect();
        if payments.is_empty() {
            // Payments that no longer read as credits called payments would look
            // the same as none. The service page dates the latest one.
            let service = self.portal.service(account)?;
            if ledger_covers(&rows, service.last_payment_date.as_deref()) {
                return Err(shape_error("transaction history"));
            }
        }
        payments.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(payments)
    }

    async fn usage(&self, account: Option<&str>) -> Result<Vec<UsagePeriod>, AppError> {
        let rows = self.portal.usage(account)?;
        if rows.is_empty() {
            let service = self.portal.service(account)?;
            empty_is_an_error(service.last_read_date.as_ref(), "usage history")?;
            return Ok(Vec::new());
        }
        let mut usage: Vec<UsagePeriod> = rows.iter().filter_map(usage_of).collect();
        if usage.is_empty() {
            return Err(shape_error("usage history"));
        }
        usage.sort_by(|a, b| b.period.cmp(&a.period));
        Ok(usage)
    }
}

#[cfg(test)]
mod tests;
