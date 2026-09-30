import { strict as assert } from 'node:assert';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { checkLinks, markdownFiles } from '../scripts/doc-links.mjs';

function fixture(t, files) {
  const root = mkdtempSync(join(tmpdir(), 'modelprepper-docs-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [name, body] of Object.entries(files)) {
    const path = join(root, name);
    mkdirSync(join(path, '..'), { recursive: true });
    writeFileSync(path, body);
  }
  return root;
}

test('checks nested paths, images, encoded spaces, fragments, and extensionless files', (t) => {
  const root = fixture(t, {
    'README.md': '[doc](docs/guide.md#intro) [license](LICENSE) ![image](image.svg) [space](<docs/a b.md>) [encoded](docs/a%20b.md?x=1)',
    'docs/guide.md': '[root](../README.md "Home")',
    'docs/a b.md': '# File',
    'LICENSE': 'MIT',
    'image.svg': '<svg/>',
  });
  assert.deepEqual(checkLinks(root), []);
  assert.equal(markdownFiles(root).length, 3);
});

test('reports broken paths and invalid encoding', (t) => {
  const root = fixture(t, { 'README.md': '[broken](missing.md) [bad](bad%ZZ.md)' });
  const errors = checkLinks(root);
  assert.equal(errors.length, 2);
  assert.match(errors[0], /missing local target: missing.md/);
  assert.match(errors[1], /invalid URL encoding/);
});

test('ignores external links, fragment links, code fences, and generated directories', (t) => {
  const root = fixture(t, {
    'README.md': '[web](https://example.com) [mail](mailto:a@example.com) [anchor](#intro) [web](//example.com)\n```text\n[example](absent.md)\n```',
    'node_modules/broken.md': '[bad](absent.md)',
    '.git/broken.md': '[bad](absent.md)',
    'target/broken.md': '[bad](absent.md)',
    'coverage/broken.md': '[bad](absent.md)',
    'notes.txt': '[bad](absent.md)',
  });
  assert.deepEqual(checkLinks(root), []);
  assert.equal(markdownFiles(root).length, 1);
});
