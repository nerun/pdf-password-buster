// PDF Password Buster v1.0 - command line batch mode.
// Copyright 2026 Daniel Dias Rodrigues
//
// Licensed under the MIT License. You may use, copy, modify, merge, publish,
// distribute, sublicense and/or sell copies of this software under the terms
// of that license; see the LICENSE file distributed with this program.
// SPDX-License-Identifier: MIT

//! Command line batch mode, compatible with the original pdfdecrypt options:
//! -i <file|dir>  -o <file|dir>  -u <user pw>  -w <owner pw>  -v  -l <log>  -h/-?

use crate::engine::{self, Open};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const USAGE: &str = "\
PDF Password Buster v1.0 (command line)
Copyright 2026 Daniel Dias Rodrigues - MIT License
---------------------------------------------------------------------
Usage: pdf-password-buster [options]
 -i [pdf file name] : PDF filename or directory to be decrypted.
 -o [pdf file name] : Output PDF filename or directory. If you do not
                      specify -o, the output overwrites the input PDF
                      file.
 -u                 : User password.
 -w                 : Owner password.
 -v                 : View the decrypted PDF file after decryption.
 -l [log file name] : Specify log file for output message.
 -? or -h           : Help.
---------------------------------------------------------------------
Example:
 pdf-password-buster -i ~/sample.pdf -o ~/decrypt.pdf \\
     -w owner -u user -l ~/error.log
 pdf-password-buster -i ~/sample.pdf
 pdf-password-buster -i ~/sample.pdf -o ~/out.pdf -w \"owner password\"
 pdf-password-buster -i ~/sample.pdf -o ~/out.pdf -u \"user password\"
 pdf-password-buster -i ~/pdfdir/ -o ~/outdir/ -l ~/error.log
";

/// True when the arguments ask for the command line mode.
pub fn wants_cli(args: &[String]) -> bool {
    args.iter().any(|a| matches!(a.as_str(), "-i" | "-h" | "-?" | "--help"))
}

struct Log {
    file: Option<std::fs::File>,
}

impl Log {
    fn line(&mut self, msg: &str) {
        println!("{msg}");
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{msg}");
        }
    }
}

/// Runs the batch mode and returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let mut input = None;
    let mut output = None;
    let mut user = String::new();
    let mut owner = String::new();
    let mut view = false;
    let mut log_path = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-i" => input = it.next().cloned(),
            "-o" => output = it.next().cloned(),
            "-u" => user = it.next().cloned().unwrap_or_default(),
            "-w" => owner = it.next().cloned().unwrap_or_default(),
            "-l" => log_path = it.next().cloned(),
            "-v" => view = true,
            "-h" | "-?" | "--help" => {
                print!("{USAGE}");
                return 0;
            }
            _ => {}
        }
    }
    let Some(input) = input else {
        print!("{USAGE}");
        return 1;
    };
    let mut log = Log {
        file: log_path.and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok()),
    };

    let input = PathBuf::from(input);
    let mut jobs: Vec<(PathBuf, PathBuf)> = Vec::new();
    if input.is_dir() {
        let out_dir = output.map(PathBuf::from).unwrap_or_else(|| input.clone());
        let _ = std::fs::create_dir_all(&out_dir);
        let mut files: Vec<PathBuf> = std::fs::read_dir(&input)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| is_pdf(p)).collect())
            .unwrap_or_default();
        files.sort();
        for f in files {
            let target = out_dir.join(f.file_name().unwrap_or_default());
            jobs.push((f, target));
        }
    } else {
        let target = match output {
            Some(o) if Path::new(&o).is_dir() => Path::new(&o).join(input.file_name().unwrap_or_default()),
            Some(o) => PathBuf::from(o),
            None => input.clone(),
        };
        jobs.push((input, target));
    }

    let mut failures = 0;
    for (src, dst) in jobs {
        let shown = src.display().to_string();
        match engine::open(&src, &[&owner, &user]) {
            Open::NotEncrypted => {
                failures += 1;
                log.line(&format!("[ERROR] '{shown}': File is not encrypted."));
            }
            Open::Unreadable(_) => {
                failures += 1;
                log.line(&format!("[ERROR] Failed to open '{shown}' file."));
            }
            Open::NeedPassword => {
                failures += 1;
                log.line(&format!("[FAILED] '{shown}': Password is not correct."));
            }
            Open::Decrypted(doc) => match engine::save_plain(*doc, &dst) {
                Ok(()) => {
                    log.line(&format!("[SUCCEED] '{shown}' -> '{}': Password Removed.", dst.display()));
                    if view {
                        crate::open_with_system(&dst);
                    }
                }
                Err(_) => {
                    failures += 1;
                    log.line(&format!("[FAILED] Failed to create \"{}\" file.", dst.display()));
                }
            },
        }
    }
    if failures > 0 { 2 } else { 0 }
}

pub fn is_pdf(p: &Path) -> bool {
    p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}
