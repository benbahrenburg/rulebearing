import type { Identified } from '../model/base';

export class Store {
  private items: Identified[] = [];

  put(item: Identified): void {
    this.items.push(item);
  }
}

export namespace Shapes {
  export class Circle {
    static Unit = class {
      radius = 1;
    };
  }

  export namespace Solid {
    export class Sphere extends Circle {}
  }

  class Hidden {}
}
