# `@goddard-ai/paths`

`@goddard-ai/paths` owns pure path resolution for Goddard-managed roots and files across host environments.

It does not read files, write files, persist tokens, or open SQLite databases.

## Package Surfaces

- `@goddard-ai/paths`
  - Shared constants and host-agnostic path names.
- `@goddard-ai/paths/node`
  - Synchronous Node path helpers built on `node:path` and `node:os`.

## Related Docs

- [Paths Glossary](./glossary.md)

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).
