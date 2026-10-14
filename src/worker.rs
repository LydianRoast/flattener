//! Isolated conversion with parent-owned cancellation, timeouts and output publication.
use crate::engine::{Options, Progress, Report};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub input: PathBuf,
    pub output: PathBuf,
    pub options: Options,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    Progress(Progress),
    Complete(Report),
    Error(String),
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub fn convert(
    input: &Path,
    output: &Path,
    options: &Options,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<Report> {
    ensure!(!cancel.load(Ordering::Relaxed), "Conversion cancelled.");
    ensure!(
        output.file_name().is_some()
            && output
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("pdf")),
        "Choose an output filename ending in .pdf."
    );
    ensure!(
        (72..=600).contains(&options.dpi),
        "Resolution must be between 72 and 600 DPI."
    );
    ensure!(
        (50..=100).contains(&options.jpeg_quality),
        "JPEG quality must be between 50 and 100."
    );
    ensure!(
        !output.exists(),
        "The output already exists. Choose another filename."
    );
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    ensure!(parent.is_dir(), "The output folder does not exist.");
    // The parent owns this directory even if the renderer must be forcibly stopped.
    let workspace = tempfile::Builder::new()
        .prefix(".flattener-job-")
        .tempdir_in(parent)
        .context("Cannot create temporary output. Check free space and folder permissions.")?;
    let staged = workspace.path().join("converted.pdf");
    let mut command =
        Command::new(std::env::current_exe().context("Cannot locate the application.")?);
    command
        .arg("--worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = OwnedChild(
        command
            .spawn()
            .context("Cannot start the conversion worker.")?,
    );
    #[cfg(windows)]
    let mut job = Some(Job::attach(&child.0).context("Cannot isolate the conversion worker.")?);
    let mut stdin = child
        .0
        .stdin
        .take()
        .context("Worker input is unavailable.")?;
    serde_json::to_writer(
        &mut stdin,
        &Request {
            input: input.to_owned(),
            output: staged.clone(),
            options: options.clone(),
        },
    )?;
    stdin.write_all(b"\n")?;
    drop(stdin);
    let stdout = child
        .0
        .stdout
        .take()
        .context("Worker output is unavailable.")?;
    let stderr = child
        .0
        .stderr
        .take()
        .context("Worker diagnostics are unavailable.")?;
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("conversion-events".into())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_event(&mut reader) {
                    Ok(Some(event)) => {
                        if tx.send(Ok(event)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        break;
                    }
                }
            }
        })
        .context("Cannot start the conversion progress reader.")?;
    // Continuously drain stderr; save only a bounded diagnostic prefix.
    let (diagnostic_tx, diagnostic_rx) = mpsc::channel();
    thread::Builder::new()
        .name("conversion-diagnostics".into())
        .spawn(move || {
            let mut source = stderr;
            let mut saved = Vec::new();
            let mut buffer = [0u8; 4096];
            while let Ok(count) = source.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let keep = count.min(32_768usize.saturating_sub(saved.len()));
                saved.extend_from_slice(&buffer[..keep]);
            }
            let _ = diagnostic_tx.send(String::from_utf8_lossy(&saved).into_owned());
        })
        .context("Cannot start the conversion diagnostic reader.")?;
    let started = Instant::now();
    let mut last_progress = Instant::now();
    let mut result = None;
    let status = loop {
        ensure!(!cancel.load(Ordering::Relaxed), "Conversion cancelled.");
        ensure!(
            started.elapsed() < Duration::from_secs(3600),
            "Conversion exceeded one hour. Try a smaller document or lower resolution."
        );
        ensure!(
            last_progress.elapsed() < Duration::from_secs(300),
            "A conversion stage made no progress for five minutes. Try a lower resolution or inspect this PDF."
        );
        match rx.recv_timeout(Duration::from_millis(75)) {
            Ok(event) => match event? {
                Event::Progress(p) => {
                    last_progress = Instant::now();
                    progress(p);
                }
                Event::Complete(report) => {
                    last_progress = Instant::now();
                    result = Some(report);
                }
                Event::Error(error) => bail!("{error}"),
            },
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
        }
        if let Some(status) = child.0.try_wait()? {
            // Close the Windows job first so no descendant can hold the pipe open.
            #[cfg(windows)]
            drop(job.take());
            let drain = Instant::now();
            loop {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => match event? {
                        Event::Progress(p) => progress(p),
                        Event::Complete(report) => result = Some(report),
                        Event::Error(error) => bail!("{error}"),
                    },
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => ensure!(
                        drain.elapsed() < Duration::from_secs(10),
                        "The worker output stream did not close."
                    ),
                }
            }
            break status;
        }
    };
    let details = diagnostic_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();
    ensure!(
        status.success(),
        "The conversion worker stopped unexpectedly. Try a lower DPI or split the document into smaller PDFs. {}",
        details.trim()
    );
    let report = result.context("The conversion worker returned no validated output.")?;
    ensure!(
        report.output_pages > 0 && report.output_pages == report.input_pages,
        "The output page count did not match the input."
    );
    ensure!(
        staged.metadata()?.len() == report.output_bytes,
        "The staged output size changed."
    );
    ensure!(!cancel.load(Ordering::Relaxed), "Conversion cancelled.");
    tempfile::TempPath::try_from_path(staged)?
        .persist_noclobber(output)
        .map_err(|e| e.error)
        .context("Cannot publish the output. The destination may already exist or be locked.")?;
    Ok(report)
}

