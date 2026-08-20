import type { FeedPageModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";
import { StoryCard } from "../components/StoryCard";
import { card, cardContent } from "../server/solid-ui";

export function ModernFeed(props: { page: FeedPageModel }) {
  const pageHref = () => `/stream/${props.page.activeStream}`;
  return ModernShell({
      username: props.page.username,
      activeHref: pageHref(),
      streams: props.page.streams,
      activeStream: props.page.activeStream,
      fontFamily: props.page.fontFamily,
      children: <>
      <header class="page-header">
        <div>
          <h1>{props.page.title}</h1>
        </div>
      </header>
      <div class="feed-toolbar-enhancement" data-feed-toolbar-enhancement />
      <div class="feed-toolbar-fallback" data-enhancement-fallback>
        <nav aria-label="Story view"><a href={pageHref()} aria-current="page">All</a><span aria-disabled="true">Unread</span></nav>
        <a href="/search">Search stories</a>
        <span aria-disabled="true">All topics</span>
        <span aria-disabled="true">Ranked</span>
        <span aria-disabled="true">Compact</span>
      </div>
      <section class="story-list" aria-label="Stories" data-story-list>
        {props.page.stories.length === 0 ? (
          card(cardContent(<><h2>Give Rill one good source</h2><p>Paste a website, RSS feed, or Telegram channel.</p><a class="primary-action" href="/sources">Add your first source</a></>), "empty-state")
        ) : null}
        {props.page.stories.map((story) => StoryCard({ story }))}
      </section>
      {props.page.nextPage ? <div class="infinite-feed-sentinel" data-infinite-feed data-stream={props.page.activeStream} data-offset={props.page.stories.length} role="status">Loading more stories…</div> : null}
      </>,
    });
}
