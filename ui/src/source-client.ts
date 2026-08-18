interface SourceView {
  id: string; kind: string; name: string; visibility: string; enabled: boolean;
  lastSuccessAt: number | null; consecutiveFailures: number; lastErrorMessage: string | null;
}

interface StreamFilter {
  includeSources?: string[]; excludeSources?: string[]; includeCurators?: string[]; excludeCurators?: string[];
  includePublishers?: string[]; excludePublishers?: string[]; includeTopics?: string[]; excludeTopics?: string[];
  languages?: string[]; textQuery?: string | null; maximumAgeHours?: number | null; minimumCoverage?: number | null;
  read?: boolean | null; favorite?: boolean | null;
}

interface StreamView {
  name: string; slug: string; icon: string | null; position: number;
  semanticDescription: string | null; rankingInstruction: string | null;
  filter: StreamFilter;
}
interface TelegramBindingView { bound: boolean; telegramUserId: number | null; botUsername: string | null; }
interface TelegramBindingChallengeView { deepLink: string; expiresAt: number; botUsername: string; }
interface QuickAddSourceResponse { id: string; kind: string; name: string; url: string; }

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
  const output = document.getElementById("sources-error");
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

async function jsonMutation(path: string, body: unknown, message: string): Promise<Response | null> {
  const response = await api(path, {
    method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body),
  });
  if (!response.ok) { report(message); return null; }
  report("");
  return response;
}

function sourceCard(source: SourceView, reload: () => Promise<void>): HTMLElement {
  const card = node("article"); card.className = "admin-card";
  const health = source.lastErrorMessage ? `${source.consecutiveFailures} failures · ${source.lastErrorMessage}`
    : source.lastSuccessAt ? `Last success ${new Date(source.lastSuccessAt * 1000).toLocaleString()}` : "Never fetched";
  card.append(node("h3", source.name), node("p", `${source.kind} · ${source.visibility}`), node("p", health));
  const poll = node("button", "Fetch now"); poll.type = "button";
  poll.addEventListener("click", async () => {
    if (await jsonMutation(`/api/v1/sources/${source.id}/poll`, {}, "Source could not be queued.")) await reload();
  });
  const toggle = node("button", source.enabled ? "Disable" : "Enable"); toggle.type = "button";
  toggle.addEventListener("click", async () => {
    if (await jsonMutation(`/api/v1/sources/${source.id}/enabled`, { enabled: !source.enabled }, "Source state could not be changed.")) await reload();
  });
  const remove = node("button", "Remove"); remove.type = "button";
  remove.addEventListener("click", async () => {
    const response = await api(`/api/v1/sources/${source.id}`, { method: "DELETE" });
    if (response.ok) await reload(); else report("Source could not be removed.");
  });
  card.append(poll, toggle, remove);
  return card;
}

function streamCard(stream: StreamView, index: number, streams: StreamView[], reload: () => Promise<void>): HTMLElement {
  const card = node("article"); card.className = "admin-card";
  const link = node("a", stream.name); link.href = `/stream/${stream.slug}`;
  const title = node("h3"); title.append(link);
  const form = node("form"); form.className = "admin-form";
  const name = node("input"); name.name = "name"; name.value = stream.name; name.required = true; name.setAttribute("aria-label", `Name for ${stream.name}`);
  const description = node("textarea"); description.name = "semanticDescription"; description.value = stream.semanticDescription ?? ""; description.setAttribute("aria-label", `Semantic description for ${stream.name}`);
  const ranking = node("textarea"); ranking.name = "rankingInstruction"; ranking.value = stream.rankingInstruction ?? ""; ranking.setAttribute("aria-label", `Ranking instruction for ${stream.name}`);
  const includeTopics = node("input"); includeTopics.name = "includeTopics"; includeTopics.value = (stream.filter.includeTopics ?? []).join(", "); includeTopics.placeholder = "Include topics"; includeTopics.setAttribute("aria-label", `Included topics for ${stream.name}`);
  const excludeTopics = node("input"); excludeTopics.name = "excludeTopics"; excludeTopics.value = (stream.filter.excludeTopics ?? []).join(", "); excludeTopics.placeholder = "Exclude topics"; excludeTopics.setAttribute("aria-label", `Excluded topics for ${stream.name}`);
  const save = node("button", "Save"); save.type = "submit";
  const field = (text: string, control: HTMLElement) => {
    const label = node("label", text);
    label.append(control);
    return label;
  };
  const advanced = node("details"); advanced.className = "advanced-options";
  advanced.append(node("summary", "Advanced tag filters"), field("Include topics", includeTopics), field("Exclude topics", excludeTopics));
  form.append(
    field("Name", name),
    field("What belongs here?", description),
    field("What should rank higher?", ranking),
    advanced,
    save,
  );
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (await jsonMutation(`/api/v1/streams/${stream.slug}`, {
      name: name.value, icon: stream.icon, filter: { ...stream.filter, includeTopics: commaValues(includeTopics.value), excludeTopics: commaValues(excludeTopics.value) },
      semanticDescription: description.value || null, rankingInstruction: ranking.value || null,
    }, "Stream could not be updated.")) await reload();
  });
  card.append(title, form);
  for (const [label, destination] of [["Move up", index - 1], ["Move down", index + 1]] as const) {
    const button = node("button", label); button.type = "button";
    button.disabled = destination < 0 || destination >= streams.length;
    button.addEventListener("click", async () => {
      const slugs = streams.map((item) => item.slug);
      [slugs[index], slugs[destination]] = [slugs[destination], slugs[index]];
      if (await jsonMutation("/api/v1/streams/reorder", { slugs }, "Stream order could not be changed.")) await reload();
    });
    card.append(button);
  }
  if (stream.slug !== "home") {
    const remove = node("button", "Delete"); remove.type = "button";
    remove.addEventListener("click", async () => {
      const response = await api(`/api/v1/streams/${stream.slug}`, { method: "DELETE" });
      if (response.ok) await reload(); else report("Stream could not be deleted.");
    });
    card.append(remove);
  }
  return card;
}

