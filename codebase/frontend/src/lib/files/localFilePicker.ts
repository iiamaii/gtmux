export interface LocalFilePickOptions {
  accept?: string;
}

/**
 * Reentrant guard — 이미 native file picker 가 열려있으면 새 호출 즉시
 * `null` resolve. 이유: 사용자가 PDF 등 무거운 파일 upload 중에 또 클릭
 * 했을 때 native picker 가 중복 열리는 회귀 (2026-05-22 사용자 보고) 차단.
 * Tool 의 자동 복귀 (caller 책임) 와 *별 layer* — locked tool + 동시 caller
 * (Canvas / DocumentNode / ImageNode / ItemInfoView) 모두 자연 보호.
 *
 * 정책 = silent block: pending picker 의 시각 단서 (native picker 자체) 가
 * 이미 사용자 입장에서 충분. toast 알람 없음 — noise 회피.
 */
let pendingPicker = false;

export function pickLocalFile(options: LocalFilePickOptions = {}): Promise<File | null> {
  if (pendingPicker) return Promise.resolve(null);
  pendingPicker = true;
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
      pendingPicker = false;
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
