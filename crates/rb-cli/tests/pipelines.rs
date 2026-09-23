//! The TypeScript reference pipeline from the design, run line for line over a fixture monorepo,
//! and the determinism promise: two runs on the same inputs serialise byte for byte.
//!
//! - Source: [design § Three pipelines](../../../docs/artifacts/design.md#three-pipelines)
//!   (the monorepo already on dependency-cruiser: `cruise`, `fmt --exit-code`, `count`)
//! - Plan: [Wave 1, Step 13](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d)
//!   (the reference pipeline lines as an integration test; determinism)
//! - Contract: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md) (exit codes)
//! - Requirements: [FR-CORE-06](../../../docs/prd.md#fr-core-06), [FR-RULE-06](../../../docs/prd.md#fr-rule-06)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// A dependency-cruiser configuration, unchanged from what such a monorepo already has.
const CONFIG: &str = r#"module.exports = {
  forbidden: [
    {
      name: "no-app-to-app",
      comment: "An app never imports another app. plan:wave-1",
      severity: "error",
      from: { path: "^apps/([^/]+)/" },
      to: { path: "^apps/([^/]+)/", pathNot: "^apps/$1/" },
    },
    {
      name: "no-circular",
      severity: "warn",
      from: {},
      to: { circular: true },
    },
  ],
  options: {
    doNotFollow: { path: "node_modules" },
    tsPreCompilationDeps: true,
  },
};
"#;

const FILES: &[(&str, &str)] = &[
    (".dependency-cruiser.cjs", CONFIG),
    (
        "apps/web/src/app/home/page.tsx",
        "import { query } from \"../../server/db\";\nimport { format } from \"../../../../../packages/ui/src/format\";\nexport default function Page() { return format(query()); }\n",
    ),
    (
        "apps/web/src/app/api/route.ts",
        "import { query } from \"../../server/db\";\nimport { rule } from \"../../domain/rule\";\nexport const GET = () => rule(query());\n",
    ),
    (
        "apps/web/src/server/db.ts",
        "export const query = () => 1;\n",
    ),
    (
        "apps/web/src/domain/rule.ts",
        "export const rule = (n: number) => n + 1;\n",
    ),
    (
        "apps/admin/src/app/page.tsx",
        "import { query } from \"../../../web/src/server/db\";\nexport default function Admin() { return query(); }\n",
    ),
    (
        "packages/ui/src/format.ts",
        "import { helper } from \"./helper\";\nexport const format = (n: number) => helper(n);\n",
    ),
    (
        "packages/ui/src/helper.ts",
        "import { format } from \"./format\";\nexport const helper = (n: number) => String(n) + typeof format;\n",
    ),
];

fn monorepo(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-pipeline-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (file, text) in FILES {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    std::fs::create_dir_all(dir.join(".graph"))?;
    std::fs::create_dir_all(dir.join("eng"))?;
    Ok(dir)
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(BIN).args(args).current_dir(dir).output()?)
}

/// A run with the clock pinned, as a reproducible build pins it: `teamcity` stamps each message.
fn pinned(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .output()?)
}

