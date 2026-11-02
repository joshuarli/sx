use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use sx_kernel::authority::{ScopeCheck, Verdict};
use sx_kernel::registry::{Capability, Effect, Pack, PackCapability};
use sx_kernel::types::{BYTES, CallErrorKind, EnumType, INT, RecordType, STRING, Type, UNIT};
use sx_kernel::values::{RecordValue, Value, Variant, show_value};

use crate::{Failure, Performer};

pub const READ_TEXT: &str = "fs.read-text";
pub const READ_BYTES: &str = "fs.read-bytes";
pub const LIST: &str = "fs.list";
pub const STAT: &str = "fs.stat";
pub const WRITE_TEXT: &str = "fs.write-text";
pub const MAKE_DIRECTORY: &str = "fs.make-directory";
pub const MOVE: &str = "fs.move";
pub const DELETE: &str = "fs.delete";

const KIND: &str = "fs.Kind";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
    Symlink,
    Other,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::File, Kind::Directory, Kind::Symlink, Kind::Other];

    fn name(self) -> &'static str {
        match self {
            Kind::File => "File",
            Kind::Directory => "Directory",
            Kind::Symlink => "Symlink",
            Kind::Other => "Other",
        }
    }

    fn of(file_type: fs::FileType) -> Kind {
        if file_type.is_symlink() {
            Kind::Symlink
        } else if file_type.is_dir() {
            Kind::Directory
        } else if file_type.is_file() {
            Kind::File
        } else {
            Kind::Other
        }
    }

    fn value(self) -> Value {
        Value::Variant(Arc::new(Variant { enum_name: Arc::from(KIND), variant: Arc::from(self.name()), payload: None }))
    }
}

fn record(name: Option<&str>, fields: Vec<(&str, Type)>) -> Type {
    Type::Record(RecordType {
        fields: fields.into_iter().map(|(n, t)| (n.to_string(), t)).collect(),
        name: name.map(str::to_string),
        open: false,
    })
}

fn kind_type() -> Type {
    Type::Enum(Arc::new(EnumType {
        name: KIND.to_string(),
        variants: Kind::ALL.iter().map(|kind| (kind.name().to_string(), None)).collect(),
    }))
}

fn entry_type() -> Type {
    record(Some("fs.Entry"), vec![("name", STRING), ("kind", kind_type())])
}

fn stat_type() -> Type {
    record(Some("fs.Stat"), vec![("kind", kind_type()), ("size", INT)])
}

pub fn scope_type() -> Type {
    record(None, vec![("roots", Type::List(Box::new(STRING)))])
}

fn path_input() -> Type {
    record(None, vec![("path", STRING)])
}

fn read_check() -> ScopeCheck {
    ScopeCheck::new(prepare, check_read)
}

fn write_check() -> ScopeCheck {
    ScopeCheck::new(prepare, check_write)
}

fn destructive_check(check: fn(&Value, &Value) -> Verdict) -> ScopeCheck {
    ScopeCheck::new(prepare, check)
}

fn capability(id: &str, effect: Effect, input: Type, output: Type, description: &str, check: ScopeCheck) -> PackCapability {
    PackCapability {
        capability: Capability {
            id: id.to_string(),
            effect,
            input,
            output,
            description: description.to_string(),
            idempotent: effect == Effect::Write && id != MOVE,
            scope: Some(scope_type()),
        },
        check: Some(check),
    }
}

