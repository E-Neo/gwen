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
    scaffold_project_skill(root, Some(template), name)?;
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
    scaffold_project_skill(root, None, name)?;
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

/// Scaffold the project-level skill at `.agents/skills/gwen/`. A template may
/// ship extra skills under its own `.agents/` (copied verbatim), but the
/// `gwen` skill name is reserved by gwen itself - templates must not provide it.
fn scaffold_project_skill(root: &Path, template: Option<&Path>, _name: &str) -> Result<()> {
    if let Some(template) = template {
        let reserved = template.join(".agents").join("skills").join("gwen");
        if reserved.is_dir() {
            return Err(miette::miette!(
                "the `gwen` skill name is reserved by gwen; remove `.agents/skills/gwen` from the template"
            ));
        }
        let agents = template.join(".agents");
        if agents.is_dir() {
            copy_dir_all(&agents, &root.join(".agents"))?;
        }
    }
    let dir = root.join(".agents").join("skills").join("gwen");
    create_dir(&dir.join("references"))?;
    let spec = DEFAULT_DSL_SPEC
        .replace("{{shape_types}}", &shape_types())
        .replace("{{option_fields}}", &option_fields());
    write_file(&dir.join("SKILL.md"), DEFAULT_SKILL_MD)?;
    write_file(&dir.join("references").join("toml-dsl.md"), &spec)?;
    Ok(())
}

/// All supported shape preset ids as snake_case, sorted, in one paragraph.
fn shape_types() -> String {
    let mut ids: Vec<String> = gwen::opts::SHAPE_PRESETS
        .iter()
        .map(|p| gwen::opts::camel_to_snake(p))
        .collect();
    ids.sort();
    ids.join(", ")
}

