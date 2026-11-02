use sx_kernel::help::GuideLevel;
use sx_kernel::json::repr_str;

use crate::Exit;
use crate::helptext::{CHECK_HELP, CORES_HELP, EXPLAIN_HELP, HELP_HELP, RUN_HELP, TOP_HELP};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpArgs {
    pub name: Option<String>,
    pub declarations: Option<String>,
    pub level: GuideLevel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoresArgs {
    pub format: Format,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgramKind {
    Check { emit_ir: bool },
    Explain,
    Run { responses: Option<String>, inputs: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramArgs {
    pub kind: ProgramKind,
    pub file: String,
    pub declarations: Option<String>,
    pub allow: Vec<String>,
    pub format: Format,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invocation {
    Help(HelpArgs),
    Cores(CoresArgs),
    Program(ProgramArgs),
}

pub enum Parsed {
    Invoke(Invocation),
    Done(Exit),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Declarations,
    Allow,
    Format,
    Emit,
    Responses,
    Input,
    Level,
}

struct Flag {
    name: &'static str,
    slot: Slot,
    choices: Option<&'static [&'static str]>,
}

const FORMATS: &[&str] = &["text", "json"];
const LEVELS: &[&str] = &["minimal", "standard", "full"];
const EMITS: &[&str] = &["ir"];

const DECLARATIONS: Flag = Flag { name: "--declarations", slot: Slot::Declarations, choices: None };
const ALLOW: Flag = Flag { name: "--allow", slot: Slot::Allow, choices: None };
const FORMAT: Flag = Flag { name: "--format", slot: Slot::Format, choices: Some(FORMATS) };
const EMIT: Flag = Flag { name: "--emit", slot: Slot::Emit, choices: Some(EMITS) };
const RESPONSES: Flag = Flag { name: "--responses", slot: Slot::Responses, choices: None };
const INPUT: Flag = Flag { name: "--input", slot: Slot::Input, choices: None };
const LEVEL: Flag = Flag { name: "--level", slot: Slot::Level, choices: Some(LEVELS) };

#[derive(Clone, Copy, PartialEq, Eq)]
enum Name {
    Help,
    Cores,
    Check,
    Explain,
    Run,
}

struct Subcommand {
    name: Name,
    word: &'static str,
    flags: &'static [Flag],
    positional: Positional,
    help: &'static str,
}

#[derive(Clone, Copy)]
enum Positional {
    None,
    Optional,
    Required(&'static str),
}

const SUBCOMMANDS: [Subcommand; 5] = [
    Subcommand {
        name: Name::Help,
        word: "help",
        flags: &[DECLARATIONS, LEVEL],
        positional: Positional::Optional,
        help: HELP_HELP,
    },
    Subcommand { name: Name::Cores, word: "cores", flags: &[FORMAT], positional: Positional::None, help: CORES_HELP },
    Subcommand {
        name: Name::Check,
        word: "check",
        flags: &[DECLARATIONS, ALLOW, FORMAT, EMIT],
        positional: Positional::Required("file"),
        help: CHECK_HELP,
    },
    Subcommand {
        name: Name::Explain,
        word: "explain",
        flags: &[DECLARATIONS, ALLOW, FORMAT],
        positional: Positional::Required("file"),
        help: EXPLAIN_HELP,
    },
    Subcommand {
        name: Name::Run,
        word: "run",
        flags: &[DECLARATIONS, ALLOW, FORMAT, RESPONSES, INPUT],
        positional: Positional::Required("file"),
        help: RUN_HELP,
    },
];

const COMMAND_WORDS: [&str; 5] = ["help", "cores", "check", "explain", "run"];

struct Failure {
    usage: &'static str,
    prog: String,
    message: String,
}

impl Failure {
    fn into_exit(self) -> Exit {
        Exit { code: 2, stdout: String::new(), stderr: format!("{}{}: error: {}\n", self.usage, self.prog, self.message) }
    }
}

fn usage_of(help: &'static str) -> &'static str {
    match help.find("\n\n") {
        Some(end) => &help[..end + 1],
        None => help,
    }
}

fn help_exit(help: &'static str) -> Parsed {
    Parsed::Done(Exit { code: 0, stdout: help.to_string(), stderr: String::new() })
}

fn matching_blocks(a: &[char], b: &[char], alo: usize, ahi: usize, blo: usize, bhi: usize) -> usize {
    let (mut best_i, mut best_j, mut best_size) = (alo, blo, 0);
    let mut previous = vec![0usize; b.len() + 1];
    for i in alo..ahi {
        let mut current = vec![0usize; b.len() + 1];
        for j in blo..bhi {
            if a.get(i) == b.get(j) {
                let size = previous.get(j).copied().unwrap_or(0) + 1;
                if let Some(slot) = current.get_mut(j + 1) {
                    *slot = size;
                }
                if size > best_size {
                    best_i = i + 1 - size;
                    best_j = j + 1 - size;
                    best_size = size;
                }
            }
        }
        previous = current;
    }
    if best_size == 0 {
        return 0;
    }
    best_size
        + matching_blocks(a, b, alo, best_i, blo, best_j)
        + matching_blocks(a, b, best_i + best_size, ahi, best_j + best_size, bhi)
}

fn similarity(candidate: &str, word: &str) -> f64 {
    let a: Vec<char> = candidate.chars().collect();
    let b: Vec<char> = word.chars().collect();
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    2.0 * matching_blocks(&a, &b, 0, a.len(), 0, b.len()) as f64 / total as f64
}

fn close_match<'a>(word: &str, choices: &[&'a str]) -> Option<&'a str> {
    let mut best: Option<(f64, &'a str)> = None;
    for choice in choices {
        let score = similarity(choice, word);
        if score < 0.6 {
            continue;
        }
        if best.is_none_or(|(top, name)| (score, *choice) > (top, name)) {
            best = Some((score, choice));
        }
    }
    best.map(|(_, name)| name)
}

fn invalid_choice(value: &str, choices: &[&str]) -> String {
    let listed: Vec<String> = choices.iter().map(|c| repr_str(c)).collect();
    let hint = close_match(value, choices).map(|name| format!(", maybe you meant {}?", repr_str(name)));
    match hint {
        Some(hint) => format!("invalid choice: {}{hint} (choose from {})", repr_str(value), listed.join(", ")),
        None => format!("invalid choice: {} (choose from {})", repr_str(value), listed.join(", ")),
    }
}

fn is_negative_number(arg: &str) -> bool {
    let Some(rest) = arg.strip_prefix('-') else {
        return false;
    };
    match rest.split_once('.') {
        None => !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()),
        Some((whole, fraction)) => {
            whole.bytes().all(|b| b.is_ascii_digit()) && !fraction.is_empty() && fraction.bytes().all(|b| b.is_ascii_digit())
        }
    }
}

enum Token<'a> {
    Positional,
    Help,
    Flag { flag: &'a Flag, explicit: Option<&'a str> },
    Unknown,
}

fn classify<'a>(arg: &'a str, flags: &'a [Flag]) -> Token<'a> {
    if !arg.starts_with('-') || arg.is_empty() {
        return Token::Positional;
    }
    if arg == "-h" || arg == "--help" {
        return Token::Help;
    }
    if let Some(flag) = flags.iter().find(|flag| flag.name == arg) {
        return Token::Flag { flag, explicit: None };
    }
    if arg.len() == 1 {
        return Token::Positional;
    }
    if let Some((name, value)) = arg.split_once('=') {
        if name == "--help" {
            return Token::Help;
        }
        if let Some(flag) = flags.iter().find(|flag| flag.name == name) {
            return Token::Flag { flag, explicit: Some(value) };
        }
    }
    if arg.starts_with("--") {
        let (prefix, explicit) = match arg.split_once('=') {
            Some((prefix, value)) => (prefix, Some(value)),
            None => (arg, None),
        };
        if "--help".starts_with(prefix) {
            return Token::Help;
        }
        let mut matches = flags.iter().filter(|flag| flag.name.starts_with(prefix));
        if let Some(flag) = matches.next() {
            return Token::Flag { flag, explicit };
        }
    } else if arg.starts_with("-h") {
        return Token::Help;
    }
    if is_negative_number(arg) || arg.contains(' ') {
        return Token::Positional;
    }
    Token::Unknown
}

struct Collected {
    values: Vec<(Slot, String)>,
    positional: Option<String>,
    extras: Vec<String>,
}

enum Scan {
    Help,
    Collected(Collected),
    Failed(Failure),
}

fn scan(command: &Subcommand, args: &[String]) -> Scan {
    let usage = usage_of(command.help);
    let prog = format!("sx {}", command.word);
    let failure = |message: String| Scan::Failed(Failure { usage, prog: prog.clone(), message });
    let mut collected = Collected { values: Vec::new(), positional: None, extras: Vec::new() };
    let mut at = 0;
    let mut after_separator = false;
    while let Some(arg) = args.get(at) {
        at += 1;
        if !after_separator && arg == "--" {
            after_separator = true;
            continue;
        }
        let token = if after_separator { Token::Positional } else { classify(arg, command.flags) };
        match token {
            Token::Help => return Scan::Help,
            Token::Unknown => collected.extras.push(arg.clone()),
            Token::Positional => match (command.positional, &collected.positional) {
                (Positional::None, _) | (_, Some(_)) => collected.extras.push(arg.clone()),
                (_, None) => collected.positional = Some(arg.clone()),
            },
            Token::Flag { flag, explicit } => {
                let value = match explicit {
                    Some(value) => value.to_string(),
                    None => match args.get(at) {
                        Some(next) if next != "--" && matches!(classify(next, command.flags), Token::Positional) => {
                            at += 1;
                            next.clone()
                        }
                        _ => return failure(format!("argument {}: expected one argument", flag.name)),
                    },
                };
                if let Some(choices) = flag.choices
                    && !choices.contains(&value.as_str())
                {
                    return failure(format!("argument {}: {}", flag.name, invalid_choice(&value, choices)));
                }
                collected.values.push((flag.slot, value));
            }
        }
    }
    if let (Positional::Required(name), None) = (command.positional, &collected.positional) {
        return failure(format!("the following arguments are required: {name}"));
    }
    Scan::Collected(collected)
}

fn last(collected: &Collected, slot: Slot) -> Option<String> {
    collected.values.iter().rev().find(|(held, _)| *held == slot).map(|(_, value)| value.clone())
}

fn all(collected: &Collected, slot: Slot) -> Vec<String> {
    collected.values.iter().filter(|(held, _)| *held == slot).map(|(_, value)| value.clone()).collect()
}

fn format_of(collected: &Collected) -> Format {
    match last(collected, Slot::Format).as_deref() {
        Some("json") => Format::Json,
        _ => Format::Text,
    }
}

fn build(command: &Subcommand, collected: Collected) -> Invocation {
    match command.name {
        Name::Help => Invocation::Help(HelpArgs {
            name: collected.positional.clone(),
            declarations: last(&collected, Slot::Declarations),
            level: last(&collected, Slot::Level)
                .and_then(|level| GuideLevel::from_name(&level))
                .unwrap_or(GuideLevel::Standard),
        }),
        Name::Cores => Invocation::Cores(CoresArgs { format: format_of(&collected) }),
        Name::Check | Name::Explain | Name::Run => {
            let kind = match command.name {
                Name::Check => ProgramKind::Check { emit_ir: last(&collected, Slot::Emit).is_some() },
                Name::Explain => ProgramKind::Explain,
                _ => ProgramKind::Run {
                    responses: last(&collected, Slot::Responses),
                    inputs: all(&collected, Slot::Input),
                },
            };
            Invocation::Program(ProgramArgs {
                kind,
                file: collected.positional.clone().unwrap_or_default(),
                declarations: last(&collected, Slot::Declarations),
                allow: all(&collected, Slot::Allow),
                format: format_of(&collected),
            })
        }
    }
}

fn top_failure(message: String) -> Parsed {
    Parsed::Done(Failure { usage: usage_of(TOP_HELP), prog: "sx".to_string(), message }.into_exit())
}

pub fn parse(args: &[String]) -> Parsed {
    let mut extras: Vec<String> = Vec::new();
    let mut at = 0;
    let mut command_word: Option<&String> = None;
    while let Some(arg) = args.get(at) {
        at += 1;
        if arg == "--" {
            continue;
        }
        match classify(arg, &[]) {
            Token::Help => return help_exit(TOP_HELP),
            Token::Unknown => extras.push(arg.clone()),
            Token::Positional | Token::Flag { .. } => {
                command_word = Some(arg);
                break;
            }
        }
    }
    let Some(word) = command_word else {
        return top_failure("the following arguments are required: command".to_string());
    };
    let Some(command) = SUBCOMMANDS.iter().find(|command| command.word == word) else {
        return top_failure(format!("argument command: {}", invalid_choice(word, &COMMAND_WORDS)));
    };
    let rest = args.get(at..).unwrap_or_default();
    match scan(command, rest) {
        Scan::Help => help_exit(command.help),
        Scan::Failed(failure) => Parsed::Done(failure.into_exit()),
        Scan::Collected(mut collected) => {
            extras.append(&mut collected.extras);
            if !extras.is_empty() {
                return top_failure(format!("unrecognized arguments: {}", extras.join(" ")));
            }
            Parsed::Invoke(build(command, collected))
        }
    }
}
