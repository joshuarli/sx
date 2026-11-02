use std::io;

use sx_kernel::authority::{GrantError, Grants};
use sx_kernel::json::{Layout, dumps};
use sx_kernel::registry::Registry;
use sx_packs::mcp::{Server, serve as serve_connection};

use crate::commands::{Stop, load_registry, read_json};
use crate::mcp::{Options, scan, usage};
use crate::pyfile::path_text;
use crate::{EXIT_FAILED, EXIT_OK, Exit};

pub(crate) fn stopped(stop: Stop) -> Exit {
    match stop {
        Stop::Usage(message) => usage(&message),
        Stop::Internal(message) => Exit { code: EXIT_FAILED, stdout: String::new(), stderr: format!("sx mcp: internal error: {message}\n") },
    }
}

pub(crate) fn grants_of(options: &Options, registry: &Registry) -> Result<Grants, Stop> {
    let mut grants = match options.last("--grants") {
        None => Grants::new(),
        Some(path) => {
            let json = read_json(path, "grants")?;
            Grants::load(registry, &json)
                .map_err(|e| Stop::Usage(format!("invalid grants {}: {e}", path_text(path))))?
        }
    };
    for id in options.all("--allow") {
        grants.grant_unscoped(registry, &id).map_err(|e| match e {
            GrantError::MissingScope { .. } => Stop::Usage(format!("--allow {id}: {e}; grant it with --grants")),
            other => Stop::Usage(format!("--allow {id}: {other}")),
        })?;
    }
    Ok(grants)
}

fn build(args: &[String], new_registry: &dyn Fn() -> Registry, what: &str) -> Result<Server, Exit> {
    let options = scan(args, &["--declarations", "--grants", "--allow"]).map_err(|message| usage(&message))?;
    if let Some(extra) = options.positional.first() {
        return Err(usage(&format!("unrecognized argument {extra}")));
    }
    if options.command.is_some() {
        return Err(usage(&format!("sx mcp {what} starts no server command")));
    }
    let Some(declarations) = options.last("--declarations") else {
        return Err(usage(&format!("sx mcp {what} needs --declarations FILE")));
    };
    let (registry, _, endpoints) = load_registry(Some(declarations), new_registry()).map_err(stopped)?;
    let grants = grants_of(&options, &registry).map_err(stopped)?;
    let server = Server::new(&registry, grants, endpoints).map_err(|message| usage(&message))?;
    for skipped in server.not_served() {
        eprintln!("sx mcp {what}: not serving {}: {}", skipped.id, skipped.reason);
    }
    if server.names().next().is_none() {
        return Err(usage("no granted capability can be served; grant one with --grants or --allow"));
    }
    Ok(server)
}

pub(crate) fn describe(args: &[String], new_registry: &dyn Fn() -> Registry) -> Exit {
    let server = match build(args, new_registry, "describe") {
        Ok(server) => server,
        Err(exit) => return exit,
    };
    match dumps(&server.describe(), Layout::Indented) {
        Ok(text) => Exit { code: EXIT_OK, stdout: format!("{text}\n"), stderr: String::new() },
        Err(error) => Exit { code: EXIT_FAILED, stdout: String::new(), stderr: format!("sx mcp: {}\n", error.0) },
    }
}

pub(crate) fn serve(args: &[String], new_registry: &dyn Fn() -> Registry) -> Exit {
    let server = match build(args, new_registry, "serve") {
        Ok(server) => server,
        Err(exit) => return exit,
    };
    match serve_connection(&server, io::stdin().lock(), io::stdout()) {
        Ok(()) => Exit { code: EXIT_OK, stdout: String::new(), stderr: String::new() },
        Err(message) => Exit { code: EXIT_FAILED, stdout: String::new(), stderr: format!("sx mcp: {message}\n") },
    }
}