pub fn pack() -> Pack {
    let capabilities = vec![
        capability(READ_TEXT, Effect::Read, path_input(), STRING, "Reads a whole file as UTF-8 text.", read_check()),
        capability(READ_BYTES, Effect::Read, path_input(), BYTES, "Reads a whole file as bytes.", read_check()),
        capability(
            LIST,
            Effect::Read,
            path_input(),
            Type::List(Box::new(entry_type())),
            "Lists a directory's entries, sorted by name.",
            read_check(),
        ),
        capability(
            STAT,
            Effect::Read,
            path_input(),
            Type::Option(Box::new(stat_type())),
            "Reports a path's kind and size, or none when nothing is there.",
            read_check(),
        ),
        capability(
            WRITE_TEXT,
            Effect::Write,
            record(None, vec![("path", STRING), ("text", STRING)]),
            UNIT,
            "Replaces a file's contents with UTF-8 text, atomically.",
            write_check(),
        ),
        capability(
            MAKE_DIRECTORY,
            Effect::Write,
            path_input(),
            UNIT,
            "Creates a directory and any missing parents, or succeeds when the directory exists.",
            destructive_check(check_entry),
        ),
        capability(
            MOVE,
            Effect::Write,
            record(None, vec![("from", STRING), ("to", STRING)]),
            UNIT,
            "Renames a file, symlink, or directory, failing with Conflict when the destination exists.",
            destructive_check(check_move),
        ),
        capability(
            DELETE,
            Effect::Write,
            path_input(),
            UNIT,
            "Deletes a file, a symlink, or an empty directory, never recursively.",
            destructive_check(check_entry),
        ),
    ];
    Pack {
        id: "fs".to_string(),
        types: vec![
            (KIND.to_string(), kind_type()),
            ("fs.Entry".to_string(), entry_type()),
            ("fs.Stat".to_string(), stat_type()),
        ],
        capabilities,
    }
}

pub fn performer(capability: &str) -> Option<Performer> {
    match capability {
        READ_TEXT => Some(read_text),
        READ_BYTES => Some(read_bytes),
        LIST => Some(list),
        STAT => Some(stat),
        WRITE_TEXT => Some(write_text),
        MAKE_DIRECTORY => Some(make_directory),
        MOVE => Some(move_entry),
        DELETE => Some(delete),
        _ => None,
    }
}

pub(crate) fn shown(path: &str) -> String {
    show_value(&Value::string(path))
}

