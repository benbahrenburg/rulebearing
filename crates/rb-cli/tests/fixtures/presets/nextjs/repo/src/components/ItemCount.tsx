import { items } from "../server/db";

export function ItemCount(): number {
  return items().length;
}
