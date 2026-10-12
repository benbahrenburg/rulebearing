//! Usage counts end to end, through the binary: off by default, counted only with
//! `telemetry.enabled`, the CI guard, the salted hashes, the monthly cadence, `--usage-counts
//! print`, the published schema, and no network call anywhere in the binary.
//!
//! - Decision: [ADR-0076](../../../docs/adr/0076-usage-counts-are-counted-and-printed-but-not-sent.md)
//! - Plan: [plan 0004, Step 13](../../../docs/plans/pending/0004-wave-4-reach.md#25-steps-for-sub-wave-4e-usage-counts-and-the-adoption-review)
//!   (off by default, the CI guard, the salt test)
//! - Requirements: [FR-REACH-03](../../../docs/prd.md#fr-reach-03) ("No network call is made
//!   unless usage counts are opted in"), [NFR-SEC-01](../../../docs/prd.md#nfr-sec-01) ("The
//!   binary MUST make no outbound connection")
//! - Schema and retention: [docs/usage-counts.md](../../../docs/usage-counts.md)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// 2026-09-15, 2026-10-02 and 2026-10-20, noon UTC.
const SEPTEMBER: &str = "1789473600";
const OCTOBER: &str = "1790942400";
const LATE_OCTOBER: &str = "1792497600";

const SALT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const RULES: &str = "rules:
  dependencies:
    forbidden:
      - name: payments-not-to-ledger
        comment: adr:0001
        severity: error
        from: { path: \"^src/payments\" }
        to: { path: \"^src/ledger\" }
      - name: no-circular
        comment: adr:0001
        severity: warn
        from: {}
        to: { circular: true }
      - name: quiet-rule
        comment: adr:0001
        severity: error
        from: { path: \"^src/nowhere\" }
        to: {}
allowEmpty: [quiet-rule]
";

struct Repo(PathBuf);

impl Repo {
    fn new(name: &str, telemetry: &str) -> Result<Self, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("rb-usage-e2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (file, text) in [
            (
                "src/payments/pay.ts",
                "import { post } from \"../ledger/post\";\nexport const pay = post;\n",
            ),
            (
                "src/ledger/post.ts",
                "import { pay } from \"../payments/pay\";\nexport const post = () => pay;\n",
            ),
            ("rulebearing.yaml", &format!("{telemetry}{RULES}")),
        ] {
            let path = dir.join(file);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, text)?;
        }
        std::fs::create_dir_all(dir.join(".git"))?;
        Ok(Self(dir))
    }

    fn usage(&self) -> PathBuf {
        self.0.join(".graph/usage")
    }

    fn run(&self, args: &[&str], epoch: &str, ci: Option<&str>) -> Result<Output, Box<dyn Error>> {
        let mut command = Command::new(BIN);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command.env_remove("CI");
        if let Some(ci) = ci {
            command.env("CI", ci);
        }
        Ok(command
            .args(args)
            .current_dir(&self.0)
            .env("SOURCE_DATE_EPOCH", epoch)
            .output()?)
    }

    fn cruise(&self, epoch: &str, ci: Option<&str>) -> Result<Output, Box<dyn Error>> {
        self.run(&["cruise", "src"], epoch, ci)
    }

    fn print(&self, epoch: &str) -> Result<Output, Box<dyn Error>> {
        self.run(&["--usage-counts", "print"], epoch, None)
    }

    fn month(&self, month: &str) -> Result<String, Box<dyn Error>> {
        Ok(std::fs::read_to_string(
            self.usage().join(format!("{month}.json")),
        )?)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn schema() -> Result<jsonschema::Validator, Box<dyn Error>> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/usage-counts-v1.json"),
    )?;
    let schema: Value = serde_json::from_str(&text)?;
    Ok(jsonschema::validator_for(&schema)?)
}

fn valid(validator: &jsonschema::Validator, payload: &str) -> Result<Value, Box<dyn Error>> {
    let value: Value = serde_json::from_str(payload)?;
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|e| e.to_string())
        .collect();
    if !errors.is_empty() {
        return Err(format!("{payload} is not valid: {errors:?}").into());
    }
    Ok(value)
}

#[test]
fn off_by_default_no_counter_file_is_written() -> Result<(), Box<dyn Error>> {
    let repo = Repo::new("off", "")?;
    let cruise = repo.cruise(SEPTEMBER, None)?;
    assert_eq!(cruise.status.code(), Some(1), "{}", text(&cruise.stderr));
    assert!(
        !text(&cruise.stderr).contains("usage counts"),
        "{}",
        text(&cruise.stderr)
    );
    let rules = repo.run(&["rules"], SEPTEMBER, None)?;
    assert_eq!(rules.status.code(), Some(0), "{}", text(&rules.stderr));
    assert!(
        !repo.usage().exists(),
        "no counter file without the config key"
    );
    let explicit = Repo::new(
        "off-explicit",
        "telemetry: { enabled: false, ciAllowed: true }\n",
    )?;
    explicit.cruise(SEPTEMBER, Some("true"))?;
    assert!(!explicit.usage().exists());
    Ok(())
}

