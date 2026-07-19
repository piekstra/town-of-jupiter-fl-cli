//! Command handlers. Each returns `anyhow::Result<()>` and prints its own
//! output (table, JSON, or CSV) via [`Format`].

use crate::cli::*;
use crate::output::{opt, Format};
use anyhow::{anyhow, Context, Result};
use std::io::{IsTerminal, Read};
use tojfl_sdk::{config, CompareTarget, Config, Portal};

/// Shared context threaded through handlers.
pub struct Ctx {
    pub fmt: Format,
    pub cfg: Config,
    pub username: Option<String>,
    pub verbose: bool,
}

impl Ctx {
    fn portal(&self) -> Result<Portal> {
        let portal = Portal::new(&self.cfg).context("initializing portal client")?;
        if self.verbose {
            eprintln!(
                "[tojfl] base_url={} authenticated_username={}",
                portal.base_url(),
                portal.username().unwrap_or_else(|| "(none)".to_string())
            );
        }
        Ok(portal)
    }
}

// --- auth -----------------------------------------------------------------

pub fn auth(ctx: &Ctx, cmd: &AuthCmd) -> Result<()> {
    match cmd {
        AuthCmd::Login(args) => auth_login(ctx, args),
        AuthCmd::Logout { forget } => auth_logout(ctx, *forget),
        AuthCmd::Status => auth_status(ctx),
    }
}

fn auth_login(ctx: &Ctx, args: &LoginArgs) -> Result<()> {
    let username = ctx
        .username
        .clone()
        .or_else(|| ctx.cfg.username.clone())
        .or_else(|| prompt_line("Portal username: ").ok())
        .ok_or_else(|| anyhow!("a username is required"))?;

    let password = resolve_password(args)?;

    let mut portal = ctx.portal()?;
    let path = portal.login(&username, &password).context("login failed")?;

    if args.save {
        config::keychain_set(&password).context("saving password to keychain")?;
        let mut cfg = ctx.cfg.clone();
        cfg.username = Some(username.clone());
        cfg.save().context("saving username to config")?;
    }

    if ctx.fmt.json {
        ctx.fmt.print_json(&serde_json::json!({
            "status": "ok",
            "username": username,
            "session_saved_to": path.display().to_string(),
            "credentials_saved": args.save,
        }))?;
    } else {
        println!("✓ Logged in as {username}. Session saved.");
        if args.save {
            println!("✓ Password stored in the OS keychain; username saved to config.");
        }
    }
    Ok(())
}

fn auth_logout(ctx: &Ctx, forget: bool) -> Result<()> {
    let portal = ctx.portal()?;
    portal.logout().context("clearing session")?;
    if forget {
        config::keychain_clear().context("clearing keychain password")?;
    }
    if ctx.fmt.json {
        ctx.fmt.print_json(&serde_json::json!({
            "status": "ok",
            "forgot_password": forget,
        }))?;
    } else {
        println!(
            "✓ Session cleared.{}",
            if forget {
                " Password removed from keychain."
            } else {
                ""
            }
        );
    }
    Ok(())
}

fn auth_status(ctx: &Ctx) -> Result<()> {
    use pk_cli_auth::{AuthMethod, AuthStatus};
    let portal = ctx.portal()?;
    let authed = portal.is_authenticated().unwrap_or(false);
    let mut st = AuthStatus::new(true, authed, AuthMethod::Password);
    st.username = portal.username();
    st.session_valid = Some(authed);
    st.emit(ctx.fmt.json);
    Ok(())
}

/// `info` — cli-info/v1 capability discovery.
pub fn info(_ctx: &Ctx) -> Result<()> {
    use pk_cli_core::info::{AuthInfo, CliInfo};
    let info = CliInfo::new(
        "tojfl",
        env!("CARGO_PKG_VERSION"),
        "https://github.com/piekstra/town-of-jupiter-fl-cli",
        AuthInfo {
            required: true,
            method: "password".into(),
            login_hint: Some("tojfl auth login --save".into()),
        },
        &[
            "summary",
            "snapshot",
            "account",
            "balance",
            "bills",
            "usage",
            "meters",
            "transactions",
            "pay",
            "profile",
            "ebill",
            "service",
            "contact",
        ],
    );
    pk_cli_core::output::json(&serde_json::to_value(&info)?);
    Ok(())
}

// --- summary --------------------------------------------------------------

