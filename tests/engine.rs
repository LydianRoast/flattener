use flattener::engine::{Options, convert};
use lopdf::{Document, Object, Stream, dictionary};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

fn options() -> Options {
    Options {
        dpi: 72,
        lossless: true,
        searchable: false,
        ..Default::default()
    }
}

fn fixture(path: &Path, rotate: i64, crop: bool, annotations: bool, user_unit: f32) {
    let mut doc = Document::with_version("2.0");
    let pages = doc.new_object_id();
    let stream = doc.add_object(Stream::new(
        dictionary! {},
        b"0.5 g 0 0 144 72 re f\n1 0 0 rg 36 0 108 72 re f\n".to_vec(),
    ));
    let mut page = dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 144.into(), 72.into()],
        "Contents" => stream, "Resources" => dictionary! {}, "Rotate" => rotate,
        "UserUnit" => Object::Real(user_unit),
    };
    if crop {
        page.set("CropBox", vec![0.into(), 0.into(), 108.into(), 72.into()]);
    }
    if annotations {
        let appearance = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()],
                "Resources" => dictionary! {},
            },
            b"0 0 1 rg 0 0 20 20 re f".to_vec(),
        ));
        let annot = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Stamp", "F" => 4,
            "Rect" => vec![45.into(), 20.into(), 65.into(), 40.into()],
            "AP" => dictionary! { "N" => appearance },
        });
        page.set("Annots", vec![annot.into()]);
    }
    let page = doc.add_object(page);
    doc.objects.insert(
        pages,
        dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1}.into(),
    );
    let root = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
    doc.trailer.set("Root", root);
    doc.save(path).unwrap();
}

fn assert_no_temporary(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".flattener-")
        );
    }
}

#[test]
fn modern_input_becomes_opaque_rgb_classic_pdf_with_crop_rotation_and_annotation() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 90, true, true, 1.0);
    let original = fs::read(&input).unwrap();
    let report = convert(&input, &output, &options(), &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!((report.input_pages, report.output_pages), (1, 1));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(original, fs::read(&input).unwrap());
    let bytes = fs::read(&output).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.6"));
    assert!(bytes.windows(5).any(|w| w == b"xref\n"));
    let doc = Document::load(&output).unwrap();
    let page = doc
        .get_dictionary(*doc.get_pages().get(&1).unwrap())
        .unwrap();
    let media = page.get(b"MediaBox").unwrap().as_array().unwrap();
    assert_eq!(media[2].as_float().unwrap(), 72.0);
    assert_eq!(media[3].as_float().unwrap(), 108.0);
    assert!(!page.has(b"Rotate"));
    assert!(!page.has(b"Annots"));
    let image = doc
        .objects
        .values()
        .find_map(|o| match o {
            Object::Stream(s)
                if s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image") =>
            {
                Some(s)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        image.dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
        b"DeviceRGB"
    );
    assert!(!image.dict.has(b"SMask"));
    let pixels = image.decompressed_content().unwrap();
    assert_eq!(pixels.len(), 72 * 108 * 3);
    assert!(
        pixels
            .chunks_exact(3)
            .any(|rgb| rgb[2] > 240 && rgb[0] < 10 && rgb[1] < 10),
        "Blue annotation appearance must survive"
    );
    assert!(
        pixels
            .chunks_exact(3)
            .any(|rgb| rgb[0] > 240 && rgb[1] < 10 && rgb[2] < 10)
    );
    assert!(
        pixels
            .chunks_exact(3)
            .any(|rgb| rgb[0] > 100 && rgb[0] < 160 && rgb[0] == rgb[1] && rgb[1] == rgb[2])
    );
    assert_no_temporary(dir.path());
}

#[test]
fn preserves_physical_user_unit_and_jpeg_profile() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, false, 2.0);
    let opts = Options {
        lossless: false,
        ..options()
    };
    convert(&input, &output, &opts, &AtomicBool::new(false), |_| {}).unwrap();
    let doc = Document::load(&output).unwrap();
    let page = doc
        .get_dictionary(*doc.get_pages().get(&1).unwrap())
        .unwrap();
    let media = page.get(b"MediaBox").unwrap().as_array().unwrap();
    assert_eq!(media[2].as_float().unwrap(), 288.0);
    assert_eq!(media[3].as_float().unwrap(), 144.0);
    assert!(doc.objects.values().any(|o| matches!(o, Object::Stream(s) if s.dict.get(b"Filter").and_then(Object::as_name).ok() == Some(b"DCTDecode"))));
}

#[test]
fn refuses_existing_destination_and_same_source() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, false, 1.0);
    let bytes = fs::read(&input).unwrap();
    fs::write(&output, b"keep me").unwrap();
    assert!(convert(&input, &output, &options(), &AtomicBool::new(false), |_| {}).is_err());
    assert!(convert(&input, &input, &options(), &AtomicBool::new(false), |_| {}).is_err());
    assert_eq!(bytes, fs::read(&input).unwrap());
    assert_eq!(fs::read(&output).unwrap(), b"keep me");
    assert_no_temporary(dir.path());
}

