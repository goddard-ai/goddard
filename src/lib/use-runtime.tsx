import { useStore } from '@octane-xplat/ui';

import { runtime, runtimeState } from './runtime-store';

/** The reference app's useRuntime() context value: state slices + actions. */
export function useRuntime() {
  const state = useStore(runtimeState);
  return { ...state, ...runtime };
}
