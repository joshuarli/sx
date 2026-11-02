use std::fs;

use sx_kernel::registry::Registry;

fn main() {
    let dir = std::env::args().nth(1).expect("directory of .ts files");
    let mut paths: Vec<_> = fs::read_dir(dir).expect("dir").map(|e| e.expect("entry").path()).collect();
    paths.sort();
    let registry = Registry::new();
    for path in paths.iter().filter(|p| p.extension().is_some_and(|e| e == "ts")) {
        let source = fs::read_to_string(path).expect("read");
        let stem = path.file_stem().expect("stem").to_string_lossy().to_string();
        let result = std::panic::catch_unwind(|| sx_ts::lower_source(&source, &Registry::new()));
        match result {
            Err(_) => println!("{stem} PANIC"),
            Ok(Ok(_)) => println!("{stem} ok"),
            Ok(Err(diagnostics)) => {
                let mut codes: Vec<&str> = diagnostics.iter().map(|d| d.code).filter(|c| *c != "unknown-tool").collect();
                codes.sort();
                codes.dedup();
                println!("{stem} {}", if codes.is_empty() { "ok".to_string() } else { codes.join(",") });
            }
        }
    }
    let _ = registry;
}
