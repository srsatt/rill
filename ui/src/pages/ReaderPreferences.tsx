import type { ReaderPreferencesPageModel } from "../../generated/render-contract";

export function ReaderPreferences(props: { page: ReaderPreferencesPageModel; csrfToken: string }) {
  return (
    <main class="reader">
      <h1>{props.page.title}</h1>
      <p>Reading as {props.page.username}</p>
      <nav aria-label="Streams">
        {props.page.streams.map((stream) => (
          <a href={`/reader/stream/${stream.slug}`} aria-current={stream.slug === props.page.activeStream ? "page" : undefined}>
            {stream.name}
          </a>
        ))}
      </nav>
      <form method="post" action="/reader/logout">
        <input type="hidden" name="csrf_token" value={props.csrfToken} />
        <button type="submit">Exit reader mode</button>
      </form>
      <p><a href="/reader">Back to feed</a></p>
    </main>
  );
}
