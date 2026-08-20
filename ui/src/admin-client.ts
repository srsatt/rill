interface UserView {
  id: string; username: string; email: string | null; role: "admin" | "user"; disabled: boolean;
  createdAt: number; activeBrowserSessions: number; activeReaderDevices: number;
}

interface SourceView {
  id: string; kind: string; name: string; visibility: string; enabled: boolean;
  lastSuccessAt: number | null; consecutiveFailures: number; lastErrorMessage: string | null;
}

interface JobAttemptView {
  attemptNumber: number; outcome: string | null; errorMessage: string | null;
}

interface JobView {
  id: string; kind: string; status: string; attemptCount: number; maxAttempts: number;
  lastErrorMessage: string | null; attempts: JobAttemptView[];
}

interface AuditView {
  id: string; eventType: string; userId: string | null; targetType: string | null;
  targetId: string | null; createdAt: number;
}

interface ModelSettingView {
  slot: "embedding" | "ranking" | "text_parse";
  mode: string;
  provider: string;
  model: string;
  version: string;
  baseUrl: string | null;
  apiKeyConfigured: boolean;
}

interface TelegramBotView {
  configured: boolean;
  active: boolean;
  username: string | null;
}

function csrfToken(): string {
  return document.cookie
    .split(";")
    .map((part) => part.trim().split("="))
    .find(([name]) => name === "rill_csrf")?.[1] ?? "";
}

async function api(path: string, init?: RequestInit): Promise<Response> {
  const headers = new Headers(init?.headers);
  if (init?.method && init.method !== "GET") headers.set("x-csrf-token", csrfToken());
  return fetch(path, { ...init, credentials: "same-origin", headers });
}

function node<K extends keyof HTMLElementTagNameMap>(tag: K, text?: string): HTMLElementTagNameMap[K] {
  const value = document.createElement(tag);
  if (text !== undefined) value.textContent = text;
  return value;
}

function report(message: string): void {
  const error = document.getElementById("admin-error");
  if (!(error instanceof HTMLParagraphElement)) return;
  error.textContent = message;
  error.hidden = message.length === 0;
}

function wireTabs(tabs: HTMLButtonElement[], panels: HTMLElement[], valueOf: (element: HTMLElement) => string | undefined): void {
  if (tabs.length === 0 || tabs.length !== panels.length) return;
  const select = (tab: HTMLButtonElement, focus: boolean) => {
    const value = valueOf(tab);
    tabs.forEach((item) => {
      const selected = item === tab;
      item.setAttribute("aria-selected", String(selected));
      item.tabIndex = selected ? 0 : -1;
    });
    panels.forEach((panel) => { panel.hidden = valueOf(panel) !== value; });
    if (focus) tab.focus();
  };
  tabs.forEach((tab, index) => {
    tab.addEventListener("click", () => select(tab, false));
    tab.addEventListener("keydown", (event) => {
      const target = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1
        : event.key === "ArrowRight" ? (index + 1) % tabs.length
          : event.key === "ArrowLeft" ? (index - 1 + tabs.length) % tabs.length : -1;
      if (target < 0) return;
      event.preventDefault(); select(tabs[target], true);
    });
  });
}

async function mutate(path: string, init: RequestInit, message: string): Promise<boolean> {
  const response = await api(path, init);
  if (!response.ok) report(message);
  return response.ok;
}

function userCard(user: UserView, reload: () => Promise<void>): HTMLElement {
  const card = node("article");
  card.className = "admin-card";
  card.append(
    node("h3", user.username),
    node("p", `${user.email ?? "No email"} · ${user.role}${user.disabled ? " · disabled" : ""}`),
    node("p", `${user.activeBrowserSessions} browser sessions · ${user.activeReaderDevices} readers`),
  );
  const role = node("button", user.role === "admin" ? "Make user" : "Make administrator");
  role.type = "button";
  role.addEventListener("click", async () => {
    if (await mutate(`/api/v1/admin/users/${user.id}/role`, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ role: user.role === "admin" ? "user" : "admin" }),
    }, "User role could not be changed.")) await reload();
  });
  const disabled = node("button", user.disabled ? "Enable" : "Disable");
  disabled.type = "button";
  disabled.addEventListener("click", async () => {
    if (await mutate(`/api/v1/admin/users/${user.id}/disabled`, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ disabled: !user.disabled }),
    }, "User state could not be changed.")) await reload();
  });
  card.append(role, disabled);
  return card;
}

