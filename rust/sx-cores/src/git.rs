pub const STALE_LOCAL_WORK_DAYS: i64 = 7;
const MILLISECONDS_PER_DAY: i128 = 86_400_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub staged: i64,
    pub unstaged: i64,
    pub untracked: i64,
    pub ahead: Option<i64>,
    pub behind: Option<i64>,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checkout {
    Missing,
    Invalid(String),
    OriginMismatch { branch: Option<String>, head: Head, error: String },
    Detached(Head),
    OnBranch { branch: String, head: Head },
}

pub fn head(checkout: &Checkout) -> Option<&Head> {
    match checkout {
        Checkout::Missing | Checkout::Invalid(_) => None,
        Checkout::Detached(head) | Checkout::OriginMismatch { head, .. } | Checkout::OnBranch { head, .. } => {
            Some(head)
        }
    }
}

pub fn branch(checkout: &Checkout) -> Option<&str> {
    match checkout {
        Checkout::OnBranch { branch, .. } => Some(branch),
        Checkout::OriginMismatch { branch, .. } => branch.as_deref(),
        Checkout::Missing | Checkout::Invalid(_) | Checkout::Detached(_) => None,
    }
}

pub fn error(checkout: &Checkout) -> Option<&str> {
    match checkout {
        Checkout::Invalid(error) | Checkout::OriginMismatch { error, .. } => Some(error),
        Checkout::Missing | Checkout::Detached(_) | Checkout::OnBranch { .. } => None,
    }
}

fn changed_files(head: &Head) -> i128 {
    i128::from(head.staged) + i128::from(head.unstaged) + i128::from(head.untracked)
}

fn sync_state(head: &Head) -> String {
    let changed = changed_files(head);
    if changed != 0 {
        return format!("dirty ({changed})");
    }
    match (head.ahead, head.behind) {
        (Some(ahead), Some(behind)) if ahead > 0 && behind > 0 => format!("diverged {ahead}/{behind} (last fetch)"),
        (Some(ahead), _) if ahead > 0 => format!("ahead {ahead} (last fetch)"),
        (_, Some(behind)) if behind > 0 => format!("behind {behind} (last fetch)"),
        _ => "clean".to_owned(),
    }
}

pub fn local_label(
    checkout: &Checkout,
    default_branch: Option<&str>,
    branch_days: Option<i64>,
    unpushed_days: Option<i64>,
) -> String {
    let (branch, head) = match checkout {
        Checkout::Missing => return "missing".to_owned(),
        Checkout::Invalid(_) => return "invalid".to_owned(),
        Checkout::OriginMismatch { .. } => return "invalid origin".to_owned(),
        Checkout::Detached(_) => return "detached".to_owned(),
        Checkout::OnBranch { branch, head } => (branch, head),
    };
    let mut state = sync_state(head);
    if let Some(days) = unpushed_days {
        state.push_str(&format!(", oldest unpushed {days}d"));
    }
    if default_branch != Some(branch.as_str()) {
        state.push_str(&format!(" on {branch}"));
        if let Some(days) = branch_days {
            state.push_str(&format!(" for {days}d"));
        }
    }
    state
}

pub fn age_days(now_ms: i64, since_seconds: Option<i64>) -> Option<i64> {
    let since = since_seconds?;
    let elapsed = i128::from(now_ms) - i128::from(since) * 1000;
    Some(i64::try_from(elapsed.div_euclid(MILLISECONDS_PER_DAY).max(0)).unwrap_or(i64::MAX))
}

pub fn stale_work(branch_days: Option<i64>, unpushed_days: Option<i64>) -> bool {
    [branch_days, unpushed_days].into_iter().flatten().any(|days| days >= STALE_LOCAL_WORK_DAYS)
}
