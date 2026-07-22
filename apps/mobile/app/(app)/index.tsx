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
