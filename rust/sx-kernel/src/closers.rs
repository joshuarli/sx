use std::collections::{BTreeMap, HashMap};

use crate::authority::Authority;
use crate::budgets::Budgets;
use crate::checker::{CheckResult, compile_source};
use crate::json::Json;
use crate::reader::{ReadResult, closer_of, indent, is_blank, read};
use crate::registry::Registry;
use crate::types::Type;

const WHITESPACE: &[char] = &[' ', '\t', '\r', '\n', ','];
const MAX_EDIT_DISTANCE: usize = 6;
const MAX_STATES_PER_LINE: usize = 1024;
const LONG_RUN: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloserEdit {
    pub line: usize,
    pub column: usize,
    pub written: String,
    pub used: String,
}

#[derive(Debug)]
pub struct Inferred {
    pub text: String,
    pub edits: Vec<CloserEdit>,
}

#[derive(Debug)]
pub struct InferredRead {
    pub result: ReadResult,
    pub text: String,
    pub edits: Vec<CloserEdit>,
}

pub struct Compiled {
    pub check: CheckResult,
    pub source: String,
    pub edits: Vec<CloserEdit>,
}

enum Bracket {
    Open { closer: char, column: usize },
    Close(char),
}

struct Scanned {
    chars: Vec<char>,
    blank: bool,
    starts_with_closer: bool,
    indent: usize,
    brackets: Vec<Bracket>,
    run_start: usize,
    run_end: usize,
    written: String,
}

fn is_closer(c: char) -> bool {
    matches!(c, ')' | ']' | '}')
}

fn scan_line(text: &str) -> Option<Scanned> {
    let chars: Vec<char> = text.chars().collect();
    let mut events: Vec<(usize, Bracket)> = Vec::new();
    let mut in_string = false;
    let mut code_end = 0;
    let mut first_code = None;
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        if in_string {
            if c == '\\' {
                at += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
                code_end = at + 1;
            }
            at += 1;
            continue;
        }
        if c == ';' {
            break;
        }
        if WHITESPACE.contains(&c) {
            at += 1;
            continue;
        }
        first_code.get_or_insert(c);
        code_end = at + 1;
        if c == '"' {
            in_string = true;
        } else if let Some(closer) = closer_of(c) {
            let hash = at > 0 && chars.get(at - 1) == Some(&'#');
            events.push((at, Bracket::Open { closer, column: at - usize::from(hash) }));
        } else if is_closer(c) {
            events.push((at, Bracket::Close(c)));
        }
        at += 1;
    }
    if in_string {
        return None;
    }
    let mut run_start = code_end;
    while run_start > 0 && chars.get(run_start - 1).is_some_and(|c| is_closer(*c) || WHITESPACE.contains(c)) {
        run_start -= 1;
    }
    while run_start < code_end && chars.get(run_start).is_some_and(|c| WHITESPACE.contains(c)) {
        run_start += 1;
    }
    let written: String = chars[run_start..code_end].iter().filter(|c| is_closer(**c)).collect();
    let brackets = events.into_iter().filter(|(index, _)| *index < run_start).map(|(_, bracket)| bracket).collect();
    Some(Scanned {
        blank: is_blank(text),
        starts_with_closer: first_code.is_some_and(is_closer),
        indent: indent(text),
        chars,
        brackets,
        run_start,
        run_end: code_end,
        written,
    })
}

