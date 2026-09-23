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
major_font = "Arial Black"
minor_font = "Arial"

[[sections]]        # required; the slide ordering index
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

`[[sections]]` is required — building without it is an error. A slide listed
in more than one section is an error; a slide not listed at all is not
rendered.

## `masters/<name>.toml`

The master name is the file stem — there is no `title` field. Elements are
`[[shapes]]`, exactly like slides.

```toml
background = { color = "FFFFFF" }   # or a color shorthand string
margin = 0.5                        # inches (EMU/unit strings/arrays also OK)

# Slide number: put a page-number box in the corner of every slide.
slide_number = { x = "12.2in", y = "7.1in", w = "1in", h = "0.3in",
                 font_size = 12, color = "999999", align = "right" }

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

## `slides/<name>.toml`

```toml
master = "brand"                    # optional master name
background = "112233"               # color string or background object
hidden = false                      # accepted; not applied (pptxgenjs has no hidden slides)
notes = "presenter notes"

# slide-level keys (master/background/hidden/notes) must come BEFORE the
# first [[shapes]] — in TOML, keys after a [[shapes]] header belong to the
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

Equivalently, a text shape can draw a preset explicitly with
`shape = "rect"`.

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
