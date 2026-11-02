#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budgets {
    pub source_bytes: u64,
    pub nodes: u64,
    pub depth: u64,
    pub steps: u64,
    pub calls: u64,
    pub concurrency: u64,
    pub value_size: u64,
    pub applications: u64,
    pub delay_ms: u64,
    pub instantiations: u64,
}

pub const DEFAULT_BUDGETS: Budgets = Budgets {
    source_bytes: 65_536,
    nodes: 20_000,
    depth: 128,
    steps: 2_000_000,
    calls: 1_000,
    concurrency: 16,
    value_size: 1_000_000,
    applications: 200_000,
    delay_ms: 3_600_000,
    instantiations: 2_000,
};

impl Default for Budgets {
    fn default() -> Budgets {
        DEFAULT_BUDGETS
    }
}

impl Budgets {
    pub fn set(&mut self, name: &str, value: u64) -> Result<(), UnknownBudget> {
        let slot = match name {
            "source_bytes" => &mut self.source_bytes,
            "nodes" => &mut self.nodes,
            "depth" => &mut self.depth,
            "steps" => &mut self.steps,
            "calls" => &mut self.calls,
            "concurrency" => &mut self.concurrency,
            "value_size" => &mut self.value_size,
            "applications" => &mut self.applications,
            "delay_ms" => &mut self.delay_ms,
            "instantiations" => &mut self.instantiations,
            _ => return Err(UnknownBudget(name.to_string())),
        };
        *slot = value;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownBudget(pub String);

impl std::fmt::Display for UnknownBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown budget {:?}", self.0)
    }
}

impl std::error::Error for UnknownBudget {}
