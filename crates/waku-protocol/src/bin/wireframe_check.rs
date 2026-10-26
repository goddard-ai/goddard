//! Validate a JSON or HTML wireframe and print a per-screen summary.
//!
//! Usage: `wireframe_check <document.wireframe.json|document.wireframe.html>`

use std::io::Read;
use std::process::ExitCode;

use waku_protocol::wireframe::Wireframe;
use waku_protocol::wireframe_html::{HtmlWireframe, MAX_HTML_BYTES};

fn summarize(path: &str, content: &str) -> anyhow::Result<Vec<String>> {
    if path.to_ascii_lowercase().ends_with(".wireframe.html") {
        return Ok(HtmlWireframe::parse(content)?
            .screens()
            .iter()
            .map(|screen| {
                format!(
                    "{}: {}×{}, {} anchors",
                    screen.name,
                    screen.width,
                    screen.height,
                    screen.anchors.len()
                )
            })
            .collect());
    }
    if !path.to_ascii_lowercase().ends_with(".wireframe.json") {
        anyhow::bail!("wireframe paths must end in .wireframe.json or .wireframe.html");
    }
    Ok(Wireframe::parse(content)?
        .screens
        .iter()
        .map(|screen| {
            format!(
                "{}: {}×{}, {} nodes",
                screen.name,
                screen.width,
                screen.height,
                screen.node_count()
            )
        })
        .collect())
}

fn main() -> ExitCode {
    let Some(document) = std::env::args().nth(1) else {
        eprintln!("usage: wireframe_check <document.wireframe.json|document.wireframe.html>");
        return ExitCode::from(2);
    };
    let mut content = String::new();
    let read = std::fs::File::open(&document).and_then(|file| {
        file.take((MAX_HTML_BYTES + 1) as u64)
            .read_to_string(&mut content)
    });
    if let Err(error) = read {
        eprintln!("{document}: {error}");
        return ExitCode::FAILURE;
    }
    if content.len() > MAX_HTML_BYTES {
        eprintln!("{document}: wireframe exceeds the 1 MiB limit");
        return ExitCode::FAILURE;
    }
    let screens = match summarize(&document, &content) {
        Ok(screens) => screens,
        Err(error) => {
            eprintln!("{document}: {error}");
            return ExitCode::FAILURE;
        }
    };
    for screen in &screens {
        println!("{screen}");
    }
    println!(
        "{document}: ok — {} screen{}",
        screens.len(),
        if screens.len() == 1 { "" } else { "s" },
    );
    ExitCode::SUCCESS
}
