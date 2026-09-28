//! `plugin:<path>` reporters, run in the configuration sandbox.
//!
//! - Architecture: [architecture § Security posture](../../../../docs/architecture.md#security-posture)
//! - Decisions: [ADR-0006](../../../../docs/adr/0006-embedded-quickjs-config-evaluator.md),
//!   [ADR-0027](../../../../docs/adr/0027-pure-path-and-url-modules-in-the-config-sandbox.md),
//!   [ADR-0030](../../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)
//! - Plan: [Wave 3, Step 7](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar),
//!   the [`plugin:<path>` contract](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//! - Requirements: [FR-OUT-01](../../../../docs/prd.md#fr-out-01), [NFR-SEC-01](../../../../docs/prd.md#nfr-sec-01)
//! - Coverage: [coverage § Output types](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `plugin:<path>`
//! - Specification: dependency-cruiser 18.2.0's `src/report/plugins.mjs` and its specs
//!   `test/report/plugins/*.spec.mjs`, run unmodified by conformance gate 1 layer 3
//!
//! A plugin is a JavaScript module whose default export takes the cruise result and returns
//! `{ output, exitCode }`. [`Sandbox::resolve`] finds the module as dependency-cruiser's
//! `import()` would (a path, a `file://` URL or a package name), but only inside the repository;
//! [`Sandbox::report`] loads it in a fresh sandbox, checks it as upstream's `isValidPlugin` does
//! (called with a minimal cruise result, it must return an own `output` and a numeric own
//! `exitCode`) and then calls it with the real result. The module gets the sandbox's `require`
//! and `import` (files and packages under the repository, the pure `path` and `url`) and nothing
//! else: no `process`, no filesystem, no network, no timers. A refusal, a time or memory overrun
//! and a reference to a host global are [`PluginError::Sandbox`], named as such.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::{Context, Ctx, Function, Module, Object, Runtime, Value};

use super::{
    Kind, Limits, Policy, Purpose, SandboxLoader, SandboxResolver, canonical, exception_message,
    install, kind_of, normalise, with_import_meta,
};

/// The prefix of a plugin output type.
pub const PREFIX: &str = "plugin:";

/// The module name after `plugin:`, as upstream's `/^plugin:(?<pluginName>.+)$/` takes it, or
/// `None` when `output_type` is not a plugin.
pub fn plugin_name(output_type: &str) -> Option<&str> {
    output_type.strip_prefix(PREFIX).filter(|n| !n.is_empty())
}

/// Globals a Node or browser plugin may reach for that the sandbox does not have. A reference to
/// one is a sandbox refusal, not a bug in the plugin. `require`, `module` and `__dirname` are not
/// here: an ES module has none of them under Node either.
const HOST_GLOBALS: &[&str] = &[
    "process",
    "global",
    "Buffer",
    "fetch",
    "XMLHttpRequest",
    "WebSocket",
    "EventSource",
    "Request",
    "Response",
    "Headers",
    "navigator",
    "setTimeout",
    "setInterval",
    "setImmediate",
    "clearTimeout",
    "clearInterval",
];

/// Upstream's `isValidPlugin`, then the call: returns `{ valid, reason }` for an invalid plugin,
/// `{ valid: true }` when only checking, and the report's shape otherwise.
const CALL: &str = r"(function rbPlugin(plugin, result, checkOnly) {
  const minimal = {
    modules: [],
    summary: { error: 0, info: 0, warn: 0, ignore: 0, totalCruised: 0, violations: [], optionsUsed: {} },
  };
  if (typeof plugin !== 'function') {
    return { valid: false, reason: 'its default export is ' + (plugin === null ? 'null' : typeof plugin) + ', not a function' };
  }
  const probe = plugin(minimal);
  if (!Object.hasOwn(probe, 'output')) {
    return { valid: false, reason: 'called with a minimal cruise result, it returned no own `output`' };
  }
  if (!Object.hasOwn(probe, 'exitCode')) {
    return { valid: false, reason: 'called with a minimal cruise result, it returned no own `exitCode`' };
  }
  if (typeof probe.exitCode !== 'number') {
    return { valid: false, reason: 'called with a minimal cruise result, it returned an `exitCode` that is a ' + typeof probe.exitCode + ', not a number' };
  }
  if (checkOnly) return { valid: true };
  const report = plugin(result);
  const shape = report === null || typeof report !== 'object' ? {} : report;
  return {
    valid: true,
    outputType: typeof shape.output,
    output: typeof shape.output === 'string' ? shape.output : undefined,
    exitCodeType: typeof shape.exitCode,
    exitCode: typeof shape.exitCode === 'number' ? shape.exitCode : undefined,
  };
})";

