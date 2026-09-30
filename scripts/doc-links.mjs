import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';

export function markdownFiles(root) {
  const files = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (['.git', 'node_modules', 'target', 'coverage'].includes(entry.name)) continue;
    const path = join(root, entry.name);
    if (entry.isDirectory()) files.push(...markdownFiles(path));
    else if (entry.isFile() && entry.name.endsWith('.md')) files.push(path);
  }
  return files;
}

export function checkLinks(root) {
  const errors = [];
  for (const file of markdownFiles(root)) {
    const body = readFileSync(file, 'utf8').replace(/```[^\n]*\n[\s\S]*?```/g, '');
    const links = body.matchAll(/!?\[[^\]]*\]\((<[^>]+>|[^\s)]+)(?:\s+"[^"]*")?\)/g);
    for (const match of links) {
      const target = match[1].replace(/^<|>$/g, '');
      if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('#') || target.startsWith('//')) continue;
      let path;
      try {
        path = decodeURIComponent(target.split(/[?#]/)[0]);
      } catch {
        errors.push(`${relative(root, file)}: invalid URL encoding: ${target}`);
        continue;
      }
      if (!existsSync(resolve(dirname(file), path))) {
        errors.push(`${relative(root, file)}: missing local target: ${target}`);
      }
    }
  }
  return errors;
}
