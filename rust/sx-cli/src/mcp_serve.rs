use std::io;

use sx_kernel::authority::{GrantError, Grants};
use sx_kernel::registry::Registry;
use sx_packs::mcp::{Server, serve as serve_connection};

use crate::commands::{Stop, load_registry, read_json};
use crate::mcp::{Options, scan, usage};
use crate::pyfile::path_text;
use crate::{EXIT_FAILED, EXIT_OK, Exit};

fn stopped(stop: Stop) -> Exit {
    match stop {
        Stop::Usage(message) => usage(&message),
        Stop::Internal(message) => Exit { code: EXIT_FAILED, stdout: String::new(), stderr: format!("sx mcp: internal error: {message}\n") },
    }
}

fn grants_of(options: &Options, registry: &Registry) -> Result<Grants, Stop> {
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

pub(crate) fn serve(args: &[String], new_registry: &dyn Fn() -> Registry) -> Exit {
    let options = match scan(args, &["--declarations", "--grants", "--allow"]) {
        Ok(options) => options,
        Err(message) => return usage(&message),
    };
    if let Some(extra) = options.positional.first() {
        return usage(&format!("unrecognized argument {extra}"));
    }
    if options.command.is_some() {
        return usage("sx mcp serve starts no server command");
    }
    let Some(declarations) = options.last("--declarations") else {
        return usage("sx mcp serve needs --declarations FILE");
    };
    let (registry, _, endpoints) = match load_registry(Some(declarations), new_registry()) {
        Ok(loaded) => loaded,
        Err(stop) => return stopped(stop),
    };
    let grants = match grants_of(&options, &registry) {
        Ok(grants) => grants,
        Err(stop) => return stopped(stop),
    };
    let server = match Server::new(&registry, grants, endpoints) {
        Ok(server) => server,
        Err(message) => return usage(&message),
    };
    for skipped in server.not_served() {
        eprintln!("sx mcp serve: not serving {}: {}", skipped.id, skipped.reason);
    }
    if server.names().next().is_none() {
        return usage("no granted capability can be served; grant one with --grants or --allow");
    }
    match serve_connection(&server, io::stdin().lock(), io::stdout()) {
        Ok(()) => Exit { code: EXIT_OK, stdout: String::new(), stderr: String::new() },
        Err(message) => Exit { code: EXIT_FAILED, stdout: String::new(), stderr: format!("sx mcp: {message}\n") },
    }
}
