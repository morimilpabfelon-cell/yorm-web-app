import { describe, expect, it, vi } from 'vitest';

import { createYormApiClient, YormApiError } from './client';

describe('Yorm API client', () => {
  it('uses Bearer auth without leaking the token into errors', async () => {
    const token = 'opaque-secret-token';
    const fetchImpl = vi.fn(async () =>
      new Response(
        JSON.stringify({ error: { code: 'invalid_session', message: 'Sesión inválida.' } }),
        { status: 401, headers: { 'Content-Type': 'application/json' } },
      ),
    );
    const client = createYormApiClient({
      baseUrl: 'http://127.0.0.1:8787',
      fetchImpl: fetchImpl as typeof fetch,
      timeoutMs: 100,
    });

    await expect(client.getMe(token)).rejects.toMatchObject<Partial<YormApiError>>({
      status: 401,
      code: 'invalid_session',
      message: 'Sesión inválida.',
    });
    expect(fetchImpl).toHaveBeenCalledOnce();
    const init = fetchImpl.mock.calls[0]?.[1];
    expect(init?.headers).toMatchObject({ Authorization: `Bearer ${token}` });

    try {
      await client.getMe(token);
    } catch (error) {
      expect(String(error)).not.toContain(token);
    }
  });
});
