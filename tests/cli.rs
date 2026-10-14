//! Exercise the real parent/worker process boundary, including publication.
use flattener::engine::Report;
use lopdf::{Document, Stream, dictionary};
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn fixture(path: &Path) {
    let mut doc = Document::with_version("2.0");
    let pages = doc.new_object_id();
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"0.1 0.4 0.8 rg 10 10 50 30 re f".to_vec(),
    ));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 120.into(), 100.into()],
        "Resources" => dictionary! {}, "Contents" => content,
    });
    doc.objects.insert(
        pages,
        dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1}.into(),
    );
    let root = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
    doc.trailer.set("Root", root);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    doc.save_modern(&mut file).unwrap();
}

fn run(input: &Path, output: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_flattener"));
    command
        .arg("convert")
        .arg(input)
        .arg("--output")
        .arg(output)
        .args(["--no-ocr", "--dpi", "72", "--lossless"]);
    run_command(command)
}

fn run_command(mut command: Command) -> Output {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("The CLI did not exit within 60 seconds");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn cli_unknown_option_exits_with_help_without_a_dialog() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_flattener"));
    command.arg("--bad-option");
    let result = run_command(command);
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("Unknown option"), "{error}");
    assert!(error.contains("--help"), "{error}");
}

fn no_staging_entries(directory: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".flattener")
        );
    }
}

#[test]
fn cli_converts_unicode_paths_through_an_isolated_worker_and_reports_json() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory
        .path()
        .join("R\u{e9}sum\u{e9}-\u{6e2c}\u{8a66} source.pdf");
    let output = directory
        .path()
        .join("Converted \u{00fc}-\u{6e2c}\u{8a66}.pdf");
    fixture(&input);
    let original = fs::read(&input).unwrap();
    let result = run(&input, &output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Report = serde_json::from_slice(&result.stdout).unwrap_or_else(|error| {
        panic!(
            "CLI output is not its JSON report: {error}; stdout={:?}; stderr={:?}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
    });
    assert_eq!((report.input_pages, report.output_pages), (1, 1));
    assert!(!report.searchable);
    assert_eq!(report.output_bytes, fs::metadata(&output).unwrap().len());
    assert_eq!(original, fs::read(&input).unwrap());
    let doc = Document::load(&output).unwrap();
    assert_eq!(doc.version, "1.6");
    assert_eq!(doc.get_pages().len(), 1);
    no_staging_entries(directory.path());
}

#[test]
fn cli_preserves_existing_output_and_does_not_leave_staging() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("source.pdf");
    let output = directory.path().join("keep.pdf");
    fixture(&input);
    fs::write(&output, b"KEEP THIS FILE").unwrap();
    let result = run(&input, &output);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("already exists"));
    assert_eq!(fs::read(&output).unwrap(), b"KEEP THIS FILE");
    no_staging_entries(directory.path());
}

#[test]
fn cli_reports_invalid_source_and_cleans_its_worker_directory() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("damaged.pdf");
    let output = directory.path().join("copy.pdf");
    fs::write(&input, b"This is not a PDF").unwrap();
    let result = run(&input, &output);
    assert!(!result.status.success());
    assert!(!result.stderr.is_empty());
    assert!(!output.exists());
    no_staging_entries(directory.path());
}
