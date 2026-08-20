# Rill PoC completion plan

Status: completed on 2026-08-19. Evidence: [implementation-report.md](implementation-report.md) and [visual-audit.md](visual-audit.md).

## Goal

Ship a coherent local-first PoC in which:

- stories ingest source and discussion links without feed-specific hacks;
- feedback, likes, and embeddings are durable, while recommendation uses a small local logistic-regression model;
- favoriting a story can invoke a generic HTTP action, proven against Karakeep;
- every page has been visually audited at representative viewports;
- Reader mode has clear title/summary hierarchy and remains server-rendered without client JavaScript;
- feedback and navigation controls use a small, consistent icon set.

The experimental vector/attention ranker is out of the PoC. Preserve its files outside the application repository for later work.

## Context capsule

These decisions are enough to resume work after a context reset:

- Keep the current Solid TSX + ScriptC/WASI renderer architecture. Every UI dependency must retain 100% static ScriptC coverage; no dynamic JavaScript runtime.
- Keep Rusqlite work behind the existing bounded blocking boundary.
- Persist raw feedback and story embeddings. Refit a local logistic-regression ranker lazily after a small number of new labeled events; there is no long offline training job.
- Both positive and negative examples are required before enabling personalized ranking. Fall back to the existing deterministic ranking until then.
- Disable any Ollama/provider recommendation override. Ollama remains responsible only for enrichment and embeddings.
- A local Favorite must succeed before an external action is queued. External failure must never undo the local Favorite.
- Do not commit `.env`, tokens, screenshots, databases, model artifacts, or experimental datasets.

## Phase 0 — isolate experiments and checkpoint the baseline

1. Enumerate the exact contents of `experiments/` and confirm the sibling destination resolves to `/Users/pavel.reutov/Documents/prog/rill-experiments`.
2. Move the whole directory without deleting or rewriting its contents, then verify source absence and destination file counts.
3. Preserve the current story-rendering 500 fix and all intentional repository files. Ignore unrelated `.DS_Store`, caches, local databases, and generated artifacts.
4. Run the current focused renderer regression and workspace checks.
5. Before committing, verify the effective Git author and GitHub authentication are the personal `srsatt` identity. Commit the clean baseline; do not push unless separately requested.

Exit condition: experiments live outside the repository, the app baseline is reproducible, and the checkpoint commit contains no secrets or generated artifacts.

## Phase 1 — establish the visual feedback loop

Start the application with a deterministic fixture user and content set. Capture an unannotated baseline screenshot plus accessibility snapshot for every page template. Record console errors, failed requests, overflow, focus problems, and layout defects in `docs/visual-audit.md`. Put screenshot artifacts in ignored `artifacts/visual-audit/before/`.

### Page and state matrix

| Area | Routes/templates | Required states |
| --- | --- | --- |
| Authentication | `/login` | default, invalid credentials |
| Feed | `/`, `/stream/home`, `/stream/{slug}` | populated, empty, long title/summary, feedback selected |
| Story | `/story/{story_id}` | normal, long content, missing/error |
| Library | `/search`, `/favorites`, `/history` | populated, empty, long query/result |
| Sources | `/sources` | empty, configured, validation/error |
| Settings | `/settings/readers` | default, validation/error |
| Administration | `/admin` | populated, narrow viewport, validation/error |
| Reader feed | `/reader`, `/reader/stream/{slug}`, `/reader/page/{page}` | populated, empty, pagination, long title/summary |
| Reader story | `/reader/story/{story_id}` | normal, long content, missing/error |
| Reader setup | `/reader/settings`, `/reader/pair` | default, validation/error, pairing code |

Capture every template at 1440×1000. Also capture the high-risk feed, story, sources, settings, admin, Reader feed/story, and pairing pages at 390×844 and 1024×768.

### Visual acceptance criteria

- No unintended horizontal scrolling, clipping, overlap, obscured controls, or content hidden behind sticky/fixed regions.
- Page gutters and vertical rhythm use a small shared spacing scale; mobile layouts do not merely shrink desktop gaps.
- Hover, pressed, loading, and focus states do not move surrounding geometry.
- Interactive targets are at least 44×44 CSS pixels where touch use is expected.
- Focus indicators remain visible and keyboard order is logical.
- Text meets WCAG AA contrast. Reader title and summary are visibly distinct by size, weight, color, and spacing—not color alone.
- Long titles, summaries, source names, URLs, and translated content wrap without breaking their container.
- No browser console errors or unexpected failed requests on the audited paths.

