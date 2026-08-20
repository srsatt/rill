import type { JSX } from "solid-js";
import type { StreamLink } from "../../generated/render-contract";

interface ModernShellProps {
  username?: string;
  activeHref: string;
  streams?: StreamLink[];
  activeStream?: string;
  fontFamily?: string;
  children: JSX.Element;
  detail?: JSX.Element;
}

const primaryLinks = [
  { href: "/stream/home", label: "Feed" },
  { href: "/search", label: "Search" },
  { href: "/favorites", label: "Favorites" },
  { href: "/history", label: "History" },
  { href: "/sources", label: "Sources" },
  { href: "/reader", label: "Reader mode" },
  { href: "/settings/readers", label: "Settings" },
];

export function ModernShell(props: ModernShellProps) {
  const streams = () => props.streams ?? [];
  const username = () => props.username || "Account";
  const mobileData = () => JSON.stringify(streams());
  const sidebarLinks: JSX.Element[] = [];
  const mobileLinks: JSX.Element[] = [];
  for (const link of primaryLinks) {
    sidebarLinks.push(<li><a href={link.href} aria-current={props.activeHref === link.href ? "page" : undefined}>{link.label}</a></li>);
    mobileLinks.push(<a href={link.href}>{link.label}</a>);
  }
  return (
    <div class={`modern-app reading-font-${props.fontFamily === "serif" ? "serif" : "sans"}${props.detail ? " has-detail" : ""}`}>
      <a class="skip-link" href="#main-content">Skip to main content</a>
      <aside class="app-sidebar" aria-label="Rill navigation">
        <div class="sidebar-brand">
          <a class="wordmark" href="/stream/home">Rill</a>
        </div>
        <nav class="sidebar-nav" aria-label="Main navigation">
          <ul>
            {sidebarLinks}
          </ul>
        </nav>
        {streams().length > 0 && (
          <section class="stream-nav" aria-labelledby="streams-heading">
            <h2 id="streams-heading">Streams</h2>
            <nav aria-label="Streams">
              <ul>
                {streams().map((stream) => (
                  <li>
                    <a href={`/stream/${stream.slug}`} aria-current={stream.slug === props.activeStream ? "page" : undefined}>
                      <span aria-hidden="true" class="stream-dot" />
                      {stream.name}
                    </a>
                  </li>
                ))}
              </ul>
            </nav>
          </section>
        )}
        <div class="sidebar-account">
          <div data-account-enhancement data-username={username()} />
          <div class="account-fallback" data-enhancement-fallback>
            <span class="account-avatar" aria-hidden="true">{username().slice(0, 1).toUpperCase()}</span>
            <strong>{username()}</strong>
          </div>
        </div>
      </aside>

      <header class="mobile-header">
        <a class="wordmark" href="/stream/home">Rill</a>
        <div
          data-mobile-nav-enhancement
          data-username={username()}
          data-streams={mobileData()}
          data-active-stream={props.activeStream ?? ""}
          data-active-href={props.activeHref}
        />
        <details class="mobile-nav-fallback" data-enhancement-fallback>
          <summary>Menu</summary>
          <nav aria-label="Mobile navigation">
            {mobileLinks}
          </nav>
        </details>
      </header>

      <main id="main-content" class="app-main" tabindex="-1">
        {props.children}
      </main>
      {props.detail && <aside class="app-detail" aria-label="Context and provenance">{props.detail}</aside>}
    </div>
  );
}

export { primaryLinks };
