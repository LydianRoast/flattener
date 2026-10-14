# Validation

Validation is specific to the tested files and application version. It is not a guarantee of identical rendering for every PDF feature.

## Automated checks

The release suite passed all **34 tests**: 15 unit tests, 4 command-line tests, and 15 conversion-engine tests. Coverage includes page pixels, form appearances, classic cross-reference output, inherited and fractional crop boxes, rotation, physical page scaling, Unicode text mappings, offline OCR, cancellation, corrupt content, output collisions, and the actual isolated command-line worker. Theme tests exercise persistence and text contrast in both palettes.

The full-page OCR regression checks an inverted heading, body text, and a saved form value, together with plausible word positions. This catches the difference between the bundled detector's required 0.5 classification threshold and the library's 0.2 default for an older model. A separate small-word test and a blank-page test also pass; recognition does not require a download.

Formatting and strict Clippy checks pass with Rust 1.96.0. The dependency graph declares Rust 1.94 as its highest minimum supported version; this run uses 1.96.0.

## Viewer validation

The PDF 1.6 output was opened and checked in **Adobe Acrobat 7.0 Professional**, executable version 7.0.0.2004121400, on 2026-10-14. These checks used the release conversion binary with the corrected OCR threshold.

| Test document | Source bytes | Output bytes | Acrobat result |
|---|---:|---:|---|
| Two-page synthetic PDF 2.0 with object streams, saved form appearance, crop box, and rotation | 1,694 | 222,083 | Both pages open; heading, color blocks, grayscale, form value, border, and rotation render correctly. Find succeeds for `HELLO`, `123`, and `FIELD42`. |
| Two-page scanned technical reference with mixed page orientations | 3,858,269 | 945,788 | Both pages open; page dimensions and searchable text are present. Fine chart labels still contain OCR errors. |

The synthetic output has page boxes of 612 x 792 and 330 x 540 points, including the cropped, rotated second page. The scan retains its 792 x 612 and 612 x 792 point boxes. Acrobat returned positioned word quadrilaterals, confirming a usable text layer rather than only page metadata. A separate glyphless-font fixture verified accented and non-BMP Unicode extraction in Acrobat; the viewer normalizes the micro sign to Greek mu. This text-encoding test does not expand the OCR model's supported alphabet.

The first synthetic page was inspected visually in Acrobat, and the rotated second page was inspected separately. Its sideways text renders correctly, but its OCR is not a reliable transcription. SHA-256 hashes of both source files were unchanged after conversion and viewer checks.

The scanned sample is about 75.5% smaller. The tiny vector source grows substantially when rasterized, so these measurements do not imply that every converted file becomes smaller.

## Interface and cancellation

The serif interface was inspected in both light and dark themes. Theme selection persists after a normal close and relaunch. The default 930 x 660 window and minimum 760 x 640 window fit a 1366 x 768 display, and the offline license dialog is readable in both themes.

An active conversion worker was cancelled through the interface. The child process stopped, the interface reported one cancelled job and zero failed jobs, no output was published, and the source hash remained unchanged. Dark mode was retained after the check.

The application palette changes immediately when its theme is switched. The native Windows title bar can retain its previous color until the window redraws or is resized.

The release executable contains version 0.2.0.0 and nine icon sizes: 16, 20, 24, 32, 40, 48, 64, 128, and 256 pixels. Windows Shell extraction succeeds at 32 and 16 pixels, and the running window displays the ink bottle and quill icon. Both interface screenshots were captured from this release.

## Practical limits

The output is a rasterized visual copy with optional newly recognized text. It cannot preserve editable vectors, interactive forms, layers, attachments, or valid digital signatures. Some source features may render differently or be unsupported; inspect important drawing details and reported warnings.

OCR supports Latin-script text and can misread numbers, equipment tags, and small or rotated labels. Page dimensions are retained, but rendered linework has finite pixel resolution. The selected resolution also controls OCR input resolution.

No throughput comparison against the former executable has been established. Conversion times depend on page size, complexity, resolution, OCR, processor, and storage. Large drawings may need 150 DPI to stay within the default 40-million-pixel page limit.

The conversion worker has explicit input, output, pixel, memory, and time bounds. These improve failure handling; they are not a security sandbox or proof that every file within the bounds can be rendered.
