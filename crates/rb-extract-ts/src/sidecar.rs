//! `--sidecar node`: CoffeeScript and LiveScript files extracted by the repository's own
//! dependency-cruiser, run by Node, and merged into the walk.
//!
//! - Architecture: [Extractors](../../../docs/architecture.md#extractors) (not native:
//!   CoffeeScript, LiveScript), [Security posture](../../../docs/architecture.md#security-posture)
//!   (the sidecar is explicit)
//! - Decisions: [ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md) (the only
//!   Node spawn besides `--config-via-node`; without the flag, exit 2 with a named reason),
//!   [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) (the sidecar is
//!   dispatch inside this crate; its options arrive through `rb_model::TypeScriptOptions`)
//! - Plan: [Wave 3, Step 10](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar),
//!   [§ 1.6](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#16-decisions-applied-and-decisions-this-wave-must-make)
//!   (a version other than 18.2.0 is a warning)
//! - Requirements: [FR-EXT-TS-05](../../../docs/prd.md#fr-ext-ts-05), [NFR-SEC-01](../../../docs/prd.md#nfr-sec-01)
//! - Source: [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution)
//!   (row "CoffeeScript, LiveScript")
//!
//! **Locating.** [`Sidecar::node`] finds `dependency-cruiser` as Node would from the repository:
//! `node_modules/dependency-cruiser` in the repository or a folder above it, or the folder itself
//! when its `package.json` is dependency-cruiser's (a checkout of dependency-cruiser, which is how
//! the conformance run uses it). Node is `$RULEBEARING_NODE` or `node` on the path, as for
//! `--config-via-node`. Node is started once to ask dependency-cruiser's public API which
//! extensions it can read (`allExtensions`): dependency-cruiser reads a file whose transpiler is
//! missing as plain JavaScript without a word, so a missing `coffeescript` or `livescript` stops
//! the run here instead, naming the package to install.
//!
//! **Running.** Node runs dependency-cruiser's own command line, no shell, arguments as a vector:
//! `--config <file> --output-type json --` and the CoffeeScript and LiveScript files the walk
//! reached, each starting with `-` given as `./<file>`, so no file name is ever read as an option
//! (a file named `--output-to=x.coffee` would otherwise make dependency-cruiser write a file). The
//! configuration file is created new, mode 0600, in a folder of its own made fresh with mode 0700
//! under the system's temporary folder, and the folder is removed after the run, so another user
//! of a shared temporary folder cannot plant or swap it. It holds the run's TypeScript options block ([`options_block`]), whatever format the
//! run's configuration was in: a dependency-cruiser configuration's options are that block
//! already, and a native `rulebearing.yaml`'s `languages.typescript` block uses dependency-cruiser's
//! names, so both translate without loss and dependency-cruiser never reads a file it would
//! refuse. Two keys differ from the run's: `maxDepth` is `0`, because dependency-cruiser does not
//! read a file it first meets at the depth limit even when it is also an initial source, and the
//! rules are two `ignore` rules exactly when the run reads licences or deprecations, which is
//! what makes dependency-cruiser resolve them. No rule of the run reaches dependency-cruiser.
//!
//! **Merging** ([`extract_modules`]). The native walk runs first. Each CoffeeScript or LiveScript
//! file it reaches is not parsed; it is handed to the sidecar, and dependency-cruiser's result
//! for it (and for every other such file dependency-cruiser read on the way) stands in for
//! reading it, exactly as an unchanged file's earlier result stands in for it in an incremental
//! run ([`crate::pipeline::Reused`]). The walk then continues from those files' dependencies,
//! so a JavaScript file a CoffeeScript file imports is extracted natively, once, and a module is
//! never duplicated. The native walk's modules win for every file it reads; dependency-cruiser's
//! answer is taken only for the files the walk cannot read. The module order is the walk's, so
//! the document stays deterministic. Every dependency of a file the sidecar extracted is marked
//! `sidecar: true`, with no `line` or `column` (dependency-cruiser 18.2.0 records neither); every
//! other dependency has no `sidecar` field.
//!
//! **The receipt.** `summary.sidecar` names the tool, its version (from its `package.json`) and
//! how many files of the graph it extracted. An incremental run that spawns nothing keeps the
//! earlier run's version, so a cached run's receipt is the cold run's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use rb_model::{Module, SidecarReceipt, TypeScriptOptions};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::pipeline::{self, ExtractedModule, Reused, Settings};
use crate::resolve::{self, ResolveConfig};

