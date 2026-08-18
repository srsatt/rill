import { createServer } from "node:http";
import { readFile } from "node:fs/promises";

const port = Number(process.env.RILL_FIXTURE_PORT ?? "3011");
const feed = await readFile(new URL("../fixtures/rss/feed.xml", import.meta.url));
const largeFeed = await readFile(new URL("../fixtures/rss/large-roundup.xml", import.meta.url));

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
    json(response, { data: input.map((_, index) => ({ index, embedding: [0.25, 0.5, 0.75, 1] })) });
    return;
  }
  if (request.url === "/v1/chat/completions") {
    let raw = "";
    for await (const chunk of request) raw += chunk;
    const prompt = JSON.parse(raw || "{}").messages?.at(-1)?.content ?? "";
    let content = "The fixture reports a concrete change with implementation detail. It is deterministic and safe for local tests.";
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
  if (["/feedback", "/action"].includes(request.url ?? "")) {
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
