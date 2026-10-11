// What the playground component does, apart from finding its elements in the page: check the
// loaded module, run `evaluate` and time it, and show the report or the reason it failed. The
// elements are passed in as plain objects, so the tests drive this without a browser.
//
// Architecture: docs/architecture.md#security-posture (the playground runs entirely client-side).
// Decisions: docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.md,
// docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3; the exports are frozen in § 1.6.
// Requirement: FR-REACH-01.

/** The `rb-wasm` exports the component calls. */
export interface RbWasm {
  evaluate(config: string, cruiseJson: string, outputType: string): string;
  version(): string;
}

/** The module `wasm-bindgen --target web` writes: an initialiser and the exports. */
export interface RbWasmModule {
  readonly init: () => Promise<void>;
  readonly api: RbWasm;
}

function isFunction(value: unknown): value is (...args: string[]) => unknown {
  return typeof value === 'function';
}

/** Checks that `loaded` is the `rb-wasm` module, and returns its initialiser and exports. */
export function asRbWasm(loaded: unknown): RbWasmModule {
  if (typeof loaded !== 'object' || loaded === null) {
    throw new Error('the playground module did not load');
  }
  const { default: init, evaluate, version } = loaded as Record<string, unknown>;
  if (!isFunction(init) || !isFunction(evaluate) || !isFunction(version)) {
    throw new Error('the playground module does not export evaluate and version');
  }
  return {
    init: async () => {
      await init();
    },
    api: {
      evaluate: (config, cruiseJson, outputType) =>
        String(evaluate(config, cruiseJson, outputType)),
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

/** The parts of one playground component the logic touches. */
export interface PlaygroundView {
  readonly config: { readonly value: string };
  readonly graph: { readonly value: string };
  readonly outputType: { readonly value: string };
  readonly run: {
    disabled: boolean;
    addEventListener(type: 'click', listener: () => void): void;
  };
  readonly output: { textContent: string | null };
  readonly status: { textContent: string | null };
  /** The component's `dataset`: `state` is `ready`, `ok` or `error`, `elapsed` the last run in ms. */
  readonly data: Record<string, string | undefined>;
}

/** Runs `evaluate` once over the view's inputs, showing the report or the reason. */
export function runOnce(api: RbWasm, view: PlaygroundView, clock: () => number): void {
  const started = clock();
  try {
    const report = api.evaluate(view.config.value, view.graph.value, view.outputType.value);
    const elapsed = clock() - started;
    view.output.textContent = report;
    view.data.state = 'ok';
    view.data.elapsed = elapsed.toFixed(1);
    view.status.textContent = `Evaluated in ${elapsed.toFixed(1)} ms.`;
  } catch (error) {
    view.output.textContent = '';
    view.data.state = 'error';
    delete view.data.elapsed;
    view.status.textContent = errorText(error);
  }
}

/** Makes the view live once the module has loaded. */
export function ready(api: RbWasm, view: PlaygroundView, clock: () => number): void {
  view.run.addEventListener('click', () => {
    runOnce(api, view, clock);
  });
  view.run.disabled = false;
  view.data.state = 'ready';
  view.status.textContent = `Ready: rulebearing ${api.version()}.`;
}

/** Shows that the module could not be loaded. */
export function failed(view: PlaygroundView, error: unknown): void {
  view.run.disabled = true;
  view.data.state = 'error';
  view.status.textContent = `The rulebearing module did not load: ${errorText(error)}`;
}
