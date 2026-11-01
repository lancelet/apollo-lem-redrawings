# Apollo LEM redrawings

Redrawn, fully constrained versions of drawings from the Apollo Lunar
Excursion Module (LEM, the Lunar Module), each line tied to evidence in the
original scan, with a viewer that shows the redraw beside the scan.

**Live site: https://lancelet.github.io/apollo-lem-redrawings/**

The first sheet is Grumman drawing **LDW280-17619, sheet 3**, a detail drawing
of a flat tapered link.


## What you can do

Hover (or tap) over a line, dimension or piece of lettering to light it in the
redraw and the ink it comes from in the scan. Click to pin it and see the
constraints that tie it to the rest of the drawing, and Details for the
evidence and where it came from, back to the archive. The viewer needs a
browser with WebGPU (current Chrome, Edge or Safari); other browsers get a
static page with the redraw and the scan side by side.

## Source of the scans, and rights

The scans are from the National Archives (Southwest region, Fort Worth)
aperture-card box 511 of Grumman Lunar Module drawings. They were uploaded to
the Internet Archive by the Virtual AGC Project
(https://archive.org/details/apertureCardBox511Part1NARASW_images), where they
are marked with the Creative Commons Public Domain Mark 1.0
(https://creativecommons.org/publicdomain/mark/1.0/). That is the uploader's
statement; it has not been checked independently. The drawings were created by
the Grumman Aircraft Engineering Corporation.

The site carries a cropped copy of the scan (tiles, plus maps of which ink
belongs to which part of the redraw). The full original is on the Internet
Archive; each sheet's `manifest.json` gives its address and checksum.

## Licence

The code is dual-licensed under either of

* the MIT licence (`LICENSE-MIT`), or
* the Apache License, Version 2.0 (`LICENSE-APACHE`),

at your option. The drawings and scans carry the Public Domain Mark as stated
above.

## Building from source

You need Rust (the toolchain in `rust-toolchain.toml`, with the
`wasm32-unknown-unknown` target) and [trunk](https://trunkrs.dev/).

```sh
cargo test                       # the library crates' tests
crates/inspect-web/site.sh OUT BUNDLE_DIR   # build the site into OUT
```

`BUNDLE_DIR` holds the sheet bundles (`<drawing>/sh<N>/`, as in the site's
`sheets/` directory). To see the site, serve OUT with any static file server.

## Where this comes from

This repository is generated from a private development repository, where
the redraws are made (marked up from the scans, fitted and solved). Only the
viewer, the document format and its renderer are here, with the built site.
Generated from development commit f5d1d7f (with uncommitted changes) on 2026-11-01.
