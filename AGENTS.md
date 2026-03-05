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
# Sync all repos listed in synced_docs.json
pnpm sync-docs

# Sync a single repo ad-hoc (entire repo — all *.md files)
pnpm sync-docs <git-url>

# Sync a single repo ad-hoc (sparse checkout — specific subfolder only)
pnpm sync-docs <git-url> <subfolder>
```

**Examples**

```bash
# Sync everything in the manifest
pnpm sync-docs

# Drizzle ORM — grab everything
pnpm sync-docs https://github.com/drizzle-team/drizzle-orm

# Cloudflare Workers SDK — only the docs/ subtree
pnpm sync-docs https://github.com/cloudflare/workers-sdk docs
```

### Pinning repos for all agents

Add entries to `synced_docs.json` at the repo root to make a library always
available to every agent without needing to pass arguments:

```json
[
  { "url": "https://github.com/drizzle-team/drizzle-orm" },
  { "url": "https://github.com/cloudflare/workers-sdk", "subfolder": "docs" },
  { "url": "https://github.com/drizzle-team/drizzle-orm", "name": "drizzle" }
]
```

The optional **`name`** field overrides the folder name under `docs/third_party/`.
Without it the folder name is derived from the last segment of the repo URL.

Running `pnpm sync-docs` (no arguments) will iterate the list and sync each entry.

### Referencing dependency versions in synced_docs.json

Any string field in a `synced_docs.json` entry can use `{{…}}` tokens to pull
version numbers directly from a `package.json`, keeping doc refs in sync with
the actual installed dependency:

```
{{<field>.<package-name>}}                 # root package.json
{{<field>.<package-name>:<package-dir>}}   # <package-dir>/package.json
```

- **`field`** is a top-level key in the target `package.json`
  (e.g. `dependencies`, `devDependencies`, `peerDependencies`).
- **`package-name`** is the dependency key within that field.
- **`package-dir`** is an optional path to a workspace package
  (relative to the repo root).

Semver range specifiers (`^`, `~`, `>=`, `>`, `<=`, `<`) are automatically
stripped so the resolved value is a bare version string.

**Examples**

```json
[
  {
    "url": "https://github.com/drizzle-team/drizzle-orm",
    "subfolder": "{{dependencies.drizzle-orm}}"
  },
  {
    "url": "https://github.com/microsoft/TypeScript",
    "subfolder": "v{{devDependencies.typescript}}"
  },
  {
    "url": "https://github.com/tursodatabase/libsql",
    "subfolder": "{{dependencies.@libsql/client:backend}}"
  }
]
```

### Where docs land

Clones are written to `docs/third_party/<repo-name>/`, where `<repo-name>` is the
last segment of the repo URL or the `name` field if one is set in `synced_docs.json`.
After cloning, every file that is not a `*.md` or `*.mdx` file is deleted; only Markdown and
the `.git/` bookkeeping folder survive. This keeps the directory lightweight and
diff-free.

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
