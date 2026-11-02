use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use sx_kernel::authority::{Grant, Grants, Screen, Verdict};
use sx_kernel::json::Json;
use sx_kernel::registry::Registry;
use sx_kernel::types::CallErrorKind;
use sx_kernel::values::{RecordValue, Value};
use sx_packs::fs::{LIST, READ_BYTES, READ_TEXT, STAT, WRITE_TEXT};
use sx_packs::{Failure, pack, performer};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

fn sandbox(label: &str) -> PathBuf {
    let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fs-scope-{}-{label}-{serial}", std::process::id()));
    if directory.exists() {
        fs::remove_dir_all(&directory).expect("old sandbox removed");
    }
    fs::create_dir_all(&directory).expect("sandbox");
    directory
}

fn text(path: &Path) -> &str {
    path.to_str().expect("test paths are UTF-8")
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.add_pack(pack("fs").expect("the fs pack")).expect("the fs pack registers");
    registry
}

fn scope_json(roots: &[&Path]) -> Json {
    Json::object([("roots", Json::Array(roots.iter().map(|root| Json::string(text(root))).collect()))])
}

fn granted(roots: &[&Path]) -> Grants {
    let registry = registry();
    let scope = scope_json(roots);
    let all = Json::Object(
        [READ_TEXT, READ_BYTES, LIST, STAT, WRITE_TEXT].iter().map(|id| ((*id).to_string(), scope.clone())).collect(),
    );
    Grants::load(&registry, &all).expect("the grants load")
}

fn record(fields: &[(&str, &str)]) -> Value {
    Value::Record(RecordValue::new(fields.iter().map(|(n, v)| (Arc::from(*n), Value::string(v))).collect()))
}

fn path_input(path: &str) -> Value {
    record(&[("path", path)])
}

fn screen(grants: &Grants, capability: &str, input: &Value) -> Verdict {
    match grants.screen(capability, input) {
        Screen::Verdict(verdict) => verdict,
        other => panic!("{capability} has no scope check: {other:?}"),
    }
}

fn read_verdict(grants: &Grants, path: &str) -> Verdict {
    screen(grants, READ_TEXT, &path_input(path))
}

fn write_verdict(grants: &Grants, path: &str) -> Verdict {
    screen(grants, WRITE_TEXT, &record(&[("path", path), ("text", "x")]))
}

fn denied(verdict: Verdict) -> String {
    match verdict {
        Verdict::Denied(reason) => reason,
        Verdict::Allowed => panic!("the request was allowed"),
    }
}

fn perform(grants: &Grants, capability: &str, input: &Value) -> Result<Value, Failure> {
    let scope = match grants.get(capability) {
        Some(Grant::Scoped { scope, .. }) => scope.clone(),
        other => panic!("{capability} is not granted with a scope: {other:?}"),
    };
    performer(capability).expect("the fs pack performs it")(&scope, input)
}

struct Tree {
    base: PathBuf,
    root: PathBuf,
    outside: PathBuf,
}

fn tree(label: &str) -> Tree {
    let base = fs::canonicalize(sandbox(label)).expect("canonical sandbox");
    let root = base.join("root");
    let outside = base.join("outside");
    fs::create_dir_all(root.join("sub")).expect("root");
    fs::create_dir_all(&outside).expect("outside");
    fs::write(root.join("note.txt"), "inside").expect("note");
    fs::write(root.join("sub/deep.txt"), "deep").expect("deep");
    fs::write(outside.join("secret.txt"), "secret").expect("secret");
    Tree { base, root, outside }
}

#[test]
fn a_path_inside_a_root_is_allowed_and_read() {
    let t = tree("inside");
    let grants = granted(&[&t.root]);
    let note = t.root.join("note.txt");
    assert_eq!(read_verdict(&grants, text(&note)), Verdict::Allowed);
    assert_eq!(read_verdict(&grants, text(&t.root)), Verdict::Allowed);
    assert_eq!(read_verdict(&grants, text(&t.root.join("not-yet/there.txt"))), Verdict::Allowed);
    assert_eq!(perform(&grants, READ_TEXT, &path_input(text(&note))), Ok(Value::string("inside")));
}

