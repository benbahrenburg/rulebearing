// The licence check for the npm packages the docs site is built and tested with: every package in
// their dependency trees, as package-lock.json records them, must carry a licence on the same
// allow-list `cargo deny` holds the Rust crates to (deny.toml), or a named exception.
//
// Architecture: docs/architecture.md#security-posture. Decisions: docs/adr/0019-mit-licence.md,
// docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, § 1.7 (ADR-0019 row: "the docs site generator and
// wasm toolchain pass `cargo deny` and the npm licence check") and § 1.11.

/** The npm packages the site's build and tests use directly; their trees are checked. */
export const SITE_PACKAGES: readonly string[] = ['markdown-it', 'playwright'];

/**
 * Packages allowed with a licence outside the list, each with its reason, as deny.toml scopes its
 * exceptions to one crate.
 */
export const EXCEPTIONS: Readonly<Record<string, string>> = {
  argparse:
    "markdown-it's command-line parser, under the permissive Python-2.0 licence; the site imports markdown-it's library entry, which never loads it",
};

/** The `allow` list of deny.toml's `[licenses]` table. */
export function allowedLicences(denyToml: string): string[] {
  const section = /\[licenses\][^[]*?allow\s*=\s*\[([^\]]*)\]/su.exec(denyToml);
  if (section === null) {
    throw new Error('deny.toml has no [licenses] allow list');
  }
  return [...(section[1] ?? '').matchAll(/"([^"]+)"/gu)].map((m) => m[1] ?? '');
}

/**
 * Whether an SPDX expression is satisfied by `allowed`: an `OR` needs one side, an `AND` both.
 * Parentheses around the whole expression are dropped; nested groups are not used by any package
 * this check reads and are refused.
 */
export function satisfies(expression: string, allowed: readonly string[]): boolean {
  const text = expression.trim().replace(/^\((.*)\)$/su, '$1');
  if (text.includes('(')) {
    return false;
  }
  return text
    .split(/\s+OR\s+/u)
    .some((alternative) =>
      alternative.split(/\s+AND\s+/u).every((id) => allowed.includes(id.trim())),
    );
}

interface LockPackage {
  readonly license?: string;
  readonly dependencies?: Readonly<Record<string, string>>;
  readonly optionalDependencies?: Readonly<Record<string, string>>;
}

/** One package whose licence is not allowed. */
export interface LicenceFinding {
  readonly name: string;
  readonly path: string;
  readonly licence: string;
}

/**
 * Walks the dependency trees of `roots` in a package-lock (v3) and returns every package whose
 * licence the allow-list does not satisfy and no exception names, sorted by path.
 */
export function checkLicences(
  lockText: string,
  allowed: readonly string[],
  roots: readonly string[] = SITE_PACKAGES,
): LicenceFinding[] {
  const lock = JSON.parse(lockText) as { packages?: Record<string, LockPackage> };
  const packages = lock.packages ?? {};
  const seen = new Set<string>();
  const findings: LicenceFinding[] = [];
  const locate = (name: string, from: string): string | undefined => {
    let base = from;
    for (;;) {
      const candidate = base === '' ? `node_modules/${name}` : `${base}/node_modules/${name}`;
      if (candidate in packages) {
        return candidate;
      }
      if (base === '') {
        return undefined;
      }
      const at = base.lastIndexOf('/node_modules/');
      base = at === -1 ? '' : base.slice(0, at);
    }
  };
  const visit = (name: string, from: string): void => {
    const path = locate(name, from);
    if (path === undefined) {
      if (from === '') {
        findings.push({ name, path: `node_modules/${name}`, licence: 'not installed' });
      }
      return;
    }
    if (seen.has(path)) {
      return;
    }
    seen.add(path);
    const entry = packages[path] ?? {};
    const licence = entry.license ?? 'none declared';
    if (!(name in EXCEPTIONS) && !satisfies(licence, allowed)) {
      findings.push({ name, path, licence });
    }
    for (const dep of Object.keys({ ...entry.dependencies, ...entry.optionalDependencies })) {
      visit(dep, path);
    }
  };
  for (const root of roots) {
    visit(root, '');
  }
  return findings.sort((a, b) => a.path.localeCompare(b.path));
}
