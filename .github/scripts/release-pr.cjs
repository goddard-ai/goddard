const { execFileSync } = require('node:child_process');
const { mkdtempSync, rmSync, readFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { join } = require('node:path');

const branch = 'release/next';
const marker = '<!-- goddard-release-pr -->';
const botEmail = '41898282+github-actions[bot]@users.noreply.github.com';

function git(args, cwd = process.cwd()) {
  return execFileSync('git', ['-c', 'user.name=github-actions[bot]', '-c', `user.email=${botEmail}`, ...args], {
    cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
}

async function openCandidate(github, repo) {
  const { data } = await github.rest.pulls.list({ ...repo, state: 'open', base: 'main', head: `${repo.owner}:${branch}` });
  return data[0];
}

function editable(pr) {
  return !pr || (pr.draft && pr.body?.includes(marker));
}

async function dispatchChecks(github, repo, pr) {
  await github.rest.actions.createWorkflowDispatch({ ...repo, workflow_id: 'fast-ci.yml', ref: branch });
  const current = (await github.rest.pulls.get({ ...repo, pull_number: pr.number })).data;
  if (!current.draft) {
    await github.rest.actions.createWorkflowDispatch({ ...repo, workflow_id: 'test.yml', ref: branch });
  }
}

module.exports = async function updateReleasePr({ github, context, core }) {
  const repo = context.repo;
  const eventRun = context.payload.workflow_run;
  // Manual retries select the latest successful push run; automatic events
  // must come from this repository's dev branch, never a fork or PR run.
  const run = eventRun ?? (await github.rest.actions.listWorkflowRuns({
    ...repo, workflow_id: 'fast-ci.yml', branch: 'dev', event: 'push', status: 'success', per_page: 1,
  })).data.workflow_runs[0];
  if (!run || run.conclusion !== 'success' || run.event !== 'push' || run.head_branch !== 'dev' ||
      run.head_repository?.full_name !== `${repo.owner}/${repo.repo}`) {
    throw new Error('No successful Fast CI push run on this repository’s dev branch');
  }
  const source = run.head_sha;
  if (!/^[a-f0-9]{40}$/.test(source)) throw new Error('Invalid source SHA');
  let pr = await openCandidate(github, repo);
  if (!editable(pr)) return core.notice('Release candidate is frozen or is not owned by this automation');
  const releases = await github.paginate(github.rest.repos.listReleases, { ...repo, per_page: 100 });
  if (releases.some((release) => release.draft)) return core.notice('Publish or remove the existing draft release before preparing another');

  git(['fetch', 'origin', source]);
  const dev = (await github.rest.git.getRef({ ...repo, ref: 'heads/dev' })).data.object.sha;
  git(['fetch', 'origin', dev]);
  if (git(['merge-base', source, dev]) !== source) return core.notice('Passing commit is no longer on dev');
  const main = (await github.rest.git.getRef({ ...repo, ref: 'heads/main' })).data.object.sha;
  git(['fetch', 'origin', main]);
  if (git(['merge-base', main, source]) === source) return core.notice('main already contains this passing commit');
  let previous;
  try {
    previous = (await github.rest.git.getRef({ ...repo, ref: `heads/${branch}` })).data.object.sha;
  } catch (error) {
    if (error.status !== 404) throw error;
  }
  if (previous) {
    git(['fetch', 'origin', previous]);
    if (!pr) {
      const closed = (await github.rest.pulls.list({ ...repo, state: 'closed', base: 'main', head: `${repo.owner}:${branch}`, sort: 'updated', direction: 'desc', per_page: 1 })).data[0];
      if (closed && (!closed.merged_at || closed.head.sha !== previous || !closed.body?.includes(marker))) {
        throw new Error('release/next has unmerged work without an open PR; inspect it before reusing it');
      }
    }
    const message = git(['show', '-s', '--format=%B', previous]);
    const author = git(['show', '-s', '--format=%ae', previous]);
    if (author !== botEmail || !/^Release-Source: [a-f0-9]{40}$/m.test(message)) {
      return core.notice('Candidate has manual commits; refusing to overwrite them');
    }
    if (pr && git(['merge-base', source, previous]) === source) {
      // Also recovers a dispatch failure after a successful branch/PR write.
      await dispatchChecks(github, repo, pr);
      return core.notice('Candidate already includes this run or a newer passing commit');
    }
  }

  const candidate = mkdtempSync(join(tmpdir(), 'goddard-release-'));
  try {
    git(['worktree', 'add', '--detach', candidate, main]);
    // Real merges retain history and fail visibly on conflicts. Never replace
    // main's tree with a dev snapshot or rewrite the QA-approved dev commits.
    git(['merge', '--no-ff', '--no-commit', source], candidate);
    const unsafeLink = git(['ls-files', '--stage'], candidate).split('\n').some((entry) =>
      /^120000 /.test(entry) && /\t(?:Cargo\.(?:toml|lock)$|CHANGELOG(?:\.mobile)?\.md$|\.changelog(?:\/|$)|assets(?:\/|$))/.test(entry));
    if (unsafeLink) throw new Error('Release inputs and changelog destinations must not contain symlinks');
    const baseVersion = git(['show', `${main}:Cargo.toml`]).match(/^version = "([^"]+)"/m)?.[1];
    if (!baseVersion) throw new Error('main has no package version');
    const output = execFileSync('bun', [join(__dirname, 'prepare-release.ts'), candidate, baseVersion], {
      encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
    });
    const version = output.match(/^RELEASE_VERSION=(\S+)$/m)?.[1];
    if (!version) throw new Error('Release preparation returned no version');
    const existingTag = await github.rest.git.getRef({ ...repo, ref: `tags/v${version}` }).catch((error) => {
      if (error.status === 404) return null;
      throw error;
    });
    if (existingTag || releases.some((release) => release.tag_name === `v${version}`)) {
      throw new Error(`v${version} already exists; choose a new version on dev`);
    }
    git(['add', '--all'], candidate);
    git(['commit', '-m', `chore: prepare release v${version}\n\nRelease-Source: ${source}\nRelease-Base: ${main}`], candidate);
    const head = git(['rev-parse', 'HEAD'], candidate);
    // Recheck after preparation: a human may have marked the PR ready or
    // changed main while the candidate was being built.
    pr = await openCandidate(github, repo);
    if (!editable(pr)) return core.notice('Candidate became ready during preparation; leaving it frozen');
    if ((await github.rest.git.getRef({ ...repo, ref: 'heads/main' })).data.object.sha !== main) {
      throw new Error('main moved during preparation; retry the workflow');
    }
    git(['push', `--force-with-lease=refs/heads/${branch}:${previous ?? ''}`, 'origin', `${head}:refs/heads/${branch}`], candidate);
    const changelog = readFileSync(join(candidate, 'CHANGELOG.md'), 'utf8');
    const body = `${marker}\n\nSource: ${source}\nBase: ${main}\n\n` +
      'This draft follows passing dev checkpoints. Review the entire diff for unfinished work.\n\n' +
      'Mark ready to freeze the candidate and run full CI. Merge with a merge commit; publishing remains manual.\n\n' +
      '## Release notes\n\n' + (changelog.split(`## [${version}]`)[1]?.split(/^## /m)[0]?.trim() || 'No user-facing changelog entries.');
    if (pr) {
      await github.rest.pulls.update({ ...repo, pull_number: pr.number, title: `Release v${version}`, body });
    } else {
      pr = (await github.rest.pulls.create({ ...repo, base: 'main', head: branch, title: `Release v${version}`, body, draft: true })).data;
    }
    // GITHUB_TOKEN pushes do not reliably start downstream workflows. Dispatch
    // explicitly, including full CI if readiness raced with the final push.
    await dispatchChecks(github, repo, pr);
    core.notice(`Prepared ${pr.html_url} at ${head}`);
  } finally {
    git(['worktree', 'remove', '--force', candidate]);
    rmSync(candidate, { recursive: true, force: true });
  }
};

module.exports.editable = editable;
