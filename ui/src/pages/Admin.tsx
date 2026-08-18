import type { AdminPageModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";
import { card, cardContent, cardHeader } from "../server/solid-ui";

function adminSection(id: string, title: string, description: string, children: import("solid-js").JSX.Element) {
  return <section aria-labelledby={id}>{card(<>{cardHeader(<><h2 id={id}>{title}</h2><p class="section-description">{description}</p></>, "p-0 pb-5")}{cardContent(children, "p-0")}</>, "admin-section")}</section>;
}

export function Admin(props: { page: AdminPageModel }) {
  return ModernShell({
    username: props.page.username,
    activeHref: "/admin",
    children: <>
      <div class="admin-page">
      <header class="page-header"><div><p class="eyebrow">System</p><h1>{props.page.title}</h1><p>Manage people, providers, jobs, plugins, and audit history.</p></div></header>
      <p id="admin-error" role="alert" class="error" hidden></p>

      {adminSection("users-heading", "Users", "Create accounts and review active access.", <>
        <form id="user-create" class="admin-form">
          <label>Username <input name="username" required minLength="3" /></label>
          <label>Email <input name="email" type="email" autocomplete="email" /></label>
          <label>Initial password <input name="password" type="password" required minLength="12" autocomplete="new-password" /></label>
          <label>Role <select name="role"><option value="user">User</option><option value="admin">Administrator</option></select></label>
          <button type="submit" class="primary-action">Create user</button>
        </form>
        <div id="user-list" class="loading-region" aria-live="polite"><p>Loading users…</p></div>
      </>)}

      {adminSection("sources-heading", "Source health", "Current polling and failure state for connected sources.", <div id="source-list" class="loading-region" aria-live="polite"><p>Loading sources…</p></div>)}

      {adminSection("models-heading", "Model providers", "Configured providers used by ingestion, grouping, and ranking.", <div id="model-list" class="loading-region" aria-live="polite"><p>Loading providers…</p></div>)}

      {adminSection("telegram-bot-heading", "Telegram bot", "Configure the global bot used for account binding and channel subscription control.", <div id="telegram-bot-setting" class="loading-region" aria-live="polite"><p>Loading Telegram bot settings…</p></div>)}

      {adminSection("jobs-heading", "Jobs", "Queue status and retry history.", <div id="job-list" class="loading-region" aria-live="polite"><p>Loading jobs…</p></div>)}

      {adminSection("plugins-heading", "Source plugins", "Install WebAssembly source adapters in a disabled state.", <>
        <form id="plugin-install" class="admin-form">
          <label>WebAssembly component <input id="plugin-component" type="file" accept=".wasm,.wat,application/wasm" required /></label>
          <button type="submit" class="primary-action">Install disabled</button>
        </form>
        <div id="plugin-list"><p>No plugins installed.</p></div>
      </>)}

      {adminSection("audit-heading", "Audit trail", "Recent security and administration events.", <div id="audit-list" class="loading-region" aria-live="polite"><p>Loading events…</p></div>)}
      </div>
    </>,
  });
}
