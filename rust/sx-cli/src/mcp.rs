use std::path::Path;
use std::time::Duration;

use sx_kernel::json::{Json, Layout, dumps, parse};
use sx_kernel::registry::Registry;
use sx_packs::mcp::{self, Connection, Limits};

use crate::argparse::{Format, ProgramArgs, ProgramKind};
use crate::pyfile::{path_text, read_text};
use crate::{EXIT_FAILED, EXIT_OK, EXIT_USAGE, Exit};

const MCP_HELP: &str = r#"usage: sx mcp import --name NAME (--tools FILE | -- COMMAND [ARG ...])
                    [--timeout-ms MS]
       sx mcp run FILE --declarations FILE [--allow ID ...] [--grants FILE]
                  [--input NAME=JSON ...] [--format {text,json}] [--name NAME]
                  [--timeout-ms MS] -- COMMAND [ARG ...]

Import the tools of an MCP server as Sx declarations, and run a program
against that server over stdio.

import options:
  --name NAME       Server name; its tools become capabilities mcp-NAME.TOOL
  --tools FILE      Read a saved tools/list result instead of starting a server
  --timeout-ms MS   Wait at most MS milliseconds for each server answer

run options:
  --declarations FILE  Declarations written by sx mcp import
  --allow ID           Authorize one tool
  --grants FILE        JSON object granting tools, with their scopes
  --input NAME=JSON    Supply a declared input
  --format {text,json}
  --name NAME          Serve the tools of this server when the declarations
                       hold several
  --timeout-ms MS      Per-call timeout, 60000 by default
"#;

const SAVED_RESULT_TOOLS: &str = "tools";

#[derive(Clone, Debug)]
pub(crate) struct Host {
    pub command: Vec<String>,
    pub name: Option<String>,
    pub limits: Limits,
}

struct Options {
    values: Vec<(String, String)>,
    positional: Vec<String>,
    command: Option<Vec<String>>,
}

fn usage(message: &str) -> Exit {
    Exit { code: EXIT_USAGE, stdout: String::new(), stderr: format!("sx mcp: {message}\n") }
}

fn scan(args: &[String], flags: &[&str]) -> Result<Options, String> {
    let (own, command) = match args.iter().position(|arg| arg == "--") {
        Some(at) => (args.get(..at).unwrap_or_default(), Some(args.get(at + 1..).unwrap_or_default().to_vec())),
        None => (args, None),
    };
    let mut values = Vec::new();
    let mut positional = Vec::new();
    let mut at = 0;
    while let Some(arg) = own.get(at) {
        at += 1;
        if !arg.starts_with("--") {
            positional.push(arg.clone());
            continue;
        }
        let (name, explicit) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (arg.as_str(), None),
        };
        if !flags.contains(&name) {
            return Err(format!("unrecognized argument {arg}"));
        }
        let value = match explicit {
            Some(value) => value,
            None => {
                let next = own.get(at).ok_or_else(|| format!("argument {name}: expected one argument"))?;
                at += 1;
                next.clone()
            }
        };
        values.push((name.to_string(), value));
    }
    Ok(Options { values, positional, command })
}

impl Options {
    fn last(&self, flag: &str) -> Option<&str> {
        self.values.iter().rev().find(|(name, _)| name == flag).map(|(_, value)| value.as_str())
    }

    fn all(&self, flag: &str) -> Vec<String> {
        self.values.iter().filter(|(name, _)| name == flag).map(|(_, value)| value.clone()).collect()
    }

    fn timeout(&self) -> Result<Option<Duration>, String> {
        match self.last("--timeout-ms") {
            None => Ok(None),
            Some(text) => match text.parse::<u64>() {
                Ok(milliseconds) if milliseconds > 0 => Ok(Some(Duration::from_millis(milliseconds))),
                _ => Err(format!("argument --timeout-ms: {text:?} is not a positive number of milliseconds")),
            },
        }
    }

    fn command(&self) -> Result<Vec<String>, String> {
        match &self.command {
            Some(command) if !command.is_empty() => Ok(command.clone()),
            _ => Err("a server command is needed after --".to_string()),
        }
    }
}

pub fn run(args: &[String], new_registry: &dyn Fn() -> Registry) -> Exit {
    match args.split_first() {
        Some((word, _)) if word == "-h" || word == "--help" => {
            Exit { code: EXIT_OK, stdout: MCP_HELP.to_string(), stderr: String::new() }
        }
        Some((word, rest)) if word == "import" => import(rest),
        Some((word, rest)) if word == "run" => run_program(rest, new_registry),
        Some((word, _)) => usage(&format!("{word:?} is not a subcommand; choose import or run")),
        None => usage("choose a subcommand: import or run"),
    }
}

