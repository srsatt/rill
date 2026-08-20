import type { StoryCardModel } from "../../generated/render-contract";
import { BookmarkIcon, ThumbsDownIcon, ThumbsUpIcon } from "./icons";

export function StoryCard(props: { story: StoryCardModel }) {
  const tags = props.story.tags || [];
  const selectFeedback = async (event: MouseEvent) => {
    const selected = event.currentTarget as HTMLButtonElement;
    const feedback = selected.dataset.feedback;
    const csrf = document.cookie
      .split(";")
      .map((part) => part.trim().split("="))
      .find(([name]) => name === "rill_csrf")?.[1];
    if (!feedback || !csrf) return;
    selected.disabled = true;
    const response = await fetch(`/api/v1/stories/${props.story.id}/feedback`, {
      method: "POST",
      credentials: "same-origin",
      headers: { "content-type": "application/json", "x-csrf-token": csrf },
      body: JSON.stringify({ feedback }),
    });
    selected.disabled = false;
    if (!response.ok) return;
    const group = selected.parentElement;
    if (!group) return;
    for (const button of group.querySelectorAll("button")) {
      button.setAttribute("aria-pressed", String(button === selected));
    }
  };

  return (
    <article class="story-row" data-story-id={props.story.id} data-published-at={props.story.publishedAt} data-topics={tags.join(",")}>
      <div class="story-card-content">
          <div class="story-copy">
              <h2><a href={`/story/${props.story.id}`}>{props.story.title}</a></h2>
              {props.story.summary && props.story.summary !== "No summary available." ? <p class="summary">{props.story.summary}</p> : null}
              <div class="story-source-line">
                {props.story.canonicalUrl
                  ? <a class="story-source-link" href={props.story.canonicalUrl} target="_blank" rel="noopener noreferrer">{props.story.source}</a>
                  : <span>{props.story.source}</span>}
                {props.story.curator ? <span>via {props.story.curator}</span> : null}
                <span>{props.story.readingMinutes} min</span>
              </div>
              {tags.length > 0 ? <div class="story-tags" aria-label="Topics">
                {tags.map((tag) => <a class="topic-link" href={`/search?topic=${encodeURIComponent(tag)}`}>{tag}</a>)}
              </div> : null}
              {props.story.coverageCount > 1 ? <p class="meta">{props.story.coverageCount} sources</p> : null}
            </div>
            <div class="story-feedback-enhancement" data-story-feedback-enhancement data-story-id={props.story.id} />
            <div class="feedback" aria-label="Story feedback" data-enhancement-fallback>
              <button type="button" data-feedback="like" aria-label="Like" title="Like" aria-pressed="false" onClick={selectFeedback}><ThumbsUpIcon /><span class="feedback-label">Like</span></button>
              <button type="button" data-feedback="dislike" aria-label="Dislike" title="Dislike" aria-pressed="false" onClick={selectFeedback}><ThumbsDownIcon /><span class="feedback-label">Dislike</span></button>
              <button type="button" data-feedback="favorite" aria-label="Favorite" title="Favorite" aria-pressed="false" onClick={selectFeedback}><BookmarkIcon /><span class="feedback-label">Favorite</span></button>
            </div>
      </div>
    </article>
  );
}
