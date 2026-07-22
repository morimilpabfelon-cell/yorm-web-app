const DEFAULT_API_URL = 'http://127.0.0.1:8787';

export function normalizeApiBaseUrl(value: string): string {
  const trimmed = value.trim();
  const parsed = new URL(trimmed);

  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new Error('La URL de la API debe usar http o https.');
  }
  if (parsed.username || parsed.password) {
    throw new Error('La URL pública de la API no puede contener credenciales.');
  }

  return parsed.toString().replace(/\/$/, '');
}

export const API_BASE_URL = normalizeApiBaseUrl(
  process.env.EXPO_PUBLIC_YORM_API_URL ?? DEFAULT_API_URL,
);