fn saved_tools(path: &str) -> Result<Vec<Json>, String> {
    let shown = path_text(path);
    let text = read_text(Path::new(path)).map_err(|e| format!("cannot read {shown}: {e}"))?;
    let saved = parse(&text).map_err(|e| format!("cannot read {shown}: {e}"))?;
    if !matches!(saved.get("nextCursor"), None | Some(Json::Null)) {
        return Err(format!("{shown} is one page of tools/list; merge every page into one tools list first"));
    }
    match saved.get(SAVED_RESULT_TOOLS).and_then(Json::as_array) {
        Some(tools) => Ok(tools.to_vec()),
        None => Err(format!("{shown} must be a tools/list result: an object with a tools list")),
    }
}

fn import(args: &[String]) -> Exit {
    let options = match scan(args, &["--name", "--tools", "--timeout-ms"]) {
        Ok(options) => options,
        Err(message) => return usage(&message),
    };
    let Some(name) = options.last("--name") else {
        return usage("sx mcp import needs --name NAME");
    };
    if let Some(extra) = options.positional.first() {
        return usage(&format!("unrecognized argument {extra}"));
    }
    let timeout = match options.timeout() {
        Ok(timeout) => timeout.unwrap_or(mcp::STANDARD_TIMEOUT),
        Err(message) => return usage(&message),
    };
    let tools = match (options.last("--tools"), &options.command) {
        (Some(_), Some(_)) => return usage("give --tools or a server command, not both"),
        (Some(path), None) => match saved_tools(path) {
            Ok(tools) => tools,
            Err(message) => return usage(&message),
        },
        (None, _) => {
            let command = match options.command() {
                Ok(command) => command,
                Err(message) => return usage(&message),
            };
            let listed = Connection::open(&command, timeout).and_then(|connection| connection.list_tools(timeout));
            match listed {
                Ok(tools) => tools,
                Err(message) => return failure(&message),
            }
        }
    };
    let imported = match mcp::import(name, &tools) {
        Ok(imported) => imported,
        Err(mcp::ImportError::ServerName(message)) => return usage(&message),
        Err(error) => return failure(&error.to_string()),
    };
    let mut stderr = String::new();
    for skipped in &imported.skipped {
        stderr.push_str(&format!("sx mcp import: skipped {}: {}\n", skipped.tool, skipped.reason));
    }
    let kept = imported.declarations.get("capabilities").and_then(Json::as_array).map_or(0, <[Json]>::len);
    stderr.push_str(&format!("sx mcp import: imported {kept} tools, skipped {}\n", imported.skipped.len()));
    match dumps(&imported.declarations, Layout::Indented) {
        Ok(text) => Exit { code: EXIT_OK, stdout: format!("{text}\n"), stderr },
        Err(error) => failure(&error.0),
    }
}

fn failure(message: &str) -> Exit {
    Exit { code: EXIT_FAILED, stdout: String::new(), stderr: format!("sx mcp: {message}\n") }
}

fn run_program(args: &[String], new_registry: &dyn Fn() -> Registry) -> Exit {
    let flags = ["--declarations", "--allow", "--grants", "--input", "--format", "--name", "--timeout-ms"];
    let options = match scan(args, &flags) {
        Ok(options) => options,
        Err(message) => return usage(&message),
    };
    let file = match options.positional.as_slice() {
        [file] => file.clone(),
        [] => return usage("sx mcp run needs a program file"),
        [_, extra, ..] => return usage(&format!("unrecognized argument {extra}")),
    };
    let Some(declarations) = options.last("--declarations") else {
        return usage("sx mcp run needs --declarations FILE");
    };
    let format = match options.last("--format") {
        None | Some("text") => Format::Text,
        Some("json") => Format::Json,
        Some(other) => return usage(&format!("argument --format: {other:?} is not text or json")),
    };
    let command = match options.command() {
        Ok(command) => command,
        Err(message) => return usage(&message),
    };
    let limits = match options.timeout() {
        Ok(timeout) => Limits { call: timeout.unwrap_or(Limits::STANDARD.call), ..Limits::STANDARD },
        Err(message) => return usage(&message),
    };
    let program = ProgramArgs {
        kind: ProgramKind::Run { responses: None, inputs: options.all("--input") },
        file,
        declarations: Some(declarations.to_string()),
        allow: options.all("--allow"),
        grants: options.last("--grants").map(str::to_string),
        format,
    };
    let host = Host { command, name: options.last("--name").map(str::to_string), limits };
    crate::commands::execute_on_mcp(&program, new_registry(), &host)
}
