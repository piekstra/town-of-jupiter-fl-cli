//! Command handlers. Each returns `anyhow::Result<()>` and prints its own
//! output (table or JSON) via [`Format`].

use crate::cli::*;
use crate::output::{opt, Format};
use anyhow::{anyhow, Context, Result};
use std::io::Read;
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
                portal.username().unwrap_or("(none)")
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
    st.username = portal.username().map(String::from);
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
            "account",
            "balance",
            "bills",
            "usage",
            "transactions",
            "pay",
            "profile",
            "ebill",
            "contact",
        ],
    );
    pk_cli_core::output::json(&serde_json::to_value(&info)?);
    Ok(())
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
        BillsCmd::List { limit } => {
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
                    opt(&b.amount),
                    opt(&b.balance),
                    opt(&b.due_date),
                    if b.document_url.is_some() {
                        "✓".into()
                    } else {
                        "—".into()
                    },
                ]
            })
            .collect();
        ctx.fmt
            .print_table(&["Date", "Amount", "Balance", "Due date", "PDF"], &rows);
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
        return Err(anyhow!(
            "no statement at position {} — the billing history has {} statement(s)",
            args.index,
            items.len()
        ));
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
        UsageCmd::List { limit } => {
            let mut items = portal.usage()?;
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

// --- transactions ---------------------------------------------------------

pub fn transactions(ctx: &Ctx, cmd: &TransactionsCmd) -> Result<()> {
    let portal = ctx.portal()?;
    let mut items = portal.transactions()?;
    let TransactionsCmd::List { limit } = cmd;
    if let Some(n) = limit {
        items.truncate(*n);
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
    let acct = portal.account_summary()?;
    if ctx.fmt.json {
        ctx.fmt.print_json(&serde_json::json!({
            "paperless": acct.paperless,
            "autopay": acct.autopay,
        }))?;
    } else {
        println!("Paperless / eBill: {}", tri_state(acct.paperless));
        println!("Autopay / bank draft: {}", tri_state(acct.autopay));
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