fn scan(source: &str) -> Option<Vec<Scanned>> {
    source.split('\n').map(scan_line).collect()
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Open {
    closer: char,
    column: usize,
    anchor: usize,
}

fn enter(stack: &mut Vec<Open>, bracket: &Bracket, anchor: usize) -> Option<()> {
    match bracket {
        Bracket::Open { closer, column } => stack.push(Open { closer: *closer, column: *column, anchor }),
        Bracket::Close(c) => {
            if stack.pop()?.closer != *c {
                return None;
            }
        }
    }
    Some(())
}

fn balanced(lines: &[Scanned]) -> bool {
    let mut stack = Vec::new();
    for line in lines {
        let run: Vec<Bracket> = line.written.chars().map(Bracket::Close).collect();
        for bracket in line.brackets.iter().chain(&run) {
            if enter(&mut stack, bracket, line.indent).is_none() {
                return false;
            }
        }
    }
    stack.is_empty()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(x != *y)).min(row[j] + 1).min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

const CRITERIA: usize = 3;
const SHORT_RUN_EDITS: usize = 0;
const CLOSED_AGAINST_INDENT: usize = 1;
const OPEN_AGAINST_INDENT: usize = 2;
const RANKINGS: [[usize; CRITERIA]; 6] = [
    [SHORT_RUN_EDITS, CLOSED_AGAINST_INDENT, OPEN_AGAINST_INDENT],
    [SHORT_RUN_EDITS, OPEN_AGAINST_INDENT, CLOSED_AGAINST_INDENT],
    [CLOSED_AGAINST_INDENT, SHORT_RUN_EDITS, OPEN_AGAINST_INDENT],
    [CLOSED_AGAINST_INDENT, OPEN_AGAINST_INDENT, SHORT_RUN_EDITS],
    [OPEN_AGAINST_INDENT, SHORT_RUN_EDITS, CLOSED_AGAINST_INDENT],
    [OPEN_AGAINST_INDENT, CLOSED_AGAINST_INDENT, SHORT_RUN_EDITS],
];

#[derive(Clone, Copy)]
struct Cost {
    edits: usize,
    criteria: [usize; CRITERIA],
}

impl Cost {
    fn key(&self, ranking: &[usize; CRITERIA]) -> [usize; CRITERIA + 1] {
        let [first, second, third] = *ranking;
        [self.edits, self.criteria[first], self.criteria[second], self.criteria[third]]
    }
}

struct Reached {
    cost: Cost,
    paths: u8,
    from: Vec<Open>,
    popped: usize,
}

struct CodeLine<'a> {
    index: usize,
    scanned: &'a Scanned,
    next_indent: Option<usize>,
}

fn code_lines(lines: &[Scanned]) -> Vec<CodeLine<'_>> {
    let mut found: Vec<CodeLine<'_>> = Vec::new();
    for (index, scanned) in lines.iter().enumerate() {
        if scanned.blank {
            continue;
        }
        let next_indent =
            lines[index + 1..].iter().find(|later| !later.blank && !later.starts_with_closer).map(|later| later.indent);
        found.push(CodeLine { index, scanned, next_indent });
    }
    found
}

fn against_indent(stack: &[Open], popped: usize, next_indent: Option<usize>) -> (usize, usize) {
    let Some(next) = next_indent else {
        return (0, 0);
    };
    let kept = stack.len() - popped;
    let closed_early = stack[kept..].iter().filter(|open| next > open.column).count();
    let left_open = stack[..kept].iter().filter(|open| next <= open.anchor).count();
    let indented_out = stack[..kept].iter().rev().take_while(|open| next <= open.column).count();
    (closed_early + left_open, indented_out)
}

enum Searched {
    Unique(Vec<usize>),
    Ambiguous,
    Unreachable,
    Overflow,
}

