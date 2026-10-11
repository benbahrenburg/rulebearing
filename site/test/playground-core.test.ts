// The playground component's logic, driven with plain objects in place of the page's elements: the
// module check, the two modes, the examples, explain, and the reasons shown verbatim.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3; the exports in § 1.6.
// Decision: docs/adr/0073-extractors-read-files-through-rb-model.md.
import { describe, expect, it } from 'vitest';
import {
  Playground,
  asExample,
  asRbWasm,
  errorText,
  failed,
  languageFor,
} from '../web/playground-core.js';
import type { Example, PlaygroundView, RbWasm } from '../web/playground-core.js';

interface Clickable {
  disabled: boolean;
  click: () => void;
  addEventListener: (type: 'click', fn: () => void) => void;
}

function button(): Clickable {
  let listener: (() => void) | undefined;
  return {
    disabled: true,
    addEventListener: (_type, fn) => {
      listener = fn;
    },
    click: () => listener?.(),
  };
}

function choice(value: string): {
  value: string;
  change: () => void;
  addEventListener: (type: 'change', fn: () => void) => void;
} {
  let listener: (() => void) | undefined;
  return {
    value,
    addEventListener: (_type, fn) => {
      listener = fn;
    },
    change: () => listener?.(),
  };
}

function view(): PlaygroundView & {
  run: Clickable;
  explain: Clickable;
  mode: ReturnType<typeof choice>;
  examples: { id: string; button: Clickable }[];
  pickers: { disabled: boolean }[];
} {
  return {
    config: { value: 'forbidden: []' },
    graph: { value: '{}' },
    outputType: { value: 'json' },
    mode: choice('graph'),
    language: { value: 'auto' },
    rule: { value: 'no-circular' },
    run: button(),
    explain: button(),
    examples: [
      { id: 'ts', button: button() },
      { id: 'gone', button: button() },
    ],
    pickers: [{ disabled: true }, { disabled: true }],
    graphPanel: { hidden: false },
    filesPanel: { hidden: true },
    filesNote: { textContent: null },
    output: { textContent: null },
    status: { textContent: null },
    data: {},
  };
}

const show = (files: ReadonlyMap<string, string>): string =>
  [...files].map(([p, t]) => `${p}=${t}`).join(',');

const api: RbWasm = {
  evaluate: (config, graph, type) => `evaluate:${type}:${config}:${graph}`,
  cruiseTs: (files, config, type) => `ts:${type}:${config}:${show(files)}`,
  cruisePy: (files, config, type) => `py:${type}:${config}:${show(files)}`,
  explain: (config, graph, rule) => `explain:${rule}:${config}:${graph}`,
  version: () => '0.1.3',
};

const tsExample: Example = {
  id: 'ts',
  mode: 'files',
  config: 'ts-config',
  outputType: 'err',
  rule: 'no-unresolvable',
  language: 'typescript',
  files: { 'src/b.ts': 'b', 'src/a.ts': 'a' },
};

/** A clock that advances 12.5 ms per reading. */
function ticking(): () => number {
  let now = 100;
  return () => (now += 12.5);
}

function live(v = view(), examples = new Map([['ts', tsExample]])): Playground {
  const playground = new Playground(api, v, ticking(), examples);
  playground.ready();
  return playground;
}

describe('asRbWasm', () => {
  it('takes the module wasm-bindgen writes, with all five exports', async () => {
    let initialised = false;
    const loaded = asRbWasm({
      default: () => {
        initialised = true;
        return Promise.resolve({});
      },
      evaluate: (a: string, b: string, c: string) => a + b + c,
      cruise_ts: (f: Map<string, string>, c: string, t: string) => `${String(f.size)}${c}${t}`,
      cruise_py: (f: Map<string, string>, c: string, t: string) => `py${String(f.size)}${c}${t}`,
      explain: (a: string, b: string, r: string) => `${r}${a}${b}`,
      version: () => '1.0.0',
    });
    await loaded.init();
    expect(initialised).toBe(true);
    const files = new Map([['a.ts', '']]);
    expect(loaded.api.evaluate('a', 'b', 'c')).toBe('abc');
    expect(loaded.api.cruiseTs(files, 'c', 'json')).toBe('1cjson');
    expect(loaded.api.cruisePy(files, 'c', 'err')).toBe('py1cerr');
    expect(loaded.api.explain('c', 'g', 'r')).toBe('rcg');
    expect(loaded.api.version()).toBe('1.0.0');
  });

  it('refuses anything else, naming the exports it needs', () => {
    expect(() => asRbWasm(undefined)).toThrow(/did not load/u);
    expect(() => asRbWasm({ default: () => 0, evaluate: () => '', version: () => '' })).toThrow(
      'the playground module does not export evaluate, cruise_ts, cruise_py, explain, version',
    );
  });
});

