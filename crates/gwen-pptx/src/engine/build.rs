use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::dto::ShapeDto;
use crate::error::{AppError, AppResult};
use crate::opc::{Package, Relationship};

use super::generate;

const PRES_CT: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const SLIDE_CT: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const NOTES_SLIDE_CT: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
const MASTER_CT: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
const LAYOUT_CT: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml";
const THEME_CT: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const CHART_CT: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const NOTES_MASTER_CT: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
const CORE_CT: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const APP_CT: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const OFFICE_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const CORE_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const EXT_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const THUMB_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const SLIDE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const LAYOUT_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
const MASTER_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
const NOTES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
const NOTES_MASTER_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";
const THEME_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const CHART_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";

/// Everything the compiler needs from a project directory.
pub struct Project<'a> {
    pub doc: &'a Value,
    /// `src/media`: extracted image files keyed by basename.
    pub media_dir: Option<&'a Path>,
}

fn walk_dir(dir: &Path) -> AppResult<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// Compile a `.pptx` package from the parsed project document and its media.
pub fn compile_package(project: &Project<'_>) -> AppResult<Package> {
    let mut pkg = Package::empty();

    let obj = project
        .doc
        .as_object()
        .ok_or_else(|| AppError::InvalidValue("project document must be an object".to_string()))?;
    let width = obj.get("slide_width").and_then(Value::as_i64).unwrap_or(0);
    let height = obj.get("slide_height").and_then(Value::as_i64).unwrap_or(0);

    let masters = doc_masters(obj);
    let slides = doc_slides(obj);

    let mut chart_counter = next_chart_number(&pkg);

    for master in &masters {
        compile_master(&mut pkg, master)?;
    }
    for master in &masters {
        for layout in &master.layouts {
            compile_layout(&mut pkg, layout)?;
            add_rel(&mut pkg, &layout.uri, &master.uri, MASTER_REL);
        }
    }

    // Media extracted by the mirror (`src/media`) become `ppt/media/<name>`.
    // Picture shapes may only reference files that exist here.
    let mut media_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(media_dir) = project.media_dir.filter(|d| d.is_dir()) {
        for path in walk_dir(media_dir)? {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                media_files.insert(name.to_string());
                pkg.set_part(&format!("ppt/media/{name}"), std::fs::read(&path)?);
            }
        }
    }

    for slide in &slides {
        compile_slide(&mut pkg, slide, &masters, &media_files, &mut chart_counter)?;
    }

    // Notes slides need their master; create it once before relationship
    // pruning so the notesSlide -> notesMaster rels resolve.
    if slides.iter().any(|s| s.notes.is_some()) {
        compile_notes_master(&mut pkg)?;
    }

    compile_presentation(&mut pkg, obj, &masters, &slides)?;
    compile_theme(&mut pkg, obj)?;
    compile_core_props(&mut pkg, obj)?;
    compile_package_rels(&mut pkg)?;
    compile_content_types(&mut pkg)?;

    prune_dangling_relationships(&mut pkg);

    let _ = width;
    let _ = height;
    Ok(pkg)
}

/// Drop relationships whose internal target part is no longer part of the
/// package, e.g. after a slide or layout was deleted from the mirror. External
/// targets are always kept.
fn prune_dangling_relationships(pkg: &mut Package) {
    let dangling: Vec<(String, String)> = {
        let sources: Vec<String> = pkg.rels_uris().map(|(s, _)| s.clone()).collect();
        let mut out = Vec::new();
        for source in sources {
            let rels: Vec<(String, String)> = pkg
                .get_rels(&source)
                .map(|rels| {
                    rels.values()
                        .filter(|r| {
                            r.target_mode.as_deref() != Some("External")
                                && resolve_rel_target(&source, r)
                                    .is_some_and(|t| !pkg.part_exists(&t))
                        })
                        .map(|r| (r.id.clone(), r.target.clone()))
                        .collect()
                })
                .unwrap_or_default();
            out.extend(rels.into_iter().map(|(id, _)| (source.clone(), id)));
        }
        out
    };
    for (source, id) in dangling {
        pkg.remove_relationship(&source, &id);
    }
}

struct DocMaster {
    uri: String,
    layouts: Vec<DocLayout>,
    shapes: Vec<Value>,
}

struct DocLayout {
    uri: String,
    /// Package-unique `p:sldLayoutId` value (>= 2147483648, distinct from the
    /// master ids).
    id: u32,
    shapes: Vec<Value>,
}

