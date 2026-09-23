import type { Shape } from "./shape";
import { area } from "./area";

export const size = (s: Shape): number => area(s);
