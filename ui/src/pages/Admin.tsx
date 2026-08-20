import type { AdminPageModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";

function adminSection(id: string, tab: string, title: string, description: string, children: import("solid-js").JSX.Element) {
  return <section class="admin-panel" id={`admin-panel-${tab}`} role="tabpanel" aria-labelledby={`admin-tab-${tab}`} data-admin-panel={tab} hidden={tab !== "users"}>
    <header class="admin-panel-heading"><h2 id={id}>{title}</h2><p>{description}</p></header>
    {children}
  </section>;
}

export function Admin(props: { page: AdminPageModel }) {
  return ModernShell({
    username: props.page.username,
    activeHref: "/admin",
    children: <div class="admin-page">
      <header class="page-header"><div><h1>{props.page.title}</h1><p>Manage people, sources, providers, jobs, and audit history.</p></div></header>
      <p id="admin-error" role="alert" class="error" hidden></p>
      <div class="settings-tabs admin-tabs" role="tablist" aria-label="Administration">
        <button type="button" role="tab" id="admin-tab-users" aria-controls="admin-panel-users" aria-selected="true" tabindex="0" data-admin-tab="users">Users</button>
        <button type="button" role="tab" id="admin-tab-sources" aria-controls="admin-panel-sources" aria-selected="false" tabindex="-1" data-admin-tab="sources">Sources</button>
        <button type="button" role="tab" id="admin-tab-models" aria-controls="admin-panel-models" aria-selected="false" tabindex="-1" data-admin-tab="models">Models</button>
        <button type="button" role="tab" id="admin-tab-telegram" aria-controls="admin-panel-telegram" aria-selected="false" tabindex="-1" data-admin-tab="telegram">Telegram</button>
        <button type="button" role="tab" id="admin-tab-jobs" aria-controls="admin-panel-jobs" aria-selected="false" tabindex="-1" data-admin-tab="jobs">Jobs</button>
        <button type="button" role="tab" id="admin-tab-audit" aria-controls="admin-panel-audit" aria-selected="false" tabindex="-1" data-admin-tab="audit">Audit</button>
      </div>

      {adminSection("users-heading", "users", "Users", "Create accounts and review active access.", <>
        <form id="user-create" class="admin-form">
          <label>Username <input name="username" required minLength="3" /></label>
          <label>Email <input name="email" type="email" autocomplete="email" /></label>
          <label>Initial password <input name="password" type="password" required minLength="12" autocomplete="new-password" /></label>
          <label>Role <select name="role"><option value="user">User</option><option value="admin">Administrator</option></select></label>
          <button type="submit" class="primary-action">Create user</button>
        </form>
        <div id="user-list" class="loading-region" aria-live="polite"><p>Loading users…</p></div>
      </>)}

      {adminSection("sources-heading", "sources", "Source health", "Current polling state, latest success, and failures.", <div id="source-list" class="loading-region" aria-live="polite"><p>Loading sources…</p></div>)}
      {adminSection("models-heading", "models", "Model providers", "Choose and test the provider used for each AI task.", <div id="model-list" class="loading-region" aria-live="polite"><p>Loading providers…</p></div>)}
      {adminSection("telegram-bot-heading", "telegram", "Telegram bot", "Configure the global bot used for account binding and channel subscriptions.", <div id="telegram-bot-setting" class="loading-region" aria-live="polite"><p>Loading Telegram bot settings…</p></div>)}
      {adminSection("jobs-heading", "jobs", "Jobs", "Recent queue work. Failed jobs include their latest error and retry history.", <div id="job-list" class="loading-region job-list" aria-live="polite"><p>Loading jobs…</p></div>)}
      {adminSection("audit-heading", "audit", "Audit trail", "Recent security and administration events.", <div id="audit-list" class="loading-region" aria-live="polite"><p>Loading events…</p></div>)}
    </div>,
  });
}
