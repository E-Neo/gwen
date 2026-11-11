//! Translate a TOML deck (load `main.toml`, masters and slides, resolve
//! defaults/styles/options, parse rich text, normalise everything to the
//! units pptxgenjs expects) into the JSON spec the embedded bridge renders.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use base64::Engine;
use miette::{Result, miette};
use serde_json::{Map, Value, json};
use toml_edit::{Document, InlineTable, Item, Table};

use crate::model::{Kind, Main, Master, Project, Section, Shape, SlideNumber};
use crate::opts::{self, Ctx};
use crate::richtext;
use crate::units::{Coord, EMU_PER_IN};

/// The typed shape keys that live outside the pptxgenjs option map.
const SHAPE_SKIP: &[&str] = &[
    "type",
    "styles",
    "text",
    "paragraphs",
    "src",
    "rows",
    "col_styles",
    "row_styles",
    "x",
    "y",
    "w",
    "h",
];
/// The typed paragraph keys outside the option map.
const PARA_SKIP: &[&str] = &["text"];
/// The typed slide-number keys outside the option map.
const SLIDE_NUMBER_SKIP: &[&str] = &["x", "y", "w", "h", "locked"];

/// A parsed deck file backed by a `toml_edit` document, so validation errors
/// can point at the exact `path:line:col` in the source TOML.
struct FileSrc {
    path: String,
    text: String,
    doc: Document<String>,
}

impl FileSrc {
    fn new(path: PathBuf, text: String) -> Result<FileSrc> {
        let doc = text
            .parse::<Document<String>>()
            .map_err(|e| miette!("cannot parse `{}`: {e}", path.display()))?;
        Ok(FileSrc {
            path: path.display().to_string(),
            text,
            doc,
        })
    }

    fn src(&self) -> opts::Src {
        opts::Src {
            path: self.path.clone(),
            text: self.text.clone(),
        }
    }

    fn shapes(&self) -> Vec<Table> {
        let Some(shapes) = self
            .doc
            .as_table()
            .get("shapes")
            .and_then(|i| i.as_array_of_tables())
        else {
            return Vec::new();
        };
        shapes.iter().cloned().collect()
    }

    fn shape_at(&self, idx: usize) -> Option<&Table> {
        self.doc
            .as_table()
            .get("shapes")
            .and_then(|i| i.as_array_of_tables())
            .and_then(|a| a.get(idx))
    }

    fn paras_for(&self, shape: &Table) -> Vec<Table> {
        let Some(paras) = shape.get("paragraphs").and_then(|i| i.as_array_of_tables()) else {
            return Vec::new();
        };
        paras.iter().cloned().collect()
    }

    fn table_of(&self, key: &str) -> Option<Table> {
        self.doc.as_table().get(key).and_then(|i| match i {
            Item::Value(toml_edit::Value::InlineTable(it)) => Some(inline_to_table(it)),
            Item::Table(t) => Some(t.clone()),
            _ => None,
        })
    }
}

fn inline_to_table(it: &InlineTable) -> Table {
    let mut t = Table::new();
    for (k, v) in it.iter() {
        t.insert(k, Item::Value(v.clone()));
    }
    t
}

/// Per-slide rendering context, bundling the deck-level inputs that every
/// `shape_value` call needs.
struct SlideCtx<'a> {
    project: &'a Project,
    main: &'a Main,
    width: i64,
    height: i64,
    file: &'a str,
    file_src: &'a FileSrc,
}

impl SlideCtx<'_> {
    fn shape_table(&self, idx: usize) -> Option<&Table> {
        self.file_src.shape_at(idx)
    }

    fn paras(&self, idx: usize) -> Vec<Table> {
        self.shape_table(idx)
            .map(|t| self.file_src.paras_for(t))
            .unwrap_or_default()
    }
}

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

    let main_raw = std::fs::read_to_string(project.dir.join("main.toml")).map_err(|e| {
        miette!(
            "cannot read `{}`: {e}",
            project.dir.join("main.toml").display()
        )
    })?;
    let main_src = FileSrc::new(project.dir.join("main.toml"), main_raw)?;

    validate_styles_located(&main_src)?;
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
        let ctx = opts::ctx_for_style_type(opts::canonical_preset(ty).unwrap_or(ty))
            .ok_or_else(|| miette!("unknown style type `[styles.{ty}]` (not a shape type)"))?;
        opts::validate_ctx(
            ctx,
            &toml::Value::Table(table.clone()),
            &format!("[styles.{ty}]"),
        )?;
        opts::validate_list_markers(table, &format!("[styles.{ty}]"))?;
        validate_styles_value(table, &format!("[styles.{ty}]"))?;
    }
    for (name, table) in &main.styles.named {
        opts::validate_ctx(
            Ctx::Style,
            &toml::Value::Table(table.clone()),
            &format!("[styles.named.{name}]"),
        )?;
        opts::validate_list_markers(table, &format!("[styles.named.{name}]"))?;
        validate_styles_value(table, &format!("[styles.named.{name}]"))?;
    }
    main.validate_style_graph()
}

/// A bucket's `styles` key must be a string or a list of strings.
fn validate_styles_value(table: &toml::Table, where_: &str) -> Result<()> {
    if let Some(v) = table.get("styles")
        && opts::parse_styles_value(v).is_none()
    {
        return Err(miette!(
            "{where_}: `styles` must be a string or a list of strings"
        ));
    }
    Ok(())
}

/// Located version of `validate_styles`: validate the `[styles.*]` buckets of
/// `main.toml` against the parsed toml_edit document so diagnostics carry the
/// exact key/line in `main.toml`.
fn validate_styles_located(main_src: &FileSrc) -> Result<()> {
    let Some(styles) = main_src
        .doc
        .as_table()
        .get("styles")
        .and_then(|i| i.as_table())
    else {
        return Ok(());
    };
    let src = main_src.src();
    for (ty, item) in styles.iter() {
        if ty == "named" {
            if let Some(named) = item.as_table() {
                for (name, item) in named.iter() {
                    let Some(table) = item.as_table() else {
                        continue;
                    };
                    let where_ = format!("[styles.named.{name}]");
                    validate_source_bucket(Ctx::Style, table, &src, &where_)?;
                }
            }
            continue;
        }
        let Some(table) = item.as_table() else {
            continue;
        };
        let canon = opts::canonical_preset(ty).unwrap_or(ty);
        let ctx = opts::ctx_for_style_type(canon)
            .ok_or_else(|| miette!("unknown style type `[styles.{ty}]` (not a shape type)"))?;
        validate_source_bucket(ctx, table, &src, &format!("[styles.{ty}]"))?;
    }
    Ok(())
}

