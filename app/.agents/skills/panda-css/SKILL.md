---
name: panda-css
description: Build, refactor, debug, and review web UI code that uses Panda CSS for build-time styling via `css`, `styled`, JSX style props, patterns, recipes, slot recipes, theme tokens, presets, utilities, or Panda CLI/PostCSS integration. Use when Codex needs to implement or troubleshoot Panda CSS setup, static extraction, `panda.config.*`, `styled-system` output, dynamic styling workarounds, component-library packaging, or framework-specific Panda integration.
---

# panda-css

Use Panda as a build-time styling system. Preserve static analyzability, treat `styled-system` output as generated unless the repo clearly versions it, and fix setup or extraction issues before rewriting component styles.

## Start Here

- Inspect the current integration before editing. Check `panda.config.*`, the project scripts, the generated outdir, and where the Panda CSS output is imported.
- Load [overview-and-faq.md](./references/overview-and-faq.md) for Panda's execution model, browser support, high-level tradeoffs, and common FAQ answers.
- Load [installation.md](./references/installation.md) for framework-specific setup, PostCSS versus CLI integration, and setup troubleshooting.
- Load [styling-foundations.md](./references/styling-foundations.md) for cascade layers, conditional styles, `extend`, global styles, integration hooks, JSX style context, and style merging.
- Load [patterns-recipes-and-responsive.md](./references/patterns-recipes-and-responsive.md) for patterns, `cva`, config recipes, slot recipes, responsive variants, and recipe tracking rules.
- Load [jsx-style-props-and-writing-styles.md](./references/jsx-style-props-and-writing-styles.md) for JSX style props, `styled-system` authoring, template literals, virtual colors, and nested style composition.
- Load [theming-and-tokens.md](./references/theming-and-tokens.md) for tokens, semantic tokens, layer styles, text styles, animation styles, Panda Studio, and runtime token access.
- Load [utilities.md](./references/utilities.md) for utility-prop behavior and property-level authoring.
- Load [customization.md](./references/customization.md) for custom conditions, presets, config helpers, theme extensions, custom utilities, and deprecations.
- Load [guides-and-troubleshooting.md](./references/guides-and-troubleshooting.md) for dynamic styling, component-library packaging, debugging, custom fonts, multi-theme tokens, and static CSS generation.
- Load [migration.md](./references/migration.md) when porting code from Stitches, Styled Components, or Theme UI.
- Load [cli-and-config.md](./references/cli-and-config.md) for CLI commands and `panda.config.*` options.
- Search `./references/guides-and-troubleshooting.md` with `rg -n "staticCss|token\\.var|css\\.raw|runtime conditions|dynamic styling"`.
- Search `./references/patterns-recipes-and-responsive.md` with `rg -n "cva|defineRecipe|slot recipe|splitVariantProps|jsx:"`.
- Search `./references/cli-and-config.md` with `rg -n "defineConfig|importMap|outdir|include|exclude|presets|jsxFramework"`.

## Workflow

- Confirm whether the project uses Panda through PostCSS, `panda --watch`, or explicit `panda codegen` or `panda cssgen` commands before changing build scripts.
- Preserve static extraction. Inline statically knowable values, avoid renaming style or variant props in ways Panda cannot track, and prefer recipes, CSS custom properties, semantic tokens, or `data-*` and condition-based styling over opaque runtime values.
- Choose the smallest authoring primitive that matches the job. Use `css` and utilities for one-off styles, patterns for layout primitives, `cva` for colocated variants, and config recipes for design-system components or responsive recipe variants.
- Prefer tokens and semantic tokens over raw literals when the codebase already has a theme.
- Keep `importMap`, `include`, `exclude`, and CSS imports aligned with the actual module paths. Missing styles are often configuration or layer issues rather than component bugs.
- Run the relevant Panda command after config or recipe changes. In most repos that means `panda codegen`, `panda --watch`, or the project build that triggers PostCSS.

## Decision Rules

- Read [installation.md](./references/installation.md) before touching framework-specific setup, CSS entrypoints, or package scripts.
- Read [styling-foundations.md](./references/styling-foundations.md) before changing cascade layers, conditional styles, `css` composition, global styles, or JSX style context helpers.
- Read [patterns-recipes-and-responsive.md](./references/patterns-recipes-and-responsive.md) before choosing between `cva`, config recipes, slot recipes, or responsive variants.
- Read [jsx-style-props-and-writing-styles.md](./references/jsx-style-props-and-writing-styles.md) before editing JSX style props, `styled`, template literals, or nested authored styles.
- Read [theming-and-tokens.md](./references/theming-and-tokens.md) before extending tokens, semantic tokens, text styles, layer styles, or animation styles.
- Read [guides-and-troubleshooting.md](./references/guides-and-troubleshooting.md) first when styles are missing, HMR or absolute imports break extraction, runtime values are involved, or the task is packaging a component library.
- Read [cli-and-config.md](./references/cli-and-config.md) before editing `panda.config.*`, changing CLI flags, or reasoning about what Panda will emit.
