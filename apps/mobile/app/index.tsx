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
