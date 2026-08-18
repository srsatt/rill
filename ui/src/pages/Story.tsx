import type { StoryLinkModel, StoryPageModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";
import { BookmarkCheckIcon, BookmarkIcon, ExternalLinkIcon, EyeIcon, EyeOffIcon, MessageCircleIcon } from "../components/icons";
import { badge, card, cardContent, cardHeader, table, tableBody, tableCell, tableHead, tableHeader, tableRow } from "../server/solid-ui";

export function Story(props: { page: StoryPageModel; csrfToken?: string }) {
  if (props.page.reader) return ReaderStory({ page: props.page, csrfToken: props.csrfToken ?? "" });

  const mutate = async (path: string, body: unknown) => {
    const csrf = document.cookie
      .split(";")
      .map((part) => part.trim().split("="))
      .find(([name]) => name === "rill_csrf")?.[1];
    if (!csrf) return;
    const response = await fetch(path, {
      method: "POST", credentials: "same-origin",
      headers: { "content-type": "application/json", "x-csrf-token": csrf },
      body: JSON.stringify(body),
    });
    if (response.ok) window.location.reload();
  };
  const representative = () => props.page.representative;
  return ModernShell({
      activeHref: "/stream/home",
      detail: (
        <section aria-labelledby="provenance-heading">
          <p class="eyebrow">Provenance</p>
          <h2 id="provenance-heading" class="detail-heading">Coverage map</h2>
          <p class="detail-description">Rill grouped {props.page.coverageCount} versions and selected one representative.</p>
          {table(<>
            {tableHeader(tableRow(<>{tableHead("Publisher")}{tableHead("Status")}</>))}
            {tableBody(<>
              {props.page.variants.map((variant) => (
                tableRow(<>{tableCell(variant.publisher ?? "Unknown")}{tableCell(variant.selected ? "Selected" : "Alternative")}</>)
              ))}
            </>)}
          </>)}
        </section>
      ),
      children: <>
      <header class="story-page-header">
        <a href="/stream/home">← Back to feed</a>
        <div
          data-story-actions-enhancement
          data-story-id={props.page.storyId}
          data-read={String(props.page.read)}
          data-favorite={String(props.page.favorite)}
          data-feedback={props.page.explicitFeedback ?? ""}
        />
        <div class="feedback story-controls" aria-label="Story controls" data-enhancement-fallback>
          <button type="button" onClick={() => void mutate(`/api/v1/stories/${props.page.storyId}/read-state`, { read: !props.page.read })}>{props.page.read ? <EyeOffIcon /> : <EyeIcon />} Mark {props.page.read ? "unread" : "read"}</button>
          <button type="button" aria-pressed={props.page.explicitFeedback === "like"} onClick={() => void mutate(`/api/v1/stories/${props.page.storyId}/feedback`, { feedback: "like" })}>👍 Like</button>
          <button type="button" aria-pressed={props.page.explicitFeedback === "dislike"} onClick={() => void mutate(`/api/v1/stories/${props.page.storyId}/feedback`, { feedback: "dislike" })}>👎 Dislike</button>
          <button type="button" aria-pressed={props.page.favorite} onClick={() => void mutate(`/api/v1/stories/${props.page.storyId}/feedback`, { feedback: "favorite" })}>{props.page.favorite ? <BookmarkCheckIcon /> : <BookmarkIcon />} {props.page.favorite ? "Favorited" : "Favorite"}</button>
        </div>
      </header>
      <article class="story-document">
        {card(<>
          {cardHeader(<>
            <div class="story-source-line">
              {badge(representative().publisher ?? "Unknown publisher", "outline")}
              {representative().author ? <span>By {representative().author}</span> : null}
              <span>{props.page.coverageCount} sources</span>
            </div>
            <h1>{representative().title}</h1>
            <p class="story-deck">{representative().summary}</p>
          </>, "story-title-block")}
          {cardContent(<>
            <div class="article-body">{representative().bodyText}</div>
            {StoryLinks({ links: representative().links, canonicalUrl: representative().canonicalUrl })}
          </>)}
        </>, "shadow-none")}
      </article>

      <section class="coverage-section" aria-labelledby="coverage-heading">
        <div class="section-heading"><div><p class="eyebrow">Sources</p><h2 id="coverage-heading">Coverage ({props.page.coverageCount})</h2></div></div>
        <div class="coverage-list">
          {props.page.variants.map((variant) => (
            <div class={variant.selected ? "coverage-variant-frame selected" : "coverage-variant-frame"}>
              {card(<>
                {cardHeader(<>
                  <div class="story-source-line">{badge(variant.selected ? "Selected version" : "Alternative", variant.selected ? "default" : "outline")}<span>{variant.publisher ?? "Unknown publisher"}</span></div>
                  <h3>{variant.title}</h3>
                </>)}
                {cardContent(<>
                  {variant.curators.length === 0 ? <p class="meta">Direct source</p> : variant.curators.map((path) => (
                    <p class="curator-path">Via {path.sourceName ?? path.curatorId}{path.parentTitle ? ` in ${path.parentTitle}` : ""}{path.curatorCommentary ? `: ${path.curatorCommentary}` : ""}</p>
                  ))}
                  {!variant.selected && <button class="secondary-action" type="button" onClick={() => void mutate(`/api/v1/stories/${props.page.storyId}/representative`, { documentId: variant.documentId })}>Use this version</button>}
                </>)}
              </>, "coverage-variant")}
            </div>
          ))}
        </div>
      </section>
    </>,
  });
}

function ReaderStory(props: { page: StoryPageModel; csrfToken: string }) {
  return (
    <main class="reader story">
      <p><a href="/reader">← Back to feed</a></p>
      <article>
        <h1 class="reader-story-title">{props.page.representative.title}</h1>
        <p class="reader-story-meta">{props.page.representative.publisher ?? "Unknown publisher"}{props.page.representative.author ? ` · ${props.page.representative.author}` : ""}</p>
        <p class="reader-story-summary">{props.page.representative.summary}</p>
        <div class="article-body">{props.page.representative.bodyText}</div>
        {StoryLinks({ links: props.page.representative.links, canonicalUrl: props.page.representative.canonicalUrl })}
      </article>
      <section aria-labelledby="coverage-heading">
        <h2 id="coverage-heading">Coverage ({props.page.coverageCount})</h2>
        {props.page.variants.map((variant) => (
          <article>
            <h3>{variant.title}</h3>
            <p>{variant.publisher ?? "Unknown publisher"}{variant.selected ? " · selected" : ""}</p>
            {variant.curators.map((path) => <p>Via {path.sourceName ?? path.curatorId}{path.parentTitle ? ` in ${path.parentTitle}` : ""}{path.curatorCommentary ? `: ${path.curatorCommentary}` : ""}</p>)}
            {!variant.selected && (
              <form method="post" action={`/reader/story/${props.page.storyId}/variant`}>
                <input type="hidden" name="csrf_token" value={props.csrfToken} />
                <input type="hidden" name="document_id" value={variant.documentId} />
                <button type="submit">Use this version</button>
              </form>
            )}
          </article>
        ))}
      </section>
      <section aria-label="Story controls">
        <form method="post" action={`/reader/story/${props.page.storyId}/read`}>
          <input type="hidden" name="csrf_token" value={props.csrfToken} />
          <input type="hidden" name="read" value={props.page.read ? "false" : "true"} />
          <button type="submit">{props.page.read ? <EyeOffIcon /> : <EyeIcon />} Mark {props.page.read ? "unread" : "read"}</button>
        </form>
        <form method="post" action={`/reader/story/${props.page.storyId}/feedback`}>
          <input type="hidden" name="csrf_token" value={props.csrfToken} />
          <button name="feedback" value="like" aria-pressed={props.page.explicitFeedback === "like"}>👍 Like</button>
          <button name="feedback" value="dislike" aria-pressed={props.page.explicitFeedback === "dislike"}>👎 Dislike</button>
          <button name="feedback" value="favorite" aria-pressed={props.page.favorite}>{props.page.favorite ? <BookmarkCheckIcon /> : <BookmarkIcon />} {props.page.favorite ? "Favorited" : "Favorite"}</button>
        </form>
      </section>
    </main>
  );
}

function StoryLinks(props: { links: StoryLinkModel[]; canonicalUrl: string | null }) {
  const links = props.links ?? [];
  const original = links.find((link) => link.relation === "alternate")?.url ?? props.canonicalUrl;
  const discussion = links.find((link) => link.relation === "replies" && link.url !== original)?.url;
  if (!original && !discussion) return null;
  return <p class="original-link">
    {original ? <a href={original} rel="noopener noreferrer"><ExternalLinkIcon /> Open original</a> : null}
    {original && discussion ? " · " : null}
    {discussion ? <a href={discussion} rel="noopener noreferrer"><MessageCircleIcon /> Discussion</a> : null}
  </p>;
}
