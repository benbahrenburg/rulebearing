export interface OutboxEvent {
  readonly topic: string;
  readonly payload: string;
}

const pending: OutboxEvent[] = [];

export function enqueue(event: OutboxEvent): void {
  pending.push(event);
}

export function drain(): OutboxEvent[] {
  return pending.splice(0, pending.length);
}
