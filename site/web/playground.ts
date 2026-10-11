// The page script of the playground component: finds each component on the page, loads the
// `rb-wasm` module once from the URL the component names, and hands both to playground-core.
//
// Architecture: docs/architecture.md#security-posture. Decisions: docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.md,
// docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Requirement: FR-REACH-01.

import { asRbWasm, failed, ready } from './playground-core.js';
import type { PlaygroundView } from './playground-core.js';

function element<T extends Element>(
  root: Element,
  selector: string,
  kind: abstract new () => T,
): T {
  const found = root.querySelector(selector);
  if (!(found instanceof kind)) {
    throw new Error(`the playground component has no ${selector}`);
  }
  return found;
}

function viewOf(section: HTMLElement): PlaygroundView {
  return {
    config: element(section, 'textarea[name="config"]', HTMLTextAreaElement),
    graph: element(section, 'textarea[name="graph"]', HTMLTextAreaElement),
    outputType: element(section, 'select[name="output-type"]', HTMLSelectElement),
    run: element(section, '[data-run]', HTMLButtonElement),
    output: element(section, '[data-output]', HTMLElement),
    status: element(section, '[data-status]', HTMLElement),
    data: section.dataset,
  };
}

const sections = [...document.querySelectorAll<HTMLElement>('[data-playground]')];
const views = sections.map(viewOf);
const moduleUrl = sections[0]?.dataset.module;
if (moduleUrl !== undefined) {
  try {
    const loaded = asRbWasm(await import(new URL(moduleUrl, document.baseURI).href));
    await loaded.init();
    const clock = (): number => performance.now();
    for (const view of views) {
      ready(loaded.api, view, clock);
    }
  } catch (error) {
    for (const view of views) {
      failed(view, error);
    }
  }
}
