# Third-party notices

Flattener's original Rust source and documentation are licensed under the [MIT license](../LICENSE). Its dependencies and bundled data keep their respective licenses. The application license does not relicense third-party fonts, OCR model weights, character maps, or color profiles.

The distributable notice bundle is [THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt). It contains attribution and license texts for the bundled data and Rust dependencies. Keep it with source and binary distributions; the application also embeds it for offline viewing.

## Bundled data

| Material | Source | License and retained notice |
|---|---|---|
| Text detection and recognition weights | [Robert Knight / Ocrs](https://huggingface.co/robertknight/ocrs) | [CC BY-SA 4.0](../assets/ocr/LICENSE-CC-BY-SA-4.0.txt); [attribution, revision, and hashes](../assets/ocr/README.md) |
| PDF standard-font substitutes | PDFium / Foxit, supplied through Hayro | [BSD-style copyright and redistribution notice](../licenses/assets/PDFium-Foxit-fonts-BSD.txt) |
| Predefined PDF character maps | Adobe CMap resources, supplied through Hayro | [Adobe BSD-style notice](../licenses/assets/Adobe-CMap-resources-BSD.txt) |
| Compact CMYK color profile | [Compact ICC Profiles](https://github.com/saucecontrol/Compact-ICC-Profiles), supplied through Hayro | [CC0 1.0](../licenses/assets/Compact-ICC-Profiles-CC0.txt) |
| Libre Baskerville interface font | [Libre Baskerville Project Authors / Google Fonts](../assets/fonts/README.md) | [SIL Open Font License 1.1 and reserved-font-name notice](../assets/fonts/OFL.txt) |
| Hack Regular font data in GUI dependency | epaint default fonts / Source Foundry | [Hack font license](../licenses/assets/Hack-font-license.txt) |
| Noto Emoji font data in GUI dependency | epaint default fonts / Google | [SIL Open Font License](../licenses/assets/Noto-Emoji-OFL.txt) |
| Ubuntu Light font data in GUI dependency | epaint default fonts / Canonical | [Ubuntu Font License](../licenses/assets/Ubuntu-font-UFL.txt) |
| Emoji icon font data in GUI dependency | epaint default fonts / emoji-icon-font | [MIT notice](../licenses/assets/emoji-icon-font-MIT.txt) |

The OCR models are copied without modification from the pinned upstream revision. Their CC BY-SA 4.0 attribution and license remain applicable independently of the application source license. Recognition is limited to Latin-script text and can contain errors. Critical values should be checked against the visible page.

Hayro incorporates material adapted from PDFBox, pdf.js, and the png crate. Their retained attribution is in [Hayro-NOTICE.md](../licenses/Hayro-NOTICE.md).

The OCR contour postprocessing is adapted from Ocrs under its MIT license; [Ocrs-NOTICE.md](../licenses/Ocrs-NOTICE.md) identifies the source revision, adaptation, and full license. The model-specific detection threshold does not modify model weights.

## Rust libraries

The main rendering, recognition, document-writing, and interface components are Hayro, Ocrs, RTen, lopdf, image, eframe/egui, and rfd. Versions are pinned by `Cargo.lock`; the [dependency inventory](DEPENDENCIES.md) lists the Windows dependency graph, and applicable full texts are included in the notice bundle. Dependencies use upstream releases without patched crates; the application-level contour adaptation is attributed separately above.

Fonts used in the interface and PDF standard-font substitutions are separate from the searchable output's glyphless text layer. The output text layer uses application-generated glyph definitions and Unicode mappings; it does not copy an installed operating-system font.

Flattener does not claim endorsement by Adobe, Google, Foxit, Canonical, Robert Knight, or any other dependency or asset author.
