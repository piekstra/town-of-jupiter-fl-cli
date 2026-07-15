//! Output rendering — human tables (comfy-table), JSON, or CSV.

use comfy_table::{presets::UTF8_FULL, Cell, ContentArrangement, Table};
use serde::Serialize;

/// How to render output: human tables, machine-readable JSON, or CSV.
///
/// `json` and `csv` are mutually exclusive (enforced at the flag layer); when
/// neither is set, output is human-formatted tables. Command handlers only ever
/// branch on `json` — the `else` arm calls [`Format::print_table`] /
/// [`Format::print_kv`], which emit CSV instead when `csv` is set. That keeps
/// CSV support entirely inside this module.
#[derive(Clone, Copy)]
pub struct Format {
    pub json: bool,
    pub csv: bool,
}

impl Format {
    pub fn new(json: bool, csv: bool) -> Self {
        Format { json, csv }
    }

    /// Print any serializable value as pretty JSON.
    pub fn print_json<T: Serialize>(&self, value: &T) -> anyhow::Result<()> {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    }

    /// Print a titled key/value block (for single-record views).
    ///
    /// In CSV mode this becomes a two-column `field,value` sheet (the title is
    /// dropped, since CSV has no place for it).
    pub fn print_kv(&self, title: &str, pairs: &[(&str, String)]) {
        if self.csv {
            let rows: Vec<Vec<String>> = pairs
                .iter()
                .map(|(k, v)| vec![k.to_string(), v.clone()])
                .collect();
            print!("{}", csv_document(&["field", "value"], &rows));
            return;
        }
        let mut table = base_table();
        table.set_header(vec![Cell::new(title), Cell::new("")]);
        for (k, v) in pairs {
            table.add_row(vec![Cell::new(k), Cell::new(v)]);
        }
        println!("{table}");
    }

    /// Print a row-oriented table.
    pub fn print_table(&self, headers: &[&str], rows: &[Vec<String>]) {
        if self.csv {
            // Emit CSV even for zero rows — a bare header line is still valid
            // and lets scripts distinguish "no data" from a broken column set.
            print!("{}", csv_document(headers, rows));
            return;
        }
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

/// Render a header row plus data rows as an RFC 4180-style CSV document
/// (LF line endings, for pipe/spreadsheet friendliness).
fn csv_document(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    out.push_str(&csv_record(headers.iter().map(|h| h.to_string())));
    for row in rows {
        out.push_str(&csv_record(row.iter().cloned()));
    }
    out
}

fn csv_record(fields: impl IntoIterator<Item = String>) -> String {
    let line = fields
        .into_iter()
        .map(|f| csv_escape(&f))
        .collect::<Vec<_>>()
        .join(",");
    format!("{line}\n")
}

/// Escape one CSV field per RFC 4180. The human tables use an em-dash ("—") for
/// missing values; for CSV an empty cell is the conventional representation, so
/// we normalize it away.
fn csv_escape(field: &str) -> String {
    if field == "—" {
        return String::new();
    }
    if field.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_escapes_special_fields() {
        // Plain fields pass through; the em-dash placeholder becomes empty.
        assert_eq!(csv_escape("Water"), "Water");
        assert_eq!(csv_escape("—"), "");
        // Commas, quotes, and newlines force quoting; inner quotes double.
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape(r#"say "hi""#), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("line1\nline2"), "\"line1\nline2\"");
    }

    #[test]
    fn csv_document_has_header_and_rows() {
        // The dates carry a comma, so they must be quoted; the em-dash becomes
        // an empty trailing cell.
        let doc = csv_document(
            &["Date", "Usage"],
            &[
                vec!["Jun 12, 2026".into(), "9.90".into()],
                vec!["May 13, 2026".into(), "—".into()],
            ],
        );
        assert_eq!(
            doc,
            "Date,Usage\n\"Jun 12, 2026\",9.90\n\"May 13, 2026\",\n"
        );
    }

    #[test]
    fn csv_document_emits_bare_header_for_no_rows() {
        assert_eq!(csv_document(&["A", "B"], &[]), "A,B\n");
    }
}
