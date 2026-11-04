import { afterEach, describe, expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import updateReleasePr from "./release-pr.cjs";
import { nextVersion, packageVersion, prepareRelease } from "./prepare-release";

const originalCwd = process.cwd();
const directories: string[] = [];
function temp() {
  const path = mkdtempSync(join(tmpdir(), "release-pr-test-"));
  directories.push(path);
  return path;
}
afterEach(() => {
  process.chdir(originalCwd);
  for (const path of directories.splice(0)) rmSync(path, { recursive: true, force: true });
});

function git(...args: string[]): string {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}
function writePackage(root: string) {
  writeFileSync(join(root, "Cargo.toml"), '[package]\nname = "waku"\nversion = "0.18.0"\n');
  writeFileSync(join(root, "Cargo.lock"), 'version = 4\n\n[[package]]\nname = "waku"\nversion = "0.18.0"\n');
  writeFileSync(join(root, "CHANGELOG.md"), "# Changelog\n\n## [0.18.0]\n\nPrevious release.\n");
}

function fixture() {
  const root = temp();
  const remote = join(root, "remote.git");
  execFileSync("git", ["init", "--bare", remote], { stdio: "ignore" });
  const checkout = join(root, "checkout");
  execFileSync("git", ["clone", remote, checkout], { stdio: "ignore" });
  process.chdir(checkout);
  git("config", "user.name", "Test Maintainer");
  git("config", "user.email", "test@example.com");
  git("checkout", "-b", "main");
  writePackage(checkout);
  git("add", ".");
  git("commit", "-m", "previous release");
  git("push", "origin", "main");
  git("checkout", "-b", "dev");
  mkdirSync(".changelog");
  writeFileSync(".changelog/feat-sample.md", "- Add a sample feature.\n");
  writeFileSync("feature.txt", "first feature\n");
  git("add", ".");
  git("commit", "-m", "feat: sample");
  git("push", "origin", "dev");
  let source = git("rev-parse", "HEAD");
  let pr: any;
  let closedPr: any;
  const dispatches: string[] = [];
  const notices: string[] = [];
  let releases: any[] = [];
  const missing = () => Object.assign(new Error("Not found"), { status: 404 });
  const api: any = {
    rest: {
      git: {
        getRef: async ({ ref }: any) => {
          try {
            return { data: { object: { sha: git("--git-dir", remote, "rev-parse", "--verify", `refs/${ref}`) } } };
          } catch { throw missing(); }
        },
      },
      pulls: {
        list: async ({ state }: any) => ({ data: state === 'closed' ? (closedPr ? [closedPr] : []) : (pr ? [pr] : []) }),
        create: async (input: any) => {
          pr = { ...input, number: 1, html_url: "https://github.com/example/project/pull/1" };
          return { data: pr };
        },
        update: async (input: any) => { Object.assign(pr, input); return { data: pr }; },
        get: async () => ({ data: pr }),
      },
      repos: { listReleases: () => {} },
      actions: {
        listWorkflowRuns: async () => ({ data: { workflow_runs: [run()] } }),
        createWorkflowDispatch: async ({ workflow_id }: any) => { dispatches.push(workflow_id); },
      },
    },
    paginate: async () => releases,
  };
  function run() {
    return { conclusion: "success", event: "push", head_branch: "dev", head_sha: source, head_repository: { full_name: "example/project" } };
  }
  function update(overrides = {}) {
    return updateReleasePr({ github: api, core: { notice: (message: string) => notices.push(message) },
      context: { repo: { owner: "example", repo: "project" }, payload: { workflow_run: { ...run(), ...overrides } } } });
  }
  return { remote, checkout, update, dispatches, notices, get pr() { return pr; },
    mergePr: () => {
      closedPr = { ...pr, merged_at: '2026-11-04T00:00:00Z', head: { sha: git('--git-dir', remote, 'rev-parse', 'refs/heads/release/next') } };
      pr = undefined;
    },
    setSource: (sha: string) => { source = sha; }, setReleases: (value: any[]) => { releases = value; } };
}

describe("release candidate lifecycle", () => {
  test("prepares notes and version, advances only to newer passing commits, and freezes on ready", async () => {
    const f = fixture();
    const firstSource = git("rev-parse", "HEAD");
    await f.update();
    expect(f.pr.draft).toBe(true);
    expect(f.pr.title).toBe("Release v0.19.0");
    expect(f.pr.body).toContain("Add a sample feature.");
    expect(f.dispatches).toEqual(["fast-ci.yml"]);
    git("fetch", "origin", "release/next");
    const first = git("rev-parse", "FETCH_HEAD");
    expect(git("merge-base", firstSource, first)).toBe(firstSource);
    expect(git("show", `${first}:Cargo.lock`)).toContain('version = "0.19.0"');
    expect(git("ls-tree", "-r", "--name-only", first)).not.toContain(".changelog/feat-sample.md");

    writeFileSync("feature.txt", "second feature\n");
    git("add", "."); git("commit", "-m", "feat: second"); git("push", "origin", "dev");
    const secondSource = git("rev-parse", "HEAD");
    f.setSource(secondSource);
    await f.update();
    git("fetch", "origin", "release/next");
    const second = git("rev-parse", "FETCH_HEAD");
    expect(second).not.toBe(first);
    expect(git("show", `${second}:feature.txt`)).toBe("second feature");
    f.setSource(firstSource);
    await f.update(); // A slower, older run finishes after the new one.
    expect(f.notices.at(-1)).toContain("already includes");
    expect(git("--git-dir", f.remote, "rev-parse", "refs/heads/release/next")).toBe(second);

    f.pr.draft = false;
    await f.update();
    expect(f.notices.at(-1)).toContain("frozen");
    expect(git("--git-dir", f.remote, "rev-parse", "refs/heads/release/next")).toBe(second);
  });

  test("preserves main-only work in the next release cycle", async () => {
    const f = fixture();
    await f.update();
    git("fetch", "origin", "release/next");
    const candidate = git("rev-parse", "FETCH_HEAD");
    git("push", "origin", `${candidate}:refs/heads/main`);
    writeFileSync("next.txt", "next release\n");
    git("add", "."); git("commit", "-m", "feat: next release"); git("push", "origin", "dev");
    f.setSource(git("rev-parse", "HEAD"));
    f.mergePr(); // The merged candidate branch can remain on the remote.
    await f.update();
    git("fetch", "origin", "release/next");
    expect(f.pr.title).toBe("Release v0.20.0");
    expect(git("merge-base", candidate, "FETCH_HEAD")).toBe(candidate);
    expect(git("show", "FETCH_HEAD:CHANGELOG.md")).toContain("Add a sample feature.");
  });

  test("does not overwrite manual candidate commits", async () => {
    const f = fixture();
    await f.update();
    git("fetch", "origin", "release/next");
    git("checkout", "--detach", "FETCH_HEAD");
    writeFileSync("manual.txt", "review fix\n");
    git("add", "."); git("commit", "-m", "fix: review feedback");
    const manual = git("rev-parse", "HEAD");
    git("push", "origin", `${manual}:refs/heads/release/next`);
    await f.update();
    expect(f.notices.at(-1)).toContain("manual commits");
    expect(git("--git-dir", f.remote, "rev-parse", "refs/heads/release/next")).toBe(manual);
  });

  test("rejects untrusted and unsuccessful runs and waits for unpublished releases", async () => {
    const f = fixture();
    await expect(f.update({ conclusion: "failure" })).rejects.toThrow("No successful");
    await expect(f.update({ event: "pull_request" })).rejects.toThrow("No successful");
    await expect(f.update({ head_repository: { full_name: "fork/project" } })).rejects.toThrow("No successful");
    f.setReleases([{ draft: true }]);
    await f.update();
    expect(f.pr).toBeUndefined();
    expect(f.notices.at(-1)).toContain("existing draft release");
  });

  test("merge conflicts leave main and the candidate untouched", async () => {
    const f = fixture();
    await f.update();
    const before = git("--git-dir", f.remote, "rev-parse", "refs/heads/release/next");
    git("checkout", "main");
    writeFileSync("feature.txt", "different main work\n");
    git("add", "."); git("commit", "-m", "fix: main work"); git("push", "origin", "main");
    const main = git("rev-parse", "HEAD");
    git("checkout", "dev");
    writeFileSync("feature.txt", "conflicting dev work\n");
    git("add", "."); git("commit", "-m", "feat: conflicting work"); git("push", "origin", "dev");
    f.setSource(git("rev-parse", "HEAD"));
    await expect(f.update()).rejects.toThrow();
    expect(git("--git-dir", f.remote, "rev-parse", "refs/heads/main")).toBe(main);
    expect(git("--git-dir", f.remote, "rev-parse", "refs/heads/release/next")).toBe(before);
  });
});

test("uses a deliberate higher version or follows the pre-1.0 release policy without touching other packages", async () => {
  expect(nextVersion("0.19.0", "0.18.0")).toBe("0.19.0");
  expect(nextVersion("0.18.1", "0.18.0")).toBe("0.18.1");
  expect(nextVersion("1.2.0", "1.2.0")).toBe("1.2.1");
  expect(nextVersion("0.18.0", "0.19.0")).toBe("0.20.0");
  expect(packageVersion('version = "0.19.0-beta.1"')).toBe("0.19.0-beta.1");
  const root = temp(); writePackage(root);
  await prepareRelease(root, "0.18.0");
  expect(readFileSync(join(root, "Cargo.lock"), "utf8")).toBe('version = 4\n\n[[package]]\nname = "waku"\nversion = "0.19.0"\n');
});
