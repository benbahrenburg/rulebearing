// What the playground component does, apart from finding its elements in the page: check the
// loaded module; in the cruise-result mode run `evaluate` over a pasted cruise result, in the
// source-files mode run `cruise_ts` or `cruise_py` over the files the visitor chose; run `explain`
// over the current cruise result; time each run and show the report or the module's reason,
// verbatim. The elements are passed in as plain objects, so the tests drive this without a browser.
//
// Architecture: docs/architecture.md#security-posture (the playground runs entirely client-side).
// Decisions: docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.md,
// docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md,
// docs/adr/0073-extractors-read-files-through-rb-model.md (the files are read from memory).
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3; the exports are frozen in § 1.6.
// Requirement: FR-REACH-01.

import { configIn, describeFiles, inferLanguage } from './playground-files.js';
import type { FileSet, Language } from './playground-files.js';

/** The `rb-wasm` exports the component calls. */
export interface RbWasm {
  evaluate(config: string, cruiseJson: string, outputType: string): string;
  cruiseTs(files: ReadonlyMap<string, string>, config: string, outputType: string): string;
  cruisePy(files: ReadonlyMap<string, string>, config: string, outputType: string): string;
  explain(config: string, cruiseJson: string, rule: string): string;
  version(): string;
}

/** The module `wasm-bindgen --target web` writes: an initialiser and the exports. */
export interface RbWasmModule {
  readonly init: () => Promise<void>;
  readonly api: RbWasm;
}

function isFunction(value: unknown): value is (...args: unknown[]) => unknown {
  return typeof value === 'function';
}

const EXPORTS = ['evaluate', 'cruise_ts', 'cruise_py', 'explain', 'version'] as const;

/** Checks that `loaded` is the `rb-wasm` module, and returns its initialiser and exports. */
export function asRbWasm(loaded: unknown): RbWasmModule {
  if (typeof loaded !== 'object' || loaded === null) {
    throw new Error('the playground module did not load');
  }
  const module = loaded as Record<string, unknown>;
  const {
    default: init,
    evaluate,
    cruise_ts: cruiseTs,
    cruise_py: cruisePy,
    explain,
    version,
  } = module;
  if (
    !isFunction(init) ||
    !isFunction(evaluate) ||
    !isFunction(cruiseTs) ||
    !isFunction(cruisePy) ||
    !isFunction(explain) ||
    !isFunction(version)
  ) {
    throw new Error(`the playground module does not export ${EXPORTS.join(', ')}`);
  }
  return {
    init: async () => {
      await init();
    },
    api: {
      evaluate: (config, cruiseJson, outputType) =>
        String(evaluate(config, cruiseJson, outputType)),
      cruiseTs: (files, config, outputType) => String(cruiseTs(files, config, outputType)),
      cruisePy: (files, config, outputType) => String(cruisePy(files, config, outputType)),
      explain: (config, cruiseJson, rule) => String(explain(config, cruiseJson, rule)),
      version: () => String(version()),
    },
  };
}

/** The text of an error the module threw: wasm-bindgen throws the reason as a string. */
export function errorText(error: unknown): string {
  if (typeof error === 'string') {
    return error;
  }
  return error instanceof Error ? error.message : String(error);
}

/** A button the component enables once the module has loaded. */
export interface Button {
  disabled: boolean;
  addEventListener(type: 'click', listener: () => void): void;
}

/** A control whose value the visitor picks. */
export interface Choice {
  value: string;
  addEventListener(type: 'change', listener: () => void): void;
}