/// Why a plugin reporter could not render. The caller decides the exit code; every variant
/// names the plugin and says what to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PluginError {
    /// No module by that name inside the repository, or it failed to load (upstream's message).
    #[error(
        "Could not find reporter plugin '{name}' (or it isn't valid): {reason}. Name a JavaScript module inside the repository by its path from the working directory, by a file:// URL, or by the name of a package under node_modules"
    )]
    NotFound {
        /// The name after `plugin:`.
        name: String,
        /// What went wrong.
        reason: String,
    },
    /// The module loaded, but its default export is not a reporter (upstream's message).
    #[error(
        "{name} is not a valid plugin: {reason}. A plugin's default export is a function that takes the cruise result and returns {{ output: string, exitCode: number }}"
    )]
    Invalid {
        /// The name after `plugin:`.
        name: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The plugin asked for something the sandbox does not give, or ran past a limit.
    #[error(
        "{name} is not a valid plugin: the reporter sandbox refused it: {reason}. Plugin reporters run in Rulebearing's sandbox, with no filesystem outside the repository, no network, no process and bounded time and memory (ADR-0006); remove what the plugin reaches for, or use a built-in reporter"
    )]
    Sandbox {
        /// The name after `plugin:`.
        name: String,
        /// The refusal.
        reason: String,
    },
    /// The plugin threw while it was checked or called.
    #[error("plugin:{name} threw: {message}. Fix the plugin; it was called with the cruise result")]
    Thrown {
        /// The name after `plugin:`.
        name: String,
        /// The exception's message.
        message: String,
    },
}

/// A plugin found by [`Sandbox::resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    name: String,
    file: PathBuf,
}

impl Plugin {
    /// The name after `plugin:`, as written.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The module file, canonical.
    pub fn file(&self) -> &Path {
        &self.file
    }
}

/// What a plugin returned for the real result.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginOutput {
    /// `output`.
    pub output: String,
    /// `exitCode`, a JavaScript number.
    pub exit_code: f64,
}

impl PluginOutput {
    /// `exitCode` as a process exit count: a whole number of zero or more, or `None`.
    pub fn whole_exit_code(&self) -> Option<u64> {
        let code = self.exit_code;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "used only when whole and non-negative; `as` saturates above u64::MAX"
        )]
        let whole = code as u64;
        (code.is_finite() && code >= 0.0 && code.fract() == 0.0).then_some(whole)
    }
}

/// The sandbox a plugin reporter runs in: the repository it may read, the directory relative
/// plugin paths start from, and its limits.
#[derive(Debug, Clone)]
pub struct Sandbox {
    root: PathBuf,
    cwd: PathBuf,
    limits: Limits,
}

/// Where a failure happened: loading the module is upstream's `import()`; checking and calling
/// it are the plugin's own code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Load,
    Call,
}

/// What one run of [`CALL`] produced.
enum Called {
    Valid,
    Invalid(String),
    Report(PluginOutput),
}

impl Sandbox {
    /// A sandbox over the repository at `root`, resolving relative plugin paths from `cwd`.
    pub fn new(root: &Path, cwd: &Path, limits: Limits) -> Self {
        Self {
            root: canonical(root),
            cwd: canonical(cwd),
            limits,
        }
    }