pub(crate) fn strings(value: &Value) -> Option<Vec<&str>> {
    match value {
        Value::List(items) => items
            .iter()
            .map(|item| match item {
                Value::String(s) => Some(&**s),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

pub(crate) fn field<'v>(value: &'v Value, name: &str) -> Option<&'v Value> {
    match value {
        Value::Record(record) => record.get(name),
        _ => None,
    }
}

pub(crate) fn text_field<'v>(value: &'v Value, name: &str) -> Option<&'v str> {
    match field(value, name) {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

pub(crate) fn prepare(scope: &Value) -> Result<Value, String> {
    let roots = field(scope, "roots").and_then(strings).ok_or("a fs scope must be {:roots [PATH ...]}")?;
    if roots.is_empty() {
        return Err("a fs scope needs at least one root".to_string());
    }
    Ok(Value::Record(RecordValue::new(vec![(Arc::from("roots"), Value::list(canonical_roots(&roots)?))])))
}

pub(crate) fn canonical_roots(roots: &[&str]) -> Result<Vec<Value>, String> {
    let mut canonical = Vec::new();
    for root in roots {
        if !root.starts_with('/') {
            return Err(format!("root {} is not an absolute path", shown(root)));
        }
        let resolved = fs::canonicalize(root).map_err(|e| match e.kind() {
            ErrorKind::NotFound => format!("root {} does not exist", shown(root)),
            _ => format!("root {} cannot be resolved: {e}", shown(root)),
        })?;
        let metadata = fs::metadata(&resolved).map_err(|e| format!("root {} cannot be read: {e}", shown(root)))?;
        if !metadata.is_dir() {
            return Err(format!("root {} is not a directory", shown(root)));
        }
        let text = resolved
            .to_str()
            .ok_or_else(|| format!("root {} resolves to a path that is not valid UTF-8", shown(root)))?;
        canonical.push(Value::string(text));
    }
    Ok(canonical)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roots(Vec<PathBuf>);

impl Roots {
    pub fn of(scope: &Value) -> Result<Roots, String> {
        let roots = field(scope, "roots").and_then(strings).ok_or("the fs scope has no roots")?;
        Ok(Roots(roots.into_iter().map(PathBuf::from).collect()))
    }

    fn contain(&self, path: &Path) -> bool {
        self.0.iter().any(|root| path.starts_with(root))
    }
}

fn segments(path: &str) -> Result<Vec<&str>, String> {
    if path.contains('\0') {
        return Err(format!("{} contains a NUL character", shown(path)));
    }
    if !path.starts_with('/') {
        return Err(format!("{} is not an absolute path", shown(path)));
    }
    Ok(path.split('/').filter(|segment| !segment.is_empty()).collect())
}

fn joined(segments: &[&str]) -> PathBuf {
    let mut path = PathBuf::from("/");
    path.extend(segments);
    path
}

fn resolve(path: &str, segments: &[&str]) -> Result<PathBuf, String> {
    let mut existing = segments.len();
    let base = loop {
        let Some(prefix) = segments.get(..existing) else {
            return Err(format!("{} cannot be resolved", shown(path)));
        };
        let candidate = joined(prefix);
        match fs::canonicalize(&candidate) {
            Ok(canonical) => break canonical,
            Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) && existing > 0 => {
                if error.kind() == ErrorKind::NotFound && fs::symlink_metadata(&candidate).is_ok() {
                    return Err(format!(
                        "{} goes through {}, a symlink whose target does not exist",
                        shown(path),
                        candidate.display()
                    ));
                }
                existing -= 1;
            }
            Err(error) => return Err(format!("{} cannot be resolved: {error}", shown(path))),
        }
    };
    let remainder = segments.get(existing..).unwrap_or_default();
    if remainder.iter().any(|segment| *segment == "." || *segment == "..") {
        return Err(format!("{} has . or .. in a part that does not exist", shown(path)));
    }
    let mut resolved = base;
    resolved.extend(remainder);
    Ok(resolved)
}

fn outside(path: &str, resolved: &Path) -> String {
    if Path::new(path) == resolved {
        format!("{} is outside every granted root", shown(path))
    } else {
        format!("{} resolves to {}, outside every granted root", shown(path), resolved.display())
    }
}

pub fn resolve_read(roots: &Roots, path: &str) -> Result<PathBuf, String> {
    let resolved = resolve(path, &segments(path)?)?;
    if !roots.contain(&resolved) {
        return Err(outside(path, &resolved));
    }
    Ok(resolved)
}

fn locate_entry(roots: &Roots, path: &str) -> Result<PathBuf, String> {
    let segments = segments(path)?;
    let Some((name, parent)) = segments.split_last() else {
        return Err(format!("{} names no file to write", shown(path)));
    };
    if *name == "." || *name == ".." {
        return Err(format!("{} ends in . or .., which names no file to write", shown(path)));
    }
    let directory = resolve(path, parent)?;
    if !roots.contain(&directory) {
        return Err(format!(
            "{} is in {}, outside every granted root",
            shown(path),
            directory.display()
        ));
    }
    Ok(directory.join(name))
}

fn refuse_symlink(path: &str, target: PathBuf, refusal: &str) -> Result<PathBuf, String> {
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!("{} is a symlink, and {refusal}", shown(path))),
        Ok(_) => Ok(target),
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => Ok(target),
        Err(error) => Err(format!("{} cannot be resolved: {error}", shown(path))),
    }
}

pub fn resolve_write(roots: &Roots, path: &str) -> Result<PathBuf, String> {
    refuse_symlink(path, locate_entry(roots, path)?, "fs.write-text never writes through one")
}

pub fn resolve_entry(roots: &Roots, path: &str) -> Result<PathBuf, String> {
    locate_entry(roots, path)
}

pub fn resolve_destination(roots: &Roots, path: &str) -> Result<PathBuf, String> {
    refuse_symlink(path, locate_entry(roots, path)?, "fs.move never replaces one")
}

