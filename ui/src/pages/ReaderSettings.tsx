import type { ReaderSettingsPageModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";

export function ReaderSettings(props: { page: ReaderSettingsPageModel; csrfToken: string }) {
  const defaultTab = props.page.newPairingCode ? "devices" : "experience";
  return ModernShell({
    username: props.page.username,
    activeHref: "/settings/readers",
    children: <>
      <header class="page-header"><div><h1>{props.page.title}</h1><p>Reading preferences, streams, automations, security, and devices.</p></div></header>
      <p id="settings-error" role="alert" class="error" hidden></p>
      <div class="settings-tabs" role="tablist" aria-label="Account settings">
        <button type="button" role="tab" id="settings-tab-experience" aria-controls="settings-panel-experience" aria-selected={defaultTab === "experience" ? "true" : "false"} tabindex={defaultTab === "experience" ? "0" : "-1"} data-settings-tab="experience">Experience</button>
        <button type="button" role="tab" id="settings-tab-streams" aria-controls="settings-panel-streams" aria-selected="false" tabindex="-1" data-settings-tab="streams">Streams</button>
        <button type="button" role="tab" id="settings-tab-actions" aria-controls="settings-panel-actions" aria-selected="false" tabindex="-1" data-settings-tab="actions">Actions</button>
        <button type="button" role="tab" id="settings-tab-security" aria-controls="settings-panel-security" aria-selected="false" tabindex="-1" data-settings-tab="security">Security</button>
        <button type="button" role="tab" id="settings-tab-devices" aria-controls="settings-panel-devices" aria-selected={defaultTab === "devices" ? "true" : "false"} tabindex={defaultTab === "devices" ? "0" : "-1"} data-settings-tab="devices">Devices</button>
      </div>
      <div class="settings-panel" id="settings-panel-experience" role="tabpanel" aria-labelledby="settings-tab-experience" data-settings-panel="experience" hidden={defaultTab !== "experience"}>
      <section class="settings-form-section" aria-labelledby="experience-heading">
        <header><h2 id="experience-heading">Feed behavior</h2><p class="section-description">Choose how Rill presents and assigns stories for this account.</p></header>
        <form id="user-preferences" class="admin-form">
          <div class="settings-field-group">
            <label class="checkbox-label"><input name="aiFreeMode" type="checkbox" /> AI-free mode</label>
            <p class="field-help">Hide generated summaries and topics, and use deterministic freshness ordering.</p>
          </div>
          <div class="settings-field-group">
            <label>Stream assignment
              <select name="streamMembershipMode">
                <option value="multiple">A story can appear in multiple streams</option>
                <option value="exclusive">Assign to the first matching subject stream</option>
              </select>
            </label>
            <p class="field-help">All always remains a complete view.</p>
          </div>
          <div class="settings-field-group">
            <label>Typography
              <select name="fontFamily">
                <option value="sans">Sans serif</option>
                <option value="serif">Serif</option>
              </select>
            </label>
          </div>
          <div class="settings-field-group">
            <label>Article instructions
              <textarea name="processingPrompt" maxlength="4000" placeholder="Keep Russian and English unchanged. Translate German into English. Exclude celebrity news." />
            </label>
            <p class="field-help">Applied to articles from every source. Source-specific instructions still apply.</p>
          </div>
          <button type="submit" class="primary-action">Save preferences</button>
          <p id="preferences-status" class="field-help" role="status" aria-live="polite"></p>
        </form>
      </section>
      </div>
      <div class="settings-panel" id="settings-panel-streams" role="tabpanel" aria-labelledby="settings-tab-streams" data-settings-panel="streams" hidden>
      <section class="settings-collection" aria-labelledby="streams-heading">
        <header class="settings-collection-header"><div><h2 id="streams-heading">Streams</h2><p>Choose a stream to preview or edit. All always stays first.</p></div><button id="add-stream" type="button" class="primary-action">Add stream</button></header>
        <div class="settings-browser">
          <div id="stream-list" class="settings-list loading-region" role="listbox" aria-label="Streams" aria-live="polite"><p>Loading streams…</p></div>
          <div id="stream-detail" class="settings-detail">
          <div id="stream-create-detail">
          <h3>New stream</h3>
          <form id="stream-create" class="admin-form stream-create-form">
          <label>Stream name <input name="name" placeholder="Local life" required /></label>
          <label>What belongs here? <textarea name="semanticDescription" placeholder="Local events, transport, city policy, and useful neighborhood news." /></label>
          <label>What should rank higher? <textarea name="rankingInstruction" placeholder="Prefer practical changes and original reporting over opinion." /></label>
          <details class="advanced-options"><summary>Advanced tag filters</summary><div class="admin-form">
            <label>Include topics <input name="includeTopics" placeholder="transport, local politics" /></label>
            <label>Exclude topics <input name="excludeTopics" placeholder="sponsored, cryptocurrency" /></label>
          </div></details>
          <button class="primary-action">Create stream</button>
        </form>
          </div>
          </div>
        </div>
      </section>
      </div>
      <div class="settings-panel" id="settings-panel-actions" role="tabpanel" aria-labelledby="settings-tab-actions" data-settings-panel="actions" hidden>
      <section class="settings-collection" aria-labelledby="favorite-actions-heading">
        <header class="settings-collection-header"><div><h2 id="favorite-actions-heading">Actions</h2><p>Choose an action to inspect or edit what happens after a favorite.</p></div><button id="add-favorite-action" type="button" class="primary-action">Add action</button></header>
        <div class="settings-browser">
          <div id="favorite-action-list" class="settings-list loading-region" role="listbox" aria-label="Actions" aria-live="polite"><p>Loading actions…</p></div>
          <div id="favorite-action-detail" class="settings-detail">
          <div id="favorite-action-create-detail">
          <h3>New action</h3>
        <form id="favorite-action-create" class="admin-form">
          <label>Name <input name="name" placeholder="Save to my notes" required /></label>
          <label>Destination URL <input name="url" type="url" placeholder="https://example.com/inbox" required /></label>
          <label>Method <select name="method"><option>POST</option><option>PUT</option><option>PATCH</option></select></label>
          <details>
            <summary>Advanced request options</summary>
            <label>JSON body template <textarea name="body_template" placeholder='{"url":"${story.url}","title":"${story.title}"}' /></label>
            <label>Private header name <input name="header_name" placeholder="Authorization" /></label>
            <label>Private header value <input name="header_value" type="password" autocomplete="new-password" placeholder="Bearer token" /></label>
            <label>Additional private headers (JSON) <textarea name="headers" placeholder='{"X-Account":"personal"}' /></label>
          </details>
          <button type="submit" class="primary-action">Add favorite action</button>
        </form>
          </div>
          </div>
        </div>
      </section>
      </div>
      <div class="settings-panel" id="settings-panel-security" role="tabpanel" aria-labelledby="settings-tab-security" data-settings-panel="security" hidden>
      <section class="settings-form-section" aria-labelledby="password-heading">
        <header><h2 id="password-heading">Change password</h2><p class="section-description">Changing your password signs out every active session.</p></header>
        <form method="post" action="/settings/password" class="admin-form">
          <input type="hidden" name="csrf_token" value={props.csrfToken} />
          <label>Current password <input name="old_password" type="password" autocomplete="current-password" required /></label>
          <label>New password <input name="new_password" type="password" autocomplete="new-password" minlength="12" required /></label>
          <button type="submit" class="primary-action">Change password and sign out everywhere</button>
        </form>
      </section>
      </div>
      <div class="settings-panel" id="settings-panel-devices" role="tabpanel" aria-labelledby="settings-tab-devices" data-settings-panel="devices" hidden={defaultTab !== "devices"}>
      <section class="settings-collection" aria-labelledby="devices-heading">
        <header class="settings-collection-header"><div><h2 id="devices-heading">Devices</h2><p>Choose a reader to inspect or pair a new one.</p></div><button type="button" class="primary-action" data-settings-select="device-pair">Add device</button></header>
        <div class="settings-browser">
          <div class="settings-list" role="listbox" aria-label="Devices">
            {props.page.devices.length === 0 ? <p class="settings-empty">No paired readers.</p> : props.page.devices.map((device) => (
              <button type="button" role="option" aria-selected="false" data-settings-select={`device-${device.id}`}><strong>{device.label}</strong><span>{device.userAgent || "Reader"}</span></button>
            ))}
          </div>
          <div class="settings-detail">
          <div data-settings-detail="device-pair" hidden={props.page.devices.length > 0 && !props.page.newPairingCode}>
            <h3>Pair a reader</h3>
            <p class="field-help">Create a short-lived code, then enter it on the reader.</p>
            <form method="post" action="/settings/readers/pair" class="admin-form">
          <input type="hidden" name="csrf_token" value={props.csrfToken} />
          <label>Device label <input name="label" maxlength="80" required /></label>
          <button type="submit" class="primary-action">Create one-time code</button>
        </form>
        {props.page.newPairingCode && (
          <p class="pairing-code" role="status">Code: <strong>{props.page.newPairingCode}</strong></p>
        )}
          </div>
          {props.page.devices.map((device, index) => (
            <div data-settings-detail={`device-${device.id}`} hidden={Boolean(props.page.newPairingCode) || index !== 0}>
              <h3>{device.label}</h3>
              <p class="field-help">{device.userAgent || "Unknown reader"}</p>
            <form method="post" action={`/settings/readers/${device.id}/revoke`}>
              <input type="hidden" name="csrf_token" value={props.csrfToken} />
              <button type="submit" class="danger-action">Revoke reader</button>
            </form>
            </div>
          ))}
          </div>
        </div>
      </section>
      </div>
    </>,
  });
}
