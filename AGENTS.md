# AGENTS.md — Goddard AI (Root)

## Project overview

This is the root of the Goddard AI monorepo. See each package's own `AGENTS.md` for
package-specific rules. These top-level guidelines apply everywhere.

---

## Fetching third-party documentation on-demand

When you need documentation for an external library, framework, or tool — and that
documentation is not already present locally — use `sync-docs.ts` to pull it in
without committing it to the repository.

### How to run

```bash
# Entire repository (all *.md files)
pnpm tsx sync-docs.ts <git-url>

# Specific subfolder only (sparse checkout — faster, smaller)
pnpm tsx sync-docs.ts <git-url> <subfolder>
```

**Examples**

```bash
# Drizzle ORM — grab everything
pnpm tsx sync-docs.ts https://github.com/drizzle-team/drizzle-orm

# Cloudflare Workers SDK — only the docs/ subtree
pnpm tsx sync-docs.ts https://github.com/cloudflare/workers-sdk docs
```

### Where docs land

Clones are written to `docs/third_party/<repo-name>/`. After cloning, every file
that is not a `*.md` file is deleted; only Markdown and the `.git/` bookkeeping
folder survive. This keeps the directory lightweight and diff-free.

`docs/third_party/` is listed in `.gitignore` — these fetched docs are never
committed.

### Keeping docs fresh

If you run the command for a repo that has already been cloned, the script hard-resets
it to `origin/<current-branch>` before re-running the declutter step. Run it again
any time you suspect the upstream docs have changed.

### When to use this

- You are implementing or debugging against a third-party API and need accurate,
  up-to-date reference docs.
- The library's npm package lacks inline JSDoc and the source is on GitHub.
- You want to cross-check behaviour against the official changelog or migration guide.

Do **not** use this for documentation you can reliably obtain from types alone
(e.g. well-typed npm packages where hovering over a symbol is sufficient).
