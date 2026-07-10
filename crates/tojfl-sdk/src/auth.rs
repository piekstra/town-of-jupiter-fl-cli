//! DNN forms authentication against `Login.aspx`.
//!
//! Flow:
//!   1. GET `Login.aspx` to obtain the WebForms hidden fields.
//!   2. Locate the username/password inputs (their DNN-prefixed names vary).
//!   3. POST the fields back with our credentials and the login button as the
//!      `__EVENTTARGET`.
//!   4. Success is detected by receiving a forms-auth cookie and no longer
//!      being shown the login form.

use crate::client::Client;
use crate::dnn::{find_input_name_ending_with, find_name_by_id_ending_with, FormState};
use crate::error::{Error, Result};
use crate::pages;

/// Attempt to log in. On success the client holds session cookies you can
/// snapshot and persist.
pub fn login(client: &Client, username: &str, password: &str) -> Result<()> {
    let page = client.get_text(pages::LOGIN)?;
    let mut form = FormState::from_html(&page);

    let user_field = find_input_name_ending_with(&page, "txtUsername")
        .ok_or_else(|| Error::MissingFormField("txtUsername (login username input)".to_string()))?;
    let pass_field = find_input_name_ending_with(&page, "txtPassword")
        .ok_or_else(|| Error::MissingFormField("txtPassword (login password input)".to_string()))?;

    form.set(&user_field, username);
    form.set(&pass_field, password);

    // The login button posts back via WebForm_DoPostBackWithOptions with the
    // command control as the event target. Its name ends with `cmdLogin`.
    if let Some(login_btn) = find_name_by_id_ending_with(&page, "cmdLogin")
        .or_else(|| find_input_name_ending_with(&page, "cmdLogin"))
    {
        form.with_event(&login_btn, "");
    }

    let action = form
        .action
        .clone()
        .unwrap_or_else(|| pages::LOGIN.to_string());
    let body = client.post_form_text(&action, &form.to_pairs())?;

    // If the response still shows the login form and we lack an auth cookie,
    // authentication failed. DNN echoes an error message we surface if present.
    let still_login = pages::looks_like_login(&body);
    if client.has_auth_cookie() && !still_login {
        return Ok(());
    }
    if !still_login {
        // No login form and we got somewhere — treat as success even if the
        // cookie name differs from what we sniff for.
        return Ok(());
    }

    Err(Error::Auth(extract_login_error(&body).unwrap_or_else(
        || "invalid username or password (portal returned the login form again)".to_string(),
    )))
}

/// Verify an existing session is still valid by fetching a protected page and
/// checking we weren't bounced to login.
pub fn verify(client: &Client) -> Result<bool> {
    let body = client.get_text(pages::USER_PROFILE)?;
    Ok(!pages::looks_like_login(&body))
}

/// Pull a human-readable error message out of a failed DNN login response.
fn extract_login_error(html: &str) -> Option<String> {
    use scraper::{Html, Selector};
    let doc = Html::parse_document(html);
    // DNN renders login failures in a label with id ending in `lblMessage`
    // or in a `.dnnFormMessage`/`.NormalRed` element.
    for sel in [
        "[id$=lblMessage]",
        ".dnnFormMessage",
        ".NormalRed",
        ".dnnFormValidationSummary",
        "span[id*=Error]",
    ] {
        if let Ok(s) = Selector::parse(sel) {
            if let Some(el) = doc.select(&s).next() {
                let text = el.text().collect::<String>().trim().to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }
    None
}
