interface ActionView {
  id: string;
  name: string;
  enabled: boolean;
  event: string;
  config: { url: string; method: string; bodyTemplate?: unknown };
  hasHeaders: boolean;
}

interface StreamFilter {
  includeTopics?: string[];
  excludeTopics?: string[];
}

interface StreamView {
  name: string;
  slug: string;
  icon: string | null;
  semanticDescription: string | null;
  rankingInstruction: string | null;
  filter: StreamFilter;
}

interface UserPreferences {
  aiFreeMode: boolean;
  streamMembershipMode: "multiple" | "exclusive";
  fontFamily: "sans" | "serif";
}

let draggedStreamSlug = "";

function csrfToken(): string {
  return document.cookie.split(";").map((part) => part.trim().split("="))
    .find(([name]) => name === "rill_csrf")?.[1] ?? "";
}

async function api(path: string, init?: RequestInit): Promise<Response> {
  const headers = new Headers(init?.headers);
  if (init?.method && init.method !== "GET") headers.set("x-csrf-token", csrfToken());
  return fetch(path, { ...init, headers, credentials: "same-origin" });
}

function node<K extends keyof HTMLElementTagNameMap>(tag: K, text?: string): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (text !== undefined) element.textContent = text;
  return element;
}

function report(message: string): void {
  const output = document.getElementById("settings-error");
  if (!(output instanceof HTMLParagraphElement)) return;
  output.textContent = message;
  output.hidden = !message;
}

function commaValues(value: FormDataEntryValue | null): string[] {
  return String(value ?? "").split(",").map((item) => item.trim().toLocaleLowerCase()).filter(Boolean);
}

function slugify(value: FormDataEntryValue | null): string {
  return String(value ?? "stream").toLocaleLowerCase().normalize("NFKD")
    .replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 64) || "stream";
}

async function jsonMutation(path: string, body: unknown, message: string): Promise<boolean> {
  const response = await api(path, {
    method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body),
  });
  if (!response.ok) report(message);
  return response.ok;
}

function collectionRow(title: string, subtitle: string, select: () => void): HTMLButtonElement {
  const row = node("button");
  row.type = "button";
  row.className = "settings-list-row";
  row.setAttribute("role", "option");
  row.setAttribute("aria-selected", "false");
  row.append(node("strong", title), node("span", subtitle));
  row.addEventListener("click", select);
  return row;
}

function selectRow(list: HTMLElement, row: HTMLElement): void {
  list.querySelectorAll<HTMLElement>("[role=option]").forEach((item) => item.setAttribute("aria-selected", String(item === row)));
}

function actionDetail(action: ActionView, reload: () => Promise<void>): HTMLElement {
  const detail = node("article");
  detail.className = "settings-preview";
  detail.append(
    node("h3", action.name),
    node("p", `${action.config.method} ${action.config.url}`),
    node("p", `Runs on Favorite · ${action.config.bodyTemplate ? "custom JSON body" : "default event body"} · private headers: ${action.hasHeaders ? "encrypted" : "none"}`),
  );
  const toggle = node("button", action.enabled ? "Pause" : "Enable");
  toggle.type = "button";
  toggle.addEventListener("click", async () => {
    const response = await api(`/api/v1/actions/${action.id}/enabled`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ enabled: !action.enabled }),
    });
    if (response.ok) await reload(); else report("Favorite action could not be updated.");
  });
  const remove = node("button", "Remove");
  remove.type = "button";
  remove.addEventListener("click", async () => {
    const response = await api(`/api/v1/actions/${action.id}`, { method: "DELETE" });
    if (response.ok) await reload(); else report("Favorite action could not be removed.");
  });
  detail.append(toggle, remove);
  return detail;
}

