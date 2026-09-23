//! `gwen new` scaffolding: copy a user template from the gwen home when one
//! exists, otherwise fall back to the built-in starter deck.
//!
//! The gwen home is `$GWEN_HOME` if set, else `~/.gwen`. A `template/`
//! directory there provides `main.toml` (required) plus optional
//! `masters/`, `slides/` and `media/` trees that are copied verbatim. The
//! `[presentation] title` of the copied `main.toml` is always overwritten
//! with the directory name given to `gwen new`.

use std::path::{Path, PathBuf};

use gwen::error::Result;

pub fn gwen_home() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("GWEN_HOME")
        && !home.trim().is_empty()
    {
        return Ok(PathBuf::from(home));
    }
    let home = std::env::home_dir().ok_or_else(|| {
        miette::miette!("cannot determine the home directory (set GWEN_HOME or $HOME)")
    })?;
    Ok(home.join(".gwen"))
}

/// `<gwen home>/template`, if it exists.
pub fn template_dir() -> Option<PathBuf> {
    let dir = gwen_home().ok()?.join("template");
    dir.is_dir().then_some(dir)
}

/// Scaffold a new project into `root` (which must not exist yet). `name` is
/// the directory name, written into `[presentation] title`.
pub fn scaffold(root: &Path, name: &str, no_template: bool) -> Result<()> {
    create_dir(root)?;
    if !no_template && let Some(template) = template_dir() {
        scaffold_from_template(root, &template, name)?;
        eprintln!(
            "created project `{}` from template `{}`",
            root.display(),
            template.display()
        );
        eprintln!(
            "  edit {}/main.toml and slides/*.toml, then run `gwen build {}`",
            root.display(),
            root.display()
        );
        return Ok(());
    }
    scaffold_builtin(root, name)
}

fn scaffold_from_template(root: &Path, template: &Path, name: &str) -> Result<()> {
    let main_src = template.join("main.toml");
    if !main_src.is_file() {
        return Err(miette::miette!(
            "template `{}` has no `main.toml`",
            template.display()
        ));
    }
    let contents = std::fs::read_to_string(&main_src)
        .map_err(|e| miette::miette!("cannot read `{}`: {e}", main_src.display()))?;
    write_file(&root.join("main.toml"), &set_title(&contents, name))?;
    for sub in ["masters", "slides", "media"] {
        let src = template.join(sub);
        if src.is_dir() {
            copy_dir_all(&src, &root.join(sub))?;
        }
    }
    Ok(())
}

fn scaffold_builtin(root: &Path, name: &str) -> Result<()> {
    create_dir(&root.join("slides"))?;
    create_dir(&root.join("masters"))?;
    create_dir(&root.join("media"))?;
    write_file(
        &root.join("main.toml"),
        &DEFAULT_MAIN.replace("__NAME__", name),
    )?;
    write_file(&root.join("masters").join("base.toml"), DEFAULT_MASTER)?;
    write_file(&root.join("slides").join("title.toml"), DEFAULT_TITLE)?;
    write_file(&root.join("slides").join("intro.toml"), DEFAULT_INTRO)?;
    eprintln!("created project `{}`", root.display());
    eprintln!(
        "  edit {}/main.toml and slides/*.toml, then run `gwen build {}`",
        root.display(),
        root.display()
    );
    Ok(())
}

/// Overwrite the `[presentation] title` in `main.toml` with `name`: replace
/// the value of an existing `title = "…"` line inside the `[presentation]`
/// section, or insert a `title = "<name>"` line if there is none. Everything
/// else is left byte-for-byte intact.
fn set_title(contents: &str, name: &str) -> String {
    let name_escaped = name.replace('"', "\\\"");
    let needle = format!("title = \"{name_escaped}\"");
    let mut out_lines: Vec<String> = Vec::new();
    let mut in_presentation = false;
    let mut replaced = false;

    for line in contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_presentation = trimmed.starts_with("[presentation]");
        } else if in_presentation
            && !replaced
            && let Some(eq) = trimmed.find('=')
            && trimmed[..eq].trim() == "title"
        {
            let indent = &line[..line.len() - line.trim_start().len()];
            out_lines.push(format!("{indent}{needle}"));
            replaced = true;
            continue;
        }
        out_lines.push(line.to_string());
    }

    if !replaced {
        // No [presentation] section at all: put one at the very top.
        if !contents.contains("[presentation]") {
            return format!("[presentation]\ntitle = \"{name_escaped}\"\n{contents}");
        }
        // Insert a title line right after the [presentation] header.
        let mut final_lines = Vec::new();
        for line in &out_lines {
            final_lines.push(line.clone());
            if line.trim_start().starts_with("[presentation]") {
                final_lines.push(format!("title = \"{name_escaped}\""));
            }
        }
        return final_lines.join("\n") + "\n";
    }
    out_lines.join("\n") + "\n"
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    create_dir(dst)?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| miette::miette!("cannot list `{}`: {e}", src.display()))?
    {
        let entry = entry.map_err(|e| miette::miette!("cannot read `{}`: {e}", src.display()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| miette::miette!("cannot copy `{}`: {e}", from.display()))?;
        }
    }
    Ok(())
}

