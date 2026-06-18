# Overview Docs

Read this ruleset when creating, reorganizing, or editing an `overview/` documentation folder.

- Treat `overview/` as public conceptual product documentation for a supported capability area.
- Write for readers who may not know the repository, package layout, implementation history, or local jargon.
- Technical terms, config files, commands, and identifiers are allowed when they explain supported behavior or capabilities; define them in context and keep the page conceptual.
- Start each overview page with one short Markdown blockquote, not a quoted sentence, that explains what the concept is and why it matters.
- Organize pages around user-findable concepts, states, ownership boundaries, workflows, guardrails, recovery paths, and decisions.
- Prefer one page per concept a user, agent, or reviewer might reasonably search for directly.
- Keep directory `README.md` files as public scan-first maps grouped by user task or concept.
- Describe supported behavior, visible outcomes, ownership boundaries, guardrails, and recovery paths.
- Do not write implementation walkthroughs, source tours, private source paths, helper names, private schema notes, exact diagnostic wording, internal execution order, TODOs, changelog entries, or unreleased plans.
- Keep terminology consistent with the nearest glossary and sibling overview pages.
- Run `pnpm run check:overview` after editing overview docs; it checks page shape, opening blockquotes, local links, directory README files, and hard-banned internal markers.
- Agents still must verify that overview prose is public-facing, conceptually useful, terminology-aligned, and not divergent from `spec/`.
