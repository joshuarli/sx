use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use sx_kernel::authority::{ScopeCheck, Verdict};
use sx_kernel::registry::{Capability, Effect, Pack, PackCapability};
use sx_kernel::types::{CallErrorKind, EnumType, INT, RecordType, STRING, Type};
use sx_kernel::values::{RecordValue, Value, Variant, show_value};
use std::sync::Arc;

use crate::fs::{Roots, canonical_roots, field, resolve_read, shown, strings, text_field};
use crate::{Failure, Performer};

pub const RUN: &str = "process.run";

const COMMAND: &str = "process.Command";
const ARG: &str = "process.Arg";
const OUTPUT: &str = "process.Output";

pub const OUTPUT_LIMIT: usize = 1 << 20;
pub const TIMEOUT: Duration = Duration::from_secs(60);
const REAP_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub timeout: Duration,
    pub output: usize,
}

impl Limits {
    pub const STANDARD: Limits = Limits { timeout: TIMEOUT, output: OUTPUT_LIMIT };
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Arg {
    Exact(String),
    OneOf(Vec<String>),
    AnyOne,
    Rest,
}

impl Arg {
    fn accepts(&self, argument: &str) -> bool {
        match self {
            Arg::Exact(expected) => expected == argument,
            Arg::OneOf(allowed) => allowed.iter().any(|candidate| candidate == argument),
            Arg::AnyOne | Arg::Rest => true,
        }
    }

    fn value(&self) -> Value {
        let (variant, payload) = match self {
            Arg::Exact(text) => ("Exact", Some(Value::string(text))),
            Arg::OneOf(texts) => ("OneOf", Some(Value::list(texts.iter().map(|text| Value::string(text)).collect()))),
            Arg::AnyOne => ("AnyOne", None),
            Arg::Rest => ("Rest", None),
        };
        Value::Variant(Arc::new(Variant { enum_name: Arc::from(ARG), variant: Arc::from(variant), payload }))
    }

    fn of(value: &Value) -> Result<Arg, String> {
        let Value::Variant(variant) = value else {
            return Err("an argument pattern must be (Exact S), (OneOf [S ...]), (AnyOne), or (Rest)".to_string());
        };
        match (&*variant.variant, &variant.payload) {
            ("Exact", Some(Value::String(text))) => Ok(Arg::Exact(text.to_string())),
            ("OneOf", Some(payload)) => match strings(payload) {
                Some(texts) => Ok(Arg::OneOf(texts.into_iter().map(str::to_string).collect())),
                None => Err("OneOf takes a list of strings".to_string()),
            },
            ("AnyOne", None) => Ok(Arg::AnyOne),
            ("Rest", None) => Ok(Arg::Rest),
            (other, _) => Err(format!("{other} is not an argument pattern")),
        }
    }

