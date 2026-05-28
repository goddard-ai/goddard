# @goddard-ai/schema

This package contains schemas for Goddard's core daemon, backend, and shared substrate contracts. Feature-owned product schemas live in the owning feature package's `./schema` entrypoint instead of being added or re-exported here.

## Related Docs

- [Schema Glossary](./glossary.md)

## Usage

Core packages import core schemas from this package. Feature-owned schemas should
be imported from their owning feature package, such as
`@goddard-ai/pull-request/schema`.

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).
