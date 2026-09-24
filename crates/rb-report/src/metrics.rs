//! `metrics`: the stability metrics of folders and modules as a text table (N, Ca, Ce, I, and the
//! experimental size and top-level statement count when present). dependency-cruiser 18.2.0's
//! `src/report/metrics.mjs`, ported.
//!
//! - Specification: `test/report/metrics/metrics.spec.mjs`, run unmodified by conformance gate 1
//!   layer 3 ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `metrics`; [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `reporterOptions.metrics`: `hideFolders`, `hideModules`, `orderBy`
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)
//!
//! A result without `folders` was cruised without metrics, and the reporter says so with exit 1,
//! as upstream's does. Rows sort by name, then by `orderBy` (default `instability`) descending,
//! with a stable sort, so equal figures stay in name order.

use serde_json::{Map, Value, json};

use crate::style::{Style, percentage, styled};
use crate::{Rendered, js};

const METRIC_WIDTH: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Integer,
    Percent,
}

struct Column {
    key: &'static str,
    title: &'static str,
    width: usize,
    format: Format,
}

fn columns(name_width: usize) -> Vec<Column> {
    let column = |key, title, width, format| Column {
        key,
        title,
        width,
        format,
    };
    vec![
        column("type", "type", 6, Format::Text),
        column("name", "name", name_width, Format::Text),
        column("moduleCount", "N", METRIC_WIDTH, Format::Integer),
        column("afferentCouplings", "Ca", METRIC_WIDTH, Format::Integer),
        column("efferentCouplings", "Ce", METRIC_WIDTH, Format::Integer),
        column("instability", "I (%)", METRIC_WIDTH, Format::Percent),
        column("size", "size", METRIC_WIDTH + METRIC_WIDTH, Format::Integer),
        column(
            "topLevelStatementCount",
            "#tls",
            METRIC_WIDTH,
            Format::Integer,
        ),
    ]
}

/// A row as upstream builds it: every key present, `undefined` ones as `None`.
type Row = Vec<(&'static str, Option<Value>)>;

fn get(row: &Row, key: &str) -> Option<Value> {
    row.iter()
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| v.clone())
}

fn with_stats(mut row: Row, item: &Value) -> Row {
    if let Some(stats) = item
        .get("experimentalStats")
        .filter(|s| js::truthy(Some(s)))
    {
        row.push(("size", stats.get("size").cloned()));
        row.push((
            "topLevelStatementCount",
            stats.get("topLevelStatementCount").cloned(),
        ));
    }
    row
}

/// `getMetricsFromFolder`.
fn folder_row(folder: &Value) -> Row {
    let row = vec![
        ("type", Some(json!("folder"))),
        ("name", folder.get("name").cloned()),
        ("moduleCount", folder.get("moduleCount").cloned()),
        (
            "afferentCouplings",
            folder.get("afferentCouplings").cloned(),
        ),
        (
            "efferentCouplings",
            folder.get("efferentCouplings").cloned(),
        ),
        ("instability", folder.get("instability").cloned()),
    ];
    with_stats(row, folder)
}

/// `getMetricsFromModule`.
fn module_row(module: &Value) -> Row {
    let count = |key: &str| Some(json!(rb_rules::js::array(module, key).len()));
    let row = vec![
        ("type", Some(json!("module"))),
        ("name", module.get("source").cloned()),
        ("moduleCount", Some(json!(1))),
        ("afferentCouplings", count("dependents")),
        ("efferentCouplings", count("dependencies")),
        ("instability", module.get("instability").cloned()),
    ];
    with_stats(row, module)
}

/// `componentIsCalculable`: an integer module count above -1.
fn calculable(row: &Row) -> bool {
    let count = get(row, "moduleCount");
    js::is_integer(count.as_ref()) && js::to_number(count.as_ref()) > -1.0
}

/// `new Intl.NumberFormat(undefined).format(integer)` in the `en-US` locale.
fn format_integer(value: f64) -> String {
    let negative = value < 0.0;
    let digits = js::number_to_string(value.abs());
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    if negative {
        format!("-{grouped}")
    } else {
        grouped
    }
}

fn cell(column: &Column, value: Option<&Value>) -> String {
    let width = column.width + 1;
    match column.format {
        Format::Text => js::pad_end(&js::to_string(value), width),
        Format::Percent => js::pad_start(&percentage(js::to_number(value)), width),
        Format::Integer if js::is_integer(value) => {
            js::pad_start(&format_integer(js::to_number(value)), width)
        }
        Format::Integer => js::pad_start("", width),
    }
}

/// `(pRight[attribute] || 0) - (pLeft[attribute] || 0)`.
fn by_number(attribute: &str, left: &Row, right: &Row) -> f64 {
    let figure = |row: &Row| {
        let value = get(row, attribute);
        if js::truthy(value.as_ref()) {
            js::to_number(value.as_ref())
        } else {
            0.0
        }
    };
    figure(right) - figure(left)
}

