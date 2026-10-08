# espeak-rs-sys custom profile patch

This directory contains the crates.io `espeak-rs-sys` 0.2.0 release
(checksum `2d45d148019084e930df6cc3964a58c4c211342451ec5d3c328d8a6cc6b3464d`).
The only source change is in `build.rs`: derive the profile directory from
`OUT_DIR` instead of matching an ancestor against `PROFILE`.

Cargo reports `PROFILE=release` for our release-derived `nightly` profile,
but writes artifacts under `target/nightly`. The upstream implementation
panics before building eSpeak because no ancestor is named `release`.

Remove this directory and the Cargo patch when an upstream release supports
custom profiles. Upstream: https://github.com/thewh1teagle/piper-rs.