function sourceCard(source: SourceView): HTMLElement {
  const card = node("article");
  card.className = "admin-card";
  const health = source.lastErrorMessage
    ? `${source.consecutiveFailures} failures · ${source.lastErrorMessage}`
    : source.lastSuccessAt ? `Last success ${new Date(source.lastSuccessAt * 1000).toLocaleString()}` : "Never fetched";
  const status = node("span", !source.enabled ? "Disabled" : source.lastErrorMessage ? "Failing" : source.lastSuccessAt ? "Healthy" : "Never fetched");
  status.className = `status-tag ${!source.enabled ? "status-muted" : source.lastErrorMessage ? "status-error" : source.lastSuccessAt ? "status-success" : "status-warning"}`;
  const heading = node("div"); heading.className = "admin-card-heading"; heading.append(node("h3", source.name), status);
  card.append(heading, node("p", `${source.kind} · ${source.visibility}`), node("p", health));
  return card;
}

function modelSettingCard(setting: ModelSettingView, reload: () => Promise<void>): HTMLElement {
  const title = setting.slot === "ranking" ? "Ranking model" : setting.slot === "embedding" ? "Embedding model" : "Text parsing model";
  const card = node("article");
  card.className = "admin-card model-setting-card";
  const heading = node("h3", title);
  const status = node("p", `${setting.mode} · ${setting.provider}/${setting.model} · ${setting.version}${setting.apiKeyConfigured ? " · API key stored" : " · no API key"}`);
  card.append(heading, status);

  const form = node("form");
  form.className = "admin-form compact model-setting-form";
  const field = (labelText: string, name: string, value: string, type = "text") => {
    const label = node("label", labelText);
    const input = node("input");
    input.name = name;
    input.type = type;
    input.value = value;
    input.required = name !== "apiKey";
    if (name === "apiKey") {
      input.autocomplete = "new-password";
      input.placeholder = setting.apiKeyConfigured ? "Leave blank to keep the stored key" : "Optional API key";
    }
    label.append(input);
    return label;
  };
  const presets = {
    openai: ["openai", "https://api.openai.com/v1/"],
    claude: ["claude", "https://api.anthropic.com/v1/"],
    gemini: ["gemini", "https://generativelanguage.googleapis.com/v1beta/openai/"],
    ollama: ["ollama", "http://127.0.0.1:11434/v1/"],
    custom: ["openai-compatible", setting.baseUrl ?? ""],
  } as const;
  const activePreset = Object.keys(presets).find((key) => presets[key as keyof typeof presets][0] === setting.provider) as keyof typeof presets | undefined;
  const presetLabel = node("label", "Provider preset");
  const preset = node("select"); preset.name = "preset";
  for (const [value, label] of [["openai", "OpenAI"], ["claude", "Claude"], ["gemini", "Gemini"], ["ollama", "Ollama"], ["custom", "Custom OpenAI-compatible"]]) {
    const option = node("option", label); option.value = value; preset.append(option);
  }
  preset.value = activePreset ?? "custom";
  presetLabel.append(preset);
  const baseUrl = field("Base URL", "baseUrl", setting.baseUrl ?? presets[activePreset ?? "custom"][1], "url");
  const provider = field("Provider identifier", "provider", setting.provider);
  const model = field("Model", "model", setting.model);
  const defaults = {
    embedding: { openai: "text-embedding-3-small", claude: "voyage-3", gemini: "text-embedding-004", ollama: "nomic-embed-text" },
    ranking: { openai: "gpt-4.1-mini", claude: "claude-sonnet-4-5", gemini: "gemini-2.5-flash", ollama: "qwen3" },
    text_parse: { openai: "gpt-4.1-mini", claude: "claude-sonnet-4-5", gemini: "gemini-2.5-flash", ollama: "qwen3" },
  } as const;
  preset.addEventListener("change", () => {
    const choice = preset.value as keyof typeof presets;
    const providerInput = provider.querySelector("input");
    const baseInput = baseUrl.querySelector("input");
    const modelInput = model.querySelector("input");
    if (providerInput) providerInput.value = presets[choice][0];
    if (baseInput) baseInput.value = presets[choice][1];
    if (modelInput && choice !== "custom") modelInput.value = defaults[setting.slot][choice];
  });
  form.append(
    presetLabel,
    baseUrl,
    provider,
    model,
    field("Version", "version", setting.version),
    field("API token", "apiKey", "", "password"),
  );
  const clearLabel = node("label");
  clearLabel.className = "checkbox-label";
  const clear = node("input");
  clear.type = "checkbox";
  clear.name = "clearApiKey";
  clear.disabled = !setting.apiKeyConfigured;
  clearLabel.append(clear, " Remove the stored API key");
  const formBody = () => {
    const data = new FormData(form);
    const body: Record<string, unknown> = {
      baseUrl: data.get("baseUrl"), provider: data.get("provider"), model: data.get("model"),
      version: data.get("version"), clearApiKey: data.get("clearApiKey") === "on",
    };
    const apiKey = String(data.get("apiKey") ?? "");
    if (apiKey) body.apiKey = apiKey;
    return body;
  };
  const actions = node("div"); actions.className = "model-form-actions";
  const test = node("button", "Test model");
  test.type = "button";
  const testStatus = node("span"); testStatus.className = "model-test-status"; testStatus.setAttribute("role", "status");
  test.addEventListener("click", async () => {
    test.disabled = true; testStatus.textContent = "Testing…";
    const response = await api(`/api/v1/admin/settings/models/${setting.slot}/test`, {
      method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(formBody()),
    });
    test.disabled = false;
    if (!response.ok) { testStatus.textContent = "Test failed."; return; }
    const health = await response.json() as { ready: boolean; detail: string };
    testStatus.textContent = health.ready ? `Ready · ${health.detail}` : `Unavailable · ${health.detail}`;
  });
  const save = node("button", `Save ${title.toLowerCase()}`);
  save.type = "submit";
  save.className = "primary-action";
  const reset = node("button", `Reset ${title.toLowerCase()}`);
  reset.type = "button";
  reset.className = "secondary-action";
  reset.addEventListener("click", async () => {
    if (await mutate(`/api/v1/admin/settings/models/${setting.slot}`, { method: "DELETE" }, `${title} could not be reset.`)) await reload();
  });
  actions.append(test, save, reset, testStatus);
  form.append(clearLabel, actions);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (await mutate(`/api/v1/admin/settings/models/${setting.slot}`, {
      method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify(formBody()),
    }, `${title} could not be saved.`)) await reload();
  });
  card.append(form);
  return card;
}

