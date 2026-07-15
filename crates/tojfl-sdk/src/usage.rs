//! Usage / consumption history flow.
//!
//! `UsageHistory.aspx` is form-first: you choose a service type and submit an
//! ImageButton before the consumption grid renders. This drives that postback,
//! then hands the resulting page to the scraper.

use crate::client::Client;
use crate::dnn::{find_input_name_ending_with, first_real_option, FormState};
use crate::error::Result;
use crate::model::{MeterRead, UsageComparison, UsageRecord};
use crate::pages;
use crate::scrape;

/// Which group average to compare your consumption against.
#[derive(Debug, Clone, Copy)]
pub enum CompareTarget {
    Street,
    Region,
    City,
}

impl CompareTarget {
    /// The `ctlCompare` option value the portal expects.
    fn portal_value(self) -> &'static str {
        match self {
            CompareTarget::Street => "Street",
            CompareTarget::Region => "Region",
            CompareTarget::City => "CITY",
        }
    }
}

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

/// Fetch meter-reading history: select the real service (skipping the
/// "Select Service Type" placeholder) and submit the ImageButton, then scrape.
pub fn fetch_meter_reads(client: &Client) -> Result<Vec<MeterRead>> {
    let page = client.get_text(pages::METER_READING_HISTORY)?;
    let mut form = FormState::from_html(&page);

    // The service dropdown defaults to a placeholder that submits to an empty
    // grid — pick the first real option instead.
    if let Some(svc) = find_input_name_ending_with(&page, "ctlServices") {
        if let Some(val) = first_real_option(&page, "ctlServices") {
            form.set(&svc, val);
        }
    }
    let btn = match find_input_name_ending_with(&page, "ImageButton1") {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    form.set(format!("{btn}.x"), "3");
    form.set(format!("{btn}.y"), "3");

    let action = form
        .action
        .clone()
        .unwrap_or_else(|| pages::METER_READING_HISTORY.to_string());
    let body = client.post_form_text(&action, &form.to_pairs())?;
    Ok(scrape::parse_meter_reads(&body))
}

/// Compare your consumption to a group average (street/region/city). Submits the
/// comparison form (`ctlServices2` + `ctlCompare` + the `btnCompare` ImageButton)
/// and scrapes the resulting grid.
pub fn compare(client: &Client, target: CompareTarget) -> Result<Vec<UsageComparison>> {
    let page = client.get_text(pages::USAGE_HISTORY)?;
    let mut form = FormState::from_html(&page);

    // Service defaults to the sole option (Water); set the comparison group.
    if let Some(cmp) = find_input_name_ending_with(&page, "ctlCompare") {
        form.set(&cmp, target.portal_value());
    }
    let btn = match find_input_name_ending_with(&page, "btnCompare") {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    form.set(format!("{btn}.x"), "3");
    form.set(format!("{btn}.y"), "3");

    let action = form
        .action
        .clone()
        .unwrap_or_else(|| pages::USAGE_HISTORY.to_string());
    let body = client.post_form_text(&action, &form.to_pairs())?;
    Ok(scrape::parse_comparison(&body))
}