For the reported Reader hierarchy problem, reproduce it before editing and test these ranked hypotheses independently:

1. title and summary currently share too much color, weight, and scale;
2. insufficient space between title, metadata, and summary merges them visually;
3. summary line length/line-height makes it compete with the title.

## Phase 2 — durable feedback, embeddings, and local recommendation

1. Keep the raw feedback events and embedding vectors as durable data. Verify that restarting the app loses neither.
2. Implement a bounded local feature vector from the stored embedding plus stable non-sensitive story signals.
3. Fit regularized binary logistic regression only when both classes exist. Refit lazily after a configurable small batch of new likes/dislikes (default: five), and cap the fit window (default: 500 recent labeled examples).
4. Store model metadata/version and coefficients atomically. Never block the feedback HTTP request on a refit; schedule it through the job system.
5. Blend the predicted preference score with the existing deterministic rank. If the model is absent, stale, invalid, or under-labeled, use deterministic ranking unchanged.
6. Add tests for cold start, one-class feedback, threshold crossing, refit after several events, restart persistence, corrupt-model fallback, and deterministic tie-breaking.

Exit condition: a few new likes/dislikes update ranking shortly afterward, without a long training run and without discarding the underlying events or embeddings.

## Phase 3 — generic source/discussion link relations

Yes, Hacker News can be supported generically. RSS 2.0 exposes the article URL in `<link>` and the discussion URL in `<comments>`; Atom uses link relations such as `rel="replies"`. The implementation must recognize standards, not `news.ycombinator.com`.

1. Replace untyped external URL handling with a bounded relation model such as `{ url, relation, title, ordinal }`. Support at least `alternate`, `replies`, `related`, `via`, and `other`, while preserving unknown safe relation strings for forward compatibility.
2. Treat the feed entry's normal link as `alternate` and RSS `<comments>` / Atom `rel="replies"` as `replies`.
3. Because the current feed parser omits RSS 2.0 `<comments>`, add a small supplementary standards parser using the existing `quick-xml` dependency and merge its result by stable item identity. Do not inspect hostnames or special-case Hacker News.
4. Accept only absolute HTTP(S) URLs, normalize and deduplicate them, cap count/length, and preserve deterministic order.
5. Persist link relations in normalized storage and expose them through the story view model.
6. Render simple labeled actions: original source and discussion. If both URLs normalize to the same value, render only one action.
7. Keep the canonical article/source URL as the URL sent to Favorite actions; do not accidentally save the comments page.

Tests must cover RSS source + comments, Atom `replies`, a non-Hacker-News RSS feed, malformed/unsafe URLs, duplicate URLs, persistence/reload, and UI rendering.

Exit condition: an HN story shows both “Open original” and “Discussion” when distinct, and the same standard fixture works on a non-HN domain.

## Phase 4 — generic Favorite HTTP actions and Karakeep proof

The existing action lifecycle is correct, but its fixed event body cannot call Karakeep's create-bookmark API. Extend the generic HTTP action rather than adding a Karakeep branch.

1. Add an optional structured JSON body template to HTTP actions. Resolve only a small documented set of scalar values, including story ID, title, summary, canonical URL, source, curator, publication time, related links, and event ID.
2. Preserve the current event JSON as the default when no template is configured, so existing actions remain compatible.
3. Reject unknown placeholders, non-JSON output, oversized templates/output, unsafe target URLs, and excessive response bodies before dispatch.
4. Keep headers encrypted at rest. Accept the per-user header value as a password field and store it immediately through the existing secret store; never return it in browser state, logs, or reports.
5. Add an optional advanced body-template field to action settings, while keeping the default UI simple.
6. Configure the PoC action with the user's private token through the Action settings form. A Karakeep-compatible body is conceptually:

   ```json
   {
     "type": "link",
     "url": "${story.url}",
     "title": "${story.title}",
     "summary": "${story.summary}",
     "source": "api",
     "favourited": true
   }
   ```

