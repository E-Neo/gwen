//! Translate a TOML deck (load `main.toml`, masters and slides, resolve
//! defaults/styles/options, parse rich text, normalise everything to the
//! units pptxgenjs expects) into the JSON spec the embedded bridge renders.

use std::collections::BTreeSet;

use miette::{Result, miette};
use serde_json::{Map, Value, json};

use crate::model::{Kind, Main, Master, Project, Section, Shape, SlideNumber};
use crate::opts::{self, Ctx};
use crate::richtext;
use crate::units::{Coord, EMU_PER_IN};
use base64::Engine;

/// Build the JSON spec string passed to the pptxgenjs bridge.
///
/// If `root` is `None` a fresh in-memory slidedeck is built; otherwise it is
/// written to the given root.
pub fn spec_json(project: &Project) -> Result<String> {
    let main_ = &project.main;
    let width = main_
        .presentation
        .width
        .emu(0)
        .map_err(|e| miette!("[presentation].width: {e}"))?;
    let height = main_
        .presentation
        .height
        .emu(0)
        .map_err(|e| miette!("[presentation].height: {e}"))?;

    validate_styles(main_)?;

    let masters = load_masters(project, width, height, main_)?;
    let sections = resolve_sections(project)?;
    let slides = build_sections(project, &sections, main_, width, height)?;

    let out = json!({
        "width": width as f64 / EMU_PER_IN,
        "height": height as f64 / EMU_PER_IN,
        "majorFont": main_.theme.major_font,
        "minorFont": main_.theme.minor_font,
        "masters": masters,
        "sections": slides,
    });
    serde_json::to_string(&out).map_err(|e| miette!("cannot serialise spec: {e}"))
}

/// Validate the unified `[styles.*]` tables once, up front.
fn validate_styles(main: &Main) -> Result<()> {
    for (ty, table) in &main.styles.by_type {
        let ctx = opts::ctx_for_style_type(ty)
            .ok_or_else(|| miette!("unknown style type `[styles.{ty}]` (not a shape type)"))?;
        opts::validate_ctx(
            ctx,
            &toml::Value::Table(table.clone()),
            &format!("[styles.{ty}]"),
        )?;
    }
    for (name, table) in &main.styles.named {
        opts::validate_ctx(
            Ctx::Style,
            &toml::Value::Table(table.clone()),
            &format!("[styles.named.{name}]"),
        )?;
    }
    Ok(())
}

fn load_masters(project: &Project, width: i64, height: i64, main: &Main) -> Result<Vec<Value>> {
    let dir = project.dir.join("masters");
    let mut files: Vec<_> = if dir.is_dir() {
        std::fs::read_dir(&dir)
            .map_err(|e| miette!("cannot list `{}`: {e}", dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().map(|t| t.is_file()).unwrap_or(false)
                    && e.file_name().to_string_lossy().ends_with(".toml")
            })
            .collect()
    } else {
        Vec::new()
    };
    files.sort_by_key(|e| e.file_name());

    let mut out = Vec::new();
    for entry in files {
        let stem = entry
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let raw = std::fs::read_to_string(entry.path())
            .map_err(|e| miette!("cannot read `{}`: {e}", entry.path().display()))?;
        let master: Master = toml::from_str(&raw)
            .map_err(|e| miette!("cannot parse `{}`: {e}", entry.path().display()))?;
        let mut objects = Vec::new();
        for obj in &master.shapes {
            if !matches!(obj.ty.as_str(), "chart") && opts::ctx_for_style_type(&obj.ty).is_none() {
                return Err(miette!("master `{stem}`: unknown shape type `{}`", obj.ty));
            }
            let mut opts = merge_opts(main, &obj.ty, obj.style.as_deref(), &obj.opts)?;
            let ctx = match obj.ty.as_str() {
                "text" => Some(Ctx::Text),
                "placeholder" => Some(Ctx::Placeholder),
                "image" => Some(Ctx::Image),
                "chart" => None,
                _ => Some(Ctx::Shape),
            };
            if let Some(ctx) = ctx {
                opts::validate_ctx(ctx, &opts, &format!("master `{stem}` shape `{}`", obj.ty))?;
            }
            if obj.ty == "placeholder" {
                placeholder_opts(&mut opts, &stem)?;
            }
            let mut out = Map::new();
            out.insert("type".into(), json!(obj.ty));
            out.insert("x".into(), json!(geo(&obj.x, width, "x")?));
            out.insert("y".into(), json!(geo(&obj.y, height, "y")?));
            out.insert("w".into(), json!(geo(&obj.w, width, "w")?));
            out.insert("h".into(), json!(geo(&obj.h, height, "h")?));
            out.insert("text".into(), json!(obj.text));
            out.insert("options".into(), to_pptxgen(&opts));
            if obj.ty == "image"
                && let Some(src) = &obj.src
            {
                let (data, mime) = media_data(project, src)?;
                out.insert("data".into(), json!(data));
                out.insert("mime".into(), json!(mime));
            }
            objects.push(Value::Object(out));
        }
        let slide_number = match &master.slide_number {
            Some(sn) => slide_number_value(sn, width, height, &stem)?,
            None => Value::Null,
        };
        out.push(json!({
            "name": stem,
            "background": normalize_background(&master.background, "master")?,
            "margin": normalize_margin(&master.margin)?,
            "slideNumber": slide_number,
            "objects": objects,
        }));
    }
    Ok(out)
}

