// The site's heading anchors equal the documentation link check's (xtask/src/doclinks.rs): the
// same examples as its `slug_matches_github_rules` test, and its two properties over generated
// headings. Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3.
import { describe, expect, it } from 'vitest';
import { slug } from '../src/slug.js';

describe('slug', () => {
  it.each([
    ['## The rule engine', '-the-rule-engine'],
    ['The rule engine', 'the-rule-engine'],
    ['1. Architect section (for the board)', '1-architect-section-for-the-board'],
    ['`rb-model` and friends', 'rb-model-and-friends'],
    ['  Trailing space  ', 'trailing-space'],
    ['FR-CORE-01', 'fr-core-01'],
    ['Wave 4A: docs site, playground', 'wave-4a-docs-site-playground'],
    ['§ 1.6 Interfaces', '-16-interfaces'],
    ['snake_case and Ünïcode', 'snake_case-and-ünïcode'],
  ])('%s -> %s', (heading, expected) => {
    expect(slug(heading)).toBe(expected);
  });

  // Seeded generation, so a failure reproduces: the link check's proptest invariants.
  const alphabet = 'aZ09 -_.,:;`()[]§#*/éÜ\t';
  let seed = 7;
  const next = (): number => {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed;
  };
  const headings = Array.from({ length: 500 }, () =>
    Array.from({ length: next() % 80 }, () => alphabet[next() % alphabet.length]).join(''),
  );

  it('only emits anchor characters', () => {
    for (const heading of headings) {
      expect(slug(heading)).toMatch(/^[\p{Alphabetic}\p{N}_-]*$/u);
    }
  });

  it('is idempotent', () => {
    for (const heading of headings) {
      expect(slug(slug(heading))).toBe(slug(heading));
    }
  });
});
