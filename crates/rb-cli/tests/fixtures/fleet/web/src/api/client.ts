import { createOrder } from "./generated/orders";

export function placeOrder(id: number): Promise<Response> {
  return createOrder({ id });
}
