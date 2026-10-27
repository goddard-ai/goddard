import { expect, test } from "bun:test";
import { parseCargoProgress, readBuildLines } from "./dev-build-output";

test("Cargo's colored progress supplies build-unit counts and active jobs", () => {
  expect(parseCargoProgress(
    "\x1b[K\x1b[1m\x1b[96m    Building\x1b[0m [====>     ] 1176/1194: waku-exec(build), waku(build)   ",
  )).toEqual({ done: 1176, total: 1194, detail: "waku-exec(build), waku(build)" });
  expect(parseCargoProgress("    Building [ ] 0/1: app")).toEqual({
    done: 0, total: 1, detail: "app",
  });
  expect(parseCargoProgress("    Building [====] 1/1")).toEqual({
    done: 1, total: 1, detail: "",
  });
  for (const line of [
    "error: build script failed",
    "    Building [====] 2/1: app",
    "    Building [====] 0/0: app",
    "    Building [====] 1/9007199254740992: app",
    "    Building [====] unknown: app",
  ]) {
    expect(parseCargoProgress(line)).toBeUndefined();
  }
});

test("split terminal redraws preserve progress, diagnostics, UTF-8 and final output", async () => {
  const input = new TextEncoder().encode(
    "\x1b[1m    Building\x1b[0m [> ] 0/2: app\r\x1b[K" +
    "    Building [=>] 1/2: helper\r\x1b[K" +
    "warning: café\r\nerror: build script failed\nfinal detail",
  );
  const lines: string[] = [];
  await readBuildLines(new ReadableStream({
    start(controller) {
      // One byte at a time exercises splits inside ANSI, counts and UTF-8.
      for (const byte of input) controller.enqueue(Uint8Array.of(byte));
      controller.close();
    },
  }), (line) => lines.push(line));
  expect(lines.map(parseCargoProgress)).toEqual([
    { done: 0, total: 2, detail: "app" },
    { done: 1, total: 2, detail: "helper" },
    undefined, undefined, undefined,
  ]);
  expect(lines.slice(2)).toEqual([
    "\x1b[Kwarning: café", "error: build script failed", "final detail",
  ]);
});