/// The package the sidecar runs.
pub const TOOL: &str = "dependency-cruiser";

/// The version the sidecar's edges are proven against: the conformance pin. Any other version
/// runs, with a warning ([plan § 1.6](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#16-decisions-applied-and-decisions-this-wave-must-make)).
pub const PINNED_VERSION: &str = "18.2.0";

/// Why the sidecar could not answer.
#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    /// No dependency-cruiser here or above.
    #[error(
        "dependency-cruiser is not installed in {repo} or a folder above it; run `npm install --save-dev dependency-cruiser@{PINNED_VERSION} coffeescript` there",
        repo = repo.display()
    )]
    NotInstalled {
        /// Where the search started.
        repo: PathBuf,
    },
    /// dependency-cruiser's `package.json` does not say what the sidecar needs.
    #[error("{manifest} is not a dependency-cruiser package.json the sidecar can use: {reason}; reinstall dependency-cruiser", manifest = manifest.display())]
    Manifest {
        /// The file.
        manifest: PathBuf,
        /// What is missing.
        reason: String,
    },
    /// Node could not be started.
    #[error(
        "could not start `{node}`: {reason}; install Node 22 or later (https://nodejs.org), or set RULEBEARING_NODE to its path"
    )]
    NodeMissing {
        /// The executable tried.
        node: String,
        /// The operating system's message.
        reason: String,
    },
    /// dependency-cruiser cannot load the transpiler a file needs, and would read the file as
    /// JavaScript.
    #[error(
        "dependency-cruiser {version} cannot read {extension} files because `{transpiler}` is not installed where it can load it; run `npm install --save-dev {transpiler}` beside dependency-cruiser"
    )]
    NoTranspiler {
        /// dependency-cruiser's version.
        version: String,
        /// The file's extension.
        extension: String,
        /// The package that reads it.
        transpiler: &'static str,
    },
    /// Node or dependency-cruiser exited with an error.
    #[error("{what} exited {status}: {stderr}")]
    Failed {
        /// What was run.
        what: &'static str,
        /// The exit code, `-1` when a signal ended it.
        status: i32,
        /// Its standard error, trimmed.
        stderr: String,
    },
    /// The output is not what dependency-cruiser writes.
    #[error("dependency-cruiser's output is not a cruise result: {reason}")]
    Output {
        /// The parser's message.
        reason: String,
    },
    /// dependency-cruiser did not read a file it was given.
    #[error(
        "dependency-cruiser did not extract {file}; check that the file exists and that no exclude or includeOnly pattern removes it"
    )]
    Unanswered {
        /// The file.
        file: String,
    },
    /// The configuration file could not be written.
    #[error("cannot write the sidecar's configuration {path}: {source}", path = path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The error.
        source: std::io::Error,
    },
}

/// The options block dependency-cruiser is given: `options` as dependency-cruiser's names spell
/// them, with `maxDepth` `0` (see the module doc).
pub fn options_block(options: &TypeScriptOptions) -> Map<String, Value> {
    let mut block = match serde_json::to_value(options) {
        Ok(Value::Object(block)) => block,
        _ => Map::new(),
    };
    block.insert("maxDepth".into(), Value::from(0));
    block
}

/// The configuration dependency-cruiser runs with: `block` from [`options_block`], and the
/// `ignore` rules that make it resolve licences and deprecations when the run does.
pub fn configuration(block: &Map<String, Value>, config: &ResolveConfig) -> Value {
    let mut forbidden = Vec::new();
    if config.resolve_licenses {
        forbidden.push(json!({
            "name": "rulebearing-sidecar-licenses",
            "severity": "ignore",
            "from": {},
            "to": { "license": "^$" }
        }));
    }
    if config.resolve_deprecations {
        forbidden.push(json!({
            "name": "rulebearing-sidecar-deprecations",
            "severity": "ignore",
            "from": {},
            "to": { "dependencyTypes": ["deprecated"] }
        }));
    }
    let mut out = Map::new();
    if !forbidden.is_empty() {
        out.insert("forbidden".into(), Value::Array(forbidden));
    }
    out.insert("options".into(), Value::Object(block.clone()));
    Value::Object(out)
}

/// The transpiler package dependency-cruiser needs for a sidecar file.
pub fn transpiler_for(file: &str) -> &'static str {
    if resolve::extension(file) == ".ls" {
        "livescript"
    } else {
        "coffeescript"
    }
}

