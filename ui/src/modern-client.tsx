import { hydrate } from "solid-js/web";
import { ModernFeed } from "./pages/ModernFeed";
import { Story } from "./pages/Story";
import { Library } from "./pages/Library";
import { readHydrationState } from "./shared/hydration";
import type { AdminPageModel, FeedPageModel, LibraryPageModel, ReaderSettingsPageModel, SourcesPageModel, StoryPageModel } from "../generated/render-contract";
import "./app.css";

const root = document.getElementById("rill-root");
if (!(root instanceof HTMLElement)) {
  throw new Error("missing Rill root");
}
const page = readHydrationState<AdminPageModel | FeedPageModel | LibraryPageModel | ReaderSettingsPageModel | SourcesPageModel | StoryPageModel>();
const renderId = root.dataset.renderId;
if (!renderId) {
  throw new Error("missing render ID");
}
const activateEnhancements = () => {
  void import("./enhancements/modern").then((module) => module.activateModernEnhancements());
};
if (window.location.pathname === "/admin") {
  void Promise.all([import("./pages/Admin"), import("./admin-client")]).then(([pageModule, clientModule]) => {
    hydrate(() => <pageModule.Admin page={page as AdminPageModel} />, root, { renderId });
    activateEnhancements();
    clientModule.activateAdmin();
  });
} else if (window.location.pathname === "/sources") {
  void Promise.all([import("./pages/Sources"), import("./source-client")]).then(([pageModule, clientModule]) => {
    hydrate(() => <pageModule.Sources page={page as SourcesPageModel} />, root, { renderId });
    activateEnhancements();
    clientModule.activateSources();
  });
} else if (window.location.pathname.startsWith("/settings/readers")) {
  void Promise.all([import("./pages/ReaderSettings"), import("./settings-client")]).then(([pageModule, clientModule]) => {
    hydrate(() => <pageModule.ReaderSettings page={page as ReaderSettingsPageModel} csrfToken={csrfToken()} />, root, { renderId });
    activateEnhancements();
    clientModule.activateUserSettings();
  });
} else {
  hydrate(
    () => window.location.pathname.startsWith("/story/")
      ? <Story page={page as StoryPageModel} />
      : ["/search", "/favorites", "/history"].includes(window.location.pathname)
        ? <Library page={page as LibraryPageModel} />
        : <ModernFeed page={page as FeedPageModel} />,
    root,
    { renderId },
  );
  activateEnhancements();
}

function csrfToken(): string {
  return document.cookie
    .split(";")
    .map((part) => part.trim().split("="))
    .find(([name]) => name === "rill_csrf")?.[1] ?? "";
}
