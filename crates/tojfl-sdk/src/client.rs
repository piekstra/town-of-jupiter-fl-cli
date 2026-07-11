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

    /// GET a page, following redirects manually and capturing cookies.
    pub fn get(&self, path: &str) -> Result<Response> {
        let mut url = self.url(path)?;
        let mut resp = self.http.get(url.clone()).send()?;
        self.record_cookies(&resp);
        let mut hops = 0;
        while resp.status().is_redirection() && hops < MAX_REDIRECTS {
            let next = self.redirect_target(&resp, &url)?;
            url = next.clone();
            resp = self.http.get(next).send()?;
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
        let mut resp = self.http.post(url.clone()).form(form).send()?;
        self.record_cookies(&resp);
        let mut current = url;
        let mut hops = 0;
        while resp.status().is_redirection() && hops < MAX_REDIRECTS {
            let next = self.redirect_target(&resp, &current)?;
            current = next.clone();
            resp = self.http.get(next).send()?;
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
