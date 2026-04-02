import { test, expect, Page } from '@playwright/test';

/**
 * Detect layout issues by checking computed styles of key elements.
 * Catches: missing gaps, broken flex layouts, zero-height elements,
 * overlapping siblings, and misaligned flex children.
 */

interface LayoutIssue {
  selector: string;
  element: string;
  issue: string;
  detail: string;
}

async function auditLayout(page: Page): Promise<LayoutIssue[]> {
  return page.evaluate(() => {
    const issues: Array<{selector: string; element: string; issue: string; detail: string}> = [];

    function desc(el: Element): string {
      const tag = el.tagName.toLowerCase();
      const cls = el.className ? `.${el.className.toString().split(' ').join('.')}` : '';
      const id = el.id ? `#${el.id}` : '';
      return `${tag}${id}${cls}`.slice(0, 80);
    }

    // 1. Check all flex/grid containers for gap issues
    document.querySelectorAll('*').forEach(el => {
      const cs = getComputedStyle(el);
      if (cs.display !== 'flex' && cs.display !== 'inline-flex' && cs.display !== 'grid') return;
      if (el.children.length < 2) return;

      // Check if gap is declared in CSS but computed as 0
      const gap = cs.gap || cs.rowGap || cs.columnGap;
      // Skip elements that intentionally have no gap
      if (gap === 'normal' || gap === '0px') {
        // Check if this element has a CSS class that suggests it should have a gap
        const classes = el.className.toString();
        const gapClasses = ['toolbar', 'nav-links', 'header-left', 'page-header',
          'inline-row', 'repo-meta', 'pr-item-meta', 'release-item-meta',
          'audit-toolbar', 'summary-bar', 'install-meta', 'dashboard-grid',
          'repo-grid', 'pr-list', 'release-list', 'activity-list',
          'skeleton-list', 'btn-row'];
        const shouldHaveGap = gapClasses.some(c => classes.includes(c));
        if (shouldHaveGap) {
          issues.push({
            selector: desc(el),
            element: el.outerHTML.slice(0, 120),
            issue: 'zero-gap',
            detail: `gap=${gap} but class suggests gap is expected`,
          });
        }
      }
    });

    // 2. Check for zero-height visible elements (collapsed layout)
    document.querySelectorAll('.card, .repo-card, .pr-item, .release-item, .dash-card, .activity-item, .skeleton-item').forEach(el => {
      const rect = el.getBoundingClientRect();
      if (rect.height === 0 && !el.hasAttribute('hidden')) {
        issues.push({
          selector: desc(el),
          element: el.outerHTML.slice(0, 120),
          issue: 'zero-height',
          detail: `visible element has height=0`,
        });
      }
    });

    // 3. Check flex children alignment — siblings should not overlap vertically in column layout
    document.querySelectorAll('.repo-grid, .pr-list, .release-list, .activity-list').forEach(container => {
      const children = Array.from(container.children);
      for (let i = 1; i < children.length; i++) {
        const prev = children[i - 1].getBoundingClientRect();
        const curr = children[i].getBoundingClientRect();
        if (prev.bottom > curr.top + 1) { // 1px tolerance
          issues.push({
            selector: desc(container),
            element: `child[${i-1}] and child[${i}]`,
            issue: 'overlap',
            detail: `prev.bottom=${prev.bottom.toFixed(1)} > curr.top=${curr.top.toFixed(1)}`,
          });
        }
      }
    });

    // 4. Check flex row containers for wrapping overflow
    document.querySelectorAll('.page-header, .toolbar, .audit-toolbar, .pr-detail-header').forEach(el => {
      const rect = el.getBoundingClientRect();
      const parent = el.parentElement;
      if (!parent) return;
      const parentRect = parent.getBoundingClientRect();
      if (rect.right > parentRect.right + 2) { // 2px tolerance
        issues.push({
          selector: desc(el),
          element: el.outerHTML.slice(0, 120),
          issue: 'overflow',
          detail: `element overflows parent by ${(rect.right - parentRect.right).toFixed(1)}px`,
        });
      }
    });

    return issues;
  });
}

