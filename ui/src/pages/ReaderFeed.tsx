import type { FeedPageModel } from "../../generated/render-contract";
import { BookmarkIcon, ExternalLinkIcon, TelegramIcon, ThumbsDownIcon, ThumbsUpIcon } from "../components/icons";

function telegramChannel(url: string | null): string | null {
  if (!url?.startsWith("https://t.me/")) return null;
  const channel = url.slice(13).split("/")[0];
  return channel ? `@${channel}` : null;
}

export function ReaderFeed(props: { page: FeedPageModel; csrfToken: string }) {
  return (
    <main class={`reader reading-font-${props.page.fontFamily === "serif" ? "serif" : "sans"}`}>
      <header class="reader-header">
      <div class="reader-brand">
        <a class="reader-wordmark" href="/reader">Rill</a>
        <a class="reader-full" href="/stream/all" aria-label="Open full Rill" title="Full Rill"><ExternalLinkIcon /><span class="sr-only">Full Rill</span></a>
      </div>
      <nav aria-label="Streams">
        {props.page.streams.map((stream) => (
          <a href={`/reader/stream/${stream.slug}`} aria-current={stream.slug === props.page.activeStream ? "page" : undefined}>{stream.name}</a>
        ))}
      </nav>
      </header>
      {props.page.stories.length === 0 ? <p>No stories yet.</p> : null}
      {props.page.stories.map((story) => {
        const tags = story.tags || [];
        const channel = telegramChannel(story.canonicalUrl);
        return <article>
          <h2 class="reader-story-title"><a href={`/reader/story/${story.id}`}>{story.title}</a></h2>
            {story.summary && story.summary !== "No summary available." ? <p class="reader-story-summary">{story.summary}</p> : null}
          {tags.length > 0 ? <div class="reader-topics" aria-label="Topics">{tags.map((tag) => <a href={`/search?topic=${encodeURIComponent(tag)}`}>{tag}</a>)}</div> : null}
          <div class="reader-story-footer">
            <p class="reader-story-meta">
              {channel && story.canonicalUrl
                ? <a class="reader-telegram-link" href={story.canonicalUrl} target="_blank" rel="noopener noreferrer"><TelegramIcon />{channel}</a>
                : story.canonicalUrl ? <a href={story.canonicalUrl} target="_blank" rel="noopener noreferrer">{story.source}</a> : story.source} · {story.readingMinutes} min
            </p>
            <form class="reader-feedback-actions" method="post" action={`/reader/story/${story.id}/feedback`}>
              <input type="hidden" name="csrf_token" value={props.csrfToken} />
              <button name="feedback" value="like" aria-label="Like" title="Like" aria-pressed="false"><ThumbsUpIcon /></button>
              <button name="feedback" value="dislike" aria-label="Dislike" title="Dislike" aria-pressed="false"><ThumbsDownIcon /></button>
              <button name="feedback" value="favorite" aria-label="Favorite" title="Favorite" aria-pressed="false"><BookmarkIcon /></button>
            </form>
          </div>
        </article>;
      })}
      {props.page.previousPage || props.page.nextPage ? <nav class="reader-pagination" aria-label="Story pages">
        {props.page.previousPage ? <a href={`?page=${props.page.previousPage}`}>Newer</a> : null}
        {props.page.nextPage ? <a href={`?page=${props.page.nextPage}`}>Older</a> : null}
      </nav> : null}
    </main>
  );
}
