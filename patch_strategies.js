const fs = require('fs');

const content = `export type CycleContext = {
  cycleNumber: number;
  lastSummary?: string;
};

export type CycleStrategy = {
  nextPrompt(ctx: CycleContext): string;
};

export class DefaultStrategy implements CycleStrategy {
  nextPrompt(ctx: CycleContext): string {
    return \`Cycle \${ctx.cycleNumber}. Last: \${ctx.lastSummary ?? "none"}. codebase -> ONE improvement -> SUMMARY|DONE\`;
  }
}
`;
fs.writeFileSync('core/loop/src/strategies.ts', content);
