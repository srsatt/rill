import { expect, test } from "@playwright/test";

const fixtureUrl = `http://127.0.0.1:${process.env.RILL_E2E_FIXTURE_PORT ?? "3011"}`;

interface ProbeWindow extends Window {
  __rillRootReplaced?: boolean;
}

async function login(page: import("@playwright/test").Page) {
  await page.goto("/login");
  await page.getByLabel("Username or email").fill("admin");
  await page.getByLabel("Password").fill("rill-e2e-password");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/stream\/home$/);
}

test("authenticated Solid feed hydrates in place", async ({ page }) => {
  await login(page);
  const consoleErrors: string[] = [];
  const failedAssets: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("requestfailed", (request) => {
    if (request.url().includes("/static/")) failedAssets.push(request.url());
  });
  await page.addInitScript(() => {
    const probe = window as ProbeWindow;
    probe.__rillRootReplaced = false;
    let originalRoot: Element | null = null;
    new MutationObserver(() => {
      const currentRoot = document.getElementById("rill-root");
      if (!originalRoot && currentRoot) originalRoot = currentRoot;
      if (originalRoot && (currentRoot !== originalRoot || !originalRoot.isConnected)) {
        probe.__rillRootReplaced = true;
      }
    }).observe(document, { childList: true, subtree: true });
  });

  await page.goto("/stream/home", { waitUntil: "networkidle" });
  await expect(page.getByRole("heading", { name: "Home" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Give Rill one good source" })).toBeVisible();
  await expect(page.locator("[data-feed-toolbar-enhancement][data-enhanced=true]")).toBeVisible();
  await expect(page.getByRole("tab", { name: "All" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByLabel("Filter stories")).toBeVisible();
  const styles = await page.locator('link[rel="stylesheet"]').evaluateAll((links) => links.map((link) => (link as HTMLLinkElement).sheet !== null));
  expect(styles).toEqual([true]);

  expect(await page.evaluate(() => (window as ProbeWindow).__rillRootReplaced)).toBe(false);
  expect(consoleErrors).toEqual([]);
  expect(failedAssets).toEqual([]);
});

test("modern shell reflows at 320px and exposes a keyboard-safe mobile sheet", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 760 });
  await login(page);
  await page.goto("/stream/home", { waitUntil: "networkidle" });
  await expect(page.locator("[data-mobile-nav-enhancement][data-enhanced=true]")).toBeVisible();

  const menu = page.getByRole("button", { name: "Menu" });
  await expect(menu).toBeVisible();
  const menuBox = await menu.boundingBox();
  expect(menuBox?.width).toBeGreaterThanOrEqual(44);
  expect(menuBox?.height).toBeGreaterThanOrEqual(44);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);

  await menu.click();
  await expect(page.getByRole("dialog", { name: "Rill navigation" })).toBeVisible();
  await expect(page.getByRole("dialog").getByRole("link", { name: "Sources" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(menu).toBeFocused();
});

test("modern shell starts with a skip link and named navigation landmarks", async ({ page }) => {
  await login(page);
  await page.goto("/stream/home", { waitUntil: "networkidle" });
  await page.keyboard.press("Tab");
  const skipLink = page.getByRole("link", { name: "Skip to main content" });
  await expect(skipLink).toBeFocused();
  await expect(skipLink).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.locator("#main-content")).toBeFocused();
  await expect(page.getByRole("navigation", { name: "Main navigation" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Streams" })).toBeVisible();
  const sidebarBox = await page.locator(".app-sidebar").boundingBox();
  const accountBox = await page.getByRole("button", { name: /Account menu/ }).boundingBox();
  expect(sidebarBox).not.toBeNull();
  expect(accountBox).not.toBeNull();
  expect((accountBox?.x ?? 0) - (sidebarBox?.x ?? 0)).toBeLessThan(32);
});

test("account theme picker persists light and dark choices", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: /Account menu/ }).click();
  await page.getByRole("menuitemradio", { name: "Dark" }).click();
  await expect(page.locator("html")).toHaveClass(/dark/);

  await page.reload();
  await expect(page.locator("html")).toHaveClass(/dark/);
  await page.getByRole("button", { name: /Account menu/ }).click();
  await page.getByRole("menuitemradio", { name: "Light" }).click();
  await expect(page.locator("html")).not.toHaveClass(/dark/);
});

test("reader pairing and feed work with JavaScript disabled", async ({ browser, page }) => {
  await login(page);
  const consoleErrors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  await page.goto("/settings/readers");
  await page.getByRole("tab", { name: "Devices" }).click();
  await page.getByLabel("Device label").fill("Playwright reader");
  await page.getByRole("button", { name: "Create one-time code" }).click();
  const pairingText = await page.locator(".pairing-code strong").innerText();
  expect(consoleErrors).toEqual([]);

  const context = await browser.newContext({ javaScriptEnabled: false });
  const reader = await context.newPage();
  await reader.goto("/reader/pair");
  await reader.getByLabel("Pairing code").fill(pairingText);
  await reader.getByRole("button", { name: "Pair reader" }).click();

  await expect(reader).toHaveURL(/\/reader$/);
  await expect(reader.getByRole("link", { name: "Home" })).toHaveAttribute("aria-current", "page");
  await expect(reader.getByText("No stories yet.")).toBeVisible();
  await expect(reader.locator("script")).toHaveCount(0);
  await context.close();
});

test("source, story feedback, and stream management use the real API", async ({ page }) => {
  await login(page);

  expect((await page.request.delete(`${fixtureUrl}/action/requests`)).ok()).toBe(true);
  await page.goto("/settings/readers");
  await page.getByRole("tab", { name: "Actions" }).click();
  const actionForm = page.locator("#favorite-action-create");
  await actionForm.getByLabel("Name", { exact: true }).fill("Fixture favorite action");
  await actionForm.getByLabel("Destination URL").fill(`${fixtureUrl}/action`);
  await actionForm.locator("summary", { hasText: "Advanced request options" }).click();
  await actionForm.getByLabel("JSON body template").fill('{"url":"${story.url}","title":"${story.title}","eventId":"${eventId}"}');
  await actionForm.getByLabel("Private header name").fill("X-E2E-Token");
  await actionForm.getByLabel("Private header value").fill("private-test-value");
  await actionForm.getByRole("button", { name: "Add favorite action" }).click();
  await expect(page.getByRole("heading", { name: "Fixture favorite action" })).toBeVisible();

  await page.goto("/sources");
  const rssForm = page.locator("#rss-create");
  await rssForm.getByLabel("Name").fill("Fixture RSS");
  await rssForm.getByLabel("Feed URL").fill(`${fixtureUrl}/rss.xml`);
  await rssForm.getByRole("button", { name: "Add feed" }).click();
  const source = page.locator("article").filter({ has: page.getByRole("heading", { name: "Fixture RSS" }) });
  await expect(source).toBeVisible();
  await source.getByRole("button", { name: "Fetch now" }).click();

  await expect.poll(async () => {
    await page.goto("/stream/home");
    return page.getByRole("link", { name: "Germany changes public software procurement" }).count();
  }, { timeout: 20_000 }).toBe(1);
  const storyCard = page.locator(".story-row").first();
  await expect(storyCard.locator(".story-source-link")).toHaveAttribute("href", `${fixtureUrl}/article/direct`);
  const cardBeforeHover = await storyCard.boundingBox();
  expect(cardBeforeHover).not.toBeNull();
  await storyCard.hover();
  await page.waitForTimeout(250);
  const cardAfterHover = await storyCard.boundingBox();
  expect(cardAfterHover?.x).toBe(cardBeforeHover?.x);
  expect(cardAfterHover?.y).toBe(cardBeforeHover?.y);
  await page.getByRole("link", { name: "Germany changes public software procurement" }).click();
  await expect(page.getByRole("link", { name: "Open original" })).toHaveAttribute("href", `${fixtureUrl}/article/direct`);
  await expect(page.getByRole("link", { name: "Discussion" })).toHaveAttribute("href", `${fixtureUrl}/discussion/direct`);
  for (const feedback of ["Like", "Dislike", "Favorite"]) {
    const response = page.waitForResponse((candidate) =>
      candidate.url().includes("/feedback") && candidate.request().method() === "POST");
    await page.getByRole("button", { name: feedback, exact: true }).click();
    expect((await response).ok()).toBe(true);
  }
  await expect.poll(async () => {
    const response = await page.request.get(`${fixtureUrl}/action/requests`);
    return await response.json();
  }, { timeout: 20_000 }).toEqual([
    expect.objectContaining({
      method: "POST",
      headers: expect.objectContaining({ "idempotency-key": expect.any(String), "x-e2e-token": "private-test-value" }),
      body: expect.objectContaining({
        url: `${fixtureUrl}/article/direct`,
        title: "Germany changes public software procurement",
        eventId: expect.any(String),
      }),
    }),
  ]);

  await page.goto("/settings/readers");
  await page.getByRole("tab", { name: "Streams" }).click();
  await page.getByRole("button", { name: "Add stream" }).click();
  const streamForm = page.locator("#stream-create");
  await streamForm.getByLabel("Stream name").fill("Fixture stream");
  await streamForm.getByLabel("What belongs here?").fill("Systems implementation details");
  await streamForm.locator("summary", { hasText: "Advanced tag filters" }).click();
  await streamForm.getByLabel("Include topics").fill("rust, wasm");
  await streamForm.getByLabel("Exclude topics").fill("sponsored");
  await streamForm.getByRole("button", { name: "Create stream" }).click();
  await expect(page.getByRole("link", { name: "Fixture stream" })).toBeVisible();
  let stream = page.locator(".settings-list-entry").filter({ has: page.getByRole("option", { name: /Fixture stream/ }) });
  const homeStream = page.locator(".settings-list-entry").filter({ has: page.getByRole("option", { name: /Home/ }) });
  const transfer = await page.evaluateHandle(() => new DataTransfer());
  await stream.locator(".stream-drag-handle").dispatchEvent("dragstart", { dataTransfer: transfer });
  await homeStream.dispatchEvent("drop", { dataTransfer: transfer });
  await expect(page.getByRole("option").nth(0)).toContainText("All");
  await expect(page.getByRole("option").nth(1)).toContainText("Fixture stream");
  await page.getByRole("option", { name: /Fixture stream/ }).click();
  const streamDetail = page.locator("#stream-detail");
  await expect(streamDetail.getByLabel("Include topics")).toHaveValue("rust, wasm");
  await expect(streamDetail.getByLabel("Exclude topics")).toHaveValue("sponsored");
  await streamDetail.getByLabel("Name").fill("Fixture engineering");
  await streamDetail.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("link", { name: "Fixture engineering" })).toBeVisible();
  await streamDetail.getByRole("button", { name: "Delete" }).click();
  await expect(page.getByRole("link", { name: "Fixture engineering" })).toHaveCount(0);

  await page.getByRole("tab", { name: "Experience" }).click();
  await page.getByLabel("AI-free mode").check();
  await page.getByLabel("Stream assignment").selectOption("exclusive");
  await page.getByLabel("Typography").selectOption("serif");
  await page.getByRole("button", { name: "Save preferences" }).click();
  await expect(page.locator("#preferences-status")).toHaveText("Saved.");
  await expect(page.locator(".modern-app")).toHaveClass(/reading-font-serif/);
  await page.goto("/reader");
  await expect(page.locator(".reader")).toHaveClass(/reading-font-serif/);
  await page.goto("/stream/home");
  await expect(page.locator(".modern-app")).toHaveClass(/reading-font-serif/);
});

test("feed loads older stories without pagination and keeps compact actions inside cards", async ({ page }) => {
  test.setTimeout(60_000);
  await login(page);
  await page.goto("/sources");
  const rssForm = page.locator("#rss-create");
  await rssForm.getByLabel("Name").fill("Visual audit RSS");
  await rssForm.getByLabel("Feed URL").fill(`${fixtureUrl}/visual-audit-rss.xml`);
  await rssForm.getByRole("button", { name: "Add feed" }).click();
  const source = page.locator("article").filter({ has: page.getByRole("heading", { name: "Visual audit RSS" }) });
  await source.getByRole("button", { name: "Fetch now" }).click();

  await expect.poll(async () => {
    const response = await page.request.get("/stream/all");
    return (await response.text()).match(/class="[^"]*story-row/g)?.length ?? 0;
  }, { timeout: 30_000 }).toBe(5);
  await expect.poll(async () => {
    const response = await page.request.get("/api/v1/streams/all/feed?offset=5&limit=10");
    const body = await response.json() as { stories: unknown[] };
    return body.stories.length;
  }, { timeout: 30_000 }).toBeGreaterThan(0);
  await page.goto("/stream/all");
  await expect(page.getByRole("navigation", { name: "Story pages" })).toHaveCount(0);
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect.poll(() => page.locator(".story-row").count(), { timeout: 15_000 }).toBeGreaterThan(5);

  const card = page.locator(".story-row").first();
  const actions = card.locator(".story-feedback-enhancement");
  await expect(actions).toHaveCSS("opacity", "0");
  await page.locator(".compact-switch").click();
  await card.hover();
  await expect(actions).toHaveCSS("opacity", "1");
  const cardBox = await card.boundingBox();
  const actionBox = await card.getByRole("button", { name: "Favorite" }).boundingBox();
  expect(cardBox).not.toBeNull();
  expect(actionBox).not.toBeNull();
  expect((actionBox?.x ?? 0) + (actionBox?.width ?? 0)).toBeLessThanOrEqual((cardBox?.x ?? 0) + (cardBox?.width ?? 0));
  expect((actionBox?.y ?? 0) + (actionBox?.height ?? 0)).toBeLessThanOrEqual((cardBox?.y ?? 0) + (cardBox?.height ?? 0));
  await expect(card.getByText("1 source in this story")).toHaveCount(0);
});

test("account menu stays in the viewport and does not lock page scrolling", async ({ page }) => {
  await login(page);
  await page.goto("/sources", { waitUntil: "networkidle" });
  const before = await page.evaluate(() => {
    window.scrollTo(0, document.documentElement.scrollHeight);
    return window.scrollY;
  });
  await page.getByRole("button", { name: /Account menu/ }).click();
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  const box = await menu.boundingBox();
  const trigger = await page.getByRole("button", { name: /Account menu/ }).boundingBox();
  expect(box).not.toBeNull();
  expect(trigger).not.toBeNull();
  expect(Math.abs((box?.x ?? 0) - (trigger?.x ?? 0))).toBeLessThan(2);
  expect((box?.y ?? 0) + (box?.height ?? 0)).toBeLessThanOrEqual(await page.evaluate(() => window.innerHeight));
  await page.mouse.wheel(0, -300);
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeLessThan(before);
});

test("admin sections and model tasks use tabs", async ({ page }) => {
  await login(page);
  await page.goto("/admin", { waitUntil: "networkidle" });
  await page.getByRole("tab", { name: "Models", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Model providers" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Ranking" })).toHaveAttribute("aria-selected", "true");
  await page.getByRole("tab", { name: "Embedding" }).click();
  await expect(page.getByRole("heading", { name: "Embedding model" })).toBeVisible();
  const actions = page.locator(".model-form-actions");
  const buttons = actions.getByRole("button");
  await expect(buttons).toHaveCount(3);
  const tops = await buttons.evaluateAll((elements) => elements.map((element) => element.getBoundingClientRect().top));
  expect(Math.max(...tops) - Math.min(...tops)).toBeLessThan(2);
  const save = actions.getByRole("button", { name: "Save embedding model" });
  const reset = actions.getByRole("button", { name: "Reset embedding model" });
  expect(await save.evaluate((element) => getComputedStyle(element).backgroundColor))
    .not.toBe(await reset.evaluate((element) => getComputedStyle(element).backgroundColor));
});
