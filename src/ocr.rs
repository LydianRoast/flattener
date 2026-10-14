//! Offline, Latin-script OCR with models loaded once for each conversion.
//!
//! Recognition is an aid to search and copying, not a verified transcription.
//! Coordinates describe the supplied RGB raster, with the origin at its top left.

use anyhow::{Context, Result, ensure};
use ocrs::{ImageSource, OcrEngine as RecognitionEngine, OcrEngineParams, TextItem};
use rten::Model;
use rten_imageproc::{RetrievalMode, RotatedRect, find_contours, min_area_rect, simplify_polygon};
use rten_tensor::NdTensorView;
use rten_tensor::prelude::*;

const DETECTION_MODEL: &[u8] = include_bytes!("../assets/ocr/text-detection-ssfbcj81.rten");
const RECOGNITION_MODEL: &[u8] = include_bytes!("../assets/ocr/text-rec-checkpoint-s52qdbqt.rten");
// This pinned HF detector requires 0.5, unlike the older model's 0.2 default.
// https://github.com/robertknight/ocrs/issues/254#issuecomment-4620351611
const TEXT_THRESHOLD: f32 = 0.5;

pub const OCR_NOTICE: &str = "Searchable text uses Latin-script OCR and may contain recognition errors. Verify critical values against the visible page.";

/// A recognized word and its axis-aligned bounds in source raster pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrWord {
    pub text: String,
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// Reuse this object for every page in a conversion. No network or cache is used.
pub struct OcrEngine {
    inner: RecognitionEngine,
}

impl OcrEngine {
    pub fn new() -> Result<Self> {
        let detection_model = Model::load_static_slice(DETECTION_MODEL)
            .context("The embedded text detection model could not be loaded")?;
        let recognition_model = Model::load_static_slice(RECOGNITION_MODEL)
            .context("The embedded text recognition model could not be loaded")?;
        let inner = RecognitionEngine::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .context("The offline OCR engine could not start")?;
        Ok(Self { inner })
    }

    /// Recognize tightly packed, eight-bit RGB pixels.
    ///
    /// Single-character words are retained: equipment identifiers, numbers and
    /// drawing labels can legitimately consist of just one character. A blank
    /// page has no words and is a successful result. OCR confidence is not
    /// fabricated, since the upstream word API does not expose a score.
    pub fn recognize(&self, rgb: &[u8], width: u32, height: u32) -> Result<Vec<OcrWord>> {
        validate_rgb(rgb, width, height)?;
        if rgb.chunks_exact(3).all(|pixel| pixel == &rgb[..3]) {
            return Ok(Vec::new());
        }

        let image = ImageSource::from_bytes(rgb, (width, height))
            .context("The OCR page raster could not be read")?;
        let input = self
            .inner
            .prepare_input(image)
            .context("The OCR page could not be prepared")?;
        let text_probabilities = self
            .inner
            .detect_text_pixels(&input)
            .context("Text detection failed on this page")?;
        let word_rectangles = word_rectangles(text_probabilities.view());
        let line_rectangles = self.inner.find_text_lines(&input, &word_rectangles);
        let lines = self
            .inner
            .recognize_text(&input, &line_rectangles)
            .context("Text recognition failed on this page")?;
        let mut words = Vec::new();
        for line in lines.iter().flatten() {
            for word in line.words() {
                let bounds = word.bounding_rect();
                let text = word.to_string();
                let left = (bounds.left() as f32).clamp(0.0, width as f32);
                let right = (bounds.right() as f32).clamp(0.0, width as f32);
                let top = (bounds.top() as f32).clamp(0.0, height as f32);
                let bottom = (bounds.bottom() as f32).clamp(0.0, height as f32);
                ensure!(
                    right > left && bottom > top,
                    "OCR produced an invalid text position; searchable text was not accepted"
                );
                ensure!(
                    !text.is_empty() && !text.chars().any(char::is_control),
                    "OCR produced invalid text; searchable text was not accepted"
                );
                words.push(OcrWord {
                    text,
                    left,
                    top,
                    right,
                    bottom,
                });
            }
        }
        Ok(words)
    }
}

/// Contour postprocessing adapted from Ocrs 0.13.1 `src/detection.rs`.
/// Copyright (c) 2023 Ocrs project contributors; MIT. See licenses/Ocrs-NOTICE.md.
fn word_rectangles(probabilities: NdTensorView<f32, 2>) -> Vec<RotatedRect> {
    let mask = probabilities.map(|value| *value > TEXT_THRESHOLD);
    find_contours(mask.view(), RetrievalMode::External)
        .iter()
        .filter_map(|polygon| {
            let points: Vec<_> = polygon.iter().map(|point| point.to_f32()).collect();
            let simplified = simplify_polygon(&points, 2.0);
            min_area_rect(&simplified).map(|mut bounds| {
                // Retain upstream's three-pixel expansion and minimum area.
                bounds.resize(bounds.width() + 6.0, bounds.height() + 6.0);
                bounds
            })
        })
        .filter(|bounds| bounds.area() >= 100.0)
        .collect()
}

