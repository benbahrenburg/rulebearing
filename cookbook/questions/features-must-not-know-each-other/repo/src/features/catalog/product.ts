import type { Money } from '../../shared/money';

export interface Product {
  readonly sku: string;
  readonly price: Money;
}

export function findProduct(sku: string): Product {
  return { sku, price: { cents: 1000, currency: 'EUR' } };
}
