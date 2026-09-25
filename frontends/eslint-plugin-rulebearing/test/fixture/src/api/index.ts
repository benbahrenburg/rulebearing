export * from '../db/query';
export { store } from '../db/store';
export { fetchAll } from '../services/api';

export async function loadCache(): Promise<unknown> {
  return import('../db/cache');
}
