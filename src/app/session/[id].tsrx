/** @jsxImportSource @nativescript-community/octane */
import { useRoute } from '@octane-xplat/ui';

import { SessionView } from '@/components/session-view';

export default function SessionScreen() {
  const route = useRoute('root');
  const params = (route?.params ?? {}) as { id?: string; devPrompt?: string };
  return <SessionView devPrompt={params.devPrompt} sessionId={params.id} />;
}
