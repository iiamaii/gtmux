const script = document.querySelector('script[data-base]');
const input = document.getElementById('search'), results = document.getElementById('search-results'), article = document.querySelector('article');
let index;
input.addEventListener('input', async () => {
  const query = input.value.trim().toLowerCase(); results.replaceChildren(); results.hidden = !query; article.hidden = Boolean(query);
  if (!query) return;
  try { index ??= await fetch(`${script.dataset.base}assets/search-${script.dataset.language}.json`).then(r => { if (!r.ok) throw new Error('search unavailable'); return r.json(); }); } catch { results.textContent = script.dataset.language === 'ko' ? '검색을 불러오지 못했습니다. 문서 목록을 이용하세요.' : 'Search could not load. Use the documentation navigation.'; return; }
  if (input.value.trim().toLowerCase() !== query) return;
  for (const page of index.filter(p => (p.title + ' ' + p.text).toLowerCase().includes(query)).slice(0, 12)) {
    const row = document.createElement('div'); row.className = 'result';
    const link = document.createElement('a'); link.href = page.url; link.textContent = page.title;
    const snippet = document.createElement('p'); const at = page.text.toLowerCase().indexOf(query); snippet.textContent = page.text.slice(Math.max(0, at - 60), Math.max(0, at - 60) + 210);
    row.append(link, snippet); results.append(row);
  }
  if (!results.childElementCount) results.textContent = script.dataset.language === 'ko' ? '검색 결과가 없습니다.' : 'No matching pages.';
});
for (const pre of document.querySelectorAll('pre')) {
  const button = document.createElement('button'); button.textContent = script.dataset.language === 'ko' ? '복사' : 'Copy';
  button.addEventListener('click', async () => { try { await navigator.clipboard.writeText(pre.querySelector('code').textContent); button.textContent = script.dataset.language === 'ko' ? '복사됨' : 'Copied'; } catch { button.textContent = script.dataset.language === 'ko' ? '직접 선택해 복사하세요' : 'Select text to copy'; } }); pre.append(button);
}
