//! Output rendering — human tables (comfy-table) or JSON.

use comfy_table::{presets::UTF8_FULL, Cell, ContentArrangement, Table};
use serde::Serialize;

/// Whether to render machine-readable JSON or human tables.
#[derive(Clone, Copy)]
pub struct Format {
    pub json: bool,
}

impl Format {
    pub fn new(json: bool) -> Self {
        Format { json }
    }

    /// Print any serializable value as pretty JSON.
    pub fn print_json<T: Serialize>(&self, value: &T) -> anyhow::Result<()> {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    }

    /// Print a titled key/value block (for single-record views).
    pub fn print_kv(&self, title: &str, pairs: &[(&str, String)]) {
        let mut table = base_table();
        table.set_header(vec![Cell::new(title), Cell::new("")]);
        for (k, v) in pairs {
            table.add_row(vec![Cell::new(k), Cell::new(v)]);
        }
        println!("{table}");
    }

    /// Print a row-oriented table.
    pub fn print_table(&self, headers: &[&str], rows: &[Vec<String>]) {
        if rows.is_empty() {
            println!("(no rows)");
            return;
        }
        let mut table = base_table();
        table.set_header(headers.iter().map(Cell::new).collect::<Vec<_>>());
        for row in rows {
            table.add_row(row.iter().map(Cell::new).collect::<Vec<_>>());
        }
        println!("{table}");
    }
}

fn base_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table
}

/// Helper to render an `Option<T: Display>` as a string, using "—" for None.
pub fn opt<T: std::fmt::Display>(v: &Option<T>) -> String {
    match v {
        Some(x) => x.to_string(),
        None => "—".to_string(),
    }
}
