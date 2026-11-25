// PDF Password Buster v1.0 - repair of damaged /Perms entries in AES-256 PDFs.
// Copyright 2026 Daniel Dias Rodrigues
//
// Licensed under the MIT License. You may use, copy, modify, merge, publish,
// distribute, sublicense and/or sell copies of this software under the terms
// of that license; see the LICENSE file distributed with this program.
// SPDX-License-Identifier: MIT

//! Repair of a broken `/Perms` entry in AES-256 (revision 5/6) encrypted PDFs.
//!
//! `/Perms` is only a consistency check of the `/P` permission flags; it plays no
//! part in decrypting the content. Some files carry a damaged or missing `/Perms`
//! (for example after a tool edited `/P`), and the PDF library then refuses the
//! file even with the right password. Here the password is checked directly
//! against `/U` or `/O` (ISO 32000-2 Algorithms 2.A/2.B), and when it is right a
//! valid `/Perms` is appended to an in-memory copy as an incremental update, so
//! the normal decryption path can open it. The file on disk is never changed.

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use aes::{Aes128, Aes256};
use lopdf::{Document, Object, StringFormat, Writer};
use sha2::{Digest, Sha256, Sha384, Sha512};

fn aes128_cbc_encrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut prev = [0u8; 16];
    prev.copy_from_slice(iv);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut block = [0u8; 16];
        for i in 0..16 {
            block[i] = chunk[i] ^ prev[i];
        }
        let mut b = GenericArray::from(block);
        cipher.encrypt_block(&mut b);
        prev.copy_from_slice(&b);
        out.extend_from_slice(&b);
    }
    out
}

/// AES-256-CBC decryption with a zero IV and no padding (used for /UE and /OE).
fn aes256_cbc_decrypt_zero_iv(key: &[u8], data: &[u8]) -> Vec<u8> {
    let cipher = Aes256::new(GenericArray::from_slice(key));
    let mut prev = [0u8; 16];
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut b = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut b);
        for i in 0..16 {
            out.push(b[i] ^ prev[i]);
        }
        prev.copy_from_slice(chunk);
    }
    out
}

