use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("'jq' is required but was not found on PATH")]
    JqMissing,
    #[error("could not read {}: {source}", path.display())]
    ReadInput { path: PathBuf, source: std::io::Error },
    #[error("could not read stdin: {0}")]
    ReadStdin(std::io::Error),
    #[error("invalid config {}: {message}", path.display())]
    Config { path: PathBuf, message: String },
    #[error(transparent)]
    Terminal(#[from] std::io::Error),
}