pub fn summary(ctx: &Ctx) -> Result<()> {
    let portal = ctx.portal()?;
    let s = portal.summary()?;
    if ctx.fmt.json {
        ctx.fmt.print_json(&s)?;
    } else {
        let acct = if s.account.account_number.is_empty() {
            opt(&s.enrollment.account_number)
        } else {
            s.account.account_number.clone()
        };
        let last_payment = match (&s.service.last_payment_amount, &s.service.last_payment_date) {
            (None, None) => "—".to_string(),
            (a, d) => format!("{} on {}", opt(a), opt(d)),
        };
        ctx.fmt.print_kv(
            "Account Overview",
            &[
                ("Account #", acct),
                ("Balance", opt(&s.account.balance)),
                ("Due date", opt(&s.account.due_date)),
                ("Last read date", opt(&s.service.last_read_date)),
                ("Last bill", opt(&s.service.last_bill_amount)),
                ("Last payment", last_payment),
                ("Paperless", tri_state(s.enrollment.paperless).into()),
                ("Autopay", tri_state(s.enrollment.autopay).into()),
            ],
        );
    }
    Ok(())
}

// --- snapshot -------------------------------------------------------------

pub fn snapshot(ctx: &Ctx, all_accounts: bool) -> Result<()> {
    let portal = ctx.portal()?;
    if all_accounts {
        let snaps = portal.snapshot_all()?;
        if ctx.fmt.json {
            // An array keeps the shape stable regardless of account count.
            ctx.fmt.print_json(&snaps)?;
        } else {
            // Row-oriented (one row per account) so CSV stays a single valid
            // table and each account is self-describing.
            let rows: Vec<Vec<String>> = snaps.iter().map(snapshot_row).collect();
            ctx.fmt.print_table(&SNAPSHOT_COLUMNS, &rows);
        }
    } else {
        let s = portal.snapshot()?;
        if ctx.fmt.json {
            ctx.fmt.print_json(&s)?;
        } else {
            print_snapshot_kv(ctx, &s);
        }
    }
    Ok(())
}

const SNAPSHOT_COLUMNS: [&str; 13] = [
    "Account #",
    "Name",
    "Service address",
    "Balance",
    "Pending",
    "Effective",
    "Due date",
    "Past due",
    "Last payment",
    "Usage",
    "Charges",
    "Payments",
    "Net",
];

/// A single snapshot as a row aligned to [`SNAPSHOT_COLUMNS`].
fn snapshot_row(s: &tojfl_sdk::Snapshot) -> Vec<String> {
    vec![
        opt(&s.account),
        opt(&s.name),
        opt(&s.service_address),
        opt(&s.balance),
        opt(&s.pending_payments),
        opt(&s.effective_balance),
        opt(&s.due_date),
        if s.past_due { "yes" } else { "no" }.into(),
        snapshot_last_payment(s),
        snapshot_usage(s),
        s.ledger.charges.to_string(),
        s.ledger.payments.to_string(),
        s.ledger.net.to_string(),
    ]
}

fn snapshot_last_payment(s: &tojfl_sdk::Snapshot) -> String {
    match (&s.last_payment_amount, &s.last_payment_date) {
        (None, None) => "—".to_string(),
        (a, d) => format!("{} on {}", opt(a), opt(d)),
    }
}

fn snapshot_usage(s: &tojfl_sdk::Snapshot) -> String {
    match &s.usage {
        Some(u) => format!(
            "{} avg over {} periods{}",
            fmt_num(u.average),
            u.periods,
            u.unit
                .as_deref()
                .map(|x| format!(" ({x})"))
                .unwrap_or_default()
        ),
        None => "—".to_string(),
    }
}

/// Render one snapshot as a flattened key/value block (single-account view).
fn print_snapshot_kv(ctx: &Ctx, s: &tojfl_sdk::Snapshot) {
    // Always shown (matching `account show` and the --all-accounts table),
    // "—" when absent — unlike the genuinely-optional pending rows below.
    let mut pairs: Vec<(&str, String)> = vec![
        ("Account #", opt(&s.account)),
        ("Name", opt(&s.name)),
        ("Service address", opt(&s.service_address)),
        ("Balance", opt(&s.balance)),
    ];
    // Only present when a payment is still pending (otherwise balance is final).
    if let Some(p) = &s.pending_payments {
        pairs.push(("Pending payments", p.to_string()));
    }
    if let Some(e) = &s.effective_balance {
        pairs.push(("Effective balance", e.to_string()));
    }
    pairs.extend([
        ("Due date", opt(&s.due_date)),
        ("Past due", if s.past_due { "yes" } else { "no" }.into()),
        ("Last payment", snapshot_last_payment(s)),
        ("Usage", snapshot_usage(s)),
        ("Ledger charges", s.ledger.charges.to_string()),
        ("Ledger payments", s.ledger.payments.to_string()),
        ("Ledger net", s.ledger.net.to_string()),
    ]);
    ctx.fmt.print_kv("Account Snapshot", &pairs);
}

// --- account --------------------------------------------------------------

