//! Validate a `.wireframe.json` document — the same `Wireframe::parse` the
//! in-app preview runs — and print a per-screen summary. This is the cheap
//! proof that a hand-authored document parses and satisfies the v1
//! invariants before the app opens it.
//!
//! Usage: `wireframe_check <document.wireframe.json>`

use std::process::ExitCode;

use waku_protocol::wireframe::Wireframe;

fn main() -> ExitCode {
    let Some(document) = std::env::args().nth(1) else {
        eprintln!("usage: wireframe_check <document.wireframe.json>");
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
    for screen in &wireframe.screens {
        println!(
            "{}: {}×{}, {} nodes",
            screen.name,
            screen.width,
            screen.height,
            screen.node_count(),
        );
    }
    println!(
        "{document}: ok — {} screen{}",
        wireframe.screens.len(),
        if wireframe.screens.len() == 1 {
            ""
        } else {
            "s"
        },
    );
    ExitCode::SUCCESS
}
