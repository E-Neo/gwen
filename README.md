# gwen

Generate clean PowerPoint decks (`.pptx`) from TOML.

## How it works

A deck is a directory of TOML files - `main.toml`, `masters/*.toml` and
`slides/*.toml` - that is the single source of truth. `gwen build` compiles it
into a JSON spec and renders that spec with gwen's **native PPTX writer**. The
writer is a direct Rust port of pptxgenjs 4.0.1's OOXML emitters, so existing
decks keep producing the same `.pptx` parts - while gwen stays self-contained
and needs neither Node nor an embedded JavaScript runtime.

## Quick start

```sh
gwen new <deck>          # scaffold a project
gwen new <deck> --pptx a.pptx   # import masters/slides/media from an existing deck
gwen update <deck>       # sync an existing project with the installed template
gwen fmt <deck>          # normalize the project's TOML whitespace
gwen fmt <deck> --check  # report only; exits 1 if a file would change
gwen build <deck>        # render <deck>/target/<title>.pptx
gwen render <deck>       # PNG previews in <deck>/target/render/ (no office suite)
gwen clean <deck>        # remove the generated <deck>/target/ directory
```

`gwen clean` refuses to run unless `<deck>/main.toml` exists, so it cannot
delete an unrelated `target/`.

`gwen new` uses a template from gwen's home directory when one exists: `$GWEN_HOME`
if set, otherwise `~/.gwen`. If `<home>/template/` is present, the new project
is copied from it - `template/main.toml` (required, with its
`[presentation] title` overwritten by the `<deck>` directory name) plus
`template/masters/`, `template/slides/` and `template/media/` (copied
recursively when present). Without a template, a small built-in starter deck is
used. Pass `--no-template` to force the built-in scaffold even when a template
is installed.

`gwen update <deck>` brings an existing project up to date with that template:
`masters/`, `media/` and the template's skills are refreshed, and the template's
`[theme]` / `[styles.*]` are merged into `main.toml` key by key - comments and
project-only keys survive. `slides/`, `[[sections]]` and the `[presentation]`
table are never touched, and files the template no longer ships are reported as
stale rather than deleted. Overwritten files are backed up under
`target/update-backup/`; `--dry-run` reports without writing anything and
`--keep-styles` skips the `main.toml` merge.

`gwen fmt <deck>` tidies the whitespace in every `*.toml` under the project -
`main.toml`, `masters/`, `slides/` and `.agents/`, skipping `target/`. It puts
one space around `=`, canonicalizes inline tables and single-line arrays, keeps
multi-line arrays one element per line with a trailing comma, collapses runs of
blank lines to one (and puts exactly one before each table header), strips
trailing whitespace and ends the file with a single newline. Comments, key
order and `[[shapes]]` / `[[sections]]` order never change, and multi-line
strings are left byte-for-byte alone. `--check` writes nothing and exits 1 when
a file would change, so it works as a CI gate.

## Previewing slides

`gwen render [project] [--page N[,N-R]] [--format png|svg|png,svg]` paints the
deck straight from the TOML spec, without LibreOffice, PowerPoint or any other
office suite. Outputs are named after the source slide TOML:

```
<deck>/target/render/01-trend.png   # default format, from slides/01-trend.toml
<deck>/target/render/01-trend.svg   # only with --format svg or png,svg
```

PNG is the default and is the artifact to inspect visually; it is rasterized
at 2x for readable text. SVG is never written unless requested. `--page`
accepts 1-based numbers and ranges (`1`, `3-5`), and stale rendered images
from an earlier run are removed so the directory always reflects the current
invocation.

Fonts are loaded from `<deck>/fonts`, then `$GWEN_HOME/fonts`, then the system.
An unknown family falls back to an available sans-serif face instead of
dropping text. Features the renderer still approximates (for example inner
shadows, mixed-direction span order, or an unknown shape preset) are reported
per slide on stderr.

Every new project also gets a project-level skill at
`.agents/skills/gwen/` - an agent- and human-readable reference to the gwen
TOML DSL (`SKILL.md` overview + `references/toml-dsl.md` full specification).
A template may ship extra skills under its own `.agents/skills/`, copied
verbatim; the `gwen` skill name itself is reserved. Every project is also
given a `.gitignore` that ignores `target/`.

## Importing a `.pptx`