pub fn account(ctx: &Ctx, cmd: &AccountCmd) -> Result<()> {
    match cmd {
        AccountCmd::Show => account_show(ctx),
        AccountCmd::List => account_list(ctx),
    }
}

fn account_show(ctx: &Ctx) -> Result<()> {
    let portal = ctx.portal()?;
    let acct = portal.account_summary()?;
    if ctx.fmt.json {
        ctx.fmt.print_json(&acct)?;
    } else {
        ctx.fmt.print_kv(
            "Account Summary",
            &[
                ("Name", opt(&acct.name)),
                ("Service address", opt(&acct.service_address)),
                ("Balance", opt(&acct.balance)),
                ("Due date", opt(&acct.due_date)),
                (
                    "Account #",
                    if acct.account_number.is_empty() {
                        "—".into()
                    } else {
                        acct.account_number.clone()
                    },
                ),
            ],
        );
    }
    Ok(())
}

fn account_list(ctx: &Ctx) -> Result<()> {
    let portal = ctx.portal()?;
    let accounts = portal.list_accounts()?;
    if ctx.fmt.json {
        ctx.fmt.print_json(&accounts)?;
    } else {
        let rows: Vec<Vec<String>> = accounts
            .iter()
            .map(|a| {
                vec![
                    a.account_number.clone(),
                    opt(&a.name),
                    opt(&a.service_address),
                    opt(&a.past_due),
                    opt(&a.balance),
                ]
            })
            .collect();
        ctx.fmt.print_table(
            &[
                "Account #",
                "Name",
                "Service address",
                "Past due",
                "Balance",
            ],
            &rows,
        );
        if accounts.len() > 1 {
            eprintln!("Tip: target one with `tojfl --account <ACCOUNT#> <command>`.");
        }
    }
    Ok(())
}

pub fn balance(ctx: &Ctx) -> Result<()> {
    let portal = ctx.portal()?;
    let bal = portal.balance()?;
    if ctx.fmt.json {
        ctx.fmt.print_json(&serde_json::json!({ "balance": bal }))?;
    } else {
        match bal {
            Some(b) => println!("Balance due: {b}"),
            None => println!("Balance not available."),
        }
    }
    Ok(())
}

// --- bills ----------------------------------------------------------------

pub fn bills(ctx: &Ctx, cmd: &BillsCmd) -> Result<()> {
    let portal = ctx.portal()?;
    let items = portal.bills()?;

    if let BillsCmd::Get(args) = cmd {
        return bills_get(ctx, &portal, &items, args);
    }

    let mut items = items;
    match cmd {
        BillsCmd::Latest => items.truncate(1),
        BillsCmd::List {
            limit,
            since,
            until,
        } => {
            let (since, until) = date_bounds(since, until)?;
            items.retain(|b| tojfl_sdk::date::in_range(&b.date, since, until));
            if let Some(n) = limit {
                items.truncate(*n);
            }
        }
        BillsCmd::Get(_) => unreachable!("handled above"),
    }
    if ctx.fmt.json {
        ctx.fmt.print_json(&items)?;
    } else {
        let rows: Vec<Vec<String>> = items
            .iter()
            .map(|b| {
                vec![
                    b.date.clone(),
                    opt(&b.current_charges),
                    opt(&b.amount),
                    opt(&b.balance_forward),
                    opt(&b.due_date),
                    if b.document_url.is_some() {
                        "✓".into()
                    } else {
                        "—".into()
                    },
                ]
            })
            .collect();
        ctx.fmt.print_table(
            &["Date", "Current", "Total", "Fwd", "Due date", "PDF"],
            &rows,
        );
        if items.iter().any(|b| b.document_url.is_some()) {
            eprintln!("Tip: download a statement with `tojfl bills get <N>` (1 = most recent).");
        }
    }
    Ok(())
}

fn bills_get(
    ctx: &Ctx,
    portal: &Portal,
    items: &[tojfl_sdk::Bill],
    args: &BillsGetArgs,
) -> Result<()> {
    // A raw PDF stream can't also be JSON — reject the conflict up front rather
    // than silently ignoring --json (every other command honors it).
    if ctx.fmt.json && args.output.as_deref() == Some("-") {
        return Err(anyhow!(
            "--json and `-o -` are mutually exclusive: a binary PDF can't be JSON-encoded"
        ));
    }
    if args.index == 0 || args.index > items.len() {
        return Err(tojfl_sdk::Error::NotFound(format!(
            "no statement at position {} — the billing history has {} statement(s)",
            args.index,
            items.len()
        ))
        .into());
    }
    let bill = &items[args.index - 1];
    let pdf = portal
        .download_bill(bill)
        .context("downloading statement PDF")?;

    if args.output.as_deref() == Some("-") {
        use std::io::Write;
        return std::io::stdout()
            .write_all(&pdf)
            .context("writing PDF to stdout");
    }

    let path = args.output.clone().unwrap_or_else(|| {
        let safe: String = bill
            .date
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let stem = safe.trim_matches('-');
        format!(
            "bill-{}.pdf",
            if stem.is_empty() {
                args.index.to_string()
            } else {
                stem.to_string()
            }
        )
    });
    std::fs::write(&path, &pdf).with_context(|| format!("writing {path}"))?;

    if ctx.fmt.json {
        ctx.fmt.print_json(&serde_json::json!({
            "saved": path,
            "bytes": pdf.len(),
            "date": bill.date,
        }))?;
    } else {
        println!(
            "✓ Saved statement {} ({} bytes) to {}",
            bill.date,
            pdf.len(),
            path
        );
    }
    Ok(())
}

