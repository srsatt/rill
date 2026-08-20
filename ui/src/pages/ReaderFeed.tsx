import type { FeedPageModel } from "../../generated/render-contract";
import { BookmarkIcon, ThumbsDownIcon, ThumbsUpIcon } from "../components/icons";

export function ReaderFeed(props: { page: FeedPageModel; csrfToken: string }) {
  return (
    <main class={`reader reading-font-${props.page.fontFamily === "serif" ? "serif" : "sans"}`}>
      <header class="reader-header">
      <a class="reader-wordmark" href="/reader">Rill</a>
      <nav aria-label="Streams">
        {props.page.streams.map((stream) => (
          <a href={`/reader/stream/${stream.slug}`} aria-current={stream.slug === props.page.activeStream ? "page" : undefined}>{stream.name}</a>
        ))}
      </nav>
       <p class="reader-full"><a href="/stream/home">Full Rill</a></p>
      </header>
      {props.page.stories.length === 0 ? <p>No stories yet.</p> : null}
      {props.page.stories.map((story) => {
        const tags = story.tags || [];
        return <article>
          <h2 class="reader-story-title"><a href={`/reader/story/${story.id}`}>{story.title}</a></h2>
            {story.summary && story.summary !== "No summary available." ? <p class="reader-story-summary">{story.summary}</p> : null}
          {tags.length > 0 ? <div class="reader-topics" aria-label="Topics">{tags.map((tag) => <a href={`/search?topic=${encodeURIComponent(tag)}`}>{tag}</a>)}</div> : null}
          <p class="reader-story-meta">
            {story.canonicalUrl ? <a href={story.canonicalUrl} target="_blank" rel="noopener noreferrer">{story.source}</a> : story.source} · {story.readingMinutes} min
          </p>
          <form method="post" action={`/reader/story/${story.id}/feedback`}>
            <input type="hidden" name="csrf_token" value={props.csrfToken} />
            <button name="feedback" value="like" aria-pressed="false"><ThumbsUpIcon /> Like</button>
            <button name="feedback" value="dislike" aria-pressed="false"><ThumbsDownIcon /> Dislike</button>
            <button name="feedback" value="favorite" aria-pressed="false"><BookmarkIcon /> Favorite</button>
          </form>
        </article>;
      })}
      {props.page.previousPage || props.page.nextPage ? <nav class="reader-pagination" aria-label="Story pages">
        {props.page.previousPage ? <a href={`?page=${props.page.previousPage}`}>Newer</a> : null}
        {props.page.nextPage ? <a href={`?page=${props.page.nextPage}`}>Older</a> : null}
      </nav> : null}
    </main>
  );
}
