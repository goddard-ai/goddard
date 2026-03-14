const fs = require('fs');

let index = fs.readFileSync('core/sdk/src/index.ts', 'utf8');

index = index.replace('import { Models } from "@goddard-ai/config"\n', '');
index = index.replace(/models: typeof Models/g, '');
index = index.replace(/models: Models,/g, '');

fs.writeFileSync('core/sdk/src/index.ts', index);
