const fs = require('fs');

let index = fs.readFileSync('core/loop/src/index.ts', 'utf8');

index = index.replace(
  '// In absence of custom strategy logic, fallback to generic prompt\n        const prompt = `Cycle ${status.cycle}. Last: ${lastSummary ?? "none"}. ${validated.systemPrompt}`; // Fallback\n          cycleNumber: status.cycle,\n          lastSummary\n        });',
  '// In absence of custom strategy logic, fallback to generic prompt\n        const prompt = `Cycle ${status.cycle}. Last: ${lastSummary ?? "none"}. ${validated.systemPrompt}`;'
);

fs.writeFileSync('core/loop/src/index.ts', index);
