//! Generate a small, public, synthetic PDF for checking conversion in a viewer.
use anyhow::{Result, ensure};
use lopdf::{Document, Stream, dictionary};
use std::path::PathBuf;

fn main() -> Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "validation-source.pdf".into());
    ensure!(!output.exists(), "The fixture destination already exists");
    let mut doc = Document::with_version("2.0");
    let pages = doc.new_object_id();
    let font = doc.add_object(
        dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"},
    );
    let resources = dictionary! {"Font" => dictionary! {"F1" => font}};
    let content = doc.add_object(Stream::new(
        dictionary! {},
        br#"
q
1 1 1 rg 0 0 612 792 re f
0.04 0.14 0.27 rg 36 690 540 66 re f
1 1 1 rg BT /F1 28 Tf 52 714 Td (HELLO 123 ABC) Tj ET
0 0 0 rg BT /F1 18 Tf 40 650 Td (Flattener validation document) Tj ET
BT /F1 14 Tf 40 620 Td (Page 1: text, color, grayscale and a saved form appearance.) Tj ET
0.8 0.1 0.1 rg 40 450 140 100 re f
0.1 0.5 0.9 rg 200 450 140 100 re f
0.5 g 360 450 140 100 re f
0 0 0 RG 0.5 w 40 420 m 540 420 l S
BT /F1 12 Tf 40 390 Td (Search for HELLO, 123, ABC, and FIELD42 in the converted file.) Tj ET
BT /F1 12 Tf 40 370 Td (The converted pages should remain visibly identical at normal zoom.) Tj ET
Q
"#
        .to_vec(),
    ));
    let appearance = doc.add_object(Stream::new(dictionary! {
        "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 240.into(), 40.into()],
        "Resources" => resources.clone(),
    }, b"0.9 g 0 0 240 40 re f 0 g BT /F1 22 Tf 10 11 Td (FIELD42) Tj ET".to_vec()));
    let widget = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx", "T" => lopdf::Object::string_literal("SampleField"),
        "V" => lopdf::Object::string_literal("FIELD42"), "F" => 4,
        "Rect" => vec![40.into(), 280.into(), 280.into(), 320.into()], "AP" => dictionary! {"N" => appearance},
    });
    let page1 = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => resources.clone(), "Contents" => content, "Annots" => vec![widget.into()],
    });
    let content2 = doc.add_object(Stream::new(
        dictionary! {},
        br#"
q
0.1 0.5 0.9 rg 40 40 500 300 re f
1 1 1 rg BT /F1 26 Tf 70 280 Td (ROTATION 90) Tj ET
BT /F1 16 Tf 70 240 Td (Page 2 has a crop box and a rotated page.) Tj ET
0 0 0 RG 4 w 60 60 460 260 re S
Q
"#
        .to_vec(),
    ));
    let page2 = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 396.into()],
        "CropBox" => vec![30.into(), 30.into(), 570.into(), 360.into()], "Rotate" => 90,
        "Resources" => resources, "Contents" => content2,
    });
    doc.objects.insert(
        pages,
        dictionary! {"Type" => "Pages", "Kids" => vec![page1.into(), page2.into()], "Count" => 2}
            .into(),
    );
    let root = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages, "AcroForm" => dictionary! {"Fields" => vec![widget.into()]}});
    doc.trailer.set("Root", root);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    doc.save_modern(&mut file)?;
    file.sync_all()?;
    println!("Created {}", output.display());
    Ok(())
}
