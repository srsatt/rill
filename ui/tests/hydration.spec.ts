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
  await expect(page).toHaveURL(/\/stream\/all$/);
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

  await page.goto("/stream/all", { waitUntil: "networkidle" });
  await expect(page.getByRole("heading", { name: "All" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Give Rill one good source" })).toBeVisible();
  const toolbar = page.locator("[data-feed-toolbar-enhancement][data-enhanced=true]");
  await expect(toolbar).toBeVisible();
  const filters = toolbar.locator("summary[aria-label='Filters']");
  await expect(filters).toBeVisible();
  await expect(filters).toContainText("Filters");
  await expect(page.getByRole("tab", { name: "All" })).toBeHidden();
  await expect(page.getByLabel("Filter stories")).toBeHidden();
  const titleBox = await page.getByRole("heading", { name: "All" }).boundingBox();
  const filtersBox = await filters.boundingBox();
  const titleFilterGap = (filtersBox?.x ?? 0) - (titleBox?.x ?? 0) - (titleBox?.width ?? 0);
  expect(titleFilterGap).toBeGreaterThanOrEqual(0);
  expect(titleFilterGap).toBeLessThanOrEqual(24);
  const storyListTop = (await page.locator("[data-story-list]").boundingBox())?.y;
  await filters.click();
  await expect(page.getByRole("tab", { name: "All" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByLabel("Filter stories")).toBeVisible();
  expect((await page.locator("[data-story-list]").boundingBox())?.y).toBe(storyListTop);
  const tagAlignment = await page.evaluate(() => {
    const tags = document.createElement("div");
    tags.className = "story-tags";
    tags.innerHTML = '<a class="topic-link">layout</a>';
    document.querySelector("[data-story-list]")?.append(tags);
    const text = tags.querySelector("a")?.firstChild;
    if (!text) return Number.POSITIVE_INFINITY;
    const range = document.createRange();
    range.selectNode(text);
    const difference = Math.abs(range.getBoundingClientRect().x - tags.getBoundingClientRect().x);
    tags.remove();
    return difference;
  });
  expect(tagAlignment).toBeLessThan(1);
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
  for (const route of ["/search", "/favorites", "/history", "/sources", "/settings/readers", "/admin", "/reader"]) {
    await page.goto(route, { waitUntil: "networkidle" });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), route).toBe(true);
  }
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
  const menu = page.getByRole("menu");
  for (const destination of ["Sources", "Settings", "Administration"]) await expect(menu.getByRole("menuitem", { name: destination, exact: true })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Reader mode", exact: true })).toHaveCount(0);
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
  await expect(reader.getByRole("link", { name: "All" })).toHaveAttribute("aria-current", "page");
  await expect(reader.getByText("No stories yet.")).toBeVisible();
  await expect(reader.locator("script")).toHaveCount(0);
  await context.close();
});

test("source, story feedback, and stream management use the real API", async ({ page }) => {
  await login(page);

  expect((await page.request.delete(`${fixtureUrl}/action/requests`)).ok()).toBe(true);
  await page.goto("/settings/readers");
  await expect(page.locator("#user-preferences")).toHaveCSS("row-gap", "20px");
  await expect(page.locator("#user-preferences .settings-field-group").first()).toHaveCSS("row-gap", "8px");
  for (const tab of ["Streams", "Actions", "Devices"]) {
    await page.getByRole("tab", { name: tab, exact: true }).click();
    await expect(page.locator(".settings-panel:not([hidden]) .settings-collection")).toHaveCSS("border-top-width", "0px");
  }
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
  await expect(rssForm).toBeHidden();
  await page.locator(".source-method").filter({ hasText: "RSS and Atom" }).locator("summary").click();
  await rssForm.getByLabel("Name").fill("Fixture RSS");
  await rssForm.getByLabel("Feed URL").fill(`${fixtureUrl}/rss.xml`);
  await rssForm.getByRole("button", { name: "Add feed" }).click();
  const source = page.locator(".source-row").filter({ hasText: "Fixture RSS" });
  await expect(source).toBeVisible();
  await source.click();
  const sourceDetail = page.locator("#source-manager-detail");
  await expect(page.locator(".configured-sources .settings-browser")).toHaveCSS("min-height", "0px");
  await expect(sourceDetail.getByRole("button", { name: "Remove" })).toHaveClass(/source-remove-action/);
  expect(await page.locator(".source-method").first().locator("summary").evaluate((element) => getComputedStyle(element, "::after").content)).not.toBe("none");
  await sourceDetail.getByRole("button", { name: "Fetch now" }).click();

  await expect.poll(async () => {
    await page.goto("/stream/home");
    return page.getByRole("link", { name: "Germany changes public software procurement" }).count();
  }, { timeout: 20_000 }).toBe(1);
  const storyTitle = page.getByRole("link", { name: "Germany changes public software procurement" });
  const storyCard = page.locator(".story-row").filter({ has: storyTitle });
  await expect(storyCard.locator(".story-source-link")).toHaveAttribute("href", `${fixtureUrl}/article/direct`);
  const cardBeforeHover = await storyCard.boundingBox();
  expect(cardBeforeHover).not.toBeNull();
  await storyCard.hover();
  await page.waitForTimeout(250);
  const cardAfterHover = await storyCard.boundingBox();
  expect(cardAfterHover?.x).toBe(cardBeforeHover?.x);
  expect(cardAfterHover?.y).toBe(cardBeforeHover?.y);
  await storyTitle.click();
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
  await expect(rssForm).toBeHidden();
  await page.locator(".source-method").filter({ hasText: "RSS and Atom" }).locator("summary").click();
  await rssForm.getByLabel("Name").fill("Visual audit RSS");
  await rssForm.getByLabel("Feed URL").fill(`${fixtureUrl}/visual-audit-rss.xml`);
  await rssForm.getByRole("button", { name: "Add feed" }).click();
  const source = page.locator(".source-row").filter({ hasText: "Visual audit RSS" });
  await source.click();
  await page.locator("#source-manager-detail").getByRole("button", { name: "Fetch now" }).click();
  await rssForm.getByLabel("Name").fill("Idle source");
  await rssForm.getByLabel("Feed URL").fill(`${fixtureUrl}/idle.xml`);
  await rssForm.getByRole("button", { name: "Add feed" }).click();

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
  await page.locator("[data-feed-toolbar-enhancement] summary[aria-label='Filters']").click();
  const sourceFilter = page.getByLabel("Source");
  await expect(sourceFilter.getByRole("option", { name: "All sources" })).toHaveCount(1);
  await expect(sourceFilter.getByRole("option", { name: "Idle source" })).toHaveCount(1);
  await expect(sourceFilter.getByRole("option", { name: "Visual audit RSS" })).toHaveCount(1);
  await sourceFilter.selectOption({ label: "Idle source" });
  await expect(card).toBeHidden();
  await sourceFilter.selectOption({ label: "All sources" });
  await expect(card).toBeVisible();
  await page.locator(".compact-switch").click();
  await page.locator("[data-feed-toolbar-enhancement] summary[aria-label='Filters']").click();
  await card.hover();
  await expect(actions).toHaveCSS("opacity", "1");
  await card.locator("h2 a").evaluate((link) => { link.textContent = "A deliberately long story title that verifies wrapping across narrow cards without clipping controls, metadata, summaries, or source names"; });
  const longTitleBox = await card.locator("h2").boundingBox();
  const longActionsBox = await actions.boundingBox();
  expect((longTitleBox?.x ?? 0) + (longTitleBox?.width ?? 0)).toBeLessThanOrEqual(longActionsBox?.x ?? 0);
  const cardBox = await card.boundingBox();
  const actionBox = await card.getByRole("button", { name: "Favorite" }).boundingBox();
  expect(cardBox).not.toBeNull();
  expect(actionBox).not.toBeNull();
  expect((actionBox?.x ?? 0) + (actionBox?.width ?? 0)).toBeLessThanOrEqual((cardBox?.x ?? 0) + (cardBox?.width ?? 0));
  expect((actionBox?.y ?? 0) + (actionBox?.height ?? 0)).toBeLessThanOrEqual((cardBox?.y ?? 0) + (cardBox?.height ?? 0));
  await expect(card.getByText("1 source in this story")).toHaveCount(0);

  await page.goto("/reader/stream/all");
  for (const name of ["Like", "Dislike", "Favorite"]) {
    const action = page.getByRole("button", { name, exact: true }).first();
    await expect(action).toHaveCSS("border-top-width", "0px");
    const box = await action.boundingBox();
    expect(box?.width).toBeGreaterThanOrEqual(44);
    expect(box?.height).toBeGreaterThanOrEqual(44);
  }
  const telegramAlignment = await page.evaluate(() => {
    const meta = document.createElement("p");
    meta.className = "reader-story-meta";
    meta.innerHTML = '<a class="reader-telegram-link"><svg class="rill-icon" width="17" height="17"></svg>@genau</a> · 1 min';
    document.querySelector(".reader")?.append(meta);
    const channel = meta.querySelector("a")?.lastChild;
    const time = meta.lastChild;
    if (!channel || !time) return Number.POSITIVE_INFINITY;
    const center = (node: Node) => {
      const range = document.createRange();
      range.selectNode(node);
      const box = range.getBoundingClientRect();
      return box.y + box.height / 2;
    };
    return Math.abs(center(channel) - center(time));
  });
  expect(telegramAlignment).toBeLessThan(2);
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
