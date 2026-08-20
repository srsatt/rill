import { mkdirSync } from "node:fs";
import { expect, test } from "@playwright/test";

const pages = [
  ["feed", "/stream/all"],
  ["search", "/search"],
  ["favorites", "/favorites"],
  ["history", "/history"],
  ["sources", "/sources"],
  ["reader", "/reader"],
  ["settings", "/settings/readers"],
  ["admin", "/admin"],
] as const;

test("capture main pages", async ({ page }) => {
  await page.goto("/login");
  await page.getByLabel("Username or email").fill("admin");
  await page.getByLabel("Password").fill("rill-e2e-password");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/stream\/all$/);

  mkdirSync("screenshots", { recursive: true });
  for (const [name, path] of pages) {
    await page.goto(path, { waitUntil: "networkidle" });
    await page.screenshot({ path: `screenshots/${name}.png`, fullPage: true });
  }
});
