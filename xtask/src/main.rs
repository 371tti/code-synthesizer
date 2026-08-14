use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MODULE_INFO: &str = r#"{
  "Name": "Code Synthesizer",
  "Version": "0.1.0",
  "Factory Info": {
    "Vendor": "Code Synthesizer",
    "URL": "",
    "E-Mail": ""
  },
  "Classes": [
    {
      "CID": "9A3D1F6C2B7E4A15B8C0D2E4F617293B",
      "Category": "Audio Module Class",
      "Name": "Code Synthesizer",
      "Vendor": "Code Synthesizer",
      "Version": "0.1.0",
      "SDKVersion": "VST 3.8.0",
      "Sub Categories": ["Instrument", "Synth"]
    }
  ]
}
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("bundle") => bundle(args.any(|arg| arg == "--release")),
        _ => {
            eprintln!("usage: cargo xtask bundle [--release]");
            Ok(())
        }
    }
}

fn bundle(release: bool) -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root()?;
    let npm = if cfg!(target_os = "windows") {
        "npm.cmd"
    } else {
        "npm"
    };
    run(Command::new(npm)
        .args(["run", "build"])
        .current_dir(root.join("ui")))?;

    let mut cargo = Command::new("cargo");
    cargo.args(["build", "-p", "synth-vst3"]);
    if release {
        cargo.arg("--release");
    }
    run(cargo.current_dir(&root))?;

    let profile = if release { "release" } else { "debug" };
    let binary_name = if cfg!(target_os = "windows") {
        "synth_vst3.dll"
    } else if cfg!(target_os = "macos") {
        "libsynth_vst3.dylib"
    } else {
        "libsynth_vst3.so"
    };
    let source = root.join("target").join(profile).join(binary_name);
    if !source.exists() {
        return Err(format!("plugin binary was not produced: {}", source.display()).into());
    }

    let bundle = root
        .join("target")
        .join("bundled")
        .join("Code Synthesizer.vst3");
    let binary_dir = if cfg!(target_os = "windows") {
        bundle.join("Contents").join("x86_64-win")
    } else if cfg!(target_os = "linux") {
        bundle.join("Contents").join("x86_64-linux")
    } else {
        bundle.join("Contents").join("MacOS")
    };
    let resources = bundle.join("Contents").join("Resources");
    fs::create_dir_all(&binary_dir)?;
    fs::create_dir_all(&resources)?;
    fs::copy(source, binary_dir.join("Code Synthesizer.vst3"))?;
    fs::write(resources.join("moduleinfo.json"), MODULE_INFO)?;
    println!("Bundled {}", bundle.display());
    Ok(())
}

fn workspace_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest
        .parent()
        .ok_or("xtask has no workspace parent")?
        .to_owned())
}

fn run(command: &mut Command) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed with {status}").into())
    }
}