/** The parts of one playground component the logic touches. */
export interface PlaygroundView {
  readonly config: { value: string };
  readonly graph: { value: string };
  readonly outputType: { value: string };
  /** `graph` (a pasted cruise result) or `files` (source files). */
  readonly mode: Choice;
  /** `auto` (inferred from the files), `typescript` or `python`. */
  readonly language: { value: string };
  readonly rule: { value: string };
  readonly run: Button;
  readonly explain: Button;
  /** The examples' buttons, each with the example's id. */
  readonly examples: readonly { readonly id: string; readonly button: Button }[];
  /** The folder and file pickers and the drop zone's input, enabled with the rest. */
  readonly pickers: readonly { disabled: boolean }[];
  readonly graphPanel: { hidden: boolean };
  readonly filesPanel: { hidden: boolean };
  /** What the chosen files are: how many were read, which were skipped and why. */
  readonly filesNote: { textContent: string | null };
  readonly output: { textContent: string | null };
  readonly status: { textContent: string | null };
  /**
   * The component's `dataset`: `state` is `ready`, `ok` or `error`, `elapsed` the last run in ms,
   * `action` what the last run was (`evaluate`, `cruise_ts`, `cruise_py` or `explain`), and
   * `preload`, set by the page, the example loaded once the module is ready.
   */
  readonly data: Record<string, string | undefined>;
}

/** A preloaded example the page embeds, from a committed fixture. */
export interface Example {
  readonly id: string;
  readonly mode: 'graph' | 'files';
  readonly config: string;
  readonly outputType: string;
  /** The rule `explain` is preset to. */
  readonly rule: string;
  /** The cruise result, in the cruise-result mode. */
  readonly graph?: string;
  /** The language, in the source-files mode. */
  readonly language?: Language;
  /** Paths to source text, in the source-files mode. */
  readonly files?: Readonly<Record<string, string>>;
}

function isStringRecord(value: unknown): value is Record<string, string> {
  return (
    typeof value === 'object' &&
    value !== null &&
    Object.values(value).every((v) => typeof v === 'string')
  );
}

/** Checks the JSON the page embeds for an example. */
export function asExample(value: unknown): Example {
  if (typeof value !== 'object' || value === null) {
    throw new Error('an example is an object');
  }
  const { id, mode, config, outputType, rule, graph, language, files } = value as Record<
    string,
    unknown
  >;
  if (
    typeof id !== 'string' ||
    typeof config !== 'string' ||
    typeof outputType !== 'string' ||
    typeof rule !== 'string'
  ) {
    throw new Error('an example has an id, a config, an output type and a rule');
  }
  if (mode === 'graph' && typeof graph === 'string') {
    return { id, mode, config, outputType, rule, graph };
  }
  if (
    mode === 'files' &&
    (language === 'typescript' || language === 'python') &&
    isStringRecord(files)
  ) {
    return { id, mode, config, outputType, rule, language, files };
  }
  throw new Error(`example ${id}: a graph, or a language and files`);
}

/** The language a source-files run uses: the visitor's pick, or inferred from the paths. */
export function languageFor(choice: string, paths: Iterable<string>): Language {
  if (choice === 'typescript' || choice === 'python') {
    return choice;
  }
  return inferLanguage(paths);
}

/** One playground component, live. */
export class Playground {
  private files: FileSet | undefined;

  constructor(
    private readonly api: RbWasm,
    private readonly view: PlaygroundView,
    private readonly clock: () => number,
    private readonly examples: ReadonlyMap<string, Example> = new Map(),
  ) {}

  /** Wires the controls and enables them. */
  ready(): void {
    const { view } = this;
    view.run.addEventListener('click', () => {
      this.run();
    });
    view.explain.addEventListener('click', () => {
      this.explain();
    });
    view.mode.addEventListener('change', () => {
      this.showMode();
    });
    for (const { id, button } of view.examples) {
      button.addEventListener('click', () => {
        this.load(id);
      });
      button.disabled = false;
    }
    for (const picker of view.pickers) {
      picker.disabled = false;
    }
    this.showMode();
    view.run.disabled = false;
    view.explain.disabled = false;
    view.data.state = 'ready';
    view.status.textContent = `Ready: rulebearing ${this.api.version()}.`;
    // A cookbook page opens with its own repository and rule loaded.
    const preload = view.data.preload;
    if (preload !== undefined && preload !== '') {
      this.load(preload);
    }
  }

  /** Shows the panel of the mode picked, hiding the other. */
  showMode(): void {
    const files = this.view.mode.value === 'files';
    this.view.filesPanel.hidden = !files;
    this.view.graphPanel.hidden = files;
  }

