//! Entry point for the `ratasm` binary.

use std::process::ExitCode;

fn main() -> ExitCode {
    ratasm::ui::install_panic_hook();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The guard has already restored the terminal by this point, so it
            // is safe to write a plain diagnostic to stderr.
            eprintln!("ratasm: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let _guard = ratasm::ui::TerminalGuard::new()?;
    Ok(())
}