7. Unit-test template substitution and escaping. Integration-test against a local mock server for method, path, Authorization header, JSON body, idempotency key, timeout, retry, and response limits.
8. Test the failure contract: the Rill Favorite remains saved when the external endpoint fails, while the action job records a useful retryable/terminal status.
9. Run one live proof through the real Favorite UI after the token exists: favorite a uniquely identifiable fixture story, wait for the action job to succeed, query Karakeep by exact URL, and confirm exactly one bookmark. Repeat the delivery path to confirm it does not create a duplicate. Leave the created test bookmark in place and report its URL/ID; deletion is not part of this task.

Exit condition: Karakeep receives the canonical article exactly once through the normal Favorite workflow, and the same action engine can target an unrelated JSON HTTP endpoint without code changes.

## Phase 5 — simple iconography and visual fixes

1. Add `👍 Like` and `👎 Dislike` consistently to feed cards, story pages, Reader pages, and enhanced browser controls. Preserve visible text, accessible names, and `aria-pressed` state.
2. Use a small Lucide set for non-feedback actions: bookmark/bookmark-check, external-link, message-circle, search, and eye/eye-off where already useful. Keep the set under roughly eight icons, 16–18px, one stroke weight, and mark decorative icons `aria-hidden`.
3. Prefer `lucide-solid`. First prove TypeScript, browser build, SSR, and 100% ScriptC static coverage. If that package crosses the static compiler boundary, use Lucide's official static SVG package/assets through one tiny local component—do not hand-copy unrelated icon markup throughout the UI.
4. Introduce semantic Reader classes for title, metadata, and summary. Strengthen title scale/weight, reduce summary prominence while retaining at least 4.5:1 contrast on white, and increase their separating space.
5. Consolidate page gutters, section gaps, card padding, control heights, borders, and focus rings into shared tokens/components. Fix every issue recorded in the baseline audit, not only the reported Reader screen.
6. Keep Reader mode server-only. No icon or styling change may introduce client JavaScript there.

## Phase 6 — full verification and handoff

1. Re-run the identical page/state/viewport matrix and save screenshots to `artifacts/visual-audit/after/`. Compare every before/after pair and update `docs/visual-audit.md` with resolved and intentionally deferred findings.
2. For each static defect, keep one annotated evidence screenshot. For an interaction defect, keep step screenshots or a short recording. Do not annotate the aesthetic review screenshots themselves.
3. Run:
   - focused Rust tests for feed parsing, persistence, actions, jobs, recommendation, and renderer views;
   - `cargo test --workspace`;
   - `pnpm typecheck` and `pnpm build` in `ui/`;
   - renderer determinism/escaping/resource tests against the stripped WASM artifact;
   - the existing browser E2E suite plus new Favorite/action and link-relation coverage.
4. Verify no debug instrumentation, token values, local databases, screenshots, caches, or model artifacts are tracked.
5. Update the implementation report with a short TL;DR, architecture changes, screenshots/report link, test evidence, live Karakeep evidence, known limitations, and exact follow-up experiments.
6. Inspect the complete diff, verify `srsatt` identity again, and create focused commits. Do not push or release unless separately requested.

## Final definition of done

- Every route template and required state has before/after visual evidence.
- No open critical or major visual defects; any minor deferral is explicitly listed with evidence.
- Reader title/summary hierarchy is visibly improved and objectively AA-compliant.
- Like/dislike show emoji; the remaining icon set is minimal, consistent, accessible, and static-renderer compatible.
- Likes, dislikes, and embeddings survive restart; local logistic ranking updates after a small feedback batch.
- HN and non-HN standard fixtures expose typed original/discussion links without hostname-specific code.
- Favoriting a story persists locally and creates exactly one verified Karakeep bookmark through a generic HTTP action.
- All workspace, renderer, UI, and E2E gates pass; no secret or generated artifact is committed.

## Explicit non-goals for this PoC

- experimental attention/vector-native preference models;
- model-provider feedback learning or provider-controlled ranking;
- bidirectional Favorite deletion or Karakeep synchronization;
- a general-purpose scripting language in HTTP action templates;
- provider/domain-specific RSS parsing;
- a broad icon redesign or icon-only controls.