    fn has_nul(&self) -> bool {
        match self {
            Arg::Exact(text) => text.contains('\0'),
            Arg::OneOf(texts) => texts.iter().any(|text| text.contains('\0')),
            Arg::AnyOne | Arg::Rest => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Granted {
    program: String,
    args: Vec<Arg>,
}

impl Granted {
    fn accepts(&self, arguments: &[&str]) -> bool {
        let mut remaining = arguments.iter();
        for pattern in &self.args {
            if *pattern == Arg::Rest {
                return true;
            }
            match remaining.next() {
                Some(argument) if pattern.accepts(argument) => {}
                _ => return false,
            }
        }
        remaining.next().is_none()
    }

    fn value(&self) -> Value {
        Value::Record(RecordValue::new(vec![
            (Arc::from("program"), Value::string(&self.program)),
            (Arc::from("args"), Value::list(self.args.iter().map(Arg::value).collect())),
        ]))
    }
}

fn record(name: Option<&str>, fields: Vec<(&str, Type)>) -> Type {
    Type::Record(RecordType {
        fields: fields.into_iter().map(|(n, t)| (n.to_string(), t)).collect(),
        name: name.map(str::to_string),
        open: false,
    })
}

fn arg_type() -> Type {
    Type::Enum(Arc::new(EnumType {
        name: ARG.to_string(),
        variants: vec![
            ("Exact".to_string(), Some(STRING)),
            ("OneOf".to_string(), Some(Type::List(Box::new(STRING)))),
            ("AnyOne".to_string(), None),
            ("Rest".to_string(), None),
        ],
    }))
}

fn command_type() -> Type {
    record(Some(COMMAND), vec![("program", STRING), ("args", Type::List(Box::new(arg_type())))])
}

fn output_type() -> Type {
    record(Some(OUTPUT), vec![("status", INT), ("stdout", STRING), ("stderr", STRING)])
}

pub fn scope_type() -> Type {
    record(None, vec![("commands", Type::List(Box::new(command_type()))), ("roots", Type::List(Box::new(STRING)))])
}

fn input_type() -> Type {
    record(
        None,
        vec![
            ("program", STRING),
            ("args", Type::List(Box::new(STRING))),
            ("cwd", STRING),
            ("stdin", Type::Option(Box::new(STRING))),
        ],
    )
}

pub fn pack() -> Pack {
    Pack {
        id: "process".to_string(),
        types: vec![
            (ARG.to_string(), arg_type()),
            (COMMAND.to_string(), command_type()),
            (OUTPUT.to_string(), output_type()),
        ],
        capabilities: vec![PackCapability {
            capability: Capability {
                id: RUN.to_string(),
                effect: Effect::Write,
                input: input_type(),
                output: output_type(),
                description: "Runs one granted program with exactly the given arguments, never through a shell."
                    .to_string(),
                idempotent: false,
                scope: Some(scope_type()),
            },
            check: Some(ScopeCheck { prepare, check }),
        }],
    }
}

pub fn performer(capability: &str) -> Option<Performer> {
    match capability {
        RUN => Some(run),
        _ => None,
    }
}

fn granted_commands(scope: &Value) -> Result<Vec<Granted>, String> {
    let Some(Value::List(items)) = field(scope, "commands") else {
        return Err("a process scope must be {:commands [{:program P :args [PATTERN ...]} ...] :roots [PATH ...]}".to_string());
    };
    items
        .iter()
        .map(|item| {
            let program = text_field(item, "program").ok_or("a command needs a program")?;
            let Some(Value::List(patterns)) = field(item, "args") else {
                return Err(format!("the command {} needs an args list", shown(program)));
            };
            let args = patterns.iter().map(Arg::of).collect::<Result<Vec<_>, _>>()?;
            Ok(Granted { program: program.to_string(), args })
        })
        .collect()
}

fn prepare(scope: &Value) -> Result<Value, String> {
    let commands = granted_commands(scope)?;
    if commands.is_empty() {
        return Err("a process scope needs at least one command".to_string());
    }
    let roots = field(scope, "roots").and_then(strings).ok_or("a process scope needs a list of roots")?;
    if roots.is_empty() {
        return Err("a process scope needs at least one root".to_string());
    }
    let mut prepared = Vec::new();
    for command in commands {
        let shown_program = shown(&command.program);
        if command.args.iter().take(command.args.len().saturating_sub(1)).any(|arg| *arg == Arg::Rest) {
            return Err(format!("in the command {shown_program}, (Rest) is allowed only as the last argument pattern"));
        }
        if command.args.iter().any(|arg| matches!(arg, Arg::OneOf(texts) if texts.is_empty())) {
            return Err(format!("in the command {shown_program}, (OneOf []) matches nothing"));
        }
        if command.args.iter().any(Arg::has_nul) {
            return Err(format!("in the command {shown_program}, an argument pattern contains a NUL character"));
        }
        let canonical = granted_program(&command.program)?;
        prepared.push(Granted { program: canonical, args: command.args }.value());
    }
    Ok(Value::Record(RecordValue::new(vec![
        (Arc::from("commands"), Value::list(prepared)),
        (Arc::from("roots"), Value::list(canonical_roots(&roots)?)),
    ])))
}

fn granted_program(program: &str) -> Result<String, String> {
    let canonical = canonical_program(program).map_err(|reason| format!("the granted program {reason}"))?;
    let metadata = fs::metadata(&canonical).map_err(|e| format!("the granted program {} cannot be read: {e}", shown(program)))?;
    if !metadata.is_file() {
        return Err(format!("the granted program {} is not a regular file", shown(program)));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!("the granted program {} is not executable", shown(program)));
    }
    canonical
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("the granted program {} resolves to a path that is not valid UTF-8", shown(program)))
}

fn canonical_program(program: &str) -> Result<PathBuf, String> {
    if program.contains('\0') {
        return Err(format!("{} contains a NUL character", shown(program)));
    }
    if !program.starts_with('/') {
        return Err(format!("{} is not an absolute path, and process.run never searches PATH", shown(program)));
    }
    fs::canonicalize(program).map_err(|e| match e.kind() {
        ErrorKind::NotFound => format!("{} does not exist", shown(program)),
        _ => format!("{} cannot be resolved: {e}", shown(program)),
    })
}

struct Request<'v> {
    program: &'v str,
    args: Vec<&'v str>,
    cwd: &'v str,
    stdin: Option<&'v str>,
}

impl<'v> Request<'v> {
    fn of(input: &'v Value) -> Option<Request<'v>> {
        let stdin = match field(input, "stdin")? {
            Value::None => None,
            Value::Some(inner) => match &**inner {
                Value::String(text) => Some(&**text),
                _ => return None,
            },
            _ => return None,
        };
        Some(Request {
            program: text_field(input, "program")?,
            args: strings(field(input, "args")?)?,
            cwd: text_field(input, "cwd")?,
            stdin,
        })
    }
}

struct Authorized {
    program: PathBuf,
    cwd: PathBuf,
}

fn authorize(scope: &Value, request: &Request<'_>) -> Result<Authorized, String> {
    let commands = granted_commands(scope)?;
    let roots = Roots::of(scope)?;
    let program = canonical_program(request.program)?;
    let canonical = program.to_str().ok_or_else(|| format!("{} resolves to a path that is not valid UTF-8", shown(request.program)))?;
    if !commands.iter().any(|command| command.program == canonical) {
        return Err(if canonical == request.program {
            format!("{} is not a granted program", shown(request.program))
        } else {
            format!("{} resolves to {canonical}, which is not a granted program", shown(request.program))
        });
    }
    if let Some(position) = request.args.iter().position(|argument| argument.contains('\0')) {
        return Err(format!("argument {} contains a NUL character", position + 1));
    }
    if !commands.iter().any(|command| command.program == canonical && command.accepts(&request.args)) {
        let arguments = Value::list(request.args.iter().map(|argument| Value::string(argument)).collect());
        return Err(format!("the arguments {} match no granted argument list for {canonical}", show_value(&arguments)));
    }
    let cwd = resolve_read(&roots, request.cwd)?;
    Ok(Authorized { program, cwd })
}

fn check(scope: &Value, input: &Value) -> Verdict {
    let Some(request) = Request::of(input) else {
        return Verdict::Denied("the input does not match process.run's type".to_string());
    };
    match authorize(scope, &request) {
        Ok(_) => Verdict::Allowed,
        Err(reason) => Verdict::Denied(reason),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub arg0: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub stdin: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub status: i64,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    fn value(&self) -> Value {
        Value::Record(RecordValue::new(vec![
            (Arc::from("status"), Value::Int(self.status)),
            (Arc::from("stdout"), Value::string(&self.stdout)),
            (Arc::from("stderr"), Value::string(&self.stderr)),
        ]))
    }
}

fn run(scope: &Value, input: &Value) -> Result<Value, Failure> {
    let request = Request::of(input)
        .ok_or_else(|| Failure::new(CallErrorKind::Failed, format!("{RUN} received an input that does not match its type")))?;
    let authorized =
        authorize(scope, &request).map_err(|reason| Failure::new(CallErrorKind::PermissionDenied, reason))?;
    let invocation = Invocation {
        program: authorized.program,
        arg0: request.program.to_string(),
        args: request.args.iter().map(|argument| argument.to_string()).collect(),
        cwd: authorized.cwd,
        stdin: request.stdin.map(str::to_string),
    };
    Ok(execute(&invocation, Limits::STANDARD)?.value())
}

#[derive(Clone, Copy, Debug)]
struct Group(libc::pid_t);

impl Group {
    fn of(pid: u32) -> Option<Group> {
        libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 1).map(Group)
    }

    #[allow(unsafe_code)]
    fn kill(self) {
        // SAFETY: killpg takes two integers and touches no memory. The id is above 1, so it
        // never names the caller's own group (0) or every process (-1 as a pid).
        unsafe {
            libc::killpg(self.0, libc::SIGKILL);
        }
    }
}

struct KillOnDrop(Group);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.0.kill();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    fn name(self) -> &'static str {
        match self {
            Stream::Stdout => "stdout",
            Stream::Stderr => "stderr",
        }
    }
}