#[test]
fn cancellation_before_and_after_render_leaves_no_partial_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, false, 1.0);
    assert!(convert(&input, &output, &options(), &AtomicBool::new(true), |_| {}).is_err());
    let cancel = AtomicBool::new(false);
    let result = convert(&input, &output, &options(), &cancel, |event| {
        if event.page == 1 {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert!(!output.exists());
    assert_no_temporary(dir.path());
}

#[test]
fn atomic_publication_does_not_replace_a_destination_created_during_render() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, false, 1.0);
    let result = convert(
        &input,
        &output,
        &options(),
        &AtomicBool::new(false),
        |event| {
            if event.page == 1 && !output.exists() {
                fs::write(&output, b"other process").unwrap();
            }
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&output).unwrap(), b"other process");
    assert_no_temporary(dir.path());
}

#[test]
fn invalid_input_and_pixel_limit_fail_without_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fs::write(&input, b"%PDF-2.0\ncorrupt").unwrap();
    assert!(convert(&input, &output, &options(), &AtomicBool::new(false), |_| {}).is_err());
    fixture(&input, 0, false, false, 1.0);
    let opts = Options {
        max_page_pixels: 10,
        ..options()
    };
    assert!(
        format!(
            "{:#}",
            convert(&input, &output, &opts, &AtomicBool::new(false), |_| {}).unwrap_err()
        )
        .contains("pixel limit")
    );
    assert!(!output.exists());
    assert_no_temporary(dir.path());
}

#[test]
fn rejects_xfa_and_portfolios_without_pretending_they_are_flat_pages() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    for key in ["XFA", "Collection"] {
        fixture(&input, 0, false, false, 1.0);
        let mut doc = Document::load(&input).unwrap();
        if key == "XFA" {
            doc.catalog_mut().unwrap().set(
                "AcroForm",
                dictionary! {"XFA" => Object::string_literal("dynamic form")},
            );
        } else {
            doc.catalog_mut().unwrap().set("Collection", dictionary! {});
        }
        doc.save(&input).unwrap();
        assert!(
            format!(
                "{:#}",
                convert(&input, &output, &options(), &AtomicBool::new(false), |_| {}).unwrap_err()
            )
            .contains(if key == "XFA" { "XFA" } else { "portfolios" })
        );
        assert!(!output.exists());
        assert_no_temporary(dir.path());
    }
}

#[test]
fn missing_annotation_appearance_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, true, 1.0);
    let mut doc = Document::load(&input).unwrap();
    for obj in doc.objects.values_mut() {
        if let Object::Dictionary(dict) = obj
            && dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Annot")
        {
            dict.remove(b"AP");
        }
    }
    doc.save(&input).unwrap();
    let report = convert(&input, &output, &options(), &AtomicBool::new(false), |_| {}).unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("no saved appearance"))
    );
}

#[test]
fn encrypted_document_with_empty_user_password_is_still_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, false, 1.0);
    let mut doc = Document::load(&input).unwrap();
    let file_id = Object::String(
        b"0123456789ABCDEF".to_vec(),
        lopdf::StringFormat::Hexadecimal,
    );
    doc.trailer
        .set("ID", Object::Array(vec![file_id.clone(), file_id]));
    let encryption = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner",
        user_password: "",
        key_length: 128,
        permissions: lopdf::Permissions::all(),
    })
    .unwrap();
    doc.encrypt(&encryption).unwrap();
    doc.save(&input).unwrap();
    let error = convert(&input, &output, &options(), &AtomicBool::new(false), |_| {}).unwrap_err();
    assert!(format!("{error:#}").contains("Encrypted"));
    assert!(!output.exists());
    assert_no_temporary(dir.path());
}

#[test]
fn blank_page_is_white_and_has_no_transparency() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, false, 1.0);
    let mut doc = Document::load(&input).unwrap();
    let id = *doc.get_pages().get(&1).unwrap();
    doc.change_page_content(id, Vec::new()).unwrap();
    doc.save(&input).unwrap();
    convert(&input, &output, &options(), &AtomicBool::new(false), |_| {}).unwrap();
    let doc = Document::load(&output).unwrap();
    let pixels = doc
        .objects
        .values()
        .find_map(|obj| match obj {
            Object::Stream(stream)
                if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image") =>
            {
                Some(stream.decompressed_content().unwrap())
            }
            _ => None,
        })
        .unwrap();
    assert!(pixels.iter().all(|byte| *byte == 255));
}

#[test]
fn blank_page_with_ocr_reports_no_actual_searchable_text() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, false, 1.0);
    let mut doc = Document::load(&input).unwrap();
    let id = *doc.get_pages().get(&1).unwrap();
    doc.change_page_content(id, Vec::new()).unwrap();
    doc.save(&input).unwrap();
    let report = convert(
        &input,
        &output,
        &Options {
            searchable: true,
            ..options()
        },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(report.recognized_words, 0);
    assert!(!report.searchable);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("no searchable text"))
    );
}

