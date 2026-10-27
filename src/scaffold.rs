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
/// the directory name, written into `[presentation] title`. When `pptx` is
/// given, masters/slides/media (and main.toml) are imported from that file
/// instead of copied from the template/built-in; the skills and .gitignore are
/// still created as usual.
pub fn scaffold(root: &Path, name: &str, no_template: bool, pptx: Option<&Path>) -> Result<()> {
    create_dir(root)?;
    let template = (!no_template).then(template_dir).flatten();
    if let Some(pptx) = pptx {
        scaffold_project_skill(root, template.as_deref(), name)?;
        gwen::import_pptx(pptx, root, Some(name))?;
        write_gitignore(root)?;
        eprintln!(
            "created project `{}` from `{}`",
            root.display(),
            pptx.display()
        );
        eprintln!(
            "  edit {}/main.toml and slides/*.toml, then run `gwen build {}`",
            root.display(),
            root.display()
        );
        return Ok(());
    }
    if let Some(template) = template {
        scaffold_from_template(root, &template, name)?;
        write_gitignore(root)?;
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
    scaffold_builtin(root, name)?;
    write_gitignore(root)?;
    Ok(())
}

/// Write (or append to) the project `.gitignore` so `target/` is ignored.
/// Returns true when the file changed.
pub(crate) fn write_gitignore(root: &Path) -> Result<bool> {
    let path = root.join(".gitignore");
    if path.exists() {
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| miette::miette!("cannot read `{}`: {e}", path.display()))?;
        if contents.lines().any(|l| l.trim() == "target/") {
            return Ok(false);
        }
        let mut out = contents;
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("target/\n");
        std::fs::write(&path, out)
            .map_err(|e| miette::miette!("cannot write `{}`: {e}", path.display()))?;
        Ok(true)
    } else {
        std::fs::write(&path, "target/\n")
            .map_err(|e| miette::miette!("cannot write `{}`: {e}", path.display()))?;
        Ok(true)
    }
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
    for (rel, contents) in gwen_skill_files() {
        write_file(&root.join(rel), &contents)?;
    }
    Ok(())
}