fn validate_rgb(rgb: &[u8], width: u32, height: u32) -> Result<()> {
    ensure!(
        width > 0 && height > 0,
        "OCR requires a nonempty page raster"
    );
    ensure!(
        width <= i32::MAX as u32 && height <= i32::MAX as u32,
        "The OCR page dimensions exceed the supported range"
    );
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(3))
        .context("The OCR page dimensions exceed the addressable size")?;
    ensure!(
        rgb.len() == expected,
        "The OCR RGB buffer length does not match its dimensions"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_rgb_before_model_processing() {
        assert!(validate_rgb(&[], 0, 1).is_err());
        assert!(validate_rgb(&[], 1, 1).is_err());
        assert!(validate_rgb(&[0; 4], 1, 1).is_err());
        assert!(validate_rgb(&[255; 3], 1, 1).is_ok());
        assert!(validate_rgb(&[], u32::MAX, 1).is_err());
    }

    #[test]
    fn bundled_models_load_and_blank_pages_have_no_text() {
        let engine = OcrEngine::new().expect("bundled OCR models must load offline");
        let white_page = vec![255; 64 * 64 * 3];
        assert!(engine.recognize(&white_page, 64, 64).unwrap().is_empty());
        assert!(engine.recognize(&[0; 2], 1, 1).is_err());
    }

    #[test]
    fn recognizes_generated_text_and_preserves_pixel_positions() {
        // This fixture is authored here, rather than copied from a document or
        // dependent on an installed font. These are ordinary 5x7 capital glyphs.
        const HELLO: [[u8; 7]; 5] = [
            [17, 17, 17, 31, 17, 17, 17], // H
            [31, 16, 16, 30, 16, 16, 31], // E
            [16, 16, 16, 16, 16, 16, 31], // L
            [16, 16, 16, 16, 16, 16, 31], // L
            [14, 17, 17, 17, 17, 17, 14], // O
        ];
        let (width, height, scale) = (400usize, 160usize, 8usize);
        let mut rgb = vec![255; width * height * 3];
        for (glyph_index, glyph) in HELLO.iter().enumerate() {
            for (row, bits) in glyph.iter().enumerate() {
                for column in 0..5 {
                    if bits & (1 << (4 - column)) == 0 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = 48 + glyph_index * 6 * scale + column * scale + dx;
                            let y = 48 + row * scale + dy;
                            rgb[(y * width + x) * 3..(y * width + x) * 3 + 3].fill(0);
                        }
                    }
                }
            }
        }
        let engine = OcrEngine::new().unwrap();
        let words = engine.recognize(&rgb, width as u32, height as u32).unwrap();
        let text = words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("HELLO"), "recognized: {text:?}");
        for word in words {
            assert!(word.left >= 0.0 && word.right <= width as f32);
            assert!(word.top >= 0.0 && word.bottom <= height as f32);
            assert!(word.left < word.right && word.top < word.bottom);
        }
    }

    #[test]
    fn recognizes_full_document_heading_body_and_form_appearance() {
        // Generated from examples/validation_fixture.rs, not a user document.
        // The 0.2 threshold gives giant false positives on this full-page image;
        // a small cropped-word fixture alone does not catch that regression.
        let page = image::load_from_memory(include_bytes!("../assets/tests/ocr-document.jpg"))
            .unwrap()
            .into_rgb8();
        let engine = OcrEngine::new().unwrap();
        let words = engine
            .recognize(page.as_raw(), page.width(), page.height())
            .unwrap();
        let text = words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for expected in ["HELLO", "123", "ABC", "grayscale", "appearance.", "FIELD42"] {
            assert!(text.contains(expected), "missing {expected:?} in {text:?}");
        }
        let heading = words.iter().find(|word| word.text == "HELLO").unwrap();
        assert!(heading.top > 80.0 && heading.bottom < 200.0, "{heading:?}");
        assert!(heading.left > 70.0 && heading.right < 400.0, "{heading:?}");
        assert!(
            words
                .iter()
                .any(|word| word.text == "FIELD42" && word.top > 950.0)
        );
        for word in words {
            assert!(word.right <= page.width() as f32 && word.bottom <= page.height() as f32);
            assert!(word.left >= 0.0 && word.top >= 0.0);
            assert!(
                word.bottom - word.top < 100.0,
                "implausible word bounds: {word:?}"
            );
        }
    }
}
