// Scriptc-safe subset of Solid's server runtime used by compiler-emitted helpers.
// Browser builds use the official Solid runtime. Keep behavior covered by parity tests.

export interface SsrFragment {
  t: string;
}

export type SsrNode = string | number | boolean | null | undefined | SsrFragment | SsrNode[];

interface HydrationContext {
  id: string;
  count: number;
}

let hydrationContext: HydrationContext | null = null;

export function renderToString(
  code: () => SsrNode,
  options: { renderId: string }
): string {
  hydrationContext = { id: options.renderId, count: 0 };
  const output = resolveSsrNode(code(), true);
  hydrationContext = null;
  return output;
}

export function createComponent<P>(component: (props: P) => SsrNode, props: P): SsrNode {
  if (hydrationContext === null) return component(props);
  const parent = hydrationContext;
  hydrationContext = { id: nextContextId(parent), count: 0 };
  const result = component(props);
  hydrationContext = parent;
  return result;
}

export function mergeProps<T extends Record<string, unknown>>(...sources: T[]): T {
  return Object.assign({}, ...sources);
}

export function ssr(template: readonly string[], ...nodes: SsrNode[]): SsrFragment {
  if (nodes.length === 0) return { t: template[0] };
  let result = "";
  for (let index = 0; index < nodes.length; index += 1) {
    result += template[index];
    const node = nodes[index];
    if (node !== undefined) result += resolveSsrNode(node, false);
  }
  return { t: result + template[nodes.length] };
}

export function ssrHydrationKey(): string {
  if (hydrationContext === null) return "";
  return ` data-hk="${nextContextId(hydrationContext)}"`;
}

export function ssrAttribute(key: string, value: SsrNode, isBoolean: boolean): string {
  if (isBoolean) return value ? ` ${key}` : "";
  return value === null || value === undefined ? "" : ` ${key}="${resolveSsrNode(value, true)}"`;
}

const voidElements = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"]);
const booleanAttributes = new Set(["autofocus", "checked", "disabled", "hidden", "multiple", "open", "readonly", "required", "selected"]);

function spreadAttributes(props: Record<string, unknown>): string {
  let output = "";
  let classWritten = false;
  for (const [property, value] of Object.entries(props)) {
    if (property === "children" || property === "innerHTML" || property === "textContent" || property === "ref" || property.startsWith("on")) continue;
    if (value === null || value === undefined || value === false) continue;
    if (property === "class" || property === "className" || property === "classList") {
      if (classWritten) continue;
      const classList = props.classList && typeof props.classList === "object"
        ? Object.entries(props.classList).filter(([, enabled]) => enabled).map(([name]) => name).join(" ")
        : "";
      const className = [props.class, props.className, classList].filter(Boolean).join(" ");
      output += ` class="${resolveSsrNode(escape(String(className), true), true)}"`;
      classWritten = true;
      continue;
    }
    if (property === "style") {
      const style = typeof value === "string" ? value : Object.entries(value as Record<string, unknown>)
        .filter(([, item]) => item !== null && item !== undefined)
        .map(([name, item]) => `${name}:${String(item)}`).join(";");
      output += ` style="${resolveSsrNode(escape(style, true), true)}"`;
      continue;
    }
    const name = property === "htmlFor" ? "for" : property === "className" ? "class" : property;
    if (booleanAttributes.has(name.toLowerCase())) {
      if (value) output += ` ${name}`;
      continue;
    }
    output += ` ${name}="${resolveSsrNode(escape(String(value), true), true)}"`;
  }
  return output;
}

export function ssrElement(tag: string, props: Record<string, unknown> | null, children?: SsrNode, needsId = false): SsrFragment {
  const values = props ?? {};
  const isVoid = voidElements.has(tag);
  let child = children;
  if (child === undefined && !isVoid) {
    if (values.innerHTML !== undefined || tag === "script" || tag === "style") child = values.innerHTML as SsrNode;
    else if (values.textContent !== undefined) child = escape(values.textContent as SsrNode);
    else child = escape(values.children as SsrNode);
  }
  const opening = `<${tag}${needsId ? ssrHydrationKey() : ""}${spreadAttributes(values)}>`;
  if (isVoid) return { t: opening.slice(0, -1) + "/>" };
  return { t: `${opening}${resolveSsrNode(child, true)}</${tag}>` };
}

export function escape(value: SsrNode, attribute = false): SsrNode {
  if (typeof value === "string") return escapeString(value, attribute);
  if (Array.isArray(value)) {
    const escaped: SsrNode[] = [];
    for (const entry of value) escaped.push(escape(entry, attribute));
    return escaped;
  }
  if (attribute && typeof value === "boolean") return String(value);
  return value;
}

function resolveSsrNode(node: SsrNode, top: boolean): string {
  if (typeof node === "string") return node;
  if (node === null || node === undefined || typeof node === "boolean") return "";
  if (Array.isArray(node)) {
    let output = "";
    let previousWasText = false;
    for (const entry of node) {
      const currentIsText = typeof entry !== "object";
      if (!top && previousWasText && currentIsText) output += "<!--!$-->";
      output += resolveSsrNode(entry, false);
      previousWasText = currentIsText;
    }
    return output;
  }
  if (typeof node === "object") return node.t;
  return String(node);
}

function nextContextId(context: HydrationContext): string {
  const count = context.count;
  context.count += 1;
  const number = String(count);
  const lengthMarker = number.length > 1 ? String.fromCharCode(95 + number.length) : "";
  return context.id + lengthMarker + number;
}

function escapeString(value: string, attribute: boolean): string {
  const output: string[] = [];
  let start = 0;
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    let replacement = "";
    if (character === "&") replacement = "&amp;";
    else if (character === "<") replacement = "&lt;";
    else if (character === '"' && attribute) replacement = "&quot;";
    if (!replacement) continue;
    if (start < index) output.push(value.slice(start, index));
    output.push(replacement);
    start = index + 1;
  }
  if (start === 0) return value;
  if (start < value.length) output.push(value.slice(start));
  return output.join("");
}
