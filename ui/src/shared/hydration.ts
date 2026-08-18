export function readHydrationState<T>(): T {
  const node = document.getElementById("rill-hydration");
  if (!(node instanceof HTMLScriptElement)) {
    throw new Error("missing hydration state");
  }
  return JSON.parse(node.textContent ?? "") as T;
}
