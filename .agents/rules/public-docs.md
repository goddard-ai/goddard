# Public Docs

Read this ruleset when creating, reorganizing, or editing a public `docs/` documentation folder.

- Treat `docs/` as public conceptual product documentation for a supported capability area.
- Write for readers who may not know the repository, package layout, implementation history, or local jargon.
- Technical terms, config files, commands, and identifiers are allowed when they explain supported behavior or capabilities; define them in context and keep the page conceptual.
- Start each public docs page with one short plain opening paragraph that explains what the concept is and why it matters.
- Use `##` headings for page sections; do not use bold bulleted items as section headings.
- Use bulleted lists for section content, with nested bullets encouraged when they make states, decisions, or consequences easier to scan.
- Organize pages around user-findable concepts, states, ownership boundaries, workflows, guardrails, recovery paths, and decisions.
- Prefer one page per concept a user, agent, or reviewer might reasonably search for directly.
- Keep directory `README.md` files as public scan-first maps grouped by user task or concept.
- Describe current supported behavior, visible outcomes, ownership boundaries, guardrails, and recovery paths.
- `docs/` may diverge from `spec/` when the spec describes an ideal or future shape; prefer the current product shape in public docs.
- Do not write implementation walkthroughs, source tours, private source paths, helper names, private schema notes, exact diagnostic wording, internal execution order, TODOs, changelog entries, or unreleased plans.
- Keep terminology consistent with the nearest glossary and sibling public docs pages.
- Run `pnpm run check:docs` after editing public docs; it checks page shape, opening paragraphs, local links, directory README files, and hard-banned internal markers.
- Agents still must verify that public docs prose is public-facing, conceptually useful, terminology-aligned, and accurate to current supported behavior.
