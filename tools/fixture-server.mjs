import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";

const port = Number(process.env.RILL_FIXTURE_PORT ?? "3011");
const localize = (source) => Buffer.from(source.replaceAll("127.0.0.1:3011", `127.0.0.1:${port}`));
const feed = localize(await readFile(new URL("../fixtures/rss/feed.xml", import.meta.url), "utf8"));
const largeFeed = localize(await readFile(new URL("../fixtures/rss/large-roundup.xml", import.meta.url), "utf8"));
const visualAuditFeed = Buffer.from(`<?xml version="1.0" encoding="UTF-8"?><rss version="2.0"><channel><title>Rill visual audit</title><link>http://127.0.0.1:${port}/</link><description>Deterministic visual audit stories</description>${Array.from({ length: 25 }, (_, index) => {
  const number = index + 1;
  const title = number === 1
    ? "A deliberately long story title that verifies wrapping across narrow cards without clipping controls, metadata, summaries, or source names"
    : `Visual audit story ${String(number).padStart(2, "0")} with distinct deterministic content`;
  return `<item><guid>visual-audit-${number}</guid><title>${title}</title><link>http://127.0.0.1:${port}/article/visual-audit-${number}</link><comments>http://127.0.0.1:${port}/discussion/visual-audit-${number}</comments><description>Story ${number} carries distinct deterministic content for pagination, ranking, wrapping, and responsive layout checks. The summary remains long enough to exercise several lines without relying on public services.</description><pubDate>Tue, ${String(number).padStart(2, "0")} Jul 2026 12:00:00 GMT</pubDate></item>`;
}).join("")}</channel></rss>`);
assert.equal((visualAuditFeed.toString().match(/<item>/g) ?? []).length, 25);
const actionRequests = [];

function json(response, value) {
  const body = Buffer.from(JSON.stringify(value));
  response.writeHead(200, { "content-type": "application/json", "content-length": body.length });
  response.end(body);
}

createServer(async (request, response) => {
  if (request.url === "/rss.xml") {
    response.writeHead(200, { "content-type": "application/rss+xml", "content-length": feed.length, etag: '"rill-fixture-1"' });
    response.end(feed);
    return;
  }
  if (request.url === "/large-rss.xml") {
    response.writeHead(200, { "content-type": "application/rss+xml", "content-length": largeFeed.length, etag: '"rill-large-fixture-1"' });
    response.end(largeFeed);
    return;
  }
  if (request.url === "/visual-audit-rss.xml") {
    response.writeHead(200, { "content-type": "application/rss+xml", "content-length": visualAuditFeed.length, etag: '"rill-visual-audit-1"' });
    response.end(visualAuditFeed);
    return;
  }
  if (request.url?.startsWith("/article/")) {
    const title = request.url.split("/").at(-1)?.replaceAll("-", " ") ?? "fixture article";
    const body = Buffer.from(`<html><head><title>${title}</title></head><body><article><h1>${title}</h1><p>This deterministic fixture article contains enough concrete text for extraction, summaries, embeddings, clustering, and browser tests.</p><p>It never requires public network access or credentials.</p></article></body></html>`);
    response.writeHead(200, { "content-type": "text/html; charset=utf-8", "content-length": body.length });
    response.end(body);
    return;
  }
  if (request.url === "/v1/embeddings") {
    let raw = "";
    for await (const chunk of request) raw += chunk;
    const input = JSON.parse(raw || "{}").input ?? [];
    json(response, { data: input.map((value, index) => ({ index, embedding: [...createHash("sha256").update(String(value)).digest()].map(byte => (byte - 127.5) / 127.5) })) });
    return;
  }
  if (request.url === "/v1/chat/completions") {
    let raw = "";
    for await (const chunk of request) raw += chunk;
    const prompt = JSON.parse(raw || "{}").messages?.at(-1)?.content ?? "";
    let content = `Fixture summary ${createHash("sha256").update(prompt).digest("hex").slice(0, 8)} reports a concrete change with deterministic implementation detail.`;
    if (prompt.startsWith("Classify a possible link collection.")) {
      const allowedUrls = prompt.split("\nAllowed URLs: ").at(-1)?.split("\n").filter(Boolean) ?? [];
      content = JSON.stringify({
        isCollection: allowedUrls.length > 1,
        confidence: allowedUrls.length > 1 ? 0.99 : 0.1,
        entries: allowedUrls.map((url, index) => ({
          url,
          titleHint: `Fixture link ${index + 1}`,
          commentary: `Fixture commentary ${index + 1}`,
          authorHint: null,
          confidence: 0.99
        }))
      });
    }
    json(response, { choices: [{ message: { content } }] });
    return;
  }
  if (request.url === "/rank") {
    let raw = "";
    for await (const chunk of request) raw += chunk;
    const input = JSON.parse(raw || "{}");
    json(response, { requestId: "fixture-rank", ranked: (input.candidates ?? []).map((candidate, index) => ({ storyId: candidate.storyId, score: 1 - index / 100, features: { fixture: 1 } })) });
    return;
  }
  if (request.url === "/action/requests") {
    if (request.method === "DELETE") actionRequests.length = 0;
    json(response, actionRequests);
    return;
  }
  if (request.url === "/action") {
    let raw = "";
    for await (const chunk of request) raw += chunk;
    actionRequests.push({ method: request.method, headers: request.headers, body: JSON.parse(raw || "null") });
    json(response, { accepted: true });
    return;
  }
  if (request.url === "/feedback") {
    json(response, { accepted: true });
    return;
  }
  if (["/health", "/v1/models"].includes(request.url ?? "")) {
    json(response, { ready: true, data: [] });
    return;
  }
  response.writeHead(404).end();
}).listen(port, "127.0.0.1", () => {
  process.stdout.write(`Rill fixture server listening on http://127.0.0.1:${port}\n`);
});
