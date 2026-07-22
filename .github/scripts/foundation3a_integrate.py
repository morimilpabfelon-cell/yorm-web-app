from __future__ import annotations

import json
import shutil
import sys
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TEMPLATE = Path(sys.argv[1]).resolve()
MOBILE = ROOT / "apps" / "mobile"


def clean(value: str) -> str:
    return textwrap.dedent(value).lstrip("\n")


def write(relative_path: str, content: str) -> None:
    path = ROOT / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(clean(content), encoding="utf-8")


if not (TEMPLATE / "package.json").exists():
    raise SystemExit(f"Expo template not found: {TEMPLATE}")

if MOBILE.exists():
    shutil.rmtree(MOBILE)
MOBILE.mkdir(parents=True)

package = json.loads((TEMPLATE / "package.json").read_text(encoding="utf-8"))
package["name"] = "@yorm/mobile"
package["private"] = True
package["version"] = "0.0.0"
package["scripts"] = {
    "start": "expo start",
    "android": "expo start --android",
    "ios": "expo start --ios",
    "web": "expo start --web",
    "typecheck": "tsc --noEmit",
    "test": "vitest run",
    "build": "expo export --platform web --output-dir dist",
    "doctor": "expo-doctor",
    "clean": "node -e \"const fs=require('node:fs'); for (const p of ['dist','.expo']) fs.rmSync(p,{recursive:true,force:true})\"",
}
package.setdefault("dependencies", {})["@yorm/contracts"] = "workspace:*"
package["dependencies"]["@yorm/design-tokens"] = "workspace:*"
(MOBILE / "package.json").write_text(
    json.dumps(package, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
)

write(
    "apps/mobile/app.json",
    """
    {
      "expo": {
        "name": "Yorm Pay",
        "slug": "yorm-pay",
        "version": "0.0.0",
        "orientation": "portrait",
        "scheme": "yormpay",
        "userInterfaceStyle": "light",
        "newArchEnabled": true,
        "plugins": [
          "expo-router",
          "expo-secure-store"
        ],
        "web": {
          "bundler": "metro"
        }
      }
    }
    """,
)

write(
    "apps/mobile/tsconfig.json",
    """
    {
      "extends": "expo/tsconfig.base",
      "compilerOptions": {
        "strict": true,
        "noUncheckedIndexedAccess": true,
        "baseUrl": ".",
        "paths": {
          "@/*": ["src/*"]
        }
      },
      "include": [
        "**/*.ts",
        "**/*.tsx",
        ".expo/types/**/*.ts",
        "expo-env.d.ts"
      ]
    }
    """,
)

write(
    "apps/mobile/expo-env.d.ts",
    """
    /// <reference types="expo/types" />
    /// <reference types="expo-router/types" />
    """,
)

write(
    "apps/mobile/.env.example",
    """
    # iOS simulator / web on the same computer
    EXPO_PUBLIC_YORM_API_URL=http://127.0.0.1:8787

    # Android emulator normally uses the host alias below instead:
    # EXPO_PUBLIC_YORM_API_URL=http://10.0.2.2:8787
    """,
)

write(
    "apps/mobile/src/config.ts",
    """
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
    """,
)

write(
    "apps/mobile/src/format.ts",
    """
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
    """,
)

write(
    "apps/mobile/src/api/client.ts",
    """
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
    """,
)

write(
    "apps/mobile/src/session/state.ts",
    """
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
    """,
)

write(
    "apps/mobile/src/session/storage.ts",
    """
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
    """,
)

write(
    "apps/mobile/src/session/SessionProvider.tsx",
    """
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

    interface SessionContextValue extends SessionState {
      readonly signIn: (session: MobileSession) => Promise<void>;
      readonly clearSession: () => Promise<void>;
    }

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
    """,
)

write(
    "apps/mobile/src/theme.ts",
    """
    import { colors, radii, spacing } from '@yorm/design-tokens';

    export const theme = {
      colors: {
        background: colors.paper,
        surface: '#FFFFFF',
        muted: colors.stone,
        accent: colors.coral,
        text: colors.black,
        subduedText: '#5E5A54',
        error: '#9F261D',
      },
      radii,
      spacing,
    } as const;
    """,
)

write(
    "apps/mobile/src/ui.tsx",
    """
    import type { PropsWithChildren, ReactNode } from 'react';
    import {
      ActivityIndicator,
      Pressable,
      StyleSheet,
      Text,
      TextInput,
      type TextInputProps,
      View,
    } from 'react-native';

    import { theme } from './theme';

    export function SandboxBadge() {
      return (
        <View style={styles.badge} accessibilityLabel="Entorno sandbox">
          <Text style={styles.badgeText}>SANDBOX</Text>
        </View>
      );
    }

    export function Card({ children }: PropsWithChildren) {
      return <View style={styles.card}>{children}</View>;
    }

    export function PrimaryButton({
      label,
      onPress,
      disabled = false,
    }: {
      readonly label: string;
      readonly onPress: () => void;
      readonly disabled?: boolean;
    }) {
      return (
        <Pressable
          accessibilityRole="button"
          disabled={disabled}
          onPress={onPress}
          style={({ pressed }) => [
            styles.primaryButton,
            disabled && styles.disabled,
            pressed && !disabled && styles.pressed,
          ]}
        >
          <Text style={styles.primaryButtonText}>{label}</Text>
        </Pressable>
      );
    }

    export function SecondaryButton({
      label,
      onPress,
      disabled = false,
    }: {
      readonly label: string;
      readonly onPress: () => void;
      readonly disabled?: boolean;
    }) {
      return (
        <Pressable
          accessibilityRole="button"
          disabled={disabled}
          onPress={onPress}
          style={({ pressed }) => [
            styles.secondaryButton,
            disabled && styles.disabled,
            pressed && !disabled && styles.pressed,
          ]}
        >
          <Text style={styles.secondaryButtonText}>{label}</Text>
        </Pressable>
      );
    }

    export function FormField({ label, ...props }: TextInputProps & { readonly label: string }) {
      return (
        <View style={styles.fieldGroup}>
          <Text style={styles.fieldLabel}>{label}</Text>
          <TextInput placeholderTextColor={theme.colors.subduedText} style={styles.input} {...props} />
        </View>
      );
    }

    export function ErrorNotice({ message }: { readonly message: string }) {
      return (
        <View style={styles.errorBox} accessibilityRole="alert">
          <Text style={styles.errorText}>{message}</Text>
        </View>
      );
    }

    export function LoadingState({ label = 'Cargando…' }: { readonly label?: string }) {
      return (
        <View style={styles.loading}>
          <ActivityIndicator color={theme.colors.accent} />
          <Text style={styles.loadingText}>{label}</Text>
        </View>
      );
    }

    export function LabelValue({ label, value }: { readonly label: string; readonly value: ReactNode }) {
      return (
        <View style={styles.labelValue}>
          <Text style={styles.label}>{label}</Text>
          <Text style={styles.value}>{value}</Text>
        </View>
      );
    }

    const styles = StyleSheet.create({
      badge: {
        alignSelf: 'flex-start',
        backgroundColor: theme.colors.muted,
        borderRadius: theme.radii.pill,
        paddingHorizontal: theme.spacing.sm,
        paddingVertical: theme.spacing.xs,
      },
      badgeText: { color: theme.colors.text, fontSize: 12, fontWeight: '800', letterSpacing: 1.2 },
      card: {
        backgroundColor: theme.colors.surface,
        borderRadius: theme.radii.large,
        padding: theme.spacing.lg,
        gap: theme.spacing.md,
      },
      primaryButton: {
        alignItems: 'center',
        backgroundColor: theme.colors.accent,
        borderRadius: theme.radii.pill,
        paddingHorizontal: theme.spacing.lg,
        paddingVertical: theme.spacing.md,
      },
      primaryButtonText: { color: theme.colors.text, fontSize: 16, fontWeight: '800' },
      secondaryButton: {
        alignItems: 'center',
        borderColor: theme.colors.text,
        borderRadius: theme.radii.pill,
        borderWidth: 1,
        paddingHorizontal: theme.spacing.lg,
        paddingVertical: theme.spacing.md,
      },
      secondaryButtonText: { color: theme.colors.text, fontSize: 16, fontWeight: '700' },
      disabled: { opacity: 0.45 },
      pressed: { opacity: 0.72 },
      fieldGroup: { gap: theme.spacing.xs },
      fieldLabel: { color: theme.colors.text, fontSize: 14, fontWeight: '700' },
      input: {
        backgroundColor: theme.colors.surface,
        borderColor: theme.colors.muted,
        borderRadius: theme.radii.medium,
        borderWidth: 1,
        color: theme.colors.text,
        fontSize: 16,
        paddingHorizontal: theme.spacing.md,
        paddingVertical: theme.spacing.md,
      },
      errorBox: {
        backgroundColor: '#FDE9E6',
        borderRadius: theme.radii.medium,
        padding: theme.spacing.md,
      },
      errorText: { color: theme.colors.error, fontWeight: '600' },
      loading: { alignItems: 'center', gap: theme.spacing.sm, justifyContent: 'center', padding: theme.spacing.xl },
      loadingText: { color: theme.colors.subduedText },
      labelValue: { gap: theme.spacing.xxs },
      label: { color: theme.colors.subduedText, fontSize: 12, fontWeight: '700', textTransform: 'uppercase' },
      value: { color: theme.colors.text, fontSize: 16, fontWeight: '600' },
    });
    """,
)

write(
    "apps/mobile/app/_layout.tsx",
    """
    import { Stack } from 'expo-router';
    import { StatusBar } from 'expo-status-bar';
    import { SafeAreaProvider } from 'react-native-safe-area-context';

    import { SessionProvider } from '@/session/SessionProvider';
    import { theme } from '@/theme';

    export default function RootLayout() {
      return (
        <SafeAreaProvider>
          <SessionProvider>
            <StatusBar style="dark" />
            <Stack
              screenOptions={{
                contentStyle: { backgroundColor: theme.colors.background },
                headerShadowVisible: false,
                headerStyle: { backgroundColor: theme.colors.background },
                headerTintColor: theme.colors.text,
              }}
            >
              <Stack.Screen name="index" options={{ headerShown: false }} />
              <Stack.Screen name="sandbox-access" options={{ title: 'Acceso sandbox' }} />
              <Stack.Screen name="(app)" options={{ headerShown: false }} />
              <Stack.Screen name="receipt/[transactionId]" options={{ title: 'Pay Receipt' }} />
            </Stack>
          </SessionProvider>
        </SafeAreaProvider>
      );
    }
    """,
)

write(
    "apps/mobile/app/index.tsx",
    """
    import { Redirect } from 'expo-router';

    import { useSession } from '@/session/SessionProvider';
    import { LoadingState } from '@/ui';

    export default function IndexScreen() {
      const session = useSession();

      if (session.status === 'hydrating') {
        return <LoadingState label="Abriendo Yorm Pay…" />;
      }

      return <Redirect href={session.status === 'authenticated' ? '/(app)' : '/sandbox-access'} />;
    }
    """,
)

write(
    "apps/mobile/app/sandbox-access.tsx",
    """
    import { useState } from 'react';
    import {
      KeyboardAvoidingView,
      Platform,
      ScrollView,
      StyleSheet,
      Text,
      View,
    } from 'react-native';
    import { router } from 'expo-router';

    import { yormApi, YormApiError } from '@/api/client';
    import { API_BASE_URL } from '@/config';
    import { useSession } from '@/session/SessionProvider';
    import { theme } from '@/theme';
    import { Card, ErrorNotice, FormField, PrimaryButton, SandboxBadge } from '@/ui';

    export default function SandboxAccessScreen() {
      const { signIn } = useSession();
      const [email, setEmail] = useState('');
      const [displayName, setDisplayName] = useState('');
      const [countryCode, setCountryCode] = useState('PE');
      const [submitting, setSubmitting] = useState(false);
      const [error, setError] = useState<string | null>(null);

      async function submit() {
        if (!email.trim() || !displayName.trim() || countryCode.trim().length !== 2) {
          setError('Completa correo, nombre y código de país de dos letras.');
          return;
        }

        setSubmitting(true);
        setError(null);
        try {
          const identity = await yormApi.createIdentity({
            email: email.trim().toLowerCase(),
            display_name: displayName.trim(),
            country_code: countryCode.trim().toUpperCase(),
          });
          const session = await yormApi.createSession(identity.id);
          await yormApi.createWallet(session.access_token);
          await signIn({
            accessToken: session.access_token,
            identityId: identity.id,
            expiresAtEpochSeconds: session.expires_at_epoch_seconds,
          });
          router.replace('/(app)');
        } catch (cause) {
          setError(
            cause instanceof YormApiError
              ? cause.message
              : 'No fue posible crear el acceso sandbox.',
          );
        } finally {
          setSubmitting(false);
        }
      }

      return (
        <KeyboardAvoidingView
          behavior={Platform.OS === 'ios' ? 'padding' : undefined}
          style={styles.flex}
        >
          <ScrollView contentContainerStyle={styles.container} keyboardShouldPersistTaps="handled">
            <SandboxBadge />
            <View style={styles.heading}>
              <Text style={styles.title}>Yorm Pay</Text>
              <Text style={styles.subtitle}>
                Crea una identidad ficticia para consultar la wallet y el ledger sandbox.
              </Text>
            </View>

            <Card>
              <FormField
                autoCapitalize="none"
                autoComplete="email"
                keyboardType="email-address"
                label="Correo sandbox"
                onChangeText={setEmail}
                placeholder="usuario@yorm.local"
                value={email}
              />
              <FormField
                autoCapitalize="words"
                label="Nombre visible"
                onChangeText={setDisplayName}
                placeholder="Nombre de prueba"
                value={displayName}
              />
              <FormField
                autoCapitalize="characters"
                label="País"
                maxLength={2}
                onChangeText={setCountryCode}
                value={countryCode}
              />
              {error ? <ErrorNotice message={error} /> : null}
              <PrimaryButton
                disabled={submitting}
                label={submitting ? 'Creando acceso…' : 'Entrar al sandbox'}
                onPress={() => void submit()}
              />
            </Card>

            <Text style={styles.endpoint}>API: {API_BASE_URL}</Text>
            <Text style={styles.legal}>
              No representa dinero real, una cuenta bancaria ni una verificación KYC.
            </Text>
          </ScrollView>
        </KeyboardAvoidingView>
      );
    }

    const styles = StyleSheet.create({
      flex: { flex: 1 },
      container: {
        backgroundColor: theme.colors.background,
        flexGrow: 1,
        gap: theme.spacing.lg,
        padding: theme.spacing.lg,
      },
      heading: { gap: theme.spacing.sm, paddingTop: theme.spacing.lg },
      title: { color: theme.colors.text, fontSize: 44, fontWeight: '900', letterSpacing: -1.5 },
      subtitle: { color: theme.colors.subduedText, fontSize: 18, lineHeight: 26 },
      endpoint: { color: theme.colors.subduedText, fontSize: 12 },
      legal: { color: theme.colors.subduedText, fontSize: 13, lineHeight: 19 },
    });
    """,
)

write(
    "apps/mobile/app/(app)/_layout.tsx",
    """
    import { Redirect, Tabs } from 'expo-router';

    import { useSession } from '@/session/SessionProvider';
    import { theme } from '@/theme';
    import { LoadingState } from '@/ui';

    export default function AuthenticatedLayout() {
      const session = useSession();

      if (session.status === 'hydrating') {
        return <LoadingState />;
      }
      if (session.status !== 'authenticated') {
        return <Redirect href="/sandbox-access" />;
      }

      return (
        <Tabs
          screenOptions={{
            headerShadowVisible: false,
            headerStyle: { backgroundColor: theme.colors.background },
            headerTintColor: theme.colors.text,
            tabBarActiveTintColor: theme.colors.accent,
            tabBarInactiveTintColor: theme.colors.subduedText,
            tabBarStyle: { backgroundColor: theme.colors.surface, borderTopColor: theme.colors.muted },
          }}
        >
          <Tabs.Screen name="index" options={{ title: 'Inicio', headerTitle: 'Yorm Pay' }} />
          <Tabs.Screen name="activity" options={{ title: 'Actividad', headerTitle: 'Pay Activity' }} />
        </Tabs>
      );
    }
    """,
)

write(
    "apps/mobile/app/(app)/index.tsx",
    """
    import type { IdentityView, PayLimitsResponse, WalletView } from '@yorm/contracts';
    import { router } from 'expo-router';
    import { useCallback, useEffect, useState } from 'react';
    import { RefreshControl, ScrollView, StyleSheet, Text, View } from 'react-native';

    import { yormApi, YormApiError } from '@/api/client';
    import { formatMoneyMinor } from '@/format';
    import { useSession } from '@/session/SessionProvider';
    import { theme } from '@/theme';
    import {
      Card,
      ErrorNotice,
      LabelValue,
      LoadingState,
      SandboxBadge,
      SecondaryButton,
    } from '@/ui';

    interface DashboardData {
      readonly identity: IdentityView;
      readonly limits: PayLimitsResponse;
      readonly wallet: WalletView;
    }

    export default function HomeScreen() {
      const sessionState = useSession();
      const session = sessionState.status === 'authenticated' ? sessionState.session : null;
      const [data, setData] = useState<DashboardData | null>(null);
      const [loading, setLoading] = useState(true);
      const [refreshing, setRefreshing] = useState(false);
      const [error, setError] = useState<string | null>(null);

      const load = useCallback(async () => {
        if (!session) return;
        setError(null);
        try {
          const [identity, limits, wallet] = await Promise.all([
            yormApi.getMe(session.accessToken),
            yormApi.getLimits(session.accessToken),
            yormApi.getWallet(session.accessToken),
          ]);
          setData({ identity, limits, wallet });
        } catch (cause) {
          setError(cause instanceof YormApiError ? cause.message : 'No se pudo cargar la wallet.');
        } finally {
          setLoading(false);
          setRefreshing(false);
        }
      }, [session]);

      useEffect(() => {
        void load();
      }, [load]);

      async function logout() {
        if (session) {
          try {
            await yormApi.logout(session.accessToken);
          } catch {
            // La sesión local se elimina incluso si la revocación remota falla por conectividad.
          }
        }
        await sessionState.clearSession();
        router.replace('/sandbox-access');
      }

      if (loading) {
        return <LoadingState label="Consultando el ledger…" />;
      }

      return (
        <ScrollView
          contentContainerStyle={styles.container}
          refreshControl={
            <RefreshControl
              refreshing={refreshing}
              onRefresh={() => {
                setRefreshing(true);
                void load();
              }}
              tintColor={theme.colors.accent}
            />
          }
        >
          <SandboxBadge />
          {error ? <ErrorNotice message={error} /> : null}

          {data ? (
            <>
              <View style={styles.heading}>
                <Text style={styles.greeting}>Hola, {data.identity.display_name}</Text>
                <Text style={styles.caption}>Saldo confirmado por el backend sandbox</Text>
              </View>

              <Card>
                <Text style={styles.balanceLabel}>Saldo disponible</Text>
                <Text style={styles.balance}>
                  {formatMoneyMinor(data.wallet.balance_minor_units, data.wallet.currency)}
                </Text>
                <Text style={styles.caption}>El móvil no calcula ni modifica este saldo.</Text>
              </Card>

              <Card>
                <Text style={styles.sectionTitle}>Pay Limits</Text>
                <LabelValue
                  label="Por operación"
                  value={formatMoneyMinor(
                    data.limits.per_operation_minor_units,
                    data.limits.currency,
                  )}
                />
                <LabelValue label="Nivel" value={data.limits.kyc_tier} />
                <LabelValue label="Pagos reales" value="Deshabilitados" />
              </Card>

              <Card>
                <Text style={styles.sectionTitle}>Cuenta sandbox</Text>
                <LabelValue label="Correo" value={data.identity.email} />
                <LabelValue label="País" value={data.identity.country_code} />
                <LabelValue label="Wallet" value={data.wallet.id} />
              </Card>
            </>
          ) : null}

          <SecondaryButton label="Cerrar sesión" onPress={() => void logout()} />
        </ScrollView>
      );
    }

    const styles = StyleSheet.create({
      container: {
        backgroundColor: theme.colors.background,
        flexGrow: 1,
        gap: theme.spacing.lg,
        padding: theme.spacing.lg,
      },
      heading: { gap: theme.spacing.xs },
      greeting: { color: theme.colors.text, fontSize: 30, fontWeight: '900' },
      caption: { color: theme.colors.subduedText, fontSize: 14, lineHeight: 20 },
      balanceLabel: { color: theme.colors.subduedText, fontSize: 14, fontWeight: '700' },
      balance: { color: theme.colors.text, fontSize: 40, fontWeight: '900', letterSpacing: -1 },
      sectionTitle: { color: theme.colors.text, fontSize: 20, fontWeight: '900' },
    });
    """,
)

write(
    "apps/mobile/app/(app)/activity.tsx",
    """
    import type { PayActivityItem } from '@yorm/contracts';
    import { router } from 'expo-router';
    import { useCallback, useEffect, useState } from 'react';
    import {
      FlatList,
      Pressable,
      RefreshControl,
      StyleSheet,
      Text,
      View,
    } from 'react-native';

    import { yormApi, YormApiError } from '@/api/client';
    import { formatEpochSeconds, formatMoneyMinor } from '@/format';
    import { useSession } from '@/session/SessionProvider';
    import { theme } from '@/theme';
    import { ErrorNotice, LoadingState, SandboxBadge } from '@/ui';

    export default function ActivityScreen() {
      const sessionState = useSession();
      const session = sessionState.status === 'authenticated' ? sessionState.session : null;
      const [items, setItems] = useState<readonly PayActivityItem[]>([]);
      const [cursor, setCursor] = useState<string | null>(null);
      const [loading, setLoading] = useState(true);
      const [loadingMore, setLoadingMore] = useState(false);
      const [refreshing, setRefreshing] = useState(false);
      const [error, setError] = useState<string | null>(null);

      const load = useCallback(
        async (nextCursor: string | null, append: boolean) => {
          if (!session) return;
          setError(null);
          try {
            const page = await yormApi.getActivity(session.accessToken, nextCursor, 20);
            setItems((current) => (append ? [...current, ...page.items] : [...page.items]));
            setCursor(page.next_cursor);
          } catch (cause) {
            setError(
              cause instanceof YormApiError ? cause.message : 'No se pudo cargar Pay Activity.',
            );
          } finally {
            setLoading(false);
            setLoadingMore(false);
            setRefreshing(false);
          }
        },
        [session],
      );

      useEffect(() => {
        void load(null, false);
      }, [load]);

      if (loading) {
        return <LoadingState label="Cargando Pay Activity…" />;
      }

      return (
        <FlatList
          contentContainerStyle={styles.container}
          data={items}
          keyExtractor={(item) => item.transaction_id}
          ListHeaderComponent={
            <View style={styles.header}>
              <SandboxBadge />
              {error ? <ErrorNotice message={error} /> : null}
            </View>
          }
          ListEmptyComponent={
            <View style={styles.empty}>
              <Text style={styles.emptyTitle}>Sin movimientos</Text>
              <Text style={styles.emptyText}>
                Pay Activity aparecerá cuando el backend confirme una operación sandbox.
              </Text>
            </View>
          }
          ListFooterComponent={
            loadingMore ? <LoadingState label="Cargando más…" /> : <View style={styles.footer} />
          }
          onEndReached={() => {
            if (cursor && !loadingMore) {
              setLoadingMore(true);
              void load(cursor, true);
            }
          }}
          onEndReachedThreshold={0.4}
          refreshControl={
            <RefreshControl
              refreshing={refreshing}
              onRefresh={() => {
                setRefreshing(true);
                void load(null, false);
              }}
              tintColor={theme.colors.accent}
            />
          }
          renderItem={({ item }) => (
            <Pressable
              accessibilityRole="button"
              disabled={!item.receipt_available}
              onPress={() =>
                router.push({
                  pathname: '/receipt/[transactionId]',
                  params: { transactionId: item.transaction_id },
                })
              }
              style={({ pressed }) => [styles.item, pressed && styles.pressed]}
            >
              <View style={styles.row}>
                <Text style={styles.kind}>
                  {item.transaction_kind === 'sandbox_credit' ? 'Crédito sandbox' : 'Transferencia P2P'}
                </Text>
                <Text style={item.direction === 'credit' ? styles.credit : styles.debit}>
                  {item.direction === 'credit' ? '+' : '-'}
                  {formatMoneyMinor(item.amount_minor_units, item.currency)}
                </Text>
              </View>
              <Text style={styles.meta}>{formatEpochSeconds(item.posted_at_epoch_seconds)}</Text>
              <Text style={styles.meta}>
                {item.counterparty ? item.counterparty.display_name : 'Yorm Pay sandbox'}
              </Text>
              <Text style={styles.balanceAfter}>
                Saldo posterior: {formatMoneyMinor(item.balance_after_minor_units, item.currency)}
              </Text>
            </Pressable>
          )}
        />
      );
    }

    const styles = StyleSheet.create({
      container: { backgroundColor: theme.colors.background, flexGrow: 1, padding: theme.spacing.lg },
      header: { gap: theme.spacing.md, marginBottom: theme.spacing.lg },
      item: {
        backgroundColor: theme.colors.surface,
        borderRadius: theme.radii.large,
        gap: theme.spacing.xs,
        marginBottom: theme.spacing.md,
        padding: theme.spacing.lg,
      },
      pressed: { opacity: 0.7 },
      row: { alignItems: 'center', flexDirection: 'row', gap: theme.spacing.sm, justifyContent: 'space-between' },
      kind: { color: theme.colors.text, flex: 1, fontSize: 16, fontWeight: '800' },
      credit: { color: '#176A3A', fontSize: 16, fontWeight: '900' },
      debit: { color: theme.colors.error, fontSize: 16, fontWeight: '900' },
      meta: { color: theme.colors.subduedText, fontSize: 13 },
      balanceAfter: { color: theme.colors.text, fontSize: 13, fontWeight: '700', marginTop: theme.spacing.xs },
      empty: { alignItems: 'center', gap: theme.spacing.sm, padding: theme.spacing.xxl },
      emptyTitle: { color: theme.colors.text, fontSize: 22, fontWeight: '900' },
      emptyText: { color: theme.colors.subduedText, lineHeight: 21, textAlign: 'center' },
      footer: { height: theme.spacing.lg },
    });
    """,
)

write(
    "apps/mobile/app/receipt/[transactionId].tsx",
    """
    import type { PayReceiptResponse } from '@yorm/contracts';
    import { useLocalSearchParams } from 'expo-router';
    import { useEffect, useState } from 'react';
    import { ScrollView, StyleSheet, Text } from 'react-native';

    import { yormApi, YormApiError } from '@/api/client';
    import { formatEpochSeconds, formatMoneyMinor } from '@/format';
    import { useSession } from '@/session/SessionProvider';
    import { theme } from '@/theme';
    import { Card, ErrorNotice, LabelValue, LoadingState, SandboxBadge } from '@/ui';

    export default function ReceiptScreen() {
      const params = useLocalSearchParams<{ transactionId?: string | string[] }>();
      const transactionId = Array.isArray(params.transactionId)
        ? params.transactionId[0]
        : params.transactionId;
      const sessionState = useSession();
      const session = sessionState.status === 'authenticated' ? sessionState.session : null;
      const [receipt, setReceipt] = useState<PayReceiptResponse | null>(null);
      const [loading, setLoading] = useState(true);
      const [error, setError] = useState<string | null>(null);

      useEffect(() => {
        if (!session || !transactionId) {
          setError('No se recibió una transacción válida.');
          setLoading(false);
          return;
        }

        let active = true;
        void yormApi
          .getReceipt(session.accessToken, transactionId)
          .then((value) => {
            if (active) setReceipt(value);
          })
          .catch((cause: unknown) => {
            if (active) {
              setError(
                cause instanceof YormApiError
                  ? cause.message
                  : 'No fue posible consultar Pay Receipt.',
              );
            }
          })
          .finally(() => {
            if (active) setLoading(false);
          });

        return () => {
          active = false;
        };
      }, [session, transactionId]);

      if (loading) {
        return <LoadingState label="Verificando el comprobante…" />;
      }

      return (
        <ScrollView contentContainerStyle={styles.container}>
          <SandboxBadge />
          {error ? <ErrorNotice message={error} /> : null}
          {receipt ? (
            <>
              <Card>
                <Text style={styles.title}>Operación confirmada</Text>
                <Text style={styles.amount}>
                  {receipt.direction === 'credit' ? '+' : '-'}
                  {formatMoneyMinor(receipt.amount_minor_units, receipt.currency)}
                </Text>
                <Text style={styles.caption}>
                  Emitido únicamente a partir de una transacción posteada y balanceada.
                </Text>
              </Card>

              <Card>
                <LabelValue label="Referencia" value={receipt.receipt_reference} />
                <LabelValue label="Estado" value={receipt.status} />
                <LabelValue label="Dirección" value={receipt.direction} />
                <LabelValue label="Fecha" value={formatEpochSeconds(receipt.posted_at_epoch_seconds)} />
                <LabelValue label="Transacción" value={receipt.transaction_id} />
                <LabelValue
                  label="Saldo posterior"
                  value={formatMoneyMinor(receipt.balance_after_minor_units, receipt.currency)}
                />
              </Card>

              <Card>
                <Text style={styles.sectionTitle}>Verificación del ledger</Text>
                <LabelValue label="Asientos" value={String(receipt.ledger_entry_count)} />
                <LabelValue
                  label="Débitos"
                  value={formatMoneyMinor(
                    receipt.ledger_debit_total_minor_units,
                    receipt.currency,
                  )}
                />
                <LabelValue
                  label="Créditos"
                  value={formatMoneyMinor(
                    receipt.ledger_credit_total_minor_units,
                    receipt.currency,
                  )}
                />
              </Card>
            </>
          ) : null}
        </ScrollView>
      );
    }

    const styles = StyleSheet.create({
      container: {
        backgroundColor: theme.colors.background,
        flexGrow: 1,
        gap: theme.spacing.lg,
        padding: theme.spacing.lg,
      },
      title: { color: theme.colors.text, fontSize: 24, fontWeight: '900' },
      amount: { color: theme.colors.text, fontSize: 38, fontWeight: '900', letterSpacing: -1 },
      caption: { color: theme.colors.subduedText, fontSize: 13, lineHeight: 19 },
      sectionTitle: { color: theme.colors.text, fontSize: 20, fontWeight: '900' },
    });
    """,
)

write(
    "apps/mobile/app/+not-found.tsx",
    """
    import { Link, Stack } from 'expo-router';
    import { StyleSheet, Text, View } from 'react-native';

    import { theme } from '@/theme';

    export default function NotFoundScreen() {
      return (
        <>
          <Stack.Screen options={{ title: 'No encontrado' }} />
          <View style={styles.container}>
            <Text style={styles.title}>Esta pantalla no existe.</Text>
            <Link href="/" style={styles.link}>Volver a Yorm Pay</Link>
          </View>
        </>
      );
    }

    const styles = StyleSheet.create({
      container: { alignItems: 'center', flex: 1, gap: theme.spacing.md, justifyContent: 'center', padding: theme.spacing.lg },
      title: { color: theme.colors.text, fontSize: 22, fontWeight: '900' },
      link: { color: theme.colors.accent, fontSize: 16, fontWeight: '800' },
    });
    """,
)

write(
    "apps/mobile/src/config.test.ts",
    """
    import { describe, expect, it } from 'vitest';

    import { normalizeApiBaseUrl } from './config';

    describe('normalizeApiBaseUrl', () => {
      it('removes the trailing slash', () => {
        expect(normalizeApiBaseUrl('http://127.0.0.1:8787/')).toBe('http://127.0.0.1:8787');
      });

      it('rejects embedded credentials', () => {
        expect(() => normalizeApiBaseUrl('https://user:secret@example.com')).toThrow(
          'no puede contener credenciales',
        );
      });

      it('rejects non-http protocols', () => {
        expect(() => normalizeApiBaseUrl('file:///tmp/yorm')).toThrow('http o https');
      });
    });
    """,
)

write(
    "apps/mobile/src/format.test.ts",
    """
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
    """,
)

write(
    "apps/mobile/src/session/state.test.ts",
    """
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
    """,
)

write(
    "apps/mobile/src/api/client.test.ts",
    """
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
    """,
)

write(
    "apps/mobile/vitest.config.ts",
    """
    import { defineConfig } from 'vitest/config';

    export default defineConfig({
      test: {
        environment: 'node',
        include: ['src/**/*.test.ts'],
      },
    });
    """,
)

# Workspace packages expose source types to consumers before dist is built.
for relative in ["packages/contracts/package.json", "packages/design-tokens/package.json"]:
    path = ROOT / relative
    value = json.loads(path.read_text(encoding="utf-8"))
    value["types"] = "./src/index.ts"
    value["exports"] = {
        ".": {
            "types": "./src/index.ts",
            "react-native": "./src/index.ts",
            "import": "./dist/index.js",
            "default": "./dist/index.js",
        }
    }
    value["main"] = "./dist/index.js"
    value["react-native"] = "./src/index.ts"
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")

root_package_path = ROOT / "package.json"
root_package = json.loads(root_package_path.read_text(encoding="utf-8"))
root_package.setdefault("scripts", {})["test"] = "pnpm -r --if-present run test"
root_package_path.write_text(json.dumps(root_package, indent=2) + "\n", encoding="utf-8")

readme_path = ROOT / "README.md"
readme = readme_path.read_text(encoding="utf-8")
readme = readme.replace("FOUNDATION 2C — IN PROGRESS", "FOUNDATION 3A — IN PROGRESS")
readme = readme.replace("mobile/    frontera futura React Native/Expo", "mobile/    Expo/React Native — cliente sandbox")
readme = readme.replace("Tracks #11.", "Tracks #13.")
if "## Aplicación móvil sandbox" not in readme:
    readme += clean(
        """

        ## Aplicación móvil sandbox

        Foundation 3A incorpora un cliente Expo/React Native en `apps/mobile`.

        ```powershell
        Copy-Item .\apps\mobile\.env.example .\apps\mobile\.env
        pnpm --filter @yorm/mobile start
        ```

        La URL pública del backend se configura con `EXPO_PUBLIC_YORM_API_URL`. No debe contener secretos. En Android Emulator suele utilizarse `http://10.0.2.2:8787`; en web o iOS Simulator sobre el mismo equipo puede utilizarse `http://127.0.0.1:8787`.

        Validación estática:

        ```powershell
        pnpm typecheck
        pnpm test
        pnpm build
        ```

        El cliente móvil crea identidad, sesión y wallet únicamente en sandbox; después consulta perfil, Pay Limits, saldo, Pay Activity y Pay Receipt. El ledger sigue siendo la única fuente de verdad financiera.
        """
    )
readme_path.write_text(readme, encoding="utf-8")

agents_path = ROOT / "AGENTS.md"
agents = agents_path.read_text(encoding="utf-8")
mobile_rules = """
- El cliente móvil no calcula saldos ni fabrica estados financieros; consume respuestas confirmadas de la API.
- En Android/iOS el token Bearer solo puede persistirse mediante SecureStore; no usar AsyncStorage.
- Variables `EXPO_PUBLIC_*` son públicas y nunca pueden contener secretos.
- La exportación web no persiste la sesión entre recargas.
- Foundation 3A no incluye envío P2P, crédito sandbox, biometría, notificaciones, cámara, QR, NFC ni publicación en tiendas.
"""
if "El cliente móvil no calcula saldos" not in agents:
    marker = "- No exponer claves idempotentes, fingerprints internos ni códigos de cuenta en respuestas de actividad o recibos.\n"
    agents = agents.replace(marker, marker + mobile_rules)
agents = agents.replace(
    "Issue #11\nFoundation 2C\nPay Activity + Pay Receipt derivados del ledger\nRiesgo R3.3",
    "Issue #13\nFoundation 3A\nBase móvil Expo/React Native y cliente API sandbox\nRiesgo R4.1",
)
agents_path.write_text(agents, encoding="utf-8")

gitignore_path = ROOT / ".gitignore"
gitignore = gitignore_path.read_text(encoding="utf-8") if gitignore_path.exists() else ""
for entry in ["apps/mobile/.expo/", "apps/mobile/dist/", "apps/mobile/.env"]:
    if entry not in gitignore:
        gitignore += f"\n{entry}"
gitignore_path.write_text(gitignore.lstrip("\n") + "\n", encoding="utf-8")
