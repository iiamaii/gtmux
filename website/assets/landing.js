const preview = document.querySelector('#workspace-preview img');
for (const button of document.querySelectorAll('[data-theme]')) {
  button.addEventListener('click', () => {
    preview.src = preview.src.replace(/workspace-(dark|light)\.png$/, `workspace-${button.dataset.theme}.png`);
    for (const peer of document.querySelectorAll('[data-theme]')) peer.setAttribute('aria-pressed', String(peer === button));
  });
}
