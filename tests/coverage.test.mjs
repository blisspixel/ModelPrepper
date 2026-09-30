import { strict as assert } from 'node:assert';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

function gate(t, lines, branches) {
  const dir = mkdtempSync(join(tmpdir(), 'modelprepper-coverage-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const path = join(dir, 'coverage.json');
  writeFileSync(path, JSON.stringify({ data: [{ totals: { lines, branches } }] }));
  return spawnSync(process.execPath, [resolve('scripts/check-rust-coverage.mjs'), path], { encoding: 'utf8' });
}

test('coverage gate accepts measured coverage at exactly 80%', (t) => {
  const result = gate(t, { count: 10, percent: 80 }, { count: 10, percent: 80 });
  assert.equal(result.status, 0);
  assert.match(result.stdout, /Rust branches: 80.00%/);
});

test('coverage gate independently rejects low line and branch coverage', (t) => {
  for (const totals of [[79.9, 100], [100, 79.9]]) {
    const result = gate(t, { count: 10, percent: totals[0] }, { count: 10, percent: totals[1] });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /coverage below 80%/);
  }
});

test('coverage gate rejects unmeasured branches and invalid percentages', (t) => {
  for (const branches of [{ count: 0, percent: 100 }, { count: 1, percent: null }]) {
    assert.equal(gate(t, { count: 10, percent: 100 }, branches).status, 1);
  }
});
