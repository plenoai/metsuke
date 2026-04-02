import { test, expect } from '@playwright/test';

test.describe('Error page', () => {
  test.beforeEach(async ({ page }) => {
    await page.context().clearCookies();
    // Trigger CSRF error by passing invalid state
    await page.goto('/auth/callback?code=fake&state=web:bad');
  });

  test('displays error card with correct content', async ({ page }) => {
    await expect(page.locator('.error-page__mark')).toHaveText('障');
    await expect(page.locator('.error-page__title')).toContainText('認証エラー');
    await expect(page.locator('.error-page__msg')).toContainText('CSRF');
    await expect(page.locator('.error-page__back')).toHaveAttribute('href', '/');
  });

  test('error card is centered', async ({ page }) => {
    const body = page.locator('body');
    await expect(body).toHaveClass(/error-page/);
    const display = await page.evaluate(() =>
      getComputedStyle(document.body).display
    );
    expect(display).toBe('flex');
  });

  test('loads CSS externally (no inline styles)', async ({ page }) => {
    const styleBlocks = await page.locator('style').count();
    expect(styleBlocks).toBe(0);
    const styleCss = page.locator('link[href="/static/style.css"]');
    await expect(styleCss).toHaveCount(1);
  });

  test('theme is applied', async ({ page }) => {
    const themeInit = page.locator('script[src="/static/theme-init.js"]');
    await expect(themeInit).toHaveCount(1);
  });

  test('back link navigates to landing', async ({ page }) => {
    await page.click('.error-page__back');
    await expect(page).toHaveURL(/\/$/);
  });
});
