import { describe, expect, it } from 'vitest';

import { createYormApiClient, YormApiError } from './client';

describe('Yorm API client', () => {
  it('uses Bearer auth without leaking the token into errors', async () => {
    const token = 'opaque-secret-token';
    const calls: Array<Parameters<typeof fetch>> = [];
    const fetchImpl: typeof fetch = async (...args) => {
      calls.push(args);
      return new Response(
        JSON.stringify({ error: { code: 'invalid_session', message: 'Sesión inválida.' } }),
        { status: 401, headers: { 'Content-Type': 'application/json' } },
      );
    };
    const client = createYormApiClient({
      baseUrl: 'http://127.0.0.1:8787',
      fetchImpl,
      timeoutMs: 100,
    });

    await expect(client.getMe(token)).rejects.toMatchObject({
      status: 401,
      code: 'invalid_session',
      message: 'Sesión inválida.',
    });
    expect(calls).toHaveLength(1);
    const init = calls[0]?.[1];
    expect(init?.headers).toMatchObject({ Authorization: `Bearer ${token}` });

    try {
      await client.getMe(token);
    } catch (error) {
      expect(error).toBeInstanceOf(YormApiError);
      expect(String(error)).not.toContain(token);
    }
  });

  it('reports the public API URL on network failures without leaking the token', async () => {
    const token = 'another-opaque-secret';
    const baseUrl = 'http://127.0.0.1:8787';
    const fetchImpl: typeof fetch = async () => {
      throw new TypeError('Failed to fetch');
    };
    const client = createYormApiClient({ baseUrl, fetchImpl, timeoutMs: 100 });

    await expect(client.getMe(token)).rejects.toMatchObject({
      status: 0,
      code: 'network_error',
      message: expect.stringContaining(baseUrl),
    });

    try {
      await client.getMe(token);
    } catch (error) {
      expect(String(error)).not.toContain(token);
    }
  });
});
