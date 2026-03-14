const fs = require('fs');

let sdkLoop = fs.readFileSync('core/sdk/src/node/loop.ts', 'utf8');

sdkLoop = sdkLoop.replace(
  'import { createJiti } from "@mariozechner/jiti"\nimport { dirname, join } from "node:path"\nimport { mkdir, writeFile } from "node:fs/promises"\n',
  ''
);

sdkLoop = sdkLoop.replace(
  'import {\n  getGlobalConfigPath,\n  getLocalConfigPath,\n  fileExists,\n  resolveLoopConfigPath,\n} from "@goddard-ai/storage"\n',
  ''
);

sdkLoop = sdkLoop.replace(/function quoteSystemdValue[\s\S]*?\} \n\}/, '');
sdkLoop = sdkLoop.replace(/function renderSystemdEnvironment[\s\S]*?\}\n/, '');

fs.writeFileSync('core/sdk/src/node/loop.ts', sdkLoop);
