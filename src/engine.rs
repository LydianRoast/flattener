//! Bounded, page-at-a-time rendering into an Acrobat 7 compatible PDF.
use anyhow::{Context, Result, anyhow, bail, ensure};
use hayro::hayro_interpret::font::FontQuery;
use hayro::hayro_interpret::{InterpreterSettings, InterpreterWarning};
use hayro::hayro_syntax::Pdf;
use image::codecs::jpeg::JpegEncoder;
use lopdf::{Document, LoadOptions, Object, Stream};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_OUTPUT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_STREAM_BYTES: usize = 128 * 1024 * 1024;
const MAX_PAGES: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    pub dpi: u32,
    pub jpeg_quality: u8,
    pub lossless: bool,
    pub max_page_pixels: u64,
    pub searchable: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            dpi: 150,
            jpeg_quality: 90,
            lossless: false,
            max_page_pixels: 40_000_000,
            searchable: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    /// Completed pages; zero during preflight and before the first page.
    pub page: usize,
    pub total_pages: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub input_pages: usize,
    pub output_pages: usize,
    pub output_bytes: u64,
    pub warnings: Vec<String>,
    /// True only when the output contains at least one recognized word.
    pub searchable: bool,
    pub recognized_words: usize,
}

fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Conversion cancelled. The original file was not changed."
    );
    Ok(())
}