    /// The repository root the sandbox reads, canonical.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The module `name` names: `file://` and an absolute path as they are, a path beginning with
    /// `.` from the working directory, anything else as a package under `node_modules` (or the
    /// repository's own package, through its `exports`).
    ///
    /// # Errors
    /// [`PluginError::Sandbox`] for a path outside the repository or a Node built-in;
    /// [`PluginError::NotFound`] when no module is there.
    pub fn resolve(&self, name: &str) -> Result<Plugin, PluginError> {
        let policy = Policy::for_purpose(&self.root, Purpose::Plugin);
        let path_like = name.strip_prefix("file://").map(percent_decode);
        let from = self.cwd.join("plugin");
        let found = match path_like {
            Some(path) => self.find_path(&policy, name, &path)?,
            None if name.starts_with('.') || Path::new(name).is_absolute() => {
                self.find_path(&policy, name, name)?
            }
            None => policy
                .resolve(&from.to_string_lossy(), name)
                .map(PathBuf::from)
                .map_err(|reason| self.refusal_or(&policy, name, &reason))?,
        };
        if found.to_string_lossy().starts_with("rb:") {
            return Err(PluginError::Invalid {
                name: name.to_owned(),
                reason: "the sandbox's pure `path` and `url` modules are not reporters".into(),
            });
        }
        Ok(Plugin {
            name: name.to_owned(),
            file: canonical(&found),
        })
    }

    /// A path (or a decoded `file://` URL) from the working directory, refused outside the
    /// repository before anything is read, so no file outside it is even probed.
    fn find_path(&self, policy: &Policy, name: &str, path: &str) -> Result<PathBuf, PluginError> {
        let candidate = normalise(&self.cwd.join(path));
        if !candidate.starts_with(&self.root) {
            return Err(PluginError::Sandbox {
                name: name.to_owned(),
                reason: format!(
                    "{} is outside the repository; the reporter sandbox reads only files inside {}",
                    candidate.display(),
                    self.root.display()
                ),
            });
        }
        policy
            .find(&candidate)
            .ok_or_else(|| PluginError::NotFound {
                name: name.to_owned(),
                reason: format!("no module at {} inside the repository", candidate.display()),
            })
    }

    fn refusal_or(&self, policy: &Policy, name: &str, reason: &str) -> PluginError {
        match policy.refused.borrow().clone() {
            Some(refusal) => PluginError::Sandbox {
                name: name.to_owned(),
                reason: refusal,
            },
            None => PluginError::NotFound {
                name: name.to_owned(),
                reason: format!("{reason} (from {})", self.cwd.display()),
            },
        }
    }

    /// Whether the plugin is valid, as upstream's `isValidPlugin` decides.
    ///
    /// # Errors
    /// [`PluginError`] when it cannot be loaded, the sandbox refuses it, or it throws.
    pub fn is_valid(&self, plugin: &Plugin) -> Result<bool, PluginError> {
        match self.check(plugin) {
            Ok(()) => Ok(true),
            Err(PluginError::Invalid { .. }) => Ok(false),
            Err(other) => Err(other),
        }
    }

    /// Loads and checks the plugin as upstream's `getExternalPluginReporter` does, without calling
    /// it with a result.
    ///
    /// # Errors
    /// [`PluginError::Invalid`] with the reason when `isValidPlugin` would say no; the other
    /// variants as [`Sandbox::is_valid`].
    pub fn check(&self, plugin: &Plugin) -> Result<(), PluginError> {
        match self.run(plugin, None)? {
            Called::Valid | Called::Report(_) => Ok(()),
            Called::Invalid(reason) => Err(PluginError::Invalid {
                name: plugin.name.clone(),
                reason,
            }),
        }
    }

    /// Checks the plugin as upstream does, then calls it with `result`.
    ///
    /// # Errors
    /// [`PluginError::Invalid`] for a plugin upstream would refuse, or one whose report has no
    /// string `output` or no numeric `exitCode`; the other variants as [`Sandbox::is_valid`].
    pub fn report(
        &self,
        plugin: &Plugin,
        result: &serde_json::Value,
    ) -> Result<PluginOutput, PluginError> {
        match self.run(plugin, Some(result))? {
            Called::Report(output) => Ok(output),
            Called::Invalid(reason) => Err(PluginError::Invalid {
                name: plugin.name.clone(),
                reason,
            }),
            Called::Valid => Err(PluginError::Invalid {
                name: plugin.name.clone(),
                reason: "it returned no report".into(),
            }),
        }
    }