function streamDetail(stream: StreamView, reload: () => Promise<void>): HTMLElement {
  const detail = node("article");
  detail.className = "settings-preview";
  const title = node("div");
  title.className = "settings-preview-heading";
  const link = node("a", stream.name);
  link.href = `/stream/${stream.slug}`;
  title.append(link, node("span", stream.slug === "all" ? "Built-in" : "Custom"));
  detail.append(
    title,
    node("p", stream.semanticDescription || (stream.slug === "all" ? "Every story in one complete view." : "A broad reading lane.")),
    node("p", stream.rankingInstruction ? `Ranking: ${stream.rankingInstruction}` : "Default ranking"),
  );
  if (stream.slug === "all") return detail;

  const form = node("form");
  form.className = "admin-form compact";
  const field = (labelText: string, name: string, value: string, multiline = false) => {
    const label = node("label", labelText);
    const input = multiline ? node("textarea") : node("input");
    input.name = name;
    input.value = value;
    label.append(input);
    return label;
  };
  form.append(
    field("Name", "name", stream.name),
    field("What belongs here?", "semanticDescription", stream.semanticDescription ?? "", true),
    field("What ranks higher?", "rankingInstruction", stream.rankingInstruction ?? "", true),
    field("Include topics", "includeTopics", (stream.filter.includeTopics ?? []).join(", ")),
    field("Exclude topics", "excludeTopics", (stream.filter.excludeTopics ?? []).join(", ")),
  );
  const actions = node("div");
  actions.className = "settings-detail-actions";
  const save = node("button", "Save");
  save.className = "primary-action";
  const remove = node("button", "Delete");
  remove.type = "button";
  remove.className = "danger-action";
  actions.append(save, remove);
  form.append(actions);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(form);
    if (await jsonMutation(`/api/v1/streams/${stream.slug}`, {
      name: data.get("name"), icon: stream.icon,
      semanticDescription: data.get("semanticDescription") || null,
      rankingInstruction: data.get("rankingInstruction") || null,
      filter: { ...stream.filter, includeTopics: commaValues(data.get("includeTopics")), excludeTopics: commaValues(data.get("excludeTopics")) },
    }, "Stream could not be updated.")) await reload();
  });
  remove.addEventListener("click", async () => {
    const response = await api(`/api/v1/streams/${stream.slug}`, { method: "DELETE" });
    if (response.ok) await reload(); else report("Stream could not be deleted.");
  });
  detail.append(form);
  return detail;
}

function streamRow(
  stream: StreamView,
  index: number,
  streams: StreamView[],
  select: (row: HTMLElement) => void,
  reload: () => Promise<void>,
): HTMLElement {
  const wrapper = node("div");
  wrapper.className = "settings-list-entry";
  wrapper.dataset.streamSlug = stream.slug;
  const row = collectionRow(stream.name, stream.semanticDescription || (stream.slug === "all" ? "Every story" : "Reading lane"), () => select(row));
  wrapper.append(row);
  if (stream.slug === "all") return wrapper;

  const move = async (destination: number) => {
    if (destination < 1 || destination >= streams.length || destination === index) return;
    const slugs = streams.map((item) => item.slug);
    const [moved] = slugs.splice(index, 1);
    slugs.splice(destination, 0, moved);
    if (await jsonMutation("/api/v1/streams/reorder", { slugs }, "Stream order could not be changed.")) await reload();
  };
  const handle = node("span", "⋮⋮");
  handle.className = "stream-drag-handle";
  handle.draggable = true;
  handle.setAttribute("aria-hidden", "true");
  handle.addEventListener("dragstart", (event) => {
    draggedStreamSlug = stream.slug;
    event.dataTransfer?.setData("text/plain", stream.slug);
    wrapper.classList.add("is-dragging");
  });
  handle.addEventListener("dragend", () => { wrapper.classList.remove("is-dragging"); draggedStreamSlug = ""; });
  wrapper.addEventListener("dragover", (event) => event.preventDefault());
  wrapper.addEventListener("drop", (event) => {
    event.preventDefault();
    const source = event.dataTransfer?.getData("text/plain") || draggedStreamSlug;
    const sourceIndex = streams.findIndex((item) => item.slug === source);
    const destination = Math.max(1, index);
    if (sourceIndex < 1 || sourceIndex === destination) return;
    const slugs = streams.map((item) => item.slug);
    const [moved] = slugs.splice(sourceIndex, 1);
    slugs.splice(destination, 0, moved);
    void jsonMutation("/api/v1/streams/reorder", { slugs }, "Stream order could not be changed.").then((ok) => { if (ok) return reload(); });
  });
  const controls = node("div");
  controls.className = "stream-order-controls";
  for (const [label, destination, symbol] of [["Move up", index - 1, "↑"], ["Move down", index + 1, "↓"]] as const) {
    const button = node("button", symbol);
    button.type = "button";
    button.setAttribute("aria-label", `${label} ${stream.name}`);
    button.disabled = destination < 1 || destination >= streams.length;
    button.addEventListener("click", () => void move(destination));
    controls.append(button);
  }
  wrapper.prepend(handle);
  wrapper.append(controls);
  return wrapper;
}

