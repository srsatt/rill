import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import type { StreamLink } from "../../generated/render-contract";
import { primaryLinks } from "../components/ModernShell";
import { Alert, AlertDescription } from "../components/ui/alert";
import { Avatar, AvatarFallback } from "../components/ui/avatar";
import { Button } from "../components/ui/button";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
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
import { Tooltip, TooltipContent, TooltipTrigger } from "../components/ui/tooltip";

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
      </SheetContent>
    </Sheet>
  );
}

function AccountMenu(props: { username: string }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger as={Button} variant="ghost" class="account-menu-trigger">
        <Avatar class="size-9"><AvatarFallback>{props.username.slice(0, 1).toUpperCase()}</AvatarFallback></Avatar>
        <span><strong>{props.username}</strong><small>Account menu</small></span>
      </DropdownMenuTrigger>
      <DropdownMenuContent class="w-56">
        <DropdownMenuLabel>{props.username}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem as="a" href="/reader">Reader mode</DropdownMenuItem>
        <DropdownMenuItem as="a" href="/settings/readers">Settings</DropdownMenuItem>
        <DropdownMenuItem as="a" href="/sources">Sources and streams</DropdownMenuItem>
        <DropdownMenuItem as="a" href="/admin">Administration</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function FeedToolbar() {
  const topics = ["All topics", ...Array.from(new Set(
    Array.from(document.querySelectorAll<HTMLElement>("[data-story-id]"))
      .flatMap((row) => (row.dataset.topics ?? "").split(",").filter(Boolean)),
  )).sort((left, right) => left.localeCompare(right))];
  const [view, setView] = createSignal("all");
  const [query, setQuery] = createSignal("");
  const [topic, setTopic] = createSignal("All topics");
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
      row.hidden = !(matchesQuery && matchesView && matchesTopic);
    }
    list.classList.toggle("compact-stories", compact());
    rows.sort((left, right) => {
      if (sort() === "Newest") return (right.dataset.publishedAt ?? "").localeCompare(left.dataset.publishedAt ?? "");
      return Number(left.dataset.rank ?? "0") - Number(right.dataset.rank ?? "0");
    });
    for (const row of rows) list.append(row);
  };

  return (
    <div class="feed-toolbar" aria-label="Story filters">
      <Tabs value={view()} onChange={(value) => { setView(value); updateRows(); }}>
        <TabsList aria-label="Story view">
          <TabsTrigger value="all">All</TabsTrigger>
          <TabsTrigger value="unread">Unread</TabsTrigger>
        </TabsList>
        <TabsContent value="all" class="sr-only">Showing all stories.</TabsContent>
        <TabsContent value="unread" class="sr-only">Showing stories not marked read.</TabsContent>
      </Tabs>
      <TextField class="feed-search">
        <TextFieldLabel class="sr-only">Filter stories on this page</TextFieldLabel>
        <TextFieldInput type="search" placeholder="Filter this page" value={query()} onInput={(event) => { setQuery(event.currentTarget.value); updateRows(); }} />
      </TextField>
      {topics.length > 1 ? <Select<string>
        options={topics}
        value={topic()}
        onChange={(value) => { if (value) setTopic(value); updateRows(); }}
        itemComponent={(itemProps) => <SelectItem item={itemProps.item}>{itemProps.item.rawValue}</SelectItem>}
      >
        <SelectLabel class="sr-only">Topic</SelectLabel>
        <SelectTrigger class="feed-sort"><SelectValue<string>>{(state) => state.selectedOption()}</SelectValue></SelectTrigger>
        <SelectContent />
      </Select> : null}
      <Select<string>
        options={["Ranked", "Newest"]}
        value={sort()}
        onChange={(value) => { if (value) setSort(value); updateRows(); }}
        itemComponent={(itemProps) => <SelectItem item={itemProps.item}>{itemProps.item.rawValue}</SelectItem>}
      >
        <SelectLabel class="sr-only">Story order</SelectLabel>
        <SelectTrigger class="feed-sort"><SelectValue<string>>{(state) => state.selectedOption()}</SelectValue></SelectTrigger>
        <SelectContent />
      </Select>
      <Switch checked={compact()} onChange={(value) => { setCompact(value); updateRows(); }} class="compact-switch">
        <SwitchControl><SwitchThumb /></SwitchControl>
        <SwitchLabel>Compact summaries</SwitchLabel>
      </Switch>
      <Tooltip>
        <TooltipTrigger as={Button} variant="ghost" size="sm">Ranking help</TooltipTrigger>
        <TooltipContent>Ranked order follows this stream's instruction and your explicit feedback.</TooltipContent>
      </Tooltip>
    </div>
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
      {[["like", "Like"], ["dislike", "Dislike"], ["favorite", "Favorite"]].map(([value, label]) => (
        <Toggle pressed={selected() === value} onChange={() => void choose(value)} disabled={busy()} variant="outline" size="sm">{label}</Toggle>
      ))}
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
        <Button type="button" variant="outline" size="sm" onClick={() => void setReadState()}>Mark {read() ? "unread" : "read"}</Button>
        <Toggle pressed={feedback() === "like"} onChange={() => void setStoryFeedback("like")} variant="outline" size="sm">Like</Toggle>
        <Toggle pressed={feedback() === "dislike"} onChange={() => void setStoryFeedback("dislike")} variant="outline" size="sm">Dislike</Toggle>
        <Toggle pressed={favorite()} onChange={() => void setStoryFeedback("favorite")} variant="outline" size="sm">{favorite() ? "Favorited" : "Favorite"}</Toggle>
      </div>
    </div>
  );
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
  document.querySelectorAll<HTMLElement>("[data-story-feedback-enhancement]").forEach((host) => {
    render(() => <StoryFeedback storyId={host.dataset.storyId ?? ""} />, host);
    finishMount(host);
  });
  document.querySelectorAll<HTMLElement>("[data-story-actions-enhancement]").forEach((host) => {
    render(() => <StoryActions storyId={host.dataset.storyId ?? ""} initialRead={host.dataset.read === "true"} initialFavorite={host.dataset.favorite === "true"} initialFeedback={host.dataset.feedback ?? ""} />, host);
    finishMount(host);
  });
}
