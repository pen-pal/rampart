import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

// Regression guard for a real production crash: several views referenced the
// `reloadKey` state in a `useApi(..., [reloadKey])` deps array placed ABOVE the
// `const [reloadKey, setReloadKey] = useState(0)` declaration. `const` bindings
// are in the temporal dead zone until declared, so at render every affected
// lazy-loaded view threw `ReferenceError: Cannot access 'reloadKey' before
// initialization` and crashed to a blank page. The bundler hid it (valid
// syntax); only runtime caught it. This test fails if any view reintroduces a
// use-before-declaration of `reloadKey`.

const viewsDir = join(dirname(fileURLToPath(import.meta.url)), '..', 'views');

describe('reloadKey is declared before use in every view', () => {
  const files = readdirSync(viewsDir).filter((f) => f.endsWith('.jsx'));

  for (const file of files) {
    it(file, () => {
      const lines = readFileSync(join(viewsDir, file), 'utf8').split('\n');
      const declIdx = lines.findIndex((l) => /const \[reloadKey, setReloadKey\] = useState\(/.test(l));
      if (declIdx === -1) return; // view doesn't use the reloadKey pattern
      const firstUseIdx = lines.findIndex((l, i) => i !== declIdx && /reloadKey/.test(l));
      expect(
        firstUseIdx === -1 || firstUseIdx > declIdx,
        `${file}: reloadKey used on line ${firstUseIdx + 1} before its declaration on line ${declIdx + 1} (temporal-dead-zone crash)`,
      ).toBe(true);
    });
  }
});
