import { series } from "../lib/data";

export function Chart(): string {
  return series().join(",");
}
