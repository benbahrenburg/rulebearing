import { Entity, Column } from './decorators';

export interface Identified {
  readonly id: string;
}

export interface Named extends Identified {
  name: string;
  rename(to: string): void;
}

export enum Status {
  Active,
  Retired,
}

export const enum Flags {
  None = 0,
}

export type Key = string | number;

export type Lookup<T> = Map<Key, T>;

@Entity('records')
export abstract class Record<T> implements Identified {
  static count = 0;
  readonly id: string;
  @Column({ nullable: false })
  protected payload?: T;

  constructor(id: string) {
    this.id = id;
  }

  abstract validate(): boolean;

  save(): Status {
    this.validate();
    return Status.Active;
  }
}

class Internal {}

export function makeKey(parts: string[]): Key {
  return parts.join('.');
}
