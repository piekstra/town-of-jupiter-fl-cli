//! Command-line surface for `tojfl`, defined with clap-derive.

use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;
pub use pk_cli_selfupdate::SelfUpdateArgs;

/// tojfl — command-line client for the Town of Jupiter, FL utility portal.
///
/// View account and billing info, review usage, list transactions, and drive
/// the one-time payment lookup. Credentials come from `--username`/env/keychain
/// — never hard-coded. Add `--json` or `--csv` to any command for
/// machine-readable output.
#[derive(Debug, Parser)]
#[command(name = "tojfl", version, about, long_about = None, propagate_version = true)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
pub struct GlobalOpts {
    /// Emit JSON instead of formatted tables.
    #[arg(long, global = true)]
    pub json: bool,

    /// Emit CSV instead of formatted tables (row commands + single-record views).
    #[arg(long, global = true, conflicts_with = "json")]
    pub csv: bool,

    /// Operate on this account number (overrides config default).
    #[arg(long, global = true, value_name = "ACCOUNT")]
    pub account: Option<String>,

    /// Path to a config file (defaults to ./tojfl.toml then the OS config dir).
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<String>,

    /// Portal username (or set TOJFL_USERNAME).
    #[arg(long, global = true, env = "TOJFL_USERNAME")]
    pub username: Option<String>,

    /// Override the portal base URL.
    #[arg(long, global = true, value_name = "URL")]
    pub base_url: Option<String>,

    /// Verbose diagnostics on stderr.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress non-error stderr output.
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable ANSI color. Also honored via $NO_COLOR.
    #[arg(long, global = true, env = "NO_COLOR")]
    pub no_color: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Log in, log out, or check session status.
    #[command(subcommand)]
    Auth(AuthCmd),

    /// At-a-glance overview: balance, due, last read/bill/payment, enrollment.
    Summary,

    /// One-call dashboard payload: balance, due/past-due, last payment, usage
    /// stats, and ledger totals (best with `--json`).
    Snapshot {
        /// Snapshot every linked account instead of just the active one.
        #[arg(long)]
        all_accounts: bool,
    },

    /// Show account summary and linked accounts.
    #[command(subcommand)]
    Account(AccountCmd),

    /// Show the current balance due.
    Balance,

    /// Billing history (statements).
    #[command(subcommand)]
    Bills(BillsCmd),

    /// Metered water usage / consumption history.
    #[command(subcommand)]
    Usage(UsageCmd),

    /// Meter reading history (date, meter #, previous/current read, consumption).
    Meters {
        /// Only show the most recent N reads.
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        /// Only include reads on or after this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// Only include reads on or before this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        until: Option<String>,
    },

    /// Ledger transactions (charges, payments, adjustments).
    #[command(subcommand)]
    Transactions(TransactionsCmd),

    /// One-time payment lookup and hand-off to the hosted payment page.
    #[command(subcommand)]
    Pay(PayCmd),

    /// Account holder profile.
    #[command(subcommand)]
    Profile(ProfileCmd),

    /// Paperless / eBill enrollment status.
    #[command(subcommand)]
    Ebill(EbillCmd),

    /// Service snapshot: last read date, last bill, last payment (active account).
    Service,

    /// Show utility contact and service information.
    Contact,

    /// Manage local config and stored credentials.
    #[command(subcommand)]
    Config(ConfigCmd),

    /// Update tojfl in place from the latest GitHub release.
    SelfUpdate(SelfUpdateArgs),

    /// Print a shell completion script (e.g. `tojfl completions zsh`).
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Machine-readable capability discovery (cli-info/v1).
    Info,
}

#[derive(Debug, Subcommand)]
pub enum AuthCmd {
    /// Authenticate and persist a session.
    Login(LoginArgs),
    /// Clear the saved session (and optionally the stored password).
    Logout {
        /// Also remove the password from the OS keychain.
        #[arg(long)]
        forget: bool,
    },
    /// Report whether a valid session exists.
    Status,
}

#[derive(Debug, Args)]
pub struct LoginArgs {
    /// Save username to config and password to the OS keychain for next time.
    #[arg(long)]
    pub save: bool,
    /// Read the password from stdin instead of prompting (for scripts/pipes).
    #[arg(long)]
    pub password_stdin: bool,
}

