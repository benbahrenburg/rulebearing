import { format } from "@app/util/format";
import { total } from "./order/order";
import { chunk } from "lodash";
import { readFileSync } from "node:fs";
import { missing } from "./not-there";

export const main = () => format(total(chunk([1, 2], 1).length)) + readFileSync + missing;
