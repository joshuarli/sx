use std::fs;
use std::io;
use std::path::Path;

use sx_kernel::json::repr_str;

pub fn path_text(path: &str) -> String {
    let rooted = path.starts_with('/');
    let doubled = path.starts_with("//") && !path.starts_with("///");
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty() && *part != ".").collect();
    let joined = parts.join("/");
    match (rooted, doubled) {
        (true, true) => format!("//{joined}"),
        (true, false) => format!("/{joined}"),
        (false, _) if joined.is_empty() => ".".to_string(),
        (false, _) => joined,
    }
}

fn os_error_text(error: &io::Error, path: &str) -> String {
    match error.raw_os_error() {
        Some(code) => {
            let shown = error.to_string();
            let strerror = shown.strip_suffix(&format!(" (os error {code})")).unwrap_or(&shown);
            format!("[Errno {code}] {strerror}: {}", repr_str(path))
        }
        None => format!("{error}: {}", repr_str(path)),
    }
}

fn decode_error_text(error: &std::str::Utf8Error, bytes: &[u8]) -> String {
    let at = error.valid_up_to();
    let byte = bytes.get(at).copied().unwrap_or(0);
    let reason = match error.error_len() {
        None => "unexpected end of data",
        Some(_) if matches!(byte, 0x80..=0xbf | 0xc0 | 0xc1 | 0xf5..=0xff) => "invalid start byte",
        Some(_) => "invalid continuation byte",
    };
    format!("'utf-8' codec can't decode byte 0x{byte:02x} in position {at}: {reason}")
}

pub fn read_text(path: &Path) -> Result<String, String> {
    let shown = path_text(&path.to_string_lossy());
    let bytes = fs::read(path).map_err(|error| os_error_text(&error, &shown))?;
    let text = std::str::from_utf8(&bytes).map_err(|error| decode_error_text(&error, &bytes))?;
    Ok(if text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n") } else { text.to_string() })
}
