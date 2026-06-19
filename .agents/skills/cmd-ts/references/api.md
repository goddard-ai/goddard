# cmd-ts API Notes

Source material: `sitefetch https://cmd-ts.vercel.app/ -o /tmp/cmd-ts-docs.json` and the published `cmd-ts@0.15.0` TypeScript declarations.

## Imports

Most application code imports from `cmd-ts`:

```ts
import {
  array,
  binary,
  boolean,
  command,
  dryRun,
  extendType,
  flag,
  multioption,
  number,
  oneOf,
  option,
  optional,
  parse,
  positional,
  restPositionals,
  run,
  runSafely,
  string,
  subcommands,
  type Type,
  union,
} from "cmd-ts";
```

Battery pack imports:

```ts
import { Directory, ExistingPath, File } from "cmd-ts/batteries/fs";
import { HttpUrl, Url } from "cmd-ts/batteries/url";
```

## Command Structure

`command(config)` composes argument parsers into a runnable CLI:

- `name` is required.
- `args` is an object whose keys become typed handler properties.
- `handler` receives decoded values.
- `version`, `description`, `aliases`, and `examples` are optional help/runtime metadata.

`subcommands({ name, version?, cmds })` routes the first positional token to a child `command` or nested `subcommands` object. Use it for CLIs like `tool create`, `tool delete`, or nested command trees.

`binary(commandOrSubcommands)` ignores the Node executable path and script path. Use `run(binary(app), process.argv)` for bin files; use `run(app, process.argv.slice(2))` when the caller already removed those two entries.

## Parsers

`positional({ type?, displayName?, description? })` reads one non-option argument. The default type is `string`. Provide `displayName` for clear help output.

`restPositionals({ type?, displayName?, description? })` reads all remaining positional arguments and decodes each one. Put it last in `args`; it consumes the remaining positionals.

`rest({ displayName?, description? })` returns raw remaining strings without per-item decoding.

`option({ type?, long, short?, description?, displayName?, env?, defaultValue?, defaultValueIsSerializable?, onMissing? })` reads one `--long value`, `--long=value`, `-s value`, or `-s=value` pair. It fails when missing, duplicated, missing a value, or failing type decoding unless a default or missing handler applies. The default type is `string`.

`multioption({ type, long, short?, description?, displayName?, defaultValue?, onMissing? })` reads zero or more option occurrences. Its type receives `string[]`; use `array(itemType)` when simple per-value decoding is enough. Error locations are less granular than `option`.

`flag({ type?, long, short?, description?, displayName?, env?, defaultValue?, defaultValueIsSerializable?, onMissing? })` reads `--long`, `--long=true`, `--long=false`, `-s`, and stacked short flags such as `-abc`. It fails on duplicates, missing required flags, and non-boolean values unless a default or missing handler applies. The default type is `boolean`.

`multiflag({ type, long, short?, description?, displayName? })` reads zero or more flag occurrences. Its type receives `boolean[]`; use `array(boolean)` for simple lists.

## Types

`Type<From, To>` converts parsed input into the value used by handlers:

```ts
type Type<From, To> = {
  from(value: From): Promise<To>;
  displayName?: string;
  description?: string;
  defaultValue?: () => To;
  defaultValueIsSerializable?: boolean;
  onMissing?: () => To | Promise<To>;
};
```

Built-in types:

- `string`: `Type<string, string>` for options and positionals.
- `number`: `Type<string, number>` and rejects non-numeric strings.
- `boolean`: `Type<boolean, boolean>` for flags.
- `optional(type)`: returns `undefined` when missing.
- `array(type)`: decodes arrays, useful for `multioption` and `multiflag`.
- `union([typeA, typeB], { combineErrors? })`: tries decoders until one succeeds and combines errors if all fail.
- `oneOf(["a", "b"] as const)`: accepts an exact string literal set.

Use `extendType(baseType, nextTypeOrDecodingFunction)` to compose validation while preserving metadata from the base type. This is often cleaner than reimplementing string parsing.

## Defaults, Missing Values, and Env

Use `defaultValue()` when the fallback is synchronous, deterministic, and appropriate to show in help. Set `defaultValueIsSerializable` when printing the value is useful and safe.

Use `onMissing()` when the fallback should run only during parsing, such as reading a config file, prompting, fetching credentials, or computing a contextual value. It is not called for help generation.

Use `env: "NAME"` on `option` or `flag` when an environment variable should supply the value.

## Running and Testing

`run(app, argv)` parses, prints help/errors as needed, applies exit effects, and returns the handler result on success.

`runSafely(app, argv)` returns a `Result` without applying effects. Use it when tests or hosts must not exit.

`dryRun(app, argv)` runs without quitting and returns a `Result<string, HandlerResult>`.

`parse(app, argv)` parses without running the handler. Use it to assert parser and help behavior independently from command side effects.

## Battery Packs

`cmd-ts/batteries/fs`:

- `ExistingPath`: resolves an existing path and expands relative paths from `process.cwd()`.
- `Directory`: resolves an existing directory; if given an existing file, returns its directory.
- `File`: resolves an existing file and fails when the path is not a file.

`cmd-ts/batteries/url`:

- `Url`: decodes a string into `node:url` `URL` and requires protocol and host.
- `HttpUrl`: decodes a URL and requires `http` or `https`.

Use these before writing local equivalents.
