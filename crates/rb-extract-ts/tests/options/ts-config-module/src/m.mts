import { b } from "./b.js";
import type { T } from "./t.js";
export * from "./c.js";
export * as ns from "./d.js";
export const later = import("./e.js");
export const a: T = b;
