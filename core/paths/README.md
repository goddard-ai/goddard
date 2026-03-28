# `@goddard-ai/paths`

`@goddard-ai/paths` owns pure path resolution for Goddard-managed roots and files across host environments.

It does not read files, write files, persist tokens, or open SQLite databases.

## Package Surfaces

- `@goddard-ai/paths`
  - Shared constants and host-agnostic path names.
- `@goddard-ai/paths/node`
  - Synchronous Node path helpers built on `node:path` and `node:os`.
- `@goddard-ai/paths/tauri`
  - Async Tauri path helpers built on `@tauri-apps/api/path`.

## Related Docs

- [Paths Glossary](./glossary.md)

## License

This project is licensed under the [MIT License](./LICENSE-MIT).
