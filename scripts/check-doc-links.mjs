import { checkLinks } from './doc-links.mjs';

const errors = checkLinks(process.cwd());
if (errors.length) {
  console.error(errors.join('\n'));
  process.exitCode = 1;
} else {
  console.log('Local Markdown file links pass. External URLs and anchors are not checked.');
}
