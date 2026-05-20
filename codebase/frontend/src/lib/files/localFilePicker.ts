export interface LocalFilePickOptions {
  accept?: string;
}

export function pickLocalFile(options: LocalFilePickOptions = {}): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = options.accept ?? '';
    input.style.position = 'fixed';
    input.style.left = '-9999px';
    input.style.top = '-9999px';
    input.style.opacity = '0';

    let settled = false;
    const cleanup = () => {
      input.remove();
      window.removeEventListener('focus', onFocus);
    };
    const settle = (file: File | null) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(file);
    };
    const onFocus = () => {
      window.setTimeout(() => {
        if (input.files === null || input.files.length === 0) settle(null);
      }, 1000);
    };

    input.onchange = () => settle(input.files?.[0] ?? null);
    window.addEventListener('focus', onFocus);
    document.body.append(input);
    input.click();
  });
}

export function localFileDisplayPath(file: File): string {
  const maybeRelative = (file as File & { webkitRelativePath?: string }).webkitRelativePath;
  return maybeRelative && maybeRelative.length > 0 ? maybeRelative : file.name;
}
