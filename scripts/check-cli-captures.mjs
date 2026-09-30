import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';

const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const root = process.cwd();
function sourceFiles(directory) {
  return readdirSync(path.join(root, directory), { withFileTypes: true }).flatMap(entry => {
    const name = `${directory}/${entry.name}`;
    if (entry.isDirectory()) return entry.name === 'tests' ? [] : sourceFiles(name);
    return name.endsWith('.rs') ? [name] : [];
  });
}
try {
  const files = ['Cargo.toml', 'Cargo.lock', 'scripts/capture-cli.py', ...sourceFiles('src'), ...readdirSync('resources/licenses').filter(n => n.endsWith('.txt')).map(n => `resources/licenses/${n}`)].sort();
  const source = files.map(name => `${name}\n${readFileSync(name, 'utf8').replaceAll('\r\n', '\n')}\n`).join('');
  const manifest = JSON.parse(readFileSync('docs/assets/captures.json', 'utf8'));
  if (manifest.schema_version !== 1 || manifest.source_digest !== digest(source) || manifest.captures.length !== 2) throw new Error('CLI captures are stale; rebuild and run python scripts/capture-cli.py');
  for (const capture of manifest.captures) {
    if (!/^cli-(help|status)\.txt$/.test(capture.output) || !/^cli-(help|status)\.png$/.test(capture.image)) throw new Error('Invalid capture paths');
    for (const [file, hash] of [[capture.output, capture.output_sha256], [capture.image, capture.image_sha256]]) {
      if (digest(readFileSync(path.join('docs/assets', file))) !== hash) throw new Error(`Capture changed: ${file}`);
    }
  }
  console.log('Current CLI capture source and image/output checksums pass.');
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