/// The script Node runs to list the extensions dependency-cruiser cannot read: its public API's
/// `allExtensions`.
const AVAILABILITY: &str = r#"
const { pathToFileURL } = await import("node:url");
const cruiser = await import(pathToFileURL(process.argv[1]).href);
const missing = cruiser.allExtensions.filter((e) => !e.available).map((e) => e.extension);
process.stdout.write(JSON.stringify(missing));
"#;

/// A located dependency-cruiser and the Node that runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecar {
    node: String,
    package: PathBuf,
    version: String,
    bin: PathBuf,
    unavailable: Vec<String>,
}

/// dependency-cruiser's package: where it is, its version, its command line and its API entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The package folder.
    pub folder: PathBuf,
    /// `version`.
    pub version: String,
    /// The `depcruise` command's script.
    pub bin: PathBuf,
    /// The module `import "dependency-cruiser"` loads.
    pub entry: PathBuf,
}

impl Package {
    /// The dependency-cruiser Node would load from `repo`: `node_modules/dependency-cruiser` in
    /// `repo` or the nearest folder above it that has one, or a folder on the way that is
    /// dependency-cruiser itself.
    ///
    /// # Errors
    /// [`SidecarError::NotInstalled`] when there is none; [`SidecarError::Manifest`] when its
    /// `package.json` does not name a version, the command or the entry.
    pub fn find(repo: &Path) -> Result<Self, SidecarError> {
        // Node cannot open a script named with Windows' verbatim prefix.
        let repo = &rb_model::without_verbatim(repo);
        for folder in repo.ancestors() {
            let installed = folder.join("node_modules").join(TOOL);
            if installed.join("package.json").is_file() {
                return Self::read(&installed);
            }
            let manifest = folder.join("package.json");
            let is_the_tool = std::fs::read_to_string(&manifest)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .is_some_and(|value| value.get("name").and_then(Value::as_str) == Some(TOOL));
            if is_the_tool {
                return Self::read(folder);
            }
        }
        Err(SidecarError::NotInstalled { repo: repo.clone() })
    }

    /// The package in `folder`, from its `package.json`.
    ///
    /// # Errors
    /// [`SidecarError::Manifest`] when the file cannot be read or lacks a field.
    pub fn read(folder: &Path) -> Result<Self, SidecarError> {
        let manifest = folder.join("package.json");
        let bad = |reason: &str| SidecarError::Manifest {
            manifest: manifest.clone(),
            reason: reason.to_owned(),
        };
        let text = std::fs::read_to_string(&manifest).map_err(|e| bad(&e.to_string()))?;
        let value: Value = serde_json::from_str(&text).map_err(|e| bad(&e.to_string()))?;
        let version = value
            .get("version")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("no version"))?;
        let bin = match value.get("bin") {
            Some(Value::String(bin)) => Some(bin.as_str()),
            Some(Value::Object(bins)) => ["depcruise", "dependency-cruiser", "dependency-cruise"]
                .iter()
                .find_map(|name| bins.get(*name).and_then(Value::as_str)),
            _ => None,
        }
        .ok_or_else(|| bad("no depcruise command under bin"))?;
        let exported = value.get("exports").and_then(|exports| match exports {
            Value::String(entry) => Some(entry.as_str()),
            Value::Object(map) => map.get(".").and_then(|dot| match dot {
                Value::String(entry) => Some(entry.as_str()),
                Value::Object(conditions) => ["import", "default"]
                    .iter()
                    .find_map(|c| conditions.get(*c).and_then(Value::as_str)),
                _ => None,
            }),
            _ => None,
        });
        let entry = exported
            .or_else(|| value.get("main").and_then(Value::as_str))
            .ok_or_else(|| bad("no entry under exports or main"))?;
        Ok(Self {
            folder: folder.to_path_buf(),
            version: version.to_owned(),
            bin: folder.join(bin),
            entry: folder.join(entry),
        })
    }
}

/// A folder only this process can use, holding one run's configuration, removed with everything
/// in it when dropped, however the run ended. It is created fresh (`create_dir` fails on any
/// existing entry, a symbolic link included) with mode 0700 on Unix, and the file in it is
/// created with `create_new` and mode 0600, so another user of a shared temporary folder can
/// neither plant the file nor swap it for their own.
struct Private(PathBuf);

