const fs = require('fs');
let content = fs.readFileSync('daemon/test/daemon.test.ts', 'utf8');

content = content.replace('import { Models } from "@goddard-ai/config"\n', '');
content = content.replace('    config: {\n      models: Models,\n    },\n', '');

fs.writeFileSync('daemon/test/daemon.test.ts', content);
