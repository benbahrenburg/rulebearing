import { Record, Named, Status, Lookup } from '../model';
import * as model from '../model';
import { Store } from './store';
import { Component } from 'react';

@model.decorators.Entity('widgets')
export class Widget extends Record<Named> implements Named, model.Identified {
  name = '';
  #secret = 42;
  private static instances: Lookup<Widget> = new Map();
  public readonly created: Date = new Date();

  constructor(id: string, private readonly store: Store, protected label?: string) {
    super(id);
  }

  get title(): string {
    return this.name;
  }

  set title(value: string) {
    this.rename(value);
  }

  get size(): number {
    return 1;
  }

  accessor visible: boolean = true;

  rename(to: string): void {
    this.name = to;
    this.store.put(this);
    this.#touch();
  }

  validate(): boolean {
    const other: Widget = Widget.create('x', this.store);
    other.save();
    return this.#secret > 0;
  }

  static create(id: string, store: Store): Widget {
    return new Widget(id, store);
  }

  #touch(): void {}

  render(status: Status): JSX.Element | null {
    return null;
  }
}

export class View extends Component {}

export default class {
  show(): void {}
}