    /// One fresh runtime: load the module, run [`CALL`].
    fn run(
        &self,
        plugin: &Plugin,
        result: Option<&serde_json::Value>,
    ) -> Result<Called, PluginError> {
        let setup = |e: rquickjs::Error| PluginError::Thrown {
            name: plugin.name.clone(),
            message: e.to_string(),
        };
        let runtime = Runtime::new().map_err(setup)?;
        runtime.set_memory_limit(self.limits.memory);
        runtime.set_max_stack_size(1024 * 1024);
        let started = Instant::now();
        let limit = self.limits.time;
        runtime.set_interrupt_handler(Some(Box::new(move || started.elapsed() > limit)));
        let policy = Rc::new(Policy::for_purpose(&self.root, Purpose::Plugin));
        runtime.set_loader(
            SandboxResolver(Rc::clone(&policy)),
            SandboxLoader(Rc::clone(&policy)),
        );
        let context = Context::full(&runtime).map_err(setup)?;
        let failure = Failure {
            plugin,
            policy: &policy,
            started,
            limits: self.limits,
        };
        context.with(|ctx| {
            install(&ctx, &policy, &self.cwd).map_err(|e| failure.of(&ctx, Stage::Load, e))?;
            let exported = self
                .load(&ctx, plugin, &policy)
                .map_err(|e| failure.of(&ctx, Stage::Load, e))?;
            let answer =
                invoke(&ctx, exported, result).map_err(|e| failure.of(&ctx, Stage::Call, e))?;
            read_answer(&answer, result.is_some()).map_err(|e| failure.of(&ctx, Stage::Call, e))
        })
    }

    /// The module's default export, as `import()` gives it: an ES module's `default`, a CommonJS
    /// module's `module.exports`.
    fn load<'js>(
        &self,
        ctx: &Ctx<'js>,
        plugin: &Plugin,
        policy: &Policy,
    ) -> rquickjs::Result<Value<'js>> {
        let name = plugin.file.to_string_lossy().replace('\\', "/");
        let text = policy
            .read(&name)
            .map_err(|message| rquickjs::Exception::throw_message(ctx, &message))?;
        match kind_of(&plugin.file, &text, &self.root) {
            Kind::Module => {
                let declared =
                    Module::declare(ctx.clone(), name.as_str(), with_import_meta(&text, &name))?;
                let (evaluated, promise) = declared.eval()?;
                promise.finish::<()>()?;
                evaluated.namespace()?.get("default")
            }
            Kind::CommonJs | Kind::Json5 => {
                let quoted = serde_json::to_string(&name).unwrap_or_default();
                ctx.eval(format!("globalThis.__rb_load_cjs({quoted})"))
            }
        }
    }
}

/// Runs [`CALL`] over the default export and the result (`undefined` when only checking).
fn invoke<'js>(
    ctx: &Ctx<'js>,
    exported: Value<'js>,
    result: Option<&serde_json::Value>,
) -> rquickjs::Result<Object<'js>> {
    let function: Function = ctx.eval(CALL)?;
    let input = match result {
        Some(value) => {
            ctx.json_parse(serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned()))?
        }
        None => Value::new_undefined(ctx.clone()),
    };
    function.call((exported, input, result.is_none()))
}

