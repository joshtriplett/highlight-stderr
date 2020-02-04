use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use anyhow::bail;
use io_mux::{Mux, TaggedData};

fn env_or(varname: &str, default: &str) -> String {
    std::env::var_os(varname)
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| default.to_string())
}

fn main() -> anyhow::Result<()> {
    let mut mux = Mux::new()?;

    let mut args = std::env::args_os().skip(1);
    let cmd = if let Some(cmd) = args.next() {
        cmd
    } else {
        bail!("Usage: highlight-stderr command [args]");
    };
    let mut child = Command::new(&cmd)
        .args(args)
        .stdout(mux.make_untagged_sender()?)
        .stderr(mux.make_tagged_sender("e")?)
        .spawn()?;

    let mut done_sender = mux.make_tagged_sender("d")?;
    std::thread::spawn(move || match child.wait() {
        Ok(status) => {
            let exit_code = if let Some(code) = status.code() {
                code as u8
            } else {
                status.signal().unwrap() as u8 + 128
            };
            let _ = done_sender.write_all(&[exit_code]);
        }
        Err(e) => {
            let _ = write!(done_sender, "Error: {:?}\n", e);
        }
    });

    let highlight_stdout = colorparse::parse(&env_or("HIGHLIGHT_STDOUT", ""))?;
    let highlight_stderr = colorparse::parse(&env_or("HIGHLIGHT_STDERR", "bold red"))?;
    let out_raw = std::io::stdout();
    let out = &mut out_raw.lock();

    loop {
        let TaggedData { tag, data } = mux.read()?;
        match (tag.as_deref(), data) {
            (Some("d"), &[exit_code]) => std::process::exit(exit_code as i32),
            (Some("d"), error) => {
                std::io::stderr().write_all(error)?;
                std::process::exit(1);
            }
            (None, _) => highlight_stdout.paint(data).write_to(out)?,
            (_, _) => highlight_stderr.paint(data).write_to(out)?,
        }
    }
}