/// Write a new file, never replacing the source or an existing destination.
/// Cancellation is checked between expensive phases and pages; an individual
/// renderer/OCR call cannot be interrupted in this thread.
pub fn convert(
    input: &Path,
    output: &Path,
    options: &Options,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<Report> {
    cancelled(cancel)?;
    ensure!(
        (72..=600).contains(&options.dpi),
        "Resolution must be between 72 and 600 DPI."
    );
    ensure!(
        (50..=100).contains(&options.jpeg_quality),
        "JPEG quality must be between 50 and 100."
    );
    ensure!(
        (1..=100_000_000).contains(&options.max_page_pixels),
        "The page pixel limit must be between 1 and 100 million."
    );
    ensure!(
        output.file_name().is_some(),
        "Choose a filename for the converted PDF."
    );
    ensure!(
        output
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("pdf")),
        "The output filename must end in .pdf."
    );
    ensure!(
        !output
            .try_exists()
            .context("Cannot inspect the output location")?,
        "The output already exists. Choose a different filename."
    );
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .context("The output folder does not exist or cannot be accessed")?;
    ensure!(parent.is_dir(), "The output location is not a folder.");
    let destination = parent.join(output.file_name().unwrap());
    let source = input.canonicalize().context("Cannot find the source PDF")?;
    ensure!(
        source != destination,
        "The output must be a different file from the source."
    );
    let mut open = OpenOptions::new();
    open.read(true);
    // Keep another Windows process from changing or deleting the source while it is read.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        open.share_mode(1);
    }
    let source_file = open
        .open(&source)
        .context("Cannot open the source PDF; it may be locked")?;
    let metadata = source_file
        .metadata()
        .context("Cannot inspect the source PDF")?;
    ensure!(metadata.is_file(), "The source is not a regular file.");
    ensure!(
        metadata.len() <= MAX_INPUT_BYTES,
        "The source exceeds the 512 MiB input limit. Split it into smaller PDFs first."
    );
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(metadata.len() as usize)
        .context("Not enough memory to open this PDF")?;
    source_file
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("Cannot read the source PDF")?;
    ensure!(
        bytes.len() as u64 <= MAX_INPUT_BYTES,
        "The source grew beyond the 512 MiB input limit."
    );
    ensure!(
        bytes[..bytes.len().min(1024)]
            .windows(5)
            .any(|s| s == b"%PDF-"),
        "The source does not have a PDF header."
    );
    progress(Progress {
        page: 0,
        total_pages: 0,
        message: "Checking document".into(),
    });
    let preflight =
        preflight(&bytes, cancel).context("The PDF could not pass the compatibility check")?;
    cancelled(cancel)?;
    let pdf = Pdf::new(bytes)
        .map_err(|e| anyhow!("The PDF renderer could not open this document: {e:?}"))?;
    let pages = pdf.pages();
    ensure!(
        pages.len() == preflight.user_units.len(),
        "The PDF parsers disagree about its page count. Repair the source in its authoring application first."
    );
    let total = pages.len();
    let mut warnings = preflight.warnings;
    // Validate all page sizes before creating an output or loading the OCR models.
    let mut dimensions = Vec::with_capacity(total);
    for (i, page) in pages.iter().enumerate() {
        let crop = page.intersected_crop_box();
        ensure!(
            crop.width().is_finite()
                && crop.height().is_finite()
                && crop.width() > 0.0
                && crop.height() > 0.0,
            "Page {} has an invalid or empty crop box.",
            i + 1
        );
        ensure!(
            crop.width() >= 1.0 && crop.height() >= 1.0,
            "Page {} has a crop dimension below one PDF unit, which this renderer cannot preserve exactly.",
            i + 1
        );
        let (w, h) = page.render_dimensions();
        dimensions.push(
            page_dimensions(w, h, preflight.user_units[i], options)
                .with_context(|| format!("Page {}", i + 1))?,
        );
    }
    let ocr = if options.searchable {
        progress(Progress {
            page: 0,
            total_pages: total,
            message: "Loading text recognition".into(),
        });
        Some(crate::ocr::OcrEngine::new().context(
            "Cannot load text recognition; turn off Searchable text to make an image-only copy",
        )?)
    } else {
        None
    };
    cancelled(cancel)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".flattener-")
        .suffix(".tmp")
        .tempfile_in(&parent)
        .context("Cannot create a temporary file in the output folder")?;
    let mut writer = PdfWriter::new(temporary.as_file_mut())?;
    let mut page_ids = Vec::with_capacity(total);
    let mut recognized_words = 0;
    for (index, page) in pages.iter().enumerate() {
        cancelled(cancel)?;
        progress(Progress {
            page: index,
            total_pages: total,
            message: format!("Rendering page {} of {total}", index + 1),
        });
        let page_warnings = Arc::new(Mutex::new(BTreeSet::new()));
        let warning_target = page_warnings.clone();
        let font_warning_target = page_warnings.clone();
        let default_font_resolver = InterpreterSettings::default().font_resolver;
        let settings = InterpreterSettings {
            font_resolver: Arc::new(move |query| {
                if let FontQuery::Fallback(details) = &query {
                    let name = details
                        .font_name
                        .as_deref()
                        .or(details.post_script_name.as_deref())
                        .unwrap_or("unnamed font");
                    if let Ok(mut list) = font_warning_target.lock() {
                        list.insert(format!("The source font '{name}' is not embedded; a substitute is used. Check text appearance."));
                    }
                }
                default_font_resolver(query)
            }),
            warning_sink: Arc::new(move |warning| {
                let message = match warning {
                    InterpreterWarning::UnsupportedFont => "An unsupported font may be missing.",
                    InterpreterWarning::ImageDecodeFailure => {
                        "An image failed to decode and may be missing."
                    }
                    InterpreterWarning::UnresolvedAnnotationAppearance => {
                        "An annotation appearance could not be rendered."
                    }
                    _ => "The renderer reported an unsupported feature.",
                };
                if let Ok(mut list) = warning_target.lock() {
                    list.insert(message.to_string());
                }
            }),
            render_annotations: true,
            ..Default::default()
        };
        let (physical_w, physical_h, pixel_w, pixel_h) = dimensions[index];
        let scale = options.dpi as f32 / 72.0 * preflight.user_units[index];
        // A fresh cache prevents retained decoded images from accumulating across pages.
        let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            hayro::render(
                page,
                &hayro::RenderCache::new(),
                &settings,
                &hayro::RenderSettings::default(),
                &hayro::PixmapSettings {
                    x_scale: scale,
                    y_scale: scale,
                    bg_color: hayro::vello_cpu::color::palette::css::WHITE,
                },
            )
        }))
        .map_err(|_| {
            anyhow!(
                "The renderer failed on page {}. No output was published.",
                index + 1
            )
        })?;
        cancelled(cancel)?;
        ensure!(
            u32::from(rendered.width()) == pixel_w && u32::from(rendered.height()) == pixel_h,
            "Unexpected raster dimensions on page {}.",
            index + 1
        );
        let rgba = rendered.data_as_u8_slice();
        let mut rgb = Vec::new();
        rgb.try_reserve_exact(pixel_w as usize * pixel_h as usize * 3)
            .context("Not enough memory for the rendered page")?;
        for pixel in rgba.chunks_exact(4) {
            // The rasterizer uses premultiplied RGBA. White is explicit, and this
            // also handles any residual alpha without introducing a black background.
            let white = 255 - pixel[3];
            rgb.extend_from_slice(&[
                pixel[0].saturating_add(white),
                pixel[1].saturating_add(white),
                pixel[2].saturating_add(white),
            ]);
        }
        drop(rendered);
        for warning in page_warnings
            .lock()
            .map_err(|_| anyhow!("Cannot read rendering diagnostics"))?
            .iter()
        {
            warnings.push(format!("Page {}: {warning}", index + 1));
        }
        let words = if let Some(ocr) = &ocr {
            progress(Progress {
                page: index,
                total_pages: total,
                message: format!("Recognizing text on page {} of {total}", index + 1),
            });
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ocr.recognize(&rgb, pixel_w, pixel_h)
            }))
            .map_err(|_| {
                anyhow!(
                    "Text recognition failed on page {}. No output was published.",
                    index + 1
                )
            })?
            .with_context(|| format!("Text recognition failed on page {}", index + 1))?
        } else {
            Vec::new()
        };
        cancelled(cancel)?;
        recognized_words += words.len();
        let image_id = writer.reserve();
        let content_id = writer.reserve();
        let page_id = writer.reserve();
        let mut content = format!("q\n{physical_w:.5} 0 0 {physical_h:.5} 0 0 cm\n/Im0 Do\nQ\n");
        let font = if words.is_empty() {
            None
        } else {
            let (font_id, text) = write_search_text(
                &mut writer,
                &words,
                physical_w,
                physical_h,
                pixel_w,
                pixel_h,
            )?;
            content.push_str(&text);
            Some(font_id)
        };
        if options.lossless {
            let mut stream = Stream::new(lopdf::Dictionary::new(), rgb);
            stream
                .compress()
                .context("Cannot compress the rendered page")?;
            let filter = if stream.dict.has(b"Filter") {
                " /Filter /FlateDecode"
            } else {
                ""
            };
            writer.stream(image_id, &format!("/Type /XObject /Subtype /Image /Width {pixel_w} /Height {pixel_h} /ColorSpace /DeviceRGB /BitsPerComponent 8{filter}"), &stream.content)?;
        } else {
            let mut encoded = Vec::new();
            JpegEncoder::new_with_quality(&mut encoded, options.jpeg_quality)
                .encode(&rgb, pixel_w, pixel_h, image::ExtendedColorType::Rgb8)
                .context("Cannot encode the rendered page")?;
            drop(rgb);
            writer.stream(image_id, &format!("/Type /XObject /Subtype /Image /Width {pixel_w} /Height {pixel_h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode"), &encoded)?;
        }
        writer.stream(content_id, "", content.as_bytes())?;
        let font_resource = font
            .map(|id| format!(" /Font << /F0 {id} 0 R >>"))
            .unwrap_or_default();
        writer.object(page_id, &format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {physical_w:.5} {physical_h:.5}] /Resources << /XObject << /Im0 {image_id} 0 R >>{font_resource} >> /Contents {content_id} 0 R >>"))?;
        page_ids.push(page_id);
        ensure!(
            writer.position()? <= MAX_OUTPUT_BYTES,
            "The output exceeds the 2 GiB limit. Split the source or use a lower resolution."
        );
        progress(Progress {
            page: index + 1,
            total_pages: total,
            message: format!("Completed page {} of {total}", index + 1),
        });
    }
    cancelled(cancel)?;
    let xref = writer.finish(&page_ids)?;
    temporary
        .as_file_mut()
        .sync_all()
        .context("Cannot finish writing the converted PDF")?;
    progress(Progress {
        page: total,
        total_pages: total,
        message: "Verifying converted PDF".into(),
    });
    verify_output(temporary.path(), total, xref)?;
    cancelled(cancel)?;
    let output_bytes = temporary.as_file().metadata()?.len();
    // Same-directory publication is atomic; no-clobber also closes the race
    // between the initial destination check and this final operation.
    temporary.persist_noclobber(&destination).map_err(|e| {
        anyhow!(
            "Cannot publish the converted PDF without replacing another file: {}",
            e.error
        )
    })?;
    if options.searchable && recognized_words == 0 {
        warnings.push(
            "Text recognition found no readable words; this copy has no searchable text.".into(),
        );
    }
    Ok(Report {
        input_pages: total,
        output_pages: total,
        output_bytes,
        warnings: warnings.into_vec(),
        searchable: recognized_words > 0,
        recognized_words,
    })
}