async function renderTelegramBinding(container: HTMLDivElement): Promise<void> {
  const response = await api("/api/v1/telegram/binding");
  if (!response.ok) {
    container.replaceChildren(node("p", "Telegram bot binding is unavailable."));
    return;
  }
  const binding = await response.json() as TelegramBindingView;
  if (binding.bound) {
    const status = node("p", `Connected to Telegram${binding.telegramUserId ? ` as ${binding.telegramUserId}` : ""}.`);
    const disconnect = node("button", "Disconnect Telegram");
    disconnect.type = "button";
    disconnect.className = "danger-action";
    disconnect.addEventListener("click", async () => {
      const result = await api("/api/v1/telegram/binding", { method: "DELETE" });
      if (result.ok) await renderTelegramBinding(container); else report("Telegram could not be disconnected.");
    });
    container.replaceChildren(status, disconnect);
    return;
  }
  if (!binding.botUsername) {
    const message = node("p", "A Telegram bot must be configured by an administrator before binding.");
    container.replaceChildren(message);
    return;
  }
  const connect = node("button", `Connect with @${binding.botUsername}`);
  connect.type = "button";
  connect.className = "secondary-action";
  connect.addEventListener("click", async () => {
    const challengeResponse = await api("/api/v1/telegram/binding", { method: "POST" });
    if (!challengeResponse.ok) { report("Telegram binding link could not be created."); return; }
    const challenge = await challengeResponse.json() as TelegramBindingChallengeView;
    const link = node("a", `Open @${challenge.botUsername} in Telegram`);
    link.href = challenge.deepLink;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.className = "primary-action";
    const expiry = node("p", `This link expires at ${new Date(challenge.expiresAt * 1000).toLocaleTimeString()}.`);
    container.replaceChildren(link, expiry);
  });
  container.replaceChildren(connect);
}

