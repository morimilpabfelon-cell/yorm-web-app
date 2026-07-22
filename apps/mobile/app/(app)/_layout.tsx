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
