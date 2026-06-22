// E2E: theme system — dark applies to data-theme + native controls, and
// light restores bright surfaces. Guards the color-scheme / CSS-var overrides.
import { test, expect } from './fixtures.js';
import { ensureLoggedIn, gotoView } from './helpers.js';

const surfaceSum = (page) => page.evaluate(() => {
  const rgb = getComputedStyle(document.querySelector('.rampart')).backgroundColor;
  return (rgb.match(/\d+/g) || []).slice(0, 3).map(Number).reduce((a, b) => a + b, 0);
});

test('dark + light themes apply to surfaces and native controls', async ({ page }) => {
  await ensureLoggedIn(page);

  // Dark.
  await page.evaluate(() => localStorage.setItem('rampart_theme', 'dark'));
  await gotoView(page, '#/', '.activity-row');
  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.theme)).toBe('dark');
  const scheme = await page.evaluate(() => getComputedStyle(document.documentElement).colorScheme);
  expect(scheme).toContain('dark');
  // Poll the computed surface colour — WebKit can lag a frame applying the
  // CSS-var override after the data-theme flips, which flaked a plain read.
  await expect.poll(() => surfaceSum(page)).toBeLessThan(120);  // ~#0c0a09 → ~21

  // Light.
  await page.evaluate(() => localStorage.setItem('rampart_theme', 'light'));
  await gotoView(page, '#/', '.activity-row');
  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.theme)).toBe('light');
  await expect.poll(() => surfaceSum(page)).toBeGreaterThan(600);  // ~#fafaf9 → ~747
});