fn search(code: &[CodeLine<'_>], budget: usize, ranking: &[usize; CRITERIA]) -> Searched {
    let mut layers: Vec<HashMap<Vec<Open>, Reached>> = Vec::with_capacity(code.len() + 1);
    let start = Reached { cost: Cost { edits: 0, criteria: [0; CRITERIA] }, paths: 1, from: Vec::new(), popped: 0 };
    layers.push(HashMap::from([(Vec::new(), start)]));
    for (position, line) in code.iter().enumerate() {
        let last = position + 1 == code.len();
        let mut next: HashMap<Vec<Open>, Reached> = HashMap::new();
        for (stack, reached) in &layers[position] {
            let mut open = stack.clone();
            if line.scanned.brackets.iter().any(|bracket| enter(&mut open, bracket, line.scanned.indent).is_none()) {
                continue;
            }
            let written = line.scanned.written.as_str();
            let written_count = written.chars().count();
            let spare = budget - reached.cost.edits;
            let lowest = if last { open.len() } else { written_count.saturating_sub(spare) };
            let highest = if last { open.len() } else { (written_count + spare).min(open.len()) };
            for popped in lowest..=highest {
                let remaining = open[..open.len() - popped].to_vec();
                let used: String = open.iter().rev().take(popped).map(|o| o.closer).collect();
                let distance = edit_distance(written, &used);
                if reached.cost.edits + distance > budget {
                    continue;
                }
                let (closed, indented_out) = against_indent(&open, popped, line.next_indent);
                let short = usize::from(written_count < LONG_RUN) * distance;
                let mut cost = reached.cost;
                cost.edits += distance;
                cost.criteria[SHORT_RUN_EDITS] += short;
                cost.criteria[CLOSED_AGAINST_INDENT] += closed;
                cost.criteria[OPEN_AGAINST_INDENT] += indented_out;
                let found = Reached { cost, paths: reached.paths, from: stack.clone(), popped };
                match next.get_mut(&remaining) {
                    Some(held) if held.cost.key(ranking) < cost.key(ranking) => {}
                    Some(held) if held.cost.key(ranking) == cost.key(ranking) => {
                        held.paths = held.paths.saturating_add(found.paths).min(2);
                    }
                    Some(held) => *held = found,
                    None => {
                        next.insert(remaining, found);
                    }
                }
            }
        }
        if next.len() > MAX_STATES_PER_LINE {
            return Searched::Overflow;
        }
        layers.push(next);
    }
    let Some(end) = layers.last().and_then(|layer| layer.get(&Vec::new())) else {
        return Searched::Unreachable;
    };
    if end.paths != 1 {
        return Searched::Ambiguous;
    }
    let mut choices = vec![0; code.len()];
    let mut stack: Vec<Open> = Vec::new();
    for position in (0..code.len()).rev() {
        let Some(reached) = layers[position + 1].get(&stack) else {
            return Searched::Unreachable;
        };
        choices[position] = reached.popped;
        stack = reached.from.clone();
    }
    Searched::Unique(choices)
}

fn closer_surplus(lines: &[Scanned]) -> usize {
    let mut opened = 0usize;
    let mut closed = 0usize;
    for line in lines {
        opened += line.brackets.iter().filter(|bracket| matches!(bracket, Bracket::Open { .. })).count();
        closed += line.brackets.iter().filter(|bracket| matches!(bracket, Bracket::Close(_))).count();
        closed += line.written.chars().count();
    }
    opened.abs_diff(closed)
}

fn infer_choices(code: &[CodeLine<'_>], least_edits: usize) -> Option<Vec<Vec<usize>>> {
    for budget in least_edits.max(1)..=MAX_EDIT_DISTANCE {
        match search(code, budget, &RANKINGS[0]) {
            Searched::Unique(first) => {
                let mut winners = vec![first];
                for ranking in &RANKINGS[1..] {
                    let Searched::Unique(other) = search(code, budget, ranking) else {
                        return None;
                    };
                    if !winners.contains(&other) {
                        winners.push(other);
                    }
                }
                return Some(winners);
            }
            Searched::Unreachable => {}
            Searched::Ambiguous | Searched::Overflow => return None,
        }
    }
    None
}

fn apply(lines: &[Scanned], code: &[CodeLine<'_>], choices: &[usize]) -> Option<Inferred> {
    let mut stack: Vec<Open> = Vec::new();
    let mut rewritten: Vec<String> = lines.iter().map(|line| line.chars.iter().collect()).collect();
    let mut edits = Vec::new();
    for (line, popped) in code.iter().zip(choices) {
        for bracket in &line.scanned.brackets {
            enter(&mut stack, bracket, line.scanned.indent)?;
        }
        let used: String = stack.drain(stack.len().checked_sub(*popped)?..).rev().map(|o| o.closer).collect();
        if used == line.scanned.written {
            continue;
        }
        let head: String = line.scanned.chars[..line.scanned.run_start].iter().collect();
        let tail: String = line.scanned.chars[line.scanned.run_end..].iter().collect();
        rewritten[line.index] = format!("{head}{used}{tail}");
        edits.push(CloserEdit {
            line: line.index + 1,
            column: line.scanned.run_start + 1,
            written: line.scanned.written.clone(),
            used,
        });
    }
    (!edits.is_empty()).then(|| Inferred { text: rewritten.join("\n"), edits })
}

pub fn infer_candidates(source: &str) -> Vec<Inferred> {
    let Some(lines) = scan(source) else {
        return Vec::new();
    };
    let code = code_lines(&lines);
    if code.len() < 2 || balanced(&lines) {
        return Vec::new();
    }
    let Some(winners) = infer_choices(&code, closer_surplus(&lines)) else {
        return Vec::new();
    };
    winners
        .iter()
        .filter_map(|choices| apply(&lines, &code, choices))
        .filter(|inferred| read(&inferred.text).program.is_some())
        .collect()
}

pub fn infer_closers(source: &str) -> Option<Inferred> {
    let mut candidates = infer_candidates(source);
    (candidates.len() == 1).then(|| candidates.remove(0))
}

fn has_bracket_error(read: &ReadResult) -> bool {
    read.diagnostics.iter().any(|d| matches!(d.code, "unclosed" | "unexpected-close"))
}

pub fn read_inferring(source: &str) -> InferredRead {
    let strict = read(source);
    if strict.program.is_none()
        && has_bracket_error(&strict)
        && let Some(inferred) = infer_closers(source)
    {
        return InferredRead { result: read(&inferred.text), text: inferred.text, edits: inferred.edits };
    }
    InferredRead { result: strict, text: source.to_string(), edits: Vec::new() }
}

pub fn compile_source_inferring(
    source: &str,
    registry: &Registry,
    authority: Authority<'_>,
    inputs: &BTreeMap<String, Type>,
    budgets: Budgets,
) -> Compiled {
    let strict = compile_source(source, registry, authority, inputs, budgets);
    let unchanged = |check| Compiled { check, source: source.to_string(), edits: Vec::new() };
    if strict.program.is_some() || !strict.diagnostics.iter().any(|d| matches!(d.code, "unclosed" | "unexpected-close")) {
        return unchanged(strict);
    }
    let mut checked: Vec<(Inferred, CheckResult)> = infer_candidates(source)
        .into_iter()
        .map(|inferred| {
            let check = compile_source(&inferred.text, registry, authority, inputs, budgets);
            (inferred, check)
        })
        .collect();
    let passing: Vec<usize> =
        checked.iter().enumerate().filter(|(_, (_, check))| check.program.is_some()).map(|(index, _)| index).collect();
    let chosen = match (checked.len(), passing.as_slice()) {
        (1, _) => 0,
        (_, [only]) => *only,
        _ => return unchanged(strict),
    };
    let (inferred, check) = checked.swap_remove(chosen);
    Compiled { check, source: inferred.text, edits: inferred.edits }
}

pub fn edit_line(edit: &CloserEdit) -> String {
    format!("line {} wrote \"{}\" used \"{}\"", edit.line, edit.written, edit.used)
}

pub fn edits_note(edits: &[CloserEdit]) -> String {
    let parts: Vec<String> = edits.iter().map(edit_line).collect();
    format!("sx: the brackets did not balance, so closers were inferred from indentation: {}", parts.join("; "))
}

pub fn edits_json(edits: &[CloserEdit]) -> Json {
    Json::Array(
        edits
            .iter()
            .map(|edit| {
                Json::object([
                    ("line", Json::Int(edit.line as i64)),
                    ("column", Json::Int(edit.column as i64)),
                    ("written", Json::string(edit.written.as_str())),
                    ("used", Json::string(edit.used.as_str())),
                ])
            })
            .collect(),
    )
}
