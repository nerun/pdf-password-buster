// PDF Password Buster v1.0 - PDF decryption engine.
// Copyright 2026 Daniel Dias Rodrigues
//
// Licensed under the MIT License. You may use, copy, modify, merge, publish,
// distribute, sublicense and/or sell copies of this software under the terms
// of that license; see the LICENSE file distributed with this program.
// SPDX-License-Identifier: MIT

//! PDF decryption engine.
//!
//! Opens a PDF protected by the Standard security handler (RC4 40/128-bit,
//! AES-128, AES-256), decrypts every string and stream, and writes a copy
//! without the /Encrypt dictionary, so no password or permission flags remain.

use lopdf::{Document, LoadOptions, Object};
use std::path::Path;

/// Result of opening a file with a given set of passwords.
pub enum Open {
    /// The file has no /Encrypt entry.
    NotEncrypted,
    /// The file is encrypted and none of the passwords opened it.
    NeedPassword,
    /// The file could not be read or parsed as a PDF.
    #[allow(dead_code)]
    Unreadable(String),
    /// The file was encrypted and is now decrypted in memory.
    Decrypted(Box<Document>),
}

/// Whether a loaded document was encrypted and has been decrypted.
fn classify(doc: Document) -> Open {
    if doc.encryption_state.is_some() {
        Open::Decrypted(Box::new(doc))
    } else if doc.trailer.has(b"Encrypt") {
        Open::NeedPassword
    } else {
        Open::NotEncrypted
    }
}

/// Opens `path`, trying the empty password first and then each password given
/// (owner or user, either works).
pub fn open(path: &Path, passwords: &[&str]) -> Open {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Open::Unreadable(e.to_string()),
    };
    open_bytes(&bytes, passwords)
}

pub fn open_bytes(bytes: &[u8], passwords: &[&str]) -> Open {
    match open_plain(bytes, passwords) {
        // AES-256 file whose /Perms check is damaged or missing: verify the
        // password ourselves and retry with a repaired in-memory copy.
        // (lopdf reports it as a wrong password, or as a load error when the
        // empty user password is right.)
        failed @ (Open::NeedPassword | Open::Unreadable(_)) => match crate::perms_repair::repair(bytes, passwords) {
            Some(fixed) => match open_plain(&fixed, passwords) {
                Open::Decrypted(d) => Open::Decrypted(d),
                _ => failed,
            },
            None => failed,
        },
        other => other,
    }
}

fn open_plain(bytes: &[u8], passwords: &[&str]) -> Open {
    // Empty password first: owner-only protected files open this way.
    let first = match Document::load_mem(bytes) {
        Ok(doc) => classify(doc),
        Err(e) => return Open::Unreadable(e.to_string()),
    };
    if !matches!(first, Open::NeedPassword) {
        return first;
    }
    for pw in passwords.iter().filter(|p| !p.is_empty()) {
        if let Ok(doc) = Document::load_mem_with_options(bytes, LoadOptions::with_password(pw)) {
            if let Open::Decrypted(d) = classify(doc) {
                return Open::Decrypted(d);
            }
        }
    }
    Open::NeedPassword
}

/// Writes a decrypted document to `output` with no encryption at all.
/// The file is written next to the target first and then moved into place,
/// so the source can safely be the same file as the target.
pub fn save_plain(mut doc: Document, output: &Path) -> Result<(), String> {
    doc.encryption_state = None;
    doc.trailer.remove(b"Encrypt");
    // Object and cross-reference streams were already unpacked into plain
    // objects while loading; the containers themselves must not be written.
    doc.objects.retain(|_, obj| match obj {
        Object::Stream(s) => !(s.dict.has_type(b"ObjStm") || s.dict.has_type(b"XRef")),
        _ => true,
    });
    let max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0);
    doc.max_id = max_id;
    doc.trailer.set("Size", Object::Integer(max_id as i64 + 1));
    doc.trailer.remove(b"Prev");
    let mut buf = Vec::new();
    doc.save_to(&mut buf).map_err(|e| e.to_string())?;

    let dir = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = output.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, &buf).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, output).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}
