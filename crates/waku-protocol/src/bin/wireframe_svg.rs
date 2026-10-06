//! Render a `.wireframe.json` document to one SVG per screen — the same
//! output the in-app preview and the Figma export produce. This is the
//! cheap proof that a document parses and lays out before either of
//! those consumers exists.
//!
//! Usage: `wireframe_svg <document.wireframe.json> <output-dir>`

use std::path::PathBuf;
use std::process::ExitCode;

use waku_protocol::wireframe::Wireframe;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(document), Some(output)) = (args.next(), args.next()) else {
        eprintln!("usage: wireframe_svg <document.wireframe.json> <output-dir>");
        return ExitCode::from(2);
    };
    let json = match std::fs::read_to_string(&document) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("{document}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let wireframe = match Wireframe::parse(&json) {
        Ok(wireframe) => wireframe,
        Err(error) => {
            eprintln!("{document}: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = std::fs::create_dir_all(&output) {
        eprintln!("{output}: {error}");
        return ExitCode::FAILURE;
    }
    let output = PathBuf::from(output);
    for (index, screen) in wireframe.screens.iter().enumerate() {
        let path = output.join(format!("{:02}-{}.svg", index + 1, slug(&screen.name)));
        match std::fs::write(&path, screen.render_svg()) {
            Ok(()) => println!("{}", path.display()),
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        }
    }
    println!(
        "rendered {} screen{} from {document}",
        wireframe.screens.len(),
        if wireframe.screens.len() == 1 {
            ""
        } else {
            "s"
        },
    );
    ExitCode::SUCCESS
}

/// Screen names become filename-safe slugs: "Memory — file tree +
/// preview" → "memory-file-tree-preview".
fn slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "screen".to_owned()
    } else {
        slug.to_owned()
    }
}
