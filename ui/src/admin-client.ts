interface PluginPermission {
  capability: string;
  constraint: unknown;
}

interface PluginView {
  installationId: string;
  metadata: {
    name: string;
    version: string;
    description: string;
    requestedPermissions: string[];
  };
  componentSha256: string;
  configSchema: unknown;
  enabled: boolean;
  grantedPermissions: PluginPermission[];
  consecutiveFailures: number;
  lastErrorMessage: string | null;
}

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

async function mutate(path: string, init: RequestInit, message: string): Promise<boolean> {
  const response = await api(path, init);
  if (!response.ok) report(message);
  return response.ok;
}

function labeledInput(label: string, name: string, placeholder: string): HTMLLabelElement {
  const wrapper = node("label", label);
  const input = node("input");
  input.name = name;
  input.placeholder = placeholder;
  input.required = true;
  wrapper.append(input);
  return wrapper;
}

function pluginCard(plugin: PluginView, reload: () => Promise<void>): HTMLElement {
  const card = node("article");
  card.className = "admin-card";
  card.append(
    node("h3", `${plugin.metadata.name} ${plugin.metadata.version}`),
    node("p", plugin.metadata.description),
    node("p", `Component SHA-256: ${plugin.componentSha256}`),
    node("p", `Requested permissions: ${plugin.metadata.requestedPermissions.join(", ") || "none"}`),
    node("p", `Granted permissions: ${plugin.grantedPermissions.map((item) => item.capability).join(", ") || "none"}`),
    node("p", `Health: ${plugin.consecutiveFailures} failures${plugin.lastErrorMessage ? ` · ${plugin.lastErrorMessage}` : ""}`),
    node("pre", JSON.stringify(plugin.configSchema, null, 2)),
  );
  const toggle = node("button", plugin.enabled ? "Disable" : "Enable");
  toggle.type = "button";
  toggle.addEventListener("click", async () => {
    if (await mutate(`/api/v1/plugins/${plugin.installationId}/enabled`, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ enabled: !plugin.enabled }),
    }, "Plugin state could not be changed.")) await reload();
  });
  const remove = node("button", "Remove");
  remove.type = "button";
  remove.addEventListener("click", async () => {
    if (await mutate(`/api/v1/plugins/${plugin.installationId}`, { method: "DELETE" },
      "Plugin could not be removed; remove its source instances first.")) await reload();
  });
  card.append(toggle, remove);

  const permission = node("form");
  permission.className = "admin-form compact";
  permission.append(
    labeledInput("Capability", "capability", "http or secret:name"),
    labeledInput("Constraint JSON", "constraint", '{"hosts":["api.example.com"]}'),
    node("button", "Grant permission"),
  );
  permission.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(permission);
    let constraint: unknown;
    try { constraint = JSON.parse(String(data.get("constraint"))); }
    catch { report("Permission constraint must be JSON."); return; }
    if (await mutate(`/api/v1/plugins/${plugin.installationId}/permissions`, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ capability: data.get("capability"), constraint }),
    }, "Permission could not be granted.")) await reload();
  });
  card.append(permission);

  const source = node("form");
  source.className = "admin-form compact";
  const sourceConfig = labeledInput("Configuration JSON", "config", "{}");
  source.append(
    labeledInput("Source name", "name", "My plugin source"),
    sourceConfig,
    node("button", "Create private source"),
  );
  source.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(source);
    let config: unknown;
    try { config = JSON.parse(String(data.get("config"))); }
    catch { report("Source configuration must be JSON."); return; }
    if (await mutate(`/api/v1/plugins/${plugin.installationId}/sources`, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: data.get("name"), config, pollIntervalSeconds: 900, shared: false }),
    }, "Plugin source could not be configured.")) await reload();
  });
  card.append(source);
  return card;
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
  card.append(node("h3", source.name), node("p", `${source.kind} · ${source.visibility} · ${source.enabled ? "enabled" : "disabled"}`), node("p", health));
  return card;
}

