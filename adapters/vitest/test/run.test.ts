// Running the binary: which program starts, with which arguments, and what a run without a
// result reports. Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  CruiseError,
  cruise,
  cruiseArguments,
  resolveCommand,
  resolvePackageJson,
} from '../src/run.js';
import { copyFixture, localBinary } from './helpers.js';

let cleanup: (() => void) | undefined;
afterEach(() => {
  cleanup?.();
  cleanup = undefined;
});

describe('resolveCommand', () => {
  it('prefers the option, then RULEBEARING_BINARY', () => {
    expect(resolveCommand('/opt/rb', { RULEBEARING_BINARY: '/env/rb' })).toEqual(['/opt/rb']);
    expect(resolveCommand(undefined, { RULEBEARING_BINARY: '/env/rb' })).toEqual(['/env/rb']);
    expect(resolveCommand('', { RULEBEARING_BINARY: '/env/rb' })).toEqual(['/env/rb']);
  });

  it("otherwise starts the rulebearing package's launcher with this Node", () => {
    const command = resolveCommand(
      undefined,
      {},
      (name) => `/project/node_modules/${name}/package.json`,
    );
    expect(command).toEqual([
      process.execPath,
      join('/project/node_modules/rulebearing', 'bin', 'rulebearing.js'),
    ]);
  });

  it('says how to proceed when the package cannot be resolved', () => {
    expect(() =>
      resolveCommand(undefined, { RULEBEARING_BINARY: '' }, () => {
        throw new Error('MODULE_NOT_FOUND');
      }),
    ).toThrow(/install rulebearing, or set RULEBEARING_BINARY/);
  });

  it('resolves package manifests as Node does', () => {
    expect(resolvePackageJson('vitest')).toMatch(/node_modules[\\/]vitest[\\/]package\.json$/);
  });
});

describe('cruiseArguments', () => {
  it('asks for JSON without progress, then the options', () => {
    expect(cruiseArguments({})).toEqual(['cruise', '--output-type', 'json', '--no-progress']);
    expect(cruiseArguments({ config: 'c.yaml', graph: 'g.json', args: ['src'] })).toEqual([
      'cruise',
      '--output-type',
      'json',
      '--no-progress',
      '--config',
      'c.yaml',
      '--graph',
      'g.json',
      'src',
    ]);
  });
});

describe('cruise', () => {
  it('reads the result of a run that exits 2', () => {
    const fixture = copyFixture();
    cleanup = fixture.remove;
    const result = cruise({ binary: localBinary(), cwd: fixture.dir });
    expect(result).toHaveProperty('summary.vacuousRules', [
      { name: 'nothing-matches', side: 'from' },
    ]);
  });

  it('reads a graph document with --graph and gives the same summary', () => {
    const fixture = copyFixture();
    cleanup = fixture.remove;
    const binary = localBinary();
    const extracted = cruise({
      binary,
      cwd: fixture.dir,
      config: 'rulebearing.yaml',
      args: ['py'],
    });
    writeFileSync(join(fixture.dir, 'graph.json'), JSON.stringify(extracted));
    const read = cruise({ binary, cwd: fixture.dir, graph: 'graph.json' });
    expect((read as { summary: { violations: unknown } }).summary.violations).toEqual(
      (extracted as { summary: { violations: unknown } }).summary.violations,
    );
  });

  it('names a binary that cannot start', () => {
    expect(() => cruise({ binary: join('/no', 'such', 'rulebearing') })).toThrow(CruiseError);
    expect(() => cruise({ binary: join('/no', 'such', 'rulebearing') })).toThrow(/could not start/);
  });

  it('shows the exit code and stderr of a run without a result', () => {
    const fixture = copyFixture();
    cleanup = fixture.remove;
    expect(() =>
      cruise({ binary: localBinary(), cwd: fixture.dir, config: 'missing.yaml' }),
    ).toThrow(/exited \d+ without a result:\n.*missing\.yaml/s);
    // Node given `cruise` as a script name exits 1 and writes nothing to standard output.
    expect(() => cruise({ binary: process.execPath, cwd: fixture.dir })).toThrow(
      /exited 1 without a result/,
    );
  });
});
