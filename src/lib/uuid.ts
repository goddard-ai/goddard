/** NativeScript exposes no global crypto.randomUUID — RFC4122 v4 over
 * Math.random (same tradeoff expo-crypto documents for its JS fallback). */
export function randomUUID(): string {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    return (c === 'x' ? r : (r & 0x3) | 0x8).toString(16);
  });
}