#[test]
fn dot_dot_and_dot_are_resolved_where_the_path_exists_and_refused_where_it_does_not() {
    let t = tree("dots");
    let grants = granted(&[&t.root]);
    let escape = format!("{}/../outside/secret.txt", text(&t.root));
    assert!(denied(read_verdict(&grants, &escape)).contains("outside every granted root"));
    let within = format!("{}/sub/../note.txt", text(&t.root));
    assert_eq!(read_verdict(&grants, &within), Verdict::Allowed);
    assert_eq!(perform(&grants, READ_TEXT, &path_input(&within)), Ok(Value::string("inside")));
    let here = format!("{}/./note.txt", text(&t.root));
    assert_eq!(read_verdict(&grants, &here), Verdict::Allowed);
    let missing_up = format!("{}/missing/../../outside/secret.txt", text(&t.root));
    assert!(denied(read_verdict(&grants, &missing_up)).contains(". or .."));
    let missing_here = format!("{}/missing/./x", text(&t.root));
    assert!(denied(read_verdict(&grants, &missing_here)).contains(". or .."));
}

#[test]
fn symlinks_that_leave_every_root_are_denied() {
    let t = tree("symlinks");
    symlink(t.outside.join("secret.txt"), t.root.join("file-link")).expect("file link");
    symlink(&t.outside, t.root.join("dir-link")).expect("dir link");
    symlink(t.root.join("hop-2"), t.root.join("hop-1")).expect("chain start");
    symlink(&t.outside, t.root.join("hop-2")).expect("chain end");
    symlink(t.root.join("note.txt"), t.root.join("inner-link")).expect("inner link");
    symlink(t.base.join("nowhere"), t.root.join("dangling")).expect("dangling");
    symlink(t.root.join("loop-b"), t.root.join("loop-a")).expect("loop a");
    symlink(t.root.join("loop-a"), t.root.join("loop-b")).expect("loop b");
    let grants = granted(&[&t.root]);
    for escape in ["file-link", "dir-link/secret.txt", "dir-link/new.txt", "hop-1/secret.txt"] {
        let path = t.root.join(escape);
        let reason = denied(read_verdict(&grants, text(&path)));
        assert!(reason.contains("resolves to") && reason.contains("outside every granted root"), "{escape}: {reason}");
        denied(write_verdict(&grants, text(&path)));
    }
    assert_eq!(read_verdict(&grants, text(&t.root.join("inner-link"))), Verdict::Allowed);
    assert_eq!(perform(&grants, READ_TEXT, &path_input(text(&t.root.join("inner-link")))), Ok(Value::string("inside")));
    assert!(denied(read_verdict(&grants, text(&t.root.join("dangling/x")))).contains("symlink whose target does not exist"));
    assert!(denied(read_verdict(&grants, text(&t.root.join("dangling")))).contains("symlink whose target does not exist"));
    assert!(denied(read_verdict(&grants, text(&t.root.join("loop-a/x")))).contains("cannot be resolved"));
}

#[test]
fn a_symlinked_root_is_granted_by_its_target() {
    let t = tree("symlinked-root");
    let link = t.base.join("root-link");
    symlink(&t.root, &link).expect("root link");
    let grants = granted(&[&link]);
    let Some(Grant::Scoped { scope, .. }) = grants.get(READ_TEXT) else { panic!("scoped grant") };
    assert_eq!(scope, &Value::Record(RecordValue::new(vec![(Arc::from("roots"), Value::list(vec![Value::string(text(&t.root))]))])));
    assert_eq!(read_verdict(&grants, text(&link.join("note.txt"))), Verdict::Allowed);
    assert_eq!(read_verdict(&grants, text(&t.root.join("note.txt"))), Verdict::Allowed);
    assert!(denied(read_verdict(&grants, text(&t.base.join("outside/secret.txt")))).contains("outside every granted root"));
}