enum Captured {
    Complete(Vec<u8>),
    Overflow,
    Unreadable(io::Error),
}

enum Event {
    Exited(io::Result<ExitStatus>),
    Output(Stream, Captured),
}

fn capture(mut pipe: impl Read, limit: usize) -> Captured {
    let mut collected = Vec::new();
    match (&mut pipe).take(limit as u64 + 1).read_to_end(&mut collected) {
        Ok(_) if collected.len() > limit => Captured::Overflow,
        Ok(_) => Captured::Complete(collected),
        Err(error) => Captured::Unreadable(error),
    }
}

fn background(name: &str, work: impl FnOnce() + Send + 'static) -> Result<(), Failure> {
    thread::Builder::new()
        .name(name.to_string())
        .spawn(work)
        .map(drop)
        .map_err(|e| Failure::new(CallErrorKind::Failed, format!("cannot start the {name} thread: {e}")))
}

fn spawn_failure(program: &Path, error: &io::Error) -> Failure {
    let kind = match error.kind() {
        ErrorKind::NotFound => CallErrorKind::NotFound,
        ErrorKind::PermissionDenied => CallErrorKind::PermissionDenied,
        _ => CallErrorKind::Failed,
    };
    Failure::new(kind, format!("cannot start {}: {error}", program.display()))
}

