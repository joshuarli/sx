use std::collections::{HashMap, HashSet};

use crate::text::casefold;

const FAILURE_STATES: [&str; 6] = ["failure", "failed", "error", "timed_out", "action_required", "startup_failure"];
const PENDING_STATES: [&str; 6] = ["queued", "requested", "waiting", "pending", "expected", "in_progress"];
const CANCELLED_STATE: &str = "cancelled";
const EXTERNAL_STATUS_PREFIX: &str = "status:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CiState {
    Passed,
    Failed,
    Pending,
    Cancelled,
    Skipped,
    Neutral,
    NoChecks,
    Unknown,
    Stale,
}

impl CiState {
    pub const ALL: [CiState; 9] = [
        CiState::Passed,
        CiState::Failed,
        CiState::Pending,
        CiState::Cancelled,
        CiState::Skipped,
        CiState::Neutral,
        CiState::NoChecks,
        CiState::Unknown,
        CiState::Stale,
    ];

    pub fn variant(self) -> &'static str {
        match self {
            CiState::Passed => "Passed",
            CiState::Failed => "Failed",
            CiState::Pending => "Pending",
            CiState::Cancelled => "Cancelled",
            CiState::Skipped => "Skipped",
            CiState::Neutral => "Neutral",
            CiState::NoChecks => "NoChecks",
            CiState::Unknown => "Unknown",
            CiState::Stale => "Stale",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpstreamState {
    Identical,
    Ahead,
    Behind,
    Diverged,
    Unknown,
}

impl UpstreamState {
    pub const ALL: [UpstreamState; 5] = [
        UpstreamState::Identical,
        UpstreamState::Ahead,
        UpstreamState::Behind,
        UpstreamState::Diverged,
        UpstreamState::Unknown,
    ];

    pub fn variant(self) -> &'static str {
        match self {
            UpstreamState::Identical => "Identical",
            UpstreamState::Ahead => "Ahead",
            UpstreamState::Behind => "Behind",
            UpstreamState::Diverged => "Diverged",
            UpstreamState::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub state: String,
    pub source: Option<String>,
    pub updated_at: Option<String>,
    pub suite_id: Option<String>,
    pub run_id: Option<String>,
    pub attempt: Option<i64>,
}

impl Check {
    fn run_identity(&self) -> Option<&str> {
        match self.run_id.as_deref() {
            Some(run_id) if !run_id.is_empty() => Some(run_id),
            _ => self.suite_id.as_deref(),
        }
    }

    fn rerun(&self) -> Option<(&str, i64)> {
        self.suite_id.as_ref()?;
        Some((self.run_identity()?, self.attempt?))
    }

    fn external_status(&self) -> Option<(&str, &str)> {
        let source = self.source.as_deref()?;
        source.starts_with(EXTERNAL_STATUS_PREFIX).then_some((self.name.as_str(), source))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workflow {
    pub name: String,
    pub state: CiState,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiReport {
    pub sha: Option<String>,
    pub default_branch: Option<String>,
    pub state: CiState,
    pub age_seconds: i64,
    pub workflows: Option<Vec<Workflow>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiCache {
    NotCached(String),
    Cached(CiReport),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub parent: String,
    pub parent_branch: Option<String>,
    pub parent_sha: Option<String>,
    pub behind_by: i64,
    pub ahead_by: i64,
    pub state: UpstreamState,
    pub age_seconds: i64,
}

fn severity(state: &str) -> u8 {
    let folded = casefold(state);
    if FAILURE_STATES.contains(&folded.as_str()) {
        5
    } else if PENDING_STATES.contains(&folded.as_str()) {
        4
    } else if folded == CANCELLED_STATE {
        3
    } else {
        0
    }
}

fn replaces(previous: &Check, candidate: &Check) -> bool {
    match (&previous.updated_at, &candidate.updated_at) {
        (Some(before), Some(after)) => {
            after > before || (after == before && severity(&candidate.state) > severity(&previous.state))
        }
        _ => true,
    }
}

fn current_checks(checks: &[Check]) -> Vec<&Check> {
    let mut latest_attempts: HashMap<&str, i64> = HashMap::new();
    for (identity, attempt) in checks.iter().filter_map(Check::rerun) {
        let latest = latest_attempts.entry(identity).or_insert(0);
        *latest = (*latest).max(attempt);
    }
    let mut latest_statuses: HashMap<(String, String), &Check> = HashMap::new();
    let mut independent = Vec::new();
    for check in checks {
        if let Some((name, source)) = check.external_status() {
            if check.updated_at.is_none() {
                independent.push(check);
            } else {
                let key = (casefold(name), casefold(source));
                if latest_statuses.get(&key).is_none_or(|previous| replaces(previous, check)) {
                    latest_statuses.insert(key, check);
                }
            }
            continue;
        }
        let superseded = check
            .rerun()
            .is_some_and(|(identity, attempt)| latest_attempts.get(identity).is_some_and(|&latest| attempt < latest));
        if !superseded {
            independent.push(check);
        }
    }
    latest_statuses.into_values().chain(independent).collect()
}

fn classify(current: &[&Check]) -> CiState {
    if current.is_empty() {
        return CiState::NoChecks;
    }
    let states: HashSet<String> = current.iter().map(|check| casefold(&check.state)).collect();
    let has = |names: &[&str]| names.iter().any(|name| states.contains(*name));
    let within = |names: &[&str]| states.iter().all(|state| names.contains(&state.as_str()));
    if has(&FAILURE_STATES) {
        CiState::Failed
    } else if has(&PENDING_STATES) {
        CiState::Pending
    } else if has(&[CANCELLED_STATE]) {
        CiState::Cancelled
    } else if within(&["skipped", "neutral"]) {
        if within(&["neutral"]) { CiState::Neutral } else { CiState::Skipped }
    } else if within(&["success", "passed", "neutral", "skipped"]) {
        CiState::Passed
    } else if has(&["stale"]) {
        CiState::Stale
    } else {
        CiState::Unknown
    }
}

pub fn ci_state(checks: &[Check], complete: bool) -> CiState {
    if !complete {
        return CiState::Unknown;
    }
    classify(&current_checks(checks))
}

pub fn ci_state_name(state: CiState) -> &'static str {
    match state {
        CiState::Passed => "passed",
        CiState::Failed => "failed",
        CiState::Pending => "pending",
        CiState::Cancelled => "cancelled",
        CiState::Skipped => "skipped",
        CiState::Neutral => "neutral",
        CiState::NoChecks => "none",
        CiState::Unknown => "unknown",
        CiState::Stale => "stale",
    }
}

pub fn parse_ci_state(label: &str) -> CiState {
    CiState::ALL.into_iter().find(|&state| ci_state_name(state) == label).unwrap_or(CiState::Unknown)
}

pub fn upstream_state_name(state: UpstreamState) -> &'static str {
    match state {
        UpstreamState::Identical => "identical",
        UpstreamState::Ahead => "ahead",
        UpstreamState::Behind => "behind",
        UpstreamState::Diverged => "diverged",
        UpstreamState::Unknown => "unknown",
    }
}
