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
      children: <>
      <header class="page-header">
        <div>
          <p class="eyebrow">Your reading lane</p>
          <h1>{props.page.title}</h1>
          <p>A calm shortlist from everything Rill found · {props.page.stories.length} {props.page.stories.length === 1 ? "story" : "stories"}</p>
        </div>
        <div class="page-actions"><a class="secondary-action" href="/sources">Add sources</a><a class="secondary-action" href="/reader">Reader mode</a></div>
      </header>
      <div class="feed-toolbar-enhancement" data-feed-toolbar-enhancement />
      <div class="feed-toolbar-fallback" data-enhancement-fallback>
        <nav aria-label="Story view"><a href={pageHref()} aria-current="page">All</a><span aria-disabled="true">Unread</span></nav>
        <a href="/search">Search stories</a>
      </div>
      <section class="story-list" aria-label="Stories" data-story-list>
        {props.page.stories.length === 0 ? (
          card(cardContent(<><p class="eyebrow">Start here</p><h2>Give Rill one good source</h2><p>Paste a website, RSS feed, or Telegram channel. Fetching and AI enrichment stay in background.</p><a class="primary-action" href="/sources">Add your first source</a></>), "empty-state")
        ) : null}
        {props.page.stories.map((story) => StoryCard({ story }))}
      </section>
      {(props.page.previousPage || props.page.nextPage) && (
        <nav class="pagination" aria-label="Story pages">
          {props.page.previousPage ? <a href={`/stream/${props.page.activeStream}?page=${props.page.previousPage}`}>Newer stories</a> : <span />}
          <span>Page {props.page.page}</span>
          {props.page.nextPage ? <a href={`/stream/${props.page.activeStream}?page=${props.page.nextPage}`}>Older stories</a> : <span />}
        </nav>
      )}
      </>,
    });
}
