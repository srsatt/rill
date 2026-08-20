import type { LibraryPageModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";
import { SearchIcon } from "../components/icons";
import { StoryCard } from "../components/StoryCard";
import { card, cardContent } from "../server/solid-ui";

export function Library(props: { page: LibraryPageModel }) {
  return ModernShell({
    username: props.page.username,
    activeHref: props.page.kind === "favorites" ? "/favorites" : props.page.kind === "history" ? "/history" : "/search",
    children: <>
      <header class="page-header">
        <div><h1>{props.page.title}</h1><p>Search and revisit stories across every stream.</p></div>
      </header>
      <form method="get" action="/search" role="search" class="search-form">
        <label class="sr-only" for="library-query">Search stories</label>
        <input class="field-input" id="library-query" name="q" type="search" value={props.page.query ?? ""} maxLength="200" placeholder="Search by title, source, or topic" />
        <button type="submit" class="primary-action"><SearchIcon /> Search</button>
      </form>
      <section class="story-list" aria-label="Stories">
        {props.page.stories.length === 0 ? (
          card(cardContent(<><h2>{props.page.kind === "search" && !props.page.query ? "Find a story" : "Nothing here yet"}</h2><p>{props.page.kind === "search" && !props.page.query ? "Enter a search query." : "No stories here."}</p></>), "empty-state")
        ) : null}
        {props.page.stories.map((story) => StoryCard({ story }))}
      </section>
    </>,
  });
}
