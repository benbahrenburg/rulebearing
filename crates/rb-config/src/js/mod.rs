//! The JavaScript configuration evaluator: QuickJS, sandboxed.
//!
//! - Decisions: [ADR-0006](../../../../docs/adr/0006-embedded-quickjs-config-evaluator.md),
//!   [ADR-0027](../../../../docs/adr/0027-pure-path-and-url-modules-in-the-config-sandbox.md)
//! - Plan: [Wave 1, Step 2](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-2-the-quickjs-evaluator-1a)
//! - Requirements: [FR-CFG-03](../../../../docs/prd.md#fr-cfg-03), [NFR-SEC-01](../../../../docs/prd.md#nfr-sec-01)
//! - Security posture: [architecture § Security posture](../../../../docs/architecture.md#security-posture)
//!
//! The sandbox is a security boundary. Code inside it reaches the host through two functions
//! only, both implemented here: `__rb_resolve`, which maps a specifier to a file under the
//! repository root, a bundled preset or one of the pure `path` and `url` modules, and refuses
//! everything else; and `__rb_read`, which returns the text of a target `__rb_resolve` produced.
//! There is no `process`, no `fs`, no network and no timers. Evaluation is bounded in time and
//! memory; exceeding either is a configuration error (exit 3), never a hang.

pub mod via_node;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{Context, Ctx, Exception, Function, Module, Runtime, Value};

/// The module system the sandbox installs.
const SHIM: &str = include_str!("shim.js");

/// The bundled dependency-cruiser presets, vendored from 18.2.0 with its licence
/// (`presets/dependency-cruiser/LICENSE`).
pub const DEPENDENCY_CRUISER_PRESETS: &[(&str, &str)] = &[
    (
        "recommended.cjs",
        include_str!("../../../../presets/dependency-cruiser/recommended.cjs"),
    ),
    (
        "recommended-strict.cjs",
        include_str!("../../../../presets/dependency-cruiser/recommended-strict.cjs"),
    ),
    (
        "recommended-warn-only.cjs",
        include_str!("../../../../presets/dependency-cruiser/recommended-warn-only.cjs"),
    ),
    (
        "rules/no-circular.cjs",
        include_str!("../../../../presets/dependency-cruiser/rules/no-circular.cjs"),
    ),
    (
        "rules/no-deprecated-core.cjs",
        include_str!("../../../../presets/dependency-cruiser/rules/no-deprecated-core.cjs"),
    ),
    (
        "rules/no-duplicate-dependency-types.cjs",
        include_str!(
            "../../../../presets/dependency-cruiser/rules/no-duplicate-dependency-types.cjs"
        ),
    ),
    (
        "rules/no-non-package-json.cjs",
        include_str!("../../../../presets/dependency-cruiser/rules/no-non-package-json.cjs"),
    ),
    (
        "rules/no-orphans.cjs",
        include_str!("../../../../presets/dependency-cruiser/rules/no-orphans.cjs"),
    ),
    (
        "rules/not-to-deprecated.cjs",
        include_str!("../../../../presets/dependency-cruiser/rules/not-to-deprecated.cjs"),
    ),
    (
        "rules/not-to-unresolvable.cjs",
        include_str!("../../../../presets/dependency-cruiser/rules/not-to-unresolvable.cjs"),
    ),
];

/// The prefix of a bundled preset target.
const PRESET_PREFIX: &str = "rb:dc-preset/";

/// Node's built-in module names; asking for one is refused with a pointer to
/// `--config-via-node`.
const NODE_BUILTINS: &[&str] = &[
    "assert",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "string_decoder",
    "sys",
    "timers",
    "tls",
    "trace_events",
    "tty",
    "util",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

/// Limits on one evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Wall-clock time the evaluation may take.
    pub time: Duration,
    /// Heap the runtime may allocate, in bytes.
    pub memory: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            time: Duration::from_secs(5),
            memory: 128 * 1024 * 1024,
        }
    }
}

/// Why a JavaScript configuration could not be evaluated. Every variant is exit 3.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JsError {
    /// The code threw, or failed to parse.
    #[error("{file}: {message}")]
    Thrown {
        /// The configuration file.
        file: PathBuf,
        /// The exception's message.
        message: String,
    },
    /// The evaluation ran past the time limit.
    #[error(
        "{file}: evaluation exceeded the {seconds}-second limit; a configuration must not loop"
    )]
    Timeout {
        /// The configuration file.
        file: PathBuf,
        /// The limit.
        seconds: u64,
    },
    /// The configuration evaluated to something JSON cannot represent.
    #[error("{file}: the configuration is not a JSON-shaped object ({reason})")]
    NotAnObject {
        /// The configuration file.
        file: PathBuf,
        /// What it was instead.
        reason: String,
    },
    /// The configuration file could not be read.
    #[error("{file}: {reason}")]
    Read {
        /// The configuration file.
        file: PathBuf,
        /// The I/O error.
        reason: String,
    },
}

