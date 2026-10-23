use crate::jq::{self, Cancel, Options, Output};
use std::sync::Arc;

// What to ask jq; at most one job per kind is in flight at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobKind {
    Validate,
    Pretty { slurp: bool },
    Run { filter: String, options: Options },
    Paths { slurp: bool },
}

impl JobKind {
    pub const SLOTS: usize = 4;

    pub fn slot(&self) -> usize {
        match self {
            Self::Validate => 0,
            Self::Pretty { .. } => 1,
            Self::Run { .. } => 2,
            Self::Paths { .. } => 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub generation: u64,
    pub json: Arc<str>,
    pub kind: JobKind,
    pub cancel: Cancel,
}

#[derive(Debug)]
pub enum JobResult {
    // Stopped because a newer request superseded it; carries nothing worth showing.
    Cancelled,
    Output(Output),
    Paths(Result<Vec<String>, String>),
}

#[derive(Debug)]
pub struct Reply {
    pub generation: u64,
    pub kind: JobKind,
    pub result: JobResult,
}

pub fn execute(job: Job) -> Reply {
    let cancel = &job.cancel;
    let result = match &job.kind {
        JobKind::Validate => JobResult::Output(jq::validate(&job.json, cancel)),
        JobKind::Pretty { slurp } => JobResult::Output(jq::pretty(&job.json, *slurp, cancel)),
        JobKind::Run { filter, options } => {
            JobResult::Output(jq::run_filter(&job.json, filter, *options, cancel))
        }
        JobKind::Paths { slurp } => JobResult::Paths(jq::paths(&job.json, *slurp, cancel)),
    };
    let result = if cancel.was_cancelled() { JobResult::Cancelled } else { result };
    Reply { generation: job.generation, kind: job.kind, result }
}
