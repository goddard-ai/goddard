---
name: lingui
description: Add, configure, or debug Lingui internationalization in JavaScript or TypeScript apps using @lingui/core macros, Vite catalog loading, message extraction config, and dynamic locale activation. Use for non-JSX Lingui macro API work; avoid renderer-specific APIs unless the user explicitly asks.
---

# Lingui

Use Lingui as a compile-time macro layer over `@lingui/core`: write normal JavaScript/TypeScript strings, let macros extract ICU MessageFormat catalogs, then load the active locale into the `i18n` singleton at runtime.

## Scope

- Prefer Core Macros from `@lingui/core/macro` (`t`, `plural`, `select`, `selectOrdinal`, `msg`/`defineMessage`, `ph`).
- Do not introduce JSX or renderer-specific Lingui APIs unless the user explicitly requests them.
- Keep localizable text inside functions or runtime code paths. Core macros must not be used at module top level.

## Setup checklist

1. Confirm Lingui packages, package manager, build tool, and existing catalog layout.
2. Configure `lingui.config.ts` or `lingui.config.js` with `locales`, `sourceLocale`, `catalogs`, and `format`.
3. For Vite, add `@lingui/vite-plugin` and `lingui()` to `vite.config.ts` so catalogs compile on demand.
4. Initialize `i18n` by loading messages and activating a locale before calling translated code.
5. Use Lingui CLI extraction/compile scripts if the project does not already have them.

```ts
// lingui.config.ts
import { defineConfig } from "@lingui/cli";

export default defineConfig({
  sourceLocale: "en",
  locales: ["en", "fr"],
  catalogs: [
    {
      path: "<rootDir>/locales/{locale}/messages",
      include: ["<rootDir>/src"],
      exclude: ["**/node_modules/**"],
    },
  ],
});
```

```ts
// vite.config.ts
import { defineConfig } from "vite";
import { lingui } from "@lingui/vite-plugin";

export default defineConfig({
  plugins: [lingui()],
});
```

## Runtime loading

Load the active locale catalog, including the source/default locale. Lingui production builds can drop default messages, so do not assume English text remains available without a loaded catalog.

```ts
// src/i18n.ts
import { i18n } from "@lingui/core";

export const locales = {
  en: "English",
  fr: "Français",
};

export const defaultLocale = "en";

export async function activateLocale(locale: keyof typeof locales) {
  const { messages } = await import(`../locales/${locale}/messages.po`);
  i18n.load(locale, messages);
  i18n.activate(locale);
}
```

With Vite, the dynamic import must include the file extension. If using a non-`.po` catalog format, add the `?lingui` query:

```ts
const { messages } = await import(`../locales/${locale}/messages.json?lingui`);
```

## Core macro patterns

Use `t` for immediate strings:

```ts
import { t } from "@lingui/core/macro";

export function savedMessage(name: string) {
  return t`Attachment ${name} saved`;
}
```

Use descriptor form when translators need IDs, comments, or context:

```ts
import { t } from "@lingui/core/macro";

const label = t({
  id: "settings.save",
  comment: "Button label for saving settings",
  message: "Save",
});
```

Use `plural`, `select`, and `selectOrdinal` for ICU branching. Include `other` forms and choose plural categories for the source locale.

```ts
import { plural, select } from "@lingui/core/macro";

const inbox = plural(count, {
  one: "# unread message",
  other: "# unread messages",
});

const pronoun = select(gender, {
  female: "she",
  male: "he",
  other: "they",
});
```

Use `msg`/`defineMessage` for lazy translations that should be translated later with `i18n.t`:

```ts
import { i18n } from "@lingui/core";
import { msg } from "@lingui/core/macro";

const emptyState = msg`No results`;

export function getEmptyState() {
  return i18n.t(emptyState);
}
```

Use `ph` to give meaningful placeholder names to expressions that would otherwise become numeric placeholders:

```ts
import { ph, t } from "@lingui/core/macro";

t`Welcome, ${ph({ userName: user.profile.displayName })}`;
```

## Macro and config notes

- By default macros import `i18n` from `@lingui/core`. If using a custom `setupI18n` instance, set `runtimeConfigModule`, for example `runtimeConfigModule: ["./src/i18n", "i18n"]`.
- `catalogs[].path` must omit the catalog file extension; the configured `format` controls the extension.
- `catalogs[].include` and `exclude` drive extraction. Keep includes narrow enough to avoid generated files and dependencies.
- Use `fallbackLocales` when missing translations should fall back to another locale; use `sourceLocale` to identify the language of source messages.
- Use `compileNamespace: "es"` or `"ts"` when generated catalogs need ESM-style named `messages` exports outside Vite's `.po` dynamic import flow.
- `lingui-set` and `lingui-reset` comments can apply `context`, `comment`, or `idPrefix` to following macros in the same file; explicit macro values override directives.

## Verification

- Run extraction after adding or changing macros and review catalog diffs for clear messages, placeholder names, and translator comments.
- Run compilation or the Vite build to verify catalog imports resolve.
- Exercise startup and locale switching paths to ensure `i18n.load` happens before localized strings are read.
