use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_gwen")
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
        "gwen-help-{}-{}",
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
        .filter(|l| l.starts_with("gwen "))
        .collect()
}

#[test]
fn every_help_example_runs() {
    let subs: [Option<&str>; 3] = [None, Some("markdown"), Some("update")];

    let dir = tmp();
    let input = dir.join("deck.pptx");
    std::fs::copy(fixture("table_chart.pptx"), &input).unwrap();

    // Pre-create deck.md (the markdown mirror used by update examples) so the
    // examples run regardless of their order in the help text.
    let mirror = Command::new(bin())
        .args(["markdown", "--input", input.to_str().unwrap()])
        .output()
        .expect("seed deck.md");
    assert!(mirror.status.success(), "seed markdown failed");
    std::fs::write(dir.join("deck.md"), mirror.stdout).unwrap();

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

            // Split off an optional `> file` shell redirect so the example can
            // be executed without a shell.
            let (redirect, rest) = match tokens.iter().position(|t| t == ">") {
                Some(pos) => (Some(tokens[pos + 1].clone()), &tokens[..pos]),
                None => (None, &tokens[..]),
            };

            let output = dir.join(format!("out-{run}.pptx"));
            run += 1;
            let args = rest[1..]
                .iter()
                .map(|t| match t.as_str() {
                    "deck.pptx" => input.to_str().unwrap().to_string(),
                    "deck.md" => dir.join("deck.md").to_str().unwrap().to_string(),
                    "out.pptx" => output.to_str().unwrap().to_string(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>();

            let mut cmd = Command::new(bin());
            cmd.args(&args);
            let result = cmd.output().expect("run example");
            if let Some(file) = redirect {
                std::fs::write(dir.join(file), &result.stdout).unwrap();
            }
            assert!(
                result.status.success(),
                "example failed: {line}\nargs: {args:?}\nstderr: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}
