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
