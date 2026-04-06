import { test, expect } from '@playwright/test';

test.describe('Landing page', () => {
  test.beforeEach(async ({ page }) => {
    // Clear session cookie to ensure unauthenticated state
    await page.context().clearCookies();
    await page.goto('/');
  });

  test('renders title and CTA', async ({ page }) => {
    await expect(page.locator('.landing__title')).toHaveText('Sign in to Metsuke');
    await expect(page.locator('.landing__btn')).toContainText('Sign in with GitHub');
  });

  test('CTA links to auth login', async ({ page }) => {
    const cta = page.locator('.landing__btn');
    await expect(cta).toHaveAttribute('href', '/auth/login');
  });

  test('loads external CSS (no inline styles)', async ({ page }) => {
    // Verify no <style> blocks exist
    const styleBlocks = await page.locator('style').count();
    expect(styleBlocks).toBe(0);

    // Verify external CSS is loaded
    const themesCss = page.locator('link[href="/static/themes.css"]');
    await expect(themesCss).toHaveCount(1);
    const landingCss = page.locator('link[href="/static/landing.css"]');
    await expect(landingCss).toHaveCount(1);
  });

  test('has correct meta tags', async ({ page }) => {
    await expect(page).toHaveTitle('Metsuke — SDLC Process Inspector');
    const desc = page.locator('meta[name="description"]');
    await expect(desc).toHaveAttribute('content', /SDLC/);
    const ogTitle = page.locator('meta[property="og:title"]');
    await expect(ogTitle).toHaveAttribute('content', /Metsuke/);
  });

  test('has accessible structure', async ({ page }) => {
    // Eye motif SVG with embedded logo is present
    const eyeSvg = page.locator('.landing__eye-svg');
    await expect(eyeSvg).toBeVisible();
  });

  test('theme variables are applied', async ({ page }) => {
    // Body should have background from CSS variables
    const bgColor = await page.evaluate(() =>
      getComputedStyle(document.body).backgroundColor
    );
    expect(bgColor).not.toBe('rgba(0, 0, 0, 0)');
  });
});

test.describe('Landing page - mobile', () => {
  test.use({ viewport: { width: 375, height: 812 } });

  test('renders correctly on mobile', async ({ page }) => {
    await page.context().clearCookies();
    await page.goto('/');
    await expect(page.locator('.landing__title')).toBeVisible();
    await expect(page.locator('.landing__btn')).toBeVisible();
    // CTA should have minimum touch target
    const ctaBox = await page.locator('.landing__btn').boundingBox();
    expect(ctaBox!.height).toBeGreaterThanOrEqual(44);
  });
});
