//! The `tojfl mcp` tools, offline: `tests/fixtures/` behind the [`Reads`] seam,
//! mapped by the commands' own mappers, checked by cli-common's conformance
//! helper. `tests/mcp_stdio.rs` and `tests/mcp_http.rs` drive the real binary.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pk_cli_mcp::conformance::{assert_conforms, assert_fits, call, definitions_size};
use pk_cli_mcp::Toolset;
use serde_json::{json, Value};

use super::*;

fn fixture<T: serde::de::DeserializeOwned>(raw: &str) -> T {
    serde_json::from_str(raw).expect("fixture parses")
}

struct Fixtures {
    accounts: Vec<LinkedAccount>,
    summary: Summary,
    bills: Vec<Bill>,
    transactions: Vec<Transaction>,
    usage: Vec<UsageRecord>,
    default_account: Option<String>,
    /// Every read, with its account.
    calls: Rc<RefCell<Vec<String>>>,
    refused: bool,
}

impl Fixtures {
    fn new() -> Self {
        Self {
            accounts: fixture(include_str!("../../tests/fixtures/accounts.json")),
            summary: fixture(include_str!("../../tests/fixtures/summary.json")),
            bills: fixture(include_str!("../../tests/fixtures/bills.json")),
            transactions: fixture(include_str!("../../tests/fixtures/transactions.json")),
            usage: fixture(include_str!("../../tests/fixtures/usage.json")),
            default_account: None,
            calls: Rc::default(),
            refused: false,
        }
    }

    fn log(&self, what: &str, account: Option<&str>) -> Result<(), AppError> {
        self.calls
            .borrow_mut()
            .push(format!("{what} {}", account.unwrap_or("-")));
        if self.refused {
            // The SDK's own wording, which names a command for a terminal.
            return Err(crate::to_cli_error(&anyhow::Error::from(
                SdkError::NotAuthenticated,
            )));
        }
        Ok(())
    }
}

impl Reads for Fixtures {
    fn default_account(&self) -> Result<Option<String>, AppError> {
        Ok(self.default_account.clone())
    }

    fn accounts(&self) -> Result<Vec<LinkedAccount>, AppError> {
        self.log("accounts", None)?;
        Ok(self.accounts.clone())
    }

    fn summary(&self, account: &str) -> Result<Summary, AppError> {
        self.log("summary", Some(account))?;
        Ok(self.summary.clone())
    }

    fn service(&self, account: &str) -> Result<ServiceInfo, AppError> {
        self.log("service", Some(account))?;
        Ok(self.summary.service.clone())
    }

    fn bills(&self, account: &str) -> Result<Vec<Bill>, AppError> {
        self.log("bills", Some(account))?;
        Ok(self.bills.clone())
    }

    fn transactions(&self, account: &str) -> Result<Vec<Transaction>, AppError> {
        self.log("transactions", Some(account))?;
        Ok(self.transactions.clone())
    }

    fn usage(&self, account: &str) -> Result<Vec<UsageRecord>, AppError> {
        self.log("usage", Some(account))?;
        Ok(self.usage.clone())
    }
}

fn tools_over(portal: Fixtures) -> UtilityTools<JupiterSource<Fixtures>> {
    with_source(JupiterSource { portal }, false)
}

fn ok(t: &UtilityTools<JupiterSource<Fixtures>>, tool: &str, args: Value) -> Value {
    let result = call(t, tool, args);
    assert_fits(t, tool, &result).clone()
}

fn upstream(portal: Fixtures, tool: &str) {
    let result = call(&tools_over(portal), tool, json!({}));
    assert!(result.text().starts_with("upstream:"), "{}", result.text());
}

#[test]
fn the_definitions_conform_and_touch_no_portal() {
    let portal = Fixtures::new();
    let calls = portal.calls.clone();
    let t = tools_over(portal);
    assert_conforms(&t);
    assert!(definitions_size(&t) < 8_000);
    let tools = t.tools();
    let names: Vec<&str> = tools.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "jupiter_balance",
            "jupiter_bills",
            "jupiter_payments",
            "jupiter_usage"
        ]
    );
    assert!(calls.borrow().is_empty());
}

