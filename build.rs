// PDF Password Buster v1.0 - build script: embeds the icon in the Windows executable.
// Copyright 2026 Daniel Dias Rodrigues
//
// Licensed under the MIT License. You may use, copy, modify, merge, publish,
// distribute, sublicense and/or sell copies of this software under the terms
// of that license; see the LICENSE file distributed with this program.
// SPDX-License-Identifier: MIT

fn main() {
    println!("cargo:rerun-if-changed=assets/icons/pdf-password-buster.ico");
    // Only Windows executables carry their icon as a resource; on Linux and
    // macOS the icon comes from the desktop entry or the app bundle.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/pdf-password-buster.ico")
            .set("ProductName", "PDF Password Buster")
            .set("FileDescription", "PDF Password Buster")
            .set("LegalCopyright", "Copyright 2026 Daniel Dias Rodrigues");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