// --- usage ----------------------------------------------------------------

pub fn usage(ctx: &Ctx, cmd: &UsageCmd) -> Result<()> {
    let portal = ctx.portal()?;
    match cmd {
        UsageCmd::Compare {
            against: Some(group),
        } => usage_compare_group(ctx, &portal, *group),
        UsageCmd::Compare { against: None } => {
            let items = portal.usage()?;
            usage_compare(ctx, &items)
        }
        UsageCmd::Stats { since, until } => usage_stats(ctx, &portal, since, until),
        UsageCmd::List {
            limit,
            since,
            until,
        } => {
            let mut items = portal.usage()?;
            let (since, until) = date_bounds(since, until)?;
            items.retain(|u| tojfl_sdk::date::in_range(&u.period, since, until));
            if let Some(n) = limit {
                items.truncate(*n);
            }
            if ctx.fmt.json {
                ctx.fmt.print_json(&items)?;
            } else {
                let rows: Vec<Vec<String>> = items
                    .iter()
                    .map(|u| {
                        vec![
                            u.period.clone(),
                            u.quantity.map(fmt_num).unwrap_or_else(|| "—".into()),
                            opt(&u.unit),
                            u.days.map(|d| d.to_string()).unwrap_or_else(|| "—".into()),
                            u.average_per_day.map(fmt_num).unwrap_or_else(|| "—".into()),
                        ]
                    })
                    .collect();
                ctx.fmt
                    .print_table(&["Period", "Usage", "Unit", "Days", "Avg/day"], &rows);
            }
            Ok(())
        }
    }
}

fn usage_compare(ctx: &Ctx, items: &[tojfl_sdk::UsageRecord]) -> Result<()> {
    #[derive(serde::Serialize)]
    struct Delta {
        period: String,
        quantity: Option<f64>,
        change: Option<f64>,
        percent: Option<f64>,
    }
    let mut deltas = Vec::new();
    for (i, u) in items.iter().enumerate() {
        let prev = items.get(i + 1).and_then(|p| p.quantity);
        let change = match (u.quantity, prev) {
            (Some(c), Some(p)) => Some(c - p),
            _ => None,
        };
        let percent = match (change, prev) {
            (Some(d), Some(p)) if p != 0.0 => Some(d / p * 100.0),
            _ => None,
        };
        deltas.push(Delta {
            period: u.period.clone(),
            quantity: u.quantity,
            change,
            percent,
        });
    }
    if ctx.fmt.json {
        ctx.fmt.print_json(&deltas)?;
    } else {
        let rows: Vec<Vec<String>> = deltas
            .iter()
            .map(|d| {
                vec![
                    d.period.clone(),
                    d.quantity.map(fmt_num).unwrap_or_else(|| "—".into()),
                    d.change
                        .map(|c| format!("{}{}", if c >= 0.0 { "+" } else { "" }, fmt_num(c)))
                        .unwrap_or_else(|| "—".into()),
                    d.percent
                        .map(|p| format!("{p:+.1}%"))
                        .unwrap_or_else(|| "—".into()),
                ]
            })
            .collect();
        ctx.fmt
            .print_table(&["Period", "Usage", "Δ vs prior", "Δ %"], &rows);
    }
    Ok(())
}

