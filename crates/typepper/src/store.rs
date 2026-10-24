//! Small files that survive restarts: under the data dir on desktop
//! (`$XDG_DATA_HOME/typepper/<name>`), in localStorage on the web.

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::path::PathBuf;

    fn path(name: &str) -> Option<PathBuf> {
        let var = |k| std::env::var_os(k).map(PathBuf::from);
        let dir = var("XDG_DATA_HOME")
            .or_else(|| var("HOME").map(|h| h.join(".local/share")))
            .or_else(|| var("APPDATA"))?;
        Some(dir.join("typepper").join(name))
    }

    pub fn read(name: &str) -> Option<String> {
        std::fs::read_to_string(path(name)?).ok()
    }

    pub fn write(name: &str, text: &str) -> std::io::Result<()> {
        let p = path(name).ok_or(std::io::ErrorKind::NotFound)?;
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(p, text)
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    fn local() -> Option<ratzilla::web_sys::Storage> {
        ratzilla::web_sys::window()?.local_storage().ok()?
    }

    pub fn read(name: &str) -> Option<String> {
        local()?.get_item(&format!("typepper.{name}")).ok()?
    }

    pub fn write(name: &str, text: &str) -> Result<(), ()> {
        local()
            .ok_or(())?
            .set_item(&format!("typepper.{name}"), text)
            .map_err(|_| ())
    }
}

pub use imp::{read, write};

/// Bytes, kept as hex (localStorage only holds text).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))] // the web reads them once it uploads
pub fn read_bytes(name: &str) -> Option<Vec<u8>> {
    unhex(read(name)?.trim())
}

pub fn write_bytes(name: &str, bytes: &[u8]) {
    // ponytail: a failed save only loses this copy; no error UI for it
    let _ = write(name, &hex(bytes));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_survive_as_hex() {
        let bytes = [0, 1, 0x7f, 0x80, 0xff];
        assert_eq!(hex(&bytes), "00017f80ff");
        assert_eq!(unhex(&hex(&bytes)), Some(bytes.to_vec()));
        assert_eq!(unhex(""), Some(vec![]));
        assert_eq!(unhex("abc"), None, "odd length");
        assert_eq!(unhex("zz"), None);
    }
}
