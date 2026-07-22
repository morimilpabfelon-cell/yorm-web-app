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
