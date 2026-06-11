// componentSettings — browser-local preferences for canvas component surfaces.
//
// These settings affect only presentation in the current browser. They are not
// layout state and must not be written into session JSON or sent to the backend.

export interface ComponentSettings {
  document_scale: number;
  preview_scale: number;
  note_scale: number;
}

export type ComponentScaleKey = keyof ComponentSettings;

interface ComponentSettingsPayload {
  version: 1;
  settings: Partial<ComponentSettings>;
}

const STORAGE_KEY = 'gtmux-component-settings:v1';
export const COMPONENT_SCALE_MIN = 0.75;
export const COMPONENT_SCALE_MAX = 2;
export const COMPONENT_SCALE_STEP = 0.05;

const DEFAULT_SETTINGS: ComponentSettings = {
  document_scale: 1,
  preview_scale: 1,
  note_scale: 1,
};

function clampScale(value: number): number {
  if (!Number.isFinite(value)) return 1;
  const clamped = Math.min(COMPONENT_SCALE_MAX, Math.max(COMPONENT_SCALE_MIN, value));
  return Math.round(clamped / COMPONENT_SCALE_STEP) * COMPONENT_SCALE_STEP;
}

function readScale(value: unknown, fallback: number): number {
  return typeof value === 'number' ? clampScale(value) : fallback;
}

function loadSettings(): ComponentSettings {
  if (typeof localStorage === 'undefined') return { ...DEFAULT_SETTINGS };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return { ...DEFAULT_SETTINGS };
    const parsed = JSON.parse(raw) as Partial<ComponentSettingsPayload>;
    if (parsed.version !== 1 || typeof parsed.settings !== 'object' || parsed.settings === null) {
      return { ...DEFAULT_SETTINGS };
    }
    return {
      document_scale: readScale(parsed.settings.document_scale, DEFAULT_SETTINGS.document_scale),
      preview_scale: readScale(parsed.settings.preview_scale, DEFAULT_SETTINGS.preview_scale),
      note_scale: readScale(parsed.settings.note_scale, DEFAULT_SETTINGS.note_scale),
    };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

class ComponentSettingsStore {
  settings = $state<ComponentSettings>(loadSettings());

  get documentScale(): number {
    return this.settings.document_scale;
  }

  get previewScale(): number {
    return this.settings.preview_scale;
  }

  get noteScale(): number {
    return this.settings.note_scale;
  }

  setScale(key: ComponentScaleKey, value: number): void {
    this.settings = {
      ...this.settings,
      [key]: clampScale(value),
    };
    this.#persist();
  }

  reset(): void {
    this.settings = { ...DEFAULT_SETTINGS };
    this.#persist();
  }

  #persist(): void {
    if (typeof localStorage === 'undefined') return;
    const payload: ComponentSettingsPayload = {
      version: 1,
      settings: this.settings,
    };
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
    } catch {
      // Browser privacy/quota failures must not break rendering.
    }
  }
}

export const componentSettings = new ComponentSettingsStore();