/// Algorithm 2.B (revision 6) or the plain SHA-256 of revision 5.
fn hash(revision: i64, password: &[u8], salt: &[u8], udata: &[u8]) -> Vec<u8> {
    let mut first = Sha256::new();
    first.update(password);
    first.update(salt);
    first.update(udata);
    let mut k: Vec<u8> = first.finalize().to_vec();
    if revision < 6 {
        return k;
    }
    let mut round: u32 = 0;
    loop {
        let mut k1 = Vec::with_capacity(64 * (password.len() + k.len() + udata.len()));
        for _ in 0..64 {
            k1.extend_from_slice(password);
            k1.extend_from_slice(&k);
            k1.extend_from_slice(udata);
        }
        let e = aes128_cbc_encrypt(&k[..16], &k[16..32], &k1);
        // The first 16 bytes as a big-endian number, mod 3, equals their byte sum mod 3.
        let sum: u32 = e[..16].iter().map(|&b| b as u32).sum();
        k = match sum % 3 {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        round += 1;
        if round >= 64 && (*e.last().unwrap_or(&0) as u32) <= round - 32 {
            break;
        }
    }
    k.truncate(32);
    k
}

fn bytes_of(dict: &lopdf::Dictionary, key: &[u8]) -> Option<Vec<u8>> {
    dict.get(key).ok().and_then(|o| o.as_str().ok()).map(|s| s.to_vec())
}

/// The file encryption key, when `password` is the owner or the user password.
fn file_key(revision: i64, password: &[u8], o: &[u8], oe: &[u8], u: &[u8], ue: &[u8]) -> Option<Vec<u8>> {
    let pw = &password[..password.len().min(127)];
    if hash(revision, pw, &o[32..40], &u[..48]) == o[..32] {
        let k = hash(revision, pw, &o[40..48], &u[..48]);
        return Some(aes256_cbc_decrypt_zero_iv(&k, &oe[..32]));
    }
    if hash(revision, pw, &u[32..40], &[]) == u[..32] {
        let k = hash(revision, pw, &u[40..48], &[]);
        return Some(aes256_cbc_decrypt_zero_iv(&k, &ue[..32]));
    }
    None
}

/// Offset given by the last `startxref` of the file.
fn last_startxref(bytes: &[u8]) -> Option<u64> {
    let pos = bytes.windows(9).rposition(|w| w == b"startxref")?;
    let rest = &bytes[pos + 9..];
    let digits: String = rest
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect();
    digits.parse().ok()
}

/// Returns a copy of `bytes` with a valid `/Perms` appended, when the file is
/// AES-256 encrypted and one of `passwords` (or the empty one) is right.
pub fn repair(bytes: &[u8], passwords: &[&str]) -> Option<Vec<u8>> {
    // Hide the old /Perms key (same length, so offsets stay valid): with it gone,
    // the library loads the file in its "needs password" state, where the
    // /Encrypt dictionary can be read even when the empty password is right.
    let mut neutral = bytes.to_vec();
    let mut i = 0;
    while let Some(pos) = neutral[i..].windows(6).position(|w| w == b"/Perms") {
        neutral[i + pos + 1] = b'X';
        i += pos + 6;
    }
    let bytes = &neutral[..];
    let doc = Document::load_mem(bytes).ok()?;
    let enc_id = doc.trailer.get(b"Encrypt").ok()?.as_reference().ok()?;
    let enc = doc.get_dictionary(enc_id).ok()?.clone();
    let revision = enc.get(b"R").ok()?.as_i64().ok()?;
    if !(5..=6).contains(&revision) {
        return None;
    }
    let (o, oe, u, ue) = (bytes_of(&enc, b"O")?, bytes_of(&enc, b"OE")?, bytes_of(&enc, b"U")?, bytes_of(&enc, b"UE")?);
    if o.len() < 48 || u.len() < 48 || oe.len() < 32 || ue.len() < 32 {
        return None;
    }
    let p = enc.get(b"P").ok()?.as_i64().ok()?;
    let encrypt_metadata = enc.get(b"EncryptMetadata").ok().and_then(|o| o.as_bool().ok()).unwrap_or(true);

    let mut candidates: Vec<&str> = vec![""];
    candidates.extend(passwords.iter().copied().filter(|p| !p.is_empty()));
    let key = candidates.iter().find_map(|pw| file_key(revision, pw.as_bytes(), &o, &oe, &u, &ue))?;

    // Algorithm 10: P (little-endian) | FF FF FF FF | 'T'/'F' | "adb" | 4 filler bytes.
    let mut block = [0u8; 16];
    block[..4].copy_from_slice(&(p as u32).to_le_bytes());
    block[4..8].copy_from_slice(&[0xFF; 4]);
    block[8] = if encrypt_metadata { b'T' } else { b'F' };
    block[9..12].copy_from_slice(b"adb");
    block[12..16].copy_from_slice(b"rpwr");
    let cipher = Aes256::new(GenericArray::from_slice(&key));
    let mut b = GenericArray::from(block);
    cipher.encrypt_block(&mut b);

    let mut fixed = enc;
    fixed.set("Perms", Object::String(b.to_vec(), StringFormat::Hexadecimal));

    // Incremental update: the new /Encrypt object, a one-entry xref section and a trailer.
    let prev = last_startxref(bytes)?;
    let mut out = bytes.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let obj_offset = out.len();
    out.extend_from_slice(format!("{} {} obj\n", enc_id.0, enc_id.1).as_bytes());
    Writer::write_object(&mut out, &Object::Dictionary(fixed)).ok()?;
    out.extend_from_slice(b"\nendobj\n");
    let xref_offset = out.len();
    out.extend_from_slice(format!("xref\n{} 1\n{:010} {:05} n \n", enc_id.0, obj_offset, enc_id.1).as_bytes());

    let mut trailer = lopdf::Dictionary::new();
    for key in [&b"Root"[..], b"Info", b"ID", b"Encrypt"] {
        if let Ok(v) = doc.trailer.get(key) {
            trailer.set(key.to_vec(), v.clone());
        }
    }
    let size = doc.trailer.get(b"Size").ok().and_then(|s| s.as_i64().ok()).unwrap_or(0).max(enc_id.0 as i64 + 1);
    trailer.set("Size", Object::Integer(size));
    trailer.set("Prev", Object::Integer(prev as i64));
    out.extend_from_slice(b"trailer\n");
    Writer::write_object(&mut out, &Object::Dictionary(trailer)).ok()?;
    out.extend_from_slice(format!("\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
    Some(out)
}
