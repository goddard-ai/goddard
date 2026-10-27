// Keep the transient progress row separate from persistent watcher output.
export class DevTerminal {
  private render: (() => string) | undefined;
  private drawnWidth = 0;

  constructor(
    private readonly write: (text: string) => void,
    private readonly columns: () => number,
  ) {}

  start(render: () => string): void {
    this.render = render;
    this.draw();
  }

  private clear(): void {
    if (this.drawnWidth === 0) return;
    const columns = Math.max(1, this.columns());
    // On resize, the previous row may have reflowed across several rows.
    const rows = Math.ceil(this.drawnWidth / columns);
    this.write(`\r\x1b[2K${"\x1b[1A\r\x1b[2K".repeat(rows - 1)}`);
    this.drawnWidth = 0;
  }

  draw(): void {
    if (this.render === undefined) return;
    this.clear();
    // Leave the final column unused so terminals never enter auto-wrap.
    const width = Math.max(0, this.columns() - 1);
    const line = fitTerminalLine(this.render(), width);
    this.drawnWidth = Bun.stringWidth(line);
    this.write(line);
  }

  log(write: () => void, lineTerminated = true): void {
    this.clear();
    try {
      write();
      if (this.render !== undefined && !lineTerminated) this.write("\n");
    } finally {
      this.draw();
    }
  }

  stop(): void {
    this.clear();
    this.render = undefined;
  }
}

const graphemes = new Intl.Segmenter(undefined, { granularity: "grapheme" });

function fitTerminalLine(text: string, width: number): string {
  if (width <= 0) return "";
  // Progress is plain text; control characters in job names must not move the
  // cursor. Logs and diagnostics keep their original formatting separately.
  const line = text.replace(/[\x00-\x1f\x7f-\x9f]/g, " ");
  if (Bun.stringWidth(line) <= width) return line;
  let result = "";
  let used = 0;
  for (const { segment } of graphemes.segment(line)) {
    const cells = Bun.stringWidth(segment);
    if (used + cells > width - 1) break;
    result += segment;
    used += cells;
  }
  return `${result}…`;
}
