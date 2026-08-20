import type { SourcesPageModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";
import { card, cardContent, cardHeader } from "../server/solid-ui";

export function Sources(props: { page: SourcesPageModel }) {
  return ModernShell({
    username: props.page.username,
    activeHref: "/sources",
    children: <>
      <header class="page-header source-page-header"><div><h1>Build your reading mix</h1><p>Start with one link. Rill detects Telegram channels, RSS/Atom feeds, and feeds published by normal websites.</p></div></header>
      <p id="sources-error" role="alert" class="error" hidden></p>

      <section aria-labelledby="quick-add-heading" class="quick-add-section">
        {card(<>
          {cardHeader(<><h2 id="quick-add-heading">Paste a source link</h2><p class="section-description">Website, RSS/Atom URL, Telegram channel link, or @channel.</p></>, "p-0 pb-5")}
          {cardContent(<>
            <form id="source-quick-add" class="quick-add-form">
              <label class="sr-only" for="quick-source-input">Source link</label>
              <input id="quick-source-input" name="input" placeholder="https://example.com or https://t.me/channel" autocomplete="url" required />
              <button type="submit" class="primary-action">Add source</button>
            </form>
            <div class="quick-examples" aria-label="Examples">
              <span>Try:</span>
              <button type="button" data-quick-source="https://news.ycombinator.com/rss">Hacker News</button>
              <button type="button" data-quick-source="https://t.me/cortex_pulse">@cortex_pulse</button>
            </div>
            <p id="quick-add-result" class="quick-add-result" role="status" aria-live="polite"></p>
          </>, "p-0")}
        </>, "quick-add-card")}
      </section>

      <section aria-labelledby="configured-sources-heading">
        {card(<>
          {cardHeader(<><h2 id="configured-sources-heading">Configured sources</h2><p class="section-description">Health, polling, and access for every connected source.</p></>, "p-0 pb-5")}
          {cardContent(<div id="source-manager-list" class="loading-region" aria-live="polite"><p>Loading sources…</p></div>, "p-0")}
        </>, "admin-section")}
      </section>

      <section aria-labelledby="rss-heading" class="advanced-source-section">
        {card(<>{cardHeader(<><h2 id="rss-heading">RSS and Atom</h2><p class="section-description">Add a feed directly or move a collection with OPML.</p></>, "p-0 pb-5")}{cardContent(<>
          <form id="rss-create" class="admin-form">
            <label>Name <input name="name" required /></label>
            <label>Feed URL <input name="url" type="url" autocomplete="url" required /></label>
            <label class="checkbox-label"><input name="shared" type="checkbox" /> Share with all users</label>
            <button type="submit" class="primary-action">Add feed</button>
          </form>
          <div><form id="opml-import" class="admin-form">
            <label>Import OPML <input name="opml" type="file" accept=".opml,.xml,text/xml" required /></label>
            <button type="submit" class="secondary-action">Import feeds</button>
          </form><p class="mt-3 text-sm"><a href="/api/v1/sources/rss/opml">Export current feeds as OPML</a></p></div>
        </>, "grid gap-6 p-0 lg:grid-cols-2")}</>, "admin-section")}
      </section>

      <section aria-labelledby="email-heading" class="advanced-source-section">
        {card(<>{cardHeader(<><h2 id="email-heading">Email newsletters</h2><p class="section-description">Import newsletters from a dedicated IMAP mailbox.</p></>, "p-0 pb-5")}{cardContent(
        props.page.emailAvailable ? (
          <form id="email-create" class="admin-form">
            <label>Name <input name="name" required /></label>
            <label>IMAP host <input name="host" required /></label>
            <label>Port <input name="port" type="number" value="993" min="1" max="65535" required /></label>
            <label>Username <input name="username" autocomplete="username" required /></label>
            <label>Password <input name="password" type="password" autocomplete="current-password" required /></label>
            <label>Folders, comma separated <input name="folders" value="INBOX" /></label>
            <label class="checkbox-label"><input name="markAsRead" type="checkbox" /> Mark imported mail seen</label>
            <button type="submit" class="primary-action">Add mailbox</button>
          </form>
        ) : <p>Configure RILL_MASTER_KEY to enable encrypted credentials.</p>
        , "p-0")}</>, "admin-section")}
      </section>

      <section aria-labelledby="telegram-heading" class="advanced-source-section">
        {card(<>{cardHeader(<><h2 id="telegram-heading">Telegram channels</h2><p class="section-description">Follow a public channel by username and bind your account to the Rill bot for subscription control.</p></>, "p-0 pb-5")}{cardContent(
          <div class="form-columns telegram-settings">
            <form id="telegram-source-create" class="admin-form">
              <label>Channel username <input name="username" placeholder="channelname" pattern="@?[A-Za-z][A-Za-z0-9_]{4,31}" aria-describedby="telegram-username-help" required /></label>
              <p id="telegram-username-help" class="field-help">Use the public username from the channel link. The leading @ is optional. Public-channel subscriptions do not require the Rill bot.</p>
              <button class="primary-action">Add Telegram source</button>
            </form>
            <section aria-labelledby="telegram-binding-heading">
              <h3 id="telegram-binding-heading">Bot binding</h3>
              <p class="field-help">Create a ten-minute link, then open it in Telegram to connect your account.</p>
              <div id="telegram-binding" class="loading-region" aria-live="polite"><p>Loading binding…</p></div>
            </section>
          </div>
        , "p-0")}</>, "admin-section")}
      </section>

    </>,
  });
}