  /** The files a run in the source-files mode extracts. */
  useFiles(set: FileSet): void {
    this.files = set;
    let note = describeFiles(set);
    const config = configIn(set.files);
    if (config !== undefined) {
      this.view.config.value = set.files.get(config) ?? '';
      note += ` The configuration is ${config} from the folder.`;
    }
    this.view.filesNote.textContent = note;
  }

  /** Shows that the files could not be read. */
  filesFailed(error: unknown): void {
    this.files = undefined;
    this.view.filesNote.textContent = `The files could not be read: ${errorText(error)}`;
  }

  /** Loads an embedded example into the inputs. */
  load(id: string): void {
    const example = this.examples.get(id);
    if (example === undefined) {
      this.fail(`there is no example ${id}`);
      return;
    }
    const { view } = this;
    view.mode.value = example.mode;
    view.config.value = example.config;
    view.outputType.value = example.outputType;
    view.rule.value = example.rule;
    if (example.graph !== undefined) {
      view.graph.value = example.graph;
    }
    if (example.language !== undefined) {
      view.language.value = example.language;
    }
    if (example.files !== undefined) {
      const files = new Map(Object.entries(example.files).sort(([a], [b]) => (a < b ? -1 : 1)));
      const bytes = [...files.values()].reduce((n, t) => n + new TextEncoder().encode(t).length, 0);
      this.files = { files, skipped: [], bytes };
      view.filesNote.textContent = `The ${id} example: ${describeFiles(this.files)}`;
    }
    this.showMode();
    view.output.textContent = '';
    delete view.data.elapsed;
    view.data.state = 'ready';
    view.status.textContent = `Loaded the ${id} example.`;
  }

  /** The cruise of the chosen files: the export the language picks, and its name. */
  private cruise(outputType: string): { action: string; text: string } {
    if (this.files === undefined || this.files.files.size === 0) {
      throw new Error('choose a folder or files first, or load an example');
    }
    const { files } = this.files;
    const language = languageFor(this.view.language.value, files.keys());
    return language === 'typescript'
      ? { action: 'cruise_ts', text: this.api.cruiseTs(files, this.view.config.value, outputType) }
      : { action: 'cruise_py', text: this.api.cruisePy(files, this.view.config.value, outputType) };
  }

  /** Runs `evaluate`, or the cruise of the chosen files, and shows the report. */
  run(): void {
    this.timed(() => {
      const { view } = this;
      if (view.mode.value === 'files') {
        return this.cruise(view.outputType.value);
      }
      return {
        action: 'evaluate',
        text: this.api.evaluate(view.config.value, view.graph.value, view.outputType.value),
      };
    });
  }

  /**
   * Runs `explain` over the current cruise result: the pasted one, or in the source-files mode the
   * `json` cruise of the chosen files.
   */
  explain(): void {
    this.timed(() => {
      const { view } = this;
      const rule = view.rule.value.trim();
      if (rule === '') {
        throw new Error('name the rule to explain');
      }
      const cruised = view.mode.value === 'files' ? this.cruise('json').text : view.graph.value;
      return { action: 'explain', text: this.api.explain(view.config.value, cruised, rule) };
    });
  }

  private timed(work: () => { action: string; text: string }): void {
    const { view } = this;
    const started = this.clock();
    try {
      const { action, text } = work();
      const elapsed = this.clock() - started;
      view.output.textContent = text;
      view.data.state = 'ok';
      view.data.action = action;
      view.data.elapsed = elapsed.toFixed(1);
      view.status.textContent = `${action} ran in ${elapsed.toFixed(1)} ms.`;
    } catch (error) {
      this.fail(errorText(error));
    }
  }

  private fail(reason: string): void {
    const { view } = this;
    view.output.textContent = '';
    view.data.state = 'error';
    delete view.data.elapsed;
    delete view.data.action;
    view.status.textContent = reason;
  }
}

/** Shows that the module could not be loaded: every control stays disabled. */
export function failed(view: PlaygroundView, error: unknown): void {
  view.run.disabled = true;
  view.explain.disabled = true;
  view.data.state = 'error';
  view.status.textContent = `The rulebearing module did not load: ${errorText(error)}`;
}
