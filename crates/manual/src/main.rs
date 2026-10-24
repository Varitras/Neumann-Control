//! `neumann-control-manual <README> <output>`: writes the manual for the README; pictures are
//! read relative to the README's folder and embedded, links to repository files lead to GitHub.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [readme, output] = arguments.as_slice() else {
        eprintln!("usage: neumann-control-manual <README> <output>");
        return ExitCode::FAILURE;
    };
    match write(Path::new(readme), Path::new(output)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("manual not written: {error}");
            ExitCode::FAILURE
        }
    }
}

fn write(readme: &Path, output: &Path) -> Result<(), String> {
    let markdown =
        std::fs::read_to_string(readme).map_err(|error| format!("{readme:?}: {error}"))?;
    let folder = readme.parent().unwrap_or(Path::new("."));
    let manual =
        neumann_control_manual::render(&markdown, |name| std::fs::read(folder.join(name)).ok())?;
    std::fs::write(output, manual).map_err(|error| format!("{output:?}: {error}"))
}