function activatePreferences(): void {
  const form = document.getElementById("user-preferences");
  const status = document.getElementById("preferences-status");
  if (!(form instanceof HTMLFormElement) || !(status instanceof HTMLParagraphElement)) return;
  const aiFree = form.elements.namedItem("aiFreeMode");
  const membership = form.elements.namedItem("streamMembershipMode");
  const fontFamily = form.elements.namedItem("fontFamily");
  if (!(aiFree instanceof HTMLInputElement) || !(membership instanceof HTMLSelectElement) || !(fontFamily instanceof HTMLSelectElement)) return;
  void api("/api/v1/preferences").then(async (response) => {
    if (!response.ok) throw new Error();
    const preferences = await response.json() as UserPreferences;
    aiFree.checked = preferences.aiFreeMode;
    membership.value = preferences.streamMembershipMode;
    fontFamily.value = preferences.fontFamily;
  }).catch(() => report("Preferences could not be loaded."));
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    status.textContent = "Saving…";
    if (await jsonMutation("/api/v1/preferences", {
      aiFreeMode: aiFree.checked,
      streamMembershipMode: membership.value,
      fontFamily: fontFamily.value,
    }, "Preferences could not be saved.")) {
      status.textContent = "Saved.";
      document.querySelector(".modern-app")?.classList.toggle("reading-font-serif", fontFamily.value === "serif");
      document.querySelector(".modern-app")?.classList.toggle("reading-font-sans", fontFamily.value !== "serif");
    }
    else status.textContent = "";
  });
}

function activateStreams(): void {
  const form = document.getElementById("stream-create");
  const list = document.getElementById("stream-list");
  const detail = document.getElementById("stream-detail");
  const create = document.getElementById("stream-create-detail");
  const add = document.getElementById("add-stream");
  if (!(form instanceof HTMLFormElement) || !(list instanceof HTMLDivElement) || !detail || !create || !(add instanceof HTMLButtonElement)) return;
  let selected = "all";
  const showCreate = () => {
    selected = "";
    list.querySelectorAll<HTMLElement>("[role=option]").forEach((item) => item.setAttribute("aria-selected", "false"));
    detail.replaceChildren(create);
  };
  add.addEventListener("click", showCreate);
  const reload = async () => {
    const response = await api("/api/v1/streams");
    if (!response.ok) { report("Streams could not be loaded."); return; }
    const streams = await response.json() as StreamView[];
    let selectedRow: HTMLElement | undefined;
    const rows = streams.map((stream, index) => streamRow(stream, index, streams, (row) => {
      selected = stream.slug;
      selectRow(list, row);
      detail.replaceChildren(streamDetail(stream, reload));
    }, reload));
    list.replaceChildren(...rows);
    const selectedIndex = Math.max(0, streams.findIndex((stream) => stream.slug === selected));
    const stream = streams[selectedIndex];
    selectedRow = rows[selectedIndex]?.querySelector<HTMLElement>("[role=option]") ?? undefined;
    if (stream && selectedRow) {
      selectRow(list, selectedRow);
      detail.replaceChildren(streamDetail(stream, reload));
      selected = stream.slug;
    } else {
      showCreate();
    }
  };
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(form);
    if (await jsonMutation("/api/v1/streams", {
      name: data.get("name"), slug: slugify(data.get("name")),
      semanticDescription: data.get("semanticDescription") || null,
      rankingInstruction: data.get("rankingInstruction") || null,
      filter: { includeTopics: commaValues(data.get("includeTopics")), excludeTopics: commaValues(data.get("excludeTopics")) },
    }, "Stream could not be created.")) { selected = slugify(data.get("name")); form.reset(); await reload(); }
  });
  void reload();
}

function activateSettingsTabs(): void {
  const tabs = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-settings-tab]"));
  const panels = Array.from(document.querySelectorAll<HTMLElement>("[data-settings-panel]"));
  if (tabs.length === 0 || tabs.length !== panels.length) return;

  const select = (tab: HTMLButtonElement, focus: boolean) => {
    const value = tab.dataset.settingsTab;
    for (const item of tabs) {
      const selected = item === tab;
      item.setAttribute("aria-selected", String(selected));
      item.tabIndex = selected ? 0 : -1;
    }
    for (const panel of panels) panel.hidden = panel.dataset.settingsPanel !== value;
    if (focus) tab.focus();
  };

  tabs.forEach((tab, index) => {
    tab.addEventListener("click", () => select(tab, false));
    tab.addEventListener("keydown", (event) => {
      const target = event.key === "Home" ? 0
        : event.key === "End" ? tabs.length - 1
          : event.key === "ArrowRight" ? (index + 1) % tabs.length
            : event.key === "ArrowLeft" ? (index - 1 + tabs.length) % tabs.length
              : -1;
      if (target < 0) return;
      event.preventDefault();
      select(tabs[target], true);
    });
  });
}