/// Reads [`CALL`]'s answer.
fn read_answer(answer: &Object<'_>, called: bool) -> rquickjs::Result<Called> {
    if !answer.get::<_, bool>("valid")? {
        return Ok(Called::Invalid(
            answer
                .get::<_, Option<String>>("reason")?
                .unwrap_or_default(),
        ));
    }
    if !called {
        return Ok(Called::Valid);
    }
    let output_type: String = answer.get("outputType")?;
    let Some(output) = answer.get::<_, Option<String>>("output")? else {
        return Ok(Called::Invalid(format!(
            "called with the cruise result, it returned an `output` that is {}, not a string",
            article(&output_type)
        )));
    };
    let exit_code_type: String = answer.get("exitCodeType")?;
    let Some(exit_code) = answer.get::<_, Option<f64>>("exitCode")? else {
        return Ok(Called::Invalid(format!(
            "called with the cruise result, it returned an `exitCode` that is {}, not a number",
            article(&exit_code_type)
        )));
    };
    Ok(Called::Report(PluginOutput { output, exit_code }))
}

/// `a number`, `an object`, `undefined`.
fn article(type_name: &str) -> String {
    match type_name {
        "undefined" => "undefined".to_owned(),
        t if t.starts_with(['a', 'e', 'i', 'o', 'u']) => format!("an {t}"),
        t => format!("a {t}"),
    }
}

/// What a failed run is classified by.
struct Failure<'a> {
    plugin: &'a Plugin,
    policy: &'a Policy,
    started: Instant,
    limits: Limits,
}

impl Failure<'_> {
    /// A sandbox refusal when the sandbox caused the failure, else what upstream would say.
    fn of(&self, ctx: &Ctx<'_>, stage: Stage, error: rquickjs::Error) -> PluginError {
        let name = self.plugin.name.clone();
        let sandbox = |reason: String| PluginError::Sandbox {
            name: name.clone(),
            reason,
        };
        if self.started.elapsed() > self.limits.time {
            return sandbox(format!(
                "it ran past the {} time limit",
                duration(self.limits.time)
            ));
        }
        let message = match error {
            rquickjs::Error::Exception => exception_message(&ctx.catch()),
            rquickjs::Error::Allocation => "out of memory".to_owned(),
            other => other.to_string(),
        };
        if message.contains("out of memory") {
            return sandbox(format!(
                "it ran past the {} MiB memory limit",
                self.limits.memory / (1024 * 1024)
            ));
        }
        if let Some(refusal) = self.policy.refused.borrow().clone() {
            return sandbox(refusal);
        }
        if let Some(global) = host_global(&message) {
            return sandbox(format!(
                "`{global}` is not available in the reporter sandbox (no filesystem, network, process or timers, ADR-0006)"
            ));
        }
        match stage {
            Stage::Load => PluginError::NotFound {
                name,
                reason: message,
            },
            Stage::Call => PluginError::Thrown { name, message },
        }
    }
}

/// The host global a `ReferenceError` names, when it is one the sandbox withholds.
fn host_global(message: &str) -> Option<&'static str> {
    let subject = message.strip_suffix(" is not defined")?;
    let subject = subject.trim_matches(['\'', '"', '`']);
    HOST_GLOBALS.iter().copied().find(|g| *g == subject)
}

/// `5-second`, `200 ms`.
fn duration(limit: Duration) -> String {
    if limit.subsec_millis() == 0 && limit.as_secs() > 0 {
        format!("{}-second", limit.as_secs())
    } else {
        format!("{} ms", limit.as_millis())
    }
}

/// A `file://` URL's path with `%XX` escapes decoded; an escape that is not one is kept.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escape = (bytes[i] == b'%')
            .then(|| text.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = escape {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8_lossy(&out).into_owned();
    // `file:///C:/x` names `C:/x`, as Node's win32 fileURLToPath reads it.
    let drive = decoded.as_bytes();
    if drive.len() > 3 && drive[0] == b'/' && drive[1].is_ascii_alphabetic() && drive[2] == b':' {
        decoded[1..].to_owned()
    } else {
        decoded
    }
}

impl Limits {
    /// The limits a plugin reporter runs under: longer and larger than a configuration's, since
    /// it reads the whole cruise result, and still bounded.
    pub fn plugin() -> Self {
        Self {
            time: Duration::from_secs(30),
            memory: 512 * 1024 * 1024,
        }
    }
}

#[cfg(test)]
mod tests;
