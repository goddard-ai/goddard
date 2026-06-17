# Overview Docs

Read this ruleset when creating, reorganizing, or editing an `overview/` documentation folder.

- Treat `overview/` as a conceptual contract for a subsystem.
- Write for readers who may land on any page without knowing the surrounding package or daemon model.
- Start each overview page with one short Markdown blockquote, not a quoted sentence, that orients the reader to the concept and why the page exists.
- Keep the opening blockquote self-contained enough to give a reader their bearings in one or two sentences.
- Organize pages around user-findable concepts, states, ownership boundaries, workflows, guardrails, and recovery paths.
- Use subfolders when they make scanning easier, especially for groups such as concepts, sessions, attention, collaboration, automation, and development.
- Prefer one page per concept a user, agent, or reviewer might reasonably search for directly.
- Keep directory `README.md` files as scan-first maps that explain what each linked page answers or changes.
- Keep pages conceptual and durable: describe what is supported, what changes, what never changes, and what guardrails apply.
- Do not write implementation walkthroughs, source tours, private schema notes, helper descriptions, exact diagnostic wording, storage mechanics, or changelog entries.
- Keep terminology consistent with the nearest glossary and sibling overview pages.