/// How a file is evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// CommonJS: `module.exports`.
    CommonJs,
    /// An ES module: `export default`.
    Module,
    /// JSON5 (a `.json` dependency-cruiser config dependency-cruiser reads with `json5`).
    Json5,
}

/// The resolution policy shared by `require`, `import` and the loader.
#[derive(Debug)]
struct Policy {
    root: PathBuf,
    /// Every file read, for the attest receipt.
    read: RefCell<BTreeSet<PathBuf>>,
}

impl Policy {
    fn new(root: &Path) -> Self {
        Self {
            root: canonical(root),
            read: RefCell::new(BTreeSet::new()),
        }
    }

    /// Maps `specifier`, written in `from`, to a target, or explains the refusal.
    fn resolve(&self, from: &str, specifier: &str) -> Result<String, String> {
        let bare = specifier.strip_prefix("node:").unwrap_or(specifier);
        match bare {
            "path" | "path/posix" => return Ok("rb:path".to_owned()),
            "url" => return Ok("rb:url".to_owned()),
            _ => {}
        }
        if specifier.starts_with("node:") || NODE_BUILTINS.contains(&bare) {
            return Err(format!(
                "`{specifier}` is not available in the configuration sandbox (no filesystem, network or process access, ADR-0006); run with --config-via-node to evaluate this configuration with Node"
            ));
        }
        if let Some(preset) = specifier.strip_prefix("dependency-cruiser/configs/") {
            return preset_target(preset).ok_or_else(|| {
                format!("`{specifier}` is not a bundled dependency-cruiser preset")
            });
        }
        if let Some(from_preset) = from.strip_prefix(PRESET_PREFIX) {
            let dir = Path::new(from_preset).parent().unwrap_or(Path::new(""));
            // Preset names are virtual and always use `/`, whatever the host separator.
            let joined = normalise(&dir.join(specifier));
            return preset_target(&joined.to_string_lossy().replace('\\', "/")).ok_or_else(|| {
                format!("`{specifier}` is not a bundled dependency-cruiser preset")
            });
        }
        let from_dir = Path::new(from).parent().unwrap_or(&self.root).to_path_buf();
        let candidates: Vec<PathBuf> = if specifier.starts_with('.') || specifier.starts_with('/') {
            vec![normalise(&from_dir.join(specifier))]
        } else {
            // A package under a `node_modules` folder inside the repository.
            from_dir
                .ancestors()
                .filter(|dir| dir.starts_with(&self.root))
                .map(|dir| dir.join("node_modules").join(specifier))
                .collect()
        };
        for candidate in candidates {
            if let Some(found) = self.find(&candidate) {
                return Ok(found.to_string_lossy().replace('\\', "/"));
            }
        }
        Err(format!(
            "cannot find `{specifier}` from {from} inside the repository"
        ))
    }

    /// The first existing file for `candidate`, trying the extensions Node tries.
    fn find(&self, candidate: &Path) -> Option<PathBuf> {
        let name = candidate.to_string_lossy();
        let mut tries = vec![candidate.to_path_buf()];
        for ext in [".js", ".cjs", ".mjs", ".json"] {
            tries.push(PathBuf::from(format!("{name}{ext}")));
        }
        for index in ["index.js", "index.cjs", "index.json"] {
            tries.push(candidate.join(index));
        }
        let package_main = candidate.join("package.json");
        if let Ok(text) = std::fs::read_to_string(&package_main)
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(main) = json.get("main").and_then(serde_json::Value::as_str)
        {
            tries.insert(1, normalise(&candidate.join(main)));
        }
        tries.into_iter().find_map(|path| {
            if !path.is_file() {
                return None;
            }
            let real = canonical(&path);
            real.starts_with(&self.root).then_some(real)
        })
    }

