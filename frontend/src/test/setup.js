// Global test setup. Runs once before each test file.
import '@testing-library/jest-dom/vitest';

// jsdom doesn't ship with a real fetch — vitest does (via undici-like
// node fetch on Node 18+), but for unit tests we stub it per-test
// instead of relying on the global. Tests can do:
//   global.fetch = vi.fn().mockResolvedValue({ ok: true, status: 200, ... })

// Tighten console output: surface unhandled errors in tests.
const origError = console.error;
console.error = (...args) => {
  // Skip React's "act()" warning noise from useEffect-heavy tests.
  if (typeof args[0] === 'string' && args[0].includes('not wrapped in act')) return;
  origError(...args);
};
