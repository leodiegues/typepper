//! The protocol doc names every error the server can answer.

use std::{fs, path::Path};

#[test]
fn protocol_lists_every_error_code() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc = fs::read_to_string(crate_dir.join("../../docs/reference/protocol.md")).unwrap();
    let mut missing = vec![];
    for file in fs::read_dir(crate_dir.join("src")).unwrap() {
        let src = fs::read_to_string(file.unwrap().path()).unwrap();
        // `"err <code> <why>"`, as a literal or a format!
        for code in src
            .split("\"err ")
            .skip(1)
            .filter_map(|s| s.split(' ').next())
        {
            if !doc.contains(&format!("`{code}`")) && !doc.contains(&format!("err {code}")) {
                missing.push(code.to_string());
            }
        }
    }
    assert!(
        missing.is_empty(),
        "docs/reference/protocol.md doesn't mention: {missing:?}"
    );
}
