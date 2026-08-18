import type { ReaderSettingsPageModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";
import { badge, card, cardContent, cardHeader } from "../server/solid-ui";

export function ReaderSettings(props: { page: ReaderSettingsPageModel; csrfToken: string }) {
  return ModernShell({
    username: props.page.username,
    activeHref: "/settings/readers",
    children: <>
      <header class="page-header"><div><p class="eyebrow">Your Rill</p><h1>{props.page.title}</h1><p>Reading, favorite automations, security, and devices—all scoped to your account.</p></div></header>
      <p id="settings-error" role="alert" class="error" hidden></p>
      <section class="reader-mode-callout" aria-labelledby="reader-mode-heading">{card(<>{cardHeader(<><p class="eyebrow">Low-distraction view</p><h2 id="reader-mode-heading">Reader mode</h2><p class="section-description">Open a fast, typography-first feed with no browser JavaScript required.</p></>, "p-0 pb-5")}{cardContent(
        <a class="primary-action" href="/reader">Open reader mode</a>
      , "p-0")}</>, "admin-section")}</section>
      <section aria-labelledby="favorite-actions-heading">{card(<>{cardHeader(<><h2 id="favorite-actions-heading">Favorite actions</h2><p class="section-description">When you favorite a story, Rill can call your private HTTP endpoint. These actions belong only to {props.page.username}.</p></>, "p-0 pb-5")}{cardContent(<>
        <form id="favorite-action-create" class="admin-form">
          <label>Name <input name="name" placeholder="Save to my notes" required /></label>
          <label>Destination URL <input name="url" type="url" placeholder="https://example.com/inbox" required /></label>
          <label>Method <select name="method"><option>POST</option><option>PUT</option><option>PATCH</option></select></label>
          <label>Private headers (JSON) <textarea name="headers" placeholder='{"Authorization":"Bearer …"}' /></label>
          <button type="submit" class="primary-action">Add favorite action</button>
        </form>
        <div id="favorite-action-list" class="loading-region" aria-live="polite"><p>Loading your actions…</p></div>
      </>, "p-0")}</>, "admin-section")}</section>
      <section aria-labelledby="password-heading">{card(<>{cardHeader(<><h2 id="password-heading">Change password</h2><p class="section-description">Changing your password signs out every active session.</p></>, "p-0 pb-5")}{cardContent(
        <form method="post" action="/settings/password" class="admin-form">
          <input type="hidden" name="csrf_token" value={props.csrfToken} />
          <label>Current password <input name="old_password" type="password" autocomplete="current-password" required /></label>
          <label>New password <input name="new_password" type="password" autocomplete="new-password" minlength="12" required /></label>
          <button type="submit" class="primary-action">Change password and sign out everywhere</button>
        </form>
      , "p-0")}</>, "admin-section")}</section>
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
    </>,
  });
}