fn create_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .map_err(|e| miette::miette!("cannot create {}: {e}", path.display()))
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents)
        .map_err(|e| miette::miette!("cannot write {}: {e}", path.display()))
}

const DEFAULT_MAIN: &str = r##"[presentation]
title = "__NAME__"
author = ""
company = ""
subject = ""
width = 12196763
height = 6858000

[theme]
major_font = "Arial Black"
minor_font = "Arial"

# Slide index: [[sections]] is required and lists the slide files in order.
[[sections]]
title = "Intro"
slides = ["title.toml", "intro.toml"]

# [styles.shape] is the base style for every shape; [styles.<type>] layers on
# top of it for specific shape types (text, image, rect, ellipse, ...).
# [styles.shape]
# fill = { color = "C7000A" }
# [styles.text]
# font_face = "Arial"
# font_size = 18
# color = "262626"
# [styles.image]
# sizing = { type = "contain" }

# Named styles; a shape referencing `style = "muted"` merges these.
# [styles.named.muted]
# color = "808080"
# italic = true
"##;

const DEFAULT_MASTER: &str = r##"# masters/base.toml — the master name is the file stem.

background = { color = "FFFFFF" }
slide_number = { x = "12.2in", y = "7.1in", w = "1in", h = "0.3in",
                 font_size = 12, color = "999999", align = "right" }

[[shapes]]
type = "rect"
x = 0
y = 0
w = "100%"
h = "1.1in"
fill = { color = "C7000A" }
line = { color = "C7000A", width = 0 }

[[shapes]]
type = "placeholder"
ph_type = "title"
name = "Title"
x = "0.8in"
y = "0.18in"
w = "11.7in"
h = "0.74in"
font_size = 26
color = "FFFFFF"
bold = true
align = "left"
valign = "middle"
text = "Click to edit title"
"##;

const DEFAULT_TITLE: &str = r##"# slides/title.toml

master = "base"

[[shapes]]
type = "text"
placeholder = "Title"
text = "*Welcome to* **gwen**"
"##;

const DEFAULT_INTRO: &str = r##"# slides/intro.toml

master = "base"

[[shapes]]
type = "text"
x = "1in"
y = "1.8in"
w = "11.3in"
h = "4in"
text = """**gwen** builds .pptx from TOML.

It drives the real pptxgenjs under QuickJS, so everything is
[standard](https://gitbrent.github.io/PptxGenJS/) PowerPoint.
"""
font_size = 24
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_title_replaces_in_presentation() {
        let src = "[presentation]\ntitle = \"Old\"\nwidth = 1\n\n[theme]\nmajor_font = \"X\"\n";
        let out = set_title(src, "deck");
        assert!(out.contains("title = \"deck\""));
        assert!(!out.contains("title = \"Old\""));
        assert!(out.contains("major_font = \"X\""));
    }

    #[test]
    fn set_title_inserts_when_missing() {
        let src = "[presentation]\nwidth = 1\n\n[[sections]]\nslides = []\n";
        let out = set_title(src, "deck");
        assert!(out.contains("title = \"deck\""));
        assert!(out.contains("width = 1"));
        assert!(out.contains("[[sections]]"));
    }

    #[test]
    fn set_title_adds_presentation_when_absent() {
        let src = "[[sections]]\nslides = []\n";
        let out = set_title(src, "deck");
        assert!(out.starts_with("[presentation]"));
        assert!(out.contains("title = \"deck\""));
        assert!(out.contains("[[sections]]"));
    }
}
