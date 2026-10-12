export interface Reply {
  readonly status: number;
  readonly body: string;
}

export function ok(body: unknown): Reply {
  return { status: 200, body: JSON.stringify(body) };
}
