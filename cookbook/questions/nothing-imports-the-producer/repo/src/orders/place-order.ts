import { enqueue } from '../outbox/outbox';

export function placeOrder(id: string): void {
  enqueue({ topic: 'order-placed', payload: JSON.stringify({ id }) });
}
