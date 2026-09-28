import { money } from "../../shared/money";
import { pay } from "../checkout/pay";

export function cart(): string {
  return money(pay());
}
