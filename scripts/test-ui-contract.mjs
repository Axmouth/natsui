import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Check literal API calls against the server route declarations. Runtime tests
// cover authentication, methods and parameterized resource operations separately.
const root = resolve(
  process.argv[2] || fileURLToPath(new URL('..', import.meta.url)),
);
const declarations = readdirSync(resolve(root, 'src'))
  .filter((name) => name.endsWith('.rs'))
  .flatMap((name) =>
    [
      ...readFileSync(resolve(root, 'src', name), 'utf8').matchAll(
        /\.route\(\s*"(\/api\/[^"\s]+)"/g,
      ),
    ].map((match) => match[1]),
  );
const patterns = declarations.map(
  (route) =>
    new RegExp(
      '^' +
        route
          .split('/')
          .map((part) =>
            part.startsWith('{')
              ? '[^/]+'
              : part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'),
          )
          .join('/') +
        '$',
    ),
);
let calls = 0;
for (const name of readdirSync(resolve(root, 'web')).filter(
  (name) => name.endsWith('.js') && name !== 'demo.js',
)) {
  const source = readFileSync(resolve(root, 'web', name), 'utf8');
  for (const [, , url] of source.matchAll(
    /\b(?:api|send|fetch)\(\s*(['"])(\/api\/[^'"]*)\1\s*[,)]/g,
  )) {
    calls++;
    assert.ok(
      patterns.some((pattern) => pattern.test(url.split('?')[0])),
      `${name}: API call has no server route: ${url}`,
    );
  }
}
assert.ok(calls >= 10, 'Expected the production UI API calls to be inspected');
console.log(`UI route contract passed for ${calls} literal calls.`);