fn usage_stats(
    ctx: &Ctx,
    portal: &Portal,
    since: &Option<String>,
    until: &Option<String>,
) -> Result<()> {
    let mut items = portal.usage()?;
    let (since, until) = date_bounds(since, until)?;
    items.retain(|u| tojfl_sdk::date::in_range(&u.period, since, until));

    let stats = tojfl_sdk::UsageStats::from_records(&items);
    if ctx.fmt.json {
        // Emit an object (or `null` for no data) — stable shape for scripts.
        ctx.fmt.print_json(&stats)?;
        return Ok(());
    }
    let Some(s) = stats else {
        println!("(no usage with a numeric quantity in range)");
        return Ok(());
    };
    let unit = s.unit.as_deref().unwrap_or("");
    let with_unit = |n: f64| {
        if unit.is_empty() {
            fmt_num(n)
        } else {
            format!("{} {unit}", fmt_num(n))
        }
    };
    ctx.fmt.print_kv(
        "Usage Statistics",
        &[
            ("Periods", s.periods.to_string()),
            ("Total", with_unit(s.total)),
            ("Average", with_unit(s.average)),
            ("Min", format!("{} ({})", with_unit(s.min), s.min_period)),
            ("Max", format!("{} ({})", with_unit(s.max), s.max_period)),
        ],
    );
    Ok(())
}

fn usage_compare_group(ctx: &Ctx, portal: &Portal, group: CompareAgainst) -> Result<()> {
    let (target, label) = match group {
        CompareAgainst::Street => (CompareTarget::Street, "street"),
        CompareAgainst::Region => (CompareTarget::Region, "region"),
        CompareAgainst::City => (CompareTarget::City, "city"),
    };
    let rows = portal.usage_compare(target)?;
    if ctx.fmt.json {
        ctx.fmt.print_json(&rows)?;
    } else {
        let avg_header = format!("{label} avg");
        let table: Vec<Vec<String>> = rows
            .iter()
            .map(|r| {
                let delta = match (r.consumption, r.average) {
                    (Some(c), Some(a)) if a != 0.0 => format!("{:+.1}%", (c - a) / a * 100.0),
                    _ => "—".into(),
                };
                vec![
                    r.period.clone(),
                    r.consumption.map(fmt_num).unwrap_or_else(|| "—".into()),
                    r.average.map(fmt_num).unwrap_or_else(|| "—".into()),
                    opt(&r.unit),
                    delta,
                ]
            })
            .collect();
        ctx.fmt.print_table(
            &["Period", "Yours", &avg_header, "Unit", "Δ vs avg"],
            &table,
        );
    }
    Ok(())
}

// --- meters ---------------------------------------------------------------

pub fn meters(
    ctx: &Ctx,
    limit: Option<usize>,
    since: &Option<String>,
    until: &Option<String>,
) -> Result<()> {
    let portal = ctx.portal()?;
    let mut reads = portal.meter_reads()?;
    let (since, until) = date_bounds(since, until)?;
    reads.retain(|r| tojfl_sdk::date::in_range(&r.date, since, until));
    if let Some(n) = limit {
        reads.truncate(n);
    }
    if ctx.fmt.json {
        ctx.fmt.print_json(&reads)?;
    } else {
        let rows: Vec<Vec<String>> = reads
            .iter()
            .map(|r| {
                let n = |v: &Option<f64>| v.map(fmt_num).unwrap_or_else(|| "—".into());
                vec![
                    r.date.clone(),
                    opt(&r.meter),
                    n(&r.previous_read),
                    n(&r.current_read),
                    r.days.map(|d| d.to_string()).unwrap_or_else(|| "—".into()),
                    opt(&r.reading_type),
                    n(&r.consumption),
                ]
            })
            .collect();
        ctx.fmt.print_table(
            &["Date", "Meter", "Prev", "Current", "Days", "Type", "Usage"],
            &rows,
        );
    }
    Ok(())
}

// --- transactions ---------------------------------------------------------

pub fn transactions(ctx: &Ctx, cmd: &TransactionsCmd) -> Result<()> {
    let portal = ctx.portal()?;
    let items = portal.transactions()?;
    match cmd {
        TransactionsCmd::List {
            limit,
            since,
            until,
        } => transactions_list(ctx, items, *limit, since, until),
        TransactionsCmd::Summary { since, until } => transactions_summary(ctx, items, since, until),
    }
}

fn transactions_list(
    ctx: &Ctx,
    mut items: Vec<tojfl_sdk::Transaction>,
    limit: Option<usize>,
    since: &Option<String>,
    until: &Option<String>,
) -> Result<()> {
    let (since, until) = date_bounds(since, until)?;
    items.retain(|t| tojfl_sdk::date::in_range(&t.date, since, until));
    if let Some(n) = limit {
        items.truncate(n);
    }
    if ctx.fmt.json {
        ctx.fmt.print_json(&items)?;
    } else {
        let rows: Vec<Vec<String>> = items
            .iter()
            .map(|t| {
                vec![
                    t.date.clone(),
                    t.description.clone(),
                    opt(&t.amount),
                    opt(&t.balance),
                ]
            })
            .collect();
        ctx.fmt
            .print_table(&["Date", "Description", "Amount", "Balance"], &rows);
    }
    Ok(())
}

