import { expect, test } from "bun:test";
import { DevTerminal } from "./dev-terminal";

test("progress fits narrow terminals without splitting wide characters", () => {
  for (const columns of [1, 2, 40, 80, 120]) {
    const writes: string[] = [];
    const terminal = new DevTerminal((text) => { writes.push(text); }, () => columns);
    terminal.start(() => "[goddard-dev] Compiling daemon " + "界👩‍💻".repeat(80));
    const line = writes.at(-1)!;
    expect(Bun.stringWidth(line)).toBeLessThan(columns);
    expect(line).not.toContain("\n");
    expect(line).not.toContain("\ufffd");
    if (columns > 1) expect(line.endsWith("…")).toBe(true);
  }
});

test("background logs clear the transient row and redraw after the whole message", () => {
  let columns = 80;
  const writes: string[] = [];
  const terminal = new DevTerminal((text) => { writes.push(text); }, () => columns);
  terminal.start(() => "progress ".repeat(30));
  const progress = writes.at(-1)!;
  const message = "cloudflared: " + "connection details ".repeat(20) + "\n";
  writes.length = 0;
  terminal.log(() => { writes.push(message); });
  expect(writes).toEqual(["\r\x1b[2K", message, progress]);
  writes.length = 0;
  terminal.log(() => { writes.push("unterminated daemon log"); }, false);
  expect(writes).toEqual(["\r\x1b[2K", "unterminated daemon log", "\n", progress]);
  columns = 40;
  writes.length = 0;
  terminal.draw();
  expect(writes[0]).toBe("\r\x1b[2K\x1b[1A\r\x1b[2K");
  expect(Bun.stringWidth(writes[1]!)).toBeLessThan(columns);
  terminal.stop();
  writes.length = 0;
  terminal.log(() => { writes.push("finished"); }, false);
  expect(writes).toEqual(["finished"]);
});
