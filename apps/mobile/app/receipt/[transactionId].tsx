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