#[test]
fn balance_names_accounts_by_address_and_shows_only_the_end_of_the_number() {
    let t = tools_over(Fixtures::new());
    let out = ok(&t, "jupiter_balance", json!({}));
    assert_eq!(out["balance"], "12.34");
    assert_eq!(out["due_date"], "2026-08-01");
    assert_eq!(out["autopay"], false);
    assert_eq!(out["account"], "100 Example Street");
    assert!(out.get("account_id").is_none(), "{out}");
    assert_eq!(out["accounts"][1]["name"], "200 Example Street");
    assert!(!out.to_string().contains("EXAMPLE OWNER"), "{out}");

    let out = ok(
        &t,
        "jupiter_balance",
        json!({"response_format": "detailed"}),
    );
    assert_eq!(out["account_id"], "123456");
}

#[test]
fn an_account_is_named_by_its_address_and_reaches_the_read() {
    let portal = Fixtures::new();
    let calls = portal.calls.clone();
    let t = tools_over(portal);
    ok(&t, "jupiter_bills", json!({"account": "200 Example"}));
    assert!(
        calls.borrow().contains(&"bills 234567".to_string()),
        "{:?}",
        calls.borrow()
    );
}

#[test]
fn bills_come_newest_first_with_iso_dates_and_no_paid_state() {
    let t = tools_over(Fixtures::new());
    let page = ok(&t, "jupiter_bills", json!({}));
    assert_eq!(page["returned"], 3);
    assert_eq!(page["items"][0]["date"], "2026-07-05");
    assert_eq!(page["items"][0]["amount"], "12.34");
    assert_eq!(page["items"][0]["due_date"], "2026-08-01");
    assert!(page["items"][0].get("paid").is_none(), "{page}");
    assert_eq!(page["items"][2]["date"], "2026-03-05");

    let page = ok(&t, "jupiter_bills", json!({"since": "2026-05-01"}));
    assert_eq!(page["returned"], 2);
}

#[test]
fn payments_are_credits_the_portal_calls_payments_as_positive_amounts() {
    let t = tools_over(Fixtures::new());
    let page = ok(&t, "jupiter_payments", json!({}));
    assert_eq!(page["returned"], 2);
    assert_eq!(page["items"][0]["date"], "2026-06-20");
    assert_eq!(page["items"][0]["amount"], "23.45");
    assert_eq!(page["items"][0]["status"], "pending");
    assert_eq!(page["items"][1]["amount"], "34.56");
    assert!(page["items"][1].get("status").is_none(), "{page}");
}

#[test]
fn usage_comes_newest_first_by_month_and_filters_by_it() {
    let t = tools_over(Fixtures::new());
    let page = ok(&t, "jupiter_usage", json!({}));
    assert_eq!(page["returned"], 3);
    assert_eq!(page["items"][0]["period"], "2026-06");
    assert_eq!(page["items"][0]["quantity"], 1000.0);
    assert_eq!(page["items"][0]["unit"], "GAL");
    assert_eq!(page["items"][2]["period"], "2026-04");

    let page = ok(&t, "jupiter_usage", json!({"since": "2026-05-15"}));
    assert_eq!(page["returned"], 2);
}