function activateDeviceBrowser(): void {
  const panel = document.getElementById("settings-panel-devices");
  if (!panel) return;
  const selectors = Array.from(panel.querySelectorAll<HTMLElement>("[data-settings-select]"));
  const details = Array.from(panel.querySelectorAll<HTMLElement>("[data-settings-detail]"));
  const select = (selector: HTMLElement) => {
    const target = selector.dataset.settingsSelect;
    selectors.forEach((item) => {
      if (item.getAttribute("role") === "option") item.setAttribute("aria-selected", String(item === selector));
    });
    details.forEach((detail) => { detail.hidden = detail.dataset.settingsDetail !== target; });
  };
  selectors.forEach((selector) => selector.addEventListener("click", () => select(selector)));
  const visible = details.find((detail) => !detail.hidden);
  const initial = selectors.find((selector) => selector.dataset.settingsSelect === visible?.dataset.settingsDetail)
    ?? selectors.find((selector) => selector.getAttribute("role") === "option");
  if (initial) select(initial);
}

export function activateUserSettings(): void {
  activateSettingsTabs();
  activateDeviceBrowser();
  activatePreferences();
  activateStreams();
  const form = document.getElementById("favorite-action-create");
  const list = document.getElementById("favorite-action-list");
  const detail = document.getElementById("favorite-action-detail");
  const create = document.getElementById("favorite-action-create-detail");
  const add = document.getElementById("add-favorite-action");
  if (!(form instanceof HTMLFormElement) || !(list instanceof HTMLDivElement) || !detail || !create || !(add instanceof HTMLButtonElement)) return;
  let selectedAction = "";
  const showCreate = () => {
    selectedAction = "";
    list.querySelectorAll<HTMLElement>("[role=option]").forEach((item) => item.setAttribute("aria-selected", "false"));
    detail.replaceChildren(create);
  };
  add.addEventListener("click", showCreate);

  const reload = async () => {
    const response = await api("/api/v1/actions");
    if (!response.ok) {
      report("Favorite actions could not be loaded.");
      return;
    }
    const actions = await response.json() as ActionView[];
    const rows = actions.map((action) => collectionRow(action.name, `${action.config.method} ${action.config.url}`, () => {
      selectedAction = action.id;
      selectRow(list, rows[actions.indexOf(action)]);
      detail.replaceChildren(actionDetail(action, reload));
    }));
    list.replaceChildren(...(rows.length ? rows : [node("p", "No actions yet.")]));
    const selectedIndex = Math.max(0, actions.findIndex((action) => action.id === selectedAction));
    if (actions[selectedIndex] && rows[selectedIndex]) {
      selectedAction = actions[selectedIndex].id;
      selectRow(list, rows[selectedIndex]);
      detail.replaceChildren(actionDetail(actions[selectedIndex], reload));
    } else {
      showCreate();
    }
    report("");
  };

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(form);
    let headers: Record<string, string>;
    let bodyTemplate: unknown = null;
    try {
      const parsed = JSON.parse(String(data.get("headers") || "{}")) as unknown;
      if (!parsed || Array.isArray(parsed) || typeof parsed !== "object"
        || Object.values(parsed).some((value) => typeof value !== "string")) throw new Error();
      headers = parsed as Record<string, string>;
    } catch {
      report("Private headers must be a JSON object.");
      return;
    }
    const rawTemplate = String(data.get("body_template") || "").trim();
    if (rawTemplate) {
      try {
        bodyTemplate = JSON.parse(rawTemplate) as unknown;
      } catch {
        report("Body template must be valid JSON.");
        return;
      }
    }
    const headerName = String(data.get("header_name") || "").trim();
    const headerValue = String(data.get("header_value") || "");
    if (Boolean(headerName) !== Boolean(headerValue)) { report("Private header name and value are both required."); return; }
    if (headerName) headers[headerName] = headerValue;
    const response = await api("/api/v1/actions", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        name: data.get("name"),
        url: data.get("url"),
        method: data.get("method"),
        timeoutSeconds: 15,
        maximumResponseBytes: 65_536,
        maximumAttempts: 5,
        bodyTemplate,
        headers,
        enabled: true,
      }),
    });
    if (!response.ok) {
      report("Favorite action could not be created.");
      return;
    }
    selectedAction = String((await response.clone().json().catch(() => ({ id: "" })))?.id ?? "");
    form.reset();
    await reload();
  });

  void reload();
}