fn transactions_summary(
    ctx: &Ctx,
    mut items: Vec<tojfl_sdk::Transaction>,
    since: &Option<String>,
    until: &Option<String>,
) -> Result<()> {
    let (since, until) = date_bounds(since, until)?;
    items.retain(|t| tojfl_sdk::date::in_range(&t.date, since, until));
    let s = tojfl_sdk::TransactionSummary::from_transactions(&items);
    if ctx.fmt.json {
        ctx.fmt.print_json(&s)?;
    } else {
        ctx.fmt.print_kv(
            "Transaction Summary",
            &[
                ("Transactions", s.count.to_string()),
                ("Charges", s.charges.to_string()),
                ("Payments & credits", s.payments.to_string()),
                ("Net", s.net.to_string()),
            ],
        );
    }
    Ok(())
}

// --- pay ------------------------------------------------------------------

pub fn pay(ctx: &Ctx, cmd: &PayCmd) -> Result<()> {
    match cmd {
        PayCmd::Quote(args) => pay_quote(ctx, &args.customer, &args.account, false),
        PayCmd::Open(args) => pay_open(ctx, args),
    }
}

fn pay_quote(ctx: &Ctx, customer: &str, account: &str, _open: bool) -> Result<()> {
    let portal = ctx.portal()?;
    let quote = portal.payment_quote(customer, account)?;
    if ctx.fmt.json {
        ctx.fmt.print_json(&quote)?;
    } else {
        ctx.fmt.print_kv(
            "Payment Quote",
            &[
                ("Customer #", quote.customer_number.clone()),
                ("Account #", quote.account_number.clone()),
                ("Amount due", opt(&quote.amount_due)),
                ("Valid", quote.valid.to_string()),
                ("Message", opt(&quote.message)),
                ("Hosted page", opt(&quote.hosted_payment_url)),
            ],
        );
        if quote.hosted_payment_url.is_some() {
            println!("\nCard entry happens on the hosted page above — this tool never handles card data.");
        }
    }
    Ok(())
}

fn pay_open(ctx: &Ctx, args: &PayOpenArgs) -> Result<()> {
    let portal = ctx.portal()?;
    let quote = portal.payment_quote(&args.customer, &args.account)?;
    match &quote.hosted_payment_url {
        Some(url) => {
            if args.open {
                open_in_browser(url)?;
            }
            if ctx.fmt.json {
                ctx.fmt.print_json(&quote)?;
            } else {
                println!("Hosted payment page: {url}");
                println!("Amount due: {}", opt(&quote.amount_due));
                println!(
                    "\nComplete the payment on that page — this tool never handles card data."
                );
            }
            Ok(())
        }
        None => {
            if ctx.fmt.json {
                ctx.fmt.print_json(&quote)?;
                Ok(())
            } else {
                Err(anyhow!(
                    "could not determine a hosted payment URL{}",
                    quote
                        .message
                        .as_deref()
                        .map(|m| format!(" ({m})"))
                        .unwrap_or_default()
                ))
            }
        }
    }
}

// --- profile --------------------------------------------------------------

pub fn profile(ctx: &Ctx, cmd: &ProfileCmd) -> Result<()> {
    let ProfileCmd::Show = cmd;
    let portal = ctx.portal()?;
    let p = portal.profile()?;
    if ctx.fmt.json {
        ctx.fmt.print_json(&p)?;
    } else {
        let mut pairs = vec![
            ("Username", opt(&p.username)),
            ("First name", opt(&p.first_name)),
            ("Last name", opt(&p.last_name)),
            ("Email", opt(&p.email)),
        ];
        // Keep String values alive for the borrow in print_kv.
        let extra: Vec<(String, String)> = p
            .extra
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in &extra {
            pairs.push((k.as_str(), v.clone()));
        }
        ctx.fmt.print_kv("Profile", &pairs);
    }
    Ok(())
}

// --- ebill ----------------------------------------------------------------

pub fn ebill(ctx: &Ctx, cmd: &EbillCmd) -> Result<()> {
    let EbillCmd::Status = cmd;
    let portal = ctx.portal()?;
    let e = portal.enrollment()?;
    if ctx.fmt.json {
        ctx.fmt.print_json(&e)?;
    } else {
        ctx.fmt.print_kv(
            "Enrollment",
            &[
                ("Account #", opt(&e.account_number)),
                ("Paperless / eBill", tri_state(e.paperless).into()),
                ("eBill email", opt(&e.ebill_email)),
                ("Autopay / bank draft", tri_state(e.autopay).into()),
                ("Autopay plan", opt(&e.autopay_plan)),
                ("Draw day", opt(&e.autopay_draw_day)),
                ("Draw amount", opt(&e.autopay_draw_amount)),
            ],
        );
    }
    Ok(())
}