/// Validate placeholder options on a master shape and rewrite `ph_type` to the
/// `type` pptxgenjs's `defineSlideMaster` expects.
fn placeholder_opts(opts: &mut toml::Value, master: &str) -> Result<()> {
    let table = opts
        .as_table_mut()
        .ok_or_else(|| miette!("internal: placeholder options"))?;
    if !table.contains_key("name") {
        return Err(miette!("master `{master}` placeholder needs a `name`"));
    }
    let label = table
        .get("name")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "<unnamed>".into());
    let ph_type = table
        .remove("ph_type")
        .ok_or_else(|| miette!("master `{master}` placeholder `{label}` needs a `ph_type`"))?;
    let ph_type = ph_type.as_str().ok_or_else(|| {
        miette!("master `{master}` placeholder `{label}`: `ph_type` must be a string")
    })?;
    if !matches!(
        ph_type,
        "title" | "body" | "pic" | "chart" | "tbl" | "media"
    ) {
        return Err(miette!(
            "master `{master}` placeholder `{label}`: `ph_type` must be one of title|body|pic|chart|tbl|media, got `{ph_type}`"
        ));
    }
    table.insert("type".into(), toml::Value::String(ph_type.to_string()));
    Ok(())
}

/// Convert a master `slide_number` into inches + camelCase options for `defineSlideMaster`.
fn slide_number_value(sn: &SlideNumber, width: i64, height: i64, master: &str) -> Result<Value> {
    if !sn.opts.is_empty() {
        opts::validate_ctx(
            Ctx::Text,
            &toml::Value::Table(sn.opts.clone()),
            &format!("master `{master}` slide_number"),
        )?;
    }
    let mut out = Map::new();
    out.insert("x".into(), json!(geo(&sn.x, width, "x")?));
    out.insert("y".into(), json!(geo(&sn.y, height, "y")?));
    out.insert("w".into(), json!(geo(&sn.w, width, "w")?));
    out.insert("h".into(), json!(geo(&sn.h, height, "h")?));
    let opts = to_pptxgen(&toml::Value::Table(sn.opts.clone()));
    for (k, v) in opts.as_object().unwrap() {
        out.insert(k.clone(), v.clone());
    }
    Ok(Value::Object(out))
}

fn build_sections(
    project: &Project,
    sections: &[Section],
    main_ev: &Main,
    width: i64,
    height: i64,
) -> Result<Vec<Value>> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for section in sections {
        let mut slides = Vec::new();
        for file in &section.slides {
            if !seen.insert(file.clone()) {
                return Err(miette!("slide `{file}` appears in more than one section"));
            }
            slides.push(build_slide(project, main_ev, width, height, file)?);
        }
        out.push(json!({ "title": section.title, "slides": slides }));
    }
    Ok(out)
}

