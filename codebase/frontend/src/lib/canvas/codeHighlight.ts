export const CODE_HIGHLIGHT_MAX_BYTES = 512 * 1024;

export type CodeHighlightTheme = 'light' | 'dark';

export interface HlToken {
  content: string;
  color: string;
}

export type HlLine = HlToken[];

interface ShikiToken {
  content: string;
  color?: string;
}

interface ShikiHighlighter {
  loadLanguage(lang: string): Promise<void>;
  codeToTokens(
    raw: string,
    options: { lang: string; theme: string },
  ): { tokens: ShikiToken[][] };
}

const THEME_BY_APP_THEME: Record<CodeHighlightTheme, string> = {
  light: 'github-light',
  dark: 'github-dark',
};

let highlighterPromise: Promise<ShikiHighlighter> | null = null;

export async function highlightLines(
  raw: string,
  lang: string,
  theme: CodeHighlightTheme = 'dark',
): Promise<HlLine[] | null> {
  if (byteLength(raw) > CODE_HIGHLIGHT_MAX_BYTES) return null;
  const cleanLang = normalizeLang(lang);
  if (cleanLang === null) return null;

  try {
    const highlighter = await getHighlighter();
    await highlighter.loadLanguage(cleanLang);
    const result = highlighter.codeToTokens(raw, {
      lang: cleanLang,
      theme: THEME_BY_APP_THEME[theme],
    });
    return result.tokens.map((line) =>
      line.map((token) => ({
        content: token.content,
        color: token.color ?? 'currentColor',
      })),
    );
  } catch {
    return null;
  }
}

function getHighlighter(): Promise<ShikiHighlighter> {
  highlighterPromise ??= import('shiki').then(async ({ createHighlighter }) => (
    await createHighlighter({
      themes: Object.values(THEME_BY_APP_THEME),
      langs: [],
    })
  ) as ShikiHighlighter);
  return highlighterPromise;
}

function normalizeLang(lang: string): string | null {
  const clean = lang.trim().toLowerCase();
  if (clean.length === 0 || clean === 'text' || clean === 'plain' || clean === 'plaintext') {
    return null;
  }
  return clean;
}

function byteLength(raw: string): number {
  return new TextEncoder().encode(raw).length;
}
