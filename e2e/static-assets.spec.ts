import { test, expect } from '@playwright/test';

const STATIC_FILES = [
  '/static/style.css',
  '/static/themes.css',
  '/static/app.js',
  '/static/theme-init.js',
  '/static/verify-common.js',
  '/static/landing.css',
  '/static/favicon.svg',
  '/static/pages/repos.js',
  '/static/pages/repo-detail.js',
  '/static/pages/verify-pr.js',
  '/static/pages/verify-release.js',
  '/static/pages/audit.js',
  '/static/pages/settings.js',
  '/static/vendor/github-markdown-dark.min.css',
  '/static/vendor/github-markdown-light.min.css',
];

test.describe('Static assets', () => {
  for (const path of STATIC_FILES) {
    test(`${path} returns 200`, async ({ request }) => {
      const resp = await request.get(path);
      expect(resp.status()).toBe(200);
    });
  }

  test('static assets have no-cache header', async ({ request }) => {
    const resp = await request.get('/static/app.js');
    const cc = resp.headers()['cache-control'];
    expect(cc).toContain('no-cache');
  });

  test('health endpoint responds', async ({ request }) => {
    const resp = await request.get('/health');
    expect(resp.status()).toBe(200);
    expect(await resp.text()).toBe('ok');
  });
});

test.describe('CSP headers', () => {
  test('landing page has strict CSP', async ({ request }) => {
    const resp = await request.get('/');
    const csp = resp.headers()['content-security-policy'] || '';
    // No unsafe-inline in script-src
    expect(csp).toMatch(/script-src 'self'(?!.*unsafe-inline)/);
    // No unsafe-inline in style-src
    expect(csp).toMatch(/style-src 'self'(?!.*unsafe-inline)/);
    // No cdn.jsdelivr.net anywhere
    expect(csp).not.toContain('cdn.jsdelivr.net');
  });
});
