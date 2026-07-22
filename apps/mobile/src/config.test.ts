import { describe, expect, it } from 'vitest';

import { normalizeApiBaseUrl } from './config';

describe('normalizeApiBaseUrl', () => {
  it('removes the trailing slash', () => {
    expect(normalizeApiBaseUrl('http://127.0.0.1:8787/')).toBe('http://127.0.0.1:8787');
  });

  it('rejects embedded credentials', () => {
    expect(() => normalizeApiBaseUrl('https://user:secret@example.com')).toThrow(
      'no puede contener credenciales',
    );
  });

  it('rejects non-http protocols', () => {
    expect(() => normalizeApiBaseUrl('file:///tmp/yorm')).toThrow('http o https');
  });
});