    /// The text of a target produced by [`Policy::resolve`].
    fn read(&self, target: &str) -> Result<String, String> {
        if let Some(preset) = target.strip_prefix(PRESET_PREFIX) {
            return DEPENDENCY_CRUISER_PRESETS
                .iter()
                .find(|(name, _)| *name == preset)
                .map(|(_, text)| (*text).to_owned())
                .ok_or_else(|| format!("no bundled preset `{preset}`"));
        }
        let path = canonical(Path::new(target));
        if !path.starts_with(&self.root) {
            return Err(format!("{target} is outside the repository"));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{target}: {e}"))?;
        self.read.borrow_mut().insert(path);
        Ok(text)
    }
}

fn preset_target(name: &str) -> Option<String> {
    let name = name.trim_start_matches("./");
    [name.to_owned(), format!("{name}.cjs")]
        .into_iter()
        .find(|candidate| {
            DEPENDENCY_CRUISER_PRESETS
                .iter()
                .any(|(preset, _)| preset == candidate)
        })
        .map(|found| format!("{PRESET_PREFIX}{found}"))
}

/// Resolves `.` and `..` without touching the filesystem.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The canonical form of a path, or the path itself when it does not exist.
fn canonical(path: &Path) -> PathBuf {
    let real = path.canonicalize().unwrap_or_else(|_| normalise(path));
    PathBuf::from(real.to_string_lossy().trim_start_matches(r"\\?\"))
}

/// The module system of a file, as Node would decide it.
pub fn kind_of(path: &Path, text: &str, root: &Path) -> Kind {
    match path.extension().and_then(|e| e.to_str()) {
        Some("mjs" | "mts") => return Kind::Module,
        Some("cjs" | "cts") => return Kind::CommonJs,
        Some("json" | "json5") => return Kind::Json5,
        _ => {}
    }
    let root = canonical(root);
    let start = canonical(path);
    for dir in start.ancestors().skip(1) {
        if let Ok(manifest) = std::fs::read_to_string(dir.join("package.json"))
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&manifest)
        {
            match json.get("type").and_then(serde_json::Value::as_str) {
                Some("module") => return Kind::Module,
                Some("commonjs") => return Kind::CommonJs,
                _ => {}
            }
            break;
        }
        if dir == root {
            break;
        }
    }
    // Node's syntax detection: a file with ES module syntax and no `type` is an ES module.
    if has_module_syntax(text) {
        Kind::Module
    } else {
        Kind::CommonJs
    }
}

/// Whether a line begins with an `import` or `export` statement.
fn has_module_syntax(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        (line.starts_with("export ") || line.starts_with("export{"))
            || (line.starts_with("import ")
                && (line.contains(" from ") || line.contains('"') || line.contains('\'')))
    })
}

struct SandboxResolver(Rc<Policy>);

impl Resolver for SandboxResolver {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        self.0
            .resolve(base, name)
            .map_err(|message| Exception::throw_message(ctx, &message))
    }
}

struct SandboxLoader(Rc<Policy>);

impl Loader for SandboxLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js>> {
        let quoted = serde_json::to_string(name).unwrap_or_default();
        let source = match name {
            "rb:path" => {
                "const p = globalThis.__rb_path; export default p; export const { join, resolve, dirname, basename, extname, relative, normalize, isAbsolute, sep, parse, format, posix } = p;".to_owned()
            }
            "rb:url" => {
                "const u = globalThis.__rb_url; export default u; export const { URL, fileURLToPath, pathToFileURL } = u;".to_owned()
            }
            _ if has_extension(name, "mjs") || has_extension(name, "js") && is_module_file(&self.0, name) => {
                let text = self
                    .0
                    .read(name)
                    .map_err(|message| Exception::throw_message(ctx, &message))?;
                with_import_meta(&text, name)
            }
            _ => format!("export default globalThis.__rb_load_cjs({quoted});"),
        };
        Module::declare(ctx.clone(), name, source)
    }
}

fn has_extension(name: &str, extension: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

fn is_module_file(policy: &Policy, name: &str) -> bool {
    let path = Path::new(name);
    let text = std::fs::read_to_string(path).unwrap_or_default();
    kind_of(path, &text, &policy.root) == Kind::Module
}

/// Replaces `import.meta.url`, `import.meta.dirname` and `import.meta.filename` with literals.
fn with_import_meta(text: &str, file: &str) -> String {
    let url = serde_json::to_string(&format!("file://{file}")).unwrap_or_default();
    let dir = Path::new(file)
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    text.replace("import.meta.url", &url)
        .replace(
            "import.meta.dirname",
            &serde_json::to_string(&dir).unwrap_or_default(),
        )
        .replace(
            "import.meta.filename",
            &serde_json::to_string(file).unwrap_or_default(),
        )
}

/// What an evaluation produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluated {
    /// The configuration object as JSON.
    pub value: serde_json::Value,
    /// Every file the evaluation read, the entry included.
    pub files: Vec<PathBuf>,
}

/// Evaluates the configuration at `entry` inside the sandbox rooted at `root`.
///
/// # Errors
/// [`JsError`] for a thrown exception, a refused `require`, a timeout, a memory overrun, or a
/// value that is not a JSON object.
pub fn evaluate(entry: &Path, root: &Path, limits: Limits) -> Result<Evaluated, JsError> {
    let entry = canonical(entry);
    let text = std::fs::read_to_string(&entry).map_err(|e| JsError::Read {
        file: entry.clone(),
        reason: e.to_string(),
    })?;
    let kind = kind_of(&entry, &text, root);
    evaluate_text(&entry, &text, kind, root, limits)
}

/// Evaluates `text` as if it were the file `entry`.
///
/// # Errors
/// See [`evaluate`].
pub fn evaluate_text(
    entry: &Path,
    text: &str,
    kind: Kind,
    root: &Path,
    limits: Limits,
) -> Result<Evaluated, JsError> {
    run(entry, text, kind, root, limits, None)
}

