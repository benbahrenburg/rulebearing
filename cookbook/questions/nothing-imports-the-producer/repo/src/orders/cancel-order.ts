import { publishPending, start } from '../producer/producer';
import { enqueue } from '../outbox/outbox';

export function cancelOrder(id: string): void {
  enqueue({ topic: 'order-cancelled', payload: JSON.stringify({ id }) });
  // Publishing from the request path couples the request to the broker being up.
  publishPending(start('kafka://localhost:9092'));
}
