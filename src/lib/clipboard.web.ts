export async function setStringAsync(value: string): Promise<void> {
  await navigator.clipboard.writeText(value);
}
