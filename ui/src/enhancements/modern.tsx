import { createSignal, onMount } from "solid-js";
import { render } from "solid-js/web";
import type { StoryCardModel, StreamLink } from "../../generated/render-contract";
import { accountLinks, primaryLinks } from "../components/ModernShell";
import { BookmarkCheckIcon, BookmarkIcon, EyeIcon, EyeOffIcon, SlidersHorizontalIcon, ThumbsDownIcon, ThumbsUpIcon } from "../components/icons";
import { StoryCard } from "../components/StoryCard";
import { Alert, AlertDescription } from "../components/ui/alert";
import { Avatar, AvatarFallback } from "../components/ui/avatar";
import { Button } from "../components/ui/button";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger,
} from "../components/ui/dropdown-menu";
import {
  Select, SelectContent, SelectItem, SelectLabel, SelectTrigger, SelectValue,
} from "../components/ui/select";
import { Separator } from "../components/ui/separator";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle, SheetTrigger } from "../components/ui/sheet";
import { Switch, SwitchControl, SwitchLabel, SwitchThumb } from "../components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../components/ui/tabs";
import { TextField, TextFieldInput, TextFieldLabel } from "../components/ui/text-field";
import { Toggle } from "../components/ui/toggle";
import { currentTheme, setTheme, type Theme } from "../theme";

const [theme, setActiveTheme] = createSignal<Theme>(currentTheme());

function chooseTheme(value: string): void {
  if (value !== "light" && value !== "dark") return;
  setActiveTheme(value);
  setTheme(value);
}

function csrfToken(): string {
  return document.cookie.split(";").map((part) => part.trim().split("="))
    .find(([name]) => name === "rill_csrf")?.[1] ?? "";
}

async function mutate(path: string, body: unknown): Promise<boolean> {
  const csrf = csrfToken();
  if (!csrf) return false;
  const response = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers: { "content-type": "application/json", "x-csrf-token": csrf },
    body: JSON.stringify(body),
  });
  return response.ok;
}

function finishMount(host: HTMLElement): void {
  const fallback = host.nextElementSibling;
  if (fallback instanceof HTMLElement && fallback.hasAttribute("data-enhancement-fallback")) fallback.hidden = true;
  host.dataset.enhanced = "true";
}

function parseStreams(value: string | undefined): StreamLink[] {
  if (!value) return [];
  try {
    const parsed = JSON.parse(value) as unknown;
    return Array.isArray(parsed) ? parsed.filter((stream): stream is StreamLink => (
      typeof stream === "object" && stream !== null && typeof (stream as StreamLink).name === "string" && typeof (stream as StreamLink).slug === "string"
    )) : [];
  } catch {
    return [];
  }
}

function MobileNavigation(props: { username: string; streams: StreamLink[]; activeStream: string; activeHref: string }) {
  return (
    <Sheet>
      <SheetTrigger as={Button} variant="outline" size="sm">Menu</SheetTrigger>
      <SheetContent position="left" class="mobile-sheet">
        <SheetHeader>
          <SheetTitle>Rill navigation</SheetTitle>
          <SheetDescription>Open a library view or stream.</SheetDescription>
        </SheetHeader>
        <nav class="sheet-nav" aria-label="Main navigation">
          {primaryLinks.map(({ href, label }) => <a href={href} aria-current={props.activeHref === href ? "page" : undefined}>{label}</a>)}
        </nav>
        {props.streams.length > 0 && (
          <>
            <Separator />
            <nav class="sheet-nav" aria-label="Streams">
              <h3>Streams</h3>
              {props.streams.map((stream) => <a href={`/stream/${stream.slug}`} aria-current={stream.slug === props.activeStream ? "page" : undefined}>{stream.name}</a>)}
            </nav>
          </>
        )}
        <Separator />
        <p class="sheet-account">Signed in as <strong>{props.username}</strong></p>
        <nav class="sheet-nav" aria-label="Account">
          {accountLinks.map(({ href, label }) => <a href={href} aria-current={props.activeHref === href ? "page" : undefined}>{label}</a>)}
        </nav>
        <div class="theme-picker" aria-label="Theme">
          <span>Theme</span>
          <div class="theme-picker-controls">
            <Button type="button" variant={theme() === "light" ? "secondary" : "ghost"} size="sm" aria-pressed={theme() === "light"} onClick={() => chooseTheme("light")}>Light</Button>
            <Button type="button" variant={theme() === "dark" ? "secondary" : "ghost"} size="sm" aria-pressed={theme() === "dark"} onClick={() => chooseTheme("dark")}>Dark</Button>
          </div>
        </div>
      </SheetContent>
    </Sheet>
  );
}

