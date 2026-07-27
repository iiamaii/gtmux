// Pure-logic tests for the Preview edit mode (ADR-0057 D2/D3/D4).

import { describe, expect, it } from 'vitest';

import {
  canEnterEdit,
  isDraftDirty,
  isEditableKind,
  planConflictResolution,
} from './filePreviewEdit';
import {
  classifyWriteStatus,
  writeErrorMessage,
  type WriteErrorKind,
} from '$lib/http/fsWriteResult';

describe('isEditableKind', () => {
  it('accepts text/markdown/html only', () => {
    expect(isEditableKind('text')).toBe(true);
    expect(isEditableKind('markdown')).toBe(true);
    expect(isEditableKind('html')).toBe(true);
  });
  it('rejects non-text kinds', () => {
    for (const k of ['image', 'pdf', 'directory', 'empty', 'unsupported']) {
      expect(isEditableKind(k)).toBe(false);
    }
  });
});

describe('canEnterEdit', () => {
  const base = {
    multiSelection: false,
    hasSelection: true,
    loading: false,
    hasError: false,
    contentLoaded: true,
    kind: 'text',
  };
  it('true for a loaded single text/markdown/html selection', () => {
    expect(canEnterEdit(base)).toBe(true);
    expect(canEnterEdit({ ...base, kind: 'markdown' })).toBe(true);
    expect(canEnterEdit({ ...base, kind: 'html' })).toBe(true);
  });
  it('false while multi-selecting, loading, erroring, or not yet loaded', () => {
    expect(canEnterEdit({ ...base, multiSelection: true })).toBe(false);
    expect(canEnterEdit({ ...base, hasSelection: false })).toBe(false);
    expect(canEnterEdit({ ...base, loading: true })).toBe(false);
    expect(canEnterEdit({ ...base, hasError: true })).toBe(false);
    expect(canEnterEdit({ ...base, contentLoaded: false })).toBe(false);
  });
  it('false for non-editable kinds even when loaded', () => {
    expect(canEnterEdit({ ...base, kind: 'image' })).toBe(false);
    expect(canEnterEdit({ ...base, kind: 'pdf' })).toBe(false);
    expect(canEnterEdit({ ...base, kind: 'directory' })).toBe(false);
  });
});

describe('isDraftDirty', () => {
  it('detects divergence from baseline', () => {
    expect(isDraftDirty('a', 'a')).toBe(false);
    expect(isDraftDirty('a', 'b')).toBe(true);
    expect(isDraftDirty('', '')).toBe(false);
  });
});

describe('classifyWriteStatus', () => {
  const cases: Array<[number, WriteErrorKind]> = [
    [412, 'conflict'],
    [428, 'precondition_required'],
    [400, 'invalid'],
    [403, 'forbidden'],
    [404, 'not_found'],
    [413, 'too_large'],
    [500, 'unknown'],
    [418, 'unknown'],
  ];
  for (const [status, kind] of cases) {
    it(`${status} → ${kind}`, () => {
      expect(classifyWriteStatus(status)).toBe(kind);
    });
  }
});

describe('writeErrorMessage', () => {
  it('includes the status code', () => {
    expect(writeErrorMessage('invalid', 400)).toContain('400');
    expect(writeErrorMessage('too_large', 413)).toContain('413');
    expect(writeErrorMessage('unknown', 500)).toContain('500');
  });
  it('threads the body code into invalid/unknown messages', () => {
    expect(writeErrorMessage('invalid', 400, 'not_utf8')).toContain('not_utf8');
    expect(writeErrorMessage('unknown', 500, 'boom')).toContain('boom');
  });
});

describe('planConflictResolution (ADR-0057 D4)', () => {
  it('reload drops the draft and pulls server content', () => {
    const plan = planConflictResolution('reload');
    expect(plan).toEqual({ refetch: true, replaceDraft: true, rewrite: false, keepDraft: false });
  });
  it('overwrite keeps the draft and re-PUTs with a fresh etag', () => {
    const plan = planConflictResolution('overwrite');
    expect(plan).toEqual({ refetch: true, replaceDraft: false, rewrite: true, keepDraft: true });
  });
});