/// The placeholder `name`s a master defines, for slide-fill validation.
fn master_placeholder_names(
    project: &Project,
    master: &str,
    slide: &str,
) -> Result<BTreeSet<String>> {
    let path = project.master_path(master);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| miette!("slide `{slide}`: cannot read `{}`: {e}", path.display()))?;
    let m: Master = toml::from_str(&raw)
        .map_err(|e| miette!("slide `{slide}`: cannot parse `{}`: {e}", path.display()))?;
    Ok(m.shapes
        .iter()
        .filter(|s| s.ty == "placeholder")
        .filter_map(|s| {
            s.opts
                .get("name")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect())
}

fn build_slide(
    project: &Project,
    main: &Main,
    width: i64,
    height: i64,
    file: &str,
) -> Result<Value> {
    let path = project.slide_path(file);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| miette!("cannot read `{}`: {e}", path.display()))?;
    let slide: crate::model::Slide =
        toml::from_str(&raw).map_err(|e| miette!("cannot parse `{}`: {e}", path.display()))?;
    if let Some(master) = &slide.master
        && !project.master_path(master).is_file()
    {
        return Err(miette!(
            "slide `{file}` references unknown master `{master}` (no `{}`)",
            project.master_path(master).display()
        ));
    }
    if let Some(master) = &slide.master {
        let names = master_placeholder_names(project, master, file)?;
        for shape in &slide.shapes {
            if let Some(p) = shape.opts.get("placeholder").and_then(|v| v.as_str())
                && !names.contains(p)
            {
                return Err(miette!(
                    "slide `{file}` fills placeholder `{p}` but master `{master}` has no placeholder of that name"
                ));
            }
        }
    }

    let mut shapes = Vec::new();
    for shape in &slide.shapes {
        shapes.push(shape_value(project, main, width, height, file, shape)?);
    }

    Ok(json!({
        "master": slide.master,
        "background": normalize_background(&slide.background, "slide")?,
        "hidden": slide.hidden,
        "shapes": shapes,
        "notes": slide.notes,
    }))
}

