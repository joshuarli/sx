use std::cell::Cell;
use std::io;

pub const REQUIRED_STACK_BYTES: usize = 256 * 1024 * 1024;
const RESERVED_BYTES: usize = 32 * 1024 * 1024;

thread_local! {
    static FLOOR: Cell<Option<usize>> = const { Cell::new(None) };
}

fn here() -> usize {
    let marker = 0u8;
    std::ptr::from_ref(&marker) as usize
}

pub fn is_sized() -> bool {
    FLOOR.with(|floor| floor.get().is_some())
}

pub fn exhausted() -> bool {
    FLOOR.with(|floor| floor.get().is_some_and(|lowest| here() < lowest))
}

pub fn on_stack_of<R: Send>(stack_bytes: usize, work: impl FnOnce() -> R + Send) -> io::Result<R> {
    std::thread::scope(|threads| {
        let handle = std::thread::Builder::new().stack_size(stack_bytes).spawn_scoped(threads, || {
            let usable = stack_bytes.saturating_sub(RESERVED_BYTES.min(stack_bytes / 2));
            FLOOR.with(|floor| floor.set(Some(here().saturating_sub(usable))));
            work()
        })?;
        match handle.join() {
            Ok(result) => Ok(result),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    })
}

pub fn on_sized_stack<R: Send>(work: impl FnOnce() -> R + Send) -> io::Result<R> {
    if is_sized() {
        return Ok(work());
    }
    on_stack_of(REQUIRED_STACK_BYTES, work)
}
