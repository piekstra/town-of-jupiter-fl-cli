//! Low-level HTTP client for the portal.
//!
//! Wraps a blocking `reqwest` client with a cookie jar and manual redirect
//! following. We follow redirects by hand (rather than letting reqwest do it)
//! so we can observe every `Set-Cookie` — that's how DNN hands us the forms-auth
//! cookie during login, and we need to capture it to persist the session.

use crate::error::{Error, Result};
use reqwest::blocking::{Client as HttpClient, Response};
use reqwest::cookie::Jar;
use reqwest::header::{HeaderMap, HeaderValue, LOCATION, SET_COOKIE, USER_AGENT};
use reqwest::redirect::Policy;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const UA: &str = "tojfl/0.1 (+https://github.com/piekstra/town-of-jupiter-fl-cli) reqwest";
const MAX_REDIRECTS: usize = 10;
/// Total send attempts (so 2 retries) on a transient failure. The portal is an
/// old single-TLS-suite server that occasionally resets or 503s; long-lived
/// callers (dashboards, cron) benefit from riding out a blip.
const MAX_ATTEMPTS: u32 = 3;
const RETRY_BASE: Duration = Duration::from_millis(200);

/// Whether an HTTP status is worth retrying — a server-side transient (rate
/// limit or gateway/unavailable). 500 is excluded: it can mean the request was
/// processed and then failed, so retrying could double-apply it.
fn is_transient_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 502 | 503 | 504)
}

/// Whether a transport error is worth retrying: no or incomplete response
/// (connect failure, timeout, reset), so a retry can't double-process a request.
fn is_transient_error(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect() || e.is_request()
}

/// Exponential backoff after attempt `attempt` (1-based) failed: 200ms, 400ms,
/// 800ms… capped so the exponent can't overflow.
fn backoff_delay(attempt: u32) -> Duration {
    RETRY_BASE * 2u32.pow(attempt.min(6).saturating_sub(1))
}

/// A cookie-aware HTTP client bound to a single portal base URL.
pub struct Client {
    http: HttpClient,
    jar: Arc<Jar>,
    base: Url,
    /// Latest observed value for each cookie name, kept for session persistence.
    cookies: RefCell<BTreeMap<String, String>>,
}

