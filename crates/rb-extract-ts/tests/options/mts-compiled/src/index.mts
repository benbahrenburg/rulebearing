import { a } from "./a.js";
import type { T } from "./t.js";
export { b } from "./b.js";
export const lazy = () => import("./c.js");
export const value: T = a;
