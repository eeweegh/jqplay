use crate::jq::{self, Handle, Options, Output};
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
    pub handle: Handle,
}

#[derive(Debug)]
pub enum JobResult {
    Output(Output),
    Paths(Result<Vec<String>, Output>),
}

#[derive(Debug)]
pub struct Reply {
    pub generation: u64,
    pub kind: JobKind,
    pub result: JobResult,
}

impl JobResult {
    // A superseded or abandoned run says nothing about the current state and must not be cached.
    pub fn cancelled(&self) -> bool {
        match self {
            Self::Output(out) => out.cancelled(),
            Self::Paths(res) => res.as_ref().is_err_and(Output::cancelled),
        }
    }
}

pub fn execute(job: Job) -> Reply {
    let handle = &job.handle;
    let result = match &job.kind {
        JobKind::Validate => JobResult::Output(jq::validate(&job.json, handle)),
        JobKind::Pretty { slurp } => JobResult::Output(jq::pretty(&job.json, *slurp, handle)),
        JobKind::Run { filter, options } => {
            JobResult::Output(jq::run_filter(&job.json, filter, *options, handle))
        }
        JobKind::Paths { slurp } => JobResult::Paths(jq::paths(&job.json, *slurp, handle)),
    };
    Reply { generation: job.generation, kind: job.kind, result }
}
