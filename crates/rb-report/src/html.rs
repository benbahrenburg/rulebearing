//! `html`: the dependency matrix as one HTML page, a row and a column per module and a cell per
//! pair, coloured by whether the row's module depends on the column's and by the severity of the
//! first rule the edge breaks. dependency-cruiser 18.2.0's `src/report/html/index.mjs` and
//! `src/report/utl/dependency-to-incidence-transformer.mjs`, ported; the template is upstream's,
//! verbatim, in `html.html`.
//!
//! - Specification: `test/report/html/html.spec.mjs` (a byte comparison against upstream's
//!   fixture), run unmodified by conformance gate 1 layer 3, and upstream's reporter over every
//!   `test/report` mock ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `html`
//! - Plan: [Wave 3, Step 6](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)
//!
//! The modules are ordered as upstream orders them, non-core before core and then by `source` in
//! UTF-16 code unit order, with upstream's comparator (`a > b ? 1 : -1`) run through V8's own sort
//! ([`crate::js_sort`]): the comparator never answers "equal", so where two modules with the same
//! `source` end up (upstream's mocks have them) depends on the algorithm, and it is V8's.

use std::fmt::Write as _;

use serde_json::Value;

use crate::{Rendered, js, js_sort};

/// The page, verbatim from dependency-cruiser 18.2.0.
const TEMPLATE: &str = include_str!("html.html");

/// One cell of the matrix: whether the row's module depends on the column's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incidence {
    /// The column's module.
    pub to: String,
    /// `true`, `false`, or the severity of the first rule the dependency breaks.
    pub incidence: String,
    /// The first rule the dependency breaks, with how many others it breaks.
    pub rule: Option<String>,
}

impl Incidence {
    /// `hasRelation`: whether there is a dependency at all.
    pub fn has_relation(&self) -> bool {
        self.incidence != "false"
    }
}

/// A row of the matrix: the module and its cells, one per module.
#[derive(Debug, Clone)]
pub struct Row<'a> {
    /// The module.
    pub module: &'a Value,
    /// One cell per module, in the matrix's order.
    pub incidences: Vec<Incidence>,
}

fn sort_key(module: &Value) -> String {
    let core = if js::truthy(module.get("coreModule")) {
        "1"
    } else {
        "0"
    };
    format!("{core}-{}", js::field(module, "source"))
}

/// `determineIncidenceType(column)(row)`.
fn incidence(column: &Value, row: &Value) -> Incidence {
    let to = js::field(column, "source");
    let dependency = rb_rules::js::array(row, "dependencies")
        .iter()
        .find(|d| d.get("resolved") == column.get("source"));
    let Some(dependency) = dependency else {
        return Incidence {
            to,
            incidence: "false".into(),
            rule: None,
        };
    };
    if js::truthy(dependency.get("valid")) {
        return Incidence {
            to,
            incidence: "true".into(),
            rule: None,
        };
    }
    let rules = rb_rules::js::array(dependency, "rules");
    let first = rules.first().unwrap_or(&Value::Null);
    let others = if rules.len() > 1 {
        format!(" (+{} others)", rules.len() - 1)
    } else {
        String::new()
    };
    Incidence {
        to,
        incidence: js::to_string(first.get("severity")),
        rule: Some(format!("{}{others}", js::to_string(first.get("name")))),
    }
}

/// `transformDependenciesToIncidences(modules)`: the modules in the matrix's order, each with one
/// cell per module.
pub fn incidences(modules: &[Value]) -> Vec<Row<'_>> {
    let mut keyed: Vec<(String, &Value)> = modules.iter().map(|m| (sort_key(m), m)).collect();
    // `compareOnSource`: `key(a) > key(b) ? 1 : -1`, so `a` sorts first unless its key is greater.
    js_sort::sort(&mut keyed, |a, b| {
        js::compare_utf16(&a.0, &b.0) != std::cmp::Ordering::Greater
    });
    let sorted: Vec<&Value> = keyed.into_iter().map(|(_, m)| m).collect();
    sorted
        .iter()
        .map(|row| Row {
            module: row,
            incidences: sorted.iter().map(|column| incidence(column, row)).collect(),
        })
        .collect()
}

fn classes(module: &Value, others: &[&str]) -> String {
    let mut classes: Vec<&str> = others.to_vec();
    if js::truthy(module.get("coreModule")) {
        classes.push("cell-core-module");
    }
    if js::truthy(module.get("couldNotResolve")) {
        classes.push("cell-unresolvable-module");
    }
    if classes.is_empty() {
        String::new()
    } else {
        format!(" class=\"{}\"", classes.join(" "))
    }
}