impl Client {
    /// Build a client for `base_url` with the given request timeout.
    pub fn new(base_url: &str, timeout: Duration) -> Result<Client> {
        let base = Url::parse(base_url)
            .map_err(|e| Error::Config(format!("invalid base url `{base_url}`: {e}")))?;
        let jar = Arc::new(Jar::default());
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(UA));
        let http = HttpClient::builder()
            .cookie_provider(jar.clone())
            .redirect(Policy::none())
            .timeout(timeout)
            .default_headers(headers)
            .build()?;
        Ok(Client {
            http,
            jar,
            base,
            cookies: RefCell::new(BTreeMap::new()),
        })
    }

    pub fn base(&self) -> &Url {
        &self.base
    }

    /// Resolve a path (or absolute URL) against the base.
    pub fn url(&self, path: &str) -> Result<Url> {
        if path.starts_with("http://") || path.starts_with("https://") {
            Url::parse(path).map_err(|e| Error::invalid(format!("bad url `{path}`: {e}")))
        } else {
            self.base
                .join(path)
                .map_err(|e| Error::invalid(format!("bad path `{path}`: {e}")))
        }
    }

    /// Seed the jar and persistence map from previously saved cookie strings.
    pub fn seed_cookies(&self, cookies: &[String]) {
        for raw in cookies {
            self.jar.add_cookie_str(raw, &self.base);
            if let Some((name, value)) = raw.split(';').next().and_then(|nv| nv.split_once('=')) {
                self.cookies
                    .borrow_mut()
                    .insert(name.trim().to_string(), value.trim().to_string());
            }
        }
    }

    /// Current cookies as `name=value` strings, suitable for saving a session.
    pub fn snapshot_cookies(&self) -> Vec<String> {
        self.cookies
            .borrow()
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect()
    }

    /// True once we appear to hold a DNN forms-auth cookie.
    pub fn has_auth_cookie(&self) -> bool {
        self.cookies.borrow().keys().any(|k| {
            k.eq_ignore_ascii_case(".DOTNETNUKE") || k.eq_ignore_ascii_case("DNNPersonalization")
        })
    }

    fn record_cookies(&self, resp: &Response) {
        for hv in resp.headers().get_all(SET_COOKIE).iter() {
            if let Ok(s) = hv.to_str() {
                if let Some((name, value)) = s.split(';').next().and_then(|nv| nv.split_once('=')) {
                    self.cookies
                        .borrow_mut()
                        .insert(name.trim().to_string(), value.trim().to_string());
                }
            }
        }
    }

    /// Send a request, retrying transient transport errors and 429/5xx-gateway
    /// responses with exponential backoff. `make` rebuilds the request for each
    /// attempt (a `RequestBuilder` is single-use).
    fn send_retrying(
        &self,
        make: impl Fn() -> reqwest::blocking::RequestBuilder,
    ) -> Result<Response> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match make().send() {
                Ok(resp) => {
                    if is_transient_status(resp.status()) && attempt < MAX_ATTEMPTS {
                        std::thread::sleep(backoff_delay(attempt));
                        continue;
                    }
                    // Retries exhausted on a transient status: surface it as an
                    // error (→ Upstream/exit 5) rather than handing back the
                    // error-page body as if it were the requested content.
                    if is_transient_status(resp.status()) {
                        return match resp.error_for_status() {
                            Ok(ok) => Ok(ok),
                            Err(e) => Err(Error::Http(e)),
                        };
                    }
                    return Ok(resp);
                }
                Err(e) if is_transient_error(&e) && attempt < MAX_ATTEMPTS => {
                    std::thread::sleep(backoff_delay(attempt));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// GET a page, following redirects manually and capturing cookies.
    pub fn get(&self, path: &str) -> Result<Response> {
        let mut url = self.url(path)?;
        let mut resp = self.send_retrying(|| self.http.get(url.clone()))?;
        self.record_cookies(&resp);
        let mut hops = 0;
        while resp.status().is_redirection() && hops < MAX_REDIRECTS {
            url = self.redirect_target(&resp, &url)?;
            resp = self.send_retrying(|| self.http.get(url.clone()))?;
            self.record_cookies(&resp);
            hops += 1;
        }
        Ok(resp)
    }

    /// GET a page and return its body text.
    pub fn get_text(&self, path: &str) -> Result<String> {
        Ok(self.get(path)?.text()?)
    }

    /// GET a URL and return the raw response body bytes (for binary downloads
    /// such as statement PDFs).
    pub fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        Ok(self.get(path)?.bytes()?.to_vec())
    }

    /// POST a form-urlencoded body, following redirects (as GET) and capturing
    /// cookies. Returns the final response.
    pub fn post_form(&self, path: &str, form: &[(String, String)]) -> Result<Response> {
        let url = self.url(path)?;
        // Safe to retry: the portal's POSTs are idempotent form-postbacks that
        // re-fetch state (login, lookups) — none create a resource, so a replay
        // can't double-submit anything.
        let mut resp = self.send_retrying(|| self.http.post(url.clone()).form(form))?;
        self.record_cookies(&resp);
        let mut current = url;
        let mut hops = 0;
        while resp.status().is_redirection() && hops < MAX_REDIRECTS {
            current = self.redirect_target(&resp, &current)?;
            resp = self.send_retrying(|| self.http.get(current.clone()))?;
            self.record_cookies(&resp);
            hops += 1;
        }
        Ok(resp)
    }

    /// POST a form and return the final body text.
    pub fn post_form_text(&self, path: &str, form: &[(String, String)]) -> Result<String> {
        Ok(self.post_form(path, form)?.text()?)
    }

    fn redirect_target(&self, resp: &Response, current: &Url) -> Result<Url> {
        let loc = resp
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::parse("redirect response had no Location header"))?;
        current
            .join(loc)
            .map_err(|e| Error::parse(format!("bad redirect location `{loc}`: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::{backoff_delay, is_transient_status};
    use reqwest::StatusCode;
    use std::time::Duration;

    #[test]
    fn transient_statuses_are_retryable() {
        for s in [429u16, 502, 503, 504] {
            assert!(
                is_transient_status(StatusCode::from_u16(s).unwrap()),
                "{s} retryable"
            );
        }
        // 500 excluded (may have processed); success/auth/notfound are not transient.
        for s in [200u16, 401, 404, 500] {
            assert!(
                !is_transient_status(StatusCode::from_u16(s).unwrap()),
                "{s} not retryable"
            );
        }
    }

    #[test]
    fn backoff_grows_and_is_bounded() {
        assert_eq!(backoff_delay(1), Duration::from_millis(200));
        assert_eq!(backoff_delay(2), Duration::from_millis(400));
        assert_eq!(backoff_delay(3), Duration::from_millis(800));
        // Capped exponent — never overflows for large attempt counts.
        assert!(backoff_delay(100) <= Duration::from_millis(200) * 32);
    }
}