`gwen new <deck> --pptx deck.pptx` creates a project whose content comes from
an existing PowerPoint package instead of the template/built-in scaffold: the
template's skills are still copied, but its `masters/`, `slides/`, `media/`
and `main.toml` are replaced by the imported deck. It recovers the slide size,
theme fonts, `[[sections]]` order, one master file per slide layout, shapes
(geometry, rotation, fills, lines, shadows), text as markdown (`**bold**`,
`*italic*`, `[links](url)`, line breaks, bullets), images (copied into
`media/`), hyperlinks and notes. Groups are flattened recursively; charts,
tables, gradients and per-run font/colour differences are dropped with a
warning, and the result is a starting point to edit, not a byte-identical
copy - `gwen build` of the imported project produces a fresh, valid deck.

## The gwen project format

```
deck/
  main.toml            presentation metadata, theme, sections, styles
  masters/*.toml       one slide master per file (the file stem is the master name)
  slides/*.toml        one slide per file
  media/               images referenced by slides and masters
```

### `main.toml`

```toml
[presentation]      # most fields optional (defaults ""/false)
title = "Deck"      # output file stem (gwen new writes the directory name)
author = ""
company = ""
revision = ""
subject = ""
rtl_mode = false
width = 12196763    # slide size: EMU int or "1in"/"2.5cm"/"25mm"/"72pt"/"50%"
height = 6858000    # default width/height = 13.3386in x 7.5in

[theme]             # optional, defaults Calibri
major_font = "Arial Black"    # inherited by placeholder/heading text
minor_font = "Arial"          # inherited by ordinary text without font_face

[[sections]]        # required; the slide ordering index
title = "Intro"
slides = ["title.toml", "content.toml"]   # paths relative to slides/

[styles.shape]       # base style for EVERY shape (text, image, presets)
fill = { color = "C7000A" }

[styles.text]        # type bucket for `type = "text"` only
font_face = "Arial"
color = "262626"

[styles.placeholder] # type bucket for master placeholders
line = { type = "none" }

[styles.image]       # layered over [styles.shape] for images
sizing = { type = "contain" }

[styles.ellipse]     # ...or any shape preset id
line = { color = "333333", width = 0.5 }

[styles.named.muted] # named style; shapes opt in via styles = "muted"
color = "808080"
italic = true
```

Option keys are snake_case and map to the pptxgenjs camelCase properties
(`font_size` -> `fontSize`, `line_spacing` -> `lineSpacing`, ...); any key
pptxgenjs accepts can be set.

Precedence when building a shape's options:
`[styles.shape]` < `[styles.<type>]` < the shape's `styles` < the shape's own
keys. `[styles.text]` is an ordinary type bucket: it applies to `type = "text"`
only. Text options shared by every shape (`font_face`, `font_size`, `align`,
`margin`, ...) belong in `[styles.shape]`, which is validated against the union
of shape and text options and has inapplicable keys dropped per shape. Shape
presets (`[styles.rect]`, `[styles.ellipse]`, ...) may also carry text options
and override the shape bucket for that preset. Master placeholders use
`[styles.placeholder]`. A `styles` reference to a `[styles.named.<name>]` block
that doesn't exist is an error, as is a reference cycle.

A style bucket may hold keys that only some shape types can apply - they take
effect where valid and are ignored elsewhere. For example, an `align` in
`[styles.shape]` applies to text shapes but is dropped (not an error) for image
shapes. Style buckets are still validated strictly against the union of all
options, so typos are never silently accepted.

### Style composition

Named styles compose via a `styles` key on shapes and inside any bucket. The
value is a scalar or a list, applied in order (later wins); within a container
the container's own inline options win over what it includes.

```toml
[styles.named.brand]
fill = { color = "C7000A" }

[styles.named.dark]
styles = ["brand"]        # = brand, then dark's own keys override
color = "111111"

[styles.rect]
styles = "brand"          # every rect also gets the "brand" named style

[[shapes]]
type = "text"
styles = ["brand", "dark"]
```

`[[sections]]` is required - building without it is an error. A slide listed
in more than one section is an error; a slide not listed at all is not
rendered.

### `masters/<name>.toml`

The master name is the file stem - there is no `title` field. Elements are
`[[shapes]]`, exactly like slides.