test.describe('Layout integrity - landing page', () => {
  test('no layout issues on landing', async ({ page }) => {
    await page.context().clearCookies();
    await page.goto('/');
    await page.waitForSelector('.landing', { timeout: 5000 });
    const issues = await auditLayout(page);
    if (issues.length > 0) {
      console.log('Layout issues found:', JSON.stringify(issues, null, 2));
    }
    expect(issues, `Layout issues: ${JSON.stringify(issues)}`).toHaveLength(0);
  });
});

test.describe('Layout integrity - error page', () => {
  test('no layout issues on error page', async ({ page }) => {
    await page.context().clearCookies();
    await page.goto('/auth/callback?code=fake&state=web:bad');
    await page.waitForSelector('.error-card', { timeout: 5000 });
    const issues = await auditLayout(page);
    if (issues.length > 0) {
      console.log('Layout issues found:', JSON.stringify(issues, null, 2));
    }
    expect(issues, `Layout issues: ${JSON.stringify(issues)}`).toHaveLength(0);
  });
});

// Authenticated pages — require METSUKE_SESSION env var
const hasSession = !!process.env.METSUKE_SESSION;

test.describe('Layout integrity - repos page', () => {
  test.skip(!hasSession, 'Requires METSUKE_SESSION cookie');
  test('no layout issues on repos list', async ({ page }) => {
    await page.context().addCookies([{
      name: 'session', value: process.env.METSUKE_SESSION!,
      domain: new URL(page.url() || 'https://metsuke.fly.dev').hostname,
      path: '/',
    }]);
    await page.goto('/repos');
    await page.waitForSelector('.repo-card', { timeout: 15000 });
    const issues = await auditLayout(page);
    if (issues.length > 0) {
      console.log('Layout issues found:', JSON.stringify(issues, null, 2));
    }
    expect(issues, `Layout issues: ${JSON.stringify(issues)}`).toHaveLength(0);
  });
});

test.describe('Layout integrity - repo detail', () => {
  test.skip(!hasSession, 'Requires METSUKE_SESSION cookie');
  test('no layout issues on dashboard', async ({ page }) => {
    await page.context().addCookies([{
      name: 'session', value: process.env.METSUKE_SESSION!,
      domain: new URL(page.url() || 'https://metsuke.fly.dev').hostname,
      path: '/',
    }]);
    await page.goto('/repos/plenoai/metsuke');
    await page.waitForSelector('.dash-card', { timeout: 15000 });
    await page.waitForTimeout(2000);
    const issues = await auditLayout(page);
    if (issues.length > 0) {
      console.log('Layout issues found:', JSON.stringify(issues, null, 2));
    }
    expect(issues, `Layout issues: ${JSON.stringify(issues)}`).toHaveLength(0);
  });
});

test.describe('Layout integrity - audit page', () => {
  test.skip(!hasSession, 'Requires METSUKE_SESSION cookie');
  test('no layout issues on audit log', async ({ page }) => {
    await page.context().addCookies([{
      name: 'session', value: process.env.METSUKE_SESSION!,
      domain: new URL(page.url() || 'https://metsuke.fly.dev').hostname,
      path: '/',
    }]);
    await page.goto('/audit');
    await page.waitForSelector('.audit-table', { timeout: 15000 });
    const issues = await auditLayout(page);
    if (issues.length > 0) {
      console.log('Layout issues found:', JSON.stringify(issues, null, 2));
    }
    expect(issues, `Layout issues: ${JSON.stringify(issues)}`).toHaveLength(0);
  });
});

test.describe('Layout integrity - settings page', () => {
  test.skip(!hasSession, 'Requires METSUKE_SESSION cookie');
  test('no layout issues on settings', async ({ page }) => {
    await page.context().addCookies([{
      name: 'session', value: process.env.METSUKE_SESSION!,
      domain: new URL(page.url() || 'https://metsuke.fly.dev').hostname,
      path: '/',
    }]);
    await page.goto('/settings');
    await page.waitForSelector('.install-item, .install-hint', { timeout: 15000 });
    const issues = await auditLayout(page);
    if (issues.length > 0) {
      console.log('Layout issues found:', JSON.stringify(issues, null, 2));
    }
    expect(issues, `Layout issues: ${JSON.stringify(issues)}`).toHaveLength(0);
  });
});