// --- service --------------------------------------------------------------

pub fn service(ctx: &Ctx) -> Result<()> {
    let portal = ctx.portal()?;
    let s = portal.service_info()?;
    if ctx.fmt.json {
        ctx.fmt.print_json(&s)?;
    } else {
        let payment = match (
            &s.last_payment_description,
            &s.last_payment_amount,
            &s.last_payment_date,
        ) {
            (None, None, None) => "—".to_string(),
            (d, a, dt) => format!("{} {} {}", opt(d), opt(a), opt(dt)),
        };
        ctx.fmt.print_kv(
            "Service Information",
            &[
                ("Service", opt(&s.service)),
                ("Last read date", opt(&s.last_read_date)),
                ("Last bill date", opt(&s.last_bill_date)),
                ("Last bill amount", opt(&s.last_bill_amount)),
                ("Due date", opt(&s.due_date)),
                ("Last payment", payment),
            ],
        );
    }
    Ok(())
}

// --- contact --------------------------------------------------------------

pub fn contact(ctx: &Ctx) -> Result<()> {
    let portal = ctx.portal()?;
    let c = portal.contact();
    if ctx.fmt.json {
        ctx.fmt.print_json(&c)?;
    } else {
        ctx.fmt.print_kv(
            "Town of Jupiter Utilities",
            &[
                ("Department", c.department),
                ("Phone", c.phone),
                ("Portal", c.portal),
                ("Utilities home", c.utilities_home),
                ("Rates", c.rates_url),
            ],
        );
    }
    Ok(())
}

// --- config ---------------------------------------------------------------

pub fn config_cmd(ctx: &Ctx, cmd: &ConfigCmd) -> Result<()> {
    match cmd {
        ConfigCmd::Path => {
            let p = Config::default_path()?;
            println!("{}", p.display());
            Ok(())
        }
        ConfigCmd::Init => {
            let existing = Config::load()?;
            let path = existing.save()?;
            println!("✓ Wrote config to {}", path.display());
            println!("Edit it to set your username, or run `tojfl auth login --save`.");
            Ok(())
        }
        ConfigCmd::Show => {
            if ctx.fmt.json {
                ctx.fmt.print_json(&ctx.cfg)?;
            } else {
                ctx.fmt.print_kv(
                    "Configuration",
                    &[
                        ("Username", opt(&ctx.cfg.username)),
                        ("Default account", opt(&ctx.cfg.default_account)),
                        ("Base URL", opt(&ctx.cfg.base_url)),
                        ("Output", opt(&ctx.cfg.output)),
                        (
                            "Password in keychain",
                            config::keychain_get()
                                .ok()
                                .flatten()
                                .map(|_| "yes".to_string())
                                .unwrap_or_else(|| "no".to_string()),
                        ),
                    ],
                );
            }
            Ok(())
        }
        ConfigCmd::Set { key, value } => config_set(key, Some(value)),
        ConfigCmd::Unset { key } => config_set(key, None),
        ConfigCmd::SetPassword => {
            let pw = prompt_password("Portal password: ")?;
            config::keychain_set(&pw)?;
            println!("✓ Password stored in the OS keychain.");
            Ok(())
        }
        ConfigCmd::ClearPassword => {
            config::keychain_clear()?;
            println!("✓ Password removed from the OS keychain.");
            Ok(())
        }
    }
}

/// Set (or, with `value: None`, clear) a config key and persist the file.
/// Loads the on-disk config so transient CLI overrides aren't written back.
fn config_set(key: &str, value: Option<&str>) -> Result<()> {
    let mut cfg = Config::load()?;
    apply_config_key(&mut cfg, key, value)?;
    let path = cfg.save()?;
    let verb = if value.is_some() { "Set" } else { "Cleared" };
    println!("✓ {verb} {key} in {}", path.display());
    Ok(())
}

/// Apply a single key/value to a [`Config`] in memory, validating the key and
/// value. Kept pure (no IO) so it's unit-testable. Password is intentionally not
/// settable here — use `config set-password` (it belongs in the keychain).
fn apply_config_key(cfg: &mut Config, key: &str, value: Option<&str>) -> Result<()> {
    let usage = |m: String| -> anyhow::Error { tojfl_sdk::Error::Invalid(m).into() };
    match key {
        // `account` is the piekstra-cli/1 spec key (DESIGN.md §1.2); accept the
        // `default_account` field name as an alias.
        "account" | "default_account" => cfg.default_account = value.map(String::from),
        "username" => cfg.username = value.map(String::from),
        "base_url" => cfg.base_url = value.map(String::from),
        "output" => {
            if let Some(v) = value {
                if !matches!(v, "table" | "json" | "csv") {
                    return Err(usage(format!("output must be table|json|csv, got '{v}'")));
                }
            }
            cfg.output = value.map(String::from);
        }
        "timeout_secs" => {
            cfg.timeout_secs = match value {
                Some(v) => Some(
                    v.parse()
                        .map_err(|_| usage(format!("timeout_secs must be a number, got '{v}'")))?,
                ),
                None => None,
            };
        }
        "auto_login" => {
            cfg.auto_login = match value {
                Some(v) => Some(
                    v.parse()
                        .map_err(|_| usage(format!("auto_login must be true|false, got '{v}'")))?,
                ),
                None => None,
            };
        }
        other => {
            return Err(usage(format!(
                "unknown config key '{other}' (settable: account, username, base_url, \
                 output, timeout_secs, auto_login)"
            )));
        }
    }
    Ok(())
}