#[test]
fn a_period_this_cannot_read_stays_as_the_portal_wrote_it() {
    let mut portal = Fixtures::new();
    portal.usage = fixture(r#"[{"period": "Summer cycle", "quantity": 1.0, "extra": {}}]"#);
    let t = tools_over(portal);
    let page = ok(&t, "jupiter_usage", json!({}));
    assert_eq!(page["items"][0]["period"], "Summer cycle");
}

#[test]
fn an_answer_the_portal_did_not_give_is_an_error_not_zero_or_empty() {
    let mut portal = Fixtures::new();
    portal.summary.account.balance = None;
    upstream(portal, "jupiter_balance");

    let mut portal = Fixtures::new();
    portal.bills = fixture(r#"[{"date": "07/05/2026", "extra": {}}]"#);
    upstream(portal, "jupiter_bills");

    let mut portal = Fixtures::new();
    portal.bills = fixture(r#"[{"date": "", "amount": {"cents": 1}, "extra": {}}]"#);
    upstream(portal, "jupiter_bills");

    let mut portal = Fixtures::new();
    portal.transactions = fixture(
        r#"[{"date": "07/05/2026", "description": "", "amount": {"cents": -1}, "extra": {}}]"#,
    );
    upstream(portal, "jupiter_payments");

    let mut portal = Fixtures::new();
    portal.usage = fixture(r#"[{"period": "Jun 2026", "extra": {}}]"#);
    upstream(portal, "jupiter_usage");
}

#[test]
fn a_bill_with_no_readable_amount_is_left_out_rather_than_shown_as_zero() {
    let mut portal = Fixtures::new();
    portal.bills[0].amount = None;
    let t = tools_over(portal);
    assert_eq!(ok(&t, "jupiter_bills", json!({}))["returned"], 2);
}

#[test]
fn an_empty_list_is_an_error_when_the_service_page_says_there_is_something() {
    for tool in ["jupiter_bills", "jupiter_payments", "jupiter_usage"] {
        let mut portal = Fixtures::new();
        portal.bills.clear();
        portal.transactions.clear();
        portal.usage.clear();
        upstream(portal, tool);
    }
}

#[test]
fn an_empty_list_is_an_empty_list_when_the_service_page_agrees() {
    let mut portal = Fixtures::new();
    portal.bills.clear();
    portal.transactions.clear();
    portal.usage.clear();
    portal.summary.service = ServiceInfo::default();
    let t = tools_over(portal);
    for tool in ["jupiter_bills", "jupiter_payments", "jupiter_usage"] {
        assert_eq!(ok(&t, tool, json!({}))["returned"], 0, "{tool}");
    }
}

#[test]
fn a_ledger_that_starts_after_the_last_payment_shows_no_payments() {
    let mut portal = Fixtures::new();
    portal
        .transactions
        .retain(|t| t.description == "Utility Bill");
    let t = tools_over(portal);
    assert_eq!(ok(&t, "jupiter_payments", json!({}))["returned"], 0);
}

#[test]
fn a_ledger_that_reaches_the_last_payment_and_shows_none_is_an_error() {
    let mut portal = Fixtures::new();
    // Every amount positive, as a change of sign on the page would make them.
    for row in &mut portal.transactions {
        row.amount = row.amount.map(|m| Money::from_cents(m.cents.abs()));
    }
    upstream(portal, "jupiter_payments");
}

#[test]
fn a_ledger_covers_a_date_from_its_oldest_row_on() {
    let rows = Fixtures::new().transactions;
    assert!(ledger_covers(&rows, Some("04/01/2026")));
    assert!(ledger_covers(&rows, Some("06/20/2026")));
    assert!(!ledger_covers(&rows, Some("03/31/2026")));
    assert!(!ledger_covers(&rows, Some("not a date")));
    assert!(!ledger_covers(&rows, None));
    assert!(!ledger_covers(&[], Some("06/20/2026")));
}

#[test]
fn a_call_naming_no_account_reads_the_default_and_then_the_first_linked() {
    let mut portal = Fixtures::new();
    portal.default_account = Some("234567".into());
    let calls = portal.calls.clone();
    ok(&tools_over(portal), "jupiter_bills", json!({}));
    assert_eq!(*calls.borrow(), ["bills 234567"]);

    let portal = Fixtures::new();
    let calls = portal.calls.clone();
    ok(&tools_over(portal), "jupiter_usage", json!({}));
    assert_eq!(*calls.borrow(), ["accounts -", "usage 123456"]);
}

#[test]
fn an_empty_list_of_accounts_is_a_page_this_could_not_read() {
    // With no default there is no account to read.
    let mut portal = Fixtures::new();
    portal.accounts.clear();
    upstream(portal, "jupiter_balance");

    let mut portal = Fixtures::new();
    portal.accounts.clear();
    upstream(portal, "jupiter_bills");

    // With one, the balance says the list is unavailable, not that there is one
    // account.
    let mut portal = Fixtures::new();
    portal.accounts.clear();
    portal.default_account = Some("123456".into());
    let out = ok(&tools_over(portal), "jupiter_balance", json!({}));
    assert_eq!(out["balance"], "12.34");
    assert_eq!(out["accounts_unavailable"], true);
}

mod scoped_reads {
    use super::*;

    /// A portal session: the account it reports, and every selection made of it.
    struct FakeSession {
        active: RefCell<Option<String>>,
        selected: RefCell<Vec<String>>,
        /// The portal's home page has stopped stating a number.
        states_number: bool,
        /// Selecting this account fails.
        fails_on: Option<&'static str>,
    }

    impl FakeSession {
        fn on(account: Option<&str>) -> Self {
            Self {
                active: RefCell::new(account.map(str::to_string)),
                selected: RefCell::default(),
                states_number: true,
                fails_on: None,
            }
        }

        fn selected(&self) -> Vec<String> {
            self.selected.borrow().clone()
        }

        fn active(&self) -> Option<String> {
            self.active.borrow().clone()
        }
    }

    impl ActiveAccount for FakeSession {
        fn reported(&self) -> Result<Option<String>, SdkError> {
            Ok(self.active().filter(|_| self.states_number))
        }

        fn select(&self, account: &str) -> Result<(), SdkError> {
            self.selected.borrow_mut().push(account.to_string());
            if self.fails_on == Some(account) {
                return Err(SdkError::Portal("no".into()));
            }
            *self.active.borrow_mut() = Some(account.to_string());
            Ok(())
        }
    }

    fn is_upstream(err: SdkError) -> bool {
        matches!(cli_error(err), AppError::Upstream(_))
    }

    #[test]
    fn the_active_account_is_read_with_no_selection() {
        // Leading zeros differ between the portal's pages.
        let session = FakeSession::on(Some("0123456"));
        let read = scoped(&session, "123456", || Ok("rows"));
        assert_eq!(read.unwrap(), "rows");
        assert!(session.selected().is_empty());
    }

    #[test]
    fn another_account_is_selected_read_and_selected_back() {
        let session = FakeSession::on(Some("234567"));
        let read = scoped(&session, "123456", || {
            assert_eq!(session.active().as_deref(), Some("123456"));
            Ok("rows")
        });
        assert_eq!(read.unwrap(), "rows");
        assert_eq!(session.selected(), ["123456", "234567"]);
        assert_eq!(session.active().as_deref(), Some("234567"));
    }

    #[test]
    fn a_session_another_process_moved_is_an_error_and_is_left_where_it_is() {
        let session = FakeSession::on(Some("234567"));
        let read: Result<&str, _> = scoped(&session, "123456", || {
            *session.active.borrow_mut() = Some("345678".into());
            Ok("rows of another account")
        });
        assert!(is_upstream(read.unwrap_err()));
        assert_eq!(session.selected(), ["123456"]);
        assert_eq!(session.active().as_deref(), Some("345678"));
    }

    #[test]
    fn a_home_page_that_states_no_number_cannot_confirm_a_read() {
        let mut session = FakeSession::on(Some("123456"));
        session.states_number = false;
        let read: Result<&str, _> = scoped(&session, "123456", || Ok("rows"));
        assert!(is_upstream(read.unwrap_err()));
    }

    #[test]
    fn a_session_with_no_account_showing_is_selected_and_stays() {
        let session = FakeSession::on(None);
        let read = scoped(&session, "123456", || Ok("rows"));
        assert_eq!(read.unwrap(), "rows");
        assert_eq!(session.selected(), ["123456"]);
    }

    #[test]
    fn a_failed_read_still_selects_back() {
        let session = FakeSession::on(Some("234567"));
        let read: Result<&str, _> = scoped(&session, "123456", || Err(SdkError::NotAuthenticated));
        assert!(matches!(read, Err(SdkError::NotAuthenticated)));
        assert_eq!(session.selected(), ["123456", "234567"]);
    }

    #[test]
    fn failing_to_select_back_does_not_cost_the_read() {
        let mut session = FakeSession::on(Some("234567"));
        session.fails_on = Some("234567");
        let read = scoped(&session, "123456", || Ok("rows"));
        assert_eq!(read.unwrap(), "rows");
    }

    #[test]
    fn failing_to_select_the_account_reads_nothing() {
        let mut session = FakeSession::on(Some("234567"));
        session.fails_on = Some("123456");
        let read: Result<&str, _> = scoped(&session, "123456", || panic!("read anyway"));
        assert!(read.is_err());
    }

    #[test]
    fn a_not_linked_error_holds_the_linked_numbers_back() {
        let err = without_numbers(SdkError::NotFound(
            "account 999999 is not linked to this login (linked: 123456, 234567)".into(),
        ));
        let text = err.to_string();
        assert!(text.contains("default_account"), "{text}");
        for number in ["999999", "123456", "234567"] {
            assert!(!text.contains(number), "{text}");
        }
        assert!(matches!(
            without_numbers(SdkError::Portal("kept".into())),
            SdkError::Portal(_)
        ));
    }
}

#[test]
fn a_bad_argument_reaches_no_portal() {
    let portal = Fixtures::new();
    let calls = portal.calls.clone();
    let t = tools_over(portal);
    for (tool, args) in [
        ("jupiter_payments", json!({"since": "yesterday"})),
        ("jupiter_bills", json!({"limit": 0})),
        ("jupiter_usage", json!({"until": "2026-13-01"})),
    ] {
        let result = call(&t, tool, args);
        assert!(result.text().starts_with("usage:"), "{}", result.text());
    }
    assert!(calls.borrow().is_empty(), "{:?}", calls.borrow());
}

#[test]
fn a_lapsed_session_asks_for_the_one_sign_in_command() {
    let mut portal = Fixtures::new();
    portal.refused = true;
    let t = tools_over(portal);
    let result = call(&t, "jupiter_usage", json!({}));
    let text = result.text();
    assert!(text.starts_with("auth:"), "{text}");
    assert!(text.contains("`tojfl auth login --save`"), "{text}");
    assert!(!text.contains("auth login` first"), "{text}");
}

#[test]
fn a_password_in_the_environment_points_at_where_it_is_set() {
    let mut portal = Fixtures::new();
    portal.refused = true;
    let t = with_source(JupiterSource { portal }, true);
    let result = call(&t, "jupiter_usage", json!({}));
    let text = result.text();
    assert!(text.contains("Claude Desktop extension"), "{text}");
    assert!(!text.contains("tojfl auth login"), "{text}");
}

#[test]
fn a_config_error_is_no_usage_error() {
    assert!(matches!(
        cli_error(SdkError::Config("bad file".into())),
        AppError::Other(_)
    ));
    assert!(matches!(
        cli_error(SdkError::Auth("refused".into())),
        AppError::Auth(_)
    ));
}

#[test]
fn a_stored_password_signs_in_only_where_a_session_existed() {
    assert!(may_sign_in(None, true, false));
    assert!(!may_sign_in(None, false, false), "a logout must stay");
    assert!(may_sign_in(None, false, true), "the extension's way in");
    assert!(may_sign_in(Some(true), true, false));
    assert!(!may_sign_in(Some(false), true, true), "the config says no");
}

mod sign_in {
    use super::*;

    /// A login that counts its calls and answers as told.
    fn login_with<'a>(
        calls: &'a Cell<u32>,
        answer: impl Fn() -> Result<(), SdkError> + 'a,
    ) -> impl Fn(&str, &str) -> Result<(), SdkError> + 'a {
        move |_, _| {
            calls.set(calls.get() + 1);
            answer()
        }
    }

    fn refuse() -> Result<(), SdkError> {
        Err(SdkError::Auth("invalid username or password".into()))
    }

    fn secret(value: &str) -> Secret {
        Secret::new(value)
    }

    #[test]
    fn a_refused_password_is_not_sent_again_and_a_different_one_is() {
        let calls = Cell::new(0);
        let mut state = Refused::default();
        let login = login_with(&calls, refuse);
        assert!(state.sign_in("user", secret("old"), &login).is_err());
        let again = state.sign_in("user", secret("old"), &login).unwrap_err();
        assert_eq!(calls.get(), 1, "the refused password went out again");
        assert!(again.to_string().contains("not sent again"), "{again}");

        let login = login_with(&calls, || Ok(()));
        assert!(state.sign_in("user", secret("new"), &login).is_ok());
        assert_eq!(calls.get(), 2);
        assert!(state.0.is_none());
    }

    #[test]
    fn the_same_password_under_a_new_username_is_sent_once() {
        let calls = Cell::new(0);
        let mut state = Refused::default();
        let login = login_with(&calls, refuse);
        assert!(state.sign_in("mistyped", secret("p"), &login).is_err());
        assert!(state.sign_in("mistyped", secret("p"), &login).is_err());
        assert_eq!(calls.get(), 1);

        let login = login_with(&calls, || Ok(()));
        assert!(state.sign_in("right", secret("p"), &login).is_ok());
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn the_portal_being_down_is_not_a_refusal() {
        let calls = Cell::new(0);
        let mut state = Refused::default();
        let down = login_with(&calls, || Err(SdkError::Portal("HTTP 503".into())));
        assert!(state.sign_in("user", secret("p"), &down).is_err());
        assert!(state.sign_in("user", secret("p"), &down).is_err());
        assert_eq!(calls.get(), 2, "an outage must not stop the next try");
    }
}

/// A keychain that counts its reads and holds one password for any account.
struct StubKeychain {
    password: Option<&'static str>,
    reads: Cell<u32>,
}

impl StubKeychain {
    fn holding(password: Option<&'static str>) -> Self {
        Self {
            password,
            reads: Cell::new(0),
        }
    }
}

impl KeychainRead for StubKeychain {
    fn service(&self) -> &str {
        "stub"
    }

    fn get(&self, _account: &str) -> Result<Option<Secret>, AppError> {
        self.reads.set(self.reads.get() + 1);
        Ok(self.password.map(Secret::new))
    }
}

// Each test names its own variable: the tests share one process's environment.

#[test]
fn the_environment_password_comes_before_the_keychain() {
    const VAR: &str = "TOJFL_TEST_PASSWORD_SET";
    std::env::set_var(VAR, "from-env");
    let keychain = StubKeychain::holding(Some("stored"));
    let password = resolve_password(&keychain, VAR, None).unwrap();
    assert_eq!(password.expose(), "from-env");
    assert_eq!(keychain.reads.get(), 0);
}

#[test]
fn an_empty_environment_password_is_an_auth_error_not_a_keychain_read() {
    const VAR: &str = "TOJFL_TEST_PASSWORD_EMPTY";
    std::env::set_var(VAR, "");
    let keychain = StubKeychain::holding(Some("stored"));
    let err = resolve_password(&keychain, VAR, None).err().unwrap();
    assert!(err.to_string().contains(VAR), "{err}");
    assert!(matches!(err, AppError::Auth(_)), "{err:?}");
    assert_eq!(keychain.reads.get(), 0);
}

#[test]
fn an_unset_environment_password_falls_to_the_keychain_then_the_config() {
    let var = "TOJFL_TEST_PASSWORD_UNSET";
    let stored = resolve_password(&StubKeychain::holding(Some("stored")), var, Some("c")).unwrap();
    assert_eq!(stored.expose(), "stored");
    let configured = resolve_password(&StubKeychain::holding(None), var, Some("c")).unwrap();
    assert_eq!(configured.expose(), "c");
    let err = resolve_password(&StubKeychain::holding(None), var, Some(""))
        .err()
        .unwrap();
    assert!(matches!(err, AppError::Auth(_)), "{err:?}");
}

/// Every identity-bearing value in the fixtures is a placeholder.
#[test]
fn the_fixtures_hold_only_placeholder_identifiers() {
    let f = Fixtures::new();
    assert_eq!(f.summary.account.customer_number, "1234567");
    assert_eq!(f.summary.account.account_number, "123456");
    for a in &f.accounts {
        assert_eq!(a.name.as_deref(), Some("EXAMPLE OWNER"));
        assert!(a
            .service_address
            .as_deref()
            .unwrap()
            .ends_with("Example Street"));
    }
}
