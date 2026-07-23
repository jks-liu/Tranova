import { expect, test } from "@playwright/test";

for (const viewport of [
  { name: "desktop", width: 1440, height: 900 },
  { name: "mobile", width: 390, height: 844 },
]) {
  test(`${viewport.name} application shell renders without overflow`, async ({ page }) => {
    const browserErrors: string[] = [];
    page.on("pageerror", (error) => browserErrors.push(error.message));
    await page.setViewportSize(viewport);
    await page.goto("/", { waitUntil: "domcontentloaded" });

    await expect(page.locator("h1")).toBeVisible();
    await expect(page.locator(".app-shell")).toBeVisible();
    await expect(page).toHaveScreenshot(`${viewport.name}.png`, { animations: "disabled", fullPage: false });
    expect(browserErrors).toEqual([]);

    const dimensions = await page.evaluate(() => ({
      viewport: document.documentElement.clientWidth,
      content: document.documentElement.scrollWidth,
      heading: document.querySelector("h1")?.textContent,
      shell: document.querySelector(".app-shell")?.getBoundingClientRect().toJSON(),
    }));
    expect(dimensions.content).toBeLessThanOrEqual(dimensions.viewport);
    expect(dimensions.heading).toBeTruthy();
    expect(dimensions.shell?.width).toBeGreaterThanOrEqual(viewport.width - 1);
  });
}

test("Simplified Chinese interface localizes the primary workflow", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("tranova-language", "zh-CN"));
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.locator("h1")).toHaveText("文本翻译");
  await expect(page.locator(".sidebar-nav")).toContainText("文件翻译");
  await expect(page.locator(".brand-row")).toContainText("AI 翻译工作台");
});
