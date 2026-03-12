import fs from 'fs';
import path from 'path';
import { spawn } from 'child_process';

// 1. Prevent infinite recursion
if (process.env.MONOREPO_BUILD_IN_PROGRESS) {
  process.exit(0);
}
process.env.MONOREPO_BUILD_IN_PROGRESS = '1';

const rootDir = process.cwd();
const buildCommand = process.argv[2] || 'build'; // Defaults to "build"

const graph = new Map<string, string[]>();      // pkgPath -> string[] (dependency paths)
const pkgNames = new Map<string, string>();     // pkgPath -> string (package name)

// 2. Recursively find workspace dependencies
function buildDependencyGraph(dir: string) {
  if (graph.has(dir)) return;

  const pkgJsonPath = path.join(dir, 'package.json');
  if (!fs.existsSync(pkgJsonPath)) {
    graph.set(dir, []);
    return;
  }

  const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'));
  pkgNames.set(dir, pkg.name || path.basename(dir));

  const allDeps = {
    ...pkg.dependencies,
    ...pkg.devDependencies,
    ...pkg.peerDependencies
  };
  const workspaceDeps: string[] = [];

  for (const dep of Object.keys(allDeps)) {
    const nmPath = path.join(dir, 'node_modules', dep);
    if (fs.existsSync(nmPath)) {
      const realPath = fs.realpathSync(nmPath);
      // If the real path isn't inside a node_modules folder, it's a local workspace package
      if (!realPath.includes(`${path.sep}node_modules${path.sep}`)) {
        workspaceDeps.push(realPath);
      }
    }
  }

  graph.set(dir, workspaceDeps);

  for (const depPath of workspaceDeps) {
    buildDependencyGraph(depPath);
  }
}

// 3. Orchestrate parallel topological execution
async function runTopologicalBuild(): Promise<void> {
  buildDependencyGraph(rootDir);

  const pending = new Set<string>(graph.keys());
  // The root package will be handled by the original process after this finishes
  pending.delete(rootDir);

  const completed = new Set<string>();
  const running = new Set<string>();

  return new Promise((resolve, reject) => {
    function checkQueue() {
      if (pending.size === 0 && running.size === 0) {
        return resolve();
      }

      const ready: string[] = [];
      for (const pkgPath of pending) {
        const deps = graph.get(pkgPath) || [];
        // Only deps that are completed or are the original root are ready
        const allDepsCompleted = deps.every(d => completed.has(d) || d === rootDir);
        if (allDepsCompleted) {
          ready.push(pkgPath);
        }
      }

      if (ready.length === 0 && running.size === 0 && pending.size > 0) {
        return reject(new Error('Circular dependency detected in workspace packages!'));
      }

      for (const pkgPath of ready) {
        pending.delete(pkgPath);
        running.add(pkgPath);
        runBuild(pkgPath).then(() => {
          running.delete(pkgPath);
          completed.add(pkgPath);
          checkQueue();
        }).catch(reject);
      }
    }

    checkQueue();
  });
}

function runBuild(pkgPath: string): Promise<void> {
  const pkgName = pkgNames.get(pkgPath);
  console.log(`\x1b[36m[📦 ${pkgName}]\x1b[0m Starting build...`);

  return new Promise((resolve, reject) => {
    // We can use any package manager, but sticking with 'npm' for now
    const child = spawn('npm', ['run', buildCommand], {
      cwd: pkgPath,
      stdio: 'inherit',
      shell: true
    });

    child.on('close', (code) => {
      if (code !== 0) {
        reject(new Error(`Build failed for ${pkgName} with exit code ${code}`));
      } else {
        console.log(`\x1b[32m[✅ ${pkgName}]\x1b[0m Build successful.`);
        resolve();
      }
    });
  });
}

runTopologicalBuild()
  .then(() => {
    console.log('\x1b[32mAll workspace dependencies built successfully.\x1b[0m');
    process.exit(0);
  })
  .catch((err) => {
    console.error(`\x1b[31m${err.message}\x1b[0m`);
    process.exit(1);
  });
