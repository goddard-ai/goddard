# Third-Party Notices

Packaged Goddard desktop builds may include third-party native runtime libraries for the embedded daemon.

## macOS arm64 review-sync Git runtime

The macOS arm64 packaged app bundles Homebrew bottle artifacts for the review-sync libgit2 host:

- `libgit2`
  - Purpose: Git object and repository access for review-sync read-heavy operations.
  - Source: <https://github.com/libgit2/libgit2>
  - License: GPL-2.0-only WITH GCC-exception-2.0
- `libssh2`
  - Purpose: Runtime dependency of Homebrew `libgit2`.
  - Source: <https://github.com/libssh2/libssh2>
  - License: BSD-3-Clause
- `llhttp`
  - Purpose: Runtime dependency of Homebrew `libgit2`.
  - Source: <https://github.com/nodejs/llhttp>
  - License: MIT
- `OpenSSL`
  - Purpose: Runtime dependency of Homebrew `libssh2`.
  - Source: <https://github.com/openssl/openssl>
  - License: Apache-2.0

The app build stages these libraries from Homebrew's installed bottle layout and rewrites their macOS loader paths so they resolve within the embedded daemon runtime.
