import { GET } from "../app/api/items/route";

export function links(): string[] {
  return GET();
}
