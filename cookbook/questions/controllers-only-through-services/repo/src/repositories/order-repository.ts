export interface OrderRow {
  readonly id: string;
  readonly totalCents: number;
}

const rows = new Map<string, OrderRow>([['o-1', { id: 'o-1', totalCents: 1250 }]]);

export function findOrder(id: string): OrderRow | undefined {
  return rows.get(id);
}

export function allOrders(): OrderRow[] {
  return [...rows.values()];
}
