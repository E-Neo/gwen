use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_pptx-engineer")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn tmp() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "pptx-engineer-help-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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
        .filter(|l| l.starts_with("pptx-engineer "))
        .collect()
}

#[test]
fn every_help_example_runs() {
    let subs: [Option<&str>; 8] = [
        None,
        Some("query"),
        Some("add"),
        Some("remove"),
        Some("replace"),
        Some("move"),
        Some("copy"),
        Some("new"),
    ];

    let dir = tmp();
    let input = dir.join("deck.pptx");
    std::fs::copy(fixture("table_chart.pptx"), &input).unwrap();

    let mut run = 0;
    for sub in subs {
        let help = help_for(sub);
        let examples = example_lines(&help);
        assert!(!examples.is_empty(), "{sub:?} --help has no examples");
        assert!(
            help.contains("\nExamples:\n"),
            "{sub:?} --help missing Examples section"
        );

        for line in examples {
            let tokens =
                tokenize(line).unwrap_or_else(|| panic!("unbalanced quotes in example: {line}"));
            assert!(tokens.len() >= 2, "example too short: {line}");
            assert_eq!(
                tokens[0], "pptx-engineer",
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

            let output = dir.join(format!("out-{run}.pptx"));
            run += 1;
            let args = tokens[1..]
                .iter()
                .map(|t| match t.as_str() {
                    "deck.pptx" => input.to_str().unwrap().to_string(),
                    "out.pptx" => output.to_str().unwrap().to_string(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>();

            let result = Command::new(bin())
                .args(&args)
                .output()
                .expect("run example");
            assert!(
                result.status.success(),
                "example failed: {line}\nstderr: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}