#[derive(Debug, Subcommand)]
pub enum AccountCmd {
    /// Show the account summary (balance, due date, address). [default]
    Show,
    /// List accounts linked to this login.
    List,
}

#[derive(Debug, Subcommand)]
pub enum BillsCmd {
    /// List all statements in the billing history. [default]
    List {
        /// Only show the most recent N statements.
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        /// Only include statements on or after this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// Only include statements on or before this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        until: Option<String>,
    },
    /// Show just the most recent statement.
    Latest,
    /// Download a statement PDF by position (1 = most recent).
    Get(BillsGetArgs),
}

#[derive(Debug, Args)]
pub struct BillsGetArgs {
    /// Which statement, 1-based (1 = most recent).
    #[arg(value_name = "N", default_value = "1")]
    pub index: usize,
    /// Where to write the PDF (default: ./bill-<date>.pdf; use `-` for stdout).
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum UsageCmd {
    /// List usage periods. [default]
    List {
        /// Only show the most recent N periods.
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        /// Only include periods on or after this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// Only include periods on or before this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        until: Option<String>,
    },
    /// Compare consumption. Default: period-over-period. With `--against`,
    /// compares to a street/region/city average from the portal.
    Compare {
        /// Compare to a group average instead of period-over-period.
        #[arg(long, value_enum, value_name = "GROUP")]
        against: Option<CompareAgainst>,
    },
    /// Summary statistics over usage history (total, average, min/max period).
    Stats {
        /// Only include periods on or after this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// Only include periods on or before this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        until: Option<String>,
    },
}

/// Group to compare consumption against (`usage compare --against`).
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum CompareAgainst {
    Street,
    Region,
    City,
}

#[derive(Debug, Subcommand)]
pub enum TransactionsCmd {
    /// List ledger transactions. [default]
    List {
        /// Only show the most recent N transactions.
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        /// Only include transactions on or after this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// Only include transactions on or before this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        until: Option<String>,
    },
    /// Total charges, payments/credits, and net over the ledger.
    Summary {
        /// Only include transactions on or after this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// Only include transactions on or before this date (YYYY-MM-DD, MM/DD/YYYY, or "Mon DD, YYYY").
        #[arg(long, value_name = "DATE")]
        until: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum PayCmd {
    /// Look up an account and report the amount due (no login required).
    Quote(PayLookupArgs),
    /// Look up an account and print (or open) the hosted payment page URL.
    Open(PayOpenArgs),
}

#[derive(Debug, Args)]
pub struct PayLookupArgs {
    /// 7-digit customer number (with leading zeros).
    #[arg(long, short = 'c', value_name = "CUSTOMER")]
    pub customer: String,
    /// 6-digit account number (with leading zeros).
    #[arg(long, short = 'a', value_name = "ACCOUNT")]
    pub account: String,
}

#[derive(Debug, Args)]
pub struct PayOpenArgs {
    /// 7-digit customer number (with leading zeros).
    #[arg(long, short = 'c', value_name = "CUSTOMER")]
    pub customer: String,
    /// 6-digit account number (with leading zeros).
    #[arg(long, short = 'a', value_name = "ACCOUNT")]
    pub account: String,
    /// Open the hosted payment page in your default browser.
    #[arg(long)]
    pub open: bool,
}

#[derive(Debug, Subcommand)]
pub enum ProfileCmd {
    /// Show the account holder profile. [default]
    Show,
}

#[derive(Debug, Subcommand)]
pub enum EbillCmd {
    /// Show paperless/eBill enrollment status.
    Status,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    /// Print the resolved config file path.
    Path,
    /// Write a starter config file to the OS config dir.
    Init,
    /// Show the effective (loaded) configuration.
    Show,
    /// Set a config value, e.g. `config set account 000000`. Keys: account,
    /// username, base_url, output, timeout_secs, auto_login (piekstra-cli/1).
    Set {
        /// Config key to set.
        key: String,
        /// Value to store.
        value: String,
    },
    /// Clear a config value, e.g. `config unset account`.
    Unset {
        /// Config key to clear.
        key: String,
    },
    /// Store the portal password in the OS keychain.
    SetPassword,
    /// Remove the stored password from the OS keychain.
    ClearPassword,
}
