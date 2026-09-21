# The gwen project format

gwen turns a TOML project into a PowerPoint deck. The project is the single
source of truth; `build` never reads the original `.pptx`.

```
deck/
  main.toml            presentation metadata, layout, theme, sections, defaults, styles
  masters/*.toml       one slide master per file (the file stem is the master name)
  slides/*.toml        one slide per file
  media/               images referenced by slides and masters
```

Run `gwen new <deck>` to scaffold one, then `gwen build <deck>`. The deck is
written to `<deck>/target/<title>.pptx`. gwen translates the project into a
pptxgenjs spec and renders it with the real `pptxgenjs` bundle running on an
embedded QuickJS runtime (rquickjs); the parts pptxgenjs produces are packaged
into the `.pptx` container by gwen itself. gwen only ever calls pptxgenjs's
public API — it never writes OOXML. A `node` runtime is not required.

## `main.toml`

```toml
[presentation]      # optional (all defaults empty/""/false)
title = "Deck"      # output file stem and core.xml title
author = ""
company = ""
revision = ""
subject = ""
rtl_mode = false

[layout]            # optional, defaults 10in x 7.5in, layout name "GWEN"
name = "GWEN"
width = "13.333in"  # EMU int or "1in"/"2.5cm"/"25mm"/"72pt"/"50%"
height = "7.5in"

[theme]             # optional, defaults Calibri
major_font = "Arial Black"
minor_font = "Arial"

[[sections]]        # optional; the slide ordering index
title = "Intro"
slides = ["title.toml", "content.toml"]   # paths relative to slides/

[defaults.text]     # built-in defaults merged into every text shape
font_face = "Arial"
color = "262626"
[defaults.shape]    # every non-text shape
fill = { color = "C7000A" }
[defaults.image]
sizing = { type = "contain" }

[styles.muted]      # named styles; a shape with style = "muted" merges these
color = "808080"
italic = true
```

Option keys are snake_case and map to the pptxgenjs camelCase properties
(`font_size` -> `fontSize`, `line_spacing` -> `lineSpacing`, ...); any key
pptxgenjs accepts can be set, and unknown keys pass straight through.

Precedence when building a shape's options:
`[defaults.<type>]` < `[styles.<name>]` < the shape's own keys.

If `[[sections]]` is empty, every `slides/*.toml` is used in alphabetical
order. A slide listed in more than one section is an error; a slide not listed
at all is skipped with a warning.

## `masters/<name>.toml`

The master name is the file stem — there is no `title` field.

```toml
background = { color = "FFFFFF" }   # or a color shorthand string
margin = 0.5                        # inches (EMU/unit strings/arrays also OK)

[[objects]]
type = "rect"       # rect | line | text | image | placeholder
x = 0
y = 0
w = "13.333in"
h = "1.167in"
fill = { color = "C7000A" }
line = { color = "C7000A", width = 0 }

[[objects]]
type = "text"
x = "0.8in"
y = "0.25in"
w = "11.7in"
h = "0.667in"
text = "Acme Inc."
font_size = 14
color = "FFFFFF"
bold = true
```

## `slides/<name>.toml`

```toml
master = "title_base"               # optional master name
background = "112233"               # color string or background object
hidden = false                      # accepted; not applied (pptxgenjs has no hidden slides)

[[shapes]]
type = "text"                       # text | image | a pptxgenjs shape preset (rect, ...)
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

notes = "presenter notes"
```

`chart`, `table` and `media` shape types are recognised but not supported yet.

## Rich text

`text` (and each `[[shapes.paragraphs]]` entry) is parsed by `pulldown-cmark`:

| Markdown            | Result                                          |
|---------------------|-------------------------------------------------|
| `*italic*`          | italic run                                      |
| `**bold**`          | bold run                                        |
| `` `code` ``        | plain run (v1)                                  |
| `[text](url)`       | run with a hyperlink                            |
| single `\n`         | soft line break                                 |
| blank line (`\n\n`) | new paragraph                                   |

A single `\n` becomes a soft line break (pptxgenjs `softBreakBefore`); a blank
line starts a new paragraph. `  \n` (a hard break) behaves like a soft break.

## Explicit paragraphs

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
  level = 1
```

Paragraph options (`bullet`, `level`, `line_spacing`, `para_space_before`,
...) are applied to the paragraph's first run. `align` is not supported at the
paragraph level in v1 — pptxgenjs auto-splits paragraphs on `align` changes,
which clashes with explicit paragraph boundaries; use the shape-level `align`
instead.

## Coordinates

All `x`/`y`/`w`/`h` values are English Metric Units (EMU) when written as
plain integers. Unit-suffixed strings and percentages are also accepted:

- `"1in"`, `"2.5cm"`, `"25mm"`, `"72pt"`
- `"50%"` — relative to the slide width (`x`/`w`) or height (`y`/`h`)

## Rendering

The spec is a plain JSON document — it is `JSON.parse`d and passed to
pptxgenjs objects; nothing from the project is ever evaluated as code.
