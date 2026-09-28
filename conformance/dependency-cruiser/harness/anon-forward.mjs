// The replacement for `#report/anon/anonymize-path-element.mjs` and
// `#report/anon/anonymize-path.mjs` in conformance gate 1 layer 3: each call is answered by
// `rulebearing validate`, with the state upstream's module keeps between calls held here.
//
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 6. Protocol: the
// header of shim.mjs, and of crates/rb-report/src/conformance.rs for the stateful calls.
//
// Upstream's anonymiser takes words off the front of the array it is given, and remembers every
// part it replaced in a module-level cache until `clearCache()`. The binary answers one call at a
// time, so each request carries the cache after upstream's own arguments, and the reply
// `{ value, wordList, cache }` says what the call returned, which words are left and what the
// cache holds now. The words left are written back into the caller's array, as `shift()` would
// have left it. Nothing here decides a replacement; that is the binary's.
import { call } from './shim.mjs';

const cache = new Map();

function stateful(module, name, args, wordList) {
    const reply = call({ module, export: name, path: [], calls: [[...args, [...cache]]] });
    if (Array.isArray(wordList)) {
        wordList.splice(0, wordList.length, ...reply.wordList);
    }
    cache.clear();
    for (const [part, word] of reply.cache) {
        cache.set(part, word);
    }
    return reply.value;
}

export function anonymizePathElement(pathElement, wordList, whiteListRE, cached) {
    return stateful(
        '#report/anon/anonymize-path-element.mjs',
        'anonymizePathElement',
        [pathElement, wordList, whiteListRE, cached],
        wordList,
    );
}

export function anonymizePath(path, wordList, whiteListRE) {
    return stateful(
        '#report/anon/anonymize-path.mjs',
        'anonymizePath',
        [path, wordList, whiteListRE],
        wordList,
    );
}

export function clearCache() {
    cache.clear();
}
