use std::path::PathBuf;
use std::process::Command;

#[path = "decks.rs"]
mod decks;

#[path = "support.rs"]
mod support;

use support::{bin, tmp};

fn fixture(name: &str) -> PathBuf {
    decks::deck(name)
}

fn help_for(sub: Option<&str>) -> String {
    let mut cmd = Command::new(bin());
    if let Some(s) = sub {
        cmd.arg(s);
    }
    cmd.arg("--help");
    let out = cmd.output().expect("run --help");
    assert!(
        out.status.success(),
        "--help failed for {sub:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Tokenize a shell-ish line honoring single/double quotes. Returns None when
/// quotes are unbalanced.
fn tokenize(line: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        tokens.push(std::mem::take(&mut cur));
                    }
                }
                _ => cur.push(c),
            },
        }
    }
    if quote.is_some() {
        return None;
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    Some(tokens)
}

fn example_lines(help: &str) -> Vec<&str> {
    help.lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("gwen "))
        .collect()
}

#[test]
fn every_help_example_runs() {
    let subs: [Option<&str>; 3] = [None, Some("new"), Some("build")];

    let dir = tmp();
    let template = dir.join("template.pptx");
    std::fs::copy(fixture("two_slides.pptx"), &template).unwrap();
    let deck = dir.join("deck");

    for sub in subs {
        let help = help_for(sub);
        let examples = example_lines(&help);
        assert!(!examples.is_empty(), "{sub:?} --help has no examples");
        assert!(
            help.contains("\nExamples:\n"),
            "{sub:?} --help missing Examples section"
        );

        // Examples run in order against the same scratch dir. A `new` example
        // errors when the project already exists, while a `build` example
        // needs the project, so seed/reset around them.
        for line in examples {
            let tokens =
                tokenize(line).unwrap_or_else(|| panic!("unbalanced quotes in example: {line}"));
            assert!(tokens.len() >= 2, "example too short: {line}");
            assert_eq!(
                tokens[0], "gwen",
                "example must start with binary name: {line}"
            );
            assert!(
                !line.contains('|'),
                "example must not use pipelines: {line}"
            );
            assert!(
                !line.contains('\\'),
                "example must not use line continuations: {line}"
            );

            match tokens[1].as_str() {
                "new" => {
                    let _ = std::fs::remove_dir_all(&deck);
                }
                "build" if !deck.join("PRESENTATION.md").exists() => {
                    let seed = Command::new(bin())
                        .args([
                            "new",
                            deck.to_str().unwrap(),
                            "--pptx",
                            template.to_str().unwrap(),
                        ])
                        .output()
                        .expect("seed project");
                    assert!(
                        seed.status.success(),
                        "seeding project failed: {}",
                        String::from_utf8_lossy(&seed.stderr)
                    );
                }
                _ => {}
            }

            let args = tokens[1..]
                .iter()
                .map(|t| match t.as_str() {
                    "deck" => deck.to_str().unwrap().to_string(),
                    "template.pptx" => template.to_str().unwrap().to_string(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>();

            let result = Command::new(bin())
                .args(&args)
                .output()
                .expect("run example");
            assert!(
                result.status.success(),
                "example failed: {line}\nargs: {args:?}\nstderr: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}
