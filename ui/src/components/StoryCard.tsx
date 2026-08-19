import type { StoryCardModel } from "../../generated/render-contract";
import { badge } from "../server/solid-ui";
import { BookmarkIcon } from "./icons";

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
            <div class="story-source-line">
              {badge(props.story.source, "outline")}
              {props.story.curator ? <span>via {props.story.curator}</span> : null}
              <span>{props.story.readingMinutes} min</span>
            </div>
            <h2><a href={`/story/${props.story.id}`}>{props.story.title}</a></h2>
            <p class="summary">{props.story.summary}</p>
            {tags.length > 0 ? <div class="story-tags" aria-label="Topics">
              {tags.map((tag) => <a class="topic-link" href={`/search?topic=${encodeURIComponent(tag)}`}>{tag}</a>)}
            </div> : null}
            <p class="meta">{props.story.coverageCount} {props.story.coverageCount === 1 ? "source" : "sources"} in this story</p>
          </div>
          <div class="story-feedback-enhancement" data-story-feedback-enhancement data-story-id={props.story.id} />
          <div class="feedback" aria-label="Story feedback" data-enhancement-fallback>
            <button type="button" data-feedback="like" aria-pressed="false" onClick={selectFeedback}>👍 Like</button>
            <button type="button" data-feedback="dislike" aria-pressed="false" onClick={selectFeedback}>👎 Dislike</button>
            <button type="button" data-feedback="favorite" aria-pressed="false" onClick={selectFeedback}><BookmarkIcon /> Favorite</button>
          </div>
      </div>
    </article>
  );
}
