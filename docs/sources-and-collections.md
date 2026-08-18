# Sources and collection expansion

## Built-in sources

RSS/Atom sources can be created through the API or `rill sources add-rss`.
Conditional ETag/Last-Modified requests, stable entry IDs, bounded responses,
and OPML parsing are native Rust.

Email sources are created through `POST /api/v1/sources/email`. Rill stores the
password in encrypted secret storage, connects with IMAP/Rustls, advances a UID
cursor, bounds RFC822 size, parses text/HTML MIME, and retains List-Unsubscribe
metadata. One source represents one mailbox/account configuration.

Telegram needs no user-client credentials. Add a public channel username from
Sources, or ask an admin to configure the teloxide bot and bind it through the
one-time deep link. A bound user may forward a public channel post or send an
`@username` to subscribe. Rill fetches `https://t.me/s/<username>`, parses a
bounded page history with recent-edit overlap, and stores one shared source per
channel. Only subscribed users can see that source. Private/protected channels
and deletion detection are intentionally unsupported.

## Collection pipeline

Every connector returns `RawSourceItem`. Rill first decides whether its content
looks like a curated collection. Structured repeated cards and multiple
meaningful links raise confidence; unsubscribe, social, navigation, tracking,
and repeated links are excluded. Fan-out is globally bounded.

An accepted parent creates one normalized `collection_expansion` and ordered
`collection_entries`. Each child becomes its own raw item carrying parent ID,
curator, title hint, and per-link commentary. Reprocessing uses stable identity
and does not duplicate children. Every child independently enters extraction,
summary, embedding, deduplication, clustering, stream matching, and ranking.

Direct and roundup-discovered URLs can converge on one Document/Story, but
`document_curators` remains many-to-many. Collection commentary is stored
separately from model summary text. Feedback attaches to a Story, so explicitly
rating one child does not rate its siblings.

Source and parser health is stored in SQLite. Poll failures retain the prior
cursor, retry through durable jobs, and eventually become dead-letter jobs for
inspection.

Deterministic integration inputs live under `fixtures/rss`, `fixtures/email`,
and `fixtures/telegram`. `node tools/fixture-server.mjs` serves both a normal
feed and a 25-link maximum-fan-out roundup plus article/model/Action endpoints.