struct Preflight {
    user_units: Vec<f32>,
    warnings: WarningLog,
}

#[derive(Default)]
struct WarningLog {
    entries: Vec<String>,
    bytes: usize,
    omitted: usize,
}

impl WarningLog {
    fn push(&mut self, mut message: String) {
        if message.chars().count() > 1024 {
            message = message.chars().take(1000).collect();
            message.push_str(" [message shortened]");
        }
        if self.entries.len() >= 256 || self.bytes + message.len() > 128 * 1024 {
            self.omitted += 1;
        } else {
            self.bytes += message.len();
            self.entries.push(message);
        }
    }

    fn into_vec(mut self) -> Vec<String> {
        if self.omitted != 0 {
            self.entries.push(format!("{} additional warning messages were omitted from this bounded report. Review the converted pages against the source.", self.omitted));
        }
        self.entries
    }
}

fn preflight(bytes: &[u8], cancel: &AtomicBool) -> Result<Preflight> {
    let doc = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            max_decompressed_size: Some(MAX_STREAM_BYTES),
            ..Default::default()
        },
    )
    .context("Cannot parse this PDF. It may be damaged or password protected")?;
    ensure!(
        !doc.is_encrypted() && !doc.was_encrypted(),
        "Encrypted or password-protected PDFs are not supported. Save an unencrypted copy with your authorized PDF application first."
    );
    let catalog = doc
        .catalog()
        .context("The PDF has no valid document catalog")?;
    ensure!(
        !catalog.has(b"Collection"),
        "PDF portfolios are not supported. Extract and convert the individual documents first."
    );
    if let Ok(form) = catalog.get(b"AcroForm") {
        let (_, form) = doc.dereference(form)?;
        ensure!(
            !form.as_dict()?.has(b"XFA"),
            "XFA forms are not supported. Print or export the form to a regular PDF first."
        );
    }
    let mut warnings = WarningLog::default();
    if catalog.has(b"AcroForm") {
        warnings.push("Form fields become a fixed page appearance; their values cannot be edited in this copy.".into());
    }
    if catalog.has(b"OCProperties") {
        warnings.push("Layered content is rendered using the renderer's default visibility. Check the converted pages against the source.".into());
    }
    let pages = doc.get_pages();
    ensure!(!pages.is_empty(), "The PDF has no readable pages.");
    ensure!(
        pages.len() <= MAX_PAGES,
        "The PDF exceeds the 10,000-page limit. Split it into smaller documents first."
    );
    let mut units = Vec::with_capacity(pages.len());
    let mut decoded_page_bytes = 0usize;
    for (number, id) in pages {
        cancelled(cancel)?;
        let page = doc.get_dictionary(id)?;
        let media = inherited_attribute(&doc, id, b"MediaBox")?
            .context("A page is missing its required media box")?;
        validate_box(media).with_context(|| format!("Page {number} has an invalid media box"))?;
        if let Some(crop) = inherited_attribute(&doc, id, b"CropBox")? {
            validate_box(crop).with_context(|| format!("Page {number} has an invalid crop box"))?;
        }
        if let Some(rotation) = inherited_attribute(&doc, id, b"Rotate")? {
            let rotation = rotation
                .as_i64()
                .context("Page rotation must be an integer")?;
            ensure!(
                i32::try_from(rotation).is_ok() && rotation % 90 == 0,
                "Page {number} has an unsupported rotation; only whole quarter turns are valid."
            );
        }
        let unit = match page.get(b"UserUnit") {
            Ok(object) => number_value(doc.dereference(object)?.1).context("Invalid page unit")?,
            Err(_) => 1.0,
        };
        ensure!(
            unit.is_finite() && unit > 0.0 && unit <= 75_000.0,
            "Page {number} has an invalid physical scale."
        );
        units.push(unit);
        let page_content = doc
            .get_page_content_with_limit(id, MAX_STREAM_BYTES)
            .with_context(|| format!("Page {number} has excessive or invalid content"))?;
        decoded_page_bytes = decoded_page_bytes
            .checked_add(page_content.len())
            .context("The decoded document is too large")?;
        ensure!(
            decoded_page_bytes <= MAX_INPUT_BYTES as usize,
            "Decoded page content exceeds the 512 MiB document limit. Split the source into smaller documents first."
        );
        // lopdf's convenience page-content method falls back to raw bytes on
        // decoding errors. Reject broken filters it can validate explicitly,
        // instead of accepting a page that the renderer could leave blank.
        for content_id in doc.get_page_contents(id) {
            let stream = doc.get_object(content_id)?.as_stream()?;
            if let Ok(filters) = stream.filters()
                && filters.iter().all(|filter| {
                    matches!(*filter, b"FlateDecode" | b"LZWDecode" | b"ASCII85Decode")
                })
            {
                let parameters = stream
                    .dict
                    .get(b"DecodeParms")
                    .ok()
                    .map(|value| doc.dereference(value).map(|(_, value)| value))
                    .transpose()?;
                let mut data = stream.content.clone();
                for (index, filter) in filters.iter().enumerate() {
                    if *filter == b"FlateDecode" {
                        validate_zlib(&data).with_context(|| {
                            format!("Page {number} has a damaged content stream")
                        })?;
                    }
                    let mut stage = Stream::new(lopdf::Dictionary::new(), data);
                    stage.dict.set("Filter", Object::Name(filter.to_vec()));
                    let parameter = match parameters {
                        Some(Object::Array(values)) => values.get(index),
                        value => value,
                    };
                    if let Some(parameter) = parameter {
                        stage.dict.set("DecodeParms", parameter.clone());
                    }
                    data = stage
                        .decompressed_content_with_limit(MAX_STREAM_BYTES)
                        .with_context(|| format!("Page {number} has a damaged content stream"))?;
                }
            }
        }
        for annotation in doc
            .get_page_annotations(id)
            .context("Cannot read page annotations")?
        {
            let flags = annotation.get(b"F").and_then(Object::as_i64).unwrap_or(0);
            if flags & 2 != 0 {
                continue;
            }
            let subtype = annotation
                .get(b"Subtype")
                .and_then(Object::as_name)
                .unwrap_or(b"Unknown");
            if flags & (1 | 32) != 0 {
                warnings.push(format!("Page {number}: an annotation uses visibility flags that this renderer may not honor. Check its appearance against the source."));
            }
            if annotation.get(b"FT").and_then(Object::as_name).ok() == Some(b"Sig") {
                warnings.push(format!("Page {number}: only the signature's saved appearance is retained; the converted copy has no valid digital signature."));
            }
            ensure!(
                !matches!(subtype, b"RichMedia" | b"3D" | b"Movie" | b"Sound"),
                "Page {number} contains multimedia or 3D content that cannot be flattened faithfully."
            );
            if !annotation.has(b"AP") && subtype != b"Link" {
                warnings.push(format!("Page {number}: a {} annotation has no saved appearance and may not be visible in the converted copy.", String::from_utf8_lossy(subtype)));
            }
        }
    }
    // Reject extreme declared image allocations before handing them to a decoder.
    for object in doc.objects.values() {
        if let Object::Stream(stream) = object
            && stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
        {
            let w = stream
                .dict
                .get(b"Width")
                .and_then(Object::as_i64)
                .unwrap_or(0);
            let h = stream
                .dict
                .get(b"Height")
                .and_then(Object::as_i64)
                .unwrap_or(0);
            ensure!(
                w > 0
                    && h > 0
                    && (w as u64)
                        .checked_mul(h as u64)
                        .is_some_and(|n| n <= 100_000_000),
                "An embedded image has invalid dimensions or exceeds 100 million pixels."
            );
        }
    }
    Ok(Preflight {
        user_units: units,
        warnings,
    })
}

