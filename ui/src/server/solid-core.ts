// ScriptC-safe subset of Solid's core helpers used by SSR-safe registry primitives.

export function splitProps<T extends Record<string, unknown>>(
  props: T,
  ...groups: ReadonlyArray<ReadonlyArray<string>>
): Array<Record<string, unknown>> {
  const claimed = new Set(groups.flat());
  const parts = groups.map((keys) => propertyView(props, new Set(keys), true));
  parts.push(propertyView(props, claimed, false));
  return parts;
}

export function mergeProps<T extends Record<string, unknown>>(...sources: T[]): T {
  return Object.assign({}, ...sources);
}

function propertyView(
  source: Record<string, unknown>,
  selected: Set<string>,
  includeSelected: boolean,
): Record<string, unknown> {
  const output: Record<string, unknown> = {};
  for (const key of Object.keys(source)) {
    if (selected.has(key) !== includeSelected) continue;
    Object.defineProperty(output, key, {
      enumerable: true,
      get: () => source[key],
    });
  }
  return output;
}
