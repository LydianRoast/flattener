# Development

Flattener is a Rust application. The conversion engine, OCR integration, process supervisor, command-line interface and desktop interface are Rust source. The embedded OCR models are data with separate attribution and licensing.

Use Rust 1.94 or newer. Windows builds need the Microsoft C++ Build Tools linker and Windows SDK. No Python, Ghostscript, Adobe automation or external OCR executable is required for normal conversion.

```text
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --release --all-targets --locked
cargo build --release --locked
```

Build products are kept under `local/target`. For a command-line-only build, add `--no-default-features`. Dependency resolution is recorded in `Cargo.lock`. Use optimized builds for conversion and OCR tests; unoptimized neural-network inference is substantially slower.

## Application icon

The source artwork and derived assets are included in `assets/icon`. Regenerate the transparent 256-pixel PNG and the nine Windows icon sizes with:

```text
cargo run --release --locked --example package_icon -- assets/icon/flattener-source.png assets/icon
```

The Rust packager preserves the artwork's proportions and filters premultiplied colour to retain clean transparent edges. The ICO includes 16, 20, 24, 32, 40, 48, 64, 128 and 256-pixel images.

Windows builds use the MSVC target. `build.rs` embeds the icon and package-version metadata into the executable with the Windows SDK's `rc.exe`; no additional Rust dependency is needed. It checks `RC`, `PATH`, the SDK environment and installed SDK directories. If discovery fails, install the Windows SDK or set `RC` to the resource compiler's full executable path. Other platforms skip this Windows resource step and use the embedded PNG for the application window.

## Conversion architecture

I. Validate the destination and create a private temporary directory on the destination volume.

II. Start a separate copy of the executable as a conversion worker. Send a structured request over stdin; progress and results return as JSON lines. No shell command is assembled from filenames.

III. Parse the source with bounds, reject known unsupported document types, and check page dimensions before allocating page rasters. Render one page at a time against white and capture renderer warnings.

IV. Recognize text locally when requested. Write fresh image objects and positioned invisible text into a PDF 1.6 file with a classic cross-reference table. Preserve the displayed crop, rotation and physical page size.

V. Reopen the output and check its structure and page count. Publish without replacing an existing destination. The parent owns the temporary directory and removes it even after a worker failure or cancellation.

The Windows supervisor places the worker in a Job Object with a 2 GiB process memory limit and termination when the job closes. It enforces a five-minute stage-progress timeout and a one-hour document deadline. Other platforms use the child-process guard and deadlines; the Windows memory limit is platform-specific.

## Limits

The default page budget is 40 million pixels. The engine rejects inputs over 512 MiB, outputs over 2 GiB, documents over 10,000 pages, output page dimensions over 200 inches, excessive decompressed content and extreme image declarations. These are operational bounds, not a claim that every file below them will render.

The renderer and OCR run in one worker, so cancelling the worker interrupts expensive page operations promptly. OCR uses the selected raster resolution. The application does not rotate or deskew drawings automatically and does not emit a separate text sidecar.

## Testing and evidence

The Rust tests generate their own PDFs and OCR bitmap text. They exercise converted pixel content, annotation appearances, cropping and rotation, physical scaling, classic xref output, Unicode search mapping, invalid input, cancellation, output collisions and OCR recognition. Validation results and any remaining limits are recorded in [VALIDATION.md](VALIDATION.md).

Reference books, customer PDFs, original executables, temporary screenshots and build caches belong outside tracked source. Public screenshots must use an empty queue or synthetic sample documents.
