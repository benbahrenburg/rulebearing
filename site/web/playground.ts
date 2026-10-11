// The page script of the playground component: finds each component on the page, reads the
// examples embedded in it, loads the `rb-wasm` module once from the URL the component names, and
// hands both to playground-core. The folder and file pickers and the drop zone read the chosen
// files through playground-files; the files stay in this page.
//
// Architecture: docs/architecture.md#security-posture. Decisions: docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.md,
// docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md,
// docs/adr/0073-extractors-read-files-through-rb-model.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Requirement: FR-REACH-01.

import { Playground, asExample, asRbWasm, failed } from './playground-core.js';
import type { Example, PlaygroundView } from './playground-core.js';
import { readDropped, readPicked } from './playground-files.js';
import type { DroppedEntry, FileSet } from './playground-files.js';

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

/** The DOM parts of one component the page script wires itself: the pickers and the drop zone. */
interface Pickers {
  readonly folder: HTMLInputElement;
  readonly files: HTMLInputElement;
  readonly drop: HTMLElement;
}

function viewOf(section: HTMLElement, pickers: Pickers): PlaygroundView {
  const examples = [...section.querySelectorAll<HTMLButtonElement>('button[data-example]')].map(
    (button) => ({ id: button.dataset.example ?? '', button }),
  );
  return {
    config: element(section, 'textarea[name="config"]', HTMLTextAreaElement),
    graph: element(section, 'textarea[name="graph"]', HTMLTextAreaElement),
    outputType: element(section, 'select[name="output-type"]', HTMLSelectElement),
    mode: element(section, 'select[name="mode"]', HTMLSelectElement),
    language: element(section, 'select[name="language"]', HTMLSelectElement),
    rule: element(section, 'input[name="rule"]', HTMLInputElement),
    run: element(section, '[data-run]', HTMLButtonElement),
    explain: element(section, '[data-explain]', HTMLButtonElement),
    examples,
    pickers: [pickers.folder, pickers.files],
    graphPanel: element(section, '[data-panel="graph"]', HTMLElement),
    filesPanel: element(section, '[data-panel="files"]', HTMLElement),
    filesNote: element(section, '[data-files-note]', HTMLElement),
    output: element(section, '[data-output]', HTMLElement),
    status: element(section, '[data-status]', HTMLElement),
    data: section.dataset,
  };
}

function examplesOf(section: HTMLElement): Map<string, Example> {
  const blocks = section.querySelectorAll<HTMLScriptElement>(
    'script[type="application/json"][data-example-data]',
  );
  const examples = [...blocks].map((block) => asExample(JSON.parse(block.text)));
  return new Map(examples.map((example) => [example.id, example]));
}

function picked(input: HTMLInputElement): File[] {
  return [...(input.files ?? [])];
}

/** The entries of a drop: folders as folders, so their files are walked. */
function dropped(event: DragEvent): DroppedEntry[] {
  const items = [...(event.dataTransfer?.items ?? [])];
  return items
    .map((item) => item.webkitGetAsEntry())
    .filter((entry): entry is FileSystemEntry => entry !== null);
}

function wire(playground: Playground, pickers: Pickers): void {
  const use = (reading: Promise<FileSet>): void => {
    reading.then(
      (set) => {
        playground.useFiles(set);
      },
      (error: unknown) => {
        playground.filesFailed(error);
      },
    );
  };
  pickers.folder.addEventListener('change', () => {
    use(readPicked(picked(pickers.folder)));
  });
  pickers.files.addEventListener('change', () => {
    use(readPicked(picked(pickers.files)));
  });
  pickers.drop.addEventListener('drop', (event) => {
    event.preventDefault();
    pickers.drop.classList.remove('playground-drop-over');
    use(readDropped(dropped(event)));
  });
}

/** Keeps the browser from opening a dropped file in the tab, whether or not the module loaded. */
function holdDrops(drop: HTMLElement): void {
  drop.addEventListener('dragover', (event) => {
    event.preventDefault();
    drop.classList.add('playground-drop-over');
  });
  drop.addEventListener('dragleave', () => {
    drop.classList.remove('playground-drop-over');
  });
  drop.addEventListener('drop', (event) => {
    event.preventDefault();
  });
}

const components = [...document.querySelectorAll<HTMLElement>('[data-playground]')].map(
  (section) => {
    const pickers: Pickers = {
      folder: element(section, 'input[name="folder"]', HTMLInputElement),
      files: element(section, 'input[name="files"]', HTMLInputElement),
      drop: element(section, '[data-drop]', HTMLElement),
    };
    holdDrops(pickers.drop);
    return { section, pickers, view: viewOf(section, pickers) };
  },
);
const moduleUrl = components[0]?.section.dataset.module;
if (moduleUrl !== undefined) {
  try {
    const loaded = asRbWasm(await import(new URL(moduleUrl, document.baseURI).href));
    await loaded.init();
    const clock = (): number => performance.now();
    for (const { section, pickers, view } of components) {
      const playground = new Playground(loaded.api, view, clock, examplesOf(section));
      playground.ready();
      wire(playground, pickers);
    }
  } catch (error) {
    for (const { view } of components) {
      failed(view, error);
    }
  }
}
