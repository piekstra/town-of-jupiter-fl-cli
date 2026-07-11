//! Known portal endpoints.
//!
//! These are the `.aspx` pages the Harris eCARE / DNN portal exposes. Public
//! pages are reachable without auth; the rest redirect to `Login.aspx` until a
//! session cookie is present.

/// Default portal origin.
pub const BASE_URL: &str = "https://utilitybill.jupiter.fl.us";

// --- Public pages ---------------------------------------------------------
pub const HOME: &str = "/Home.aspx";
pub const LOGIN: &str = "/Login.aspx";
pub const CONTACT: &str = "/ContactUs.aspx";
pub const REGISTER: &str = "/Register.aspx";
pub const FORGOT_USERNAME: &str = "/ForgotUsername.aspx";
pub const PASSWORD_RECOVERY: &str = "/SecurePasswordRecovery.aspx";
pub const ONLINE_PAYMENT: &str = "/OnlinePayment.aspx";

// --- Authenticated pages (redirect to Login until authed) -----------------
pub const LIST_ACCOUNTS: &str = "/ListAccounts.aspx";
pub const BILLING_HISTORY: &str = "/BillingHistory.aspx";
pub const USAGE_HISTORY: &str = "/UsageHistory.aspx";
pub const TRANSACTION_HISTORY: &str = "/TransactionHistory.aspx";
pub const USER_PROFILE: &str = "/UserProfile.aspx";
/// The real profile surface (DNN ManageUsers). `UserProfile.aspx` shows a
/// message inbox; the "Change Profile" menu link points here.
pub const CHANGE_PROFILE: &str = "/ChangeProfile.aspx";

/// True if a fetched page is actually the login page (i.e. we got bounced).
///
/// Matches on login-specific markers: the DNN login password field, the login
/// skin, or a `returnurl=` back to the page we asked for (which is how the
/// portal signals "authenticate first").
pub fn looks_like_login(html: &str) -> bool {
    html.contains("Login_DNN$txtPassword")
        || html.contains("$txtPassword")
        || html.contains("Login_DNN$txtUsername")
        || html.contains("Skins/LinkLogin")
        || html.contains("returnurl=")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_login_and_bounce_pages() {
        // The real login form.
        assert!(looks_like_login(
            r#"<input name="dnn$ctr1216$Login$Login_DNN$txtPassword" type="password">"#
        ));
        // A protected page that bounced us back with a returnurl.
        assert!(looks_like_login(
            r#"<a href="/Login/tabid/400/Default.aspx?returnurl=%2fBillingHistory.aspx">Login</a>"#
        ));
        // A genuine data page should not be mistaken for login.
        assert!(!looks_like_login(
            r#"<table><tr><th>Bill Date</th><th>Amount</th></tr></table>"#
        ));
    }
}
