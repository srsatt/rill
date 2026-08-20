# Streams, reader pairing, and Actions

## Streams

Every user has stable Home and All streams. Create and reorder more streams in
the Streams settings tab or through `POST /api/v1/streams`.
Filters can include/exclude source, curator, publisher, enriched topic tags, language, text, age,
coverage, read, and Favorite state. An optional semantic description receives a
local embedding; an optional ranking instruction is passed only to a configured
recommendation provider. Provider failure leaves the local scored/diversified
feed usable.

Experience settings can make subject streams exclusive by user order; Home and
All remain complete. AI-free mode shows raw excerpts, hides generated topics,
and uses deterministic freshness ranking for that user.

## Reader pairing

Sign in on the modern UI, open `/settings/readers`, name a device, and create a
one-time code. On the reader open `/reader/pair`, enter the code, and submit.
The code expires, is attempt-limited, and is consumed atomically. The reader
gets a separate HttpOnly device cookie and standard-form CSRF token. Revoke the
device from settings; revocation is checked on its next request.

Reader pages contain no script and support stream navigation and feedback with
ordinary HTML forms.

## HTTP Actions

Users manage their own Actions in the Actions settings tab or `/api/v1/actions`. An Action has a name,
POST/PUT/PATCH URL, timeout, response cap, retry cap, optional encrypted headers,
and enabled state. The only trigger currently exposed is `story.favorite`.

The browser accepts private header name/value pairs and encrypts values
immediately; it never depends on a server environment-variable name for a
per-user setting and never returns stored values to the browser.

On Favorite, Rill first persists local feedback. It then inserts an Action
execution and durable job with `action:{action_id}:{feedback_event_id}` as the
idempotency key. Delivery sends a fixed JSON event/story payload and an
`Idempotency-Key` header. Failures never undo Favorite and retry with bounded
backoff. Disable or delete an Action to stop future triggers; a queued disabled
Action completes without making a request.

Treat Action targets as privileged data sinks. Private/non-routable targets are
rejected unless the global private-network fetch policy is deliberately enabled.
