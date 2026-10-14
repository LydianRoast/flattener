use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/flattener.ico");
    for name in [
        "RC",
        "PATH",
        "WindowsSdkDir",
        "WindowsSDKVersion",
        "ProgramFiles(x86)",
        "ProgramFiles",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        panic!(
            "Windows resources require the MSVC target; build with x86_64-pc-windows-msvc or aarch64-pc-windows-msvc."
        );
    }
    if let Err(error) = embed_resources() {
        panic!("Cannot embed the Flattener Windows icon and version information: {error}");
    }
}

fn embed_resources() -> Result<(), String> {
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing manifest directory")?);
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing Cargo output directory")?);
    let icon = manifest.join("assets/icon/flattener.ico");
    if !icon.is_file() {
        return Err("assets/icon/flattener.ico is missing from the source checkout".into());
    }
    let compiler = resource_compiler()?;
    let version = ["MAJOR", "MINOR", "PATCH"]
        .map(|part| env::var(format!("CARGO_PKG_VERSION_{part}")).unwrap_or_default())
        .map(|part| part.parse::<u16>().map_err(|error| error.to_string()));
    let [major, minor, patch] = version;
    let (major, minor, patch) = (major?, minor?, patch?);
    let icon = icon
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let rc = output.join("flattener.rc");
    let resource = output.join("flattener.res");
    let content = format!(
        r#"#pragma code_page(65001)
LANGUAGE 9, 1
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "LydianRoast\0"
            VALUE "FileDescription", "Flattener\0"
            VALUE "FileVersion", "{major}.{minor}.{patch}.0\0"
            VALUE "InternalName", "flattener\0"
            VALUE "LegalCopyright", "Copyright (c) 2026 LydianRoast\0"
            VALUE "OriginalFilename", "Flattener.exe\0"
            VALUE "ProductName", "Flattener\0"
            VALUE "ProductVersion", "{major}.{minor}.{patch}.0\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#
    );
    fs::write(&rc, content).map_err(|error| error.to_string())?;
    let result = Command::new(&compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg(&rc)
        .output()
        .map_err(|error| format!("could not start rc.exe: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "rc.exe failed ({}):\n{}{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    if !resource.is_file() {
        return Err("rc.exe reported success but produced no resource file".into());
    }
    println!("cargo:rustc-link-arg-bin=flattener={}", resource.display());
    Ok(())
}

fn resource_compiler() -> Result<PathBuf, String> {
    if let Some(explicit) = env::var_os("RC") {
        return find_program(&explicit).ok_or_else(|| {
            "RC is set, but it does not identify an existing rc.exe executable".into()
        });
    }
    if let Some(found) = find_program(&OsString::from("rc.exe")) {
        return Ok(found);
    }
    let mut roots = Vec::new();
    if let Some(root) = env::var_os("WindowsSdkDir") {
        roots.push(PathBuf::from(root));
    }
    for key in [
        r"HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots",
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots",
    ] {
        if let Ok(result) = Command::new("reg.exe")
            .args(["query", key, "/v", "KitsRoot10"])
            .output()
            && result.status.success()
        {
            for line in String::from_utf8_lossy(&result.stdout).lines() {
                if let Some((name, value)) = line.split_once("REG_SZ")
                    && name.trim() == "KitsRoot10"
                {
                    roots.push(PathBuf::from(value.trim()));
                }
            }
        }
    }
    for name in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(program_files) = env::var_os(name) {
            roots.push(PathBuf::from(program_files).join("Windows Kits/10"));
        }
    }
    let host = env::var("HOST").unwrap_or_default();
    let architectures = if host.starts_with("aarch64") {
        &["arm64", "x64", "x86"][..]
    } else if host.starts_with("i686") {
        &["x86"][..]
    } else {
        &["x64", "x86"][..]
    };
    for root in roots {
        let mut bins = Vec::new();
        if let Some(version) = env::var_os("WindowsSDKVersion") {
            bins.push(root.join("bin").join(version));
        }
        if let Ok(entries) = fs::read_dir(root.join("bin")) {
            let mut versions: Vec<_> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect();
            versions.sort_by_key(|path| sdk_version(path));
            versions.reverse();
            bins.extend(versions);
        }
        bins.push(root.join("bin"));
        for bin in bins {
            for architecture in architectures {
                let candidate = bin.join(architecture).join("rc.exe");
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    Err("rc.exe was not found. Install the Windows SDK through Visual Studio's Desktop development with C++ workload, or set RC to the resource compiler's full path.".into())
}

fn find_program(value: &OsString) -> Option<PathBuf> {
    let direct = PathBuf::from(value);
    if direct.is_file() {
        return Some(direct);
    }
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|directory| directory.join(value))
            .find(|candidate| candidate.is_file())
    })
}

fn sdk_version(path: &Path) -> Vec<u32> {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}
