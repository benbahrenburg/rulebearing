import { items } from "../../../server/db";

export interface Item {
  name: string;
}

export function GET(): string[] {
  return items();
}