function AccountMenu(props: { username: string }) {
  return (
    <DropdownMenu modal={false} placement="top-end">
      <DropdownMenuTrigger as={Button} variant="ghost" class="account-menu-trigger" aria-label={`Account menu for ${props.username}`}>
        <Avatar class="size-9"><AvatarFallback class="account-avatar !bg-primary !text-primary-foreground">{props.username.slice(0, 1).toUpperCase()}</AvatarFallback></Avatar>
        <strong>{props.username}</strong>
      </DropdownMenuTrigger>
      <DropdownMenuContent class="max-h-[min(24rem,calc(100dvh-1rem))] w-56 overflow-y-auto">
        <DropdownMenuLabel>{props.username}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        {accountLinks.map(({ href, label }) => <DropdownMenuItem as="a" href={href} class="pl-8 no-underline">{label}</DropdownMenuItem>)}
        <DropdownMenuSeparator />
        <DropdownMenuLabel class="pl-8">Theme</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={theme()} onChange={chooseTheme}>
          <DropdownMenuRadioItem value="light">Light</DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="dark">Dark</DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

interface FeedSource { id: string; name: string; }

let refreshFeedRows = (): void => {};

function FeedToolbar() {
  const topics = ["All topics", ...Array.from(new Set(
    Array.from(document.querySelectorAll<HTMLElement>("[data-story-id]"))
      .flatMap((row) => (row.dataset.topics ?? "").split(",").filter(Boolean)),
  )).sort((left, right) => left.localeCompare(right))];
  const [view, setView] = createSignal("all");
  const [query, setQuery] = createSignal("");
  const [topic, setTopic] = createSignal("All topics");
  const [sources, setSources] = createSignal<FeedSource[]>([]);
  const [sourceId, setSourceId] = createSignal("");
  const [sort, setSort] = createSignal("Ranked");
  const [compact, setCompact] = createSignal(false);

  const updateRows = () => {
    const list = document.querySelector<HTMLElement>("[data-story-list]");
    if (!list) return;
    const rows = Array.from(list.querySelectorAll<HTMLElement>("[data-story-id]"));
    const normalizedQuery = query().trim().toLocaleLowerCase();
    for (const row of rows) {
      const matchesQuery = !normalizedQuery || (row.textContent ?? "").toLocaleLowerCase().includes(normalizedQuery);
      const matchesView = view() !== "unread" || row.dataset.read !== "true";
      const matchesTopic = topic() === "All topics" || (row.dataset.topics ?? "").split(",").includes(topic());
      const matchesSource = !sourceId() || (row.dataset.sourceIds ?? "").split(",").includes(sourceId());
      row.hidden = !(matchesQuery && matchesView && matchesTopic && matchesSource);
    }
    list.classList.toggle("compact-stories", compact());
    rows.sort((left, right) => {
      if (sort() === "Newest") return (right.dataset.publishedAt ?? "").localeCompare(left.dataset.publishedAt ?? "");
      return Number(left.dataset.rank ?? "0") - Number(right.dataset.rank ?? "0");
    });
    for (const row of rows) {
      const item: HTMLElement | null = row.parentElement === list ? row : row.closest<HTMLElement>(".infinite-feed-page");
      if (item?.parentElement === list) list.append(item);
    }
  };
  refreshFeedRows = updateRows;
  onMount(() => {
    void fetch("/api/v1/sources", { credentials: "same-origin" })
      .then(async (response) => response.ok ? response.json() as Promise<FeedSource[]> : [])
      .then(setSources);
  });

  return (
    <details class="feed-filters">
      <summary aria-label="Filters" title="Filters"><SlidersHorizontalIcon /><span class="sr-only">Filters</span></summary>
      <div class="feed-filter-panel" aria-label="Story filters">
          <Tabs class="feed-view-filter" value={view()} onChange={(value) => { setView(value); updateRows(); }}>
            <TabsList aria-label="Story view">
              <TabsTrigger value="all">All</TabsTrigger>
              <TabsTrigger value="unread">Unread</TabsTrigger>
            </TabsList>
            <TabsContent value="all" class="sr-only">Showing all stories.</TabsContent>
            <TabsContent value="unread" class="sr-only">Showing stories not marked read.</TabsContent>
          </Tabs>
          <TextField class="feed-search">
            <TextFieldLabel class="sr-only">Filter stories</TextFieldLabel>
            <TextFieldInput type="search" placeholder="Filter stories" value={query()} onInput={(event) => { setQuery(event.currentTarget.value); updateRows(); }} />
          </TextField>
          <Select<string>
            options={topics}
            value={topic()}
            onChange={(value) => { if (value) setTopic(value); updateRows(); }}
            class="feed-topic"
            itemComponent={(itemProps) => <SelectItem item={itemProps.item}>{itemProps.item.rawValue}</SelectItem>}
          >
            <SelectLabel class="sr-only">Topic</SelectLabel>
            <SelectTrigger class="feed-sort"><SelectValue<string>>{(state) => state.selectedOption()}</SelectValue></SelectTrigger>
            <SelectContent />
          </Select>
          <label class="feed-source">
            <span class="sr-only">Source</span>
            <select aria-label="Source" value={sourceId()} onChange={(event) => { setSourceId(event.currentTarget.value); updateRows(); }}>
              <option value="">All sources</option>
              {sources().map((source) => <option value={source.id}>{source.name}</option>)}
            </select>
          </label>
          <Select<string>
            options={["Ranked", "Newest"]}
            value={sort()}
            onChange={(value) => { if (value) setSort(value); updateRows(); }}
            class="feed-order"
            itemComponent={(itemProps) => <SelectItem item={itemProps.item}>{itemProps.item.rawValue}</SelectItem>}
          >
            <SelectLabel class="sr-only">Story order</SelectLabel>
            <SelectTrigger class="feed-sort"><SelectValue<string>>{(state) => state.selectedOption()}</SelectValue></SelectTrigger>
            <SelectContent />
          </Select>
          <Switch checked={compact()} onChange={(value) => { setCompact(value); updateRows(); }} class="compact-switch">
            <SwitchControl><SwitchThumb /></SwitchControl>
            <SwitchLabel>Compact</SwitchLabel>
          </Switch>
      </div>
    </details>
  );
}

function StoryFeedback(props: { storyId: string }) {
  const [selected, setSelected] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const choose = async (feedback: string) => {
    if (busy()) return;
    setBusy(true);
    if (await mutate(`/api/v1/stories/${props.storyId}/feedback`, { feedback })) setSelected(feedback);
    setBusy(false);
  };
  return (
    <div class="feedback" aria-label="Story feedback">
       <Toggle aria-label="Like" title="Like" pressed={selected() === "like"} onChange={() => void choose("like")} disabled={busy()} variant="outline" size="sm"><ThumbsUpIcon /><span class="feedback-label">Like</span></Toggle>
       <Toggle aria-label="Dislike" title="Dislike" pressed={selected() === "dislike"} onChange={() => void choose("dislike")} disabled={busy()} variant="outline" size="sm"><ThumbsDownIcon /><span class="feedback-label">Dislike</span></Toggle>
       <Toggle aria-label="Favorite" title="Favorite" pressed={selected() === "favorite"} onChange={() => void choose("favorite")} disabled={busy()} variant="outline" size="sm"><BookmarkIcon /><span class="feedback-label">Favorite</span></Toggle>
    </div>
  );
}

function StoryActions(props: { storyId: string; initialRead: boolean; initialFavorite: boolean; initialFeedback: string }) {
  const [read, setRead] = createSignal(props.initialRead);
  const [favorite, setFavorite] = createSignal(props.initialFavorite);
  const [feedback, setFeedback] = createSignal(props.initialFeedback);
  const [error, setError] = createSignal("");
  const setReadState = async () => {
    const next = !read();
    if (await mutate(`/api/v1/stories/${props.storyId}/read-state`, { read: next })) setRead(next);
    else setError("Read state could not be changed. Try again.");
  };
  const setStoryFeedback = async (value: string) => {
    if (await mutate(`/api/v1/stories/${props.storyId}/feedback`, { feedback: value })) {
      setFeedback(value);
      if (value === "favorite") setFavorite(true);
    } else setError("Feedback could not be saved. Try again.");
  };
  return (
    <div class="story-actions-client">
      {error() && <Alert variant="destructive"><AlertDescription>{error()}</AlertDescription></Alert>}
      <div class="feedback" aria-label="Story controls">
        <Button type="button" variant="outline" size="sm" onClick={() => void setReadState()}>{read() ? <EyeOffIcon /> : <EyeIcon />} Mark {read() ? "unread" : "read"}</Button>
        <Toggle pressed={feedback() === "like"} onChange={() => void setStoryFeedback("like")} variant="outline" size="sm"><ThumbsUpIcon /> Like</Toggle>
        <Toggle pressed={feedback() === "dislike"} onChange={() => void setStoryFeedback("dislike")} variant="outline" size="sm"><ThumbsDownIcon /> Dislike</Toggle>
        <Toggle pressed={favorite()} onChange={() => void setStoryFeedback("favorite")} variant="outline" size="sm">{favorite() ? <BookmarkCheckIcon /> : <BookmarkIcon />} {favorite() ? "Favorited" : "Favorite"}</Toggle>
      </div>
    </div>
  );
}

interface StreamFeedResponse {
  stories: StoryCardModel[];
  hasMore: boolean;
}

function activateStoryFeedback(root: ParentNode): void {
  root.querySelectorAll<HTMLElement>("[data-story-feedback-enhancement]").forEach((host) => {
    if (host.dataset.enhanced === "true") return;
    render(() => <StoryFeedback storyId={host.dataset.storyId ?? ""} />, host);
    finishMount(host);
  });
}

function activateInfiniteFeed(): void {
  const sentinel = document.querySelector<HTMLElement>("[data-infinite-feed]");
  const list = document.querySelector<HTMLElement>("[data-story-list]");
  if (!sentinel || !list || !("IntersectionObserver" in window)) return;
  let offset = Number(sentinel.dataset.offset ?? "0");
  let busy = false;
  const observer = new IntersectionObserver((entries) => {
    if (busy || !entries.some((entry) => entry.isIntersecting)) return;
    busy = true;
    const slug = encodeURIComponent(sentinel.dataset.stream ?? "home");
    void fetch(`/api/v1/streams/${slug}/feed?offset=${offset}&limit=10`, { credentials: "same-origin" })
      .then(async (response) => {
        if (!response.ok) throw new Error();
        const page = await response.json() as StreamFeedResponse;
        if (page.stories.length > 0) {
          page.stories.forEach((story, index) => {
            const host = document.createElement("div");
            host.className = "infinite-feed-page";
            list.append(host);
            render(() => <StoryCard story={story} />, host);
            const row = host.querySelector<HTMLElement>("[data-story-id]");
            if (row) row.dataset.rank = String(offset + index);
            activateStoryFeedback(host);
          });
          offset += page.stories.length;
          refreshFeedRows();
        }
        if (!page.hasMore || page.stories.length === 0) {
          observer.disconnect();
          sentinel.remove();
        }
      })
      .catch(() => { sentinel.textContent = "More stories could not be loaded."; observer.disconnect(); })
      .finally(() => { busy = false; });
  }, { rootMargin: "400px 0px" });
  observer.observe(sentinel);
}

export function activateModernEnhancements(): void {
  document.querySelectorAll<HTMLElement>("[data-story-id]").forEach((row, rank) => { row.dataset.rank = String(rank); });

  document.querySelectorAll<HTMLElement>("[data-mobile-nav-enhancement]").forEach((host) => {
    render(() => <MobileNavigation username={host.dataset.username ?? "Account"} streams={parseStreams(host.dataset.streams)} activeStream={host.dataset.activeStream ?? ""} activeHref={host.dataset.activeHref ?? ""} />, host);
    finishMount(host);
  });
  document.querySelectorAll<HTMLElement>("[data-account-enhancement]").forEach((host) => {
    render(() => <AccountMenu username={host.dataset.username ?? "Account"} />, host);
    finishMount(host);
  });
  document.querySelectorAll<HTMLElement>("[data-feed-toolbar-enhancement]").forEach((host) => {
    render(() => <FeedToolbar />, host);
    finishMount(host);
  });
  activateStoryFeedback(document);
  document.querySelectorAll<HTMLElement>("[data-story-actions-enhancement]").forEach((host) => {
    render(() => <StoryActions storyId={host.dataset.storyId ?? ""} initialRead={host.dataset.read === "true"} initialFavorite={host.dataset.favorite === "true"} initialFeedback={host.dataset.feedback ?? ""} />, host);
    finishMount(host);
  });
  activateInfiniteFeed();
}
