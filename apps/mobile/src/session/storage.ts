import { Platform } from 'react-native';

import type { MobileSession } from './state';
import { isSessionExpired } from './state';

const STORAGE_KEY = 'yorm-pay.sandbox-session.v1';
let webMemoryValue: string | null = null;

function parseSession(value: string | null): MobileSession | null {
  if (!value) {
    return null;
  }

  try {
    const parsed = JSON.parse(value) as Partial<MobileSession>;
    if (
      typeof parsed.accessToken !== 'string' ||
      typeof parsed.identityId !== 'string' ||
      typeof parsed.expiresAtEpochSeconds !== 'number'
    ) {
      return null;
    }

    const session: MobileSession = {
      accessToken: parsed.accessToken,
      identityId: parsed.identityId,
      expiresAtEpochSeconds: parsed.expiresAtEpochSeconds,
    };
    return isSessionExpired(session) ? null : session;
  } catch {
    return null;
  }
}

async function secureStore() {
  return import('expo-secure-store');
}

export const sessionStorage = {
  async read(): Promise<MobileSession | null> {
    if (Platform.OS === 'web') {
      return parseSession(webMemoryValue);
    }
    const store = await secureStore();
    return parseSession(await store.getItemAsync(STORAGE_KEY));
  },

  async write(session: MobileSession): Promise<void> {
    const serialized = JSON.stringify(session);
    if (Platform.OS === 'web') {
      webMemoryValue = serialized;
      return;
    }
    const store = await secureStore();
    await store.setItemAsync(STORAGE_KEY, serialized, {
      keychainAccessible: store.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
    });
  },

  async clear(): Promise<void> {
    if (Platform.OS === 'web') {
      webMemoryValue = null;
      return;
    }
    const store = await secureStore();
    await store.deleteItemAsync(STORAGE_KEY);
  },
};