describe('asExample', () => {
  it('takes an example of either mode', () => {
    const graph = {
      id: 'g',
      mode: 'graph',
      config: 'c',
      outputType: 'err',
      rule: 'r',
      graph: '{}',
    };
    expect(asExample(graph)).toEqual(graph);
    expect(asExample(tsExample)).toEqual(tsExample);
  });

  it('refuses one that is incomplete', () => {
    expect(() => asExample(null)).toThrow('an example is an object');
    expect(() => asExample({ id: 'x' })).toThrow(/has an id, a config/u);
    const base = { id: 'x', config: 'c', outputType: 'err', rule: 'r' };
    expect(() => asExample({ ...base, mode: 'graph' })).toThrow(
      'example x: a graph, or a language and files',
    );
    expect(() =>
      asExample({ ...base, mode: 'files', language: 'rust', files: { 'a.rs': '' } }),
    ).toThrow(/example x/u);
    expect(() =>
      asExample({ ...base, mode: 'files', language: 'python', files: { 'a.py': 1 } }),
    ).toThrow(/example x/u);
    expect(() => asExample({ ...base, mode: 'files', language: 'python', files: null })).toThrow(
      /example x/u,
    );
  });
});

describe('languageFor', () => {
  it('takes the pick, or infers from the paths', () => {
    expect(languageFor('python', ['a.ts'])).toBe('python');
    expect(languageFor('typescript', ['a.py'])).toBe('typescript');
    expect(languageFor('auto', ['a.py'])).toBe('python');
    expect(() => languageFor('auto', ['README.md'])).toThrow(/pick the language/u);
  });
});

