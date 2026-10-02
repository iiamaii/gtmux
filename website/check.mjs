import { readdir, readFile, access } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
const root = new URL('./', import.meta.url);
const en = (await readdir(new URL('content/en/', root))).filter(p => p.endsWith('.md')).sort();
const ko = (await readdir(new URL('content/ko/', root))).filter(p => p.endsWith('.md')).sort();
if (JSON.stringify(en) !== JSON.stringify(ko)) throw new Error('English and Korean pages must have matching filenames.');
for (const lang of ['en', 'ko']) for (const file of en) {
  const text = await readFile(new URL(`content/${lang}/${file}`, root), 'utf8');
  if (!/^# .+/m.test(text)) throw new Error(`${lang}/${file} needs a title.`);
  for (const [, link] of text.matchAll(/\]\(([^) ]+)\)/g)) {
    if (/^(https?:|#)/.test(link)) continue;
    const target = link.split('#')[0].replace(/\.html$/, '.md');
    await access(new URL(`content/${lang}/${target}`, root));
  }
}
const base = process.env.DOCS_BASE_REF;
if (base) {
  const changed = execFileSync('git', ['diff', '--name-only', `${base}...HEAD`], { encoding: 'utf8' }).trim().split('\n');
  const english = changed.filter(p => /^website\/content\/en\/.+\.md$/.test(p));
  if (changed.some(p => /^codebase\/(backend|frontend|launcher)\//.test(p)) && !english.length)
    throw new Error('Code changes require an English documentation update and its Korean translation.');
  for (const path of english) if (!changed.includes(path.replace('/en/', '/ko/')))
    throw new Error(`Update the Korean counterpart of ${path}. CI enforces paired updates, not translation quality.`);
}
console.log('Documentation language pairs, relative links and change coverage passed.');