fn verdict(scope: &Value, input: &Value, resolve: fn(&Roots, &str) -> Result<PathBuf, String>) -> Verdict {
    let checked = Roots::of(scope).and_then(|roots| {
        let path = text_field(input, "path").ok_or("the input has no path")?;
        resolve(&roots, path)
    });
    match checked {
        Ok(_) => Verdict::Allowed,
        Err(reason) => Verdict::Denied(reason),
    }
}

fn check_read(scope: &Value, input: &Value) -> Verdict {
    verdict(scope, input, resolve_read)
}

fn check_write(scope: &Value, input: &Value) -> Verdict {
    verdict(scope, input, resolve_write)
}

fn check_entry(scope: &Value, input: &Value) -> Verdict {
    verdict(scope, input, resolve_entry)
}

fn check_move(scope: &Value, input: &Value) -> Verdict {
    let checked = Roots::of(scope).and_then(|roots| {
        let from = text_field(input, "from").ok_or("the input has no from")?;
        let to = text_field(input, "to").ok_or("the input has no to")?;
        resolve_entry(&roots, from)?;
        resolve_destination(&roots, to)
    });
    match checked {
        Ok(_) => Verdict::Allowed,
        Err(reason) => Verdict::Denied(reason),
    }
}

pub fn error_kind(error: &io::Error) -> CallErrorKind {
    match error.kind() {
        ErrorKind::AlreadyExists => CallErrorKind::Conflict,
        ErrorKind::NotFound => CallErrorKind::NotFound,
        ErrorKind::PermissionDenied => CallErrorKind::PermissionDenied,
        ErrorKind::IsADirectory
        | ErrorKind::NotADirectory
        | ErrorKind::DirectoryNotEmpty
        | ErrorKind::InvalidInput
        | ErrorKind::InvalidFilename => CallErrorKind::Invalid,
        _ if error.raw_os_error() == Some(libc::ELOOP) => CallErrorKind::Invalid,
        _ => CallErrorKind::Failed,
    }
}

fn os_failure(path: &str, error: &io::Error) -> Failure {
    Failure::new(error_kind(error), format!("{}: {error}", shown(path)))
}

fn malformed(capability: &str) -> Failure {
    Failure::new(CallErrorKind::Failed, format!("{capability} received an input that does not match its type"))
}

fn located(
    capability: &str,
    scope: &Value,
    input: &Value,
    resolve: fn(&Roots, &str) -> Result<PathBuf, String>,
) -> Result<(String, PathBuf), Failure> {
    let path = text_field(input, "path").ok_or_else(|| malformed(capability))?;
    let roots = Roots::of(scope).map_err(|reason| Failure::new(CallErrorKind::PermissionDenied, reason))?;
    let resolved = resolve(&roots, path).map_err(|reason| Failure::new(CallErrorKind::PermissionDenied, reason))?;
    Ok((path.to_string(), resolved))
}

fn read_file(path: &str, resolved: &Path) -> Result<Vec<u8>, Failure> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(resolved)
        .map_err(|e| os_failure(path, &e))?;
    let metadata = file.metadata().map_err(|e| os_failure(path, &e))?;
    if metadata.is_dir() {
        return Err(Failure::new(CallErrorKind::Invalid, format!("{} is a directory", shown(path))));
    }
    if !metadata.is_file() {
        return Err(Failure::new(CallErrorKind::Invalid, format!("{} is not a regular file", shown(path))));
    }
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).map_err(|e| os_failure(path, &e))?;
    Ok(contents)
}

fn read_text(scope: &Value, input: &Value) -> Result<Value, Failure> {
    let (path, resolved) = located(READ_TEXT, scope, input, resolve_read)?;
    let contents = read_file(&path, &resolved)?;
    let text = String::from_utf8(contents)
        .map_err(|e| Failure::new(CallErrorKind::Invalid, format!("{} is not valid UTF-8: {e}", shown(&path))))?;
    Ok(Value::string(&text))
}

