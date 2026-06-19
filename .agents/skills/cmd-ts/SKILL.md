---
name: cmd-ts
description: Build, modify, or debug TypeScript command-line interfaces that use the `cmd-ts` package. Use when Codex needs to add typed CLI parsing, commands, subcommands, options, flags, positional arguments, custom `Type` decoders, default/env-backed inputs, help output, or testable parsing behavior with cmd-ts.
---

# cmd-ts

Use `cmd-ts` as a typed adapter between shell arguments and application code. Keep command handlers focused on business logic and push validation, decoding, defaults, env fallbacks, and help metadata into parsers and `Type` definitions.

## Workflow

1. Inspect the existing package manager, TypeScript module style, test runner, and current CLI entrypoint before adding code.
2. Install or reuse `cmd-ts` only when it is already appropriate for the project. Avoid replacing another CLI framework unless requested.
3. Model the CLI from the outside in: command name, subcommands, positional arguments, options, flags, defaults, env variables, examples, and help text.
4. Use built-in parsers first: `command`, `subcommands`, `option`, `multioption`, `flag`, `multiflag`, `positional`, `restPositionals`, `binary`, and `run`.
5. Use `Type` or `extendType` for runtime validation and typed handler values instead of validating strings inside the handler.
6. Prefer `runSafely`, `dryRun`, or `parse` in tests and integration code that must not call `process.exit`.

Read [references/api.md](references/api.md) before implementing nontrivial cmd-ts behavior, custom types, subcommands, defaults, env fallbacks, or tests.

## Implementation Patterns

Create a simple command:

```ts
import { command, positional, run, string } from "cmd-ts";

const app = command({
  name: "greet",
  args: {
    name: positional({ type: string, displayName: "name" }),
  },
  handler: ({ name }) => {
    console.log(`Hello, ${name}`);
  },
});

run(app, process.argv.slice(2));
```

Wrap executable entrypoints with `binary` when passing full `process.argv`:

```ts
import { binary, command, run } from "cmd-ts";

run(binary(command({ name: "tool", args: {}, handler: () => undefined })), process.argv);
```

Make parser configuration expressive:

- Give every option and flag a stable `long` name; add `short` only for common interactive paths.
- Set `displayName` on positional arguments and values that need clear help output.
- Use `description` for user-facing help text, not implementation notes.
- Use `defaultValue` for fast synchronous defaults that can be shown in help.
- Use `onMissing` for dynamic fallbacks such as prompts, config reads, or API calls.
- Use `env` on options and flags when environment variables are part of the interface.

## Custom Types

Define a `Type<Input, Output>` when a CLI value needs validation or conversion. The `from` method may be async and should throw clear user-facing errors.

```ts
import { extendType, string, type Type } from "cmd-ts";
import { access } from "node:fs/promises";

const ExistingFile: Type<string, string> = extendType(string, {
  displayName: "file",
  description: "Path to an existing file",
  async from(path) {
    await access(path);
    return path;
  },
});
```

For filesystem and URL validation, check the battery packs before writing a custom type:

```ts
import { File } from "cmd-ts/batteries/fs";
import { HttpUrl } from "cmd-ts/batteries/url";
```

## Verification

Test parser behavior separately from command side effects where possible:

```ts
import { dryRun, parse, runSafely } from "cmd-ts";
```

Use `dryRun` or `runSafely` to assert success and failure without exiting the test process. Use `parse` when the handler must not run.
