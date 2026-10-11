import { discount } from "./discount";
import type { View } from "../ui/view";

export const total = (n: number): number => n - discount(n);
export type Shown = View;
