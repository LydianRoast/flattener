# Flattener

Create flattened, searchable PDF copies for Acrobat 7 and other viewers that support PDF 1.6. Flattener renders pages into a new document with simple image compression and a classic cross-reference table, reducing the work required from a legacy viewer.

![Flattener in dark mode](docs/images/flattener-dark.png)

## Use

Open the application, add PDFs, choose an output location and quality, and select **Convert**. Files are processed locally. OCR models are included; conversion does not require an internet connection. Dark and Light modes are available, and the selected mode is remembered.

Existing files are never replaced. The default output is a separate `_Acrobat7.pdf` copy. The application shows progress, supports cancellation, and retains per-file errors and rendering warnings.

See the [technician guide](docs/TECHNICIAN.md) for the complete workflow.

## Output

| Feature | Behavior |
| --- | --- |
| Compatibility | Fresh PDF 1.6, classic cross-reference table, opaque RGB page images |
| Search | Optional offline Latin-script OCR with positioned invisible text |
| Quality | 150, 200 and 300 DPI presets; custom resolution and JPEG quality; lossless option |
| Page geometry | Preserves displayed page size, crop and rotation |
| Batch processing | Multiple PDFs, per-file outcomes and cancellation |
| Validation | Reopens the output and verifies structure and page count before publication |

Rasterization fixes the visible page appearance; it does not preserve editable vectors or interactive forms, layers, links, signatures and attachments. Keep the original for editing and signature validation. OCR may misread numbers and labels. Unsupported PDF features are reported where detectable, but no renderer can guarantee identical appearance for every PDF; review critical drawings after conversion.

The starting profile is 150 DPI with a 40-megapixel page limit. Encrypted PDFs, XFA forms, portfolios and multimedia require an ordinary PDF export first. See the [limits and validation notes](docs/DEVELOPMENT.md).

## Command line

```text
flattener convert drawing.pdf --output drawing_Acrobat7.pdf
flattener convert drawing.pdf --dpi 150 --lossless --no-ocr
flattener --help
```

## Build

Use Rust 1.94 or newer and the Windows Build Tools linker.

```text
cargo build --release --locked
```

The executable is written to `local/target/release`. See [development instructions](docs/DEVELOPMENT.md) for tests and implementation details.

## License

Flattener's Rust source is available under the [MIT license](LICENSE), including commercial use, modification and redistribution. Embedded OCR model weights retain their [CC BY-SA 4.0 license and attribution](assets/ocr/README.md). Dependency and asset notices are listed in [third-party notices](docs/THIRD_PARTY.md).
