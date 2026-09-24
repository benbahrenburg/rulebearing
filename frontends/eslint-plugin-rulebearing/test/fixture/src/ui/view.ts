import { query } from '../db/query';
import { fetchAll } from '../services/api';
import { formatDate } from './format';

const legacy = require('../legacy/old.cjs');

export const view = (): string => formatDate(query() + fetchAll() + String(legacy));
