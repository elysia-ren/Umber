//! CLI：`catalog-builder <source1.json> [source2.json ...] -o catalog.json`
//!
//! 输入文件形状见 [`catalog_builder::SourceFile`]。

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut inputs: Vec<String> = Vec::new();
    let mut output = String::from("catalog.json");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" => match args.next() {
                Some(o) => output = o,
                None => {
                    eprintln!("error: -o requires a path");
                    return ExitCode::FAILURE;
                }
            },
            other => inputs.push(other.to_string()),
        }
    }

    let mut sources = Vec::new();
    for path in &inputs {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: cannot read {path}: {e}");
                return ExitCode::FAILURE;
            }
        };
        match serde_json::from_str::<catalog_builder::SourceFile>(&text) {
            Ok(s) => sources.push(s),
            Err(e) => {
                eprintln!("error: {path} is not a valid source file: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    match catalog_builder::build(&sources, now) {
        Ok(catalog) => {
            let json = serde_json::to_string_pretty(&catalog).expect("catalog serialization");
            if let Err(e) = std::fs::write(&output, json) {
                eprintln!("error: cannot write {output}: {e}");
                return ExitCode::FAILURE;
            }
            println!(
                "wrote {output}: {} identities, {} entries, {} sources",
                catalog.identities.len(),
                catalog.entries.len(),
                catalog.sources.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
