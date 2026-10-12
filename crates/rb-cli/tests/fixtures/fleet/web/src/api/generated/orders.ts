// Generated from api/openapi.json: one function per operation, named by its operationId.
export function createOrder(body: { id: number }): Promise<Response> {
  return fetch("/orders", { method: "POST", body: JSON.stringify(body) });
}

export function listOrders(): Promise<Response> {
  return fetch("/orders");
}
