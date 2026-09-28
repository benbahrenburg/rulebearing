import { items } from "../../../server/db";

export function GET(): string[] {
  return items();
}
