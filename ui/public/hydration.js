globalThis._$HY ||= {
  events: [],
  completed: new WeakSet(),
  r: {},
  fe() {}
};

const hydrationRoot = (node) => {
  if (!node || !node.hasAttribute) return null;
  if (node.hasAttribute("data-hk")) return node;
  const parent = node.host && node.host.nodeType ? node.host : node.parentNode;
  return hydrationRoot(parent);
};

for (const eventName of ["click", "input"]) {
  document.addEventListener(eventName, (event) => {
    if (!globalThis._$HY.events) return;
    const path = event.composedPath ? event.composedPath() : [event.target];
    const root = hydrationRoot(path[0]);
    if (root && !globalThis._$HY.completed.has(root)) {
      globalThis._$HY.events.push([root, event]);
    }
  });
}

