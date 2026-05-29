const TEXT_LABEL_CHAR_CAP = 4000;

export function deriveLabel(text: string): string {
  const firstLine = text.split('\n', 1)[0] ?? '';
  return firstLine.trim().slice(0, TEXT_LABEL_CHAR_CAP);
}

export function effectiveLabelAuto(labelAuto: boolean | undefined, label: string | undefined): boolean {
  return labelAuto ?? ((label ?? '') === '');
}

export function shouldDeriveLabel(
  labelAuto: boolean | undefined,
  label: string | undefined,
  nextText: string,
): boolean {
  return effectiveLabelAuto(labelAuto, label) && nextText.length > 0;
}
