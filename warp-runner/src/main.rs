extern crate dirs;
#[macro_use]
extern crate log;
extern crate simple_logger;

use log::Level;
use std::env;
use std::error::Error;
use std::ffi::*;
use std::fs;
use std::io;
use std::path::*;
use std::process;
use tempdir::TempDir;

mod extractor;
mod executor;

static RUNNER_OPTIONS_BUF: &'static [u8] = b"tVQhhsFFlGGD3oWV4lEPST8I8FEPP54IM0q7daes4E1y3p2U2wlJRYmWmjPYfkhZ0PlT14Ls0j8fdDkoj33f2BlRJavLj3mWGibJsGt5uLAtrCDtvxikZ8UX2mQDCrgE\0";

struct RunnerOptions {
    exec_name: &'static str,
    use_temp_dir: bool,
}

fn runner_options() -> RunnerOptions {
    let nul_pos = RUNNER_OPTIONS_BUF.iter()
        .position(|elem| *elem == b'\0')
        .expect("RUNNER_OPTIONS_BUF has no NUL terminator");

    let slice = &RUNNER_OPTIONS_BUF[..(nul_pos + 1)];
    let exec_name = CStr::from_bytes_with_nul(slice)
        .expect("Can't convert RUNNER_OPTIONS_BUF slice to CStr")
        .to_str()
        .expect("Can't convert RUNNER_OPTIONS_BUF CStr to str");
    let use_temp_dir = RUNNER_OPTIONS_BUF[nul_pos + 1] == 1;
    RunnerOptions { exec_name, use_temp_dir }
}

fn cache_path(target: &str) -> PathBuf {
    dirs::data_local_dir()
        .expect("No data local dir found")
        .join("warp")
        .join("packages")
        .join(target)
}

fn extract(exe_path: &Path, cache_path: &Path) -> io::Result<()> {
    fs::remove_dir_all(cache_path).ok();
    extractor::extract_to(&exe_path, &cache_path)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    if env::var("WARP_TRACE").is_ok() {
        simple_logger::init_with_level(Level::Trace)?;
    }

    let self_path = env::current_exe()?;
    let options = runner_options();
    let application_path = if options.use_temp_dir {
        TempDir::new("warp")?.into_path()
    } else {

        let self_file_name = self_path.file_name().unwrap();
        let cache_path = cache_path(&self_file_name.to_string_lossy());

        trace!("self_path={:?}", self_path);
        trace!("self_file_name={:?}", self_file_name);
        trace!("cache_path={:?}", cache_path);
        cache_path
    };
    let target_path = application_path.join(options.exec_name);

    trace!("target_exec={:?}", options.exec_name);
    trace!("target_path={:?}", target_path);

    let mut should_extract = true;
    if !options.use_temp_dir {
        if let Ok(cache) = fs::metadata(&application_path) {
            if cache.modified()? >= fs::metadata(&self_path)?.modified()? {
                should_extract = false;
                trace!("cache is up-to-date");
            }
        }
    }

    if should_extract {
        extract(&self_path, &application_path)?;
    }

    let exit_code = executor::execute(&target_path)?;
    if options.use_temp_dir {
        fs::remove_dir_all(application_path)?;
    }
    process::exit(exit_code);
}
