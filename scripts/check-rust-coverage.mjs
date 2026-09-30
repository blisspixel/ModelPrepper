import { readFileSync } from 'node:fs';

const report = JSON.parse(readFileSync(process.argv[2] ?? 'target/coverage.json', 'utf8'));
const totals = report.data[0].totals;
let failed = false;
for (const metric of ['lines', 'branches']) {
  const { count, percent } = totals[metric];
  if (!Number.isFinite(percent) || count <= 0 || percent < 80) {
    console.error(`${metric}: missing measurements or coverage below 80%`);
    failed = true;
  } else {
    console.log(`Rust ${metric}: ${percent.toFixed(2)}%`);
  }
}
process.exitCode = failed ? 1 : 0;