/// The generated project skill, as `(path relative to the project, contents)`.
/// Kept separate so `gwen update` can refresh the same two files from the
/// binary (the DSL spec may be newer than the installed template).
pub(crate) fn gwen_skill_files() -> [(&'static str, String); 2] {
    let spec = DEFAULT_DSL_SPEC
        .replace("{{shape_types}}", &shape_types())
        .replace("{{option_fields}}", &option_fields());
    [
        (".agents/skills/gwen/SKILL.md", DEFAULT_SKILL_MD.to_string()),
        (".agents/skills/gwen/references/toml-dsl.md", spec),
    ]
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
            "- **Nested objects**: {}\n- **Gwen-only keys** (never sent to pptxgenjs): `styles`, `ordered_markers`, `unordered_markers`, `bullet_indent`",
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
# [styles.text] only applies to `type = "text"`; shared text options belong in
# [styles.shape]. Master placeholders use [styles.placeholder].
# [styles.shape]
# bullet_indent = 10
# fill = { color = "C7000A" }
# [styles.text]
# font_face = "Arial"
# font_size = 18
# color = "262626"
# [styles.placeholder]
# line = { type = "none" }
# [styles.image]
# sizing = { type = "contain" }

# Named styles; a shape referencing `styles = "muted"` merges these.
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

It emits standard PowerPoint, with the same OOXML parts
[pptxgenjs](https://gitbrent.github.io/PptxGenJS/) produces.
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
TOML, never the `.pptx`. TOML is the single source of truth. `gwen render`
paints `target/render/<slide-stem>.png` directly from the same TOML, without an
office suite, so you can inspect a slide before building. `gwen clean` removes
the generated `target/` directory.

## When to use
Editing or building slides, masters, styles or any `.toml` file in this
project.

## Quick rules
- `[[sections]]` in main.toml is required and is the slide index
- styles precedence: `[styles.shape] < [styles.<type>] < shape styles < shape keys`; `[styles.text]` applies only to `type = "text"` and `[styles.placeholder]` to master placeholders; shared text options belong in `[styles.shape]`; `styles` accepts a string or a list and may be recursive (cycles are errors)
- shape types and marker names are snake_case (`round_rect`); option keys are
  snake_case (`font_size`)
- text is markdown: `**bold**`, `*italic*`, `[link](url)`, a single `\n` is a
  soft break, a blank line is a new paragraph, `- item` / `1. item` are
  bulleted lists
- a line that starts with `-`, `+`, `*`, `>`, or `1.` is a markdown list or
  quote marker; escape the leading character (`\- item`, `\> note`) to show
  it literally
- coordinates: EMU integers or `"1in"` / `"2.5cm"` / `"25mm"` / `"72pt"` /
  `"50%"`
- gwen validates strictly: unknown fields/options/`styles` references/cycles
  and wrong option value types are loud errors - read the diagnostics
- to look at a slide, render it: `gwen render --page 1,3-5 --format png` writes
  `target/render/<slide-stem>.png`; features that are still approximated are
  reported on stderr
- `gwen update .` refreshes template-owned files (`masters/`, `media/`,
  `.agents/`) and merges the template's `[theme]` / `[styles.*]` into
  main.toml; slides, sections and the title are never touched
- `gwen fmt .` normalizes TOML whitespace in place (comments, key order and
  multi-line strings are preserved); `gwen fmt . --check` only reports

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
TOML, never the `.pptx`. TOML is the single source of truth. `gwen render`
paints `target/render/<slide-stem>.png` directly from the same TOML, without an
office suite, so you can inspect a slide before building. `gwen clean` removes
the generated `target/` directory.

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
[styles.shape] < [styles.<type>] < shape's own `styles` < shape keys
```

- `[styles.shape]` is the base for every shape; `[styles.<type>]` layers per
  type id (`text`, `image`, `placeholder`, `rect`, `ellipse`, ...).
  `[styles.text]` is an ordinary type bucket: it applies only to
  `type = "text"`. Text options shared by every shape (font, size, align,
  margins) belong in `[styles.shape]`; master placeholders use
  `[styles.placeholder]`.
- A bucket may hold keys that only some shape types can apply: they take effect
  where valid and are dropped elsewhere (an `align` in `[styles.shape]` applies
  to text shapes but is ignored by image shapes, not an error). Buckets are
  still validated against the union of all options, so typos never pass.
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
# Add `locked = true` to render the auto-updating number on the slides as a
# locked, non-placeholder box (PowerPoint ignores shape locks on placeholders);
# the master's layout stays editable in Slide Master view. `slide_number` is
# per-master: each master's style applies to its own slides, and `locked` locks
# only that master's rendered slides.

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
the last shape. `master` is required and must name a `masters/<name>.toml`
file. `hidden` is accepted but not applied (pptxgenjs has no hidden
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

`src` accepts `png`/`jpg`/`jpeg`/`gif`/`bmp`/`tiff` and `svg`. An SVG is embedded
as a real vector picture (PowerPoint 2016+ renders it natively via
`<asvg:svgBlip>`) **and** as a PNG preview rasterized by gwen (`resvg`) at the
shape's display size, so older PowerPoint still shows the image.

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

`text` (and `[[shapes.paragraphs]]` text) is markdown - for slides **and**
masters/placeholders alike (master and placeholder text get the same run
treatment as slide text):

| Markdown            | Result                                        |
|---------------------|-----------------------------------------------|
| `*italic*`          | italic run                                    |
| `**bold**`          | bold run                                      |
| `` `code` ``        | plain run                                     |
| `[text](url)`       | run with a hyperlink                          |
| `\n` / `<br>`      | line break                                     |
| `\n\n`             | new paragraph                                 |

`<br>` is a hard line break: each one adds a line, so `a<br><br>b` renders a
blank line between `a` and `b`. Soft breaks (`\n`) are collapsible - they
guarantee a line break but don't stack, so `a\n<br>\nb` is one line break.

`- item` / `1. item` markdown lists become bulleted paragraphs; an item's
indent level equals its nesting depth (top-level 0, nested = parent + 1).
A line that starts with a markdown marker (`-`, `+`, `*`, `>`, or `1.`) is
parsed as a list or quote marker. Escape the leading character with a
backslash (`\- item`, `\+ item`, `\> note`) to show it literally.
Marker styles come from `[styles.*]` `ordered_markers` (patterns like `"1."`,
`"(1)"`, `"A."`) and `unordered_markers` (unicode runes, e.g. `"\u25BA"`),
indexed by indent level; missing levels fall back to the last marker, then
defaults. The gap between a marker and the item text is `bullet_indent`
(points, default 27) on a shape, style bucket or paragraph; a paragraph's
`bullet = { indent = ... }` overrides it.

A `<span class="name">` applies a `[styles.named.name]` to the runs it wraps:
`black with <span class="red">**bold and red**</span>` makes the inner run
bold and red. The span is transparent to markdown, so both
`<span class="red">**bold**</span>` and `**<span class="red">bold</span>**`
work; `class="a b"` merges styles in order (later wins). A span style may
only use run options (`color`, `highlight`, `font_face`, `font_size`, `bold`,
`italic`, `underline`, `strike`, `superscript`, `subscript`, `char_spacing`,
`baseline`, `lang`, `rtl_mode`); other keys, an unknown class, or unbalanced
tags are build errors. Use `\<span` to write a literal angle bracket.

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
`[top, right, bottom, left]` array. An `image` shape may give only `w` or only
`h` (when no `sizing` option is set): the missing dimension is derived from the
source image's intrinsic aspect ratio (PNG/JPEG/GIF/BMP headers, or the SVG
`viewBox`/`width`/`height`), so `w = "4in"` on a 2:1 image renders `4in x 2in`.
Shape preset and marker names are
snake_case in the DSL (`round_rect`, `flow_chart_process`, `arabicPeriod` is
`"1."`) and are canonicalised to pptxgenjs camelCase internally - including the
`shape` option, so `shape = "round_rect"` works.

## Preview

`gwen render [project] [--page N[,N-R]] [--format png|svg|png,svg]` paints the
same spec directly to `<deck>/target/render/<slide-stem>.{png,svg}`, named
after the source TOML (`slides/01-trend.toml` becomes `01-trend.png`). PNG is
the default and is rasterized at 2x; SVG is only written when requested. Page
numbers are 1-based and accept ranges (`--page 1,3-5`). Fonts come from
`<deck>/fonts`, then `$GWEN_HOME/fonts`, then the system. Any feature the
renderer still approximates is printed to stderr.

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

    #[test]
    fn skill_docs_explain_markdown_escaping_and_required_masters() {
        assert!(DEFAULT_SKILL_MD.contains("escape the leading character"));
        assert!(DEFAULT_DSL_SPEC.contains("Escape the leading character"));
        assert!(DEFAULT_DSL_SPEC.contains("`master` is required"));
    }
}
