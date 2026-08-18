# Streams, reader pairing, and Actions

## Streams

Every user has a stable `/stream/home` and `/reader/stream/home`. Create more
streams through `POST /api/v1/streams` using a lowercase hyphenated slug.
Filters can include/exclude source, curator, publisher, enriched topic tags, language, text, age,
coverage, read, and Favorite state. An optional semantic description receives a
local embedding; an optional ranking instruction is passed only to a configured
recommendation provider. Provider failure leaves the local scored/diversified
feed usable.

## Reader pairing

Sign in on the modern UI, open `/settings/readers`, name a device, and create a
one-time code. On the reader open `/reader/pair`, enter the code, and submit.
The code expires, is attempt-limited, and is consumed atomically. The reader
gets a separate HttpOnly device cookie and standard-form CSRF token. Revoke the
device from settings; revocation is checked on its next request.

Reader pages contain no script and support stream navigation and feedback with
ordinary HTML forms.

## HTTP Actions

Admins manage Actions at `/admin` or `/api/v1/actions`. An Action has a name,
POST/PUT/PATCH URL, timeout, response cap, retry cap, optional encrypted headers,
and enabled state. The only trigger currently exposed is `story.favorite`.

On Favorite, Rill first persists local feedback. It then inserts an Action
execution and durable job with `action:{action_id}:{feedback_event_id}` as the
idempotency key. Delivery sends a fixed JSON event/story payload and an
`Idempotency-Key` header. Failures never undo Favorite and retry with bounded
backoff. Disable or delete an Action to stop future triggers; a queued disabled
Action completes without making a request.

Treat Action targets as privileged data sinks. Private/non-routable targets are
rejected unless the global private-network fetch policy is deliberately enabled.