struct DocSlide {
    uri: String,
    shapes: Vec<Value>,
    background: Value,
    notes: Option<Vec<Value>>,
    /// (master index, layout index) resolved from the mirrored `slide_layout`.
    layout: Option<(usize, usize)>,
}

fn doc_masters(obj: &Map<String, Value>) -> Vec<DocMaster> {
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    if let Some(list) = obj.get("slide_masters").and_then(Value::as_array) {
        for (i, m) in list.iter().enumerate() {
            let fallback = next_uri("ppt/slideMasters/slideMaster", i + 1, &mut used);
            let uri = m
                .get("uri")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or(fallback);
            used.insert(uri.clone(), 1);
            let shapes = m
                .get("shapes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut layouts = Vec::new();
            if let Some(ll) = m.get("slide_layouts").and_then(Value::as_array) {
                for (j, l) in ll.iter().enumerate() {
                    let fallback = next_uri("ppt/slideLayouts/slideLayout", j + 1, &mut used);
                    let luri = l
                        .get("uri")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or(fallback);
                    used.insert(luri.clone(), 1);
                    let lshapes = l
                        .get("shapes")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    layouts.push(DocLayout {
                        uri: luri,
                        id: 0,
                        shapes: lshapes,
                    });
                }
            }
            out.push(DocMaster {
                uri,
                layouts,
                shapes,
            });
        }
    }
    // Layout ids live in the same 2147483648+ range as master ids; offset past
    // every master so the two lists never collide.
    let mut next_id = 2147483648u32 + out.len() as u32;
    for master in &mut out {
        for layout in &mut master.layouts {
            layout.id = next_id;
            next_id += 1;
        }
    }
    out
}

fn doc_slides(obj: &Map<String, Value>) -> Vec<DocSlide> {
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    if let Some(list) = obj.get("slides").and_then(Value::as_array) {
        for (i, s) in list.iter().enumerate() {
            let fallback = next_uri("ppt/slides/slide", i + 1, &mut used);
            let uri = s
                .get("uri")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or(fallback);
            used.insert(uri.clone(), 1);
            let shapes = s
                .get("shapes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let background = s
                .get("background")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or(Value::Null);
            let notes = s
                .get("notes")
                .filter(|v| !v.is_null())
                .and_then(|n| n.get("shapes").and_then(Value::as_array).cloned());
            let layout = s
                .get("slide_layout")
                .and_then(Value::as_object)
                .and_then(|o| {
                    let m = o.get("master").and_then(Value::as_i64)?;
                    let l = o.get("layout").and_then(Value::as_i64)?;
                    Some((m as usize, l as usize))
                });
            out.push(DocSlide {
                uri,
                shapes,
                background,
                notes,
                layout,
            });
        }
    }
    out
}

fn next_uri(prefix: &str, start: usize, used: &mut HashMap<String, usize>) -> String {
    let mut n = start;
    loop {
        let uri = format!("{prefix}{n}.xml");
        if !used.contains_key(&uri) {
            return uri;
        }
        n += 1;
    }
}

fn next_chart_number(pkg: &Package) -> usize {
    let mut max = 0usize;
    for uri in pkg.part_uris() {
        if let Some(rest) = uri.strip_prefix("ppt/charts/chart")
            && let Some(num) = rest.strip_suffix(".xml")
            && let Ok(n) = num.parse::<usize>()
            && n > max
        {
            max = n;
        }
    }
    max
}

fn as_shapes(values: &[Value]) -> AppResult<Vec<ShapeDto>> {
    let mut shapes = Vec::new();
    for v in values {
        let mut shape: ShapeDto = serde_json::from_value(v.clone())
            .map_err(|e| AppError::InvalidValue(format!("invalid shape definition: {e}")))?;
        assign_shape_ids(&mut shape, &mut 2);
        derive_text_frames(&mut shape);
        shapes.push(shape);
    }
    Ok(shapes)
}

/// Derive `has_text_frame` from the presence of text (the mirror never emits
/// the field; an explicit empty text frame is still one).
fn derive_text_frames(shape: &mut ShapeDto) {
    shape.has_text_frame = shape.text_frame.is_some();
    if let Some(children) = &mut shape.shapes {
        for child in children {
            derive_text_frames(child);
        }
    }
}

/// Assign unique shape ids to every shape (and group child) with `shape_id ==
/// 0`. The counter starts past the group id (1).
fn assign_shape_ids(shape: &mut ShapeDto, next: &mut u32) {
    if shape.shape_id == 0 {
        shape.shape_id = *next;
        *next += 1;
    }
    if let Some(children) = &mut shape.shapes {
        for child in children {
            assign_shape_ids(child, next);
        }
    }
}

/// The default head/sp/post used for regenerated parts that have no captured
/// fragment (i.e. brand-new slides, masters, layouts, notes).
const DEFAULT_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld>";
const MASTER_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:sldMaster xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld>";
const LAYOUT_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:sldLayout xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" preserve=\"1\"><p:cSld>";
const NOTES_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:notesSlide xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld>";
const NOTES_MASTER_URI: &str = "ppt/notesMasters/notesMaster1.xml";
/// The two placeholders every notes master owns: the slide image and the body.
const NOTES_MASTER_BODY: &str = "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp>";
const CLR_MAP: &str = "<a:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>";
const DEFAULT_SP: &str = "<p:spTree>";
const DEFAULT_POST: &str = "</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>";

fn assemble(head: &[u8], mid: &[u8], sp: &[u8], inner: &[u8], post: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(head);
    out.extend_from_slice(mid);
    out.extend_from_slice(sp);
    out.extend_from_slice(inner);
    out.extend_from_slice(b"</p:spTree>");
    out.extend_from_slice(post);
    out
}

fn compile_master(pkg: &mut Package, master: &DocMaster) -> AppResult<()> {
    let shapes = as_shapes(&master.shapes)?;
    let inner = generate::sp_tree_body(&shapes).into_bytes();
    let head = MASTER_HEAD.as_bytes();
    let sp = DEFAULT_SP.as_bytes();

    add_rel(pkg, &master.uri, "ppt/theme/theme1.xml", THEME_REL);
    // The master must list every layout it owns; ids come from the
    // package-unique allocation in `doc_masters`.
    let mut entries = String::new();
    for layout in &master.layouts {
        let rid = add_rel(pkg, &master.uri, &layout.uri, LAYOUT_REL);
        entries.push_str(&format!(
            "<p:sldLayoutId id=\"{}\" r:id=\"{rid}\"/>",
            layout.id
        ));
    }
    let post =
        format!("</p:cSld>{CLR_MAP}<p:sldLayoutIdLst>{entries}</p:sldLayoutIdLst></p:sldMaster>");
    pkg.set_part(
        &master.uri,
        assemble(head, b"", sp, &inner, post.as_bytes()),
    );
    Ok(())
}

fn compile_layout(pkg: &mut Package, layout: &DocLayout) -> AppResult<()> {
    let shapes = as_shapes(&layout.shapes)?;
    let inner = generate::sp_tree_body(&shapes).into_bytes();
    let head = LAYOUT_HEAD.as_bytes();
    let post = b"</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>";
    pkg.set_part(
        &layout.uri,
        assemble(head, b"", DEFAULT_SP.as_bytes(), &inner, post),
    );
    Ok(())
}

fn compile_slide(
    pkg: &mut Package,
    slide: &DocSlide,
    masters: &[DocMaster],
    media_files: &std::collections::HashSet<String>,
    chart_counter: &mut usize,
) -> AppResult<()> {
    let uri = &slide.uri;

    // Pictures: ensure a rel exists for every image filename.
    let mut image_rids: HashMap<String, String> = HashMap::new();
    let mut shapes = as_shapes(&slide.shapes)?;
    collect_image_files(&mut shapes, &mut |shape_name, fname| {
        if !media_files.contains(fname) {
            return Err(AppError::InvalidValue(format!(
                "picture shape `{shape_name}` references `src/media/{fname}`, which does not exist"
            )));
        }
        if !image_rids.contains_key(fname) {
            let rid = add_image_rel(pkg, uri, fname);
            image_rids.insert(fname.to_string(), rid);
        }
        Ok(())
    })?;

    // Charts: assign a part URI + rel for every chart shape, and point the
    // shape's `chart.r_id` at the real relationship id.
    collect_charts(&mut shapes, &mut |shape| {
        let chart_uri = format!("ppt/charts/chart{}.xml", *chart_counter + 1);
        *chart_counter += 1;
        let rid = add_chart_rel(pkg, uri, &chart_uri);
        if let Some(chart) = &mut shape.chart {
            chart.r_id = Some(rid);
        }
        let xml = generate::chart_xml(shape.chart.as_ref().expect("chart present"))?;
        pkg.set_part(&chart_uri, xml);
        Ok(())
    })?;

    // Notes slide. The notes master part is created once after the slide loop
    // (before relationship pruning), but its URI is fixed so the notes slide
    // can reference it here.
    if let Some(notes_shapes) = &slide.notes {
        let notes_uri = format!("ppt/notesSlides/notesSlide{}.xml", pkg.get_next_notes_num());
        add_rel(pkg, uri, &notes_uri, NOTES_REL);
        compile_notes(pkg, &notes_uri, uri, notes_shapes)?;
    }

    // Slide layout relationship.
    if let Some((m, l)) = slide.layout
        && let Some(master) = masters.get(m)
        && let Some(layout) = master.layouts.get(l)
    {
        add_rel(pkg, uri, &layout.uri, LAYOUT_REL);
    }

    let inner = generate::sp_tree_body(&shapes).into_bytes();

    let head = DEFAULT_HEAD.as_bytes();
    let sp = DEFAULT_SP.as_bytes();
    let post = DEFAULT_POST.as_bytes();

    // Background: a solid fill in the mirror regenerates `p:bg`; anything else
    // is left without an explicit background (inherited from the layout).
    let generated_bg = generate::slide_background_xml(&slide.background);
    let mid = generated_bg.as_deref().unwrap_or(b"");

    let mut bytes = assemble(head, mid, sp, &inner, post);

    // Rewrite picture `r:embed="<filename>"` to the assigned rel id.
    for (fname, rid) in &image_rids {
        let from = format!("r:embed=\"{fname}\"");
        let to = format!("r:embed=\"{rid}\"");
        if from != to {
            bytes = replace_all(&bytes, from.as_bytes(), to.as_bytes());
        }
    }

    pkg.set_part(uri, bytes);
    Ok(())
}

fn compile_notes(
    pkg: &mut Package,
    notes_uri: &str,
    slide_uri: &str,
    shapes: &[Value],
) -> AppResult<()> {
    // A notes slide links back to its slide and to the shared notes master.
    add_rel(pkg, notes_uri, slide_uri, SLIDE_REL);
    add_rel(pkg, notes_uri, NOTES_MASTER_URI, NOTES_MASTER_REL);
    let shapes = as_shapes(shapes)?;
    let inner = generate::sp_tree_body(&shapes).into_bytes();
    let head = NOTES_HEAD.as_bytes();
    let post = b"</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notesSlide>";
    pkg.set_part(
        notes_uri,
        assemble(head, b"", DEFAULT_SP.as_bytes(), &inner, post),
    );
    Ok(())
}

/// The one notes master all notes slides share: the standard two placeholders
/// (slide image + body), a color map and the theme relationship.
fn compile_notes_master(pkg: &mut Package) -> AppResult<()> {
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:notesMaster xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld>";
    let inner = format!(
        "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{NOTES_MASTER_BODY}"
    );
    let bytes = assemble(
        head.as_bytes(),
        b"",
        DEFAULT_SP.as_bytes(),
        inner.as_bytes(),
        format!("</p:cSld>{CLR_MAP}</p:notesMaster>").as_bytes(),
    );
    pkg.set_part(NOTES_MASTER_URI, bytes);
    add_rel(pkg, NOTES_MASTER_URI, "ppt/theme/theme1.xml", THEME_REL);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn compile_presentation(
    pkg: &mut Package,
    obj: &Map<String, Value>,
    masters: &[DocMaster],
    slides: &[DocSlide],
) -> AppResult<()> {
    let pres_uri = "ppt/presentation.xml";

    let mut master_entries = Vec::new();
    for (i, m) in masters.iter().enumerate() {
        let r_id = add_rel(pkg, pres_uri, &m.uri, MASTER_REL);
        master_entries.push(generate::ListEntry {
            id: (2147483648u32 + i as u32),
            r_id,
        });
    }

    let mut slide_entries = Vec::new();
    for (i, s) in slides.iter().enumerate() {
        let r_id = add_rel(pkg, pres_uri, &s.uri, SLIDE_REL);
        slide_entries.push(generate::ListEntry {
            id: 256 + i as u32,
            r_id,
        });
    }

    add_rel(pkg, pres_uri, "ppt/theme/theme1.xml", THEME_REL);
    let notes_master_r_id = if pkg.part_exists(NOTES_MASTER_URI) {
        Some(add_rel(pkg, pres_uri, NOTES_MASTER_URI, NOTES_MASTER_REL))
    } else {
        None
    };

    let width = obj.get("slide_width").and_then(Value::as_i64).unwrap_or(0);
    let height = obj.get("slide_height").and_then(Value::as_i64).unwrap_or(0);

    let xml = generate::presentation_xml(
        &master_entries,
        notes_master_r_id.as_deref(),
        &slide_entries,
        width,
        height,
    );
    pkg.set_part(pres_uri, xml);
    Ok(())
}

fn compile_theme(pkg: &mut Package, obj: &Map<String, Value>) -> AppResult<()> {
    let uri = "ppt/theme/theme1.xml";
    let theme = obj
        .get("theme")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let colors = theme
        .get("colors")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let fonts = theme
        .get("fonts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let xml = generate::theme_xml(&colors, &fonts);
    pkg.set_part(uri, xml);
    Ok(())
}

fn compile_core_props(pkg: &mut Package, obj: &Map<String, Value>) -> AppResult<()> {
    let props = obj.get("core_properties").cloned().unwrap_or(Value::Null);
    let xml = generate::core_props_xml(&props);
    pkg.set_part("docProps/core.xml", xml);
    Ok(())
}

fn compile_package_rels(pkg: &mut Package) -> AppResult<()> {
    add_rel(pkg, "", "ppt/presentation.xml", OFFICE_REL);
    add_rel(pkg, "", "docProps/core.xml", CORE_REL);
    if pkg.part_exists("docProps/app.xml") {
        add_rel(pkg, "", "docProps/app.xml", EXT_REL);
    }
    if pkg.part_exists("docProps/thumbnail.jpeg") {
        add_rel(pkg, "", "docProps/thumbnail.jpeg", THUMB_REL);
    }
    Ok(())
}

/// Build `[Content_Types].xml` from the final part list. Media extensions are
/// covered by `Default` entries; every part must end up covered — an unknown
/// media extension is a hard error, because a part without a content type
/// makes the whole package invalid.
fn compile_content_types(pkg: &mut Package) -> AppResult<()> {
    let mut entries = Vec::new();
    let mut defaults: Vec<(&str, &str)> = Vec::new();
    let mut media_exts: Vec<String> = Vec::new();
    let mut uris: Vec<&String> = pkg.part_uris().collect();
    uris.sort();
    for uri in &uris {
        if let Some(name) = uri.strip_prefix("ppt/media/") {
            let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            if !media_exts.contains(&ext) {
                media_exts.push(ext);
            }
        }
        entries.push(generate::PartEntry {
            uri: (*uri).clone(),
            content_type: known_content_type(uri),
        });
    }
    for ext in &media_exts {
        if let Some(ct) = media_content_type(ext) {
            defaults.push((ext.as_str(), ct));
        }
    }
    let xml = generate::content_types_xml(&entries, &defaults);

    // Compute coverage before handing the content types part back to the
    // package (the URI list borrows it).
    let uncovered: Vec<String> = uris
        .iter()
        .filter(|uri| !part_is_covered(uri, &defaults))
        .map(|uri| (*uri).clone())
        .collect();
    if !uncovered.is_empty() {
        let list = uncovered.join(", ");
        return Err(AppError::InvalidValue(format!(
            "no known content type for: {list} (unsupported file extension)"
        )));
    }

    pkg.set_part("[Content_Types].xml", xml);
    Ok(())
}

/// True when the OPC consumer can determine the part's content type via a
/// Default extension entry or a known Override.
fn part_is_covered(uri: &str, defaults: &[(&str, &str)]) -> bool {
    if known_content_type(uri).is_some() {
        return true;
    }
    let ext = uri.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    defaults.iter().any(|(e, _)| *e == ext)
        || ext == "rels"
        || (ext == "xml" && uri != "[Content_Types].xml")
}

fn media_content_type(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpeg" | "jpg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        "wmf" => Some("image/x-wmf"),
        "emf" => Some("image/x-emf"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

fn known_content_type(uri: &str) -> Option<String> {
    let numbered = |prefix: &str, ct: &str| {
        uri.strip_prefix(prefix)
            .and_then(|r| r.strip_suffix(".xml"))
            .filter(|n| n.chars().all(|c| c.is_ascii_digit()))
            .map(|_| ct.to_string())
    };
    if uri == "ppt/presentation.xml" {
        return Some(PRES_CT.to_string());
    }
    if uri == "docProps/core.xml" {
        return Some(CORE_CT.to_string());
    }
    if uri == "docProps/app.xml" {
        return Some(APP_CT.to_string());
    }
    if uri == NOTES_MASTER_URI {
        return Some(NOTES_MASTER_CT.to_string());
    }
    if let Some(ct) = numbered("ppt/slides/slide", SLIDE_CT) {
        return Some(ct);
    }
    if let Some(ct) = numbered("ppt/notesSlides/notesSlide", NOTES_SLIDE_CT) {
        return Some(ct);
    }
    if let Some(ct) = numbered("ppt/slideMasters/slideMaster", MASTER_CT) {
        return Some(ct);
    }
    if let Some(ct) = numbered("ppt/slideLayouts/slideLayout", LAYOUT_CT) {
        return Some(ct);
    }
    if let Some(ct) = numbered("ppt/theme/theme", THEME_CT) {
        return Some(ct);
    }
    if let Some(ct) = numbered("ppt/charts/chart", CHART_CT) {
        return Some(ct);
    }
    None
}

/// Add an internal relationship whose target is a package URI. OPC targets
/// are interpreted *relative to the source part*, so the absolute package URI
/// is rewritten to a proper `../`-style relative path first.
fn add_rel(pkg: &mut Package, source: &str, target: &str, rel_type: &str) -> String {
    let relative = rel_target_path(source, target);
    pkg.add_relationship(
        source,
        Relationship {
            id: String::new(),
            target: relative,
            target_mode: None,
            rel_type: rel_type.to_string(),
        },
    )
}

fn add_image_rel(pkg: &mut Package, source: &str, fname: &str) -> String {
    add_rel(pkg, source, &format!("ppt/media/{fname}"), IMAGE_REL)
}

fn add_chart_rel(pkg: &mut Package, source: &str, chart_uri: &str) -> String {
    add_rel(pkg, source, chart_uri, CHART_REL)
}

/// Resolve a relationship target to an absolute package URI. Never fails on
/// missing parts (unlike `Package::resolve_relationship_target`).
fn resolve_rel_target(source: &str, rel: &Relationship) -> Option<String> {
    if rel.target_mode.as_deref() == Some("External") {
        return None;
    }
    if rel.target.starts_with('/') {
        return Some(rel.target.trim_start_matches('/').to_string());
    }
    let base_dir = source.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut segments: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    for seg in rel.target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(seg),
        }
    }
    Some(segments.join("/"))
}

/// Compute a relative target path from `source` to `target`.
fn rel_target_path(source: &str, target: &str) -> String {
    let src_dir = source.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let src: Vec<&str> = if src_dir.is_empty() {
        Vec::new()
    } else {
        src_dir.split('/').collect()
    };
    let tgt: Vec<&str> = target.split('/').collect();
    let mut common = 0usize;
    while common < src.len() && common < tgt.len() && src[common] == tgt[common] {
        common += 1;
    }
    let mut out: Vec<&str> = vec![".."; src.len() - common];
    out.extend_from_slice(&tgt[common..]);
    if out.is_empty() {
        ".".to_string()
    } else {
        out.join("/")
    }
}

fn replace_all(haystack: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(pos) = find_subslice(rest, from) {
        out.extend_from_slice(&rest[..pos]);
        out.extend_from_slice(to);
        rest = &rest[pos + from.len()..];
    }
    out.extend_from_slice(rest);
    out
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn collect_image_files(
    shapes: &mut [ShapeDto],
    f: &mut dyn FnMut(&str, &str) -> AppResult<()>,
) -> AppResult<()> {
    for shape in shapes {
        if let Some(img) = &shape.image {
            let name = shape.name.as_deref().unwrap_or("");
            f(name, img)?;
        }
        if let Some(children) = &mut shape.shapes {
            collect_image_files(children, f)?;
        }
    }
    Ok(())
}

fn collect_charts(
    shapes: &mut [ShapeDto],
    f: &mut dyn FnMut(&mut ShapeDto) -> AppResult<()>,
) -> AppResult<()> {
    for shape in shapes {
        if shape.chart.is_some() {
            f(shape)?;
        }
        if let Some(children) = &mut shape.shapes {
            collect_charts(children, f)?;
        }
    }
    Ok(())
}
