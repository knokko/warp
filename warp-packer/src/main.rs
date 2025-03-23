extern crate clap;
extern crate dirs;
extern crate flate2;
#[macro_use]
extern crate lazy_static;
extern crate reqwest;
extern crate tar;
extern crate tempdir;

use clap::Parser;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io;
use std::io::copy;
use std::io::Write;
use std::path::Path;
use std::process;
use tempdir::TempDir;

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const AUTHOR: &str = env!("CARGO_PKG_AUTHORS");
const VERSION: &str = env!("CARGO_PKG_VERSION");

const RUNNER_MAGIC: &[u8] = b"tVQhhsFFlGGD3oWV4lEPST8I8FEPP54IM0q7daes4E1y3p2U2wlJRYmWmjPYfkhZ0PlT14Ls0j8fdDkoj33f2BlRJavLj3mWGibJsGt5uLAtrCDtvxikZ8UX2mQDCrgE\0";

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const RUNNER_LINUX_ARM64: &[u8] = include_bytes!("../../target/release/warp-runner");

#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
const RUNNER_LINUX_ARM64: &[u8] = include_bytes!("../../old-warp-runners/warp-runner-linux-arm64");

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const RUNNER_LINUX_X64: &[u8] = include_bytes!("../../target/release/warp-runner");

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
const RUNNER_LINUX_X64: &[u8] = include_bytes!("../../old-warp-runners/warp-runner-linux-x64");

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const RUNNER_MACOS_ARM64: &[u8] = include_bytes!("../../target/release/warp-runner");

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
const RUNNER_MACOS_ARM64: &[u8] = include_bytes!("../../old-warp-runners/warp-runner-macos-arm64");

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const RUNNER_MACOS_X64: &[u8] = include_bytes!("../../target/release/warp-runner");

#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
const RUNNER_MACOS_X64: &[u8] = include_bytes!("../../old-warp-runners/warp-runner-macos-x64");

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const RUNNER_WINDOWS_X64: &[u8] = include_bytes!("../../target/release/warp-runner.exe");

#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
const RUNNER_WINDOWS_X64: &[u8] = include_bytes!("../../old-warp-runners/warp-runner-windows-x64.exe");

lazy_static! {
    static ref RUNNER_BY_ARCH: HashMap<&'static str, &'static [u8]> = {
        let mut m = HashMap::new();
        m.insert("linux-x64", RUNNER_LINUX_X64);
        m.insert("linux-arm64", RUNNER_LINUX_ARM64);
        m.insert("macos-x64", RUNNER_MACOS_X64);
        m.insert("macos-arm64", RUNNER_MACOS_ARM64);
        m.insert("windows-x64", RUNNER_WINDOWS_X64);
        m
    };
}

/// Print a message to stderr and exit with error code 1
macro_rules! bail {
    () => (process::exit(1));
    ($($arg:tt)*) => ({
        eprint!("{}\n", format_args!($($arg)*));
        process::exit(1);
    })
}

fn patch_runner(arch: &str, exec_name: &str) -> io::Result<Vec<u8>> {
    // Read runner executable in memory
    let runner_contents = RUNNER_BY_ARCH.get(arch).unwrap();
    let mut buf = runner_contents.to_vec();

    // Set the correct target executable name into the local magic buffer
    let magic_len = RUNNER_MAGIC.len();
    let mut new_magic = vec![0; magic_len];
    new_magic[..exec_name.len()].clone_from_slice(exec_name.as_bytes());

    // Find the magic buffer offset inside the runner executable
    let mut offs_opt = None;
    for (i, chunk) in buf.windows(magic_len).enumerate() {
        if chunk == RUNNER_MAGIC {
            offs_opt = Some(i);
            break;
        }
    }

    if offs_opt.is_none() {
        return Err(io::Error::new(io::ErrorKind::Other, "no magic found inside runner"));
    }

    // Replace the magic with the new one that points to the target executable
    let offs = offs_opt.unwrap();
    buf[offs..offs + magic_len].clone_from_slice(&new_magic);

    Ok(buf)
}

fn create_tgz(dir: &Path, out: &Path) -> io::Result<()> {
    let f = File::create(out)?;
    let gz = GzEncoder::new(f, Compression::best());
    let mut tar = tar::Builder::new(gz);
    tar.follow_symlinks(false);
    tar.append_dir_all(".", dir)?;
    Ok(())
}

#[cfg(target_family = "unix")]
fn create_app_file(out: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o755)
        .open(out)
}

#[cfg(target_family = "windows")]
fn create_app_file(out: &Path) -> io::Result<File> {
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(out)
}

fn create_app(runner_buf: &Vec<u8>, tgz_path: &Path, out: &Path) -> io::Result<()> {
    let mut outf = create_app_file(out)?;
    let mut tgzf = File::open(tgz_path)?;
    outf.write_all(runner_buf)?;
    copy(&mut tgzf, &mut outf)?;
    Ok(())
}

#[derive(Parser)]
#[command(name = APP_NAME)]
#[command(version = VERSION)]
#[command(author = AUTHOR)]
#[command(about = "Create self-contained single binary application", long_about = None)]
struct Cli {
    /// Target architecture, must be linux-x64, linux-arm64, macos-x64, macos-arm64, or windows-x64
    #[arg(short, long)]
    arch: String,

    /// Sets the input directory containing the application and dependencies
    #[arg(short, long)]
    input_dir: String,

    /// The path to the executable file, relative to input_dir
    #[arg(short, long)]
    exec: String,

    /// The output file to be created
    #[arg(short, long)]
    output: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    if !RUNNER_BY_ARCH.contains_key(args.arch.as_str()) {
        bail!("Unknown architecture specified: {}, supported: {:?}", args.arch, RUNNER_BY_ARCH.keys());
    }

    let input_dir = Path::new(args.input_dir.as_str());
    if fs::metadata(input_dir).is_err() {
        bail!("Cannot access specified input directory {:?}", input_dir);
    }

    if args.exec.len() >= RUNNER_MAGIC.len() {
        bail!("Executable name is too long, please consider using a shorter name");
    }

    let exec_path = Path::new(input_dir).join(args.exec.as_str());
    match fs::metadata(&exec_path) {
        Err(_) => {
            bail!("Cannot find file {:?}", exec_path);
        }
        Ok(metadata) => {
            if !metadata.is_file() {
                bail!("{:?} isn't a file", exec_path);
            }
        }
    }

    let runner_buf = patch_runner(&args.arch, &args.exec)?;

    println!("Compressing input directory {:?}...", input_dir);
    let tmp_dir = TempDir::new(APP_NAME)?;
    let tgz_path = tmp_dir.path().join("input.tgz");
    create_tgz(&input_dir, &tgz_path)?;

    let exec_name = Path::new(args.output.as_str());
    println!("Creating self-contained application binary {:?}...", exec_name);
    create_app(&runner_buf, &tgz_path, &exec_name)?;

    println!("All done");
    Ok(())
}
