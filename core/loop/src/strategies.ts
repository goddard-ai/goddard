import type { LoopContext, LoopStrategy } from "@goddard-ai/schema/loop"

export class DefaultStrategy implements LoopStrategy {
  nextPrompt(ctx: LoopContext): string {
    return `Cycle ${ctx.cycleNumber}. Last: ${ctx.lastSummary ?? "none"}. codebase -> ONE improvement -> SUMMARY|DONE`
  }
}