```toml
background = { color = "FFFFFF" }   # or a color shorthand string
margin = 0.5                        # inches (EMU/unit strings/arrays also OK)

# Slide number: put a page-number box in the corner of every slide.
slide_number = { x = "12.2in", y = "7.1in", w = "1in", h = "0.3in",
                 font_size = 12, color = "999999", align = "right" }

# Add `locked = true` to render the auto-updating number on the slides as a
# locked, non-placeholder box (PowerPoint ignores shape locks on placeholders):
# it can't be selected, moved, resized, or have its text edited, and it still
# renumbers per slide. The master's layout stays editable in Slide Master view.
# slide_number = { ..., locked = true }
#
# `slide_number` is per-master: each master's own style (position, font,
# colour, ...) applies to the slides that use that master, and `locked` locks
# only that master's rendered slides.

[[shapes]]
type = "rect"       # rect | line | text | image | chart | placeholder
x = 0
y = 0
w = "100%"
h = "1.1in"
fill = { color = "C7000A" }
line = { color = "C7000A", width = 0 }

# A placeholder the slide content can fill.
[[shapes]]
type = "placeholder"
ph_type = "title"   # p:ph type: title|body|pic|chart|tbl|media
name = "Title"      # what slides reference
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
```

Placeholders stay inline in `[[shapes]]`, so their z-order is exactly what you
write. A slide fills one by setting `placeholder = "<name>"` on a text shape
(its geometry is inherited from the master). Referencing a placeholder the
master doesn't define is an error.

### `slides/<name>.toml`

```toml
master = "brand"                    # required master name
background = "112233"               # color string or background object
hidden = false                      # accepted; not applied (pptxgenjs has no hidden slides)
notes = "presenter notes"

# slide-level keys (master/background/hidden/notes) must come BEFORE the
# first [[shapes]] - in TOML, keys after a [[shapes]] header belong to the
# last shape, not the slide.

[[shapes]]
type = "text"                       # text | image | a pptxgenjs shape preset (rect, ...)
placeholder = "Title"               # fills a master placeholder (geometry inherited)
x = "1in"
y = "2.6in"
w = "11.3in"
h = "1in"
text = "*Welcome to* **gwen**"      # markdown inline text
align = "center"
font_size = 44

[[shapes]]
type = "rect"
x = "8.5in"
y = "2in"
w = "3in"
h = "1in"
fill = { color = "C7000A" }

[[shapes]]
type = "image"
x = "0.8in"
y = "5.5in"
w = "1in"
h = "1in"
src = "media/pixel.png"             # relative to the deck root
```

`src` accepts `png`/`jpg`/`jpeg`/`gif`/`bmp`/`tiff` and `svg`. An SVG is embedded as a
real vector picture (PowerPoint 2016+ renders it natively) *and* as a rasterized
PNG preview (rendered by gwen with `resvg` at the shape's display size, so older
PowerPoint still shows the image).

`chart`, `table` and `media` shape types are recognised but not supported yet.

