use clap::Parser;
use jqplay::app::Outcome;
use jqplay::cli::Cli;
use std::io::stdout;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(shell) = cli.completions {
        jqplay::generate::completions(shell, &mut stdout());
        return ExitCode::SUCCESS;
    }
    if cli.man {
        return match jqplay::generate::man_page(&mut stdout()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fail(&e),
        };
    }
    match jqplay::run(&cli) {
        Ok(Outcome::Clean(filter)) => {
            println!("jq '{filter}'");
            ExitCode::SUCCESS
        }
        Ok(Outcome::Abort) => ExitCode::FAILURE,
        Err(e) => fail(&e),
    }
}

fn fail(e: &dyn std::fmt::Display) -> ExitCode {
    eprintln!("jqplay: error: {e}");
    ExitCode::FAILURE
}
