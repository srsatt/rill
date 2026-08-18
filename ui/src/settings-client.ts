interface ActionView {
  id: string;
  name: string;
  enabled: boolean;
  event: string;
  config: { url: string; method: string; bodyTemplate?: unknown };
  hasHeaders: boolean;
}

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

function actionCard(action: ActionView, reload: () => Promise<void>): HTMLElement {
  const card = node("article");
  card.className = "admin-card favorite-action-card";
  card.append(
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
  card.append(toggle, remove);
  return card;
}

export function activateUserSettings(): void {
  const form = document.getElementById("favorite-action-create");
  const list = document.getElementById("favorite-action-list");
  if (!(form instanceof HTMLFormElement) || !(list instanceof HTMLDivElement)) return;

  const reload = async () => {
    const response = await api("/api/v1/actions");
    if (!response.ok) {
      report("Favorite actions could not be loaded.");
      return;
    }
    const actions = await response.json() as ActionView[];
    list.replaceChildren(...(actions.length
      ? actions.map((action) => actionCard(action, reload))
      : [node("p", "No favorite actions yet. Favoriting stays inside Rill until you add one.")]));
    report("");
  };

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(form);
    let headers: Record<string, string>;
    let bodyTemplate: unknown = null;
    try {
      headers = JSON.parse(String(data.get("headers") || "{}")) as Record<string, string>;
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
    const headerEnv: Record<string, { env: string; prefix: string }> = {};
    const headerEnvName = String(data.get("header_env") || "").trim();
    const headerName = String(data.get("header_name") || "").trim();
    if (headerEnvName) {
      if (!headerName) {
        report("Header name is required with an environment variable.");
        return;
      }
      headerEnv[headerName] = {
        env: headerEnvName,
        prefix: String(data.get("header_prefix") || ""),
      };
    }
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
        headerEnv,
        enabled: true,
      }),
    });
    if (!response.ok) {
      report("Favorite action could not be created.");
      return;
    }
    form.reset();
    await reload();
  });

  void reload();
}
