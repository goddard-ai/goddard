const fs = require('fs');

function updateExports(repo) {
  const pkg = JSON.parse(fs.readFileSync(repo + '/package.json', 'utf8'));
  pkg.exports = {
    ".": "./src/index.ts"
  };
  if (repo === 'cmd') {
    pkg.exports = {
      ".": "./src/index.ts",
      "./loop": "./src/loop/index.ts"
    };
  }
  fs.writeFileSync(repo + '/package.json', JSON.stringify(pkg, null, 2) + '\n');
}

updateExports('cmd');
updateExports('sdk');
updateExports('daemon');
updateExports('github-app');