fn shape_value(
    project: &Project,
    main: &Main,
    width: i64,
    height: i64,
    file: &str,
    shape: &Shape,
) -> Result<Value> {
    let mut kind = shape.kind().map_err(|e| miette!("slide `{file}`: {e}"))?;
    if matches!(kind, Kind::Shape) && !opts::is_shape_type(&shape.ty) {
        return Err(miette!("slide `{file}`: unknown shape type `{}`", shape.ty));
    }
    // A shape preset with text becomes a text box drawn with that preset
    // (pptxgenjs `addText` accepts a `shape` option), so markdown works and
    // the shape's fill/line/font options all apply.
    let carries_text =
        matches!(kind, Kind::Shape) && (shape.text.is_some() || !shape.paragraphs.is_empty());
    if carries_text {
        kind = Kind::Text;
    }
    let mut merged = merge_opts(main, &shape.ty, shape.style.as_deref(), &shape.opts)?;
    if carries_text {
        merged
            .as_table_mut()
            .unwrap()
            .insert("shape".into(), toml::Value::String(shape.ty.clone()));
    }
    let ctx = match kind {
        Kind::Text => Ctx::Text,
        Kind::Image => Ctx::Image,
        Kind::Shape => Ctx::Shape,
    };
    opts::validate_ctx(
        ctx,
        &merged,
        &format!("slide `{file}` shape `{}`", shape.ty),
    )?;
    let opts = to_pptxgen(&merged);

    let mut value = Map::new();
    value.insert("x".into(), json!(geo(&shape.x, width, "x")?));
    value.insert("y".into(), json!(geo(&shape.y, height, "y")?));
    value.insert("w".into(), json!(geo(&shape.w, width, "w")?));
    value.insert("h".into(), json!(geo(&shape.h, height, "h")?));
    match kind {
        Kind::Text => {
            value.insert("kind".into(), json!("text"));
            value.insert("runs".into(), runs_value(shape, file)?);
            for (k, v) in opts.as_object().unwrap() {
                value.insert(k.clone(), v.clone());
            }
        }
        Kind::Image => {
            value.insert("kind".into(), json!("image"));
            for (k, v) in image_value(project, file, shape)?.as_object().unwrap() {
                value.insert(k.clone(), v.clone());
            }
            for (k, v) in opts.as_object().unwrap() {
                value.insert(k.clone(), v.clone());
            }
        }
        Kind::Shape => {
            value.insert("kind".into(), json!("shape"));
            value.insert("type".into(), json!(shape.ty));
            for (k, v) in opts.as_object().unwrap() {
                value.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(Value::Object(value))
}

/// Build the flat run list for a text shape — pptxgenjs's native text model.
///
/// The markdown paragraphs are flattened into one run list; the last run of
/// every paragraph except the final one carries `breakLine` (pptxgenjs closes
/// the paragraph *after* that run) and the first run of a paragraph carries
/// its own options (`bullet`, `indent_level`, `line_spacing`, ...). A markdown
/// single `\n` already became a `softBreakBefore` run. `align` is dropped from
/// paragraph options in v1 because pptxgenjs auto-splits paragraphs on `align`
/// changes, which would double-split with `breakLine`; use the shape-level
/// `align` instead.
///
/// Text is `[[paragraphs]]` if given, else the `text` shorthand, else one
/// empty run so `addText` always has content.
fn runs_value(shape: &Shape, file: &str) -> Result<Value> {
    struct Para<'a> {
        text: &'a str,
        opts: &'a toml::Table,
    }
    let mut paragraphs: Vec<Para> = Vec::new();
    let empty = toml::Table::new();
    if !shape.paragraphs.is_empty() {
        for para in &shape.paragraphs {
            if !para.opts.is_empty() {
                opts::validate_ctx(
                    Ctx::Text,
                    &toml::Value::Table(para.opts.clone()),
                    &format!("slide `{file}` shape `{}` paragraph", shape.ty),
                )?;
            }
            paragraphs.push(Para {
                text: &para.text,
                opts: &para.opts,
            });
        }
    } else if let Some(text) = &shape.text {
        paragraphs.push(Para { text, opts: &empty });
    }
    if paragraphs.is_empty() {
        return Ok(Value::Array(vec![json!({ "text": "" })]));
    }

    // Flatten markdown paragraphs to runs, remembering paragraph boundaries.
    struct Group {
        runs: Vec<richtext::Run>,
        opts: serde_json::Map<String, Value>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for para in &paragraphs {
        let para_opts = to_pptxgen(&toml::Value::Table(para.opts.clone()));
        let para_opts = para_opts
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| k.as_str() != "align")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Map<_, _>>();
        for runs in richtext::parse_paragraphs(para.text) {
            groups.push(Group {
                runs,
                opts: para_opts.clone(),
            });
        }
    }

    let mut runs: Vec<Value> = Vec::new();
    let group_count = groups.len();
    for (gi, group) in groups.iter().enumerate() {
        for (rri, run) in group.runs.iter().enumerate() {
            let mut obj = serde_json::to_value(run)
                .map_err(|e| miette!("cannot serialise run: {e}"))?
                .as_object()
                .unwrap()
                .clone();
            if rri == 0 {
                for (k, v) in &group.opts {
                    obj.insert(k.clone(), v.clone());
                }
            }
            runs.push(Value::Object(obj));
        }
        // pptxgenjs's breakLine ends the paragraph after the run carrying it,
        // so it goes on the last run of every paragraph but the final one.
        if gi + 1 < group_count
            && let Some(last) = runs.last_mut()
            && let Some(obj) = last.as_object_mut()
        {
            obj.insert("breakLine".into(), json!(true));
        }
    }
    Ok(Value::Array(runs))
}

/// Load + base64-encode a media file referenced by an image shape.
fn image_value(project: &Project, file: &str, shape: &Shape) -> Result<Value> {
    let src = shape
        .src
        .as_deref()
        .ok_or_else(|| miette!("slide `{file}`: image shape needs `src`"))?;
    let (data, mime) = media_data(project, src)?;
    let mut obj = Map::new();
    obj.insert("data".into(), json!(data));
    obj.insert("mime".into(), json!(mime));
    if !shape.opts.contains_key("sizing") {
        obj.insert("sizing".into(), json!({ "type": "contain" }));
    }
    Ok(Value::Object(obj))
}

/// Read a media file and return a `data:` URI plus its MIME type.
fn media_data(project: &Project, src: &str) -> Result<(String, &'static str)> {
    let path = project.dir.join(src);
    let bytes =
        std::fs::read(&path).map_err(|e| miette!("cannot read image `{}`: {e}", path.display()))?;
    let mime = mime_for(src)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok((format!("data:{mime};base64,{encoded}"), mime))
}

/// Merge `[styles.<type>]` == `[styles.named.<name>]` == shape.opts (later
/// wins). An unknown named-style reference is an error so typos are caught.
fn merge_opts(
    main: &Main,
    ty: &str,
    style: Option<&str>,
    local: &toml::Table,
) -> Result<toml::Value> {
    let mut merged = main.type_defaults(ty);
    if let Some(name) = style
        && let Some(style_opts) = main.styles.named.get(name)
    {
        for (k, v) in style_opts {
            merged.insert(k.clone(), v.clone());
        }
    } else if let Some(name) = style {
        return Err(miette!(
            "unknown style `{name}` (no `[styles.named.{name}]` in main.toml)"
        ));
    }
    for (k, v) in local {
        merged.insert(k.clone(), v.clone());
    }
    Ok(toml::Value::Table(merged))
}

/// snake_case -> camelCase recursively over the pptxgenjs option object.
fn to_pptxgen(v: &toml::Value) -> Value {
    match v {
        toml::Value::Table(t) => {
            let mut out = Map::new();
            for (k, val) in t {
                out.insert(camel(k), to_pptxgen(val));
            }
            Value::Object(out)
        }
        toml::Value::Array(a) => Value::Array(a.iter().map(to_pptxgen).collect()),
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(i) => Value::from(*i),
        toml::Value::Float(f) => Value::from(*f),
        toml::Value::Boolean(b) => Value::from(*b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
    }
}

fn camel(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper_next = false;
    for c in key.chars() {
        if c == '_' {
            upper_next = true;
        } else if upper_next {
            out.push(c.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// EMU / unit string / percentage -> inches for the spec (geometry only).
fn geo(coord: &Option<Coord>, slide_emu: i64, axis: &str) -> Result<f64> {
    coord
        .as_ref()
        .map(|c| c.inches(slide_emu))
        .unwrap_or(Ok(0.0))
        .map_err(|e| miette!("invalid `{axis}` coordinate: {e}"))
}

/// A `background` value may be a color shorthand (`"FFFFFF"`) or a full
/// background object (color/path/data/transparency). Normalise to the object
/// pptxgenjs expects for slide.background / defineSlideMaster background.
fn normalize_background(bg: &Option<toml::Value>, where_: &str) -> Result<Value> {
    match bg {
        None => Ok(Value::Null),
        Some(toml::Value::Table(t)) => {
            opts::validate_ctx(
                Ctx::Background,
                &toml::Value::Table(t.clone()),
                &format!("{where_} background"),
            )?;
            Ok(to_pptxgen(&toml::Value::Table(t.clone())))
        }
        Some(toml::Value::String(s)) => Ok(json!({ "color": s })),
        Some(other) => Err(miette!(
            "{where_} background must be a color string or table, got `{other}`"
        )),
    }
}

/// Master margin: a single length or `[top, right, bottom, left]`. Normalise
/// lengths to inches for pptxgenjs.
fn normalize_margin(margin: &Option<toml::Value>) -> Result<Value> {
    fn one(v: &toml::Value) -> Result<f64> {
        match v {
            toml::Value::Integer(i) => Ok(*i as f64 / EMU_PER_IN),
            toml::Value::Float(f) => Ok(f / EMU_PER_IN),
            toml::Value::String(s) => Coord::Text(s.clone())
                .inches(0)
                .map_err(|e| miette!("invalid margin: {e}")),
            _ => Err(miette!("invalid margin entry `{v}`")),
        }
    }
    match margin {
        None => Ok(Value::Null),
        Some(toml::Value::Array(items)) => {
            let vals: Result<Vec<Value>> = items.iter().map(|i| Ok(json!(one(i)?))).collect();
            Ok(Value::Array(vals?))
        }
        Some(v) => Ok(json!(one(v)?)),
    }
}

/// The slide ordering index comes from `[[sections]]` in `main.toml`.
fn resolve_sections(project: &Project) -> Result<Vec<Section>> {
    if project.main.sections.is_empty() {
        return Err(miette!(
            "main.toml has no [[sections]]; list your slides under [[sections]] to build the deck"
        ));
    }
    Ok(project.main.sections.clone())
}

fn mime_for(filename: &str) -> Result<&'static str> {
    let ext = filename.rsplit('.').next().unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "bmp" => Ok("image/bmp"),
        "tif" | "tiff" => Ok("image/tiff"),
        "svg" => Ok("image/svg+xml"),
        other => Err(miette!("unsupported image extension `.{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_renames() {
        assert_eq!(camel("font_face"), "fontFace");
        assert_eq!(camel("line_spacing"), "lineSpacing");
        assert_eq!(camel("color"), "color");
        assert_eq!(camel("rtl_mode"), "rtlMode");
        assert_eq!(camel("bullet"), "bullet");
    }

    #[test]
    fn coord_to_inches() {
        assert_eq!(Coord::Text("1in".into()).inches(9144000).unwrap(), 1.0);
        assert_eq!(Coord::Emu(9144000).inches(9144000).unwrap(), 10.0);
    }

    #[test]
    fn options_table_converts() {
        let mut t = toml::Table::new();
        t.insert("font_size".into(), toml::Value::Integer(18));
        t.insert("fill".into(), {
            let mut f = toml::Table::new();
            f.insert("color".into(), toml::Value::String("C7000A".into()));
            toml::Value::Table(f)
        });
        let v = to_pptxgen(&toml::Value::Table(t));
        assert_eq!(v["fontSize"], json!(18));
        assert_eq!(v["fill"]["color"], json!("C7000A"));
    }
}
