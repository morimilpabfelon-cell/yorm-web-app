export interface MobileSession {
  readonly accessToken: string;
  readonly identityId: string;
  readonly expiresAtEpochSeconds: number;
}

export type SessionState =
  | { readonly status: 'hydrating'; readonly session: null }
  | { readonly status: 'anonymous'; readonly session: null }
  | { readonly status: 'authenticated'; readonly session: MobileSession };

export type SessionAction =
  | { readonly type: 'HYDRATED'; readonly session: MobileSession | null }
  | { readonly type: 'SIGNED_IN'; readonly session: MobileSession }
  | { readonly type: 'SIGNED_OUT' };

export const initialSessionState: SessionState = {
  status: 'hydrating',
  session: null,
};

export function isSessionExpired(
  session: MobileSession,
  nowEpochSeconds = Math.floor(Date.now() / 1000),
): boolean {
  return session.expiresAtEpochSeconds <= nowEpochSeconds;
}

export function sessionReducer(
  _state: SessionState,
  action: SessionAction,
): SessionState {
  switch (action.type) {
    case 'HYDRATED':
      return action.session
        ? { status: 'authenticated', session: action.session }
        : { status: 'anonymous', session: null };
    case 'SIGNED_IN':
      return { status: 'authenticated', session: action.session };
    case 'SIGNED_OUT':
      return { status: 'anonymous', session: null };
  }
}
