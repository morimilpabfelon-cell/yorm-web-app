export function minorToMajorString(value: string, fractionDigits = 2): string {
  if (!/^-?\d+$/.test(value)) {
    throw new Error('El monto debe contener unidades menores enteras.');
  }
  if (!Number.isInteger(fractionDigits) || fractionDigits < 0 || fractionDigits > 6) {
    throw new Error('La precisión monetaria no es válida.');
  }

  const negative = value.startsWith('-');
  const digits = negative ? value.slice(1) : value;
  const padded = digits.padStart(fractionDigits + 1, '0');
  const integerPart = fractionDigits === 0 ? padded : padded.slice(0, -fractionDigits);
  const fractionPart = fractionDigits === 0 ? '' : padded.slice(-fractionDigits);
  const sign = negative && BigInt(digits) !== 0n ? '-' : '';

  return fractionDigits === 0
    ? `${sign}${integerPart}`
    : `${sign}${integerPart}.${fractionPart}`;
}

export function formatMoneyMinor(
  value: string,
  currency: string,
  fractionDigits = 2,
): string {
  return `${currency.toUpperCase()} ${minorToMajorString(value, fractionDigits)}`;
}

export function formatEpochSeconds(value: number): string {
  return new Intl.DateTimeFormat('es-PE', {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(value * 1000));
}