/// dependency-cruiser 18.2.0's `pryConfigFromTheConfig` and the `resolve` read after it
/// (`src/config-utl/extract-webpack-resolve-config.mjs`), as a function of the loaded module, the
/// `env` and the `arguments`: a function-shaped config is called with both, an array's first
/// element is taken, and the result's `resolve` block (or `{}`) is returned. As upstream, a
/// config that comes out `undefined` or `null` is an error, and a promise is not awaited.
pub const WEBPACK_PICK: &str = r"(function pick(m, env, args) {
  function pry(c) {
    let r = c;
    if (typeof c === 'function') r = c(env, args);
    if (Array.isArray(c)) r = pry(c[0]);
    return r;
  }
  const config = pry(m);
  if (config === undefined || config === null) {
    throw new TypeError('the webpack config evaluated to ' + config + ', which has no resolve block');
  }
  return config.resolve ? config.resolve : {};
})";

/// What a function-shaped webpack configuration is called with: `webpackConfig.env` and
/// `webpackConfig.arguments`, `null` when absent (as upstream passes them).
#[derive(Debug, Clone, Copy)]
pub struct WebpackCall<'a> {
    /// `webpackConfig.env`.
    pub env: &'a serde_json::Value,
    /// `webpackConfig.arguments`.
    pub arguments: &'a serde_json::Value,
}

/// Evaluates the webpack configuration at `entry` inside the sandbox and returns its `resolve`
/// block ([`WEBPACK_PICK`]). The sandbox is the one every JavaScript configuration gets: the
/// same `require`, the same pure `path` and `url`, the same time and memory limits.
///
/// # Errors
/// [`JsError`] as for [`evaluate`]; [`JsError::Thrown`] when the configuration or its function
/// throws, or when it comes out `undefined`.
pub fn evaluate_webpack(
    entry: &Path,
    root: &Path,
    limits: Limits,
    call: WebpackCall<'_>,
) -> Result<Evaluated, JsError> {
    let entry = canonical(entry);
    let text = std::fs::read_to_string(&entry).map_err(|e| JsError::Read {
        file: entry.clone(),
        reason: e.to_string(),
    })?;
    let kind = kind_of(&entry, &text, root);
    run(&entry, &text, kind, root, limits, Some(call))
}

/// The evaluation behind [`evaluate_text`] and [`evaluate_webpack`].
fn run(
    entry: &Path,
    text: &str,
    kind: Kind,
    root: &Path,
    limits: Limits,
    webpack: Option<WebpackCall<'_>>,
) -> Result<Evaluated, JsError> {
    let thrown = |message: String| JsError::Thrown {
        file: entry.to_path_buf(),
        message,
    };
    let runtime = Runtime::new().map_err(|e| thrown(e.to_string()))?;
    runtime.set_memory_limit(limits.memory);
    runtime.set_max_stack_size(1024 * 1024);
    let started = Instant::now();
    let limit = limits.time;
    runtime.set_interrupt_handler(Some(Box::new(move || started.elapsed() > limit)));
    let policy = Rc::new(Policy::new(root));
    runtime.set_loader(
        SandboxResolver(Rc::clone(&policy)),
        SandboxLoader(Rc::clone(&policy)),
    );
    let context = Context::full(&runtime).map_err(|e| thrown(e.to_string()))?;
    let entry_name = entry.to_string_lossy().replace('\\', "/");
    policy.read.borrow_mut().insert(entry.to_path_buf());
    let timed_out = || started.elapsed() > limit;
    let json = context.with(|ctx| -> Result<Option<String>, JsError> {
        let caught = |ctx: &Ctx<'_>, error: rquickjs::Error| -> JsError {
            if timed_out() {
                return JsError::Timeout {
                    file: entry.to_path_buf(),
                    seconds: limit.as_secs(),
                };
            }
            let message = if matches!(error, rquickjs::Error::Exception) {
                exception_message(&ctx.catch())
            } else {
                error.to_string()
            };
            thrown(message)
        };
        install(&ctx, &policy, root).map_err(|e| caught(&ctx, e))?;
        let value: Value = match kind {
            Kind::Json5 => ctx
                .eval(format!("(\n{text}\n)"))
                .map_err(|e| caught(&ctx, e))?,
            Kind::CommonJs => {
                let name = serde_json::to_string(&entry_name).unwrap_or_default();
                let call = if text.is_empty() {
                    format!("globalThis.__rb_load_cjs({name})")
                } else {
                    format!(
                        "globalThis.__rb_run_cjs({name}, {})",
                        serde_json::to_string(text).unwrap_or_default()
                    )
                };
                ctx.eval(call).map_err(|e| caught(&ctx, e))?
            }
            Kind::Module => {
                let source = with_import_meta(text, &entry_name);
                let declared = Module::declare(ctx.clone(), entry_name.as_str(), source)
                    .map_err(|e| caught(&ctx, e))?;
                let (evaluated, promise) = declared.eval().map_err(|e| caught(&ctx, e))?;
                promise.finish::<()>().map_err(|e| caught(&ctx, e))?;
                let namespace = evaluated.namespace().map_err(|e| caught(&ctx, e))?;
                namespace.get("default").map_err(|e| caught(&ctx, e))?
            }
        };
        let value = match webpack {
            None => value,
            Some(call) => {
                let pick: Function = ctx.eval(WEBPACK_PICK).map_err(|e| caught(&ctx, e))?;
                let json = |v: &serde_json::Value| {
                    ctx.json_parse(serde_json::to_string(v).unwrap_or_else(|_| "null".into()))
                };
                let env = json(call.env).map_err(|e| caught(&ctx, e))?;
                let arguments = json(call.arguments).map_err(|e| caught(&ctx, e))?;
                pick.call((value, env, arguments))
                    .map_err(|e| caught(&ctx, e))?
            }
        };
        if !value.is_object() || value.is_array() || value.is_function() {
            return Err(JsError::NotAnObject {
                file: entry.to_path_buf(),
                reason: format!("{:?}", value.type_of()).to_lowercase(),
            });
        }
        let json = ctx.json_stringify(value).map_err(|e| caught(&ctx, e))?;
        json.map(|s| s.to_string())
            .transpose()
            .map_err(|e| caught(&ctx, e))
    })?;
    let json = json.ok_or_else(|| JsError::NotAnObject {
        file: entry.to_path_buf(),
        reason: "undefined".to_owned(),
    })?;
    let value = serde_json::from_str(&json).map_err(|e| JsError::NotAnObject {
        file: entry.to_path_buf(),
        reason: e.to_string(),
    })?;
    let files = policy.read.borrow().iter().cloned().collect();
    Ok(Evaluated { value, files })
}

