import { x } from "./excluded/x";
import { kept } from "./kept";

export const later = () => import("./lazy");
export default [x, kept];