#[test]
fn the_typescript_pipeline_runs_as_written() -> Result<(), Box<dyn Error>> {
    let dir = monorepo("ts")?;
    // - run: rulebearing cruise --config .dependency-cruiser.cjs --metrics --output-type json apps packages > .graph/cruise.json
    let cruise = run(
        &dir,
        &[
            "cruise",
            "--config",
            ".dependency-cruiser.cjs",
            "--metrics",
            "--output-type",
            "json",
            "apps",
            "packages",
        ],
    )?;
    assert_eq!(
        cruise.status.code(),
        Some(0),
        "the JSON step succeeds so the gate step runs (ADR-0030)"
    );
    std::fs::write(dir.join(".graph/cruise.json"), &cruise.stdout)?;
    let result: serde_json::Value = serde_json::from_slice(&cruise.stdout)?;
    assert_eq!(result["summary"]["totalCruised"], 7);
    assert_eq!(result["summary"]["error"], 1);
    assert_eq!(result["summary"]["warn"], 1, "the ui cycle, reported once");
    assert!(result["folders"].is_array(), "--metrics adds folders");

    // - run: rulebearing fmt --exit-code --output-type err .graph/cruise.json
    let fmt = run(
        &dir,
        &[
            "fmt",
            "--exit-code",
            "--output-type",
            "err",
            ".graph/cruise.json",
        ],
    )?;
    assert_eq!(fmt.status.code(), Some(1));
    let text = String::from_utf8(fmt.stdout)?;
    assert!(
        text.contains(
            "error no-app-to-app: apps/admin/src/app/page.tsx → apps/web/src/server/db.ts"
        ),
        "{text}"
    );

    // - run: rulebearing count --from '^apps/([^/]+)/src/app/.*/(page|route)\.tsx?$' --to '^apps/$1/src/(server|domain)/' --budget eng/routes-via-service-budget.json
    let count = [
        "count",
        "--from",
        r"^apps/([^/]+)/src/app/.*/(page|route)\.tsx?$",
        "--to",
        "^apps/$1/src/(server|domain)/",
        "--budget",
        "eng/routes-via-service-budget.json",
    ];
    std::fs::write(
        dir.join("eng/routes-via-service-budget.json"),
        "{ \"ceiling\": 3 }\n",
    )?;
    let held = run(&dir, &count)?;
    assert_eq!(
        held.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&held.stderr)
    );
    assert!(
        String::from_utf8(held.stdout)?.starts_with("3 "),
        "three direct edges; admin's crosses apps and $1 excludes it"
    );
    std::fs::write(
        dir.join("eng/routes-via-service-budget.json"),
        "{ \"ceiling\": 2 }\n",
    )?;
    assert_eq!(
        run(&dir, &count)?.status.code(),
        Some(1),
        "a count above the ceiling fails"
    );
    let raise = [&count[..], &["--write"]].concat();
    assert_ne!(
        run(&dir, &raise)?.status.code(),
        Some(0),
        "--write refuses to raise the ceiling"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn two_runs_serialise_byte_for_byte() -> Result<(), Box<dyn Error>> {
    let first = monorepo("determinism-a")?;
    let second = monorepo("determinism-b")?;
    for output_type in ["json", "err-long", "text", "csv", "teamcity", "agent"] {
        let args = [
            "cruise",
            "--config",
            ".dependency-cruiser.cjs",
            "--metrics",
            "-T",
            output_type,
            "apps",
            "packages",
        ];
        let a = pinned(&first, &args)?;
        let b = pinned(&first, &args)?;
        let c = pinned(&second, &args)?;
        assert_eq!(
            a.stdout, b.stdout,
            "{output_type}: two runs in one tree differ"
        );
        // `optionsUsed.baseDir` is the working folder, an input like the files, as upstream writes it.
        // JSON escapes a Windows path's backslashes, so both spellings are replaced.
        let placeholder = |out: &Output, dir: &Path| {
            let raw = dir.to_string_lossy();
            String::from_utf8_lossy(&out.stdout)
                .replace(&raw.replace('\\', "\\\\"), "<dir>")
                .replace(&*raw, "<dir>")
        };
        assert_eq!(
            placeholder(&a, &first),
            placeholder(&c, &second),
            "{output_type}: two identical trees differ"
        );
        assert_eq!(a.status.code(), c.status.code());
    }
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
    Ok(())
}

#[test]
fn licences_are_read_only_when_a_rule_asks() -> Result<(), Box<dyn Error>> {
    let files: &[(&str, &str)] = &[
        (
            "package.json",
            "{ \"name\": \"x\", \"dependencies\": { \"gpl-thing\": \"1.0.0\" } }\n",
        ),
        (
            "node_modules/gpl-thing/package.json",
            "{ \"name\": \"gpl-thing\", \"version\": \"1.0.0\", \"license\": \"GPL-3.0\", \"main\": \"index.js\" }\n",
        ),
        ("node_modules/gpl-thing/index.js", "module.exports = 1;\n"),
        (
            "src/index.js",
            "const thing = require(\"gpl-thing\");\nmodule.exports = thing;\n",
        ),
    ];
    let dir = std::env::temp_dir().join(format!("rb-cli-licence-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (file, text) in files {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    let licence = |config: &str| -> Result<serde_json::Value, Box<dyn Error>> {
        std::fs::write(dir.join(".dependency-cruiser.json"), config)?;
        let out = run(&dir, &["cruise", "-T", "json", "src"])?;
        let value: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        Ok(value["modules"]
            .as_array()
            .and_then(|m| m.iter().find(|m| m["source"] == "src/index.js"))
            .map(|m| m["dependencies"][0]["license"].clone())
            .unwrap_or_default())
    };
    let with_rule = r#"{ "forbidden": [{ "name": "no-gpl", "severity": "error", "from": {}, "to": { "license": "GPL" } }] }"#;
    assert_eq!(licence(with_rule)?, "GPL-3.0");
    let without = r#"{ "forbidden": [{ "name": "no-circular", "severity": "warn", "from": {}, "to": { "circular": true } }], "options": { "doNotFollow": "node_modules" } }"#;
    assert!(
        licence(without)?.is_null(),
        "no licence rule, no licence read"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
