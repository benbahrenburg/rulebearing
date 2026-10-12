import { drain } from '../outbox/outbox';
import { connect } from './broker';
import type { Broker } from './broker';

export function publishPending(broker: Broker): number {
  const events = drain();
  for (const event of events) {
    broker.send(event.topic, event.payload);
  }
  return events.length;
}

export function start(url: string): Broker {
  return connect(url);
}
