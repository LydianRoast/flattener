//! Create the checked-in PNG and Windows ICO from the original square artwork.
//! Usage: cargo run --example package_icon -- input.png assets/icon

use std::error::Error;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use image::{ImageFormat, Rgba, Rgba32FImage, RgbaImage, imageops};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let source = PathBuf::from(args.next().ok_or("provide the source PNG path")?);
    let destination = PathBuf::from(args.next().ok_or("provide the output directory")?);
    if args.next().is_some() {
        return Err("usage: package_icon input.png output-directory".into());
    }
    let original = image::open(&source)?.to_rgba8();
    if original.width() == 0 || original.height() == 0 {
        return Err("source artwork has no pixels".into());
    }
    // Filter premultiplied colour so transparent edges do not acquire a dark halo.
    let premultiplied = Rgba32FImage::from_fn(original.width(), original.height(), |x, y| {
        let pixel = original.get_pixel(x, y).0;
        let alpha = f32::from(pixel[3]) / 255.0;
        Rgba([
            f32::from(pixel[0]) / 255.0 * alpha,
            f32::from(pixel[1]) / 255.0 * alpha,
            f32::from(pixel[2]) / 255.0 * alpha,
            alpha,
        ])
    });
    fs::create_dir_all(&destination)?;
    let mut entries = Vec::new();
    for size in [16, 20, 24, 32, 40, 48, 64, 128, 256] {
        let scale = size as f64 / original.width().max(original.height()) as f64;
        let width = ((original.width() as f64 * scale).round() as u32).max(1);
        let height = ((original.height() as f64 * scale).round() as u32).max(1);
        let filtered = imageops::resize(
            &premultiplied,
            width,
            height,
            imageops::FilterType::Lanczos3,
        );
        let resized = RgbaImage::from_fn(width, height, |x, y| {
            let pixel = filtered.get_pixel(x, y).0;
            let alpha = pixel[3].clamp(0.0, 1.0);
            if alpha <= f32::EPSILON {
                return Rgba([0, 0, 0, 0]);
            }
            Rgba([
                ((pixel[0] / alpha).clamp(0.0, 1.0) * 255.0).round() as u8,
                ((pixel[1] / alpha).clamp(0.0, 1.0) * 255.0).round() as u8,
                ((pixel[2] / alpha).clamp(0.0, 1.0) * 255.0).round() as u8,
                (alpha * 255.0).round() as u8,
            ])
        });
        let mut square = RgbaImage::new(size, size);
        imageops::overlay(
            &mut square,
            &resized,
            i64::from((size - width) / 2),
            i64::from((size - height) / 2),
        );
        let mut encoded = Cursor::new(Vec::new());
        square.write_to(&mut encoded, ImageFormat::Png)?;
        let png = encoded.into_inner();
        if size == 256 {
            fs::write(destination.join("flattener.png"), &png)?;
        }
        entries.push((size, png));
    }
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut offset = 6 + entries.len() as u32 * 16;
    for (size, png) in &entries {
        let dimension = if *size == 256 { 0 } else { *size as u8 };
        ico.extend_from_slice(&[dimension, dimension, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in entries {
        ico.extend_from_slice(&png);
    }
    fs::write(destination.join("flattener.ico"), ico)?;
    println!("Created flattener.png and nine-resolution flattener.ico.");
    Ok(())
}