fn read_bytes(scope: &Value, input: &Value) -> Result<Value, Failure> {
    let (path, resolved) = located(READ_BYTES, scope, input, resolve_read)?;
    Ok(Value::Bytes(read_file(&path, &resolved)?.into()))
}

fn list(scope: &Value, input: &Value) -> Result<Value, Failure> {
    let (path, resolved) = located(LIST, scope, input, resolve_read)?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&resolved).map_err(|e| os_failure(&path, &e))? {
        let entry = entry.map_err(|e| os_failure(&path, &e))?;
        let name = entry.file_name().into_string().map_err(|raw| {
            Failure::new(
                CallErrorKind::Invalid,
                format!("{} holds an entry whose name is not valid UTF-8: {}", shown(&path), raw.display()),
            )
        })?;
        let kind = Kind::of(entry.file_type().map_err(|e| os_failure(&path, &e))?);
        entries.push((name, kind));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Value::list(
        entries
            .into_iter()
            .map(|(name, kind)| {
                Value::Record(RecordValue::new(vec![
                    (Arc::from("name"), Value::string(&name)),
                    (Arc::from("kind"), kind.value()),
                ]))
            })
            .collect(),
    ))
}

fn stat(scope: &Value, input: &Value) -> Result<Value, Failure> {
    let (path, resolved) = located(STAT, scope, input, resolve_read)?;
    let metadata = match fs::symlink_metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => return Ok(Value::None),
        Err(error) => return Err(os_failure(&path, &error)),
    };
    let size = i64::try_from(metadata.len())
        .map_err(|_| Failure::new(CallErrorKind::Failed, format!("{} is larger than an Int can hold", shown(&path))))?;
    Ok(Value::some(Value::Record(RecordValue::new(vec![
        (Arc::from("kind"), Kind::of(metadata.file_type()).value()),
        (Arc::from("size"), Value::Int(size)),
    ]))))
}

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

fn temporary(directory: &Path, path: &str) -> Result<(PathBuf, File), Failure> {
    loop {
        let serial = TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let candidate = directory.join(format!(".sx-write-{}-{serial}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .mode(0o644)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(os_failure(path, &error)),
        }
    }
}

fn replace(target: &Path, text: &str, path: &str) -> Result<(), Failure> {
    let directory = target.parent().ok_or_else(|| Failure::new(CallErrorKind::Invalid, format!("{} names no file", shown(path))))?;
    let existing = match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.is_dir() => {
            return Err(Failure::new(CallErrorKind::Invalid, format!("{} is a directory", shown(path))));
        }
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Failure::new(
                CallErrorKind::PermissionDenied,
                format!("{} is a symlink, and fs.write-text never writes through one", shown(path)),
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(Failure::new(CallErrorKind::Invalid, format!("{} is not a regular file", shown(path))));
        }
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(os_failure(path, &error)),
    };
    let (temporary_path, mut file) = temporary(directory, path)?;
    let written = existing
        .map_or(Ok(()), |permissions| file.set_permissions(fs::Permissions::from_mode(permissions.mode())))
        .and_then(|()| file.write_all(text.as_bytes()))
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temporary_path, target));
    drop(file);
    if let Err(error) = written {
        let mut failure = os_failure(path, &error);
        if let Err(cleanup) = fs::remove_file(&temporary_path) {
            failure.message.push_str(&format!("; the temporary file {} remains: {cleanup}", temporary_path.display()));
        }
        return Err(failure);
    }
    File::open(directory).and_then(|handle| handle.sync_all()).map_err(|error| {
        Failure::new(
            CallErrorKind::Failed,
            format!("{} was replaced, but its directory could not be synced: {error}", shown(path)),
        )
    })
}

fn write_text(scope: &Value, input: &Value) -> Result<Value, Failure> {
    let (path, target) = located(WRITE_TEXT, scope, input, resolve_write)?;
    let text = text_field(input, "text").ok_or_else(|| malformed(WRITE_TEXT))?;
    replace(&target, text, &path)?;
    Ok(Value::Unit)
}