#[test]
fn a_counted_run_holds_counts_and_salted_hashes_only() -> Result<(), Box<dyn Error>> {
    let repo = Repo::new("counted", "telemetry: { enabled: true }\n")?;
    let first = repo.cruise(SEPTEMBER, None)?;
    assert_eq!(
        first.status.code(),
        Some(1),
        "the exit code is the cruise's"
    );
    let stderr = text(&first.stderr);
    assert!(
        stderr.contains("usage counts are on for this repository"),
        "{stderr}"
    );
    assert!(stderr.contains("nothing leaves this machine"), "{stderr}");
    let second = repo.cruise(SEPTEMBER, None)?;
    assert!(
        !text(&second.stderr).contains("usage counts are on"),
        "said once"
    );
    repo.run(&["rules"], SEPTEMBER, None)?;
    let on_disk = repo.month("2026-09")?;
    let payload = valid(&schema()?, &on_disk)?;
    assert_eq!(payload["tool"], "rulebearing");
    assert_eq!(payload["period"], "2026-09");
    assert_eq!(payload["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        payload["commands"],
        serde_json::json!({ "cruise": 2, "rules": 1 })
    );
    let rules = payload["rules"].as_object().cloned().unwrap_or_default();
    assert_eq!(
        rules.len(),
        2,
        "two rules fired, the quiet one did not: {rules:?}"
    );
    assert_eq!(
        rules.get("no-circular"),
        Some(&Value::from(2)),
        "a library rule by name"
    );
    let hashed: Vec<&String> = rules.keys().filter(|k| k.starts_with("sha256:")).collect();
    assert_eq!(hashed.len(), 1, "{rules:?}");
    // No path, no name of a rule outside the library, no folder, no user.
    for secret in [
        "payments",
        "ledger",
        "quiet-rule",
        "src/",
        ".ts",
        "rb-usage-e2e",
        "/",
    ] {
        assert!(!on_disk.contains(secret), "{secret} in {on_disk}");
    }
    let salt = std::fs::read_to_string(repo.usage().join("salt"))?;
    assert!(
        !on_disk.contains(salt.trim()),
        "the salt is never in the payload"
    );
    Ok(())
}

#[test]
fn the_payload_is_deterministic_for_the_same_salt_and_runs() -> Result<(), Box<dyn Error>> {
    let mut printed = Vec::new();
    for name in ["same-a", "same-b"] {
        let repo = Repo::new(name, "telemetry: { enabled: true }\n")?;
        std::fs::create_dir_all(repo.usage())?;
        std::fs::write(repo.usage().join("salt"), format!("{SALT}\n"))?;
        repo.cruise(SEPTEMBER, None)?;
        repo.cruise(SEPTEMBER, None)?;
        let once = repo.print(SEPTEMBER)?;
        let twice = repo.print(SEPTEMBER)?;
        assert_eq!(once.stdout, twice.stdout);
        assert_eq!(
            text(&once.stdout),
            repo.month("2026-09")?,
            "print shows the file"
        );
        printed.push(text(&once.stdout));
    }
    assert_eq!(printed[0], printed[1], "byte for byte across repositories");
    Ok(())
}

#[test]
fn two_repositories_hash_the_same_rule_differently() -> Result<(), Box<dyn Error>> {
    let mut keys = Vec::new();
    for name in ["salt-a", "salt-b"] {
        let repo = Repo::new(name, "telemetry: { enabled: true }\n")?;
        repo.cruise(SEPTEMBER, None)?;
        let payload: Value = serde_json::from_str(&repo.month("2026-09")?)?;
        let rules = payload["rules"].as_object().cloned().unwrap_or_default();
        assert!(rules.contains_key("no-circular"));
        let hashed: Vec<String> = rules
            .keys()
            .filter(|k| k.starts_with("sha256:"))
            .cloned()
            .collect();
        assert_eq!(hashed.len(), 1);
        keys.push(hashed.concat());
    }
    assert_ne!(keys[0], keys[1], "meaningless across repositories");
    Ok(())
}

#[test]
fn a_ci_run_counts_nothing_unless_ci_is_allowed() -> Result<(), Box<dyn Error>> {
    let repo = Repo::new("ci", "telemetry: { enabled: true }\n")?;
    for ci in ["true", "1", "yes"] {
        let cruise = repo.cruise(SEPTEMBER, Some(ci))?;
        assert_eq!(cruise.status.code(), Some(1));
        assert!(!text(&cruise.stderr).contains("usage counts"), "CI={ci}");
        assert!(!repo.usage().exists(), "CI={ci}: nothing written");
    }
    for not_ci in ["false", "0", ""] {
        repo.cruise(SEPTEMBER, Some(not_ci))?;
    }
    let payload: Value = serde_json::from_str(&repo.month("2026-09")?)?;
    assert_eq!(
        payload["commands"]["cruise"], 3,
        "CI=false, 0 and empty are not CI"
    );
    let allowed = Repo::new(
        "ci-allowed",
        "telemetry: { enabled: true, ciAllowed: true }\n",
    )?;
    allowed.cruise(SEPTEMBER, Some("true"))?;
    assert!(allowed.usage().join("2026-09.json").exists());
    Ok(())
}

#[test]
fn the_monthly_report_prints_the_closed_month_once_and_sends_nothing() -> Result<(), Box<dyn Error>>
{
    let repo = Repo::new("cadence", "telemetry: { enabled: true }\n")?;
    repo.cruise(SEPTEMBER, None)?;
    repo.cruise(SEPTEMBER, None)?;
    let september = text(&repo.print(SEPTEMBER)?.stdout);
    let october = repo.cruise(OCTOBER, None)?;
    let stderr = text(&october.stderr);
    assert!(
        stderr.contains("usage counts for 2026-09, as a send would carry them:"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&september),
        "the report is what print showed:\n{stderr}"
    );
    assert!(stderr.contains("Nothing was sent"), "{stderr}");
    assert!(
        text(&october.stdout).contains("payments-not-to-ledger"),
        "stdout is the reporter's alone"
    );
    assert!(!text(&october.stdout).contains("usage counts"));
    assert!(
        !repo.usage().join("2026-09.json").exists(),
        "deleted once shown"
    );
    let state: Value =
        serde_json::from_str(&std::fs::read_to_string(repo.usage().join("state.json"))?)?;
    assert_eq!(state, serde_json::json!({ "lastReport": "2026-10" }));
    let again = repo.cruise(LATE_OCTOBER, None)?;
    assert!(
        !text(&again.stderr).contains("usage counts"),
        "at most once per calendar month"
    );
    let now: Value = serde_json::from_str(&text(&repo.print(LATE_OCTOBER)?.stdout))?;
    assert_eq!(now["commands"]["cruise"], 2);
    assert_eq!(now["period"], "2026-10");
    Ok(())
}

#[test]
fn print_shows_the_payload_writes_nothing_and_offers_no_send() -> Result<(), Box<dyn Error>> {
    let repo = Repo::new("print", "telemetry: { enabled: true }\n")?;
    let empty = repo.print(SEPTEMBER)?;
    assert_eq!(empty.status.code(), Some(0));
    valid(&schema()?, &text(&empty.stdout))?;
    assert!(
        text(&empty.stderr).contains("telemetry: { enabled: true }"),
        "{}",
        text(&empty.stderr)
    );
    assert!(!repo.usage().exists(), "print writes nothing");
    let send = repo.run(&["--usage-counts", "send"], SEPTEMBER, None)?;
    assert_eq!(send.status.code(), Some(3));
    assert!(
        text(&send.stderr).contains("[possible values: print]"),
        "{}",
        text(&send.stderr)
    );
    let both = repo.run(&["--usage-counts", "print", "cruise"], SEPTEMBER, None)?;
    assert_eq!(both.status.code(), Some(3));
    assert!(!repo.usage().exists());
    std::fs::create_dir_all(repo.usage())?;
    std::fs::write(repo.usage().join("2026-09.json"), "not a payload")?;
    let damaged = repo.print(SEPTEMBER)?;
    assert_eq!(damaged.status.code(), Some(2));
    assert!(
        text(&damaged.stderr).contains("2026-09.json"),
        "{}",
        text(&damaged.stderr)
    );
    let cruise = repo.cruise(SEPTEMBER, None)?;
    assert_eq!(
        cruise.status.code(),
        Some(1),
        "a damaged file never changes the exit code"
    );
    assert!(text(&cruise.stderr).contains("warning: usage counts were not recorded"));
    Ok(())
}

#[test]
fn a_preset_cannot_turn_counting_on() -> Result<(), Box<dyn Error>> {
    let repo = Repo::new("preset", "extends: ./shared.yaml\n")?;
    std::fs::write(repo.0.join("shared.yaml"), "telemetry: { enabled: true }\n")?;
    let cruise = repo.cruise(SEPTEMBER, None)?;
    assert_eq!(cruise.status.code(), Some(3));
    assert!(
        text(&cruise.stderr)
            .contains("`telemetry` is read from the repository's own configuration file only")
    );
    assert!(!repo.usage().exists());
    Ok(())
}

#[test]
fn the_published_schema_is_the_committed_one_and_valid() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let file = std::fs::read_to_string(root.join("schema/usage-counts-v1.json"))?;
    let page = std::fs::read_to_string(root.join("docs/usage-counts.md"))?;
    let section = page
        .split_once("## The payload schema")
        .map(|(_, rest)| rest)
        .ok_or("the page has no schema section")?;
    let fenced = section
        .split_once("```json\n")
        .and_then(|(_, rest)| rest.split_once("```"))
        .map(|(body, _)| body)
        .ok_or("the schema section has no json fence")?;
    assert_eq!(
        fenced, file,
        "the page and schema/usage-counts-v1.json differ"
    );
    let validator = schema()?;
    // The plan's example payload validates; a payload carrying anything more does not.
    let example = serde_json::json!({ "tool": "rulebearing", "version": "0.4.0", "period": "2026-11",
        "commands": { "cruise": 412, "can-import": 3891, "fix": 12 },
        "rules": { format!("sha256:{SALT}"): 14, "no-circular": 3 } });
    assert!(validator.is_valid(&example));
    for (field, value) in [
        ("path", serde_json::json!("/home/me/repo")),
        ("repository", serde_json::json!("acme/payments")),
        ("user", serde_json::json!("me")),
    ] {
        let mut more = example.clone();
        more[field] = value;
        assert!(!validator.is_valid(&more), "{field} must be refused");
    }
    let mut named = example.clone();
    named["rules"]["src/payments/Pay.ts"] = serde_json::json!(1);
    assert!(!validator.is_valid(&named), "a path is not a rule key");
    let mut zero = example;
    zero["commands"]["cruise"] = serde_json::json!(0);
    assert!(!validator.is_valid(&zero));
    // Every subcommand name and every library rule name fits the schema's key patterns.
    let mut everything = Value::from(serde_json::Map::new());
    everything["tool"] = "rulebearing".into();
    everything["version"] = env!("CARGO_PKG_VERSION").into();
    everything["period"] = "2026-01".into();
    everything["commands"] = rb_cli::subcommands()
        .into_iter()
        .map(|n| (n, Value::from(1)))
        .collect();
    everything["rules"] = rb_cli::usage::payload::library()
        .iter()
        .map(|n| (n.clone(), Value::from(1)))
        .collect();
    let errors: Vec<String> = validator
        .iter_errors(&everything)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    Ok(())
}