fn table(result: &Value, options: &Map<String, Value>, color: bool) -> String {
    let flag = |key: &str| js::truthy(options.get(key));
    let folders = rb_rules::js::array(result, "folders");
    let modules = rb_rules::js::array(result, "modules");
    let rows: Vec<Row> = folders
        .iter()
        .map(folder_row)
        .chain(
            modules
                .iter()
                .filter(|m| m.as_object().is_some_and(|o| o.contains_key("instability")))
                .map(module_row),
        )
        .filter(calculable)
        .collect();
    let name_width = rows
        .iter()
        .map(|r| js::length(&js::to_string(get(r, "name").as_ref())))
        .chain(std::iter::once(js::length("name")))
        .max()
        .unwrap_or(4);
    let columns = columns(name_width);
    let (hide_modules, hide_folders) = (flag("hideModules"), flag("hideFolders"));
    let mut shown: Vec<Row> = rows
        .into_iter()
        .filter(|r| {
            let kind = get(r, "type");
            let kind = kind.as_ref().and_then(Value::as_str);
            (!hide_modules && kind == Some("module")) || (!hide_folders && kind == Some("folder"))
        })
        .collect();
    rb_rules::js::sort(&mut shown, |a, b| {
        let name = |r: &Row| js::to_string(get(r, "name").as_ref());
        rb_model::collate::compare(&name(a), &name(b)) == std::cmp::Ordering::Less
    });
    let order_by = options
        .get("orderBy")
        .filter(|o| js::truthy(Some(o)))
        .map_or_else(|| "instability".to_owned(), |o| js::to_string(Some(o)));
    rb_rules::js::sort(&mut shown, |a, b| by_number(&order_by, a, b) < 0.0);
    let header = columns
        .iter()
        .map(|c| match c.format {
            Format::Text => js::pad_end(c.title, c.width + 1),
            _ => js::pad_start(c.title, c.width + 1),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let line = columns
        .iter()
        .map(|c| "-".repeat(c.width + 1))
        .collect::<Vec<_>>()
        .join(" ");
    let data = shown
        .iter()
        .map(|row| {
            row.iter()
                .filter_map(|(key, value)| {
                    columns
                        .iter()
                        .find(|c| c.key == *key)
                        .map(|c| cell(c, value.as_ref()))
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{}\n{line}\n{data}\n",
        styled(Some(Style::Bold), &header, color)
    )
}

/// Renders `metrics`, with `options` the `reporterOptions.metrics` section.
pub fn render(result: &Value, options: Option<&Value>, color: bool) -> Rendered {
    if !js::truthy(result.get("folders")) {
        return Rendered {
            output: "\nERROR: The cruise result didn't contain any metrics - re-running the cruise with\n       the '--metrics' command line option should fix that.\n\n".into(),
            exit_code: 1,
        };
    }
    let options = options
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Rendered {
        output: table(result, &options, color),
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modules() -> Value {
        json!([
            { "source": "src/noot.js", "dependencies": [], "dependents": ["two"], "instability": 0 },
            { "source": "src/aap.js", "dependencies": ["one", "two", "three"], "dependents": ["four"], "instability": 0.25,
              "experimentalStats": { "size": 1234, "topLevelStatementCount": null } },
            { "source": "src/mies.js", "dependencies": ["one"], "dependents": ["two"], "instability": 0.5 },
            { "source": "core", "dependencies": [] }
        ])
    }

    #[test]
    fn no_folders_is_an_error() {
        let r = render(&json!({ "modules": [] }), None, false);
        assert_eq!(r.exit_code, 1);
        assert!(r.output.contains("ERROR"));
    }

    #[test]
    fn the_table() {
        let result = json!({ "modules": modules(), "folders": [
            { "name": "src", "moduleCount": 3, "afferentCouplings": 1, "efferentCouplings": 1, "instability": 0.5 },
            { "name": "node_modules", "afferentCouplings": 1 }
        ] });
        let r = render(&result, None, false);
        assert_eq!(r.exit_code, 0);
        assert_eq!(
            r.output,
            concat!(
                "type    name              N     Ca     Ce  I (%)        size   #tls\n",
                "------- ------------ ------ ------ ------ ------ ----------- ------\n",
                "folder  src               3      1      1    50%\n",
                "module  src/mies.js       1      1      1    50%\n",
                "module  src/aap.js        1      1      3    25%       1,234       \n",
                "module  src/noot.js       1      1      0     0%\n"
            )
        );
        let by_name = render(
            &result,
            Some(&json!({ "orderBy": "name", "hideFolders": true })),
            false,
        );
        assert!(by_name.output.contains("\nmodule  src/aap.js        1      1      3    25%       1,234       \nmodule  src/mies.js"));
        assert!(!by_name.output.contains("folder"));
        let hidden = render(&result, Some(&json!({ "hideModules": true })), true);
        assert!(hidden.output.starts_with("\u{1b}[1mtype"));
        assert!(!hidden.output.contains("module  "));
        let nothing = render(&json!({ "modules": [], "folders": [] }), None, false);
        assert_eq!(nothing.output.lines().count(), 3);
        assert!(nothing.output.ends_with("------\n\n"));
    }

    #[test]
    fn integers_group_like_intl() {
        assert_eq!(format_integer(0.0), "0");
        assert_eq!(format_integer(1234.0), "1,234");
        assert_eq!(format_integer(-1_234_567.0), "-1,234,567");
        let folder = json!({ "name": "x", "moduleCount": 1, "instability": "junk" });
        let columns = columns(4);
        assert_eq!(cell(&columns[5], folder.get("instability")), "  NaN%");
        assert_eq!(cell(&columns[2], Some(&json!(1.5))), "      ");
        assert!(!calculable(&folder_row(&json!({ "moduleCount": -1 }))));
        assert!(calculable(&folder_row(&json!({ "moduleCount": 0 }))));
    }
}