fn make_directory(scope: &Value, input: &Value) -> Result<Value, Failure> {
    let (path, target) = located(MAKE_DIRECTORY, scope, input, resolve_entry)?;
    let mut missing = Vec::new();
    let mut ancestor = target.as_path();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing.push(ancestor);
                ancestor = ancestor.parent().ok_or_else(|| Failure::new(CallErrorKind::Invalid, format!("{} names no directory", shown(&path))))?;
            }
            Err(error) => return Err(os_failure(&path, &error)),
        }
    }
    if missing.is_empty() {
        return real_directory(&path, &target).map(|()| Value::Unit);
    }
    for directory in missing.into_iter().rev() {
        match fs::create_dir(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => real_directory(&path, directory)?,
            Err(error) => return Err(os_failure(&path, &error)),
        }
    }
    Ok(Value::Unit)
}

fn real_directory(path: &str, directory: &Path) -> Result<(), Failure> {
    let metadata = fs::symlink_metadata(directory).map_err(|e| os_failure(path, &e))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(Failure::new(
            CallErrorKind::Invalid,
            format!("{} exists and is not a directory", shown(&directory.display().to_string())),
        ))
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn rename_without_replacing(from: &std::ffi::CStr, to: &std::ffi::CStr) -> io::Result<()> {
    // SAFETY: both arguments are NUL-terminated strings that outlive the call, and renamex_np
    // reads them without keeping them.
    let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if status == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn rename_without_replacing(from: &std::ffi::CStr, to: &std::ffi::CStr) -> io::Result<()> {
    // SAFETY: both arguments are NUL-terminated strings that outlive the call, and renameat2
    // reads them without keeping them. AT_FDCWD makes the paths absolute-or-cwd relative.
    let status = unsafe {
        libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, from.as_ptr(), libc::AT_FDCWD, to.as_ptr(), libc::RENAME_NOREPLACE)
    };
    if status == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

fn c_path(path: &Path, shown_path: &str) -> Result<std::ffi::CString, Failure> {
    std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| Failure::new(CallErrorKind::Invalid, format!("{} contains a NUL character", shown(shown_path))))
}

fn move_entry(scope: &Value, input: &Value) -> Result<Value, Failure> {
    let roots = Roots::of(scope).map_err(|reason| Failure::new(CallErrorKind::PermissionDenied, reason))?;
    let from = text_field(input, "from").ok_or_else(|| malformed(MOVE))?;
    let to = text_field(input, "to").ok_or_else(|| malformed(MOVE))?;
    let denied = |reason: String| Failure::new(CallErrorKind::PermissionDenied, reason);
    let source = resolve_entry(&roots, from).map_err(denied)?;
    let destination = resolve_destination(&roots, to).map_err(denied)?;
    if source == destination {
        return Err(match fs::symlink_metadata(&source) {
            Ok(_) => Failure::new(CallErrorKind::Conflict, format!("{} to {}: the destination exists", shown(from), shown(to))),
            Err(error) => os_failure(from, &error),
        });
    }
    rename_without_replacing(&c_path(&source, from)?, &c_path(&destination, to)?)
        .map_err(|error| Failure::new(error_kind(&error), format!("{} to {}: {error}", shown(from), shown(to))))?;
    Ok(Value::Unit)
}

fn delete(scope: &Value, input: &Value) -> Result<Value, Failure> {
    let (path, target) = located(DELETE, scope, input, resolve_entry)?;
    let metadata = fs::symlink_metadata(&target).map_err(|e| os_failure(&path, &e))?;
    let removed = if metadata.is_dir() {
        fs::remove_dir(&target)
    } else if metadata.is_file() || metadata.file_type().is_symlink() {
        fs::remove_file(&target)
    } else {
        return Err(Failure::new(CallErrorKind::Invalid, format!("{} is not a file, symlink, or directory", shown(&path))));
    };
    removed.map_err(|e| os_failure(&path, &e))?;
    Ok(Value::Unit)
}