describe('the component', () => {
  it('becomes ready: every control enabled, the mode shown', () => {
    const v = view();
    live(v);
    expect(v.run.disabled).toBe(false);
    expect(v.explain.disabled).toBe(false);
    expect(v.examples.map((e) => e.button.disabled)).toEqual([false, false]);
    expect(v.pickers.map((p) => p.disabled)).toEqual([false, false]);
    expect(v.graphPanel.hidden).toBe(false);
    expect(v.filesPanel.hidden).toBe(true);
    expect(v.data.state).toBe('ready');
    expect(v.status.textContent).toBe('Ready: rulebearing 0.1.3.');
  });

  it('evaluates a pasted cruise result and times the run', () => {
    const v = view();
    live(v);
    v.run.click();
    expect(v.output.textContent).toBe('evaluate:json:forbidden: []:{}');
    expect(v.data).toEqual({ state: 'ok', action: 'evaluate', elapsed: '12.5' });
    expect(v.status.textContent).toBe('evaluate ran in 12.5 ms.');
  });

  it('explains a rule over the pasted cruise result', () => {
    const v = view();
    live(v);
    v.rule.value = '  no-circular ';
    v.explain.click();
    expect(v.output.textContent).toBe('explain:no-circular:forbidden: []:{}');
    expect(v.data.action).toBe('explain');
  });

  it('switches panels with the mode', () => {
    const v = view();
    live(v);
    v.mode.value = 'files';
    v.mode.change();
    expect(v.graphPanel.hidden).toBe(true);
    expect(v.filesPanel.hidden).toBe(false);
  });

  it('asks for files before a source-files run', () => {
    const v = view();
    live(v);
    v.mode.value = 'files';
    v.run.click();
    expect(v.data).toEqual({ state: 'error' });
    expect(v.status.textContent).toBe('choose a folder or files first, or load an example');
    expect(v.output.textContent).toBe('');
  });

  it('cruises chosen files with the language inferred or picked', () => {
    const v = view();
    const playground = live(v);
    v.mode.value = 'files';
    playground.useFiles({
      files: new Map([
        ['src/a.py', 'import b'],
        ['src/b.py', ''],
      ]),
      skipped: [{ path: 'logo.png', reason: 'binary' }],
      bytes: 8,
    });
    expect(v.filesNote.textContent).toBe(
      '2 file(s) read, 0.0 KiB, kept in this browser. Skipped 1: logo.png (binary).',
    );
    v.run.click();
    expect(v.output.textContent).toBe('py:json:forbidden: []:src/a.py=import b,src/b.py=');
    expect(v.data.action).toBe('cruise_py');
    v.language.value = 'typescript';
    v.run.click();
    expect(v.data.action).toBe('cruise_ts');
  });

  it("loads a chosen folder's configuration file into the configuration box", () => {
    const v = view();
    const playground = live(v);
    playground.useFiles({
      files: new Map([
        ['rulebearing.yaml', 'mine'],
        ['src/a.ts', ''],
      ]),
      skipped: [],
      bytes: 4,
    });
    expect(v.config.value).toBe('mine');
    expect(v.filesNote.textContent).toBe(
      '2 file(s) read, 0.0 KiB, kept in this browser. The configuration is rulebearing.yaml from the folder.',
    );
  });

  it('explains over the json cruise of the chosen files', () => {
    const v = view();
    const playground = live(v);
    v.mode.value = 'files';
    v.outputType.value = 'err-long';
    playground.useFiles({ files: new Map([['a.ts', 'x']]), skipped: [], bytes: 1 });
    v.explain.click();
    expect(v.output.textContent).toBe(
      'explain:no-circular:forbidden: []:ts:json:forbidden: []:a.ts=x',
    );
  });

  it('refuses to explain without a rule', () => {
    const v = view();
    live(v);
    v.rule.value = ' ';
    v.explain.click();
    expect(v.status.textContent).toBe('name the rule to explain');
  });

  it('shows that files could not be read, and then asks for files', () => {
    const v = view();
    const playground = live(v);
    playground.useFiles({ files: new Map([['a.ts', '']]), skipped: [], bytes: 0 });
    playground.filesFailed(new Error('permission denied'));
    expect(v.filesNote.textContent).toBe('The files could not be read: permission denied');
    v.mode.value = 'files';
    v.run.click();
    expect(v.data.state).toBe('error');
  });

  it('loads an example from its button: the mode, inputs and files', () => {
    const v = view();
    live(v);
    v.output.textContent = 'old';
    v.data.elapsed = '9';
    v.examples[0]?.button.click();
    expect(v.mode.value).toBe('files');
    expect(v.config.value).toBe('ts-config');
    expect(v.outputType.value).toBe('err');
    expect(v.rule.value).toBe('no-unresolvable');
    expect(v.language.value).toBe('typescript');
    expect(v.filesPanel.hidden).toBe(false);
    expect(v.output.textContent).toBe('');
    expect(v.data).toEqual({ state: 'ready' });
    expect(v.status.textContent).toBe('Loaded the ts example.');
    expect(v.filesNote.textContent).toBe(
      'The ts example: 2 file(s) read, 0.0 KiB, kept in this browser.',
    );
    v.run.click();
    expect(v.output.textContent).toBe('ts:err:ts-config:src/a.ts=a,src/b.ts=b');
  });

  it('loads a cruise-result example', () => {
    const v = view();
    const graph: Example = {
      id: 'ts',
      mode: 'graph',
      config: 'c',
      outputType: 'text',
      rule: 'r',
      graph: '{"modules":[]}',
    };
    live(v, new Map([['ts', graph]]));
    v.mode.value = 'files';
    v.examples[0]?.button.click();
    expect(v.mode.value).toBe('graph');
    expect(v.graph.value).toBe('{"modules":[]}');
    expect(v.graphPanel.hidden).toBe(false);
  });

  it('names an example the page does not hold', () => {
    const v = view();
    live(v);
    v.examples[1]?.button.click();
    expect(v.data.state).toBe('error');
    expect(v.status.textContent).toBe('there is no example gone');
  });

  it("shows the module's reason verbatim", () => {
    const v = view();
    v.data.elapsed = '3';
    const reason =
      "the files could not be extracted: Yarn Plug'n'Play cannot be used: the playground has no disk";
    const throwing: RbWasm = {
      ...api,
      evaluate: () => {
        throw new Error(reason);
      },
    };
    new Playground(throwing, v, () => 0).run();
    expect(v.output.textContent).toBe('');
    expect(v.data).toEqual({ state: 'error' });
    expect(v.status.textContent).toBe(reason);
  });

  it('shows that the module did not load, every control disabled', () => {
    const v = view();
    failed(v, new Error('404'));
    expect(v.run.disabled).toBe(true);
    expect(v.explain.disabled).toBe(true);
    expect(v.data.state).toBe('error');
    expect(v.status.textContent).toBe('The rulebearing module did not load: 404');
  });

  it('reads any thrown value as text', () => {
    expect(errorText('s')).toBe('s');
    expect(errorText(new Error('e'))).toBe('e');
    expect(errorText(42)).toBe('42');
  });
});
