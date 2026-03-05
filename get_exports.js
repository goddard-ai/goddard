const fs = require('fs');

function updateExports(repo) {
  const pkg = JSON.parse(fs.readFileSync(repo + '/package.json', 'utf8'));
  pkg.exports = {
    ".": {
      "import": {
        "types": "./dist/index.d.mts",
        "default": "./dist/index.mjs"
      },
      "require": {
        "types": "./dist/index.d.mts",
        "default": "./dist/index.mjs"
      },
      "default": "./src/index.ts"
    }
  };
  fs.writeFileSync(repo + '/package.json', JSON.stringify(pkg, null, 2) + '\n');
}

updateExports('sdk');
updateExports('daemon');
updateExports('github-app');
