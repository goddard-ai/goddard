import type { CycleContext, CycleStrategy } from "./types.ts";

export class DefaultStrategy implements CycleStrategy {
  nextPrompt(ctx: CycleContext): string {
    return `Cycle ${ctx.cycleNumber}. Last: ${ctx.lastSummary ?? "none"}. codebase -> ONE improvement -> SUMMARY|DONE`;
  }
}
