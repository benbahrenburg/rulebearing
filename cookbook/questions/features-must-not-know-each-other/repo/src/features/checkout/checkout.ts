import type { Money } from '../../shared/money';

export function charge(amount: Money): string {
  return `charged ${String(amount.cents)} ${amount.currency}`;
}