export function activateSources(): void {
  const quickAddForm = document.getElementById("source-quick-add");
  const quickAddResult = document.getElementById("quick-add-result");
  const sourceList = document.getElementById("source-manager-list");
  const rssForm = document.getElementById("rss-create");
  const opmlForm = document.getElementById("opml-import");
  const emailForm = document.getElementById("email-create");
  const streamForm = document.getElementById("stream-create");
  const streamList = document.getElementById("stream-list");
  const telegramSourceForm = document.getElementById("telegram-source-create");
  const telegramBinding = document.getElementById("telegram-binding");
  if (!(quickAddForm instanceof HTMLFormElement) || !(quickAddResult instanceof HTMLParagraphElement)
    || !(sourceList instanceof HTMLDivElement) || !(rssForm instanceof HTMLFormElement)
    || !(opmlForm instanceof HTMLFormElement) || !(streamForm instanceof HTMLFormElement)
    || !(streamList instanceof HTMLDivElement)) return;

  const reload = async () => {
    const [sourcesResponse, streamsResponse] = await Promise.all([api("/api/v1/sources"), api("/api/v1/streams")]);
    if (!sourcesResponse.ok || !streamsResponse.ok) { report("Sources could not be loaded."); return; }
    const sources = await sourcesResponse.json() as SourceView[];
    const streams = await streamsResponse.json() as StreamView[];
    sourceList.replaceChildren(...(sources.length ? sources.map((source) => sourceCard(source, reload)) : [node("p", "No sources configured.")]));
    streamList.replaceChildren(...streams.map((stream, index) => streamCard(stream, index, streams, reload)));
    if (telegramBinding instanceof HTMLDivElement) await renderTelegramBinding(telegramBinding);
  };

  rssForm.addEventListener("submit", async (event) => {
    event.preventDefault(); const data = new FormData(rssForm);
    if (await jsonMutation("/api/v1/sources/rss", { name: data.get("name"), url: data.get("url"), shared: data.get("shared") === "on", pollIntervalSeconds: 900 }, "Feed could not be added.")) { rssForm.reset(); await reload(); }
  });

  for (const example of document.querySelectorAll<HTMLButtonElement>("[data-quick-source]")) {
    example.addEventListener("click", () => {
      const input = quickAddForm.elements.namedItem("input");
      if (input instanceof HTMLInputElement) {
        input.value = example.dataset.quickSource ?? "";
        input.focus();
      }
    });
  }

  quickAddForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(quickAddForm);
    const submit = quickAddForm.querySelector<HTMLButtonElement>("button[type='submit']");
    if (submit) { submit.disabled = true; submit.textContent = "Finding source…"; }
    const response = await api("/api/v1/sources/quick-add", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ input: data.get("input") }),
    });
    if (submit) { submit.disabled = false; submit.textContent = "Add source"; }
    if (!response.ok) {
      const body = await response.json().catch(() => null) as { error?: string } | null;
      quickAddResult.textContent = body?.error ?? "Rill could not recognize that source.";
      quickAddResult.dataset.state = "error";
      return;
    }
    const source = await response.json() as QuickAddSourceResponse;
    quickAddResult.textContent = `${source.name} added as ${source.kind === "rss" ? "RSS / Atom" : "Telegram"}. First fetch is queued.`;
    quickAddResult.dataset.state = "success";
    quickAddForm.reset();
    await reload();
  });

  opmlForm.addEventListener("submit", async (event) => {
    event.preventDefault(); const input = opmlForm.elements.namedItem("opml");
    if (!(input instanceof HTMLInputElement) || !input.files?.[0]) return;
    const response = await api("/api/v1/sources/rss/opml", { method: "POST", headers: { "content-type": "text/xml" }, body: await input.files[0].text() });
    if (response.ok) { opmlForm.reset(); await reload(); } else report("OPML import failed.");
  });

  if (emailForm instanceof HTMLFormElement) emailForm.addEventListener("submit", async (event) => {
    event.preventDefault(); const data = new FormData(emailForm);
    const folders = String(data.get("folders") ?? "INBOX").split(",").map((value) => value.trim()).filter(Boolean);
    if (await jsonMutation("/api/v1/sources/email", { name: data.get("name"), host: data.get("host"), port: Number(data.get("port")), username: data.get("username"), password: data.get("password"), folders, mailbox: folders[0] ?? "INBOX", markAsRead: data.get("markAsRead") === "on", pollIntervalSeconds: 900 }, "Mailbox could not be added.")) { emailForm.reset(); await reload(); }
  });

  streamForm.addEventListener("submit", async (event) => {
    event.preventDefault(); const data = new FormData(streamForm);
    if (await jsonMutation("/api/v1/streams", { name: data.get("name"), slug: slugify(data.get("name")), filter: { includeTopics: commaValues(data.get("includeTopics")), excludeTopics: commaValues(data.get("excludeTopics")) }, semanticDescription: data.get("semanticDescription") || null, rankingInstruction: data.get("rankingInstruction") || null }, "Stream could not be created.")) { streamForm.reset(); await reload(); }
  });

  if (telegramSourceForm instanceof HTMLFormElement) telegramSourceForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(telegramSourceForm);
    const rawUsername = String(data.get("username") ?? "").trim();
    const username = rawUsername.startsWith("@") ? rawUsername.slice(1) : rawUsername;
    if (await jsonMutation("/api/v1/sources/telegram", { username }, "Telegram source could not be added.")) {
      telegramSourceForm.reset();
      await reload();
    }
  });
  void reload();
}
