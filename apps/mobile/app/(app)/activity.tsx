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
