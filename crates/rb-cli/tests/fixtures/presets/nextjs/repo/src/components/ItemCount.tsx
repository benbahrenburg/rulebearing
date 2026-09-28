import type { Item } from "../app/api/items/route";
import { items } from "../server/db";

export function ItemCount(): number {
  const listed: Item[] = items().map((name) => ({ name }));
  return listed.length;
}
