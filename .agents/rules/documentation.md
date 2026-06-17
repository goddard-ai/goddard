# Documentation

Read this ruleset when changing terminology, concepts, package boundaries, README or glossary docs, user-visible features, or undocumented feature tracking.

- Whenever implementing a new user-visible feature, add one or more entries for it to `.git/undocumented-features.yaml`.
- Read the nearest `glossary.md` before changing domain behavior, naming, states, roles, identifiers, or ownership rules in a package that has one.
- Read a sibling concept doc before editing its adjacent implementation file or another file that depends on the same local model.
- Update the relevant concept doc in the same change when you add, remove, rename, or change the meaning of a domain concept.
- Add or expand the relevant package glossary or sibling concept doc when recurring local abstractions are slow to recover from code comments, types, or signatures alone.
- Keep concept docs concise and focused on domain-level what and why.
- Do not turn concept docs into implementation walkthroughs, code tours, API references, or change logs.
- Put package boundaries and integration surfaces in the nearest `README.md`.
- Put domain terminology in the nearest `glossary.md`.
- Do not use `AGENTS.md` as a spec, plan, backlog, or changelog.
- When guidance outgrows an `AGENTS.md`, move it to a better-scoped document and leave a short pointer.
