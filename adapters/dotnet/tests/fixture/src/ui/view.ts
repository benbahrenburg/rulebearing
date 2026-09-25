import { load } from '../db/store.js';
import { call } from '../service/api.js';

export const view = (): number[] => [load(), call()];