/// lopdf deliberately accepts broken zlib checksums and partial deflate output.
/// Content streams need a strict, bounded check so corruption cannot become a
/// successful blank page. A fixed scratch buffer avoids a second decoded copy.
fn validate_zlib(data: &[u8]) -> Result<()> {
    let mut decoder = flate2::Decompress::new(true);
    let mut scratch = [0u8; 64 * 1024];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder.decompress(
            &data[before_in as usize..],
            &mut scratch,
            flate2::FlushDecompress::None,
        )?;
        ensure!(
            decoder.total_out() <= MAX_STREAM_BYTES as u64,
            "The decoded content stream exceeds the 128 MiB limit."
        );
        if status == flate2::Status::StreamEnd {
            return Ok(());
        }
        ensure!(
            decoder.total_in() > before_in || decoder.total_out() > before_out,
            "The compressed content stream is incomplete or invalid."
        );
    }
}

fn number_value(object: &Object) -> Result<f32> {
    match object {
        Object::Integer(n) => Ok(*n as f32),
        Object::Real(n) => Ok(*n),
        _ => bail!("Expected a numeric value"),
    }
}

fn inherited_attribute<'a>(
    doc: &'a Document,
    mut id: lopdf::ObjectId,
    key: &[u8],
) -> Result<Option<&'a Object>> {
    let mut visited = BTreeSet::new();
    loop {
        ensure!(
            visited.len() < 256 && visited.insert(id),
            "The PDF page inheritance tree is cyclic or too deep."
        );
        let dictionary = doc.get_dictionary(id)?;
        if let Ok(value) = dictionary.get(key) {
            return Ok(Some(doc.dereference(value)?.1));
        }
        match dictionary.get(b"Parent") {
            Ok(parent) => id = parent.as_reference().context("Invalid page parent")?,
            Err(_) => return Ok(None),
        }
    }
}