fn exception_message(value: &Value<'_>) -> String {
    if let Some(exception) = value.as_exception() {
        return exception
            .message()
            .unwrap_or_else(|| "an exception was thrown".to_owned());
    }
    value
        .as_string()
        .and_then(|s| s.to_string().ok())
        .unwrap_or_else(|| "an exception was thrown".to_owned())
}

/// Installs the host primitives and the module system.
fn install(ctx: &Ctx<'_>, policy: &Rc<Policy>, root: &Path) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let resolver = Rc::clone(policy);
    globals.set(
        "__rb_resolve",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'_>, from: String, specifier: String| {
                resolver
                    .resolve(&from, &specifier)
                    .map_err(|message| Exception::throw_message(&ctx, &message))
            },
        )?,
    )?;
    let reader = Rc::clone(policy);
    globals.set(
        "__rb_read",
        Function::new(ctx.clone(), move |ctx: Ctx<'_>, target: String| {
            reader
                .read(&target)
                .map_err(|message| Exception::throw_message(&ctx, &message))
        })?,
    )?;
    globals.set(
        "__rb_cwd",
        canonical(root).to_string_lossy().replace('\\', "/"),
    )?;
    ctx.eval::<(), _>(SHIM)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::fs;

    fn repo(files: &[(&str, &str)]) -> Result<tempdir::Dir, Box<dyn Error>> {
        let dir = tempdir::Dir::new()?;
        for (name, text) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, text)?;
        }
        Ok(dir)
    }

    fn run(dir: &tempdir::Dir, entry: &str) -> Result<serde_json::Value, JsError> {
        evaluate(&dir.path().join(entry), dir.path(), Limits::default()).map(|e| e.value)
    }

    #[test]
    fn commonjs_config_with_a_required_exceptions_json() -> Result<(), Box<dyn Error>> {
        // The reference pattern from design § The rule file: a JSON spliced into a regex.
        let dir = repo(&[
            ("exceptions.json", r#"["legacy-a", "legacy-b"]"#),
            (
                ".dependency-cruiser.cjs",
                r#"const exceptions = require("./exceptions.json");
module.exports = { forbidden: [{ name: "no-legacy", from: { pathNot: `^apps/(${exceptions.join("|")})/` }, to: {} }] };"#,
            ),
        ])?;
        let value = run(&dir, ".dependency-cruiser.cjs")?;
        assert_eq!(
            value["forbidden"][0]["from"]["pathNot"],
            "^apps/(legacy-a|legacy-b)/"
        );
        Ok(())
    }

    #[test]
    fn es_module_config_with_import_meta_and_url() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[
            (
                "base.cjs",
                "module.exports = { options: { tsPreCompilationDeps: true } };",
            ),
            (
                ".dependency-cruiser.mjs",
                r#"import { fileURLToPath } from "node:url";
import path from "node:path";
const base = fileURLToPath(new URL("base.cjs", import.meta.url));
export default { extends: path.basename(base), forbidden: [] };"#,
            ),
        ])?;
        let value = run(&dir, ".dependency-cruiser.mjs")?;
        assert_eq!(value["extends"], "base.cjs");
        Ok(())
    }

    #[test]
    fn es_module_can_import_commonjs_and_json() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[
            (
                "rules.cjs",
                "module.exports = [{ name: 'r', from: {}, to: {} }];",
            ),
            ("opts.json", r#"{ "maxDepth": 3 }"#),
            (
                "rulebearing.config.mjs",
                "import rules from './rules.cjs'; import opts from './opts.json'; export default { forbidden: rules, options: opts };",
            ),
        ])?;
        let value = run(&dir, "rulebearing.config.mjs")?;
        assert_eq!(value["forbidden"][0]["name"], "r");
        assert_eq!(value["options"]["maxDepth"], 3);
        Ok(())
    }

    #[test]
    fn bundled_presets_resolve() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[(
            ".dependency-cruiser.js",
            "const strict = require('dependency-cruiser/configs/recommended-strict'); module.exports = { forbidden: strict.forbidden };",
        )])?;
        let value = run(&dir, ".dependency-cruiser.js")?;
        let rules = value["forbidden"].as_array().cloned().unwrap_or_default();
        assert_eq!(rules.len(), 7);
        assert!(rules.iter().all(|r| r["severity"] == "error"));
        Ok(())
    }

    #[test]
    fn js_file_follows_package_type() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[
            ("package.json", r#"{ "type": "module" }"#),
            (
                ".dependency-cruiser.js",
                "export default { forbidden: [] };",
            ),
        ])?;
        assert_eq!(
            run(&dir, ".dependency-cruiser.js")?["forbidden"],
            serde_json::json!([])
        );
        Ok(())
    }

    #[test]
    fn json5_is_read() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[(
            ".dependency-cruiser.json",
            "{ // comment\n forbidden: [{ name: 'x', from: {}, to: {}, },], }",
        )])?;
        assert_eq!(
            run(&dir, ".dependency-cruiser.json")?["forbidden"][0]["name"],
            "x"
        );
        Ok(())
    }

    fn refused(dir: &tempdir::Dir, entry: &str) -> String {
        run(dir, entry)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default()
    }

    #[test]
    fn sandbox_escape_attempts_fail() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[
            ("fs.cjs", "const fs = require('fs'); module.exports = {};"),
            ("nodefs.mjs", "import fs from 'node:fs'; export default {};"),
            (
                "child.cjs",
                "require('child_process'); module.exports = {};",
            ),
            (
                "passwd.cjs",
                "require('../../../../../../../../etc/passwd'); module.exports = {};",
            ),
            ("process.cjs", "module.exports = { x: process.env.HOME };"),
            ("fetch.cjs", "module.exports = { x: typeof fetch };"),
            ("timer.cjs", "setTimeout(() => {}, 1); module.exports = {};"),
            ("loop.cjs", "for (;;) {} module.exports = {};"),
            ("array.cjs", "module.exports = [];"),
            ("throws.cjs", "throw new Error('boom');"),
            ("syntax.cjs", "module.exports = {"),
        ])?;
        assert!(refused(&dir, "fs.cjs").contains("--config-via-node"));
        assert!(refused(&dir, "nodefs.mjs").contains("not available"));
        assert!(refused(&dir, "child.cjs").contains("not available"));
        assert!(refused(&dir, "passwd.cjs").contains("inside the repository"));
        assert!(refused(&dir, "process.cjs").contains("process"));
        assert_eq!(
            run(&dir, "fetch.cjs")?["x"],
            "undefined",
            "fetch is not defined"
        );
        assert!(refused(&dir, "timer.cjs").contains("setTimeout"));
        let limits = Limits {
            time: Duration::from_millis(200),
            ..Limits::default()
        };
        let looped = evaluate(&dir.path().join("loop.cjs"), dir.path(), limits);
        assert!(matches!(looped, Err(JsError::Timeout { .. })), "{looped:?}");
        assert!(refused(&dir, "array.cjs").contains("not a JSON-shaped object"));
        assert!(refused(&dir, "throws.cjs").contains("boom"));
        assert!(!refused(&dir, "syntax.cjs").is_empty());
        Ok(())
    }

    fn webpack(
        dir: &tempdir::Dir,
        entry: &str,
        env: &serde_json::Value,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, JsError> {
        evaluate_webpack(
            &dir.path().join(entry),
            dir.path(),
            Limits::default(),
            WebpackCall { env, arguments },
        )
        .map(|e| e.value)
    }

    #[test]
    fn a_windows_drive_is_a_root_as_nodes_win32_path_reads_it() -> Result<(), Box<dyn Error>> {
        // On Windows the host hands the sandbox `C:/...` paths, so `__dirname` is one.
        let dir = repo(&[(
            "drive.cjs",
            "const path = require('path'); const url = require('url'); module.exports = { resolve: { alias: {
               resolved: path.resolve('C:/repo/app', 'src/lib'),
               climbed: path.resolve('C:/repo', '../../x'),
               other: path.resolve('C:/repo', 'D:/elsewhere'),
               joined: path.join('C:/repo', '../lib'),
               normal: path.normalize('C:/repo/./a/../b/'),
               top: path.dirname('C:/repo'),
               dir: path.dirname('C:/repo/a.js'),
               root: path.parse('C:/repo/a.js').root,
               relative: path.relative('C:/repo/a', 'C:/repo/b/c'),
               href: url.pathToFileURL('C:/repo/a b.js').href,
               back: url.fileURLToPath('file:///C:/repo/a%20b.js'),
               posix: url.fileURLToPath('file:///repo/a.js'),
               absolute: [path.isAbsolute('C:/x'), path.isAbsolute('/x'), path.isAbsolute('x'), path.isAbsolute('C:x')].join(),
             } } };",
        )])?;
        let null = serde_json::Value::Null;
        let alias = &webpack(&dir, "drive.cjs", &null, &null)?["alias"];
        assert_eq!(
            alias,
            &serde_json::json!({
                "resolved": "C:/repo/app/src/lib",
                "climbed": "C:/x",
                "other": "D:/elsewhere",
                "joined": "C:/lib",
                "normal": "C:/repo/b/",
                "top": "C:/",
                "dir": "C:/repo",
                "root": "C:/",
                "relative": "../b/c",
                "href": "file:///C:/repo/a%20b.js",
                "back": "C:/repo/a b.js",
                "posix": "/repo/a.js",
                "absolute": "true,true,false,false",
            })
        );
        Ok(())
    }

    #[test]
    fn webpack_configs_are_pried_as_upstream_pries_them() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[
            (
                "object.cjs",
                "const path = require('path'); module.exports = { entry: './x', resolve: { alias: { '@': path.join(__dirname, 'src') }, modules: ['node_modules', 'lib'], extensions: ['.ts', '.js'] } };",
            ),
            (
                "function.cjs",
                "module.exports = (env, argv) => ({ resolve: { alias: { app: env.production ? './dist' : './src' }, extensions: [argv.mode] } });",
            ),
            (
                "array.mjs",
                "export default [{ resolve: { modules: ['first'] } }, { resolve: { modules: ['second'] } }];",
            ),
            (
                "array-of-functions.cjs",
                "module.exports = [() => ({ resolve: { modules: ['made'] } })];",
            ),
            ("no-resolve.cjs", "module.exports = { entry: './x' };"),
            (
                "config.json",
                r#"{ "resolve": { "extensions": [".json5"] } }"#,
            ),
            ("undefined.cjs", "module.exports = () => undefined;"),
            ("empty-array.cjs", "module.exports = [];"),
            (
                "promise.cjs",
                "module.exports = async () => ({ resolve: { modules: ['late'] } });",
            ),
        ])?;
        let null = serde_json::Value::Null;
        let object = webpack(&dir, "object.cjs", &null, &null)?;
        assert_eq!(
            object["modules"],
            serde_json::json!(["node_modules", "lib"])
        );
        assert!(
            object["alias"]["@"]
                .as_str()
                .is_some_and(|a| a.ends_with("/src")),
            "{object}"
        );
        let env = serde_json::json!({ "production": true });
        let arguments = serde_json::json!({ "mode": ".mjs" });
        assert_eq!(
            webpack(&dir, "function.cjs", &env, &arguments)?,
            serde_json::json!({ "alias": { "app": "./dist" }, "extensions": [".mjs"] })
        );
        assert_eq!(
            webpack(&dir, "array.mjs", &null, &null)?,
            serde_json::json!({ "modules": ["first"] })
        );
        assert_eq!(
            webpack(&dir, "array-of-functions.cjs", &null, &null)?,
            serde_json::json!({ "modules": ["made"] })
        );
        assert_eq!(
            webpack(&dir, "no-resolve.cjs", &null, &null)?,
            serde_json::json!({})
        );
        assert_eq!(
            webpack(&dir, "config.json", &null, &null)?,
            serde_json::json!({ "extensions": [".json5"] })
        );
        for entry in ["undefined.cjs", "empty-array.cjs"] {
            let error = webpack(&dir, entry, &null, &null)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(error.contains("no resolve block"), "{entry}: {error}");
        }
        // As upstream, a promise is not awaited: it has no `resolve`.
        assert_eq!(
            webpack(&dir, "promise.cjs", &null, &null)?,
            serde_json::json!({})
        );
        Ok(())
    }

    #[test]
    fn a_webpack_config_gets_the_same_sandbox() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[
            (
                "webpack.fs.cjs",
                "module.exports = () => { require('fs'); return { resolve: {} }; };",
            ),
            (
                "webpack.plugin.cjs",
                "const Html = require('html-webpack-plugin'); module.exports = { plugins: [new Html()], resolve: {} };",
            ),
            (
                "webpack.passwd.cjs",
                "module.exports = { resolve: { alias: require('../../../../../../../../etc/passwd') } };",
            ),
            (
                "webpack.process.cjs",
                "module.exports = (env) => ({ resolve: { modules: [process.cwd()] } });",
            ),
            (
                "webpack.env.cjs",
                "module.exports = (env) => ({ resolve: { modules: [String(env.constructor.constructor('return typeof process')())] } });",
            ),
            (
                "webpack.loop.cjs",
                "module.exports = () => { for (;;) {} };",
            ),
            (
                "webpack.throws.cjs",
                "module.exports = () => { throw new Error('bad env'); };",
            ),
        ])?;
        let null = serde_json::Value::Null;
        let refused = |entry: &str| {
            webpack(&dir, entry, &null, &null)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default()
        };
        assert!(refused("webpack.fs.cjs").contains("--config-via-node"));
        assert!(
            !refused("webpack.plugin.cjs").is_empty(),
            "a package is not loaded"
        );
        assert!(refused("webpack.passwd.cjs").contains("inside the repository"));
        assert!(refused("webpack.process.cjs").contains("process"));
        // The env object is plain JSON parsed inside the sandbox: its constructor chain reaches
        // the sandbox's own Function, which still has no `process`.
        let env = serde_json::json!({});
        assert_eq!(
            webpack(&dir, "webpack.env.cjs", &env, &null)?,
            serde_json::json!({ "modules": ["undefined"] })
        );
        assert!(refused("webpack.throws.cjs").contains("bad env"));
        let limits = Limits {
            time: Duration::from_millis(200),
            ..Limits::default()
        };
        let looped = evaluate_webpack(
            &dir.path().join("webpack.loop.cjs"),
            dir.path(),
            limits,
            WebpackCall {
                env: &null,
                arguments: &null,
            },
        );
        assert!(matches!(looped, Err(JsError::Timeout { .. })), "{looped:?}");
        let missing = evaluate_webpack(
            &dir.path().join("nope.cjs"),
            dir.path(),
            Limits::default(),
            WebpackCall {
                env: &null,
                arguments: &null,
            },
        );
        assert!(matches!(missing, Err(JsError::Read { .. })), "{missing:?}");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_repository_is_refused() -> Result<(), Box<dyn Error>> {
        let outside = repo(&[("secret.json", r#"{"token": "x"}"#)])?;
        let dir = repo(&[("cfg.cjs", "module.exports = require('./link.json');")])?;
        std::os::unix::fs::symlink(
            outside.path().join("secret.json"),
            dir.path().join("link.json"),
        )?;
        assert!(refused(&dir, "cfg.cjs").contains("inside the repository"));
        Ok(())
    }

    #[test]
    fn memory_is_bounded() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[(
            "big.cjs",
            "const a = []; for (;;) { a.push('x'.repeat(1 << 20)); } module.exports = {};",
        )])?;
        let limits = Limits {
            memory: 16 * 1024 * 1024,
            ..Limits::default()
        };
        assert!(evaluate(&dir.path().join("big.cjs"), dir.path(), limits).is_err());
        Ok(())
    }

    #[test]
    fn files_read_are_recorded() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[
            ("a.json", "{}"),
            ("cfg.cjs", "require('./a.json'); module.exports = {};"),
        ])?;
        let evaluated = evaluate(&dir.path().join("cfg.cjs"), dir.path(), Limits::default())?;
        assert_eq!(evaluated.files.len(), 2);
        Ok(())
    }

    #[test]
    fn path_module_is_posix_and_pure() -> Result<(), Box<dyn Error>> {
        let dir = repo(&[(
            "cfg.cjs",
            r"const p = require('path');
module.exports = { j: p.join('a', '../b', 'c.js'), d: p.dirname('/x/y/z.ts'), b: p.basename('/x/y.ts', '.ts'),
  e: p.extname('a.test.ts'), r: p.relative('/a/b', '/a/c/d'), n: p.normalize('a//b/./c/..'), abs: p.isAbsolute('/x') };",
        )])?;
        let v = run(&dir, "cfg.cjs")?;
        assert_eq!(v["j"], "b/c.js");
        assert_eq!(v["d"], "/x/y");
        assert_eq!(v["b"], "y");
        assert_eq!(v["e"], ".ts");
        assert_eq!(v["r"], "../c/d");
        assert_eq!(v["n"], "a/b");
        assert_eq!(v["abs"], true);
        Ok(())
    }

    #[test]
    fn module_kind_detection() {
        let root = Path::new("/nonexistent-root");
        assert_eq!(kind_of(Path::new("/x/a.mjs"), "", root), Kind::Module);
        assert_eq!(kind_of(Path::new("/x/a.cjs"), "", root), Kind::CommonJs);
        assert_eq!(kind_of(Path::new("/x/a.json"), "", root), Kind::Json5);
        assert_eq!(
            kind_of(Path::new("/x/a.js"), "export default {}", root),
            Kind::Module
        );
        assert_eq!(
            kind_of(Path::new("/x/a.js"), "import x from './y';", root),
            Kind::Module
        );
        assert_eq!(
            kind_of(Path::new("/x/a.js"), "module.exports = {}", root),
            Kind::CommonJs
        );
    }

    /// A temporary directory removed on drop; std only, so no extra dependency.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNTER: AtomicU32 = AtomicU32::new(0);

        pub struct Dir(PathBuf);

        impl Dir {
            pub fn new() -> std::io::Result<Self> {
                let n = COUNTER.fetch_add(1, Ordering::Relaxed);
                let path =
                    std::env::temp_dir().join(format!("rb-config-js-{}-{n}", std::process::id()));
                let _ = std::fs::remove_dir_all(&path);
                std::fs::create_dir_all(&path)?;
                Ok(Self(path.canonicalize()?))
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
