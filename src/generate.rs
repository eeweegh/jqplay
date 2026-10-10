use crate::cli::Cli;
use clap::CommandFactory;
use clap_complete::Shell;
use std::io::Write;

pub fn completions(shell: Shell, out: &mut dyn Write) {
    clap_complete::generate(shell, &mut Cli::command(), "jqplay", out);
}

pub fn man_page(out: &mut dyn Write) -> std::io::Result<()> {
    clap_mangen::Man::new(Cli::command()).render(out)
}