function modelSettingCard(setting: ModelSettingView, reload: () => Promise<void>): HTMLElement {
  const title = setting.slot === "ranking" ? "Ranking model" : setting.slot === "embedding" ? "Embedding model" : "Text parsing model";
  const card = node("article");
  card.className = `admin-card model-setting-card${setting.slot === "ranking" ? " ranking-model-card" : ""}`;
  const heading = node("h3", title);
  const status = node("p", `${setting.mode} · ${setting.provider}/${setting.model} · ${setting.version}${setting.apiKeyConfigured ? " · API key stored" : " · no API key"}`);
  if (setting.slot === "ranking") {
    card.append(heading, node("p", "Orders each stream using its ranking instruction, source signals, and explicit user feedback."), status);
  } else {
    card.append(heading, status);
  }

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
  form.append(
    field("Base URL", "baseUrl", setting.baseUrl ?? "", "url"),
    field("Provider", "provider", setting.provider),
    field("Model", "model", setting.model),
    field("Version", "version", setting.version),
    field("API key", "apiKey", "", "password"),
  );
  const clearLabel = node("label");
  clearLabel.className = "checkbox-label";
  const clear = node("input");
  clear.type = "checkbox";
  clear.name = "clearApiKey";
  clear.disabled = !setting.apiKeyConfigured;
  clearLabel.append(clear, " Remove the stored API key");
  const save = node("button", `Save ${title.toLowerCase()}`);
  save.type = "submit";
  save.className = "primary-action";
  form.append(clearLabel, save);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(form);
    const apiKey = String(data.get("apiKey") ?? "");
    const body: Record<string, unknown> = {
      baseUrl: data.get("baseUrl"),
      provider: data.get("provider"),
      model: data.get("model"),
      version: data.get("version"),
      clearApiKey: data.get("clearApiKey") === "on",
    };
    if (apiKey) body.apiKey = apiKey;
    if (await mutate(`/api/v1/admin/settings/models/${setting.slot}`, {
      method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify(body),
    }, `${title} could not be saved.`)) await reload();
  });
  card.append(form);

  const reset = node("button", `Reset ${title.toLowerCase()}`);
  reset.type = "button";
  reset.className = "secondary-action";
  reset.addEventListener("click", async () => {
    if (await mutate(`/api/v1/admin/settings/models/${setting.slot}`, { method: "DELETE" }, `${title} could not be reset.`)) await reload();
  });
  card.append(reset);
  return card;
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
  card.append(node("h3", job.kind), node("p", `${job.status} · attempt ${job.attemptCount}/${job.maxAttempts}`));
  if (job.lastErrorMessage) card.append(node("p", job.lastErrorMessage));
  if (job.attempts.length) card.append(node("p", `Latest: ${job.attempts[0].outcome ?? "running"}${job.attempts[0].errorMessage ? ` · ${job.attempts[0].errorMessage}` : ""}`));
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
  const pluginList = document.getElementById("plugin-list");
  const installForm = document.getElementById("plugin-install");
  const componentInput = document.getElementById("plugin-component");
  const userForm = document.getElementById("user-create");
  const userList = document.getElementById("user-list");
  const sourceList = document.getElementById("source-list");
  const modelList = document.getElementById("model-list");
  const telegramBot = document.getElementById("telegram-bot-setting");
  const jobList = document.getElementById("job-list");
  const auditList = document.getElementById("audit-list");
  if (!(pluginList instanceof HTMLDivElement)
    || !(installForm instanceof HTMLFormElement) || !(componentInput instanceof HTMLInputElement)
    || !(userForm instanceof HTMLFormElement)
    || !(userList instanceof HTMLDivElement) || !(sourceList instanceof HTMLDivElement)
    || !(modelList instanceof HTMLDivElement) || !(telegramBot instanceof HTMLDivElement) || !(jobList instanceof HTMLDivElement)
    || !(auditList instanceof HTMLDivElement)) return;

  const reload = async () => {
    const [pluginResponse, userResponse, sourceResponse, modelResponse, telegramBotResponse, jobResponse, auditResponse] = await Promise.all([
      api("/api/v1/plugins"), api("/api/v1/admin/users"), api("/api/v1/sources"),
      api("/api/v1/admin/models"), api("/api/v1/admin/settings/telegram-bot"), api("/api/v1/admin/jobs?limit=50"), api("/api/v1/admin/audit?limit=100"),
    ]);
    if ([pluginResponse, userResponse, sourceResponse, modelResponse, telegramBotResponse, jobResponse, auditResponse].some((response) => !response.ok)) { report("Administration data could not be loaded."); return; }
    const plugins = await pluginResponse.json() as PluginView[];
    const users = await userResponse.json() as UserView[];
    const sources = await sourceResponse.json() as SourceView[];
    const models = await modelResponse.json() as ModelSettingView[];
    const telegramBotView = await telegramBotResponse.json() as TelegramBotView;
    const jobs = await jobResponse.json() as JobView[];
    const audit = await auditResponse.json() as AuditView[];
    pluginList.replaceChildren(...(plugins.length ? plugins.map((plugin) => pluginCard(plugin, reload)) : [node("p", "No plugins installed.")]));
    userList.replaceChildren(...users.map((user) => userCard(user, reload)));
    sourceList.replaceChildren(...(sources.length ? sources.map(sourceCard) : [node("p", "No sources configured.")]));
    const modelOrder = { ranking: 0, embedding: 1, text_parse: 2 } as const;
    modelList.replaceChildren(...models.sort((left, right) => modelOrder[left.slot] - modelOrder[right.slot]).map((model) => modelSettingCard(model, reload)));
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

  installForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const file = componentInput.files?.[0];
    if (!file) return;
    if (await mutate("/api/v1/plugins/install", { method: "POST", headers: { "content-type": "application/wasm" }, body: file },
      "Component installation failed.")) await reload();
  });

  void reload();
}