fn directory_failure(invocation: &Invocation) -> Option<Failure> {
    let shown_cwd = invocation.cwd.display();
    match fs::metadata(&invocation.cwd) {
        Ok(metadata) if metadata.is_dir() => None,
        Ok(_) => Some(Failure::new(CallErrorKind::Invalid, format!("the working directory {shown_cwd} is not a directory"))),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Some(Failure::new(CallErrorKind::NotFound, format!("the working directory {shown_cwd} does not exist")))
        }
        Err(error) => Some(Failure::new(
            if error.kind() == ErrorKind::PermissionDenied { CallErrorKind::PermissionDenied } else { CallErrorKind::Failed },
            format!("the working directory {shown_cwd} cannot be read: {error}"),
        )),
    }
}

fn signal_name(signal: i32) -> String {
    let name = match signal {
        libc::SIGHUP => "SIGHUP",
        libc::SIGINT => "SIGINT",
        libc::SIGQUIT => "SIGQUIT",
        libc::SIGILL => "SIGILL",
        libc::SIGABRT => "SIGABRT",
        libc::SIGFPE => "SIGFPE",
        libc::SIGKILL => "SIGKILL",
        libc::SIGBUS => "SIGBUS",
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGPIPE => "SIGPIPE",
        libc::SIGALRM => "SIGALRM",
        libc::SIGTERM => "SIGTERM",
        libc::SIGUSR1 => "SIGUSR1",
        libc::SIGUSR2 => "SIGUSR2",
        _ => return format!("signal {signal}"),
    };
    format!("{name} (signal {signal})")
}

fn reap(events: &Receiver<Event>) {
    let until = Instant::now() + REAP_GRACE;
    while let Ok(event) = events.recv_timeout(until.saturating_duration_since(Instant::now())) {
        if matches!(event, Event::Exited(_)) {
            return;
        }
    }
}

#[derive(Default)]
struct Gathered {
    exit: Option<io::Result<ExitStatus>>,
    stdout: Option<Vec<u8>>,
    stderr: Option<Vec<u8>>,
}

impl Gathered {
    fn finished(&self) -> bool {
        self.exit.is_some() && self.stdout.is_some() && self.stderr.is_some()
    }
}