// --- helpers --------------------------------------------------------------

fn resolve_password(args: &LoginArgs) -> Result<String> {
    if let Ok(pw) = std::env::var("TOJFL_PASSWORD") {
        if !pw.is_empty() {
            return Ok(pw);
        }
    }
    if args.password_stdin {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        return Ok(buf.trim_end_matches(['\n', '\r']).to_string());
    }
    // Non-interactive callers (cron, a dashboard) have no TTY to prompt at, so
    // fall back to a stored keychain password. Interactive users still get the
    // prompt, so they can enter a new password (e.g. after a portal change).
    if !std::io::stdin().is_terminal() {
        if let Ok(Some(pw)) = config::keychain_get() {
            return Ok(pw);
        }
    }
    prompt_password("Portal password: ")
}

fn prompt_password(prompt: &str) -> Result<String> {
    rpassword::prompt_password(prompt).context("reading password")
}

fn prompt_line(prompt: &str) -> Result<String> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

fn open_in_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(url).status();
    #[cfg(target_os = "linux")]
    let status = std::process::Command::new("xdg-open").arg(url).status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", url])
        .status();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let status: std::io::Result<std::process::ExitStatus> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "unsupported platform",
    ));

    status
        .map(|_| ())
        .with_context(|| format!("opening browser for {url}"))
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n:.2}")
    }
}

fn tri_state(b: Option<bool>) -> &'static str {
    match b {
        Some(true) => "enrolled",
        Some(false) => "not enrolled",
        None => "unknown",
    }
}

/// Resolve a `--since`/`--until` pair into comparable dates for history
/// filtering, reporting an unparseable value as a usage error (so it exits with
/// the family's usage-error code rather than a generic failure).
fn date_bounds(
    since: &Option<String>,
    until: &Option<String>,
) -> Result<(Option<tojfl_sdk::date::Ymd>, Option<tojfl_sdk::date::Ymd>)> {
    Ok((date_bound(since, "since")?, date_bound(until, "until")?))
}

fn date_bound(value: &Option<String>, flag: &str) -> Result<Option<tojfl_sdk::date::Ymd>> {
    match value {
        None => Ok(None),
        Some(v) => tojfl_sdk::date::parse(v).map(Some).ok_or_else(|| {
            tojfl_sdk::Error::Invalid(format!(
                "invalid --{flag} date '{v}' (use YYYY-MM-DD, MM/DD/YYYY, or 'Mon DD, YYYY')"
            ))
            .into()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::apply_config_key;
    use tojfl_sdk::Config;

    #[test]
    fn apply_config_key_sets_clears_and_validates() {
        let mut cfg = Config::default();

        // Spec key `account` and the `default_account` alias both set the field.
        apply_config_key(&mut cfg, "account", Some("000000")).unwrap();
        assert_eq!(cfg.default_account.as_deref(), Some("000000"));
        apply_config_key(&mut cfg, "default_account", None).unwrap();
        assert_eq!(cfg.default_account, None);

        apply_config_key(&mut cfg, "output", Some("csv")).unwrap();
        assert_eq!(cfg.output.as_deref(), Some("csv"));
        assert!(apply_config_key(&mut cfg, "output", Some("xml")).is_err());

        apply_config_key(&mut cfg, "timeout_secs", Some("45")).unwrap();
        assert_eq!(cfg.timeout_secs, Some(45));
        assert!(apply_config_key(&mut cfg, "timeout_secs", Some("soon")).is_err());

        apply_config_key(&mut cfg, "auto_login", Some("false")).unwrap();
        assert_eq!(cfg.auto_login, Some(false));
        assert!(apply_config_key(&mut cfg, "auto_login", Some("maybe")).is_err());

        // Unknown key is a usage error.
        assert!(apply_config_key(&mut cfg, "password", Some("x")).is_err());
        assert!(apply_config_key(&mut cfg, "nope", Some("x")).is_err());
    }
}