#[test]
fn absolute_path_tricks_are_denied() {
    let t = tree("absolute");
    fs::create_dir_all(t.base.join("root-evil")).expect("sibling");
    fs::write(t.base.join("root-evil/x.txt"), "evil").expect("sibling file");
    let grants = granted(&[&t.root]);
    assert!(denied(read_verdict(&grants, "note.txt")).contains("not an absolute path"));
    assert!(denied(read_verdict(&grants, "")).contains("not an absolute path"));
    assert!(denied(read_verdict(&grants, "~/note.txt")).contains("not an absolute path"));
    assert!(denied(read_verdict(&grants, &format!("{}\0/x", text(&t.root)))).contains("NUL"));
    let sibling = t.base.join("root-evil/x.txt");
    assert!(denied(read_verdict(&grants, text(&sibling))).contains("outside every granted root"));
    let doubled = format!("/{}//note.txt", text(&t.root));
    assert_eq!(read_verdict(&grants, &doubled), Verdict::Allowed);
    assert!(denied(read_verdict(&grants, "/")).contains("outside every granted root"));
    assert!(denied(write_verdict(&grants, text(&t.root))).contains("outside every granted root"));
    assert!(denied(write_verdict(&grants, "/")).contains("names no file"));
    assert!(denied(write_verdict(&grants, &format!("{}/sub/..", text(&t.root)))).contains(". or .."));
}

#[test]
fn write_text_refuses_a_final_symlink_even_one_that_stays_inside() {
    let t = tree("write-symlink");
    symlink(t.root.join("note.txt"), t.root.join("inner-link")).expect("inner link");
    let grants = granted(&[&t.root]);
    let reason = denied(write_verdict(&grants, text(&t.root.join("inner-link"))));
    assert!(reason.contains("is a symlink"), "{reason}");
    assert_eq!(fs::read_to_string(t.root.join("note.txt")).expect("note"), "inside");
}

fn case_insensitive(directory: &Path) -> bool {
    fs::write(directory.join("CaseProbe"), "").expect("probe");
    directory.join("caseprobe").exists()
}

#[test]
fn case_on_a_case_insensitive_file_system_resolves_to_the_stored_name() {
    let t = tree("case");
    let grants = granted(&[&t.root]);
    let shouted = t.base.join("ROOT/note.txt");
    if case_insensitive(&t.base) {
        assert_eq!(read_verdict(&grants, text(&shouted)), Verdict::Allowed);
        assert_eq!(perform(&grants, READ_TEXT, &path_input(text(&shouted))), Ok(Value::string("inside")));
        let escape = t.base.join("ROOT/../OUTSIDE/secret.txt");
        assert!(denied(read_verdict(&grants, text(&escape))).contains("outside every granted root"));
    } else {
        assert!(denied(read_verdict(&grants, text(&shouted))).contains("outside every granted root"));
    }
}

#[test]
fn a_name_that_is_not_utf8_cannot_be_listed_as_a_string() {
    let t = tree("non-utf8");
    let grants = granted(&[&t.root]);
    let name = OsStr::from_bytes(b"bad-\xff-name");
    match fs::write(t.root.join(name), "x") {
        Ok(()) => {
            let failure = perform(&grants, LIST, &path_input(text(&t.root))).expect_err("the listing fails");
            assert_eq!(failure.kind, CallErrorKind::Invalid);
            assert!(failure.message.contains("not valid UTF-8"), "{}", failure.message);
        }
        Err(error) => {
            assert_eq!(std::env::consts::OS, "macos", "only APFS refuses the name: {error}");
            assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
        }
    }
}

