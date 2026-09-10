# The gwen project format

gwen turns a Markdown project into a clean PowerPoint deck. The project is the
single source of truth; `build` never reads the original `.pptx`.

```
deck/
  config.toml            presentation metadata, theme fonts, and layouts
  src/
    SUMMARY.md           the index: one bullet per slide, in build order
    slides/*.md          one slide per file
    media/               images referenced by slides
```

Run `gwen new <deck>` to scaffold one, then `gwen build <deck>`. The deck is
written to `<deck>/target/<name>.pptx`. The package is structurally validated
before it is written; on any violation nothing is written and the problems are
reported.

## `SUMMARY.md`

Like mdbook: only top-level bullets count, in document order. Headings, prose
and nested bullets are ignored.

```markdown
# Summary

- [Title](slides/title.md)
- [Intro](slides/intro.md)
```

A listed file that does not exist is an error.

## `config.toml`

```toml
[presentation]
name = "Deck"            # output file stem and core.xml title
slide_width = 12192000   # EMU
slide_height = 6858000
default_layout = "content"

[theme]
major_font = "Arial Black"
minor_font = "Arial"
```

### Layouts

A layout is an ordered list of elements. An element is either a **static
shape** — drawn on every slide that uses the layout — or a **content slot**,
which slide Markdown binds to.

```toml
[[layouts.content.elements]]
kind = "shape"           # static decoration
shape = "round_rect"     # rect | round_rect | ellipse | line
left = 0
top = 0
width = 12192000
height = 914400
fill = "#C7000A"         # #RRGGBB
# no_fill = true         # outline only

[[layouts.content.elements]]
kind = "slot"
slot = "title"           # bound by `# Heading`
type = "text"            # text | picture
left = 914400
top = 685800
width = 10363200
height = 914400
align = "left"           # left | center | right | justify
anchor = "middle"        # top | middle | bottom
text_size = 32           # points
color = "#1D1D1A"
bold = true
italic = false
wrap = true
fill = "#FFFFFF"         # optional shape fill for a text slot
```

The four slot names the Markdown grammar binds to are `title`, `subtitle`,
`body` and `picture`. A block whose slot the layout lacks is an error.

## Slide Markdown

```markdown
---
layout: content          # required; a key under [layouts.*]
background: "#112233"    # optional solid background
---

# Slide title

A paragraph.

- bullet one
- bullet two
  - nested bullet

![logo](media/logo.png)  # one picture per slide

## Notes

Speaker notes for this slide.
```

- `# Heading` binds to the `title` slot, `## Subtitle` to `subtitle`.
- Paragraphs and `- ` bullets bind to `body` (indent = bullet level).
- `![alt](media/file.png)` binds to `picture`.
- A `## Notes` heading switches the rest of the file into the notes slide.

## Colors

Colors are plain `#RRGGBB` everywhere. There is no palette: every fill, text
color and background is written directly into the deck as an `srgbClr`. The
package still contains the standard theme part required by the format, but
nothing references its scheme colors.

## Charts and tables

Not yet supported. The layout/slot model is designed so `chart` and `table`
slot types can be added without restructuring.