/// The binary links no call that opens a connection or resolves a host name. A run of `nm` over
/// the binary the tests drive lists the symbols it takes from the system; none of them may be a
/// socket call. The list is checked to hold `write`, so an empty listing cannot pass.
#[cfg(unix)]
#[test]
fn the_binary_links_no_network_call() -> Result<(), Box<dyn Error>> {
    let mut listed = Command::new("nm").args(["-u", BIN]).output()?;
    if listed.stdout.is_empty() {
        listed = Command::new("nm").args(["-D", "-u", BIN]).output()?;
    }
    let symbols: std::collections::BTreeSet<String> = text(&listed.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .map(|symbol| {
            let bare = symbol.split('@').next().unwrap_or(symbol);
            bare.strip_prefix('_').unwrap_or(bare).to_owned()
        })
        .collect();
    assert!(
        symbols.contains("write"),
        "nm listed nothing useful: {}",
        text(&listed.stderr)
    );
    let network: Vec<&String> = symbols
        .iter()
        .filter(|s| {
            matches!(
                s.as_str(),
                "connect"
                    | "connectx"
                    | "socket"
                    | "getaddrinfo"
                    | "gethostbyname"
                    | "sendto"
                    | "sendmsg"
            )
        })
        .collect();
    assert!(network.is_empty(), "the binary links {network:?}");
    Ok(())
}

/// No crate's own code names a socket type, and the lock file carries no HTTP client: the
/// sending path does not exist, as ADR-0076 decides.
#[test]
fn no_crate_reaches_for_the_network() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    let mut pending = vec![root.join("crates")];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_dir() {
                if !matches!(
                    name.as_str(),
                    "tests" | "node_modules" | "target" | "__tests__" | "fixtures"
                ) {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                let source = std::fs::read_to_string(&path)?;
                for needle in [
                    "std::net",
                    "TcpStream",
                    "UdpSocket",
                    "TcpListener",
                    "ToSocketAddrs",
                ] {
                    if source.contains(needle) {
                        found.push(format!("{}: {needle}", path.display()));
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "{found:?}");
    let lock = std::fs::read_to_string(root.join("Cargo.lock"))?;
    for client in [
        "reqwest",
        "ureq",
        "hyper",
        "curl",
        "isahc",
        "attohttpc",
        "surf",
        "h2",
        "native-tls",
    ] {
        assert!(
            !lock.contains(&format!("name = \"{client}\"\n")),
            "Cargo.lock carries {client}"
        );
    }
    Ok(())
}