fn gather(
    gathered: &mut Gathered,
    invocation: &Invocation,
    limits: Limits,
    events: &Receiver<Event>,
    group: Group,
) -> Result<(), Failure> {
    let program = invocation.program.display();
    let deadline = Instant::now() + limits.timeout;
    while !gathered.finished() {
        match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Event::Exited(status)) => {
                group.kill();
                gathered.exit = Some(status);
            }
            Ok(Event::Output(Stream::Stdout, Captured::Complete(bytes))) => gathered.stdout = Some(bytes),
            Ok(Event::Output(Stream::Stderr, Captured::Complete(bytes))) => gathered.stderr = Some(bytes),
            Ok(Event::Output(stream, Captured::Overflow)) => {
                return Err(Failure::new(
                    CallErrorKind::Failed,
                    format!("{program} wrote more than {} bytes to {}, so it was killed", limits.output, stream.name()),
                ));
            }
            Ok(Event::Output(stream, Captured::Unreadable(error))) => {
                return Err(Failure::new(CallErrorKind::Failed, format!("cannot read {} of {program}: {error}", stream.name())));
            }
            Err(RecvTimeoutError::Timeout) => {
                let detail = if gathered.exit.is_some() {
                    "exited, but a process outside its group still holds its output open"
                } else {
                    "did not finish"
                };
                return Err(Failure::new(
                    CallErrorKind::Timeout,
                    format!("{program} {detail} within {} ms, so it was killed", limits.timeout.as_millis()),
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Failure::new(CallErrorKind::Failed, format!("the threads watching {program} stopped unexpectedly")));
            }
        }
    }
    Ok(())
}

fn finish(invocation: &Invocation, gathered: Gathered) -> Result<Output, Failure> {
    let program = invocation.program.display();
    let Gathered { exit: Some(exit), stdout: Some(stdout), stderr: Some(stderr) } = gathered else {
        return Err(Failure::new(CallErrorKind::Failed, format!("{program} finished without all of its results")));
    };
    let status = exit.map_err(|e| Failure::new(CallErrorKind::Failed, format!("cannot wait for {program}: {e}")))?;
    let Some(code) = status.code() else {
        let signal = status.signal().map_or_else(|| "an unknown cause".to_string(), signal_name);
        return Err(Failure::new(CallErrorKind::Failed, format!("{program} was terminated by {signal}")));
    };
    let decode = |bytes: Vec<u8>, stream: Stream| {
        String::from_utf8(bytes).map_err(|e| {
            Failure::new(
                CallErrorKind::Failed,
                format!("{program} exited with status {code}, but its {} is not valid UTF-8: {e}", stream.name()),
            )
        })
    };
    Ok(Output { status: i64::from(code), stdout: decode(stdout, Stream::Stdout)?, stderr: decode(stderr, Stream::Stderr)? })
}

fn start(invocation: &Invocation) -> Result<Child, Failure> {
    let mut command = Command::new(&invocation.program);
    command
        .arg0(&invocation.arg0)
        .args(&invocation.args)
        .current_dir(&invocation.cwd)
        .env_clear()
        .process_group(0)
        .stdin(if invocation.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.spawn().map_err(|e| spawn_failure(&invocation.program, &e))
}

pub fn execute(invocation: &Invocation, limits: Limits) -> Result<Output, Failure> {
    if let Some(failure) = directory_failure(invocation) {
        return Err(failure);
    }
    let mut child = start(invocation)?;
    let Some(group) = Group::of(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Failure::new(CallErrorKind::Failed, "the child process has no usable id".to_string()));
    };
    let _cleanup = KillOnDrop(group);
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(Failure::new(CallErrorKind::Failed, "the child process has no output pipes".to_string()));
    };
    let stdin = child.stdin.take();
    let (sender, events) = mpsc::channel();
    let exited = sender.clone();
    background("process-wait", move || {
        let _ = exited.send(Event::Exited(child.wait()));
    })?;
    let out = sender.clone();
    let limit = limits.output;
    background("process-stdout", move || {
        let _ = out.send(Event::Output(Stream::Stdout, capture(stdout, limit)));
    })?;
    background("process-stderr", move || {
        let _ = sender.send(Event::Output(Stream::Stderr, capture(stderr, limit)));
    })?;
    if let (Some(mut pipe), Some(text)) = (stdin, invocation.stdin.clone()) {
        background("process-stdin", move || {
            let _ = pipe.write_all(text.as_bytes());
        })?;
    }
    let mut gathered = Gathered::default();
    let result = gather(&mut gathered, invocation, limits, &events, group);
    group.kill();
    match result {
        Ok(()) => finish(invocation, gathered),
        Err(failure) => {
            if gathered.exit.is_none() {
                reap(&events);
            }
            Err(failure)
        }
    }
}