A shape preset that carries `text` draws the shape **with** the text inside it
(markdown works, and the shape's `fill`/`line`/font options all apply):

```toml
[[shapes]]
type = "rect"
x = "1cm"
y = "1cm"
w = "4cm"
h = "1cm"
fill = { color = "C7000A" }
color = "FFFFFF"
align = "center"
valign = "middle"
text = "**OK**"
```

Equivalently, a text shape can draw a preset explicitly with `shape = "rect"`.

### Rich text

`text` (and each `[[shapes.paragraphs]]` entry) is parsed by `pulldown-cmark` -
for slides **and** masters/placeholders alike:

| Markdown            | Result                                          |
|---------------------|-------------------------------------------------|
| `*italic*`          | italic run                                      |
| `**bold**`          | bold run                                        |
| `` `code` ``        | plain run                                     |
| `[text](url)`       | run with a hyperlink                            |
| `\n` / `<br>`      | line break                                     |
| `\n\n`             | new paragraph                                   |

A `\n` or `<br>` is a line break (pptxgenjs `softBreakBefore`); a blank
line starts a new paragraph. Hard breaks stack: `a<br><br>b` renders a blank
line between `a` and `b`, while `a\n<br>\nb` collapses to one line break
(soft breaks are collapsible, `<br>`s are not).

A `<span class="name">` applies a `[styles.named.name]` to the runs it wraps -
e.g. `black with <span class="red">**bold and red**</span>` colours that run
red *and* bolds it. The span is transparent to markdown, so
`**<span class="red">bold</span>**` works too; `class="a b"` merges styles in
order (later wins). A span style may only set run options (`color`,
`highlight`, `font_face`, `font_size`, `bold`, `italic`, `underline`,
`strike`, `superscript`, `subscript`, `char_spacing`, `baseline`, `lang`,
`rtl_mode`); anything else, an unknown class, or unbalanced `<span>`/`</span>`
tags are build errors pointing at the offending line. Use `\<span` to write a
literal angle bracket.

### Explicit paragraphs

For per-paragraph options use `[[shapes.paragraphs]]`; `text` is then ignored.

```toml
[[shapes]]
type = "text"
x = "0.8in"
y = "1.5in"
w = "7in"
h = "4in"
font_size = 20

[[shapes.paragraphs]]
text = "**first** bullet point"
bullet = true

[[shapes.paragraphs]]
text = "second point"
bullet = true
indent_level = 1
```

Paragraph options (`bullet`, `indent_level`, `line_spacing`,
`para_space_before`, ...) are applied to the paragraph's first run. `align` is
not supported at the paragraph level - pptxgenjs auto-splits paragraphs
on `align` changes, which clashes with explicit paragraph boundaries; use the
shape-level `align` instead.

### Lists

Markdown lists in `text` become bulleted paragraphs with the correct indent
level (top-level = 0, nested = parent + 1). A line starting with `-`, `+`,
`*`, `>`, or `1.` is treated as a list or quote marker; escape the leading
character (`\- item`, `\> note`) to show it literally. The marker style per
indent level is configured in style buckets:

```toml
[styles.text]
ordered_markers = ["1.", "A.", "i)"]     # auto-number patterns per level
unordered_markers = ["\u25BA", "\u2022"] # bullet runes per level
```

These are gwen-owned options and follow the same buckets as any other style
key. Put them in `[styles.shape]` if lists can also appear on preset shapes;
`[styles.text]` only applies to `type = "text"`.

- `ordered_markers` entries are display patterns (`"1"`, `"1."`, `"1)"`,
  `"(1)"`, `"a."`, `"A."`, `"a)"`, `"A)"`, `"(a)"`, `"(A)"`, `"i."`, `"I."`,
  `"i)"`, `"I)"`, `"(i)"`, `"(I)"`) mapped to pptxgenjs `buAutoNum` types;
  default `"1."`.
- `unordered_markers` entries are the actual bullet characters; gwen converts
  them to character codes internally. Default: the standard bullet.
- A level with no entry falls back to the last configured marker, else the
  default. These keys are gwen-owned (never passed to pptxgenjs).

Shape preset names are snake_case in the DSL - `round_rect`, `flow_chart_process`,
`action_button_back_previous` - and are canonicalised to pptxgenjs's camelCase
(`roundRect`, ...) internally; the camelCase forms still work too.

### Coordinates

All `x`/`y`/`w`/`h` values are English Metric Units (EMU) when written as
plain integers. Unit-suffixed strings and percentages are also accepted:

- `"1in"`, `"2.5cm"`, `"25mm"`, `"72pt"`
- `"50%"` - relative to the slide width (`x`/`w`) or height (`y`/`h`)

### Validation

Unknown TOML fields (a misspelled table or key such as `[[shaps]]`) and unknown
pptxgenjs option keys (`fount_size`, a stray key inside `fill`, an unknown
`styles = "<name>"`) are build errors, so typos fail loudly instead of being
silently ignored. Option **values** are validated too: numbers (with their
documented ranges, e.g. `rect_radius` 0-1, `transparency` 0-100), booleans,
enum strings (`align`, `fit`, `sizing.type`, ...), colors (6-hex or theme
names), coordinates (EMU integer, `"1cm"`, or `"50%"`), and nested
objects/arrays.

## Rendering

`gwen build` turns the project into a JSON spec string (`render.rs`) and hands
that plain **data** object to the native writer in `src/pptx/`. The writer
parses the spec, builds the same slide/master/layout/notes XML pptxgenjs would,
and packages the parts with gwen's deterministic ZIP writer. Nothing from your
TOML is ever evaluated as code.
