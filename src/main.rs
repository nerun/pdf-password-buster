// PDF Password Buster v1.0 - program entry point and command line options.
// Copyright 2026 Daniel Dias Rodrigues
//
// Licensed under the MIT License. You may use, copy, modify, merge, publish,
// distribute, sublicense and/or sell copies of this software under the terms
// of that license; see the LICENSE file distributed with this program.
// SPDX-License-Identifier: MIT

//! PDF Password Buster: removes owner/user passwords and permission
//! restrictions from PDF files on Linux, Windows and macOS. No registration
//! or activation key is needed.

// Release builds on Windows are GUI programs (no console window pops up);
// the command line mode re-attaches to the console it was started from.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod cli;
mod engine;
mod perms_repair;

use eframe::egui;
use std::path::{Path, PathBuf};

/// The program icon (keys), embedded in the executable.
pub const ICON_128: &[u8] = include_bytes!("../assets/icons/pdf-password-buster-128.png");
pub const ICON_64: &[u8] = include_bytes!("../assets/icons/pdf-password-buster-64.png");

/// Decodes an embedded PNG into width, height and RGBA pixels.
pub fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    Some((info.width, info.height, buf))
}

/// Opens a file with the desktop's default application (like ShellExecute "open").
pub fn open_with_system(path: &Path) {
    use std::process::{Command, Stdio};
    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = Command::new("explorer");
        c.creation_flags(CREATE_NO_WINDOW);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = Command::new("xdg-open");
    let _ = cmd.arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// Windows GUI builds have no console; borrow the one of the shell that
/// started the program so command line messages are visible.
#[cfg(target_os = "windows")]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: plain Win32 call with a constant argument; failure is harmless.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if cli::wants_cli(&args) {
        #[cfg(target_os = "windows")]
        attach_parent_console();
        std::process::exit(cli::run(&args));
    }

    let mut screenshot: Option<PathBuf> = None;
    let mut initial_files = Vec::new();
    let mut out_dir: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--screenshot" {
            screenshot = it.next().map(PathBuf::from);
        } else if a == "--output-dir" {
            out_dir = it.next().map(PathBuf::from);
        } else if cli::is_pdf(Path::new(a)) {
            initial_files.push(PathBuf::from(a));
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(app::TITLE)
        .with_inner_size([560.0, 400.0])
        .with_min_inner_size([460.0, 320.0])
        .with_app_id("pdf-password-buster")
        .with_drag_and_drop(true);
    if let Some((width, height, rgba)) = decode_png(ICON_128) {
        viewport = viewport.with_icon(std::sync::Arc::new(egui::IconData { rgba, width, height }));
    }
    if screenshot.is_some() {
        // Never steal focus from the person while a check takes the picture.
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }

    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    // macOS: a check's screenshot run must not bring the app to the front.
    #[cfg(target_os = "macos")]
    if screenshot.is_some() {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            builder.with_activate_ignoring_other_apps(false);
        }));
    }

    eframe::run_native(
        app::TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(app::RemoverApp::new(cc, screenshot, initial_files, out_dir)))),
    )
}
