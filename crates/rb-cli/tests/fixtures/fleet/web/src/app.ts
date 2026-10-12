import { placeOrder } from "./api/client";
import { listOrders } from "./api/generated/orders";
import { legacy } from "./legacy";

export async function start(): Promise<void> {
  await placeOrder(1);
  await listOrders();
  await legacy();
}
