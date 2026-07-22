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
