export function legacy(): Promise<Response> {
  return fetch("/admin/orders/store", { method: "POST" });
}