#[test]
fn input_size_limit_is_checked_before_allocating_the_file() {
    // Keep the large sparse/length-only fixture with Cargo's build artifacts,
    // rather than consuming the operating system's temporary drive space.
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(scratch).unwrap();
    let dir = tempfile::tempdir_in(scratch).unwrap();
    let input = dir.path().join("oversize.pdf");
    let output = dir.path().join("copy.pdf");
    fs::File::create(&input)
        .unwrap()
        .set_len(512 * 1024 * 1024 + 1)
        .unwrap();
    let error = convert(&input, &output, &options(), &AtomicBool::new(false), |_| {}).unwrap_err();
    assert!(error.to_string().contains("512 MiB"));
    assert!(!output.exists());
    assert_no_temporary(dir.path());
}

#[test]
fn modern_object_streams_inherited_crop_rotation_and_fractional_pages_keep_order() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, false, 1.0);
    let mut doc = Document::load(&input).unwrap();
    let first = *doc.get_pages().get(&1).unwrap();
    let parent = doc
        .get_dictionary(first)
        .unwrap()
        .get(b"Parent")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(first).unwrap().remove(b"Rotate");
    let mut second_page = doc.get_dictionary(first).unwrap().clone();
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"0 1 0 rg 0 0 144 72 re f".to_vec(),
    ));
    second_page.set("Contents", content);
    let second = doc.add_object(second_page);
    let tree = doc.get_dictionary_mut(parent).unwrap();
    tree.set("Kids", vec![first.into(), second.into()]);
    tree.set("Count", 2);
    tree.set("Rotate", 90);
    tree.set(
        "CropBox",
        vec![
            Object::Real(0.25),
            Object::Real(1.5),
            Object::Real(107.75),
            Object::Real(71.5),
        ],
    );
    let mut file = fs::File::create(&input).unwrap();
    doc.save_modern(&mut file).unwrap();
    drop(file);
    assert!(String::from_utf8_lossy(&fs::read(&input).unwrap()).contains("/ObjStm"));
    let mut completed = Vec::new();
    let report = convert(
        &input,
        &output,
        &Options {
            dpi: 150,
            ..options()
        },
        &AtomicBool::new(false),
        |p| {
            if p.message.starts_with("Completed page") {
                completed.push(p.page);
            }
        },
    )
    .unwrap();
    assert_eq!(report.output_pages, 2);
    assert_eq!(completed, vec![1, 2]);
    let converted = Document::load(&output).unwrap();
    for (number, id) in converted.get_pages() {
        let page = converted.get_dictionary(id).unwrap();
        let media = page.get(b"MediaBox").unwrap().as_array().unwrap();
        assert_eq!(media[2].as_float().unwrap(), 70.0);
        assert_eq!(media[3].as_float().unwrap(), 107.5);
        let image_ref = page
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"XObject")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Im0")
            .unwrap()
            .as_reference()
            .unwrap();
        let image = converted
            .get_object(image_ref)
            .unwrap()
            .as_stream()
            .unwrap();
        assert_eq!(image.dict.get(b"Width").unwrap().as_i64().unwrap(), 145);
        assert_eq!(image.dict.get(b"Height").unwrap().as_i64().unwrap(), 223);
        let pixels = image.decompressed_content().unwrap();
        if number == 2 {
            assert!(pixels.chunks_exact(3).all(|p| p == [0, 255, 0]));
        } else {
            assert!(pixels.chunks_exact(3).any(|p| p[0] > 240 && p[1] < 10));
        }
    }
}

#[test]
fn corrupt_compressed_page_content_is_not_reported_as_a_successful_blank_page() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, false, 1.0);
    let mut doc = Document::load(&input).unwrap();
    let page = *doc.get_pages().get(&1).unwrap();
    let content = doc.get_page_contents(page)[0];
    let stream = doc
        .get_object_mut(content)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    stream.set_content(b"not a compressed stream".to_vec());
    stream.dict.set("Filter", "FlateDecode");
    doc.save(&input).unwrap();
    assert!(convert(&input, &output, &options(), &AtomicBool::new(false), |_| {}).is_err());
    assert!(!output.exists());
    assert_no_temporary(dir.path());
}

#[test]
fn undecodable_source_image_is_reported_per_page() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("copy.pdf");
    fixture(&input, 0, false, false, 1.0);
    let mut doc = Document::load(&input).unwrap();
    let page = *doc.get_pages().get(&1).unwrap();
    let image = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 16, "Height" => 16,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode",
        },
        b"invalid jpeg bytes".to_vec(),
    ));
    doc.get_dictionary_mut(page).unwrap().set(
        "Resources",
        dictionary! {"XObject" => dictionary! {"BadImage" => image}},
    );
    doc.change_page_content(page, b"q 50 0 0 50 10 10 cm /BadImage Do Q".to_vec())
        .unwrap();
    doc.save(&input).unwrap();
    let report = convert(&input, &output, &options(), &AtomicBool::new(false), |_| {}).unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.starts_with("Page 1:")
                && warning.contains("image failed to decode")),
        "{:?}",
        report.warnings
    );
}
