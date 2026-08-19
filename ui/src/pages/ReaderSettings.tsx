import type { ReaderSettingsPageModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";
import { badge, card, cardContent, cardHeader } from "../server/solid-ui";

export function ReaderSettings(props: { page: ReaderSettingsPageModel; csrfToken: string }) {
  const defaultTab = props.page.newPairingCode ? "devices" : "reading";
  return ModernShell({
    username: props.page.username,
    activeHref: "/settings/readers",
    children: <>
      <header class="page-header"><div><p class="eyebrow">Your Rill</p><h1>{props.page.title}</h1><p>Reading, favorite automations, security, and devices—all scoped to your account.</p></div></header>
      <p id="settings-error" role="alert" class="error" hidden></p>
      <div class="settings-tabs" role="tablist" aria-label="Account settings">
        <button type="button" role="tab" id="settings-tab-reading" aria-controls="settings-panel-reading" aria-selected={defaultTab === "reading" ? "true" : "false"} tabindex={defaultTab === "reading" ? "0" : "-1"} data-settings-tab="reading">Reading</button>
        <button type="button" role="tab" id="settings-tab-actions" aria-controls="settings-panel-actions" aria-selected="false" tabindex="-1" data-settings-tab="actions">Actions</button>
        <button type="button" role="tab" id="settings-tab-security" aria-controls="settings-panel-security" aria-selected="false" tabindex="-1" data-settings-tab="security">Security</button>
        <button type="button" role="tab" id="settings-tab-devices" aria-controls="settings-panel-devices" aria-selected={defaultTab === "devices" ? "true" : "false"} tabindex={defaultTab === "devices" ? "0" : "-1"} data-settings-tab="devices">Devices</button>
      </div>
      <div class="settings-panel" id="settings-panel-reading" role="tabpanel" aria-labelledby="settings-tab-reading" data-settings-panel="reading" hidden={defaultTab !== "reading"}>
      <section class="reader-mode-callout" aria-labelledby="reader-mode-heading">{card(<>{cardHeader(<><p class="eyebrow">Low-distraction view</p><h2 id="reader-mode-heading">Reader mode</h2><p class="section-description">Open a fast, typography-first feed with no browser JavaScript required.</p></>, "p-0 pb-5")}{cardContent(
        <a class="primary-action" href="/reader">Open reader mode</a>
      , "p-0")}</>, "admin-section")}</section>
      </div>
      <div class="settings-panel" id="settings-panel-actions" role="tabpanel" aria-labelledby="settings-tab-actions" data-settings-panel="actions" hidden>
      <section aria-labelledby="favorite-actions-heading">{card(<>{cardHeader(<><h2 id="favorite-actions-heading">Favorite actions</h2><p class="section-description">When you favorite a story, Rill can call your private HTTP endpoint. These actions belong only to {props.page.username}.</p></>, "p-0 pb-5")}{cardContent(<>
        <form id="favorite-action-create" class="admin-form">
          <label>Name <input name="name" placeholder="Save to my notes" required /></label>
          <label>Destination URL <input name="url" type="url" placeholder="https://example.com/inbox" required /></label>
          <label>Method <select name="method"><option>POST</option><option>PUT</option><option>PATCH</option></select></label>
          <details>
            <summary>Advanced request options</summary>
            <label>JSON body template <textarea name="body_template" placeholder='{"url":"${story.url}","title":"${story.title}"}' /></label>
            <label>Header name <input name="header_name" placeholder="Authorization" /></label>
            <label>Header value environment variable <input name="header_env" placeholder="KARAKEEP_API_TOKEN" /></label>
            <label>Header prefix <input name="header_prefix" placeholder="Bearer " /></label>
            <label>Private headers (JSON) <textarea name="headers" placeholder='{"X-Account":"personal"}' /></label>
          </details>
          <button type="submit" class="primary-action">Add favorite action</button>
        </form>
        <div id="favorite-action-list" class="loading-region" aria-live="polite"><p>Loading your actions…</p></div>
      </>, "p-0")}</>, "admin-section")}</section>
      </div>
      <div class="settings-panel" id="settings-panel-security" role="tabpanel" aria-labelledby="settings-tab-security" data-settings-panel="security" hidden>
      <section aria-labelledby="password-heading">{card(<>{cardHeader(<><h2 id="password-heading">Change password</h2><p class="section-description">Changing your password signs out every active session.</p></>, "p-0 pb-5")}{cardContent(
        <form method="post" action="/settings/password" class="admin-form">
          <input type="hidden" name="csrf_token" value={props.csrfToken} />
          <label>Current password <input name="old_password" type="password" autocomplete="current-password" required /></label>
          <label>New password <input name="new_password" type="password" autocomplete="new-password" minlength="12" required /></label>
          <button type="submit" class="primary-action">Change password and sign out everywhere</button>
        </form>
      , "p-0")}</>, "admin-section")}</section>
      </div>
      <div class="settings-panel" id="settings-panel-devices" role="tabpanel" aria-labelledby="settings-tab-devices" data-settings-panel="devices" hidden={defaultTab !== "devices"}>
      <section aria-labelledby="pair-heading">{card(<>{cardHeader(<><h2 id="pair-heading">Pair a reader</h2><p class="section-description">Create a short-lived code, then enter it on the reader.</p></>, "p-0 pb-5")}{cardContent(<>
        <form method="post" action="/settings/readers/pair" class="admin-form">
          <input type="hidden" name="csrf_token" value={props.csrfToken} />
          <label>Device label <input name="label" maxlength="80" required /></label>
          <button type="submit" class="primary-action">Create one-time code</button>
        </form>
        {props.page.newPairingCode && (
          <p class="pairing-code" role="status">Code: <strong>{props.page.newPairingCode}</strong></p>
        )}
      </>, "p-0")}</>, "admin-section")}</section>
      <section aria-labelledby="devices-heading"><div class="section-heading"><div><p class="eyebrow">Access</p><h2 id="devices-heading">Paired devices</h2></div>{badge(props.page.devices.length, "secondary")}</div>
        {props.page.devices.length === 0 && <p>No paired readers.</p>}
        <div class="device-grid">{props.page.devices.map((device) => (
          card(<>{cardHeader(<><div class="story-source-line">{badge("Reader", "outline")}</div><h3>{device.label}</h3><p class="section-description">{device.userAgent || "Unknown reader"}</p></>)}{cardContent(
            <form method="post" action={`/settings/readers/${device.id}/revoke`}>
              <input type="hidden" name="csrf_token" value={props.csrfToken} />
              <button type="submit" class="danger-action">Revoke reader</button>
            </form>
          )}</>)
        ))}</div>
      </section>
      </div>
    </>,
  });
}
