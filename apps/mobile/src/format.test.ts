import { describe, expect, it } from 'vitest';

import { formatMoneyMinor, minorToMajorString } from './format';

describe('minor unit formatting', () => {
  it('formats integer minor units without floating point arithmetic', () => {
    expect(minorToMajorString('1250')).toBe('12.50');
    expect(minorToMajorString('-5')).toBe('-0.05');
    expect(formatMoneyMinor('750', 'pen')).toBe('PEN 7.50');
  });

  it('rejects non-integer values', () => {
    expect(() => minorToMajorString('12.50')).toThrow('unidades menores enteras');
  });
});
