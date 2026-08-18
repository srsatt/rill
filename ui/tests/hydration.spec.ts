import { expect, test } from "@playwright/test";

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
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
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
  await expect(page.getByLabel("Filter stories on this page")).toBeVisible();

  expect(await page.evaluate(() => (window as ProbeWindow).__rillRootReplaced)).toBe(false);
  expect(consoleErrors).toEqual([]);
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
});

test("reader pairing and feed work with JavaScript disabled", async ({ browser, page }) => {
  await login(page);
  await page.goto("/settings/readers");
  await page.getByLabel("Device label").fill("Playwright reader");
  await page.getByRole("button", { name: "Create one-time code" }).click();
  const pairingText = await page.locator(".pairing-code strong").innerText();

  const context = await browser.newContext({ javaScriptEnabled: false });
  const reader = await context.newPage();
  await reader.goto("/reader/pair");
  await reader.getByLabel("Pairing code").fill(pairingText);
  await reader.getByRole("button", { name: "Pair reader" }).click();

  await expect(reader).toHaveURL(/\/reader$/);
  await expect(reader.getByRole("heading", { name: "Home" })).toBeVisible();
  await expect(reader.getByText("No stories yet.")).toBeVisible();
  await expect(reader.locator("script")).toHaveCount(0);
  await context.close();
});

test("source, story feedback, and stream management use the real API", async ({ page }) => {
  await login(page);
  await page.goto("/sources");
  const rssForm = page.locator("#rss-create");
  await rssForm.getByLabel("Name").fill("Fixture RSS");
  await rssForm.getByLabel("Feed URL").fill("http://127.0.0.1:3011/rss.xml");
  await rssForm.getByRole("button", { name: "Add feed" }).click();
  const source = page.locator("article").filter({ has: page.getByRole("heading", { name: "Fixture RSS" }) });
  await expect(source).toBeVisible();
  await source.getByRole("button", { name: "Fetch now" }).click();

  await expect.poll(async () => {
    await page.goto("/stream/home");
    return page.getByRole("link", { name: "Germany changes public software procurement" }).count();
  }, { timeout: 20_000 }).toBe(1);
  const storyCard = page.locator(".story-card").first();
  const cardBeforeHover = await storyCard.boundingBox();
  expect(cardBeforeHover).not.toBeNull();
  await storyCard.hover();
  await page.waitForTimeout(250);
  const cardAfterHover = await storyCard.boundingBox();
  expect(cardAfterHover?.x).toBe(cardBeforeHover?.x);
  expect(cardAfterHover?.y).toBe(cardBeforeHover?.y);
  await page.getByRole("link", { name: "Germany changes public software procurement" }).click();
  for (const feedback of ["Like", "Dislike", "Favorite"]) {
    const response = page.waitForResponse((candidate) =>
      candidate.url().includes("/feedback") && candidate.request().method() === "POST");
    await page.getByRole("button", { name: feedback, exact: true }).click();
    expect((await response).ok()).toBe(true);
  }

  await page.goto("/sources");
  const streamForm = page.locator("#stream-create");
  await streamForm.getByLabel("Stream name").fill("Fixture stream");
  await streamForm.getByLabel("What belongs here?").fill("Systems implementation details");
  await streamForm.locator("summary", { hasText: "Advanced tag filters" }).click();
  await streamForm.getByLabel("Include topics").fill("rust, wasm");
  await streamForm.getByLabel("Exclude topics").fill("sponsored");
  await streamForm.getByRole("button", { name: "Create custom stream" }).click();
  await expect(page.getByRole("link", { name: "Fixture stream" })).toBeVisible();
  await expect(page.getByLabel("Included topics for Fixture stream")).toHaveValue("rust, wasm");
  await expect(page.getByLabel("Excluded topics for Fixture stream")).toHaveValue("sponsored");
  await page.getByLabel("Name for Fixture stream").fill("Fixture engineering");
  await page.getByRole("button", { name: "Save" }).last().click();
  await expect(page.getByRole("link", { name: "Fixture engineering" })).toBeVisible();
  const stream = page.locator("article").filter({ has: page.getByRole("link", { name: "Fixture engineering" }) });
  await stream.getByRole("button", { name: "Delete" }).click();
  await expect(page.getByRole("link", { name: "Fixture engineering" })).toHaveCount(0);
});
