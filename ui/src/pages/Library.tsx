import type { LibraryPageModel } from "../../generated/render-contract";
import { ModernShell } from "../components/ModernShell";
import { SearchIcon } from "../components/icons";
import { StoryCard } from "../components/StoryCard";

export function Library(props: { page: LibraryPageModel }) {
  const isSearch = props.page.kind === "search";
  const subtitle = isSearch ? "Find stories across every stream." : props.page.kind === "favorites" ? "Stories you want to keep." : "Stories you opened recently.";
  const emptyTitle = isSearch ? props.page.query ? "No results" : "Search your stories" : props.page.kind === "favorites" ? "No favorites yet" : "No reading history yet";
  const emptyCopy = isSearch ? props.page.query ? "Try a different query." : "Enter a query to find a story." : props.page.kind === "favorites" ? "Favorite a story to keep it here." : "Stories you open will appear here.";
  return ModernShell({
    username: props.page.username,
    activeHref: props.page.kind === "favorites" ? "/favorites" : props.page.kind === "history" ? "/history" : "/search",
    children: <>
      <header class="page-header">
        <div><h1>{props.page.title}</h1><p>{subtitle}</p></div>
      </header>
      {isSearch ? <form method="get" action="/search" role="search" class="search-form">
        <label class="sr-only" for="library-query">Search stories</label>
        <input class="field-input" id="library-query" name="q" type="search" value={props.page.query ?? ""} maxLength="200" placeholder="Search by title, source, or topic" />
        <button type="submit" class="primary-action"><SearchIcon /> Search</button>
      </form> : null}
      <section class="story-list" aria-label="Stories">
        {props.page.stories.length === 0 ? (
          <div class="library-empty"><h2>{emptyTitle}</h2><p>{emptyCopy}</p></div>
        ) : null}
        {props.page.stories.map((story) => StoryCard({ story }))}
      </section>
    </>,
  });
}
