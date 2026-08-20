import type { StoryLinkModel, StoryPageModel, StoryVariantModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";
import { BookmarkCheckIcon, BookmarkIcon, ExternalLinkIcon, EyeIcon, EyeOffIcon, MessageCircleIcon, ThumbsDownIcon, ThumbsUpIcon } from "../components/icons";
import { badge, card, cardContent, cardHeader } from "../server/solid-ui";

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
  const hasAlternatives = () => props.page.coverageCount > 1;
  return ModernShell({
      username: props.page.username,
      activeHref: "/stream/home",
      fontFamily: props.page.fontFamily,
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
          <button type="button" aria-pressed={props.page.explicitFeedback === "like"} onClick={() => void mutate(`/api/v1/stories/${props.page.storyId}/feedback`, { feedback: "like" })}><ThumbsUpIcon /> Like</button>
          <button type="button" aria-pressed={props.page.explicitFeedback === "dislike"} onClick={() => void mutate(`/api/v1/stories/${props.page.storyId}/feedback`, { feedback: "dislike" })}><ThumbsDownIcon /> Dislike</button>
          <button type="button" aria-pressed={props.page.favorite} onClick={() => void mutate(`/api/v1/stories/${props.page.storyId}/feedback`, { feedback: "favorite" })}>{props.page.favorite ? <BookmarkCheckIcon /> : <BookmarkIcon />} {props.page.favorite ? "Favorited" : "Favorite"}</button>
        </div>
      </header>
      <article class="story-document">
        {card(<>
          {cardHeader(<>
            <div class="story-source-line">
              {representative().canonicalUrl
                ? <a class="story-source-link" href={representative().canonicalUrl ?? undefined} target="_blank" rel="noopener noreferrer">{badge(representative().publisher ?? "Unknown publisher", "outline")}</a>
                : badge(representative().publisher ?? "Unknown publisher", "outline")}
              {representative().author ? <span>By {representative().author}</span> : null}
              {props.page.coverageCount > 1 ? <span>{props.page.coverageCount} sources</span> : null}
            </div>
            <h1>{representative().title}</h1>
            {hasDistinctSummary(representative()) ? <p class="story-deck">{representative().summary}</p> : null}
          </>, "story-title-block")}
          {cardContent(<>
            <div class="article-body">{representative().bodyText}</div>
            {StoryLinks({ links: representative().links, canonicalUrl: representative().canonicalUrl })}
          </>)}
        </>, "shadow-none")}
      </article>

      {hasAlternatives() ? <section class="coverage-section" aria-labelledby="coverage-heading">
        <div class="section-heading"><h2 id="coverage-heading">Coverage ({props.page.coverageCount})</h2></div>
        <div class="coverage-list">
          {props.page.variants.map((variant) => (
            <div class={variant.selected ? "coverage-variant-frame selected" : "coverage-variant-frame"}>
              {card(<>
                {cardHeader(<>
                  <div class="story-source-line">{badge(variant.selected ? "Selected version" : "Alternative", variant.selected ? "default" : "outline")}{variant.canonicalUrl ? <a href={variant.canonicalUrl} target="_blank" rel="noopener noreferrer">{variant.publisher ?? "Unknown publisher"}</a> : <span>{variant.publisher ?? "Unknown publisher"}</span>}</div>
                  <h3>{variant.title}</h3>
                </>)}
                {cardContent(<>
                  <p class="meta">{variant.publishedAt ? variant.publishedAt.slice(0, 10) : "Publication date unavailable"}</p>
                  {variant.curators.length === 0 ? <p class="meta">Direct source</p> : variant.curators.map((path) => (
                    <p class="curator-path">Via {path.sourceName ?? path.curatorId}{path.parentTitle ? ` in ${path.parentTitle}` : ""}{path.curatorCommentary ? `: ${path.curatorCommentary}` : ""}</p>
                  ))}
                  {StoryLinks({ links: variant.links, canonicalUrl: variant.canonicalUrl })}
                  {!variant.selected && <button class="secondary-action" type="button" onClick={() => void mutate(`/api/v1/stories/${props.page.storyId}/representative`, { documentId: variant.documentId })}>Use this version</button>}
                </>)}
              </>, "coverage-variant")}
            </div>
          ))}
        </div>
      </section> : null}
    </>,
  });
}

function ReaderStory(props: { page: StoryPageModel; csrfToken: string }) {
  return (
    <main class={`reader story reading-font-${props.page.fontFamily === "serif" ? "serif" : "sans"}`}>
      <p><a href="/reader">← Back to feed</a></p>
      <article>
        <h1 class="reader-story-title">{props.page.representative.title}</h1>
        <p class="reader-story-meta">{props.page.representative.canonicalUrl ? <a href={props.page.representative.canonicalUrl} target="_blank" rel="noopener noreferrer">{props.page.representative.publisher ?? "Unknown publisher"}</a> : props.page.representative.publisher ?? "Unknown publisher"}{props.page.representative.author ? ` · ${props.page.representative.author}` : ""}</p>
        {hasDistinctSummary(props.page.representative) ? <p class="reader-story-summary">{props.page.representative.summary}</p> : null}
        <div class="article-body">{props.page.representative.bodyText}</div>
        {StoryLinks({ links: props.page.representative.links, canonicalUrl: props.page.representative.canonicalUrl })}
      </article>
      {props.page.coverageCount > 1 ? <section aria-labelledby="coverage-heading">
        <h2 id="coverage-heading">Coverage ({props.page.coverageCount})</h2>
        {props.page.variants.map((variant) => (
          <article>
            <h3>{variant.title}</h3>
            <p>{variant.canonicalUrl ? <a href={variant.canonicalUrl} target="_blank" rel="noopener noreferrer">{variant.publisher ?? "Unknown publisher"}</a> : variant.publisher ?? "Unknown publisher"}{variant.selected ? " · selected" : ""}</p>
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
      </section> : null}
      <section aria-label="Story controls">
        <form method="post" action={`/reader/story/${props.page.storyId}/read`}>
          <input type="hidden" name="csrf_token" value={props.csrfToken} />
          <input type="hidden" name="read" value={props.page.read ? "false" : "true"} />
          <button type="submit">{props.page.read ? <EyeOffIcon /> : <EyeIcon />} Mark {props.page.read ? "unread" : "read"}</button>
        </form>
        <form method="post" action={`/reader/story/${props.page.storyId}/feedback`}>
          <input type="hidden" name="csrf_token" value={props.csrfToken} />
          <button name="feedback" value="like" aria-pressed={props.page.explicitFeedback === "like"}><ThumbsUpIcon /> Like</button>
          <button name="feedback" value="dislike" aria-pressed={props.page.explicitFeedback === "dislike"}><ThumbsDownIcon /> Dislike</button>
          <button name="feedback" value="favorite" aria-pressed={props.page.favorite}>{props.page.favorite ? <BookmarkCheckIcon /> : <BookmarkIcon />} {props.page.favorite ? "Favorited" : "Favorite"}</button>
        </form>
      </section>
    </main>
  );
}

function hasDistinctSummary(story: StoryVariantModel): boolean {
  const summary = story.summary.trim();
  return summary !== "" && summary !== story.bodyText.trim();
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