fn table_head(rows: &[Row<'_>]) -> String {
    rows.iter().fold(String::new(), |mut head, r| {
        let _ = write!(
            head,
            "<th><div{}>{}</div></th>",
            classes(r.module, &[]),
            js::field(r.module, "source")
        );
        head
    })
}

fn cell_title(module: &Value, incidence: &Incidence) -> String {
    let mut lines = Vec::new();
    if let Some(rule) = incidence.rule.as_deref().filter(|r| !r.is_empty()) {
        lines.push(format!("{rule}:"));
    }
    if incidence.has_relation() {
        lines.push(format!(
            "{} -> {}",
            js::field(module, "source"),
            incidence.to
        ));
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!(" title=\"{}\"", lines.join("\n"))
    }
}

fn table_row(row: &Row<'_>) -> String {
    let first = classes(row.module, &["first-cell"]);
    let source = js::field(row.module, "source");
    let cells = row.incidences.iter().fold(String::new(), |mut cells, i| {
        let _ = write!(
            cells,
            "<td class=\"cell cell-{}\"{}></td>",
            i.incidence,
            cell_title(row.module, i)
        );
        cells
    });
    format!(
        "\n      <tr>\n        <td{first}>{source}</td>\n        {cells}\n        <td{first}>{source}</td>\n      </tr>"
    )
}

fn table(rows: &[Row<'_>]) -> String {
    let head = table_head(rows);
    let body: String = rows.iter().map(table_row).collect();
    format!(
        r##"
  <table id="table-rotated">
    <thead>
      <tr>
        <td class="controls top-left">
          <a id="rotate" href="#table-rotated">rotate</a>
          <a id="unrotate" href="#">rotate back</a>
        </td>
        {head}
        <td class="top-right"></td>
      </tr>
    </thead>
    <tbody>
    {body}
    </tbody>
    <tfoot>
      <tr>
        <td class="bottom-left"></td>
        {head}
        <td class="bottom-right"></td>
      </tr>
    </tfoot>
  </table>
"##
    )
}

/// Renders `html`. Always exits 0, as upstream's reporter does.
pub fn render(result: &Value) -> Rendered {
    let rows = incidences(rb_rules::js::array(result, "modules"));
    Rendered {
        output: js::replace_first(TEMPLATE, "{{table-here}}", &table(&rows)),
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn modules() -> Value {
        json!({ "modules": [
            { "source": "b.js", "dependencies": [
                { "resolved": "a.js", "valid": false, "rules": [{ "name": "r1", "severity": "error" }, { "name": "r2", "severity": "warn" }] },
                { "resolved": "fs", "valid": true } ] },
            { "source": "fs", "coreModule": true, "dependencies": [] },
            { "source": "a.js", "couldNotResolve": true, "dependencies": [
                { "resolved": "b.js", "valid": false, "rules": [{ "name": "r3", "severity": "info" }] } ] },
        ] })
    }

    #[test]
    fn the_matrix_orders_and_classifies() {
        let result = modules();
        let rows = incidences(rb_rules::js::array(&result, "modules"));
        let order: Vec<String> = rows.iter().map(|r| js::field(r.module, "source")).collect();
        assert_eq!(order, ["a.js", "b.js", "fs"]);
        let b = &rows[1].incidences;
        assert_eq!(
            b[0],
            Incidence {
                to: "a.js".into(),
                incidence: "error".into(),
                rule: Some("r1 (+1 others)".into())
            }
        );
        assert_eq!(b[1].incidence, "false");
        assert!(!b[1].has_relation());
        assert_eq!(b[2].incidence, "true");
        assert_eq!(rows[0].incidences[1].rule.as_deref(), Some("r3"));
    }

    #[test]
    fn the_page_byte_for_byte() {
        let rendered = render(&modules());
        assert_eq!(rendered.exit_code, 0);
        let out = &rendered.output;
        assert!(out.starts_with("<!DOCTYPE html>\n<html>\n"));
        assert!(out.ends_with("  </table>\n\n</body>\n</html>\n"));
        assert!(!out.contains("{{table-here}}"));
        assert!(out.contains(
            "<th><div class=\"cell-unresolvable-module\">a.js</div></th><th><div>b.js</div></th><th><div class=\"cell-core-module\">fs</div></th>"
        ));
        assert!(out.contains(
            "\n      <tr>\n        <td class=\"first-cell\">b.js</td>\n        <td class=\"cell cell-error\" title=\"r1 (+1 others):\nb.js -> a.js\"></td><td class=\"cell cell-false\"></td><td class=\"cell cell-true\" title=\"b.js -> fs\"></td>\n        <td class=\"first-cell\">b.js</td>\n      </tr>"
        ));
        assert!(out.contains("<td class=\"first-cell cell-core-module\">fs</td>"));
        assert_eq!(render(&modules()), rendered, "deterministic");
    }

    #[test]
    fn replacement_patterns_in_a_name_expand_as_upstream() {
        // `String.prototype.replace` expands `$&` in the table as it would upstream.
        let result = json!({ "modules": [{ "source": "$&", "dependencies": [] }] });
        assert!(
            render(&result)
                .output
                .contains("<th><div>{{table-here}}</div></th>")
        );
        let empty = render(&json!({}));
        assert!(empty.output.contains("<tbody>\n    \n    </tbody>"));
        let odd = json!({ "modules": [
            { "source": "x", "dependencies": [{ "resolved": "x", "valid": false }] }] });
        assert!(
            render(&odd)
                .output
                .contains("cell cell-undefined\" title=\"undefined:\nx -> x\"")
        );
    }

    #[test]
    fn repeated_sources_land_where_v8_puts_them() {
        // node: [{s:"b",i:0},{s:"a",i:1},{s:"b",i:2},{s:"a",i:3},{s:"b",i:4}]
        //   .sort((x,y)=>x.s>y.s?1:-1).map(x=>x.i) is [3, 1, 4, 2, 0]
        let result = json!({ "modules": [
            { "source": "b", "i": 0 }, { "source": "a", "i": 1 }, { "source": "b", "i": 2 },
            { "source": "a", "i": 3 }, { "source": "b", "i": 4 } ] });
        let rows = incidences(rb_rules::js::array(&result, "modules"));
        let ids: Vec<String> = rows.iter().map(|r| js::field(r.module, "i")).collect();
        assert_eq!(ids, ["3", "1", "4", "2", "0"]);
    }
}
