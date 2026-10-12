import { ok } from '../shared/http';
import type { Reply } from '../shared/http';
import { orderById } from '../services/order-service';

export function getOrder(id: string): Reply {
  return ok(orderById(id));
}