fn mkfifo(path: &Path) {
    let status = std::process::Command::new("mkfifo").arg(path).status().expect("mkfifo runs");
    assert!(status.success(), "mkfifo failed");
}

#[test]
fn a_denied_request_never_opens_reads_or_writes_anything() {
    let t = tree("untouched");
    let fifo = t.outside.join("pipe");
    mkfifo(&fifo);
    let grants = granted(&[&t.root]);
    let (sender, receiver) = mpsc::channel();
    let reader = grants.clone();
    let target = text(&fifo).to_string();
    thread::spawn(move || {
        let _ = sender.send(perform(&reader, READ_TEXT, &path_input(&target)));
    });
    let outcome = receiver.recv_timeout(Duration::from_secs(5)).expect("a denied read returns without opening the FIFO");
    let failure = outcome.expect_err("the read is denied");
    assert_eq!(failure.kind, CallErrorKind::PermissionDenied);
    assert!(failure.message.contains("outside every granted root"), "{}", failure.message);
    let written = t.outside.join("created.txt");
    let failure = perform(&grants, WRITE_TEXT, &record(&[("path", text(&written)), ("text", "x")])).expect_err("denied");
    assert_eq!(failure.kind, CallErrorKind::PermissionDenied);
    let left: Vec<_> = fs::read_dir(&t.outside).expect("outside").map(|e| e.expect("entry").file_name()).collect();
    assert_eq!(left.len(), 2, "the denied write created nothing: {left:?}");
    let locked = t.outside.join("locked");
    fs::create_dir(&locked).expect("locked");
    fs::write(locked.join("inner.txt"), "x").expect("inner");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("lock");
    let verdict = read_verdict(&grants, text(&locked.join("inner.txt")));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("unlock");
    denied(verdict);
}

#[test]
fn a_root_must_be_an_existing_absolute_directory() {
    let t = tree("roots");
    let registry = registry();
    let grant = |roots: Json| Grants::load(&registry, &Json::object([(READ_TEXT, Json::object([("roots", roots)]))]));
    let message = |roots: Json| grant(roots).expect_err("the grant is refused").to_string();
    assert!(message(Json::Array(vec![Json::string("relative")])).contains("not an absolute path"));
    assert!(message(Json::Array(vec![Json::string(text(&t.base.join("missing")))])).contains("does not exist"));
    assert!(message(Json::Array(vec![Json::string(text(&t.root.join("note.txt")))])).contains("not a directory"));
    assert!(message(Json::Array(Vec::new())).contains("at least one root"));
    assert!(message(Json::string("/")).contains("expected List<String>"));
    let unscoped = Grants::load(&registry, &Json::object([(READ_TEXT, Json::Null)])).expect_err("refused");
    assert!(unscoped.to_string().contains("needs a scope value"), "{unscoped}");
}

#[test]
fn os_errors_map_to_call_error_kinds_by_a_closed_table() {
    use sx_packs::fs::error_kind;
    let kind = |code: i32| error_kind(&std::io::Error::from_raw_os_error(code));
    assert_eq!(kind(libc::ENOENT), CallErrorKind::NotFound);
    assert_eq!(kind(libc::EACCES), CallErrorKind::PermissionDenied);
    assert_eq!(kind(libc::EPERM), CallErrorKind::PermissionDenied);
    assert_eq!(kind(libc::EEXIST), CallErrorKind::Conflict);
    for code in [libc::EISDIR, libc::ENOTDIR, libc::ENOTEMPTY, libc::EINVAL, libc::ENAMETOOLONG, libc::ELOOP] {
        assert_eq!(kind(code), CallErrorKind::Invalid, "errno {code}");
    }
    for code in [libc::EIO, libc::ENOSPC, libc::EROFS, libc::EBUSY, libc::EXDEV, libc::EMFILE] {
        assert_eq!(kind(code), CallErrorKind::Failed, "errno {code}");
    }
}
