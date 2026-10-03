import { useEffect, useState } from 'octane';

import { isReduceMotionEnabled, subscribeReduceMotion } from './reduce-motion';

/** Honors the system reduce-motion setting; decorative motion (the streaming
 * veil, pulses) must check this and skip, mirroring the desktop's
 * `cx.reduce_motion()` contract. */
export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(isReduceMotionEnabled());
  useEffect(() => subscribeReduceMotion(setReduced), []);
  return reduced;
}