fn read_event(reader: &mut impl BufRead) -> Result<Option<Event>> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }
        let count = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |n| n + 1);
        ensure!(
            bytes.len() + count <= 1_048_576,
            "Worker message exceeds the allowed size."
        );
        let finished = available[count - 1] == b'\n';
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if finished {
            break;
        }
    }
    Ok(Some(
        serde_json::from_slice(&bytes).context("Invalid worker response.")?,
    ))
}

pub fn run_child() -> Result<()> {
    let mut bytes = Vec::new();
    std::io::stdin().take(1_048_577).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1_048_576, "Conversion request is too large.");
    let request: Request = serde_json::from_slice(&bytes).context("Invalid conversion request.")?;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::engine::convert(
            &request.input,
            &request.output,
            &request.options,
            &AtomicBool::new(false),
            |p| {
                let _ = emit(&Event::Progress(p));
            },
        )
    }));
    match outcome {
        Ok(Ok(report)) => emit(&Event::Complete(report)),
        Ok(Err(error)) => {
            emit(&Event::Error(format!("{error:#}")))?;
            Err(error)
        }
        Err(_) => {
            emit(&Event::Error(
                "The PDF renderer could not process this document. The original is unchanged."
                    .into(),
            ))?;
            bail!("PDF renderer stopped.")
        }
    }
}
fn emit(event: &Event) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer(&mut out, event)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Job {
    fn attach(child: &Child) -> Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        // SAFETY: owned valid handles and an initialized limits structure, kept live for FFI.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            ensure!(
                !handle.is_null(),
                "Cannot create worker job: {}",
                std::io::Error::last_os_error()
            );
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            limits.ProcessMemoryLimit = 2 * 1024 * 1024 * 1024;
            ensure!(
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    std::mem::size_of_val(&limits) as u32
                ) != 0,
                "Cannot set worker memory limits: {}",
                std::io::Error::last_os_error()
            );
            ensure!(
                AssignProcessToJobObject(handle, child.as_raw_handle()) != 0,
                "Cannot assign worker job: {}",
                std::io::Error::last_os_error()
            );
            Ok(job)
        }
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        /* SAFETY: this is the owned CreateJobObjectW handle. */
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_job_creates_nothing() {
        let d = tempfile::tempdir().unwrap();
        assert!(
            convert(
                Path::new("absent.pdf"),
                &d.path().join("result.pdf"),
                &Options::default(),
                &AtomicBool::new(true),
                |_| {}
            )
            .is_err()
        );
        assert_eq!(d.path().read_dir().unwrap().count(), 0);
    }
    #[test]
    fn existing_destination_is_preserved() {
        let d = tempfile::tempdir().unwrap();
        let output = d.path().join("result.pdf");
        std::fs::write(&output, b"existing").unwrap();
        assert!(
            convert(
                Path::new("absent.pdf"),
                &output,
                &Options::default(),
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err()
        );
        assert_eq!(std::fs::read(output).unwrap(), b"existing");
        assert_eq!(d.path().read_dir().unwrap().count(), 1);
    }
    #[test]
    fn bounded_protocol_rejects_oversized_or_truncated_messages() {
        assert!(read_event(&mut std::io::Cursor::new(vec![b'x'; 1_048_577])).is_err());
        assert!(read_event(&mut std::io::Cursor::new(b"{\"event\":".to_vec())).is_err());
        assert!(
            read_event(&mut std::io::Cursor::new(Vec::<u8>::new()))
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn wrong_output_extension_fails_without_starting_a_worker() {
        let d = tempfile::tempdir().unwrap();
        assert!(
            convert(
                Path::new("absent.pdf"),
                &d.path().join("result.txt"),
                &Options::default(),
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err()
        );
        assert_eq!(d.path().read_dir().unwrap().count(), 0);
    }
}
