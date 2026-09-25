// Decorator factories, declared here so the decorators that use them resolve.
export function Entity(table: string) {
  return (target: unknown) => target;
}

export const Column = (options?: object) => (target: unknown, key: string) => undefined;
