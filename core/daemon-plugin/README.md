# `@goddard-ai/daemon-plugin`

Internal daemon plugin support contracts for feature packages.

This package is infrastructure for statically composed Goddard feature packages. It is not a public plugin platform and should stay close to type-only until daemon feature composition needs runtime helpers.

Feature packages should import IPC schema primitives and composition helpers from `@goddard-ai/ipc`. This package only references IPC schemas as daemon plugin metadata.

Daemon plugins may expose a named `provides` map and list other daemon plugin definitions in `consumes`. The `setup(context)` callback receives the consumed plugins' provided feature extensions as first-class context fields, such as `context.session`, while daemon-owned substrate remains the responsibility of the daemon composition root.
