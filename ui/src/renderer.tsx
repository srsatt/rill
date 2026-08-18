import { readFileSync } from "node:fs";
import { renderToString } from "solid-js/web";
import type {
  AdminPageModel, FeedPageModel, LibraryPageModel, LoginPageModel, ReaderPairPageModel, ReaderPreferencesPageModel,
  ReaderSettingsPageModel, SourcesPageModel, StoryPageModel,
  RenderRequest, RenderResponse
} from "../generated/render-contract";
import { Login } from "./pages/Login";
import { ModernFeed } from "./pages/ModernFeed";
import { ReaderFeed } from "./pages/ReaderFeed";
import { ReaderPair } from "./pages/ReaderPair";
import { ReaderSettings } from "./pages/ReaderSettings";
import { Admin } from "./pages/Admin";
import { Story } from "./pages/Story";
import { ReaderPreferences } from "./pages/ReaderPreferences";
import { Library } from "./pages/Library";
import { Sources } from "./pages/Sources";

function render(request: RenderRequest): RenderResponse {
  if (request.version !== 1) {
    return errorResponse(400, "Unsupported renderer protocol");
  }
  if (request.template === "modern-feed" && request.mode === "modern") {
    const page = request.props as FeedPageModel;
    return {
      version: 1,
      status: 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <ModernFeed page={page} />, { renderId: request.renderId }),
      hydrationState: page
    };
  }
  if (request.template === "modern-login" && request.mode === "modern") {
    const page = request.props as LoginPageModel;
    return {
      version: 1,
      status: page.error ? 401 : 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <Login page={page} />, { renderId: request.renderId }),
      hydrationState: null
    };
  }
  if (request.template === "modern-library" && request.mode === "modern") {
    const page = request.props as LibraryPageModel;
    return {
      version: 1,
      status: 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <Library page={page} />, { renderId: request.renderId }),
      hydrationState: page
    };
  }
  if (request.template === "modern-sources" && request.mode === "modern") {
    const page = request.props as SourcesPageModel;
    return {
      version: 1,
      status: 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <Sources page={page} />, { renderId: request.renderId }),
      hydrationState: page
    };
  }
  if (request.template === "modern-story" && request.mode === "modern") {
    const page = request.props as StoryPageModel;
    return {
      version: 1,
      status: 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <Story page={page} />, { renderId: request.renderId }),
      hydrationState: page
    };
  }
  if (request.template === "modern-admin" && request.mode === "modern") {
    const page = request.props as AdminPageModel;
    return {
      version: 1,
      status: 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <Admin page={page} />, { renderId: request.renderId }),
      hydrationState: page
    };
  }
  if (request.template === "reader-feed" && request.mode === "reader") {
    const page = request.props as FeedPageModel;
    const csrfToken = request.csrfToken;
    return {
      version: 1,
      status: 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <ReaderFeed page={page} csrfToken={csrfToken} />, { renderId: request.renderId }),
      hydrationState: null
    };
  }
  if (request.template === "reader-pair" && request.mode === "reader") {
    const page = request.props as ReaderPairPageModel;
    const csrfToken = request.csrfToken;
    return {
      version: 1,
      status: page.error ? 400 : 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <ReaderPair page={page} csrfToken={csrfToken} />, { renderId: request.renderId }),
      hydrationState: null
    };
  }
  if (request.template === "reader-story" && request.mode === "reader") {
    const page = request.props as StoryPageModel;
    const csrfToken = request.csrfToken;
    return {
      version: 1,
      status: 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <Story page={page} csrfToken={csrfToken} />, { renderId: request.renderId }),
      hydrationState: null
    };
  }
  if (request.template === "reader-settings" && request.mode === "reader") {
    const page = request.props as ReaderPreferencesPageModel;
    const csrfToken = request.csrfToken;
    return {
      version: 1,
      status: 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <ReaderPreferences page={page} csrfToken={csrfToken} />, { renderId: request.renderId }),
      hydrationState: null
    };
  }
  if (request.template === "modern-reader-settings" && request.mode === "modern") {
    const page = request.props as ReaderSettingsPageModel;
    const csrfToken = request.csrfToken;
    return {
      version: 1,
      status: 200,
      headHtml: `<title>${escapeHtml(page.title)}</title>`,
      bodyHtml: renderToString(() => <ReaderSettings page={page} csrfToken={csrfToken} />, { renderId: request.renderId }),
      hydrationState: null
    };
  }
  return errorResponse(404, "Unknown template");
}

function errorResponse(status: number, message: string): RenderResponse {
  return {
    version: 1,
    status,
    headHtml: `<title>Error</title>`,
    bodyHtml: `<main><h1>${escapeHtml(message)}</h1></main>`,
    hydrationState: null
  };
}

function escapeHtml(value: string): string {
  const output: string[] = [];
  let start = 0;
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    let replacement = "";
    if (character === "&") replacement = "&amp;";
    else if (character === "<") replacement = "&lt;";
    else if (character === ">") replacement = "&gt;";
    else if (character === '"') replacement = "&quot;";
    else if (character === "'") replacement = "&#39;";
    if (!replacement) continue;
    if (start < index) output.push(value.slice(start, index));
    output.push(replacement);
    start = index + 1;
  }
  if (start === 0) return value;
  if (start < value.length) output.push(value.slice(start));
  return output.join("");
}

const requestText = readFileSync(0, "utf8");
const request = JSON.parse(requestText) as RenderRequest;
process.stdout.write(JSON.stringify(render(request)));