/// A builder for a folder only its owner can enter: mode 0700 where the platform has modes.
fn private_dir_builder() -> std::fs::DirBuilder {
    #[cfg(unix)]
    {
        let mut builder = std::fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
    }
    #[cfg(not(unix))]
    {
        std::fs::DirBuilder::new()
    }
}

impl Private {
    /// A fresh private folder under `parent`, skipping names already taken.
    fn create(parent: &Path) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut last = None;
        for _ in 0..64 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let dir = parent.join(format!(
                "rulebearing-sidecar-{}-{}-{nanos:08x}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match private_dir_builder().create(&dir) {
                Ok(()) => return Ok(Self(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("no free name")))
    }

    /// Writes `text` to a new file `name` in the folder; an existing entry is an error, never
    /// followed or truncated.
    fn write_new(&self, name: &str, text: &str) -> std::io::Result<PathBuf> {
        use std::io::Write as _;
        let path = self.0.join(name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(&path)?.write_all(text.as_bytes())?;
        Ok(path)
    }
}

impl Drop for Private {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `file` as an argument dependency-cruiser's command line reads as a path: one that starts with
/// `-` gets `./` in front, which dependency-cruiser normalises away, so the module keeps its name.
/// The files also follow `--`; either alone stops a file named `--output-to=x` being an option.
fn as_path_argument(file: &str) -> String {
    if file.starts_with('-') {
        format!("./{file}")
    } else {
        file.to_owned()
    }
}

/// The configuration file's name inside its [`Private`] folder.
const CONFIGURATION_FILE: &str = "dependency-cruiser.json";

/// What dependency-cruiser's `json` output holds that the sidecar reads.
#[derive(Deserialize)]
struct Cruised {
    modules: Vec<Module>,
}

/// Whether dependency-cruiser read `module` (rather than listing it for a dependency it did not
/// follow).
fn was_read(module: &Module) -> bool {
    module.followable.is_none()
        && module.core_module.is_none()
        && module.could_not_resolve.is_none()
        && module.matches_do_not_follow.is_none()
}

impl Sidecar {
    /// The sidecar for `repo`: dependency-cruiser as [`Package::find`] locates it, run by
    /// `$RULEBEARING_NODE` or the `node` on the path.
    ///
    /// # Errors
    /// See [`Sidecar::with_node`].
    pub fn node(repo: &Path) -> Result<Self, SidecarError> {
        let node = std::env::var("RULEBEARING_NODE").unwrap_or_else(|_| "node".to_owned());
        Self::with_node(&node, Package::find(repo)?)
    }

    /// The sidecar running `package` with the Node executable `node`, which is started once to
    /// ask which extensions the package can read.
    ///
    /// # Errors
    /// [`SidecarError::NodeMissing`] when Node cannot be started; [`SidecarError::Failed`] or
    /// [`SidecarError::Output`] when the package cannot be loaded.
    pub fn with_node(node: &str, package: Package) -> Result<Self, SidecarError> {
        let output = Command::new(node)
            .arg("--input-type=module")
            .arg("-e")
            .arg(AVAILABILITY)
            // The entry is a script argument, never a Node option, whatever its name.
            .arg("--")
            .arg(&package.entry)
            .current_dir(&package.folder)
            .output()
            .map_err(|e| SidecarError::NodeMissing {
                node: node.to_owned(),
                reason: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(SidecarError::Failed {
                what: "node, loading dependency-cruiser",
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        let unavailable: Vec<String> =
            serde_json::from_slice(&output.stdout).map_err(|e| SidecarError::Output {
                reason: format!("allExtensions: {e}"),
            })?;
        Ok(Self {
            node: node.to_owned(),
            package: package.folder,
            version: package.version,
            bin: package.bin,
            unavailable,
        })
    }

    /// dependency-cruiser's version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Where dependency-cruiser is installed.
    pub fn package(&self) -> &Path {
        &self.package
    }

    /// The extensions this dependency-cruiser cannot read, as its `allExtensions` lists them.
    pub fn unavailable(&self) -> &[String] {
        &self.unavailable
    }

    /// Runs dependency-cruiser in `cwd` over `files` with `configuration` (from
    /// [`configuration`]) and returns what it read of each CoffeeScript and LiveScript file, keyed
    /// by source: every file of `files` and any other such file it read on the way.
    ///
    /// # Errors
    /// [`SidecarError::NoTranspiler`] before anything runs when a file's transpiler is missing;
    /// [`SidecarError::Failed`], [`SidecarError::Output`] or [`SidecarError::Unanswered`] when
    /// dependency-cruiser fails, prints something else, or leaves a file out.
    pub fn run(
        &self,
        cwd: &Path,
        files: &[String],
        configuration: &Value,
    ) -> Result<BTreeMap<String, Module>, SidecarError> {
        for file in files {
            let extension = resolve::extension(file);
            if self.unavailable.iter().any(|e| e == extension) {
                return Err(SidecarError::NoTranspiler {
                    version: self.version.clone(),
                    extension: extension.to_owned(),
                    transpiler: transpiler_for(file),
                });
            }
        }
        let text = serde_json::to_string(configuration).map_err(|e| SidecarError::Output {
            reason: e.to_string(),
        })?;
        let temporary = std::env::temp_dir();
        let private = Private::create(&temporary).map_err(|source| SidecarError::Io {
            path: temporary.clone(),
            source,
        })?;
        let path = private
            .write_new(CONFIGURATION_FILE, &text)
            .map_err(|source| SidecarError::Io {
                path: private.0.join(CONFIGURATION_FILE),
                source,
            })?;
        let output = Command::new(&self.node)
            .arg(&self.bin)
            .arg("--config")
            .arg(&path)
            .arg("--output-type")
            .arg("json")
            .arg("--")
            .args(files.iter().map(|f| as_path_argument(f)))
            .current_dir(rb_model::without_verbatim(cwd))
            .output()
            .map_err(|e| SidecarError::NodeMissing {
                node: self.node.clone(),
                reason: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(SidecarError::Failed {
                what: "dependency-cruiser",
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        let cruised: Cruised =
            serde_json::from_slice(&output.stdout).map_err(|e| SidecarError::Output {
                reason: e.to_string(),
            })?;
        let answered: BTreeMap<String, Module> = cruised
            .modules
            .into_iter()
            .filter(|m| crate::needs_sidecar(&m.source) && was_read(m))
            .map(|m| (m.source.clone(), m))
            .collect();
        if let Some(file) = files.iter().find(|f| !answered.contains_key(*f)) {
            return Err(SidecarError::Unanswered { file: file.clone() });
        }
        Ok(answered)
    }
}

/// What [`extract_modules`] returns: the walk's modules, and the version of the
/// dependency-cruiser that extracted its sidecar files, when it has any.
#[derive(Debug)]
pub struct Walked {
    /// The modules, in the walk's order.
    pub modules: Vec<ExtractedModule>,
    /// The version the receipt records.
    pub version: Option<String>,
}

/// An error of the sidecar, as the extraction reports it: on the first file it was asked about.
fn failed(file: Option<&String>, error: &SidecarError) -> rb_model::ExtractError {
    let file = file.map_or("", String::as_str);
    rb_model::ExtractError::UnsupportedFile {
        path: PathBuf::from(file),
        reason: format!("--sidecar node: {error}{}", crate::csx_note(file)),
    }
}

/// The walk from `initial` (the inputs' initial sources, [`pipeline::gather_initial_sources`]),
/// with `reused` standing in for the files it names, and with the sidecar when
/// [`Settings::sidecar`] is set (see the module doc). Without the sidecar this is
/// [`pipeline::extract_reusing_from`], and a CoffeeScript or LiveScript file the walk reaches stops it
/// with [`crate::SIDECAR_REASON`]. `earlier` is the version of an earlier extraction whose
/// sidecar files `reused` holds.
///
/// # Errors
/// As [`pipeline::extract_reusing_from`], and a sidecar failure named on the file that needed it.
pub fn extract_modules(
    initial: &[String],
    settings: &Settings,
    config: &ResolveConfig,
    reused: &BTreeMap<String, Reused>,
    earlier: Option<&str>,
) -> Result<Walked, rb_model::ExtractError> {
    let Some(block) = &settings.sidecar else {
        return Ok(Walked {
            modules: pipeline::extract_reusing_from(initial, settings, config, reused)?,
            version: None,
        });
    };
    let mut reused = reused.clone();
    let mut sidecar: Option<Sidecar> = None;
    let mut version = earlier.map(str::to_owned);
    let configuration = configuration(block, config);
    loop {
        let pending = pipeline::sidecar_pending(initial, settings, config, &mut reused);
        if pending.is_empty() {
            break;
        }
        let running = match &mut sidecar {
            Some(running) => running,
            None => sidecar
                .insert(Sidecar::node(&settings.cwd).map_err(|e| failed(pending.first(), &e))?),
        };
        let answered = running
            .run(&settings.cwd, &pending, &configuration)
            .map_err(|e| failed(pending.first(), &e))?;
        version = Some(running.version().to_owned());
        for (source, module) in answered {
            reused.entry(source).or_insert_with(|| Reused {
                dependencies: module
                    .dependencies
                    .iter()
                    .map(crate::from_dependency)
                    .collect(),
                experimental_stats: module.experimental_stats,
                code: None,
            });
        }
    }
    let modules = pipeline::extract_reusing_from(initial, settings, config, &reused)?;
    let read_by_sidecar = modules
        .iter()
        .find(|m| m.as_dependency.is_none() && crate::needs_sidecar(&m.source));
    if let (Some(module), None) = (read_by_sidecar, &version) {
        // Reused from an extraction that did not record the version: the installed one wrote it.
        let package = Package::find(&settings.cwd).map_err(|e| failed(Some(&module.source), &e))?;
        version = Some(package.version);
    }
    Ok(Walked {
        version: read_by_sidecar.and(version),
        modules,
    })
}

/// The receipt for an extraction's modules: the files the sidecar extracted, with `version`.
pub fn receipt(modules: &[Module], version: Option<&str>) -> Option<SidecarReceipt> {
    let files = modules
        .iter()
        .filter(|m| m.language.is_some() && crate::needs_sidecar(&m.source))
        .count() as u64;
    (files > 0).then(|| SidecarReceipt {
        tool: TOOL.to_owned(),
        version: version.unwrap_or_default().to_owned(),
        files,
    })
}

/// The warning a receipt with a version other than [`PINNED_VERSION`] carries.
pub fn version_warning(receipt: &SidecarReceipt) -> Option<rb_model::Warning> {
    (receipt.version != PINNED_VERSION).then(|| rb_model::Warning {
        path: None,
        message: format!(
            "the sidecar ran {} {}; its edges are proven against {PINNED_VERSION}, the version the conformance suite pins; install dependency-cruiser@{PINNED_VERSION} for the proven result",
            receipt.tool, receipt.version
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rb-sidecar-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }

    const MANIFEST: &str = r#"{"name": "dependency-cruiser", "version": "18.2.0",
        "bin": {"depcruise": "bin/dependency-cruise.mjs"},
        "exports": {".": {"types": "./types/x.d.mts", "import": "./src/main/index.mjs"}}}"#;

    #[test]
    fn the_package_is_found_as_node_would_find_it() {
        let dir = scratch("find");
        let repo = dir.join("repo/packages/app");
        let _ = std::fs::create_dir_all(&repo);
        assert!(matches!(
            Package::find(&repo),
            Err(SidecarError::NotInstalled { .. })
        ));
        write(
            &dir.join("repo/node_modules/dependency-cruiser/package.json"),
            MANIFEST,
        );
        let found = Package::find(&repo);
        let Ok(package) = found else {
            unreachable!("{found:?}");
        };
        assert_eq!(
            package.folder,
            dir.join("repo/node_modules/dependency-cruiser")
        );
        assert_eq!(package.version, "18.2.0");
        assert_eq!(
            package.bin,
            dir.join("repo/node_modules/dependency-cruiser/bin/dependency-cruise.mjs")
        );
        assert_eq!(
            package.entry,
            dir.join("repo/node_modules/dependency-cruiser/./src/main/index.mjs")
        );
        // The repository that is dependency-cruiser itself, nearer than any node_modules above.
        write(&repo.join("package.json"), MANIFEST);
        assert_eq!(
            Package::find(&repo).map(|p| p.folder).ok(),
            Some(repo.clone())
        );
        // Another package's manifest is passed over.
        write(&repo.join("package.json"), r#"{"name": "app"}"#);
        assert_eq!(
            Package::find(&repo).map(|p| p.folder).ok(),
            Some(dir.join("repo/node_modules/dependency-cruiser"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manifest_without_what_the_sidecar_needs_is_named() {
        let dir = scratch("manifest");
        for (text, reason) in [
            ("{", "EOF"),
            (r#"{"name": "dependency-cruiser"}"#, "no version"),
            (r#"{"version": "1.0.0"}"#, "no depcruise command"),
            (
                r#"{"version": "1.0.0", "bin": {"other": "x.mjs"}}"#,
                "no depcruise command",
            ),
            (
                r#"{"version": "1.0.0", "bin": "bin/x.mjs"}"#,
                "no entry under exports or main",
            ),
        ] {
            write(&dir.join("package.json"), text);
            let message = Package::read(&dir)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(message.contains(reason), "{text}: {message}");
            assert!(message.contains("package.json"), "{message}");
        }
        write(
            &dir.join("package.json"),
            r#"{"version": "17.0.0", "bin": "bin/x.mjs", "exports": "./x.mjs"}"#,
        );
        let read = Package::read(&dir);
        assert_eq!(
            read.as_ref().map(|p| p.entry.clone()).ok(),
            Some(dir.join("./x.mjs"))
        );
        write(
            &dir.join("package.json"),
            r#"{"version": "17.0.0", "bin": "bin/x.mjs", "main": "src/main.js"}"#,
        );
        assert_eq!(
            Package::read(&dir).map(|p| p.entry).ok(),
            Some(dir.join("src/main.js"))
        );
        let missing = Package::read(&dir.join("absent"));
        assert!(matches!(missing, Err(SidecarError::Manifest { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_node_is_named_with_the_fix() {
        let package = Package {
            folder: std::env::temp_dir(),
            version: "18.2.0".into(),
            bin: PathBuf::from("bin.mjs"),
            entry: PathBuf::from("index.mjs"),
        };
        let error = Sidecar::with_node("rulebearing-no-such-node", package)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("could not start `rulebearing-no-such-node`"),
            "{error}"
        );
        assert!(
            error.contains("RULEBEARING_NODE") && error.contains("Node 22"),
            "{error}"
        );
        let not_installed = SidecarError::NotInstalled {
            repo: PathBuf::from("/r"),
        }
        .to_string();
        assert!(
            not_installed.contains("npm install --save-dev dependency-cruiser@18.2.0"),
            "{not_installed}"
        );
    }

    #[test]
    fn a_missing_transpiler_stops_the_run_before_anything_runs() {
        let sidecar = Sidecar {
            node: "rulebearing-no-such-node".into(),
            package: PathBuf::from("/p"),
            version: "18.2.0".into(),
            bin: PathBuf::from("/p/bin.mjs"),
            unavailable: vec![".ls".into()],
        };
        assert_eq!(sidecar.version(), "18.2.0");
        assert_eq!(sidecar.package(), Path::new("/p"));
        assert_eq!(sidecar.unavailable(), [".ls"]);
        let error = sidecar
            .run(Path::new("."), &["a.ls".into()], &json!({}))
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("cannot read .ls files")
                && error.contains("npm install --save-dev livescript"),
            "{error}"
        );
        // A file whose transpiler is there goes on to Node, which is missing here.
        let error = sidecar
            .run(Path::new("."), &["a.coffee".into()], &json!({}))
            .err();
        assert!(
            matches!(error, Some(SidecarError::NodeMissing { .. })),
            "{error:?}"
        );
    }

    #[test]
    fn each_extension_names_its_transpiler() {
        for (file, transpiler) in [
            ("a.coffee", "coffeescript"),
            ("a.litcoffee", "coffeescript"),
            ("a.coffee.md", "coffeescript"),
            ("a.cjsx", "coffeescript"),
            ("a.csx", "coffeescript"),
            ("a.ls", "livescript"),
        ] {
            assert_eq!(transpiler_for(file), transpiler, "{file}");
        }
    }

    #[test]
    fn the_configuration_carries_the_options_and_only_the_rules_that_resolve() {
        let options = TypeScriptOptions {
            base_dir: Some("web".into()),
            max_depth: Some(3),
            sidecar: Some(rb_model::SidecarRuntime::Node),
            ..TypeScriptOptions::default()
        };
        let block = options_block(&options);
        assert_eq!(
            Value::Object(block.clone()),
            json!({"baseDir": "web", "maxDepth": 0})
        );
        let plain = configuration(&block, &ResolveConfig::default());
        assert_eq!(plain, json!({"options": {"baseDir": "web", "maxDepth": 0}}));
        let mut config = ResolveConfig {
            resolve_licenses: true,
            ..ResolveConfig::default()
        };
        let licenses = configuration(&block, &config);
        assert_eq!(licenses["forbidden"][0]["to"], json!({"license": "^$"}));
        assert_eq!(licenses["forbidden"][0]["severity"], "ignore");
        assert_eq!(licenses["forbidden"].as_array().map(Vec::len), Some(1));
        config.resolve_licenses = false;
        config.resolve_deprecations = true;
        let deprecations = configuration(&block, &config);
        assert_eq!(
            deprecations["forbidden"][0]["to"],
            json!({"dependencyTypes": ["deprecated"]})
        );
        config.resolve_licenses = true;
        assert_eq!(
            configuration(&block, &config)["forbidden"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn the_receipt_counts_the_files_the_sidecar_read_and_warns_off_the_pin() {
        let read = |source: &str| {
            let mut module = Module::new(source);
            module.language = Some(rb_model::Language::Javascript);
            module
        };
        let modules = vec![
            read("a.coffee"),
            read("b.js"),
            read("c.ls"),
            Module::new("d.coffee"),
        ];
        assert_eq!(
            receipt(&modules, Some("18.2.0")),
            Some(SidecarReceipt {
                tool: "dependency-cruiser".into(),
                version: "18.2.0".into(),
                files: 2,
            })
        );
        assert_eq!(receipt(&modules[1..2], Some("18.2.0")), None);
        let pinned = receipt(&modules, Some(PINNED_VERSION));
        assert!(pinned.as_ref().and_then(version_warning).is_none());
        let other = receipt(&modules, Some("17.3.1"));
        let warning = other.as_ref().and_then(version_warning).map(|w| w.message);
        assert!(
            warning
                .as_deref()
                .is_some_and(|w| w.contains("dependency-cruiser 17.3.1") && w.contains("18.2.0")),
            "{warning:?}"
        );
    }

    #[test]
    fn only_modules_dependency_cruiser_read_count_as_answers() {
        let mut unfollowed = Module::new("x.coffee");
        unfollowed.followable = Some(false);
        assert!(!was_read(&unfollowed));
        let mut core = Module::new("fs");
        core.core_module = Some(true);
        assert!(!was_read(&core));
        let mut unresolved = Module::new("y");
        unresolved.could_not_resolve = Some(true);
        assert!(!was_read(&unresolved));
        let mut stopped = Module::new("z.coffee");
        stopped.matches_do_not_follow = Some(true);
        assert!(!was_read(&stopped));
        assert!(was_read(&Module::new("a.coffee")));
    }

    #[test]
    fn the_configuration_lives_in_a_private_folder_that_is_removed() -> std::io::Result<()> {
        let parent = scratch("private");
        let (a, b) = (Private::create(&parent)?, Private::create(&parent)?);
        assert_ne!(a.0, b.0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(std::fs::metadata(&a.0)?.permissions().mode() & 0o777, 0o700);
        }
        let path = a.write_new(CONFIGURATION_FILE, "{}")?;
        assert_eq!(std::fs::read_to_string(&path)?, "{}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path)?.permissions().mode() & 0o777,
                0o600
            );
        }
        // An entry already at the path is never truncated or written through.
        assert!(
            a.write_new(CONFIGURATION_FILE, "{\"extends\": \"evil\"}")
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path)?, "{}");
        let folder = a.0.clone();
        drop(a);
        assert!(!folder.exists());
        drop(b);
        let _ = std::fs::remove_dir_all(&parent);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_symbolic_link_is_never_followed() -> std::io::Result<()> {
        let parent = scratch("planted");
        let target = parent.join("victim.json");
        write(&target, "untouched");
        let private = Private::create(&parent)?;
        std::os::unix::fs::symlink(&target, private.0.join(CONFIGURATION_FILE))?;
        assert!(private.write_new(CONFIGURATION_FILE, "{}").is_err());
        assert_eq!(std::fs::read_to_string(&target)?, "untouched");
        // A folder name already taken, by a directory or a link, is skipped, never reused.
        let taken = Private::create(&parent)?;
        assert_ne!(taken.0, private.0);
        drop(private);
        assert_eq!(std::fs::read_to_string(&target)?, "untouched");
        let _ = std::fs::remove_dir_all(&parent);
        Ok(())
    }

    #[test]
    fn a_file_that_looks_like_an_option_is_passed_as_a_path() {
        assert_eq!(
            as_path_argument("--output-to=x.coffee"),
            "./--output-to=x.coffee"
        );
        assert_eq!(as_path_argument("-x.coffee"), "./-x.coffee");
        assert_eq!(
            as_path_argument("src/--output-to=x.coffee"),
            "src/--output-to=x.coffee"
        );
        assert_eq!(as_path_argument("../a.coffee"), "../a.coffee");
    }
}
