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
pub const BILLING_HISTORY: &str = "/BillingHistory.aspx";
pub const USAGE_HISTORY: &str = "/UsageHistory.aspx";
pub const TRANSACTION_HISTORY: &str = "/TransactionHistory.aspx";
pub const USER_PROFILE: &str = "/UserProfile.aspx";

/// True if a fetched page is actually the login page (i.e. we got bounced).
pub fn looks_like_login(html: &str) -> bool {
    html.contains("Login_DNN$txtUsername") || html.contains("txtUsername")
}
