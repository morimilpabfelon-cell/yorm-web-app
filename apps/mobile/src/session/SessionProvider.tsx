import {
  createContext,
  type PropsWithChildren,
  useContext,
  useEffect,
  useMemo,
  useReducer,
} from 'react';

import { sessionStorage } from './storage';
import {
  initialSessionState,
  type MobileSession,
  sessionReducer,
  type SessionState,
} from './state';

type SessionContextValue = SessionState & {
  readonly signIn: (session: MobileSession) => Promise<void>;
  readonly clearSession: () => Promise<void>;
};

const SessionContext = createContext<SessionContextValue | null>(null);

export function SessionProvider({ children }: PropsWithChildren) {
  const [state, dispatch] = useReducer(sessionReducer, initialSessionState);

  useEffect(() => {
    let active = true;
    void sessionStorage.read().then((session) => {
      if (active) {
        dispatch({ type: 'HYDRATED', session });
      }
    });
    return () => {
      active = false;
    };
  }, []);

  const value = useMemo<SessionContextValue>(
    () => ({
      ...state,
      async signIn(session) {
        await sessionStorage.write(session);
        dispatch({ type: 'SIGNED_IN', session });
      },
      async clearSession() {
        await sessionStorage.clear();
        dispatch({ type: 'SIGNED_OUT' });
      },
    }),
    [state],
  );

  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}

export function useSession(): SessionContextValue {
  const context = useContext(SessionContext);
  if (!context) {
    throw new Error('useSession debe utilizarse dentro de SessionProvider.');
  }
  return context;
}
