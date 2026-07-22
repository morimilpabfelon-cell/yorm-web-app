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
