import type { FeedPageModel } from "../../generated/render-contract";

export function ReaderFeed(props: { page: FeedPageModel; csrfToken: string }) {
  return (
    <main class="reader">
      <h1>{props.page.title}</h1>
      <nav aria-label="Streams">
        {props.page.streams.map((stream) => (
          <a href={`/reader/stream/${stream.slug}`}>{stream.name}</a>
        ))}
      </nav>
      <p><a href="/stream/home">Full Rill</a> · <a href="/reader/settings">Reader settings</a></p>
      {props.page.stories.length === 0 ? <p>No stories yet.</p> : null}
      {props.page.stories.map((story) => {
        const tags = story.tags || [];
        return <article>
          <h2><a href={`/reader/story/${story.id}`}>{story.title}</a></h2>
          <p>{story.summary}</p>
          {tags.length > 0 ? <p>Topics: {tags.join(", ")}</p> : null}
          <p>{story.source} · {story.readingMinutes} min</p>
          <form method="post" action={`/reader/story/${story.id}/feedback`}>
            <input type="hidden" name="csrf_token" value={props.csrfToken} />
            <button name="feedback" value="like">Like</button>
            <button name="feedback" value="dislike">Dislike</button>
            <button name="feedback" value="favorite">Favorite</button>
          </form>
        </article>;
      })}
      <nav aria-label="Pages">
        {props.page.previousPage ? <a href={`/reader/page/${props.page.previousPage}`}>Previous</a> : null}
        <span> Page {props.page.page} </span>
        {props.page.nextPage ? <a href={`/reader/page/${props.page.nextPage}`}>Next</a> : null}
      </nav>
    </main>
  );
}
