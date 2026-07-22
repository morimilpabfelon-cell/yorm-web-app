import { describe, expect, it } from 'vitest';

import {
  initialSessionState,
  isSessionExpired,
  sessionReducer,
  type MobileSession,
} from './state';

const session: MobileSession = {
  accessToken: 'opaque-test-token',
  identityId: 'identity-id',
  expiresAtEpochSeconds: 2_000,
};

describe('session state', () => {
  it('hydrates and signs out deterministically', () => {
    const authenticated = sessionReducer(initialSessionState, {
      type: 'HYDRATED',
      session,
    });
    expect(authenticated.status).toBe('authenticated');
    expect(sessionReducer(authenticated, { type: 'SIGNED_OUT' })).toEqual({
      status: 'anonymous',
      session: null,
    });
  });

  it('detects expiration without exposing the token', () => {
    expect(isSessionExpired(session, 1_999)).toBe(false);
    expect(isSessionExpired(session, 2_000)).toBe(true);
  });
});