fn validate_box(object: &Object) -> Result<()> {
    let values = object.as_array()?;
    ensure!(values.len() == 4, "A page box must have four coordinates.");
    let numbers = values
        .iter()
        .map(number_value)
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        numbers.iter().all(|n| n.is_finite()) && numbers[2] > numbers[0] && numbers[3] > numbers[1],
        "A page box must have finite coordinates and positive dimensions."
    );
    Ok(())
}

fn page_dimensions(w: f32, h: f32, unit: f32, options: &Options) -> Result<(f32, f32, u32, u32)> {
    let width = w * unit;
    let height = h * unit;
    ensure!(
        width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0,
        "The page dimensions are invalid."
    );
    ensure!(
        width <= 14_400.0 && height <= 14_400.0,
        "This page exceeds this converter's 200-inch output page-size limit."
    );
    let scale = options.dpi as f32 / 72.0 * unit;
    let pw = (w * scale).floor();
    let ph = (h * scale).floor();
    ensure!(
        pw >= 1.0 && ph >= 1.0 && pw <= u16::MAX as f32 && ph <= u16::MAX as f32,
        "The page is too small or too large at this resolution."
    );
    ensure!(
        (pw as u64) * (ph as u64) <= options.max_page_pixels,
        "The page exceeds the {} million pixel limit at {} DPI. Choose a lower resolution; the app will not silently reduce it.",
        options.max_page_pixels as f64 / 1_000_000.0,
        options.dpi
    );
    Ok((width, height, pw as u32, ph as u32))
}

struct PdfWriter<'a> {
    file: &'a mut File,
    offsets: Vec<u64>,
}
impl<'a> PdfWriter<'a> {
    fn new(file: &'a mut File) -> Result<Self> {
        file.write_all(b"%PDF-1.6\n%\xE2\xE3\xCF\xD3\n")?;
        Ok(Self {
            file,
            offsets: vec![0; 3],
        })
    }
    fn reserve(&mut self) -> usize {
        self.offsets.push(0);
        self.offsets.len() - 1
    }
    fn position(&mut self) -> Result<u64> {
        Ok(self.file.stream_position()?)
    }
    fn begin(&mut self, id: usize) -> Result<()> {
        ensure!(self.offsets[id] == 0, "Internal duplicate PDF object.");
        self.offsets[id] = self.position()?;
        writeln!(self.file, "{id} 0 obj")?;
        Ok(())
    }
    fn object(&mut self, id: usize, body: &str) -> Result<()> {
        self.begin(id)?;
        writeln!(self.file, "{body}\nendobj")?;
        Ok(())
    }
    fn stream(&mut self, id: usize, entries: &str, bytes: &[u8]) -> Result<()> {
        self.begin(id)?;
        writeln!(self.file, "<< {entries} /Length {} >>\nstream", bytes.len())?;
        self.file.write_all(bytes)?;
        self.file.write_all(b"\nendstream\nendobj\n")?;
        Ok(())
    }
    fn finish(mut self, pages: &[usize]) -> Result<u64> {
        self.object(1, "<< /Type /Catalog /Pages 2 0 R >>")?;
        let kids = pages
            .iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        self.object(
            2,
            &format!("<< /Type /Pages /Count {} /Kids [{kids}] >>", pages.len()),
        )?;
        let start = self.position()?;
        writeln!(
            self.file,
            "xref\n0 {}\n0000000000 65535 f ",
            self.offsets.len()
        )?;
        for offset in &self.offsets[1..] {
            ensure!(
                *offset > 0 && *offset <= 9_999_999_999,
                "The output cannot use a classic PDF cross-reference table."
            );
            writeln!(self.file, "{offset:010} 00000 n ")?;
        }
        writeln!(
            self.file,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF",
            self.offsets.len()
        )?;
        Ok(start)
    }
}

