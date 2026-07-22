import type {
  ApiErrorResponse,
  CreateIdentityRequest,
  IdentityView,
  PayActivityPage,
  PayLimitsResponse,
  PayReceiptResponse,
  SessionResponse,
  SystemStatus,
  WalletView,
} from '@yorm/contracts';

import { API_BASE_URL } from '../config';

type FetchImplementation = typeof fetch;

interface ClientOptions {
  readonly baseUrl?: string;
  readonly fetchImpl?: FetchImplementation;
  readonly timeoutMs?: number;
}

interface RequestOptions {
  readonly method?: 'GET' | 'POST' | 'PUT' | 'DELETE';
  readonly accessToken?: string;
  readonly body?: unknown;
}

export class YormApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = 'YormApiError';
    this.status = status;
    this.code = code;
  }
}

function parseApiError(payload: unknown): ApiErrorResponse | null {
  if (
    typeof payload === 'object' &&
    payload !== null &&
    'error' in payload &&
    typeof payload.error === 'object' &&
    payload.error !== null &&
    'code' in payload.error &&
    typeof payload.error.code === 'string' &&
    'message' in payload.error &&
    typeof payload.error.message === 'string'
  ) {
    return payload as ApiErrorResponse;
  }
  return null;
}

export function createYormApiClient(options: ClientOptions = {}) {
  const baseUrl = (options.baseUrl ?? API_BASE_URL).replace(/\/$/, '');
  const fetchImpl = options.fetchImpl ?? fetch;
  const timeoutMs = options.timeoutMs ?? 10_000;

  async function request<T>(path: string, requestOptions: RequestOptions = {}): Promise<T> {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), timeoutMs);
    const headers: Record<string, string> = { Accept: 'application/json' };

    if (requestOptions.body !== undefined) {
      headers['Content-Type'] = 'application/json';
    }
    if (requestOptions.accessToken) {
      headers.Authorization = `Bearer ${requestOptions.accessToken}`;
    }

    try {
      const response = await fetchImpl(`${baseUrl}${path}`, {
        method: requestOptions.method ?? 'GET',
        headers,
        body:
          requestOptions.body === undefined
            ? undefined
            : JSON.stringify(requestOptions.body),
        signal: controller.signal,
      });

      if (response.status === 204) {
        return undefined as T;
      }

      const text = await response.text();
      let payload: unknown = null;
      if (text) {
        try {
          payload = JSON.parse(text) as unknown;
        } catch {
          payload = null;
        }
      }

      if (!response.ok) {
        const apiError = parseApiError(payload);
        throw new YormApiError(
          response.status,
          apiError?.error.code ?? 'http_error',
          apiError?.error.message ?? 'La API de Yorm Pay rechazó la solicitud.',
        );
      }

      return payload as T;
    } catch (error) {
      if (error instanceof YormApiError) {
        throw error;
      }
      if (error instanceof Error && error.name === 'AbortError') {
        throw new YormApiError(0, 'timeout', 'La API no respondió dentro del tiempo esperado.');
      }
      throw new YormApiError(0, 'network_error', 'No fue posible conectar con la API sandbox.');
    } finally {
      clearTimeout(timeout);
    }
  }

  return {
    getSystemStatus: () => request<SystemStatus>('/v1/system/status'),
    createIdentity: (body: CreateIdentityRequest) =>
      request<IdentityView>('/v1/sandbox/identities', { method: 'POST', body }),
    createSession: (identityId: string) =>
      request<SessionResponse>('/v1/sandbox/sessions', {
        method: 'POST',
        body: { identity_id: identityId },
      }),
    createWallet: (accessToken: string) =>
      request<WalletView>('/v1/me/wallet', { method: 'POST', accessToken }),
    getMe: (accessToken: string) =>
      request<IdentityView>('/v1/me', { accessToken }),
    getLimits: (accessToken: string) =>
      request<PayLimitsResponse>('/v1/me/limits', { accessToken }),
    getWallet: (accessToken: string) =>
      request<WalletView>('/v1/me/wallet', { accessToken }),
    getActivity: (accessToken: string, cursor?: string | null, limit = 20) => {
      const params = new URLSearchParams({ limit: String(limit) });
      if (cursor) {
        params.set('cursor', cursor);
      }
      return request<PayActivityPage>(`/v1/me/activity?${params.toString()}`, {
        accessToken,
      });
    },
    getReceipt: (accessToken: string, transactionId: string) =>
      request<PayReceiptResponse>(
        `/v1/me/receipts/${encodeURIComponent(transactionId)}`,
        { accessToken },
      ),
    logout: (accessToken: string) =>
      request<void>('/v1/me/session', { method: 'DELETE', accessToken }),
  };
}

export const yormApi = createYormApiClient();
