const fs = require('fs');

let index = fs.readFileSync('core/sdk/src/index.ts', 'utf8');

index = index.replace(
  'export type { GoddardLoopConfig } from "@goddard-ai/loop"\n',
  ''
);

fs.writeFileSync('core/sdk/src/index.ts', index);
