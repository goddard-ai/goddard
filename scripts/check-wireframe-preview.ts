/** Non-visual macOS WebKit contract check using the actual preview assets.
 * Run through `goddard-agent resource run` with native_builds: 1.
 */
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

if (process.platform !== "darwin") throw new Error("this check requires macOS WebKit");
const root = new URL("../", import.meta.url);
const read = (path: string) => readFile(new URL(path, root), "utf8");
const [template, css, controller, base, source] = await Promise.all([
  read("src/app/html_wireframe/shell.html"),
  read("src/app/html_wireframe/shell.css"),
  read("src/app/html_wireframe/shell.js"),
  read("crates/waku-protocol/src/wireframe_html/base.css"),
  read("crates/waku-protocol/tests/fixtures/tasks.wireframe.html"),
]);
const config = {
  instance: "11111111-1111-4111-8111-111111111111",
  id: "task-review",
  // Bypass parser rejection deliberately: sandbox and CSP must protect the
  // preview independently of metadata validation.
  html: source.replace("</head>", `<style>aside { background-image: url('probe://blocked/css'); }</style><script>parent.authoredScriptRan=true</script></head>`)
    .replace("</body>", `<img src="probe://blocked/image"></body>`),
  screens: [{ id: "tasks", name: "Tasks populated", width: 960, height: 640,
    anchors: [{ id: "task-sidebar", label: "Task navigation" }] }],
  base,
  theme: { "--wf-text": "rgb(11, 22, 33)", "--wf-canvas": "white", "--wf-info": "rgb(40, 100, 200)", "--wf-color-scheme": "light" },
};
const encoded = JSON.stringify(config).replaceAll("<", "\\u003c").replaceAll("\u2028", "\\u2028").replaceAll("\u2029", "\\u2029");
const html = template.replaceAll("{{css}}", css).replaceAll("{{controller}}", controller)
  .replaceAll("{{nonce}}", "wireframe-contract-check").replaceAll("{{config}}", encoded);
const directory = await mkdtemp(join(tmpdir(), "goddard-wireframe-webkit-"));
try {
  const input = join(directory, "preview.html");
  await writeFile(input, html);
  const child = Bun.spawn(["swift", new URL("scripts/wireframe-webkit-check.swift", root).pathname, input], { stdout: "inherit", stderr: "inherit" });
  const code = await child.exited;
  if (code) throw new Error(`WebKit contract check failed (${code})`);
} finally { await rm(directory, { recursive: true, force: true }); }