function modelSettings(models: ModelSettingView[], reload: () => Promise<void>): HTMLElement[] {
  const labels = { ranking: "Ranking", embedding: "Embedding", text_parse: "Text parsing" } as const;
  const tabs = node("div"); tabs.className = "settings-tabs model-tabs"; tabs.setAttribute("role", "tablist"); tabs.setAttribute("aria-label", "Model task");
  const panels: HTMLElement[] = [];
  const buttons: HTMLButtonElement[] = [];
  models.forEach((model, index) => {
    const button = node("button", labels[model.slot]);
    button.type = "button"; button.setAttribute("role", "tab"); button.dataset.modelTab = model.slot;
    button.setAttribute("aria-selected", String(index === 0)); button.tabIndex = index === 0 ? 0 : -1;
    tabs.append(button); buttons.push(button);
    const panel = node("section"); panel.setAttribute("role", "tabpanel"); panel.dataset.modelPanel = model.slot; panel.hidden = index !== 0;
    panel.append(modelSettingCard(model, reload)); panels.push(panel);
  });
  wireTabs(buttons, panels, (element) => element.dataset.modelTab ?? element.dataset.modelPanel);
  return [tabs, ...panels];
}

function telegramBotSetting(view: TelegramBotView, reload: () => Promise<void>): HTMLElement {
  const card = node("article");
  card.className = "admin-card telegram-bot-card";
  const status = view.active
    ? `Active as @${view.username ?? "configured bot"}`
    : view.configured ? "Configured, but not active" : "Not configured";
  card.append(node("h3", "Bot token"), node("p", status), node("p", "The token is encrypted at rest and is never returned to this page."));

  const form = node("form");
  form.className = "admin-form compact";
  const label = node("label", view.configured ? "Replacement token" : "Bot token");
  const token = node("input");
  token.name = "token";
  token.type = "password";
  token.autocomplete = "new-password";
  token.required = true;
  token.placeholder = view.configured ? "Enter a new token to replace the stored token" : "Paste the token from BotFather";
  label.append(token);
  const save = node("button", view.configured ? "Replace bot token" : "Configure bot token");
  save.type = "submit";
  save.className = "primary-action";
  form.append(label, save);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (await mutate("/api/v1/admin/settings/telegram-bot", {
      method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ token: token.value }),
    }, "Telegram bot token could not be saved.")) {
      form.reset();
      await reload();
    }
  });
  card.append(form);

  if (view.configured) {
    const remove = node("button", "Remove bot token");
    remove.type = "button";
    remove.className = "danger-action";
    remove.addEventListener("click", async () => {
      if (await mutate("/api/v1/admin/settings/telegram-bot", { method: "DELETE" }, "Telegram bot token could not be removed.")) await reload();
    });
    card.append(remove);
  }
  return card;
}

