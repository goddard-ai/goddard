const fs = require('fs');
let index = fs.readFileSync('core/sdk/src/node/index.ts', 'utf8');

index = index.replace('init: loop.initLoopConfig,', '');
index = index.replace('generateSystemdService: loop.generateLoopSystemdService,', '');

fs.writeFileSync('core/sdk/src/node/index.ts', index);
