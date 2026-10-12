import { ok } from '../shared/http';
import type { Reply } from '../shared/http';
import { allOrders } from '../repositories/order-repository';

export function getRevenue(): Reply {
  const cents = allOrders().reduce((sum, row) => sum + row.totalCents, 0);
  return ok({ cents });
}
