import { add } from '../../shared/money';
import type { Money } from '../../shared/money';
import { findProduct } from '../catalog/product';
import type { Line } from './line';

export function addToCart(lines: readonly Line[], sku: string): Line[] {
  const product = findProduct(sku);
  return [...lines, { sku, price: product.price }];
}

export function total(lines: readonly Line[]): Money {
  return lines.reduce((sum, line) => add(sum, line.price), { cents: 0, currency: 'EUR' });
}
