// The playground component's logic, driven with plain objects in place of the page's elements.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3; the exports in § 1.6.
import { describe, expect, it } from 'vitest';
import { asRbWasm, errorText, failed, ready, runOnce } from '../web/playground-core.js';
import type { PlaygroundView, RbWasm } from '../web/playground-core.js';

function view(): PlaygroundView & { click: () => void } {
  let listener: (() => void) | undefined;
  return {
    config: { value: 'forbidden: []' },
    graph: { value: '{}' },
    outputType: { value: 'json' },
    run: {
      disabled: true,
      addEventListener: (_type, fn) => {
        listener = fn;
      },
    },
    output: { textContent: null },
    status: { textContent: null },
    data: {},
    click: () => listener?.(),
  };
}

const api: RbWasm = {
  evaluate: (config, graph, type) => `${type}:${config}:${graph}`,
  version: () => '0.1.3',
};

describe('asRbWasm', () => {
  it('takes the module wasm-bindgen writes', async () => {
    let initialised = false;
    const loaded = asRbWasm({
      default: () => {
        initialised = true;
        return Promise.resolve({});
      },
      evaluate: (a: string, b: string, c: string) => a + b + c,
      version: () => '1.0.0',
    });
    await loaded.init();
    expect(initialised).toBe(true);
    expect(loaded.api.evaluate('a', 'b', 'c')).toBe('abc');
    expect(loaded.api.version()).toBe('1.0.0');
  });

  it('refuses anything else', () => {
    expect(() => asRbWasm(undefined)).toThrow(/did not load/u);
    expect(() => asRbWasm({ default: () => 0, evaluate: 1 })).toThrow(/does not export/u);
  });
});

describe('the component', () => {
  it('becomes ready, then evaluates and times a run', () => {
    const v = view();
    let now = 100;
    ready(api, v, () => (now += 12.5));
    expect(v.run.disabled).toBe(false);
    expect(v.data.state).toBe('ready');
    expect(v.status.textContent).toBe('Ready: rulebearing 0.1.3.');
    v.click();
    expect(v.output.textContent).toBe('json:forbidden: []:{}');
    expect(v.data).toEqual({ state: 'ok', elapsed: '12.5' });
    expect(v.status.textContent).toBe('Evaluated in 12.5 ms.');
  });

  it('shows the reason a run failed', () => {
    const v = view();
    v.data.elapsed = '3';
    const throwing: RbWasm = {
      ...api,
      evaluate: () => {
        throw new Error('the configuration did not load: bad');
      },
    };
    runOnce(throwing, v, () => 0);
    expect(v.output.textContent).toBe('');
    expect(v.data).toEqual({ state: 'error' });
    expect(v.status.textContent).toBe('the configuration did not load: bad');
  });

  it('shows that the module did not load', () => {
    const v = view();
    failed(v, new Error('404'));
    expect(v.run.disabled).toBe(true);
    expect(v.data.state).toBe('error');
    expect(v.status.textContent).toBe('The rulebearing module did not load: 404');
  });

  it('reads any thrown value as text', () => {
    expect(errorText('s')).toBe('s');
    expect(errorText(new Error('e'))).toBe('e');
    expect(errorText(42)).toBe('42');
  });
});
