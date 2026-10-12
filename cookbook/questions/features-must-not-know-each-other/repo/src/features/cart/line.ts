import type { Money } from '../../shared/money';

export interface Line {
  readonly sku: string;
  readonly price: Money;
}
