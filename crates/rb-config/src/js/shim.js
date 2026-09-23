// The configuration sandbox's module system, evaluated once per QuickJS context.
//
// Decision: docs/adr/0006-embedded-quickjs-config-evaluator.md and
// docs/adr/0027-pure-path-and-url-modules-in-the-config-sandbox.md. Plan:
// docs/plans/pending/0001-wave-1-typescript-parity.md, Step 2.
//
// The host (Rust, crates/rb-config/src/js/mod.rs) provides exactly two primitives:
//   __rb_resolve(fromFile, specifier) -> target   resolution, refusing anything outside the
//                                                 repository, Node built-ins and the network
//   __rb_read(target) -> text                     the text of a resolved file or bundled preset
// Everything else here is plain JavaScript with no access to the host: a CommonJS `require`
// with a module cache, and pure `path` and `url` modules. There is no `process`, no timers, no
// `fs`, no network.
(function installSandbox(global) {
  'use strict';

  const cache = new Map();

  function normalizeSegments(segments, absolute) {
    const out = [];
    for (const segment of segments) {
      if (segment === '' || segment === '.') continue;
      if (segment === '..') {
        if (out.length > 0 && out[out.length - 1] !== '..') out.pop();
        else if (!absolute) out.push('..');
        continue;
      }
      out.push(segment);
    }
    return out;
  }

  const path = {
    sep: '/',
    delimiter: ':',
    isAbsolute(p) {
      return String(p).startsWith('/');
    },
    normalize(p) {
      const text = String(p);
      const absolute = text.startsWith('/');
      const trailing = text.endsWith('/') && text.length > 1;
      let joined = normalizeSegments(text.split('/'), absolute).join('/');
      if (absolute) joined = '/' + joined;
      if (joined === '') joined = absolute ? '/' : '.';
      return trailing && !joined.endsWith('/') ? joined + '/' : joined;
    },
    join(...parts) {
      const text = parts
        .map(String)
        .filter((p) => p !== '')
        .join('/');
      return text === '' ? '.' : path.normalize(text);
    },
    resolve(...parts) {
      let resolved = '';
      for (let i = parts.length - 1; i >= 0 && !resolved.startsWith('/'); i--) {
        const part = String(parts[i]);
        if (part !== '') resolved = resolved === '' ? part : part + '/' + resolved;
      }
      if (!resolved.startsWith('/')) resolved = global.__rb_cwd + '/' + resolved;
      const normal = '/' + normalizeSegments(resolved.split('/'), true).join('/');
      return normal;
    },
    dirname(p) {
      const text = String(p);
      const index = text.replace(/\/+$/, '').lastIndexOf('/');
      if (index < 0) return '.';
      if (index === 0) return '/';
      return text.slice(0, index);
    },
    basename(p, ext) {
      const base = String(p).replace(/\/+$/, '').split('/').pop() ?? '';
      return ext && base.endsWith(ext) && base !== ext ? base.slice(0, -ext.length) : base;
    },
    extname(p) {
      const base = path.basename(p);
      const index = base.lastIndexOf('.');
      return index <= 0 ? '' : base.slice(index);
    },
    relative(from, to) {
      const a = path.resolve(from).split('/').filter(Boolean);
      const b = path.resolve(to).split('/').filter(Boolean);
      let i = 0;
      while (i < a.length && i < b.length && a[i] === b[i]) i++;
      return [...a.slice(i).map(() => '..'), ...b.slice(i)].join('/');
    },
    parse(p) {
      const base = path.basename(p);
      const ext = path.extname(p);
      const dir = path.dirname(p);
      return {
        root: String(p).startsWith('/') ? '/' : '',
        dir,
        base,
        ext,
        name: ext ? base.slice(0, -ext.length) : base,
      };
    },
    format(o) {
      const base = o.base ?? (o.name ?? '') + (o.ext ?? '');
      return o.dir ? o.dir + '/' + base : base;
    },
  };
  path.posix = path;

  function fileURLToPath(url) {
    const href = typeof url === 'string' ? url : url.href;
    if (!href.startsWith('file://')) {
      throw new TypeError('The URL must be of scheme file');
    }
    return decodeURIComponent(href.slice('file://'.length));
  }

  function pathToFileURL(p) {
    return new URL('file://' + encodeURI(path.resolve(p)));
  }

  class URL {
    constructor(input, base) {
      const text = String(input);
      if (/^[a-z][a-z0-9+.-]*:/i.test(text)) {
        this.href = text;
      } else if (base !== undefined) {
        const baseHref = typeof base === 'string' ? base : base.href;
        if (!baseHref.startsWith('file://')) {
          throw new TypeError('Only file: URLs are available in the configuration sandbox');
        }
        const basePath = baseHref.slice('file://'.length);
        const joined = text.startsWith('/')
          ? text
          : path.dirname(basePath.endsWith('/') ? basePath + 'x' : basePath) + '/' + text;
        this.href = 'file://' + path.normalize(joined);
      } else {
        throw new TypeError('Invalid URL: ' + text);
      }
      const match = /^([a-z][a-z0-9+.-]*:)(?:\/\/([^/]*))?(.*)$/i.exec(this.href);
      this.protocol = match ? match[1] : '';
      this.host = match && match[2] ? match[2] : '';
      this.hostname = this.host;
      this.pathname = match ? match[3] : '';
    }
    toString() {
      return this.href;
    }
    toJSON() {
      return this.href;
    }
  }

  const url = { URL, fileURLToPath, pathToFileURL };
  const noop = () => undefined;
  global.console = { log: noop, info: noop, warn: noop, error: noop, debug: noop };
  global.URL = URL;
  global.__rb_path = path;
  global.__rb_url = url;

  function load(target) {
    if (target === 'rb:path') return path;
    if (target === 'rb:url') return url;
    const cached = cache.get(target);
    if (cached) return cached.exports;
    return run(target, global.__rb_read(target));
  }

  function run(target, text) {
    if (target.endsWith('.json')) {
      let value;
      try {
        value = JSON.parse(text);
      } catch (error) {
        throw new SyntaxError(target + ': ' + error.message, { cause: error });
      }
      cache.set(target, { exports: value });
      return value;
    }
    if (target.endsWith('.mjs')) {
      throw new Error(target + ' is an ES module; import it rather than require it');
    }
    const module = { exports: {}, id: target, filename: target, loaded: false };
    cache.set(target, module);
    let body;
    try {
      body = new Function('exports', 'require', 'module', '__filename', '__dirname', text);
    } catch (error) {
      cache.delete(target);
      throw new SyntaxError(target + ': ' + error.message, { cause: error });
    }
    body.call(
      module.exports,
      module.exports,
      makeRequire(target),
      module,
      target,
      path.dirname(target),
    );
    module.loaded = true;
    return module.exports;
  }

  function makeRequire(fromFile) {
    function require(specifier) {
      return load(global.__rb_resolve(fromFile, String(specifier)));
    }
    require.resolve = (specifier) => global.__rb_resolve(fromFile, String(specifier));
    require.cache = {};
    return require;
  }

  global.__rb_load_cjs = load;
  global.__rb_run_cjs = run;
})(globalThis);
