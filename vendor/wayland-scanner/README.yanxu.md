# Vendored wayland-scanner

This directory contains the published source of `wayland-scanner 0.31.10`
from the Smithay `wayland-rs` project under its MIT license.

Local changes are intentionally limited to:

- requiring `quick-xml 0.41`, which fixes RUSTSEC-2026-0194 and
  RUSTSEC-2026-0195;
- using `GeneralRef::xml10_content`, the upstream API adjustment needed for
  quick-xml 0.40 and later;
- omitting upstream test fixtures from the dependency package.

The public scanner implementation is otherwise byte-for-byte identical to the
published 0.31.10 crate.
