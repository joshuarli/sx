use crate::syntax::Span;

pub const REPORTED_LIMIT: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub span: Span,
    pub message: String,
    pub construct: Option<&'static str>,
    pub detail: Option<String>,
    pub fix: Option<String>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn new(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic { code, span, message: message.into(), construct: None, detail: None, fix: None, help: None }
    }

    pub fn construct(mut self, construct: &'static str) -> Diagnostic {
        self.construct = Some(construct);
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Diagnostic {
        self.detail = Some(detail.into());
        self
    }

    pub fn fix(mut self, fix: impl Into<String>) -> Diagnostic {
        self.fix = Some(fix.into());
        self
    }

    pub fn maybe_fix(mut self, fix: Option<String>) -> Diagnostic {
        self.fix = fix;
        self
    }

    pub fn help(mut self, help: impl Into<String>) -> Diagnostic {
        self.help = Some(help.into());
        self
    }

    pub fn maybe_detail(mut self, detail: Option<String>) -> Diagnostic {
        self.detail = detail;
        self
    }

    pub fn maybe_help(mut self, help: Option<String>) -> Diagnostic {
        self.help = help;
        self
    }

    pub fn label(&self) -> String {
        match self.construct {
            None => self.code.to_string(),
            Some(construct) => format!("{}:{construct}", self.code),
        }
    }
}

pub fn render(source: &str, diagnostics: &[Diagnostic]) -> String {
    let lines: Vec<&str> = source.split('\n').collect();
    let mut blocks: Vec<String> =
        diagnostics.iter().take(REPORTED_LIMIT).map(|d| render_one(&lines, d)).collect();
    let hidden = diagnostics.len().saturating_sub(REPORTED_LIMIT);
    if hidden > 0 {
        let plural = if hidden != 1 { "s" } else { "" };
        blocks.push(format!("{hidden} more error{plural} not shown.\n"));
    }
    blocks.join("\n")
}

fn render_one(lines: &[&str], diagnostic: &Diagnostic) -> String {
    let span = diagnostic.span;
    let mut out = vec![format!("error[{}] {span}", diagnostic.label())];
    let line_text = span.start.line.checked_sub(1).and_then(|at| lines.get(at)).copied().unwrap_or("");
    let start = span.start.column as i64 - 1;
    let end = if span.end.line == span.start.line {
        span.end.column as i64 - 1
    } else {
        line_text.chars().count() as i64
    };
    let width = (end - start).max(1) as usize;
    out.push(format!("  {line_text}"));
    out.push(format!("  {}{}", " ".repeat(start.max(0) as usize), "^".repeat(width)));
    out.push(format!("  {}", diagnostic.message));
    if let Some(detail) = &diagnostic.detail {
        out.extend(detail.split('\n').map(|line| format!("  {line}")));
    }
    if let Some(fix) = &diagnostic.fix {
        out.push(format!("  Fix: {fix}"));
    }
    if let Some(help) = &diagnostic.help {
        out.push(format!("  Help: sx help {help}"));
    }
    out.join("\n") + "\n"
}
