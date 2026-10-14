# OCR models

Flattener embeds Robert Knight's Ocrs text-detection and text-recognition models so recognition works offline from the first launch. These files are unmodified upstream RTen models, not Flattener-authored software. They recognize Latin-script text; output can contain recognition errors and is not a verified transcription.

The model weights are licensed under **Creative Commons Attribution-ShareAlike 4.0 International (CC BY-SA 4.0)**. Flattener's application license does not replace that license. Distributions containing the models must retain this attribution and the accompanying [license text](LICENSE-CC-BY-SA-4.0.txt). Changes to the model weights: none.

Creator: Robert Knight / Ocrs project.

Source: [Ocrs model card](https://huggingface.co/robertknight/ocrs) and [training project](https://github.com/robertknight/ocrs-models).

Pinned upstream revision: `df0edd170279ab971b53e094c627255a87e1a503`.

| File | Bytes | SHA-256 |
|---|---:|---|
| text-detection-ssfbcj81.rten | 2,523,564 | 614aafabf27c94d386f7aa036c967c2e47e4b9938fa11531ca8f5698c1ca4c36 |
| text-rec-checkpoint-s52qdbqt.rten | 9,716,444 | 606d9a0414c6b73c99df75b707c11c70d1c8b12e1d4f900922e185fc37bfca65 |

The downloaded bytes were checked against the upstream repository's SHA-256 LFS object identifiers. Reproducible source URLs:

- [Detection model](https://huggingface.co/robertknight/ocrs/resolve/df0edd170279ab971b53e094c627255a87e1a503/text-detection-ssfbcj81.rten)
- [Recognition model](https://huggingface.co/robertknight/ocrs/resolve/df0edd170279ab971b53e094c627255a87e1a503/text-rec-checkpoint-s52qdbqt.rten)

The Ocrs and RTen Rust software libraries have their own MIT/Apache-2.0 licenses. The model weights retain CC BY-SA 4.0. No affiliation or endorsement by their authors is implied.

The pinned detector uses a **0.5 text-classification threshold**, as [documented by its maintainer](https://github.com/robertknight/ocrs/issues/254#issuecomment-4620351611). Ocrs 0.13.1 defaults to 0.2 for an older detector; Flattener applies the matching threshold through the public probability-mask API. This changes application postprocessing, not the model files or weights. The adapted contour code retains its [MIT notice](../../licenses/Ocrs-NOTICE.md).
