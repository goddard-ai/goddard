---
name: pandark-ui
description: Implement, refactor, and review product UI in existing apps that use Panda CSS and Ark UI when the task is about composing screens, forms, lists, tables, empty states, or action areas in a compact, neutral, accessible way within an established design system. Use when Codex must choose semantic structure, reuse local components or recipes, pick Ark UI primitives for interaction, and apply minimal Panda CSS semantic-token styling. Do not use for Panda CSS setup, codegen, config, static-extraction, or token-plumbing work; for Ark UI component API troubleshooting or library-specific behavior in isolation; or when the task is primarily about introducing either library.
---

# pandark-ui

Use this skill for product UI work in apps that use Panda CSS and Ark UI. Prefer under-designed over over-designed. Produce correct, accessible, compact UI that fits the existing system and is easy for humans to reshape.

Your job is not to make the UI look finished. Your job is to make it structurally correct, visually restrained, and easy to edit later. When unsure, delete styling instead of adding more.

## Start Here

- Treat Panda CSS and Ark UI as established parts of the project.
- Inspect nearby screens, components, recipes, and layout patterns before editing.
- Preserve the current design system, layout conventions, and component boundaries.
- Prefer existing project components, recipes, slot recipes, and layout helpers when they already encode the system.
- Use Ark for interactive primitives and Panda for styling. Do not introduce parallel widget, accessibility, or theming systems.
- Reach for raw Ark parts only when no suitable local wrapper or project primitive already exists.
- Use the separate `panda-css` or `ark-ui` skill when the task is library-level setup, troubleshooting, or API-specific investigation rather than product-surface composition.

## Priority Order

1. Correct interaction primitives
2. Correct semantics and accessibility
3. Clear information hierarchy
4. Compact layout
5. Visual restraint
6. Easy future reshaping

## Workflow

1. Inspect the surrounding surface first.
   - Look at nearby screens and the app's existing components, recipes, spacing patterns, and state treatments.
   - Match the local level of density and restraint before inventing anything new.
2. Reuse local primitives before dropping lower.
   - Prefer project-level wrappers, recipes, and slots when they already express the design system.
   - Use raw Ark primitives when interaction behavior is needed and the project does not already wrap the relevant control.
3. Choose structure before styling.
   - Pick the semantic page and content pattern first: list, metadata, table, form, navigation, or action row.
   - Make the screen understandable in normal document flow before adding layout helpers.
4. Add the minimum Panda styling required.
   - Use semantic tokens, compact spacing, and small layout primitives.
   - Add only the styling needed for clarity, grouping, and state visibility.
5. Validate the UI at the product-surface level.
   - Check keyboard and focus behavior, labels, helper text, error text, disabled and invalid states, and obvious state transitions.
   - Check whether the UI would still make sense if most local styling disappeared.

## Structure Rules

- Prefer existing project wrappers around Ark UI before composing raw Ark parts yourself.
- Use Ark UI whenever it provides the right interactive primitive and no suitable local abstraction already exists.
- Do not rebuild dialogs, menus, popovers, tabs, comboboxes, switches, radios, selects, accordions, or tooltips with raw `div`s, custom ARIA, or ad hoc behavior when Ark already covers them.
- Use semantic HTML for page and content structure. Prefer `main`, `header`, `nav`, `section`, `article`, `aside`, `footer`, `form`, `fieldset`, `legend`, `label`, `ul`, `ol`, `li`, `table`, `thead`, `tbody`, `th`, `tr`, `td`, `dl`, `dt`, and `dd`. Use `div` and `span` only when no better structural element exists.
- Prefer native content patterns over custom layouts.
  - Lists of items: `ul` or `ol`
  - Label/value metadata: `dl`
  - Tabular data: `table`
  - Related controls: `fieldset` and `legend`
  - Page actions: compact button row
  - Navigation: `nav`
- Make hierarchy come from structure first. The UI should still make sense if most local styling disappears.
- Keep layout primitive and local. Start with normal document flow, then add only the smallest layout primitive that solves the problem: stack, inline row, wrap, or simple grid.
- Avoid deep nesting and layout-heavy wrapper trees.

## Styling Rules

- Use Panda semantic tokens only.
- Never use hex, `rgb()`, `hsl()`, named colors, raw palette values, or bespoke token names.
- If the correct semantic token is unclear, use no color.
- Default to very compact spacing. Prefer the smallest spacing that remains readable and clickable.
- Spend minimal effort on typography. Use correct heading and text elements, but do not add decorative type styling or elaborate type scales.
- Add the minimum styles needed. Local styles should mostly handle layout, spacing, alignment, overflow, state visibility, and width constraints.
- Keep containers opt-in. Do not turn every section into a card. Prefer plain document flow first, then add a border or subtle surface only when it clearly improves grouping or state.
- Use one emphasis mechanism at a time: border, subtle surface, or semantic color.
- Do not stack border, tinted background, shadow, and accent color unless explicitly required.
- Do not decorate by default. Avoid gradients, ornamental shadows, glassmorphism, decorative iconography, or heavy radius unless the design system already establishes them.
- Default to left-aligned, dense UI. Avoid centered layouts unless the screen truly calls for one.
- Make every styling choice easy to delete later.

## Content and Interaction Rules

- Keep states obvious but restrained. Hover, focus, selected, open, disabled, invalid, and destructive states should be clear but not loud.
- Prefer short action rows. Keep button groups compact and avoid large toolbars unless the task truly needs one.
- Use icons sparingly. Add an icon only when it carries meaning, saves real space, or is already part of a known control pattern.
- Keep empty states boring. Usually use a heading, one sentence, and one obvious next action.
- Prefer boring defaults for data display. Tables should look like tables. Metadata should look like metadata. Forms should look like forms.
- Avoid surface stacking. In most cases, only one visual grouping device should be present at once.

## Final Check

- Reuse project components, recipes, and slot recipes before dropping to raw Ark or one-off styling.
- Use Ark for the interactive parts.
- Keep the page structure semantic and simple.
- Avoid raw or bespoke colors.
- Keep spacing compact.
- Avoid turning everything into cards.
- Remove decorative styling that adds no clarity.
- Check whether the UI still makes sense if most local styling disappears.

## Target Output

The result should feel correct, compact, neutral, slightly plain, and easy for a designer or engineer to reshape quickly.
