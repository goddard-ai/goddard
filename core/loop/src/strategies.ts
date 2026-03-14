export type CycleContext = {
  cycleNumber: number;
  lastSummary?: string;
};

export type CycleStrategy = {
  nextPrompt(ctx: CycleContext): string;
};

export class DefaultStrategy implements CycleStrategy {
  nextPrompt(ctx: CycleContext): string {
    return `Cycle ${ctx.cycleNumber}. Last: ${ctx.lastSummary ?? "none"}. codebase -> ONE improvement -> SUMMARY|DONE`;
  }
}
