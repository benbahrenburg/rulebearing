import { total } from "./order";

export const discount = (n: number): number => (n > 10 ? total(n / 2) : 0);