function jobCard(job: JobView, reload: () => Promise<void>): HTMLElement {
  const card = node("article");
  card.className = "admin-card";
  card.append(node("h3", job.kind), node("p", `${job.status} · attempt ${job.attemptCount}/${job.maxAttempts} · ${job.id}`));
  if (job.lastErrorMessage) card.append(node("p", job.lastErrorMessage));
  if (job.attempts.length) {
    const details = node("details"); details.className = "job-attempts"; details.append(node("summary", `${job.attempts.length} recorded attempt${job.attempts.length === 1 ? "" : "s"}`));
    const list = node("ol");
    job.attempts.forEach((attempt) => {
      list.append(node("li", `#${attempt.attemptNumber}: ${attempt.outcome ?? "running"}${attempt.errorMessage ? ` · ${attempt.errorMessage}` : ""}`));
    });
    details.append(list); card.append(details);
  }
  if (job.status === "dead") {
    const retry = node("button", "Retry"); retry.type = "button";
    retry.addEventListener("click", async () => {
      if (await mutate(`/api/v1/admin/jobs/${job.id}/retry`, { method: "POST" }, "Job could not be retried.")) await reload();
    });
    card.append(retry);
  } else if (job.status === "queued") {
    const cancel = node("button", "Cancel"); cancel.type = "button";
    cancel.addEventListener("click", async () => {
      if (await mutate(`/api/v1/admin/jobs/${job.id}/cancel`, { method: "POST" }, "Job could not be cancelled.")) await reload();
    });
    card.append(cancel);
  }
  return card;
}

export function activateAdmin(): void {
  const tabs = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-admin-tab]"));
  const panels = Array.from(document.querySelectorAll<HTMLElement>("[data-admin-panel]"));
  wireTabs(tabs, panels, (element) => element.dataset.adminTab ?? element.dataset.adminPanel);
  const userForm = document.getElementById("user-create");
  const userList = document.getElementById("user-list");
  const sourceList = document.getElementById("source-list");
  const modelList = document.getElementById("model-list");
  const telegramBot = document.getElementById("telegram-bot-setting");
  const jobList = document.getElementById("job-list");
  const auditList = document.getElementById("audit-list");
  if (!(userForm instanceof HTMLFormElement)
    || !(userList instanceof HTMLDivElement) || !(sourceList instanceof HTMLDivElement)
    || !(modelList instanceof HTMLDivElement) || !(telegramBot instanceof HTMLDivElement) || !(jobList instanceof HTMLDivElement)
    || !(auditList instanceof HTMLDivElement)) return;

  const reload = async () => {
    const [userResponse, sourceResponse, modelResponse, telegramBotResponse, jobResponse, auditResponse] = await Promise.all([
      api("/api/v1/admin/users"), api("/api/v1/sources"),
      api("/api/v1/admin/models"), api("/api/v1/admin/settings/telegram-bot"), api("/api/v1/admin/jobs?limit=50"), api("/api/v1/admin/audit?limit=100"),
    ]);
    if ([userResponse, sourceResponse, modelResponse, telegramBotResponse, jobResponse, auditResponse].some((response) => !response.ok)) { report("Administration data could not be loaded."); return; }
    const users = await userResponse.json() as UserView[];
    const sources = await sourceResponse.json() as SourceView[];
    const models = await modelResponse.json() as ModelSettingView[];
    const telegramBotView = await telegramBotResponse.json() as TelegramBotView;
    const jobs = await jobResponse.json() as JobView[];
    const audit = await auditResponse.json() as AuditView[];
    userList.replaceChildren(...users.map((user) => userCard(user, reload)));
    sourceList.replaceChildren(...(sources.length ? sources.map(sourceCard) : [node("p", "No sources configured.")]));
    const modelOrder = { ranking: 0, embedding: 1, text_parse: 2 } as const;
    modelList.replaceChildren(...modelSettings(models.sort((left, right) => modelOrder[left.slot] - modelOrder[right.slot]), reload));
    telegramBot.replaceChildren(telegramBotSetting(telegramBotView, reload));
    jobList.replaceChildren(...(jobs.length ? jobs.map((job) => jobCard(job, reload)) : [node("p", "No jobs.")]));
    auditList.replaceChildren(...audit.map((event) => {
      const card = node("article"); card.className = "admin-card";
      card.append(node("h3", event.eventType), node("p", `${new Date(event.createdAt * 1000).toLocaleString()} · ${event.targetType ?? "system"} ${event.targetId ?? ""}`)); return card;
    }));
    report("");
  };

  userForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(userForm);
    if (await mutate("/api/v1/admin/users", {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ username: data.get("username"), email: data.get("email") || null,
        password: data.get("password"), role: data.get("role") }),
    }, "User could not be created.")) { userForm.reset(); await reload(); }
  });

  void reload();
}
