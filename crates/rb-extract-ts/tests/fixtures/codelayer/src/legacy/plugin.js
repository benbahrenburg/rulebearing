const { Store } = require('../ui/store');

class Plugin {
  #enabled = false;

  static of(name) {
    return new Plugin(name);
  }

  get enabled() {
    return this.#enabled;
  }

  run(store) {
    this.log();
    store.put(this);
  }

  log() {}
}

class LocalStore extends Store {}

function helper() {}

module.exports = { Plugin, LocalStore };
