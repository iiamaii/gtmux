import { marked } from 'marked';
import { home } from './home.mjs';
import { readdir, readFile, mkdir, writeFile, cp, rm } from 'node:fs/promises';
import { resolve } from 'node:path';
const base = process.env.SITE_BASE ?? '/';
if (!/^\/(?:[a-zA-Z0-9._-]+\/)*$/.test(base)) throw new Error('SITE_BASE must be an absolute directory URL.');
const escape = text => text.replace(/[&<>"']/g, c => ({ '&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;' }[c]));
const root = new URL('./', import.meta.url), dist = new URL('./dist/', root);
await rm(dist, { recursive: true, force: true }); await mkdir(dist, { recursive: true });
await cp(new URL('./assets/', root), new URL('./assets/', dist), { recursive: true });
const indexes = {};
for (const lang of ['en', 'ko']) {
  const files = (await readdir(new URL(`content/${lang}/`, root))).filter(file => file.endsWith('.md')).sort();
  indexes[lang] = [];
  const pages = await Promise.all(files.map(async file => {
    const text = await readFile(new URL(`content/${lang}/${file}`, root), 'utf8');
    return { slug: file.slice(0, -3), text, title: text.match(/^# (.+)$/m)?.[1] ?? file };
  }));
  const localSample = await readFile(new URL('../codebase/config.local.sample.toml', root), 'utf8');
  const cloudSample = await readFile(new URL('../codebase/config.cloud.sample.toml', root), 'utf8');
  const referenceTitle = lang === 'en' ? 'Configuration reference' : '설정 참조';
  pages.push({ slug: 'configuration-reference', title: referenceTitle,
    text: `# ${referenceTitle}\n\n${lang === 'en' ? 'Generated from the checked-in configuration samples on every build.' : '매 빌드마다 저장소 설정 샘플에서 자동 생성합니다.'}\n\n## Local\n\n\`\`\`toml\n${localSample}\n\`\`\`\n\n## Cloud\n\n\`\`\`toml\n${cloudSample}\n\`\`\`` });
  for (const page of pages) {
    const headings = [];
    const renderer = new marked.Renderer();
    renderer.heading = ({ text, depth, tokens }) => {
      const id = 'section-' + headings.length; headings.push({ id, text: text.replace(/<[^>]*>/g, ''), depth });
      return `<h${depth} id="${id}">${renderer.parser.parseInline(tokens)}</h${depth}>`;
    };
    const html = marked.parse(page.text, { renderer }).replace(/href="(?!https?:|#|\/)([^"]+)"/g, (_match, href) => `href="${base}${lang}/${href}"`);
    const nav = pages.map(p => `<a href="${base}${lang}/${p.slug}.html"${page.slug === p.slug ? ' aria-current="page"' : ''}>${escape(p.title)}</a>`).join('');
    const other = lang === 'en' ? 'ko' : 'en';
    const shell = `<!doctype html><html lang="${lang}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="color-scheme" content="light dark"><title>${escape(page.title)} · gtmux</title><link rel="stylesheet" href="${base}assets/site.css"></head><body>
<header><a class="brand" href="${base}${lang}/home.html">gtmux <span>documentation</span></a><label class="search-label"><span class="sr-only">${lang === 'ko' ? '문서 검색' : 'Search documentation'}</span><input id="search" type="search" placeholder="${lang === 'ko' ? '문서 검색' : 'Search documentation'}" autocomplete="off"></label><a href="${base}${other}/${page.slug}.html" lang="${other}">${other === 'ko' ? '한국어' : 'English'}</a></header>
<div class="layout"><nav aria-label="${lang === 'ko' ? '문서 목록' : 'Documentation'}">${nav}</nav><main id="main"><div id="search-results" hidden></div><article>${html}</article><footer>${lang === 'ko' ? '이 문서는 해당 소스 버전의 기능을 설명합니다.' : 'This documentation describes the features in this source version.'}</footer></main><aside aria-label="On this page">${headings.filter(h => h.depth === 2).map(h => `<a href="#${h.id}">${escape(h.text)}</a>`).join('')}</aside></div><script src="${base}assets/site.js" type="module" data-base="${base}" data-language="${lang}"></script></body></html>`;
    await mkdir(new URL(`${lang}/`, dist), { recursive: true }); await writeFile(new URL(`${lang}/${page.slug}.html`, dist), shell);

    indexes[lang].push({ title: page.title, url: `${base}${lang}/${page.slug}.html`, text: page.text.replace(/[#*`]/g, '').slice(0, 50000) });
  }
  await writeFile(new URL(`assets/search-${lang}.json`, dist), JSON.stringify(indexes[lang]));
}
for (const lang of ['en', 'ko']) await writeFile(new URL(`${lang}/home.html`, dist), home(lang, base));
await writeFile(new URL('index.html', dist), home('en', base));
await writeFile(new URL('.nojekyll', dist), '');
console.log(`Built ${Object.values(indexes).reduce((n, p) => n + p.length, 0)} documentation pages at ${base}`);
