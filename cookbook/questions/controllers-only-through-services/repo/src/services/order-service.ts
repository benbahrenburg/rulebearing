import { allOrders, findOrder } from '../repositories/order-repository';
import type { OrderRow } from '../repositories/order-repository';

export function orderById(id: string): OrderRow {
  const row = findOrder(id);
  if (row === undefined) {
    throw new Error(`no order ${id}`);
  }
  return row;
}

export function revenueCents(): number {
  return allOrders().reduce((sum, row) => sum + row.totalCents, 0);
}