fn validate_source_bucket(ctx: Ctx, table: &Table, src: &opts::Src, where_: &str) -> Result<()> {
    opts::validate_source(ctx, table, src, where_, &[])?;
    opts::validate_list_markers_edit(table, src, where_)
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
        let file_src = FileSrc::new(entry.path(), raw.clone())?;
        let src = file_src.src();
        let shape_tables = file_src.shapes();
        let master: Master = toml::from_str(&raw)
            .map_err(|e| miette!("cannot parse `{}`: {e}", entry.path().display()))?;
        let mut objects = Vec::new();
        for (idx, obj) in master.shapes.iter().enumerate() {
            let ty = opts::canonical_preset(&obj.ty).unwrap_or(&obj.ty);
            if !matches!(ty, "chart") && opts::ctx_for_style_type(ty).is_none() {
                return Err(miette!("master `{stem}`: unknown shape type `{}`", obj.ty));
            }
            if ty == "table" {
                return Err(miette!(
                    "master `{stem}`: table shapes are only supported on slides"
                ));
            }
            let mut opts = merge_opts(main, &obj.ty, &obj.styles, &obj.opts)?;
            let list = take_list_options(&mut opts);
            let where_ = format!("master `{stem}` shape `{}`", obj.ty);
            let ctx = match ty {
                "text" => Some(Ctx::Text),
                "placeholder" => Some(Ctx::Placeholder),
                "image" => Some(Ctx::Image),
                "chart" => None,
                _ => Some(Ctx::TextShape),
            };
            if let Some(ctx) = ctx {
                if let Some(table) = shape_tables.get(idx) {
                    opts::validate_source(ctx, table, &src, &where_, SHAPE_SKIP)?;
                }
                // Base buckets (`[styles.shape]`) may hold keys that only some
                // shape types can apply; keep only what this shape accepts.
                opts::retain_ctx_keys(ctx, &mut opts);
                opts::validate_ctx(ctx, &opts, &where_)?;
                opts::finalize_opts(ctx, &mut opts)?;
            }
            if obj.ty == "placeholder" {
                placeholder_opts(&mut opts, &stem)?;
            }
            let mut out = Map::new();
            out.insert("type".into(), json!(ty));
            out.insert("x".into(), json!(geo(&obj.x, width, "x")?));
            out.insert("y".into(), json!(geo(&obj.y, height, "y")?));
            let (w_in, h_in) = if ty == "image" {
                let sizing = opts.as_table().is_some_and(|t| t.contains_key("sizing"));
                image_box(
                    &obj.w,
                    &obj.h,
                    (width, height),
                    sizing,
                    obj.src.as_deref(),
                    &project.dir,
                    &where_,
                )?
            } else {
                (geo(&obj.w, width, "w")?, geo(&obj.h, height, "h")?)
            };
            reject_negative_extent(w_in, "w", &where_)?;
            reject_negative_extent(h_in, "h", &where_)?;
            out.insert("w".into(), json!(w_in));
            out.insert("h".into(), json!(h_in));
            if matches!(ty, "text" | "placeholder") {
                // Master/placeholder text gets the same markdown + `<span>`
                // run treatment as slide text.
                let mut paragraphs: Vec<TextPara> = Vec::new();
                let empty = toml::Table::new();
                if !obj.paragraphs.is_empty() {
                    for (j, para) in obj.paragraphs.iter().enumerate() {
                        if !para.opts.is_empty() {
                            opts::validate_ctx(
                                Ctx::Text,
                                &toml::Value::Table(para.opts.clone()),
                                &format!("{where_} paragraph"),
                            )?;
                        }
                        paragraphs.push(TextPara {
                            text: &para.text,
                            opts: &para.opts,
                            index: j,
                            span: shape_tables
                                .get(idx)
                                .and_then(|t| t.get("paragraphs"))
                                .and_then(|i| i.as_array_of_tables())
                                .and_then(|a| a.get(j))
                                .and_then(|t| t.get("text"))
                                .and_then(|i| i.span()),
                        });
                    }
                } else if let Some(text) = &obj.text {
                    paragraphs.push(TextPara {
                        text,
                        opts: &empty,
                        index: usize::MAX,
                        span: shape_tables
                            .get(idx)
                            .and_then(|t| t.get("text"))
                            .and_then(|i| i.span()),
                    });
                }
                out.insert(
                    "runs".into(),
                    flatten_runs(&paragraphs, &list, &main.styles.named, &file_src, &where_)?,
                );
            } else {
                out.insert("text".into(), json!(obj.text));
            }
            out.insert("options".into(), to_pptxgen(&opts));
            if obj.ty == "image"
                && let Some(src) = &obj.src
            {
                let (data, mime, preview) =
                    media_value(project, src, Some((w_in * 96.0, h_in * 96.0)))?;
                out.insert("data".into(), json!(data));
                out.insert("mime".into(), json!(mime));
                if let Some(preview) = preview {
                    out.insert("preview".into(), json!(preview));
                }
            }
            objects.push(Value::Object(out));
        }
        let slide_number = match &master.slide_number {
            Some(sn) => {
                let table = file_src.table_of("slide_number");
                slide_number_value(sn, width, height, &stem, &src, table.as_ref())?
            }
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

/// Convert a master `slide_number` into inches + camelCase options for
/// `defineSlideMaster`. `locked` is a gwen-owned key that never reaches
/// pptxgenjs; it marks whether the number should be locked (it rides in the
/// spec's `slideNumber` object, which the host reads to pick which parts to
/// lock).
fn slide_number_value(
    sn: &SlideNumber,
    width: i64,
    height: i64,
    master: &str,
    src: &opts::Src,
    table: Option<&Table>,
) -> Result<Value> {
    let where_ = format!("master `{master}` slide_number");
    if let Some(table) = table {
        opts::validate_source(Ctx::Text, table, src, &where_, SLIDE_NUMBER_SKIP)?;
    }
    let mut sn_opts = sn.opts.clone();
    let mut locked = false;
    if let Some(value) = sn_opts.remove("locked") {
        locked = match value {
            toml::Value::Boolean(b) => b,
            other => {
                return Err(miette!(
                    "master `{master}` slide_number: `locked` must be a boolean, got `{other}`"
                ));
            }
        };
    }
    if !sn_opts.is_empty() {
        opts::validate_ctx(Ctx::Text, &toml::Value::Table(sn_opts.clone()), &where_)?;
    }
    let mut opts = toml::Value::Table(sn_opts);
    opts::finalize_opts(Ctx::Text, &mut opts)?;
    let mut out = Map::new();
    out.insert("x".into(), json!(geo(&sn.x, width, "x")?));
    out.insert("y".into(), json!(geo(&sn.y, height, "y")?));
    let w_in = geo(&sn.w, width, "w")?;
    let h_in = geo(&sn.h, height, "h")?;
    reject_negative_extent(w_in, "w", &where_)?;
    reject_negative_extent(h_in, "h", &where_)?;
    out.insert("w".into(), json!(w_in));
    out.insert("h".into(), json!(h_in));
    let opts = to_pptxgen(&opts);
    for (k, v) in opts.as_object().unwrap() {
        out.insert(k.clone(), v.clone());
    }
    out.insert("locked".into(), json!(locked));
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
    let file_src = FileSrc::new(path.clone(), raw.clone())?;
    let slide: crate::model::Slide =
        toml::from_str(&raw).map_err(|e| miette!("cannot parse `{}`: {e}", path.display()))?;
    let Some(master) = &slide.master else {
        return Err(miette!(
            "slide `{file}`: missing required `master` (add `master = \"<name>\"` at the top of the file)"
        ));
    };
    if !project.master_path(master).is_file() {
        return Err(miette!(
            "slide `{file}` references unknown master `{master}` (no `{}`)",
            project.master_path(master).display()
        ));
    }
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

    let mut shapes = Vec::new();
    let slide_ctx = SlideCtx {
        project,
        main,
        width,
        height,
        file,
        file_src: &file_src,
    };
    for (idx, shape) in slide.shapes.iter().enumerate() {
        shapes.push(shape_value(&slide_ctx, shape, idx)?);
    }

    Ok(json!({
        "file": file,
        "master": slide.master,
        "background": normalize_background(&slide.background, "slide")?,
        "hidden": slide.hidden,
        "shapes": shapes,
        "notes": slide.notes,
    }))
}

fn shape_value(ctx: &SlideCtx, shape: &Shape, idx: usize) -> Result<Value> {
    let file = ctx.file;
    let mut kind = shape.kind().map_err(|e| miette!("slide `{file}`: {e}"))?;
    if kind == Kind::Table {
        return table_value(ctx, shape, idx);
    }
    if !shape.rows.is_empty() || !shape.col_styles.is_empty() || !shape.row_styles.is_empty() {
        return Err(miette!(
            "slide `{file}`: `rows`, `col_styles` and `row_styles` are only valid for `type = \"table\"`"
        ));
    }
    if matches!(kind, Kind::Shape) && !opts::is_shape_type(&shape.ty) {
        return Err(miette!("slide `{file}`: unknown shape type `{}`", shape.ty));
    }
    let canon_ty = opts::canonical_preset(&shape.ty).unwrap_or(&shape.ty);
    // A shape preset with text becomes a text box drawn with that preset
    // (pptxgenjs `addText` accepts a `shape` option), so markdown works and
    // the shape's fill/line/font options all apply.
    let carries_text =
        matches!(kind, Kind::Shape) && (shape.text.is_some() || !shape.paragraphs.is_empty());
    if carries_text {
        kind = Kind::Text;
    }
    let octx = match kind {
        Kind::Text => Ctx::Text,
        Kind::Image => Ctx::Image,
        Kind::Shape => Ctx::TextShape,
        Kind::Table => Ctx::Table,
    };
    let where_ = format!("slide `{file}` shape `{}`", shape.ty);
    let src = ctx.file_src.src();
    if let Some(table) = ctx.shape_table(idx) {
        opts::validate_source(octx, table, &src, &where_, SHAPE_SKIP)?;
    }
    if matches!(kind, Kind::Text) {
        let paras = ctx.paras(idx);
        for (j, _para) in shape.paragraphs.iter().enumerate() {
            if let Some(table) = paras.get(j) {
                opts::validate_source(
                    Ctx::Text,
                    table,
                    &src,
                    &format!("{where_} paragraph"),
                    PARA_SKIP,
                )?;
            }
        }
    }
    let mut merged = merge_opts(ctx.main, &shape.ty, &shape.styles, &shape.opts)?;
    let list = take_list_options(&mut merged);
    if carries_text {
        merged
            .as_table_mut()
            .unwrap()
            .insert("shape".into(), toml::Value::String(canon_ty.to_string()));
    }
    // Base buckets (`[styles.shape]`) may hold keys that only some shape types
    // can apply; keep only what this shape accepts.
    opts::retain_ctx_keys(octx, &mut merged);
    opts::validate_ctx(octx, &merged, &where_)?;
    opts::finalize_opts(octx, &mut merged)?;
    let opts = to_pptxgen(&merged);

    let mut value = Map::new();
    value.insert("index".into(), json!(idx));
    value.insert("x".into(), json!(geo(&shape.x, ctx.width, "x")?));
    value.insert("y".into(), json!(geo(&shape.y, ctx.height, "y")?));
    let (w_in, h_in) = if matches!(kind, Kind::Image) {
        let sizing = merged.as_table().is_some_and(|t| t.contains_key("sizing"));
        image_box(
            &shape.w,
            &shape.h,
            (ctx.width, ctx.height),
            sizing,
            shape.src.as_deref(),
            &ctx.project.dir,
            &where_,
        )?
    } else {
        (
            geo(&shape.w, ctx.width, "w")?,
            geo(&shape.h, ctx.height, "h")?,
        )
    };
    reject_negative_extent(w_in, "w", &where_)?;
    reject_negative_extent(h_in, "h", &where_)?;
    value.insert("w".into(), json!(w_in));
    value.insert("h".into(), json!(h_in));
    match kind {
        Kind::Text => {
            value.insert("kind".into(), json!("text"));
            value.insert("runs".into(), runs_value(ctx, idx, shape, &list)?);
            for (k, v) in opts.as_object().unwrap() {
                value.insert(k.clone(), v.clone());
            }
        }
        Kind::Image => {
            value.insert("kind".into(), json!("image"));
            for (k, v) in image_value(ctx.project, file, shape, w_in, h_in)?
                .as_object()
                .unwrap()
            {
                value.insert(k.clone(), v.clone());
            }
            for (k, v) in opts.as_object().unwrap() {
                value.insert(k.clone(), v.clone());
            }
        }
        Kind::Shape => {
            value.insert("kind".into(), json!("shape"));
            value.insert("type".into(), json!(canon_ty));
            for (k, v) in opts.as_object().unwrap() {
                value.insert(k.clone(), v.clone());
            }
        }
        Kind::Table => unreachable!("table shapes return early"),
    }
    Ok(Value::Object(value))
}

#[derive(Clone, Default)]
struct TableCellInput {
    text: String,
    opts: toml::Table,
    colspan: usize,
    rowspan: usize,
    span: Option<std::ops::Range<usize>>,
    origin_row: usize,
    origin_col: usize,
}

#[derive(Clone, Copy)]
enum TableSlot {
    Origin(usize),
    Filler { hmerge: bool, vmerge: bool },
}

const BORDER_SIDES: &[&str] = &["top", "right", "bottom", "left"];

fn table_value(ctx: &SlideCtx, shape: &Shape, idx: usize) -> Result<Value> {
    let file = ctx.file;
    let where_ = format!("slide `{file}` table");
    if shape.text.is_some() || !shape.paragraphs.is_empty() {
        return Err(miette!(
            "{where_}: table shapes use `rows` for content, not `text` or `paragraphs`"
        ));
    }
    if shape.src.is_some() {
        return Err(miette!("{where_}: table shapes do not take `src`"));
    }
    reject_auto_page(&shape.opts, &where_)?;

    let src = ctx.file_src.src();
    let shape_edit = ctx.shape_table(idx);
    if let Some(table) = shape_edit {
        opts::validate_source(Ctx::Table, table, &src, &where_, SHAPE_SKIP)?;
    }
    let mut merged = merge_opts(ctx.main, &shape.ty, &shape.styles, &shape.opts)?;
    let list = take_list_options(&mut merged);
    reject_auto_page(merged.as_table().unwrap_or(&toml::Table::new()), &where_)?;
    opts::retain_ctx_keys(Ctx::Table, &mut merged);
    opts::validate_ctx(Ctx::Table, &merged, &where_)?;
    opts::finalize_opts(Ctx::Table, &mut merged)?;
    let mut table_style = merged.as_table().cloned().unwrap_or_default();
    let object_name = table_style
        .remove("object_name")
        .and_then(|v| v.as_str().map(String::from));

    // Parse and validate the cell rows. The model rows provide the values;
    // the toml_edit rows provide byte spans for diagnostics.
    if shape.rows.is_empty() {
        return Err(miette!("{where_}: `rows` must not be empty"));
    }
    let edit_rows = shape_edit
        .and_then(|t| t.get("rows"))
        .and_then(|i| i.as_value())
        .and_then(|v| v.as_array());
    let mut raw_rows: Vec<Vec<TableCellInput>> = Vec::new();
    for (r, row) in shape.rows.iter().enumerate() {
        let mut cells = Vec::new();
        for (c, value) in row.iter().enumerate() {
            let cell_where = format!("{where_} row {} cell {}", r + 1, c + 1);
            let mut cell = parse_table_cell(value, &cell_where)?;
            let edit_cell = edit_rows
                .and_then(|rows| rows.get(r))
                .and_then(|row| row.as_array())
                .and_then(|cells| cells.get(c));
            cell.span = edit_cell.and_then(cell_text_span);
            if let Some(toml_edit::Value::InlineTable(t)) = edit_cell {
                let table = inline_to_table(t);
                opts::validate_source(Ctx::Cell, &table, &src, &cell_where, &["text"])?;
            }
            let mut cell_value = toml::Value::Table(cell.opts.clone());
            opts::finalize_opts(Ctx::Cell, &mut cell_value)?;
            cell.opts = cell_value.as_table().cloned().unwrap_or_default();
            cells.push(cell);
        }
        raw_rows.push(cells);
    }

    let cols: usize = raw_rows[0].iter().map(|c| c.colspan).sum();
    if cols == 0 {
        return Err(miette!("{where_}: the first row has no columns"));
    }
    let row_count = raw_rows.len();
    let mut occupied = vec![vec![false; cols]; row_count];
    let mut cover: Vec<Vec<Option<usize>>> = vec![vec![None; cols]; row_count];
    let mut origins: Vec<TableCellInput> = Vec::new();
    for r in 0..row_count {
        let mut c = 0usize;
        for cell in &raw_rows[r] {
            while c < cols && occupied[r][c] {
                c += 1;
            }
            if c + cell.colspan > cols {
                return Err(miette!(
                    "{where_}: row {} overflows the table width; its cells span more than {cols} columns",
                    r + 1
                ));
            }
            if r + cell.rowspan > row_count {
                return Err(miette!(
                    "{where_}: row {} has a cell with `rowspan` past the last row",
                    r + 1
                ));
            }
            let origin_idx = origins.len();
            for rr in r..r + cell.rowspan {
                for cc in c..c + cell.colspan {
                    if occupied[rr][cc] {
                        return Err(miette!(
                            "{where_}: row {} cell {} overlaps another merged cell",
                            r + 1,
                            c + 1
                        ));
                    }
                    occupied[rr][cc] = true;
                    cover[rr][cc] = Some(origin_idx);
                }
            }
            let mut origin = cell.clone();
            origin.origin_row = r;
            origin.origin_col = c;
            origins.push(origin);
            c += cell.colspan;
        }
        if !occupied[r].iter().all(|filled| *filled) {
            return Err(miette!(
                "{where_}: row {} is missing cells for the table width ({cols} columns)",
                r + 1
            ));
        }
    }

    // Per-column and per-row overlays. Geometry is removed before the rest is
    // merged into cells as style options.
    let col_edit = shape_edit
        .and_then(|t| t.get("col_styles"))
        .and_then(|i| i.as_value())
        .and_then(|v| v.as_array());
    let row_edit = shape_edit
        .and_then(|t| t.get("row_styles"))
        .and_then(|i| i.as_value())
        .and_then(|v| v.as_array());
    if !shape.col_styles.is_empty() && shape.col_styles.len() != cols {
        return Err(miette!(
            "{where_}: `col_styles` has {} entries but the table has {cols} columns",
            shape.col_styles.len()
        ));
    }
    if !shape.row_styles.is_empty() && shape.row_styles.len() != row_count {
        return Err(miette!(
            "{where_}: `row_styles` has {} entries but the table has {row_count} rows",
            shape.row_styles.len()
        ));
    }
    let mut col_widths: Vec<Option<f64>> = vec![None; cols];
    let mut col_style_opts: Vec<Option<toml::Table>> = vec![None; cols];
    for (i, raw) in shape.col_styles.iter().enumerate() {
        let style_where = format!("{where_} column style {}", i + 1);
        if let Some(toml_edit::Value::InlineTable(t)) = col_edit.and_then(|a| a.get(i)) {
            let table = inline_to_table(t);
            opts::validate_source(Ctx::ColStyle, &table, &src, &style_where, &[])?;
        }
        let mut style = raw.clone();
        if let Some(v) = style.remove("w") {
            col_widths[i] = Some(table_coord_inches(&v, ctx.width, "w", &style_where)?);
        }
        let mut value = toml::Value::Table(style);
        opts::finalize_opts(Ctx::ColStyle, &mut value)?;
        col_style_opts[i] = value.as_table().cloned();
    }
    let mut row_heights: Vec<Option<f64>> = vec![None; row_count];
    let mut row_style_opts: Vec<Option<toml::Table>> = vec![None; row_count];
    for (i, raw) in shape.row_styles.iter().enumerate() {
        let style_where = format!("{where_} row style {}", i + 1);
        if let Some(toml_edit::Value::InlineTable(t)) = row_edit.and_then(|a| a.get(i)) {
            let table = inline_to_table(t);
            opts::validate_source(Ctx::RowStyle, &table, &src, &style_where, &[])?;
        }
        let mut style = raw.clone();
        if let Some(v) = style.remove("h") {
            row_heights[i] = Some(table_coord_inches(&v, ctx.height, "h", &style_where)?);
        }
        let mut value = toml::Value::Table(style);
        opts::finalize_opts(Ctx::RowStyle, &mut value)?;
        row_style_opts[i] = value.as_table().cloned();
    }

    let shape_w = shape
        .w
        .as_ref()
        .map(|c| c.inches(ctx.width))
        .transpose()
        .map_err(|e| miette!("{where_}: invalid `w`: {e}"))?;
    let shape_h = shape
        .h
        .as_ref()
        .map(|c| c.inches(ctx.height))
        .transpose()
        .map_err(|e| miette!("{where_}: invalid `h`: {e}"))?;
    let default_w = (ctx.width as f64 / EMU_PER_IN - 1.0).max(0.0);
    let (widths, table_w) = resolve_columns(cols, &col_widths, shape_w, default_w, &where_)?;
    let (heights, table_h) = resolve_rows(row_count, &row_heights, shape_h, &where_)?;
    reject_negative_extent(table_w, "w", &where_)?;
    reject_negative_extent(table_h, "h", &where_)?;

    // Build the rectangular grid with merge flags for the writer/preview.
    let mut grid: Vec<Vec<TableSlot>> = vec![
        vec![
            TableSlot::Filler {
                hmerge: false,
                vmerge: false
            };
            cols
        ];
        row_count
    ];
    for (r, row) in cover.iter().enumerate() {
        for (c, slot) in row.iter().enumerate() {
            if let Some(origin_idx) = slot {
                let origin = &origins[*origin_idx];
                if origin.origin_row == r && origin.origin_col == c {
                    grid[r][c] = TableSlot::Origin(*origin_idx);
                } else {
                    grid[r][c] = TableSlot::Filler {
                        hmerge: origin.origin_col < c,
                        vmerge: origin.origin_row < r,
                    };
                }
            }
        }
    }

    // Effective cell styles and markdown runs.
    let named = &ctx.main.styles.named;
    let empty = toml::Table::new();
    let mut rows_json: Vec<Value> = Vec::new();
    for grid_row in &grid {
        let mut row_json = Vec::new();
        for slot in grid_row {
            match *slot {
                TableSlot::Filler { hmerge, vmerge } => {
                    row_json.push(json!({"hmerge": hmerge, "vmerge": vmerge}));
                }
                TableSlot::Origin(origin_idx) => {
                    let cell = &origins[origin_idx];
                    let mut effective = table_style.clone();
                    if let Some(style) =
                        col_style_opts.get(cell.origin_col).and_then(|s| s.as_ref())
                    {
                        effective.extend(style.clone());
                    }
                    if let Some(style) =
                        row_style_opts.get(cell.origin_row).and_then(|s| s.as_ref())
                    {
                        effective.extend(style.clone());
                    }
                    effective.extend(cell.opts.clone());
                    effective
                        .entry("font_size")
                        .or_insert(toml::Value::Integer(12));
                    effective
                        .entry("color")
                        .or_insert_with(|| toml::Value::String("000000".into()));
                    effective
                        .entry("valign")
                        .or_insert_with(|| toml::Value::String("top".into()));
                    if !effective.contains_key("margin") {
                        effective.insert(
                            "margin".into(),
                            toml::Value::Array(vec![
                                toml::Value::Float(0.05),
                                toml::Value::Float(0.1),
                                toml::Value::Float(0.05),
                                toml::Value::Float(0.1),
                            ]),
                        );
                    }
                    let border = effective.remove("border");
                    let border = table_border(border.as_ref(), &where_)?;
                    let mut options = to_pptxgen(&toml::Value::Table(effective));
                    options
                        .as_object_mut()
                        .unwrap()
                        .insert("border".into(), border);
                    let cell_where = format!(
                        "{where_} row {} column {}",
                        cell.origin_row + 1,
                        cell.origin_col + 1
                    );
                    let runs = flatten_runs(
                        &[TextPara {
                            text: &cell.text,
                            opts: &empty,
                            index: usize::MAX,
                            span: cell.span.clone(),
                        }],
                        &list,
                        named,
                        ctx.file_src,
                        &cell_where,
                    )?;
                    row_json.push(json!({
                        "colspan": cell.colspan,
                        "rowspan": cell.rowspan,
                        "runs": runs,
                        "options": options,
                    }));
                }
            }
        }
        rows_json.push(Value::Array(row_json));
    }

    let x = shape
        .x
        .as_ref()
        .map(|c| c.inches(ctx.width))
        .transpose()
        .map_err(|e| miette!("{where_}: invalid `x`: {e}"))?
        .unwrap_or(0.5);
    let y = shape
        .y
        .as_ref()
        .map(|c| c.inches(ctx.height))
        .transpose()
        .map_err(|e| miette!("{where_}: invalid `y`: {e}"))?
        .unwrap_or(0.5);
    let mut value = Map::new();
    value.insert("index".into(), json!(idx));
    value.insert("kind".into(), json!("table"));
    value.insert("x".into(), json!(x));
    value.insert("y".into(), json!(y));
    value.insert("w".into(), json!(table_w));
    value.insert("h".into(), json!(table_h));
    value.insert("colW".into(), json!(widths));
    value.insert("rowH".into(), json!(heights));
    value.insert("rows".into(), Value::Array(rows_json));
    if let Some(name) = object_name {
        value.insert("objectName".into(), json!(name));
    }
    Ok(Value::Object(value))
}

fn parse_table_cell(value: &toml::Value, where_: &str) -> Result<TableCellInput> {
    let mut cell = TableCellInput {
        colspan: 1,
        rowspan: 1,
        ..TableCellInput::default()
    };
    match value {
        toml::Value::String(s) => cell.text = s.clone(),
        toml::Value::Integer(i) => cell.text = i.to_string(),
        toml::Value::Float(f) => cell.text = f.to_string(),
        toml::Value::Table(t) => {
            if let Some(text) = t.get("text") {
                cell.text = match text {
                    toml::Value::String(s) => s.clone(),
                    toml::Value::Integer(i) => i.to_string(),
                    toml::Value::Float(f) => f.to_string(),
                    _ => {
                        return Err(miette!(
                            "{where_}: cell `text` must be a string or a number"
                        ));
                    }
                };
            }
            cell.opts = t.clone();
            cell.opts.remove("text");
            if let Some(v) = cell.opts.remove("colspan") {
                cell.colspan = parse_span(&v, where_, "colspan")?;
            }
            if let Some(v) = cell.opts.remove("rowspan") {
                cell.rowspan = parse_span(&v, where_, "rowspan")?;
            }
        }
        _ => {
            return Err(miette!(
                "{where_}: a table cell must be a string, a number or an inline table"
            ));
        }
    }
    Ok(cell)
}

fn parse_span(value: &toml::Value, where_: &str, key: &str) -> Result<usize> {
    match value {
        toml::Value::Integer(n) if *n >= 1 => Ok(*n as usize),
        _ => Err(miette!("{where_}: `{key}` must be a positive integer")),
    }
}

fn cell_text_span(value: &toml_edit::Value) -> Option<std::ops::Range<usize>> {
    match value {
        toml_edit::Value::InlineTable(t) => t
            .get("text")
            .and_then(|v| v.span())
            .or_else(|| value.span()),
        _ => value.span(),
    }
}

fn reject_auto_page(opts: &toml::Table, where_: &str) -> Result<()> {
    for key in opts.keys() {
        if key == "auto_page" || key.starts_with("auto_page_") {
            return Err(miette!(
                "{where_}: `{key}` is not supported; tables are static"
            ));
        }
    }
    Ok(())
}

fn table_coord_inches(
    value: &toml::Value,
    slide_emu: i64,
    axis: &str,
    where_: &str,
) -> Result<f64> {
    match value {
        toml::Value::Integer(emu) => Ok(*emu as f64 / EMU_PER_IN),
        toml::Value::String(text) => Coord::Text(text.clone())
            .inches(slide_emu)
            .map_err(|e| miette!("{where_}: invalid `{axis}`: {e}")),
        _ => Err(miette!(
            "{where_}: `{axis}` must be an EMU integer or a length string"
        )),
    }
}

fn resolve_columns(
    count: usize,
    widths: &[Option<f64>],
    total: Option<f64>,
    default_total: f64,
    where_: &str,
) -> Result<(Vec<f64>, f64)> {
    let fixed_count = widths.iter().filter(|w| w.is_some()).count();
    let fixed_sum: f64 = widths.iter().flatten().sum();
    if widths.iter().flatten().any(|w| *w < 0.0) {
        return Err(miette!("{where_}: column widths must not be negative"));
    }
    let total = match total {
        Some(t) => {
            if fixed_count == count && (fixed_sum - t).abs() > 1e-6 {
                return Err(miette!(
                    "{where_}: column widths sum to {fixed_sum}in but `w` is {t}in"
                ));
            }
            if fixed_count < count && fixed_count > 0 && t - fixed_sum <= 0.0 {
                return Err(miette!(
                    "{where_}: fixed column widths leave no room for the remaining columns"
                ));
            }
            t
        }
        None => {
            if fixed_count == count {
                fixed_sum
            } else {
                default_total
            }
        }
    };
    if total <= 0.0 {
        return Err(miette!("{where_}: table width must be positive"));
    }
    let missing = count - fixed_count;
    let each = if missing > 0 {
        let remaining = total - fixed_sum;
        if remaining <= 0.0 {
            return Err(miette!(
                "{where_}: fixed column widths exceed the table width"
            ));
        }
        remaining / missing as f64
    } else {
        0.0
    };
    let widths = widths.iter().map(|w| w.unwrap_or(each)).collect();
    Ok((widths, total))
}

fn resolve_rows(
    count: usize,
    heights: &[Option<f64>],
    total: Option<f64>,
    where_: &str,
) -> Result<(Vec<f64>, f64)> {
    let fixed_count = heights.iter().filter(|h| h.is_some()).count();
    let fixed_sum: f64 = heights.iter().flatten().sum();
    if heights.iter().flatten().any(|h| *h < 0.0) {
        return Err(miette!("{where_}: row heights must not be negative"));
    }
    let (mut heights, total) = match total {
        Some(t) => {
            if fixed_count == count && (fixed_sum - t).abs() > 1e-6 {
                return Err(miette!(
                    "{where_}: row heights sum to {fixed_sum}in but `h` is {t}in"
                ));
            }
            let missing = count - fixed_count;
            if missing > 0 && fixed_count > 0 && t - fixed_sum <= 0.0 {
                return Err(miette!(
                    "{where_}: fixed row heights leave no room for the remaining rows"
                ));
            }
            let each = if missing > 0 {
                let remaining = t - fixed_sum;
                if remaining <= 0.0 {
                    return Err(miette!(
                        "{where_}: fixed row heights exceed the table height"
                    ));
                }
                remaining / missing as f64
            } else {
                0.0
            };
            (
                heights
                    .iter()
                    .map(|h| h.unwrap_or(each))
                    .collect::<Vec<f64>>(),
                t,
            )
        }
        None => {
            let total = if fixed_count == count { fixed_sum } else { 0.0 };
            (
                heights
                    .iter()
                    .map(|h| h.unwrap_or(0.0))
                    .collect::<Vec<f64>>(),
                total,
            )
        }
    };
    if total < 0.0 {
        return Err(miette!("{where_}: table height must not be negative"));
    }
    Ok((std::mem::take(&mut heights), total))
}

fn table_border(value: Option<&toml::Value>, where_: &str) -> Result<Value> {
    let none = || json!({"type": "none"});
    let sides = match value {
        None => vec![none(), none(), none(), none()],
        Some(toml::Value::Table(t)) => {
            if t.keys().any(|k| BORDER_SIDES.contains(&k.as_str())) {
                let mut sides = Vec::new();
                for side in BORDER_SIDES {
                    sides.push(match t.get(*side) {
                        Some(v) => border_side(v, where_)?,
                        None => none(),
                    });
                }
                sides
            } else {
                let side = border_side(&toml::Value::Table(t.clone()), where_)?;
                vec![side.clone(), side.clone(), side.clone(), side]
            }
        }
        Some(toml::Value::Array(items)) => {
            if items.len() != 4 {
                return Err(miette!("{where_}: `border` needs exactly four sides"));
            }
            items
                .iter()
                .map(|v| border_side(v, where_))
                .collect::<Result<Vec<_>>>()?
        }
        _ => return Err(miette!("{where_}: invalid `border`")),
    };
    Ok(Value::Array(sides))
}

fn border_side(value: &toml::Value, where_: &str) -> Result<Value> {
    let table = value
        .as_table()
        .ok_or_else(|| miette!("{where_}: a border side must be a table"))?;
    let ty = table
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("solid");
    let mut out = Map::new();
    if ty == "none" {
        out.insert("type".into(), json!("none"));
        return Ok(Value::Object(out));
    }
    out.insert("type".into(), json!("solid"));
    out.insert(
        "color".into(),
        json!(
            table
                .get("color")
                .and_then(|v| v.as_str())
                .unwrap_or("666666")
        ),
    );
    let width = table
        .get("width")
        .and_then(|v| match v {
            toml::Value::Integer(i) => Some(*i as f64),
            toml::Value::Float(f) => Some(*f),
            _ => None,
        })
        .unwrap_or(1.0);
    out.insert("width".into(), json!(width));
    if let Some(dash) = table.get("dash_type").and_then(|v| v.as_str())
        && dash != "solid"
    {
        out.insert("dashType".into(), json!(dash));
    }
    for key in ["transparency", "alpha"] {
        if let Some(v) = table.get(key) {
            out.insert(key.into(), to_pptxgen(v));
        }
    }
    Ok(Value::Object(out))
}

/// A located diagnostic for a rich-text parse error, pointing at the `text`
/// value in the source slide file (`para_idx == usize::MAX` means the shape's
/// `text` shorthand, otherwise a `[[shapes.paragraphs]]` entry).
fn text_error(
    file_src: &FileSrc,
    span: Option<std::ops::Range<usize>>,
    where_: &str,
    msg: String,
) -> miette::Report {
    opts::located(span, &file_src.src(), format!("{where_}: {msg}"))
}

/// One text paragraph handed to the shared run builder: `index == usize::MAX`
/// marks the shape's `text` shorthand (so span errors point at the right
/// value), otherwise it's the `[[shapes.paragraphs]]` entry index.
struct TextPara<'a> {
    text: &'a str,
    opts: &'a toml::Table,
    index: usize,
    /// Byte span of the source `text` value, for located markdown errors.
    span: Option<std::ops::Range<usize>>,
}

/// Per-shape list configuration shared by slide and master text.
struct ListOptions {
    ordered: Vec<String>,
    unordered: Vec<String>,
    indent: Option<f64>,
}

/// Apply a style-level `bullet_indent` to a run's bullet. A paragraph's own
/// `bullet = { indent = ... }` wins.
fn apply_default_bullet_indent(bullet: &mut Value, indent: f64) {
    match bullet {
        Value::Bool(true) => *bullet = json!({ "indent": indent }),
        Value::Object(map) if !map.contains_key("indent") => {
            map.insert("indent".into(), json!(indent));
        }
        _ => {}
    }
}

/// Build the flat run list for text - pptxgenjs's native text model. Shared by
/// slide text and master/placeholder text so they behave identically.
///
/// The markdown paragraphs are flattened into one run list; the last run of
/// every paragraph except the final one carries `breakLine` (pptxgenjs closes
/// the paragraph *after* that run) and the first run of a paragraph carries
/// its own options (`bullet`, `indent_level`, `line_spacing`, ...). A markdown
/// single `\n` already became a `softBreakBefore` run. `align` is dropped from
/// paragraph options because pptxgenjs auto-splits paragraphs on `align`
/// changes, which would double-split with `breakLine`; use the shape-level
/// `align` instead. Empty input yields one empty run so `addText`/masters
/// always have content.
fn flatten_runs(
    paragraphs: &[TextPara<'_>],
    list: &ListOptions,
    styles: &BTreeMap<String, toml::Table>,
    file_src: &FileSrc,
    base_where: &str,
) -> Result<Value> {
    let where_ = |index: usize| -> String {
        if index == usize::MAX {
            base_where.to_string()
        } else {
            format!("{base_where} paragraph")
        }
    };
    struct Group {
        runs: Vec<richtext::Run>,
        opts: serde_json::Map<String, Value>,
        bullet_indent: Option<f64>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for para in paragraphs {
        let para_indent = match para.opts.get("bullet_indent") {
            Some(toml::Value::Integer(i)) => Some(*i as f64),
            Some(toml::Value::Float(f)) => Some(*f),
            _ => None,
        };
        let bullet_indent = para_indent.or(list.indent);
        let mut para_value = toml::Value::Table(para.opts.clone());
        opts::finalize_opts(Ctx::Text, &mut para_value)?;
        let para_opts = to_pptxgen(&para_value);
        let para_opts = para_opts
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| k.as_str() != "align" && k.as_str() != "bulletIndent")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Map<_, _>>();
        for runs in richtext::parse_paragraphs(para.text, &list.ordered, &list.unordered, styles)
            .map_err(|msg| text_error(file_src, para.span.clone(), &where_(para.index), msg))?
        {
            groups.push(Group {
                runs,
                opts: para_opts.clone(),
                bullet_indent,
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
                if let Some(indent) = group.bullet_indent
                    && let Some(bullet) = obj.get_mut("bullet")
                {
                    apply_default_bullet_indent(bullet, indent);
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

/// Build the run list for a slide text shape.
fn runs_value(ctx: &SlideCtx, idx: usize, shape: &Shape, list: &ListOptions) -> Result<Value> {
    let mut paragraphs: Vec<TextPara> = Vec::new();
    let empty = toml::Table::new();
    if !shape.paragraphs.is_empty() {
        for (j, para) in shape.paragraphs.iter().enumerate() {
            if !para.opts.is_empty() {
                opts::validate_ctx(
                    Ctx::Text,
                    &toml::Value::Table(para.opts.clone()),
                    &format!("slide `{}` shape `{}` paragraph", ctx.file, shape.ty),
                )?;
            }
            paragraphs.push(TextPara {
                text: &para.text,
                opts: &para.opts,
                index: j,
                span: ctx
                    .shape_table(idx)
                    .and_then(|t| t.get("paragraphs"))
                    .and_then(|i| i.as_array_of_tables())
                    .and_then(|a| a.get(j))
                    .and_then(|t| t.get("text"))
                    .and_then(|i| i.span()),
            });
        }
    } else if let Some(text) = &shape.text {
        paragraphs.push(TextPara {
            text,
            opts: &empty,
            index: usize::MAX,
            span: ctx
                .shape_table(idx)
                .and_then(|t| t.get("text"))
                .and_then(|i| i.span()),
        });
    }
    let base_where = format!("slide `{}` shape `{}`", ctx.file, shape.ty);
    flatten_runs(
        &paragraphs,
        list,
        &ctx.main.styles.named,
        ctx.file_src,
        &base_where,
    )
}

/// Load + base64-encode a media file referenced by an image shape.
fn image_value(
    project: &Project,
    file: &str,
    shape: &Shape,
    w_in: f64,
    h_in: f64,
) -> Result<Value> {
    let src = shape
        .src
        .as_deref()
        .ok_or_else(|| miette!("slide `{file}`: image shape needs `src`"))?;
    let (data, mime, preview) = media_value(project, src, Some((w_in * 96.0, h_in * 96.0)))?;
    let mut obj = Map::new();
    obj.insert("data".into(), json!(data));
    obj.insert("mime".into(), json!(mime));
    if let Some(preview) = preview {
        obj.insert("preview".into(), json!(preview));
    }
    if !shape.opts.contains_key("sizing") {
        obj.insert("sizing".into(), json!({ "type": "contain" }));
    }
    Ok(Value::Object(obj))
}

/// Media for an image shape: the data URI handed to pptxgenjs, its MIME, and
/// an optional rasterized PNG preview. SVG sources keep their vector data URI
/// (pptxgenjs embeds the SVG natively, rendered via `<asvg:svgBlip>` by
/// PowerPoint 2016+) and get a real PNG preview for older PowerPoint, which
/// the bridge shim feeds into pptxgenjs's preview slot.
fn media_value(
    project: &Project,
    src: &str,
    target_px: Option<(f64, f64)>,
) -> Result<(String, &'static str, Option<String>)> {
    let path = project.dir.join(src);
    let bytes =
        std::fs::read(&path).map_err(|e| miette!("cannot read image `{}`: {e}", path.display()))?;
    let mime = crate::media::mime_for(src)
        .map_err(|e| miette!("cannot read image `{}`: {e}", path.display()))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let data = format!("data:{mime};base64,{encoded}");
    let preview = if mime == "image/svg+xml" {
        let png = crate::media::svg_to_png(&bytes, target_px)
            .map_err(|e| miette!("cannot render image `{}`: {e}", path.display()))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png);
        Some(format!("data:image/png;base64,{b64}"))
    } else {
        None
    };
    Ok((data, mime, preview))
}

/// Merge `[styles.shape]` < `[styles.<type>]` < named styles < shape.opts
/// (later wins). `[styles.text]` is just the bucket for `type = "text"`;
/// text options shared by every shape belong in `[styles.shape]`. An unknown
/// named-style reference is an error so typos are caught.
fn merge_opts(
    main: &Main,
    ty: &str,
    styles: &[String],
    local: &toml::Table,
) -> Result<toml::Value> {
    let mut merged = main.style_chain(ty, styles)?;
    for (k, v) in local {
        merged.insert(k.clone(), v.clone());
    }
    Ok(toml::Value::Table(merged))
}

/// Remove the gwen-owned list options from a merged options table, returning
/// them (they configure markdown list rendering, not pptxgenjs).
fn take_list_options(merged: &mut toml::Value) -> ListOptions {
    let table = merged.as_table_mut().unwrap();
    let bullet_indent = match table.remove("bullet_indent") {
        Some(toml::Value::Integer(i)) => Some(i as f64),
        Some(toml::Value::Float(f)) => Some(f),
        _ => None,
    };
    let mut strings = |key: &str| -> Vec<String> {
        match table.remove(key) {
            Some(toml::Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        }
    };
    ListOptions {
        ordered: strings("ordered_markers"),
        unordered: strings("unordered_markers"),
        indent: bullet_indent,
    }
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

/// `a:ext` only accepts non-negative values; a negative extent cannot be
/// represented in OOXML and makes PowerPoint reject the deck.
fn reject_negative_extent(value: f64, axis: &str, where_: &str) -> Result<()> {
    if value < 0.0 {
        return Err(miette!(
            "{where_}: `{axis}` must not be negative (got {value})"
        ));
    }
    Ok(())
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

/// The `w`/`h` box (inches) for an image shape. When no `sizing` option is
/// set and exactly one of `w`/`h` is given, the other dimension is derived
/// from the source image's intrinsic aspect ratio; neither given is an error.
/// With `sizing` (or both dimensions), values pass through as today.
fn image_box(
    w: &Option<Coord>,
    h: &Option<Coord>,
    slide: (i64, i64),
    sizing: bool,
    src: Option<&str>,
    project_dir: &Path,
    where_: &str,
) -> Result<(f64, f64)> {
    if sizing || (w.is_some() && h.is_some()) {
        return Ok((geo(w, slide.0, "w")?, geo(h, slide.1, "h")?));
    }
    let src = src.ok_or_else(|| miette!("{where_}: image shape needs `src`"))?;
    match (w, h) {
        (None, None) => Err(miette!("{where_}: image `{src}` needs `w` or `h`")),
        (Some(_), None) => {
            let w_in = geo(w, slide.0, "w")?;
            let aspect = crate::media::image_aspect(&project_dir.join(src))
                .map_err(|e| miette!("{where_}: {e}"))?;
            Ok((w_in, w_in / aspect))
        }
        (None, Some(_)) => {
            let h_in = geo(h, slide.1, "h")?;
            let aspect = crate::media::image_aspect(&project_dir.join(src))
                .map_err(|e| miette!("{where_}: {e}"))?;
            Ok((h_in * aspect, h_in))
        }
        (Some(_), Some(_)) => unreachable!(),
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