fn write_search_text(
    writer: &mut PdfWriter<'_>,
    words: &[crate::ocr::OcrWord],
    width: f32,
    height: f32,
    pixel_w: u32,
    pixel_h: u32,
) -> Result<(usize, String)> {
    let mut chars: BTreeSet<char> = words
        .iter()
        .flat_map(|word| word.text.chars())
        .filter(|c| !c.is_control())
        .collect();
    chars.insert(' ');
    ensure!(
        chars.len() <= 255,
        "Text recognition produced too many distinct characters on one page. Turn off Searchable text to make an image-only copy."
    );
    let codes: BTreeMap<char, u8> = chars
        .iter()
        .enumerate()
        .map(|(i, ch)| (*ch, (i + 1) as u8))
        .collect();
    let font = writer.reserve();
    let cmap = writer.reserve();
    let glyph = writer.reserve();
    writer.stream(glyph, "", b"1000 0 d0\n")?;
    let mut mapping = format!(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /FlattenerUnicode{font} def\n/CMapType 2 def\n1 begincodespacerange\n<01> <FF>\nendcodespacerange\n",
    );
    let pairs: Vec<_> = codes.iter().collect();
    for chunk in pairs.chunks(100) {
        mapping.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (ch, code) in chunk {
            let utf16 = ch
                .encode_utf16(&mut [0; 2])
                .iter()
                .map(|c| format!("{c:04X}"))
                .collect::<String>();
            mapping.push_str(&format!("<{code:02X}> <{utf16}>\n"));
        }
        mapping.push_str("endbfchar\n");
    }
    mapping.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    writer.stream(cmap, "", mapping.as_bytes())?;
    let names = (1..=codes.len())
        .map(|n| format!("/g{n}"))
        .collect::<Vec<_>>()
        .join(" ");
    let procs = (1..=codes.len())
        .map(|n| format!("/g{n} {glyph} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    let widths = vec!["1000"; codes.len()].join(" ");
    writer.object(font, &format!("<< /Type /Font /Subtype /Type3 /Name /F0 /FontBBox [0 0 1000 1000] /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << {procs} >> /Encoding << /Type /Encoding /Differences [1 {names}] >> /FirstChar 1 /LastChar {} /Widths [{widths}] /Resources << >> /ToUnicode {cmap} 0 R >>", codes.len()))?;
    let mut content = String::new();
    for word in words {
        if ![word.left, word.top, word.right, word.bottom]
            .iter()
            .all(|n| n.is_finite())
        {
            continue;
        }
        let left = word.left.clamp(0.0, pixel_w as f32);
        let right = word.right.clamp(0.0, pixel_w as f32);
        let top = word.top.clamp(0.0, pixel_h as f32);
        let bottom = word.bottom.clamp(0.0, pixel_h as f32);
        let encoded: Vec<u8> = word
            .text
            .chars()
            .filter_map(|ch| codes.get(&ch).copied())
            .collect();
        if encoded.is_empty() || right <= left || bottom <= top {
            continue;
        }
        let x = left / pixel_w as f32 * width;
        let y = (1.0 - bottom / pixel_h as f32) * height;
        let size = (bottom - top) / pixel_h as f32 * height;
        let horizontal =
            ((right - left) / pixel_w as f32 * width) / (size * encoded.len() as f32) * 100.0;
        let mut text = encoded
            .iter()
            .map(|n| format!("{n:02X}"))
            .collect::<String>();
        text.push_str(&format!("{:02X}", codes[&' ']));
        content.push_str(&format!(
            "BT /F0 {size:.5} Tf 3 Tr {horizontal:.5} Tz 1 0 0 1 {x:.5} {y:.5} Tm <{text}> Tj ET\n"
        ));
    }
    Ok((font, content))
}

fn verify_output(path: &Path, pages: usize, xref: u64) -> Result<()> {
    let file = File::open(path).context("Cannot reopen the converted PDF")?;
    let file_size = file.metadata()?.len();
    ensure!(
        file_size <= MAX_OUTPUT_BYTES,
        "The completed output exceeds the 2 GiB limit."
    );
    let mut file = BufReader::new(file);
    let mut header = [0u8; 8];
    file.read_exact(&mut header)?;
    ensure!(
        &header == b"%PDF-1.6",
        "Output verification failed: wrong PDF version."
    );
    file.seek(SeekFrom::Start(xref))?;
    let mut marker = [0u8; 5];
    file.read_exact(&mut marker)?;
    ensure!(
        &marker == b"xref\n",
        "Output verification failed: no classic cross-reference table."
    );
    // This validates the deliberately small grammar emitted by PdfWriter.
    // Reading every compressed image into a general PDF parser here would undo
    // the page-at-a-time memory benefit on large engineering documents.
    let subsection = verified_line(&mut file)?;
    let mut fields = subsection.split_whitespace();
    ensure!(
        fields.next() == Some("0"),
        "Output verification failed: invalid xref subsection."
    );
    let count: usize = fields.next().context("Missing xref count")?.parse()?;
    ensure!(
        fields.next().is_none() && (3..=MAX_PAGES * 6 + 3).contains(&count),
        "Output verification failed: invalid object count."
    );
    ensure!(
        verified_line(&mut file)? == "0000000000 65535 f \n",
        "Output verification failed: invalid free xref entry."
    );
    let mut offsets = Vec::with_capacity(count - 1);
    let mut unique_offsets = BTreeSet::new();
    for id in 1..count {
        let line = verified_line(&mut file)?;
        let parts: Vec<_> = line.split_whitespace().collect();
        ensure!(
            parts.len() == 3 && parts[1] == "00000" && parts[2] == "n",
            "Output verification failed: malformed xref entry."
        );
        let offset: u64 = parts[0].parse()?;
        ensure!(
            offset >= 15 && offset < xref && unique_offsets.insert(offset),
            "Output verification failed: duplicate or invalid object offset."
        );
        offsets.push((id, offset));
    }
    ensure!(
        verified_line(&mut file)? == "trailer\n",
        "Output verification failed: missing trailer."
    );
    ensure!(
        verified_line(&mut file)? == format!("<< /Size {count} /Root 1 0 R >>\n"),
        "Output verification failed: invalid document root."
    );
    ensure!(
        verified_line(&mut file)? == "startxref\n",
        "Output verification failed: missing xref pointer."
    );
    ensure!(
        verified_line(&mut file)?.trim() == xref.to_string(),
        "Output verification failed: wrong xref pointer."
    );
    ensure!(
        verified_line(&mut file)? == "%%EOF\n",
        "Output verification failed: missing end marker."
    );
    ensure!(
        file.stream_position()? == file_size,
        "Output verification failed: unexpected trailing data."
    );
    let mut page_objects = BTreeSet::new();
    let mut tree = String::new();
    for (id, offset) in offsets {
        file.seek(SeekFrom::Start(offset))?;
        ensure!(
            verified_line(&mut file)? == format!("{id} 0 obj\n"),
            "Output verification failed: xref object mismatch."
        );
        let body = verified_line(&mut file)?;
        ensure!(
            body.starts_with("<< ") && body.ends_with(" >>\n"),
            "Output verification failed: invalid object dictionary."
        );
        if id == 1 {
            ensure!(
                body == "<< /Type /Catalog /Pages 2 0 R >>\n",
                "Output verification failed: invalid catalog."
            );
        }
        if id == 2 {
            tree = body.clone();
        }
        if body.starts_with("<< /Type /Page /Parent 2 0 R ") {
            page_objects.insert(id);
            let media = body
                .split_once("/MediaBox [")
                .and_then(|(_, tail)| tail.split_once(']'))
                .map(|(media, _)| media)
                .context("Output verification failed: missing page dimensions")?;
            let numbers = media
                .split_whitespace()
                .map(str::parse::<f32>)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ensure!(
                numbers.len() == 4
                    && numbers[0] == 0.0
                    && numbers[1] == 0.0
                    && numbers[2..]
                        .iter()
                        .all(|n| n.is_finite() && *n > 0.0 && *n <= 14_400.0),
                "Output verification failed: invalid physical page dimensions."
            );
        }
        if let Some((_, length)) = body.rsplit_once(" /Length ") {
            let length: u64 = length.trim_end_matches(" >>\n").parse()?;
            ensure!(
                verified_line(&mut file)? == "stream\n",
                "Output verification failed: missing stream marker."
            );
            let end = file
                .stream_position()?
                .checked_add(length)
                .context("Invalid stream length")?;
            ensure!(
                end < xref,
                "Output verification failed: stream exceeds object data."
            );
            file.seek(SeekFrom::Start(end))?;
            ensure!(
                verified_line(&mut file)? == "\n" && verified_line(&mut file)? == "endstream\n",
                "Output verification failed: truncated stream."
            );
        }
        ensure!(
            verified_line(&mut file)? == "endobj\n",
            "Output verification failed: missing object terminator."
        );
        let boundary = unique_offsets
            .range((
                std::ops::Bound::Excluded(offset),
                std::ops::Bound::Unbounded,
            ))
            .next()
            .copied()
            .unwrap_or(xref);
        ensure!(
            file.stream_position()? == boundary,
            "Output verification failed: an object is truncated or overlaps another object."
        );
    }
    ensure!(
        page_objects.len() == pages,
        "Output verification failed: page object count differs."
    );
    let prefix = format!("<< /Type /Pages /Count {pages} /Kids [");
    let kids = tree
        .strip_prefix(&prefix)
        .and_then(|s| s.strip_suffix("] >>\n"))
        .context("Output verification failed: invalid page tree")?;
    let tokens: Vec<_> = kids.split_whitespace().collect();
    ensure!(
        tokens.len() == pages * 3,
        "Output verification failed: incorrect page tree length."
    );
    let mut listed = BTreeSet::new();
    for kid in tokens.chunks_exact(3) {
        let id: usize = kid[0].parse()?;
        ensure!(
            kid[1] == "0" && kid[2] == "R" && page_objects.contains(&id) && listed.insert(id),
            "Output verification failed: invalid or repeated page reference."
        );
    }
    Ok(())
}

fn verified_line(reader: &mut BufReader<File>) -> Result<String> {
    const MAX_LINE: usize = 1024 * 1024;
    let mut bytes = Vec::new();
    (&mut *reader)
        .take((MAX_LINE + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    ensure!(
        bytes.len() <= MAX_LINE && bytes.ends_with(b"\n"),
        "Output verification failed: incomplete or oversized structural line."
    );
    String::from_utf8(bytes).context("Output verification failed: invalid structural text")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_layer_round_trips_accents_and_unicode_with_word_spacing() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut writer = PdfWriter::new(file.as_file_mut()).unwrap();
        let words = vec![
            crate::ocr::OcrWord {
                text: "R\u{e9}sum\u{e9}".into(),
                left: 10.0,
                top: 10.0,
                right: 70.0,
                bottom: 30.0,
            },
            crate::ocr::OcrWord {
                text: "\u{b5}\u{3a9}\u{1f600}".into(),
                left: 80.0,
                top: 10.0,
                right: 110.0,
                bottom: 30.0,
            },
        ];
        let (font, text) = write_search_text(&mut writer, &words, 200.0, 100.0, 200, 100).unwrap();
        let content = writer.reserve();
        let page = writer.reserve();
        writer.stream(content, "", text.as_bytes()).unwrap();
        writer.object(page, &format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F0 {font} 0 R >> >> /Contents {content} 0 R >>")).unwrap();
        let second_words = vec![crate::ocr::OcrWord {
            text: "ZEBRA".into(),
            left: 10.0,
            top: 10.0,
            right: 60.0,
            bottom: 30.0,
        }];
        let (font2, text2) =
            write_search_text(&mut writer, &second_words, 200.0, 100.0, 200, 100).unwrap();
        let content2 = writer.reserve();
        let page2 = writer.reserve();
        writer.stream(content2, "", text2.as_bytes()).unwrap();
        writer.object(page2, &format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F0 {font2} 0 R >> >> /Contents {content2} 0 R >>")).unwrap();
        let xref = writer.finish(&[page, page2]).unwrap();
        verify_output(file.path(), 2, xref).unwrap();
        let mut doc = Document::load(file.path()).unwrap();
        // PDF text extraction must prefer ToUnicode. lopdf 0.44 instead gives
        // Encoding precedence, and cannot decode private Type3 glyph names.
        // Assert the original encoding is complete, then remove it only from
        // this in-memory extraction view to exercise its real CMap decoder.
        // Acrobat 7 independently extracts this unmodified two-page fixture.
        for font_id in [font, font2] {
            let dict = doc.get_dictionary_mut((font_id as u32, 0)).unwrap();
            let count = dict.get(b"LastChar").unwrap().as_i64().unwrap() as usize;
            let encoding = dict.get(b"Encoding").unwrap().as_dict().unwrap();
            let differences = encoding.get(b"Differences").unwrap().as_array().unwrap();
            assert_eq!(differences.len(), count + 1);
            assert_eq!(differences[0].as_i64().unwrap(), 1);
            let procs = dict.get(b"CharProcs").unwrap().as_dict().unwrap();
            for (index, name) in differences.iter().skip(1).enumerate() {
                let name = name.as_name().unwrap();
                assert_eq!(name, format!("g{}", index + 1).as_bytes());
                assert!(procs.has(name));
            }
            assert_eq!(
                dict.get(b"Widths").unwrap().as_array().unwrap().len(),
                count
            );
            assert!(dict.has(b"ToUnicode"));
            dict.remove(b"Encoding");
        }
        let extracted = doc.extract_text(&[1]).unwrap();
        assert!(extracted.contains("R\u{e9}sum\u{e9} "), "{extracted:?}");
        assert!(
            extracted.contains("\u{b5}\u{3a9}\u{1f600} "),
            "{extracted:?}"
        );
        assert!(doc.extract_text(&[2]).unwrap().contains("ZEBRA "));
    }

    #[test]
    fn strict_zlib_validation_rejects_checksum_damage_and_truncation() {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"0 0 100 100 re f").unwrap();
        let compressed = encoder.finish().unwrap();
        validate_zlib(&compressed).unwrap();
        let mut bad_checksum = compressed.clone();
        *bad_checksum.last_mut().unwrap() ^= 0xFF;
        assert!(validate_zlib(&bad_checksum).is_err());
        assert!(validate_zlib(&compressed[..compressed.len() - 3]).is_err());
        assert!(validate_zlib(b"not a compressed stream").is_err());
        let empty = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default())
            .finish()
            .unwrap();
        validate_zlib(&empty).unwrap();
        let mut large = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        large.write_all(&[b' '; 200_000]).unwrap();
        validate_zlib(&large.finish().unwrap()).unwrap();
    }

    #[test]
    fn output_verifier_rejects_incorrect_stream_length() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut writer = PdfWriter::new(file.as_file_mut()).unwrap();
        let stream = writer.reserve();
        let page = writer.reserve();
        writer.stream(stream, "", b"abcdef").unwrap();
        writer
            .object(
                page,
                &format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents {stream} 0 R >>"
                ),
            )
            .unwrap();
        let xref = writer.finish(&[page]).unwrap();
        verify_output(file.path(), 1, xref).unwrap();
        let mut bytes = std::fs::read(file.path()).unwrap();
        let position = bytes
            .windows(9)
            .position(|bytes| bytes == b"/Length 6")
            .unwrap();
        bytes[position + 8] = b'9';
        file.as_file_mut().seek(SeekFrom::Start(0)).unwrap();
        file.as_file_mut().write_all(&bytes).unwrap();
        assert!(verify_output(file.path(), 1, xref).is_err());
    }
}
