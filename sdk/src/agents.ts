import { promises as fs } from "node:fs";
import * as path from "node:path";

const SPEC_INSTRUCTIONS = `
## The \`spec\` Folder

The \`spec\` directory is the Theory of Mind for the Goddard platform.
- Humans do not edit it directly; intent is translated into structured, machine-readable specifications here.
- If you are tasked with modifying or reasoning about specifications, start at \`spec/manifest.md\` as the domain routing hub.
- Source documents (\`build.md\`, \`old-cmd/spec/\`) are preserved at the repo root for historical reference but are **not** the canonical source of intent. Where they conflict with files in this directory, this directory wins.
`;

export async function appendSpecInstructions(cwd: string = process.cwd()): Promise<string> {
  let currentDir = path.resolve(cwd);

  while (currentDir !== path.parse(currentDir).root) {
    const agentsPath = path.join(currentDir, "AGENTS.md");

    try {
      const stats = await fs.stat(agentsPath);
      if (stats.isFile()) {
        const content = await fs.readFile(agentsPath, "utf-8");
        if (!content.includes("## The `spec` Folder")) {
          await fs.appendFile(agentsPath, "\n" + SPEC_INSTRUCTIONS);
        }
        return agentsPath;
      }
    } catch {
      // File does not exist, move up
    }

    currentDir = path.dirname(currentDir);
  }

  // If we reach the root and didn't find it, create it in the original cwd
  const defaultPath = path.join(path.resolve(cwd), "AGENTS.md");
  await fs.writeFile(defaultPath, SPEC_INSTRUCTIONS.trimStart());
  return defaultPath;
}
