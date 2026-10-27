import { stripVTControlCharacters } from "node:util";

// Cargo's progress is human-readable stderr, not part of its JSON protocol.
// Treat unfamiliar output as ordinary build output and leave progress unknown.
export function parseCargoProgress(line: string): {
  done: number;
  total: number;
  detail: string;
} | undefined {
  const match = stripVTControlCharacters(line).match(
    /^\s*Building\s+\[[^\]]*\]\s+(\d+)\/(\d+)(?::\s*(.*))?\s*$/,
  );
  if (!match) return undefined;
  const done = Number(match[1]);
  const total = Number(match[2]);
  if (
    !Number.isSafeInteger(done) ||
    !Number.isSafeInteger(total) ||
    total <= 0 ||
    done > total
  ) {
    return undefined;
  }
  return { done, total, detail: match[3]?.trim() ?? "" };
}

// Both JSON stdout and terminal-style stderr can arrive at arbitrary byte
// boundaries. Cargo redraws with CR; diagnostics normally end with LF.
export async function readBuildLines(
  stream: ReadableStream<Uint8Array>,
  onLine: (line: string) => void,
): Promise<void> {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  const flush = () => {
    let start = 0;
    for (let index = 0; index < buffer.length; index += 1) {
      if (buffer[index] !== "\r" && buffer[index] !== "\n") continue;
      if (index > start) onLine(buffer.slice(start, index));
      start = index + 1;
    }
    buffer = buffer.slice(start);
  };
  try {
    for (;;) {
      const chunk = await reader.read();
      if (chunk.done) break;
      buffer += decoder.decode(chunk.value, { stream: true });
      flush();
    }
    buffer += decoder.decode();
    flush();
    if (buffer) onLine(buffer);
  } finally {
    reader.releaseLock();
  }
}