/// The option key inventory per context, rendered from the whitelists.
fn option_fields() -> String {
    let group = |title: &str, keys: Option<&[&str]>| -> String {
        match keys {
            Some(keys) => {
                let list: Vec<&str> = keys.to_vec();
                format!("- **{title}**: {}", list.join(", "))
            }
            None => String::new(),
        }
    };

    let nested: Vec<String> = [
        ("fill", gwen::opts::fill_schema()),
        ("line", gwen::opts::line_schema()),
        ("shadow", gwen::opts::shadow_schema()),
        ("bullet", gwen::opts::bullet_schema()),
        ("sizing", gwen::opts::sizing_schema()),
        ("hyperlink", gwen::opts::hyperlink_schema()),
        ("underline", gwen::opts::underline_schema()),
        ("outline", gwen::opts::outline_schema()),
        ("glow", gwen::opts::glow_schema()),
    ]
    .iter()
    .map(|(name, schema)| {
        let keys: Vec<&str> = schema.iter().map(|(n, _)| *n).collect();
        format!("`{name}` (`{}`)", keys.join("`, `"))
    })
    .collect();

    [
        group("Shape/preset", Some(gwen::opts::TEXTSHAPE_KEYS)),
        group("Text", Some(gwen::opts::TEXT_KEYS)),
        group("Image", Some(gwen::opts::IMAGE_KEYS)),
        group("Background", Some(gwen::opts::BACKGROUND_KEYS)),
        format!(
            "- **Nested objects**: {}\n- **Gwen-only keys** (never sent to pptxgenjs): `styles`, `ordered_markers`, `unordered_markers`",
            nested.join("; ")
        ),
    ]
    .into_iter()
    .collect::<Vec<_>>()
    .join("\n")
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

const DEFAULT_MASTER: &str = r##"# masters/base.toml - the master name is the file stem.

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

/// The `SKILL.md` of the project-level skill generated by `gwen new` (skill
/// name `gwen`): a short overview and pointer to the full specification.
const DEFAULT_SKILL_MD: &str = r##"---
name: gwen
description: Editing or building this gwen deck's TOML files (main.toml, masters, slides, styles, rich text).
---

# gwen deck

This directory is a **gwen** project: a PowerPoint deck defined by TOML files.
`gwen build` regenerates `target/<title>.pptx` deterministically - edit the
TOML, never the `.pptx`. TOML is the single source of truth.

## When to use
Editing or building slides, masters, styles or any `.toml` file in this
project.

## Quick rules
- `[[sections]]` in main.toml is required and is the slide index
- styles precedence: `[styles.shape] < [styles.text] < [styles.<type>] < shape styles < shape keys`; `styles` accepts a string or a list and may be recursive (cycles are errors)
- shape types and marker names are snake_case (`round_rect`); option keys are
  snake_case (`font_size`)
- text is markdown: `**bold**`, `*italic*`, `[link](url)`, a single `\n` is a
  soft break, a blank line is a new paragraph, `- item` / `1. item` are
  bulleted lists
- coordinates: EMU integers or `"1in"` / `"2.5cm"` / `"25mm"` / `"72pt"` /
  `"50%"`
- gwen validates strictly: unknown fields/options/`styles` references/cycles
  and wrong option value types are loud errors - read the diagnostics

## Full specification
Read **references/toml-dsl.md** for the complete TOML DSL specification
(project layout, styles, masters, slides, supported shapes and their option
fields, validation rules).
"##;

/// The full TOML DSL specification at `.agents/skills/gwen/references/toml-dsl.md`.
const DEFAULT_DSL_SPEC: &str = r##"# gwen TOML DSL spec

## Layout

This directory is a **gwen** project: a PowerPoint deck defined by TOML files.
`gwen build` regenerates `target/<title>.pptx` deterministically - edit the
TOML, never the `.pptx`. TOML is the single source of truth.

```
deck/
  main.toml            presentation metadata, theme, sections, styles
  masters/*.toml       one slide master per file (the file stem is the master name)
  slides/*.toml        one slide per file
  media/               images referenced by slides and masters
```

## main.toml

- `[presentation]` - `title` (output file stem), `author`, `company`,
  `revision`, `subject`, `rtl_mode`, `width`/`height` (EMU or
  `"1in"`/`"2.5cm"`/`"25mm"`/`"72pt"`/`"50%"`).
- `[theme]` - `major_font` / `minor_font` (inherited by heading/body text).
- `[[sections]]` - **required**; the slide ordering index. Each lists slide
  files: `slides = ["title.toml", "content.toml"]`. A slide in two sections is
  an error; a slide in none is not rendered.
- `[styles.*]` - see below.

## Styles

Precedence when building a shape's options:

```
[styles.shape] < [styles.text] < [styles.<type>] < shape's own `styles` < shape keys
```

- `[styles.shape]` is the base for every shape; `[styles.text]` layers onto it
  for text-carrying shapes (type `text`, or a preset with a text value);
  `[styles.<type>]` layers per type id (`text`, `image`, `rect`, `ellipse`, ...).
- `[styles.named.<name>]` defines named styles. Shapes and buckets opt in with a
  `styles` key that accepts a scalar or a list, applied in order (later wins);
  named styles may reference other named styles (recursion). A reference to a
  missing name or a cycle (`a -> b -> a`) is an error. Inside a container its
  own inline options win over what it includes.
- Gwen-owned keys (never passed to pptxgenjs): `styles`, `ordered_markers`,
  `unordered_markers`.

## masters/<name>.toml

The master name is the file stem - no `title` field.

```toml
background = { color = "FFFFFF" }
margin = 0.5                       # inches/EMU/array
slide_number = { x = "12.2in", y = "7.1in", w = "1in", h = "0.3in",
                 font_size = 12, color = "999999", align = "right" }

[[shapes]]
type = "rect"                       # rect | line | text | image | chart | placeholder
x = "0.8in"; y = "0.2in"; w = "11.7in"; h = "1in"
fill = { color = "C7000A" }

[[shapes]]
type = "placeholder"                # a box slides can fill
ph_type = "title"                   # title|body|pic|chart|tbl|media
name = "Title"                      # what slides reference (case-sensitive)
font_size = 26; color = "FFFFFF"; bold = true
```

## slides/<name>.toml

Slide-level keys (`master`, `background`, `hidden`, `notes`) must come BEFORE
the first `[[shapes]]` - in TOML, keys after a `[[shapes]]` header belong to
the last shape. `hidden` is accepted but not applied (pptxgenjs has no hidden
slides).

```toml
master = "brand"
background = "112233"
notes = "presenter notes"

[[shapes]]
type = "text"                       # text | image | a shape preset id
placeholder = "Title"               # fills the master placeholder (geometry inherited)
x = "1in"; y = "2.6in"; w = "11.3in"; h = "1in"
text = "*Welcome to* **gwen**"
font_size = 44

[[shapes]]
type = "image"
src = "media/pixel.png"             # relative to the deck root
```

A shape preset that carries `text` draws the shape with the text inside it
(fill/line/font options all apply); equivalently `shape = "rect"` on a text
shape. `chart`, `table` and `media` types are not supported yet.

## Supported shapes and options

A shape's `type` is `text`, `image`, a master `placeholder`, or any of these
preset ids (snake_case or camelCase both work):
`{{shape_types}}`
For example: `type = "rect"`, `type = "round_rect"`, `type = "star5"`,
`type = "flow_chart_process"`.

Options are snake_case and mirror pptxgenjs. Per context:
{{option_fields}}

All shapes also take `x` / `y` / `w` / `h` geometry.

## Rich text

`text` (and `[[shapes.paragraphs]]` text) is markdown:

| Markdown            | Result                                        |
|---------------------|-----------------------------------------------|
| `*italic*`          | italic run                                    |
| `**bold**`          | bold run                                      |
| `` `code` ``        | plain run                                     |
| `[text](url)`       | run with a hyperlink                          |
| single `\n`         | soft line break                               |
| blank line (`\n\n`) | new paragraph                                 |

`- item` / `1. item` markdown lists become bulleted paragraphs; an item's
indent level equals its nesting depth (top-level 0, nested = parent + 1).
Marker styles come from `[styles.*]` `ordered_markers` (patterns like `"1."`,
`"(1)"`, `"A."`) and `unordered_markers` (unicode runes, e.g. `"\u25BA"`),
indexed by indent level; missing levels fall back to the last marker, then
defaults.

## Explicit paragraphs

`[[shapes.paragraphs]]` gives per-paragraph options (`bullet`,
`indent_level`, `line_spacing`, `para_space_before`, ...). `align` is only
supported at the shape level.

## Coordinates and naming

All `x`/`y`/`w`/`h` are EMU as integers, or `"1in"`, `"2.5cm"`, `"25mm"`,
`"72pt"`, `"50%"` strings. Coordinate-valued options (`margin`, `sizing`'s
`w`/`h`/`x`/`y`, `tab_stops.position`, `points`) accept the same forms: length
strings (`"0.13cm"`) are converted to inches, `"50%"` passes through, and bare
numbers are inches (what pptxgenjs expects). A `margin` is a single value or a
`[top, right, bottom, left]` array. Shape preset and marker names are
snake_case in the DSL (`round_rect`, `flow_chart_process`, `arabicPeriod` is
`"1."`) and are canonicalised to pptxgenjs camelCase internally - including the
`shape` option, so `shape = "round_rect"` works.

## Validation

Unknown TOML fields or table names, unknown pptxgenjs option keys, unknown
`styles` references, and style cycles are build errors. Option **values** are
type-checked too: numbers (with their documented ranges), booleans, enum
strings, colors (6-hex or theme names), coordinates (number, `"1cm"` or
`"50%"`), and nested objects/arrays are validated. Errors report the exact
`path:line:col` of the offending key, so just fix what gwen points at.
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
