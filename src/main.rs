#![cfg_attr(windows, windows_subsystem = "windows")]
use anyhow::{Context, Result, bail};
use flattener::engine::Options;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let child = args.first().is_some_and(|a| a == "--worker");
    let cli = args.first().is_some_and(|a| {
        a == "convert"
            || a.to_str()
                .is_some_and(|value| value.starts_with('-') && value != "--worker")
    });
    #[cfg(windows)]
    if cli {
        // SAFETY: this attaches only to the invoking process's console, when present.
        // Worker calls retain their redirected pipes and never use AttachConsole.
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }
    if let Err(error) = run(args) {
        if !child {
            eprintln!("Flattener: {error:#}");
        }
        #[cfg(feature = "gui")]
        if !child && !cli {
            rfd::MessageDialog::new()
                .set_title("Flattener")
                .set_description(format!("Cannot start Flattener.\n\n{error:#}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        std::process::exit(1);
    }
}
fn run(args: Vec<OsString>) -> Result<()> {
    match args.first().and_then(|s| s.to_str()) {
        Some("--worker") if args.len() == 1 => flattener::worker::run_child(),
        Some("--help" | "-h") => {
            println!(
                "Flattener {}\n\nCreate flattened PDF 1.6 copies for Acrobat 7.\n\nUsage:\n  flattener [PDF ...]\n  flattener convert INPUT.pdf [--output COPY.pdf] [--dpi 72..600]\n      [--quality 50..100] [--lossless] [--no-ocr]\n  flattener --licenses\n\nExisting files are never replaced. OCR uses Latin-script models.\nLarge pages may need a lower DPI to stay within the 40-megapixel page limit.",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        Some("--version" | "-V") => {
            println!("Flattener {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--licenses") => {
            println!("{}", flattener::THIRD_PARTY_LICENSES);
            Ok(())
        }
        Some("convert") => convert(&args[1..]),
        Some(v) if v.starts_with('-') => bail!("Unknown option. Use --help for usage."),
        _ => {
            #[cfg(feature = "gui")]
            {
                flattener::ui::run(args.into_iter().map(PathBuf::from).collect())
                    .map_err(|e| anyhow::anyhow!("Cannot open the application: {e}"))
            }
            #[cfg(not(feature = "gui"))]
            {
                bail!("This build has no desktop interface. Use convert INPUT.pdf or --help.")
            }
        }
    }
}
fn convert(args: &[OsString]) -> Result<()> {
    let input = Path::new(
        args.first()
            .context("Choose an input PDF. Use --help for usage.")?,
    );
    let mut stem = input.file_stem().unwrap_or_default().to_os_string();
    stem.push("_Acrobat7.pdf");
    let mut output = input.with_file_name(stem);
    let mut options = Options::default();
    let mut index = 1;
    while index < args.len() {
        let arg = args[index]
            .to_str()
            .context("An option name is not valid text.")?;
        match arg {
            "--no-ocr" => options.searchable = false,
            "--lossless" => options.lossless = true,
            "--output" | "--dpi" | "--quality" => {
                index += 1;
                let value = args.get(index).context("An option value is missing.")?;
                match arg {
                    "--output" => output = PathBuf::from(value),
                    "--dpi" => {
                        options.dpi = value
                            .to_str()
                            .context("DPI must be a number.")?
                            .parse()
                            .context("DPI must be a number.")?
                    }
                    _ => {
                        options.jpeg_quality = value
                            .to_str()
                            .context("Quality must be a number.")?
                            .parse()
                            .context("Quality must be a number.")?
                    }
                }
            }
            _ => bail!("Unknown conversion option: {arg}"),
        }
        index += 1;
    }
    let report =
        flattener::worker::convert(input, &output, &options, &AtomicBool::new(false), |p| {
            eprintln!("{}/{}: {}", p.page, p.total_pages, p.message)
        })?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
