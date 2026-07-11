//! Usage / consumption history flow.
//!
//! `UsageHistory.aspx` is form-first: you choose a service type and submit an
//! ImageButton before the consumption grid renders. This drives that postback,
//! then hands the resulting page to the scraper.

use crate::client::Client;
use crate::dnn::{find_input_name_ending_with, FormState};
use crate::error::Result;
use crate::model::UsageRecord;
use crate::pages;
use crate::scrape;

/// Fetch usage history, submitting the service-selection form if needed.
pub fn fetch(client: &Client) -> Result<Vec<UsageRecord>> {
    let page = client.get_text(pages::USAGE_HISTORY)?;

    // Some landings already show the grid; use it directly if so.
    let direct = scrape::parse_usage(&page);
    if !direct.is_empty() {
        return Ok(direct);
    }

    // Otherwise submit. FormState already carries the ViewState and the service
    // dropdown's default selection; we just add the ImageButton "click".
    let mut form = FormState::from_html(&page);
    let btn = match find_input_name_ending_with(&page, "ImageButton1") {
        Some(b) => b,
        // No submit control on this deployment — nothing more we can do.
        None => return Ok(Vec::new()),
    };
    // ASP.NET ImageButtons post their click as `<name>.x` / `<name>.y`.
    form.set(format!("{btn}.x"), "3");
    form.set(format!("{btn}.y"), "3");

    let action = form
        .action
        .clone()
        .unwrap_or_else(|| pages::USAGE_HISTORY.to_string());
    let body = client.post_form_text(&action, &form.to_pairs())?;
    Ok(scrape::parse_usage(&body))
}
