# Rill visual audit

Status: complete; no critical, major, or deferred visual findings.

## Environment

- Audit dates: 2026-08-18 to 2026-08-19
- App: isolated local server at `127.0.0.1:3015`
- Data: temporary SQLite database with populated and unsubscribed users plus 25 deterministic local RSS stories
- Models: deterministic local fixture summaries and 32-dimensional content-derived embeddings
- Evidence: ignored files under `artifacts/visual-audit/before/` and `artifacts/visual-audit/after/`
- Viewports: 1440x1000 for every route template/state; 390x844 and 1024x768 for feed, story, sources, settings, admin, Reader feed/story, and pairing

## Audited matrix

| Area | Before and after states |
| --- | --- |
| Authentication | default, invalid credentials |
| Feed | `/`, `/stream/home`, `/stream/technology`; populated, empty, long title/summary, selected feedback |
| Story | normal, long content, missing |
| Library | search empty/long result, favorites empty/populated, history empty/populated |
| Sources | empty, configured, validation error |
| Settings | default, validation error, generated pairing code |
| Administration | populated, narrow viewport, duplicate-user error |
| Reader feed | `/reader`, `/reader/stream/technology`, `/reader/page/2`; populated, empty, pagination, long title/summary |
| Reader story | normal, long content, missing |
| Reader setup | settings default, pairing default, invalid pairing code, generated pairing code |

Every aesthetic screenshot has a sibling `.a11y.txt` accessibility snapshot and is unannotated. Separate `issue-*-resolved-*.png` files carry annotations for static defect evidence.

## Resolved findings

### ISSUE-001 — Settings hydration and unsafe form fallback (high, resolved)

The server supplied `null` hydration state for `/settings/readers`, so the client threw while reading `username`. The unenhanced action form could then fall back to a GET and leak entered fields into the URL. The renderer now supplies the settings page model, and the form is intercepted normally. A second route predicate bug found during the after pass treated the POST response path `/settings/readers/pair` as a feed; settings hydration now covers the whole `/settings/readers` subtree.

Evidence: `before/issue-001-settings-hydration-error.png`, `after/issue-001-resolved-settings-hydration.png`, and the default/generated-code settings captures. A clean browser session reported no console errors after generating a pairing code.

### ISSUE-002 — Reader content hierarchy (medium, resolved)

Semantic Reader title, metadata, and summary classes now differ in scale, weight, color, line-height, spacing, and line length. At 390px the long-story title measured 24.8px/700/29.76px, metadata 14px/400/20.3px, and summary 16px/400/27.2px with a 65ch cap. Text colors `#171717`, `#525252`, and `#404040` on white exceed WCAG AA.

Evidence: `before/issue-002-reader-hierarchy.png`, `after/issue-002-resolved-reader-hierarchy.png`, and `after/objective-checks.txt`.

### ISSUE-003 — Mobile touch targets (medium, resolved)

Shared control-height tokens make visible feedback buttons, feed tabs, topic links, and Reader buttons at least 44px high. At 390px, visible modern buttons and topic links both measured 44px; audited pages had zero horizontal overflow.

Evidence: `before/issue-003-mobile-touch-targets.png`, `after/issue-003-resolved-touch-targets.png`, and `after/objective-checks.txt`.

### ISSUE-004 — Missing favicon request (low, resolved)

Modern pages declare `/static/favicon.svg`. The clean network trace records HTTP 200 for that asset and no `/favicon.ico` request.

Evidence: `after/issue-004-resolved-favicon.png` and `after/favicon-network.txt`.

## Acceptance results

- All required after-state and high-risk viewport captures exist; long content wraps without clipping or overflow.
- Feed card geometry is unchanged by hover: the audited card remained at `x=16`, `y=571`, `width=358`, `height=416.5`, with a constant 1px left border.
- Skip-link-first keyboard order and visible focus rings remain covered by browser E2E.
- Expected validation requests return controlled 400/409 responses; no unexpected resource failures occurred.
- Reader routes contain zero `<script>` elements and use only server-rendered HTML, CSS, and the static Lucide sprite.
- No visual finding is intentionally deferred.
