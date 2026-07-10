//! Helpers for driving DotNetNuke / ASP.NET WebForms pages.
//!
//! Every page on the portal is a WebForms `<form>` that round-trips a set of
//! hidden fields (`__VIEWSTATE`, `__EVENTVALIDATION`, `__VIEWSTATEGENERATOR`,
//! DNN's `__dnnVariable`, ...). To submit anything we must first GET the page,
//! scrape those fields, then POST them back alongside our own inputs and a
//! `__EVENTTARGET`/`__EVENTARGUMENT` describing the control we "clicked".

use scraper::{Html, Selector};
use std::collections::BTreeMap;

/// The hidden-field state of a rendered WebForms page, plus every named input's
/// current value, so we can faithfully replay a postback.
#[derive(Debug, Clone, Default)]
pub struct FormState {
    /// All `<input>`/`<select>`/`<textarea>` name→value pairs found on the page.
    pub fields: BTreeMap<String, String>,
    /// The `action` attribute of the WebForms `<form>` (usually the same path).
    pub action: Option<String>,
}

impl FormState {
    /// Extract the form state from a page's HTML.
    pub fn from_html(html: &str) -> FormState {
        let doc = Html::parse_document(html);
        let mut fields = BTreeMap::new();

        // Text/hidden/checkbox/radio/submit inputs.
        if let Ok(sel) = Selector::parse("input[name]") {
            for el in doc.select(&sel) {
                let name = match el.value().attr("name") {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let itype = el.value().attr("type").unwrap_or("text").to_lowercase();
                // Unchecked checkboxes/radios do not post a value; skip them.
                if (itype == "checkbox" || itype == "radio") && el.value().attr("checked").is_none()
                {
                    continue;
                }
                // Don't auto-post submit buttons; the caller drives those via
                // __EVENTTARGET so we don't accidentally trigger two actions.
                if itype == "submit" || itype == "button" || itype == "image" {
                    continue;
                }
                let value = el.value().attr("value").unwrap_or("").to_string();
                fields.insert(name, value);
            }
        }

        // <select>: take the selected option (or the first if none marked).
        if let Ok(sel) = Selector::parse("select[name]") {
            if let Ok(opt_sel) = Selector::parse("option") {
                for el in doc.select(&sel) {
                    let name = match el.value().attr("name") {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    let mut chosen: Option<String> = None;
                    let mut first: Option<String> = None;
                    for opt in el.select(&opt_sel) {
                        let val = opt
                            .value()
                            .attr("value")
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| opt.text().collect::<String>().trim().to_string());
                        if first.is_none() {
                            first = Some(val.clone());
                        }
                        if opt.value().attr("selected").is_some() {
                            chosen = Some(val);
                            break;
                        }
                    }
                    if let Some(v) = chosen.or(first) {
                        fields.insert(name, v);
                    }
                }
            }
        }

        // <textarea>.
        if let Ok(sel) = Selector::parse("textarea[name]") {
            for el in doc.select(&sel) {
                if let Some(name) = el.value().attr("name") {
                    fields.insert(name.to_string(), el.text().collect::<String>());
                }
            }
        }

        let action = Selector::parse("form")
            .ok()
            .and_then(|s| doc.select(&s).next())
            .and_then(|f| f.value().attr("action").map(|a| a.to_string()));

        FormState { fields, action }
    }

    /// Get a field value.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(|s| s.as_str())
    }

    /// Set/override a field value, returning self for chaining.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.fields.insert(name.into(), value.into());
        self
    }

    /// Prepare a postback that "clicks" the control named `event_target`.
    /// Mirrors ASP.NET's `__doPostBack(target, argument)`.
    pub fn with_event(&mut self, event_target: &str, event_argument: &str) -> &mut Self {
        self.set("__EVENTTARGET", event_target);
        self.set("__EVENTARGUMENT", event_argument);
        self
    }

    /// Materialize the fields into a form-urlencoded body list.
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        self.fields
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

/// Find the fully-qualified `name` of the first input whose name ends with the
/// given suffix. DNN prefixes controls with volatile ids like `dnn$ctr1216$...`,
/// so we match on the stable trailing part (e.g. `txtUsername`).
pub fn find_input_name_ending_with(html: &str, suffix: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("input[name], select[name], textarea[name]").ok()?;
    for el in doc.select(&sel) {
        if let Some(name) = el.value().attr("name") {
            if name.ends_with(suffix) {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Find the fully-qualified name of an element whose `id` ends with `suffix`.
pub fn find_name_by_id_ending_with(html: &str, suffix: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("input[id], select[id], textarea[id], a[id]").ok()?;
    for el in doc.select(&sel) {
        if let Some(id) = el.value().attr("id") {
            if id.ends_with(suffix) {
                if let Some(name) = el.value().attr("name") {
                    return Some(name.to_string());
                }
                return Some(id.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        <html><body>
        <form action="/Login.aspx" method="post">
          <input type="hidden" name="__VIEWSTATE" value="abc123" />
          <input type="hidden" name="__EVENTVALIDATION" value="ev456" />
          <input type="text" name="dnn$ctr1216$Login$Login_DNN$txtUsername" value="" />
          <input type="password" name="dnn$ctr1216$Login$Login_DNN$txtPassword" value="" />
          <input type="checkbox" name="rememberme" />
          <input type="submit" name="btnSubmit" value="Login" />
          <select name="state"><option value="FL" selected>FL</option><option value="GA">GA</option></select>
        </form>
        </body></html>
    "#;

    #[test]
    fn extracts_hidden_and_text_fields() {
        let fs = FormState::from_html(SAMPLE);
        assert_eq!(fs.get("__VIEWSTATE"), Some("abc123"));
        assert_eq!(fs.get("__EVENTVALIDATION"), Some("ev456"));
        assert!(fs
            .fields
            .contains_key("dnn$ctr1216$Login$Login_DNN$txtUsername"));
        // Submit buttons and unchecked checkboxes are not auto-posted.
        assert!(!fs.fields.contains_key("btnSubmit"));
        assert!(!fs.fields.contains_key("rememberme"));
        assert_eq!(fs.action.as_deref(), Some("/Login.aspx"));
    }

    #[test]
    fn selects_default_option() {
        let fs = FormState::from_html(SAMPLE);
        assert_eq!(fs.get("state"), Some("FL"));
    }

    #[test]
    fn finds_input_by_suffix() {
        let name = find_input_name_ending_with(SAMPLE, "txtUsername").unwrap();
        assert_eq!(name, "dnn$ctr1216$Login$Login_DNN$txtUsername");
    }
}
