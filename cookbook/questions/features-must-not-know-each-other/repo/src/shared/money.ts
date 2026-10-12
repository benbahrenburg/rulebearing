export interface Money {
  readonly cents: number;
  readonly currency: string;
}

export function add(a: Money, b: Money): Money {
  return { cents: a.cents + b.cents, currency: a.currency };
}
