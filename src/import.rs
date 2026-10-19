//! Convert an existing `.pptx` into a gwen TOML project: presentation size,
//! theme fonts, sections, slide masters (flattened per layout) and slides with
//! shapes, geometry, fills/lines, text (as markdown), images and notes.
//!
//! OOXML features gwen cannot express (charts, tables, groups are flattened;
//! gradients, per-run fonts/colors) are skipped or approximated with a warning.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};

use miette::{Result, miette};
use quick_xml::events::Event;

use toml::Value as T;

const EMU_PER_IN: f64 = 914_400.0;

/// Placeholder identity -> geometry, used to give slide placeholders the
/// position they inherit from the layout.
type PhGeo = HashMap<String, (i64, i64, i64, i64)>;

/// A theme's resolved `<a:clrScheme>` colours and `<a:fontScheme>` typefaces,
/// used to turn OOXML scheme references (`tx1`, `+mn-lt`, ...) into concrete
/// hex colours and real font names.
#[derive(Debug, Default)]
struct ThemeInfo {
    /// Scheme colour name (`tx1`, `dk1`, `accent1`, ...) -> hex.
    colors: HashMap<String, String>,
    /// OOXML theme font token (`+mn-lt`, `+mj-ea`, ...) -> typeface.
    fonts: HashMap<String, String>,
}

fn parse_theme(xml: &XNode) -> ThemeInfo {
    let elements = xml.child("themeElements");
    let mut info = ThemeInfo::default();
    if let Some(cs) = elements.and_then(|e| e.child("clrScheme")) {
        for child in &cs.children {
            let hex = child
                .child("srgbClr")
                .and_then(|c| c.attr("val"))
                .or_else(|| child.child("sysClr").and_then(|c| c.attr("lastClr")))
                .map(|c| c.to_ascii_uppercase());
            if let Some(hex) = hex {
                info.colors.insert(child.name.clone(), hex);
            }
        }
        // The text/background scheme names alias the dark/light colours.
        for (alias, target) in [
            ("tx1", "dk1"),
            ("tx2", "dk2"),
            ("bg1", "lt1"),
            ("bg2", "lt2"),
        ] {
            if let Some(hex) = info.colors.get(target) {
                info.colors.insert(alias.into(), hex.clone());
            }
        }
    }
    if let Some(fs) = elements.and_then(|e| e.child("fontScheme")) {
        for (node, prefix) in [(fs.child("majorFont"), "j"), (fs.child("minorFont"), "n")] {
            if let Some(node) = node {
                for (script, tag) in [("latin", "lt"), ("ea", "ea"), ("cs", "cs")] {
                    if let Some(face) = node.child(script).and_then(|f| f.attr("typeface")) {
                        info.fonts
                            .insert(format!("+m{prefix}-{tag}"), face.to_string());
                    }
                }
            }
        }
    }
    info
}

/// A parsed XML element: local (namespace-stripped) name, raw attribute keys,
/// child elements and accumulated text.
#[derive(Debug, Clone)]
struct XNode {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XNode>,
    text: String,
}

impl XNode {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn child(&self, name: &str) -> Option<&XNode> {
        self.children.iter().find(|c| c.name == name)
    }

    fn children(&self, name: &str) -> impl Iterator<Item = &XNode> {
        self.children.iter().filter(move |c| c.name == name)
    }
}

/// Strip the namespace prefix from an element name (`p:sp` -> `sp`).
fn local(name: &str) -> String {
    match name.rfind(':') {
        Some(p) => name[p + 1..].to_string(),
        None => name.to_string(),
    }
}

/// Resolve an XML entity reference (`&amp;`, `&#x41;`, `&#65;`) carried by a
/// `GeneralRef` event into the character it stands for.
fn resolve_entity(r: &quick_xml::events::BytesRef) -> String {
    let name = r.as_ref();
    match name {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        n if n.starts_with("#x") || n.starts_with("#X") => n[2..]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|c| c.to_string())
            .unwrap_or_default(),
        n if n.starts_with('#') => n[1..]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|c| c.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn parse_xml(bytes: &[u8]) -> Result<XNode> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut root: Option<XNode> = None;
    let mut stack: Vec<XNode> = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let node = node_from_start(&e);
                if stack.is_empty() {
                    root = Some(node.clone());
                }
                stack.push(node);
            }
            Ok(Event::End(_)) => {
                if let Some(node) = stack.pop() {
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => root = Some(node),
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let node = node_from_start(&e);
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(top) = stack.last_mut() {
                    top.text
                        .push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(top) = stack.last_mut() {
                    top.text
                        .push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&resolve_entity(&r));
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }
    root.ok_or_else(|| miette!("cannot parse XML: empty document"))
}

fn node_from_start(e: &quick_xml::events::BytesStart) -> XNode {
    let name = local(e.name().as_ref());
    let mut attrs = Vec::new();
    for a in e.attributes().flatten() {
        // Attribute keys keep their namespace prefix (`r:id`), unlike element
        // names which are matched by their local part.
        let key = a.key.as_ref().to_string();
        let value = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map(|c| c.into_owned())
            .unwrap_or_default();
        attrs.push((key, value));
    }
    XNode {
        name,
        attrs,
        children: Vec::new(),
        text: String::new(),
    }
}

/// Read every part of the zip container into `name -> bytes`.
fn read_zip(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| miette!("cannot open the pptx package: {e}"))?;
    let mut parts = BTreeMap::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| miette!("cannot read the pptx package: {e}"))?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().map(|c| c.into_owned()).unwrap_or_default();
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|e| miette!("cannot read `{name}`: {e}"))?;
        parts.insert(name, data);
    }
    Ok(parts)
}

/// The `rId -> target` map of a part's `.rels` file (targets resolved to the
/// package path, external URLs passed through).
fn rels_of(parts: &BTreeMap<String, Vec<u8>>, part: &str) -> Result<HashMap<String, String>> {
    let dir = Path::new(part).parent().unwrap_or(Path::new(""));
    let file = Path::new(part).file_name().unwrap_or_default();
    let rels_name = format!("{}/_rels/{}.rels", dir.display(), file.to_string_lossy());
    let Some(bytes) = parts.get(&rels_name) else {
        return Ok(HashMap::new());
    };
    let doc = parse_xml(bytes)?;
    let mut map = HashMap::new();
    for rel in doc.children("Relationship") {
        let id = rel.attr("Id").unwrap_or_default();
        let target = rel.attr("Target").unwrap_or_default();
        map.insert(id.to_string(), resolve_target(target, dir));
    }
    Ok(map)
}

/// Resolve a rel target (relative to `base`) to a package path.
fn resolve_target(target: &str, base: &Path) -> String {
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with("#")
    {
        return target.to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    for comp in base.join(target).components() {
        match comp {
            std::path::Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::CurDir => {}
            _ => {}
        }
    }
    parts.join("/")
}

/// Everything shared while converting one presentation.
struct Ptx {
    parts: BTreeMap<String, Vec<u8>>,
    rels: HashMap<String, String>,
    media: HashMap<String, String>, // package part -> gwen media basename
    used_media: BTreeSet<String>,
    master_files: Vec<(String, String)>, // (file stem, toml)
    /// Per layout: placeholder identity -> geometry, for slide placeholders
    /// that inherit their position from the layout.
    layout_ph_geo: HashMap<String, PhGeo>,
    /// Generated `[styles.named.runN]` styles for per-run text formatting,
    /// referenced by `<span class="runN">` in imported slide text.
    run_styles: BTreeMap<String, toml::Table>,
    /// Canonical run-option key -> generated style name.
    style_names: HashMap<String, String>,
    /// Parsed themes: theme part path -> resolved scheme colours/fonts.
    themes: BTreeMap<String, ThemeInfo>,
    /// Layout part -> the theme part its master uses.
    layout_themes: HashMap<String, String>,
    /// The theme part of the file currently being parsed, so `schemeClr` and
    /// `+m…` font tokens resolve against the right theme.
    current_theme: Option<String>,
    warnings: Vec<String>,
}

impl Ptx {
    fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }
}

/// Convert `pptx` into a gwen project at `out`, returning the output dir.
/// `title` sets `[presentation] title`; when `None` the pptx file stem is used.
pub fn import_pptx(pptx: &Path, out: &Path, title: Option<&str>) -> Result<PathBuf> {
    let bytes =
        std::fs::read(pptx).map_err(|e| miette!("cannot read `{}`: {e}", pptx.display()))?;
    let parts = read_zip(&bytes)?;
    let pres_part = parts
        .keys()
        .find(|k| k.as_str() == "ppt/presentation.xml")
        .cloned()
        .ok_or_else(|| miette!("`{}` has no `ppt/presentation.xml`", pptx.display()))?;
    let pres_rels = rels_of(&parts, &pres_part)?;

    let mut ptx = Ptx {
        parts,
        rels: pres_rels,
        media: HashMap::new(),
        used_media: BTreeSet::new(),
        master_files: Vec::new(),
        layout_ph_geo: HashMap::new(),
        run_styles: BTreeMap::new(),
        style_names: HashMap::new(),
        themes: BTreeMap::new(),
        layout_themes: HashMap::new(),
        current_theme: None,
        warnings: Vec::new(),
    };
    let mut themes = BTreeMap::new();
    for (part, bytes) in &ptx.parts {
        if part.starts_with("ppt/theme/")
            && let Ok(xml) = parse_xml(bytes)
        {
            themes.insert(part.clone(), parse_theme(&xml));
        }
    }
    ptx.themes = themes;
    let pres = parse_xml(&ptx.parts[&pres_part])?;
    let (width, height) = slide_size(&pres);
    let (major_font, minor_font) = theme_fonts(&ptx);
    let slide_order = slide_order(&mut ptx, &pres);

    // Slide file names first, so [[sections]] can reference them.
    let mut used_names: BTreeSet<String> = BTreeSet::new();
    let mut slide_names: HashMap<String, String> = HashMap::new();
    for part in &slide_order {
        let Some(bytes) = ptx.parts.get(part) else {
            continue;
        };
        if let Ok(slide) = parse_xml(bytes) {
            let title = slide
                .child("cSld")
                .and_then(|c| c.attr("name"))
                .unwrap_or_default();
            slide_names.insert(part.clone(), slide_name(part, title, &mut used_names));
        }
    }
    let sections = sections_of(&pres, &slide_order, &slide_names, &mut ptx);

    // Convert each slideMaster + its layouts into a gwen master file, one per
    // layout (a layout carries the placeholder set a slide inherits).
    let layout_masters = layout_masters(&mut ptx)?;

    let out_dir = out.to_path_buf();
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| miette!("cannot create `{}`: {e}", out.display()))?;
    let mut slide_files: Vec<(String, String)> = Vec::new(); // (filename, toml)
    for slide_part in &slide_order {
        let Some(bytes) = ptx.parts.get(slide_part) else {
            continue;
        };
        let slide = parse_xml(bytes)?;
        let rels = rels_of(&ptx.parts, slide_part)?;
        let layout = rels
            .values()
            .find(|t| t.starts_with("ppt/slideLayouts/"))
            .cloned();
        let master = layout
            .as_deref()
            .and_then(|l| layout_masters.get(l))
            .cloned();
        let layout_ph = layout
            .as_deref()
            .and_then(|l| ptx.layout_ph_geo.get(l))
            .cloned();
        ptx.current_theme = layout
            .as_deref()
            .and_then(|l| ptx.layout_themes.get(l))
            .cloned();
        let shapes = shapes_of_raw(&mut ptx, &rels, &slide, false, layout_ph.as_ref())?;
        ptx.current_theme = None;
        let notes = notes_of(&mut ptx, &rels);
        let filename = slide_names
            .get(slide_part)
            .cloned()
            .unwrap_or_else(|| "slide".into());
        let mut doc = toml::Table::new();
        if let Some(master) = &master {
            doc.insert("master".into(), T::String(master.clone()));
        }
        doc.insert("shapes".into(), shapes_array(&shapes));
        if let Some(notes) = notes {
            doc.insert("notes".into(), T::String(notes));
        }
        let toml_text = toml::to_string(&T::Table(doc))
            .map_err(|e| miette!("cannot serialise slide TOML: {e}"))?;
        slide_files.push((filename, toml_text));
    }

    // main.toml
    let title = title.map(String::from).unwrap_or_else(|| {
        pptx.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "deck".into())
    });
    let main = main_toml(
        &title,
        width,
        height,
        &major_font,
        &minor_font,
        &sections,
        &ptx.run_styles,
    );
    std::fs::write(out_dir.join("main.toml"), main)
        .map_err(|e| miette!("cannot write main.toml: {e}"))?;
    std::fs::create_dir_all(out_dir.join("masters"))
        .map_err(|e| miette!("cannot create masters/: {e}"))?;
    std::fs::create_dir_all(out_dir.join("slides"))
        .map_err(|e| miette!("cannot create slides/: {e}"))?;
    std::fs::create_dir_all(out_dir.join("media"))
        .map_err(|e| miette!("cannot create media/: {e}"))?;
    for (name, toml_text) in std::mem::take(&mut ptx.master_files) {
        std::fs::write(
            out_dir.join("masters").join(format!("{name}.toml")),
            toml_text,
        )
        .map_err(|e| miette!("cannot write masters/{name}.toml: {e}"))?;
    }
    for (filename, toml_text) in &slide_files {
        std::fs::write(out_dir.join("slides").join(filename), toml_text)
            .map_err(|e| miette!("cannot write slides/{filename}: {e}"))?;
    }
    for (part, name) in &ptx.media {
        if let Some(bytes) = ptx.parts.get(part) {
            std::fs::write(out_dir.join("media").join(name), bytes)
                .map_err(|e| miette!("cannot write media/{name}: {e}"))?;
        }
    }

    for w in &ptx.warnings {
        eprintln!("warn: {w}");
    }
    Ok(out_dir)
}

// ---- presentation level ----------------------------------------------------

fn slide_size(pres: &XNode) -> (i64, i64) {
    let sz = pres.child("sldSz");
    let width = sz
        .and_then(|s| s.attr("cx"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(12_196_763);
    let height = sz
        .and_then(|s| s.attr("cy"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(6_858_000);
    (width, height)
}

fn theme_fonts(ptx: &Ptx) -> (String, String) {
    let theme_part = ptx.rels.values().find(|t| t.starts_with("ppt/theme/"));
    let fonts = theme_part.and_then(|t| ptx.themes.get(t)).map(|t| &t.fonts);
    let font = |token: &str| {
        fonts
            .and_then(|f| f.get(token))
            .cloned()
            .unwrap_or_else(|| "Calibri".into())
    };
    (font("+mj-lt"), font("+mn-lt"))
}

/// Slide part names in `sldIdLst` order.
fn slide_order(ptx: &mut Ptx, pres: &XNode) -> Vec<String> {
    let mut order = Vec::new();
    for sld in pres
        .child("sldIdLst")
        .into_iter()
        .flat_map(|l| l.children("sldId"))
    {
        let rid = sld.attr("r:id").unwrap_or_default();
        if let Some(target) = ptx.rels.get(rid) {
            order.push(target.clone());
        } else {
            ptx.warn(format!("slide reference `{rid}` has no relationship"));
        }
    }
    order
}

fn sections_of(
    pres: &XNode,
    order: &[String],
    slide_names: &HashMap<String, String>,
    ptx: &mut Ptx,
) -> Vec<(String, Vec<String>)> {
    let mut id_to_part: HashMap<&str, &str> = HashMap::new();
    for sld in pres
        .child("sldIdLst")
        .into_iter()
        .flat_map(|l| l.children("sldId"))
    {
        let id = sld.attr("id").unwrap_or_default();
        if let Some(rid) = sld.attr("r:id") {
            id_to_part.insert(
                id,
                ptx.rels.get(rid).map(|s| s.as_str()).unwrap_or_default(),
            );
        }
    }
    let name_of = |part: &str| {
        slide_names
            .get(part)
            .cloned()
            .unwrap_or_else(|| "slide".into())
    };
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut covered: BTreeSet<&str> = BTreeSet::new();
    if let Some(lst) = pres.child("extLst").and_then(|e| e.child("sectionLst")) {
        for sec in lst.children("section") {
            let title = sec.attr("name").unwrap_or("Slides").to_string();
            let mut slides = Vec::new();
            for sld in sec.children("sldId") {
                let id = sld.attr("id").unwrap_or_default();
                if let Some(part) = id_to_part.get(id)
                    && !part.is_empty()
                {
                    slides.push(name_of(part));
                    covered.insert(part);
                }
            }
            sections.push((title, slides));
        }
    }
    if sections.is_empty() {
        sections.push(("Slides".into(), order.iter().map(|p| name_of(p)).collect()));
    } else {
        let leftover: Vec<String> = order
            .iter()
            .filter(|p| !covered.contains(p.as_str()))
            .map(|p| name_of(p))
            .collect();
        if !leftover.is_empty() {
            sections.push(("Slides".into(), leftover));
        }
    }
    sections
}

/// One gwen master file per slide layout: `layout part -> (file stem, toml)`.
fn layout_masters(ptx: &mut Ptx) -> Result<HashMap<String, String>> {
    let masters: Vec<String> = ptx
        .rels
        .values()
        .filter(|t| t.starts_with("ppt/slideMasters/"))
        .cloned()
        .collect();
    let mut out = HashMap::new();
    let mut used_names: BTreeSet<String> = BTreeSet::new();
    for master_part in masters {
        let Some(bytes) = ptx.parts.get(&master_part).cloned() else {
            continue;
        };
        let master = parse_xml(&bytes)?;
        let mrels = rels_of(&ptx.parts, &master_part)?;
        let master_theme = mrels
            .values()
            .find(|t| t.starts_with("ppt/theme/"))
            .cloned();
        ptx.current_theme = master_theme.clone();
        for layout in mrels
            .values()
            .filter(|t| t.starts_with("ppt/slideLayouts/"))
        {
            let Some(lbytes) = ptx.parts.get(layout.as_str()).cloned() else {
                continue;
            };
            if let Some(theme) = &master_theme {
                ptx.layout_themes.insert(layout.clone(), theme.clone());
            }
            let lrels = rels_of(&ptx.parts, layout)?;
            let layout_doc = parse_xml(&lbytes)?;
            let mut shapes = Vec::new();
            shapes.extend(shapes_of_raw(ptx, &mrels, &master, true, None)?);
            shapes.extend(shapes_of_raw(ptx, &lrels, &layout_doc, true, None)?);
            shapes = dedupe_shapes(shapes);
            let mut ph_geo = HashMap::new();
            for s in &shapes {
                if let Some(key) = &s.ph_key {
                    ph_geo.insert(key.clone(), (s.x, s.y, s.w, s.h));
                }
            }
            ptx.layout_ph_geo.insert(layout.clone(), ph_geo);
            let stem = Path::new(layout.as_str())
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "master".into());
            let name = unique_name(&stem, &mut used_names);
            let mut doc = toml::Table::new();
            doc.insert("shapes".into(), shapes_array(&shapes));
            let toml_text = toml::to_string(&T::Table(doc))
                .map_err(|e| miette!("cannot serialise master TOML: {e}"))?;
            out.insert(layout.clone(), name.clone());
            ptx.master_files.push((name, toml_text));
        }
    }
    Ok(out)
}

fn unique_name(base: &str, used: &mut BTreeSet<String>) -> String {
    let mut candidate = base.to_string();
    let mut i = 1;
    while !used.insert(candidate.clone()) {
        candidate = format!("{base}-{}", i);
        i += 1;
    }
    candidate
}

fn slide_name(part: &str, cname: &str, used: &mut BTreeSet<String>) -> String {
    let base = if cname.is_empty() {
        Path::new(part)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "slide".into())
    } else {
        sanitize(cname)
    };
    format!("{}.toml", unique_name(&base, used))
}

fn sanitize(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() { "slide".into() } else { out }
}

/// Canonical placeholder identity: `type@idx`, with the default idx (0 for
/// title placeholders, 1 otherwise) when the attribute is absent. Used to
/// match a slide placeholder to the layout/master placeholder it inherits.
fn ph_key(ph: &XNode) -> String {
    let ty = ph.attr("type").unwrap_or("body");
    let idx = ph.attr("idx").map(String::from).unwrap_or_else(|| {
        if ty == "title" || ty == "ctrTitle" {
            "0".into()
        } else {
            "1".into()
        }
    });
    format!("{ty}:{idx}")
}

// ---- shapes -----------------------------------------------------------------

fn shapes_of_raw(
    ptx: &mut Ptx,
    rels: &HashMap<String, String>,
    doc: &XNode,
    is_master: bool,
    layout_ph: Option<&PhGeo>,
) -> Result<Vec<ShapeOut>> {
    let c = doc
        .child("cSld")
        .ok_or_else(|| miette!("slide/master has no cSld"))?;
    let tree = c
        .child("spTree")
        .ok_or_else(|| miette!("slide/master has no spTree"))?;
    let mut out = Vec::new();
    for el in &tree.children {
        parse_element(ptx, rels, el, is_master, layout_ph, &mut out);
    }
    Ok(out)
}

fn parse_element(
    ptx: &mut Ptx,
    rels: &HashMap<String, String>,
    el: &XNode,
    is_master: bool,
    layout_ph: Option<&PhGeo>,
    out: &mut Vec<ShapeOut>,
) {
    match el.name.as_str() {
        "sp" => {
            if let Some(s) = parse_sp(ptx, rels, el, is_master, layout_ph) {
                out.push(s);
            }
        }
        "pic" => {
            if let Some(s) = parse_pic(ptx, rels, el) {
                out.push(s);
            }
        }
        "cxnSp" => {
            if let Some(s) = parse_cxn(ptx, el) {
                out.push(s);
            }
        }
        "grpSp" => {
            let gx = group_transform(el);
            let mut child_out = Vec::new();
            let sp_tree = el.child("spTree");
            for child in sp_tree.into_iter().flat_map(|t| &t.children) {
                parse_element(ptx, rels, child, is_master, layout_ph, &mut child_out);
            }
            for s in &mut child_out {
                gx.apply(s);
            }
            out.append(&mut child_out);
        }
        "graphicFrame" => ptx.warn("chart/table shape skipped (not supported)"),
        _ => {}
    }
}

/// A shape's parsed geometry plus a small option set, before TOML serialisation.
#[derive(Debug, Clone)]
struct ShapeOut {
    ty: String,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    rotate: Option<i64>,
    flip_h: bool,
    flip_v: bool,
    fill: Option<toml::Value>,
    line: Option<toml::Value>,
    shadow: Option<toml::Value>,
    text: Option<String>,
    paragraphs: Vec<toml::Value>,
    font_size: Option<f64>,
    color: Option<String>,
    font_face: Option<String>,
    valign: Option<String>,
    text_direction: Option<String>,
    margin: Option<Vec<f64>>,
    src: Option<String>,
    alt_text: Option<String>,
    hyperlink: Option<String>,
    object_name: Option<String>,
    /// Set on placeholder shapes; used to match slide placeholders to the
    /// layout/master placeholder whose geometry they inherit.
    ph_key: Option<String>,
}

struct GroupTransform {
    x: i64,
    y: i64,
    ch_x: i64,
    ch_y: i64,
    ch_w: i64,
    ch_h: i64,
    w: i64,
    h: i64,
}

impl GroupTransform {
    fn apply(&self, s: &mut ShapeOut) {
        let sx = if self.ch_w > 0 {
            self.w as f64 / self.ch_w as f64
        } else {
            1.0
        };
        let sy = if self.ch_h > 0 {
            self.h as f64 / self.ch_h as f64
        } else {
            1.0
        };
        let rel = |v: i64, ch: i64| (v - ch) as f64;
        s.x = self.x + (rel(s.x, self.ch_x) * sx).round() as i64;
        s.y = self.y + (rel(s.y, self.ch_y) * sy).round() as i64;
        s.w = (s.w as f64 * sx).round() as i64;
        s.h = (s.h as f64 * sy).round() as i64;
    }
}

fn group_transform(grp: &XNode) -> GroupTransform {
    let xfrm = grp.child("grpSpPr").and_then(|g| g.child("xfrm"));
    let at = |el: &str, name: &str| -> i64 {
        xfrm.and_then(|x| x.child(el))
            .and_then(|n| n.attr(name))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    GroupTransform {
        x: at("off", "x"),
        y: at("off", "y"),
        ch_x: at("chOff", "x"),
        ch_y: at("chOff", "y"),
        ch_w: (at("chExt", "x") - at("chOff", "x")).max(0),
        ch_h: (at("chExt", "y") - at("chOff", "y")).max(0),
        w: at("ext", "cx"),
        h: at("ext", "cy"),
    }
}

fn geometry(sp_pr: &XNode) -> (i64, i64, i64, i64, Option<i64>, bool, bool) {
    let xfrm = sp_pr.child("xfrm");
    let at = |el: &str, name: &str| -> i64 {
        xfrm.and_then(|x| x.child(el))
            .and_then(|n| n.attr(name))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    let x = at("off", "x");
    let y = at("off", "y");
    let w = at("ext", "cx");
    let h = at("ext", "cy");
    let rotate = xfrm
        .and_then(|x| x.attr("rot"))
        .and_then(|v| v.parse::<i64>().ok())
        .map(|r| r / 60_000);
    let flip_h = xfrm
        .and_then(|x| x.attr("flipH"))
        .map(|v| v == "1")
        .unwrap_or(false);
    let flip_v = xfrm
        .and_then(|x| x.attr("flipV"))
        .map(|v| v == "1")
        .unwrap_or(false);
    (x, y, w, h, rotate, flip_h, flip_v)
}

fn parse_sp(
    ptx: &mut Ptx,
    rels: &HashMap<String, String>,
    sp: &XNode,
    is_master: bool,
    layout_ph: Option<&PhGeo>,
) -> Option<ShapeOut> {
    let sp_pr = sp.child("spPr");
    let ph = sp
        .child("nvSpPr")
        .and_then(|n| n.child("nvPr"))
        .and_then(|p| p.child("ph"));
    let is_placeholder = ph.is_some();
    let has_xfrm = sp_pr.map(|s| s.child("xfrm").is_some()).unwrap_or(false);
    let (x, y, w, h, rotate, flip_h, flip_v) = geometry(sp_pr.unwrap_or(sp));
    // A slide placeholder with no geometry of its own inherits the position of
    // the matching layout/master placeholder.
    let (x, y, w, h) = if is_placeholder && !has_xfrm && !is_master {
        let inherited = ph
            .and_then(|ph| layout_ph.and_then(|m| m.get(&ph_key(ph))))
            .copied();
        inherited.unwrap_or((x, y, w, h))
    } else {
        (x, y, w, h)
    };
    let tx_body = sp.child("txBody");
    let (prst, prst_shape) = sp_pr
        .and_then(|s| s.child("prstGeom"))
        .and_then(|g| g.attr("prst"))
        .map(|p| (snake(p), true))
        .unwrap_or_else(|| ("rect".into(), false));
    let ty = if tx_body.is_some() && prst_shape && prst != "rect" {
        prst.clone() // preset with text
    } else if tx_body.is_some() {
        "text".into()
    } else {
        prst
    };
    let TextOut {
        mut text,
        mut paragraphs,
        font_size,
        color,
        font_face,
        valign,
        text_direction,
        margin,
    } = tx_body
        .map(|t| parse_text_body(ptx, rels, t))
        .unwrap_or_default();
    // Master/layout placeholder prompts ("Click to edit title", ...) are
    // boilerplate, not real content.
    if is_master && is_placeholder {
        text = None;
        paragraphs.clear();
    }

    let mut so = ShapeOut {
        ty,
        x,
        y,
        w,
        h,
        rotate,
        flip_h,
        flip_v,
        fill: parse_fill(sp_pr.unwrap_or(sp), ptx),
        line: parse_line(sp_pr.unwrap_or(sp), ptx),
        shadow: parse_shadow(sp_pr.unwrap_or(sp), ptx),
        text,
        paragraphs,
        font_size,
        color,
        font_face,
        valign,
        text_direction,
        margin,
        src: None,
        alt_text: sp
            .child("nvSpPr")
            .and_then(|n| n.child("cNvPr"))
            .and_then(|c| c.attr("descr"))
            .map(String::from),
        hyperlink: None,
        object_name: sp
            .child("nvSpPr")
            .and_then(|n| n.child("cNvPr"))
            .and_then(|c| c.attr("name"))
            .map(String::from),
        ph_key: ph.map(ph_key),
    };
    // shape-level hyperlink: p:nvSpPr/p:cNvPr/a:hlinkClick
    if let Some(h) = sp
        .child("nvSpPr")
        .and_then(|n| n.child("cNvPr"))
        .and_then(|c| c.child("hlinkClick"))
        .and_then(|h| h.attr("r:id"))
        .and_then(|id| ptx.rels.get(id).cloned())
    {
        so.hyperlink = Some(h);
    }
    Some(so)
}

fn parse_pic(ptx: &mut Ptx, rels: &HashMap<String, String>, pic: &XNode) -> Option<ShapeOut> {
    let sp_pr = pic.child("spPr");
    let (x, y, w, h, rotate, flip_h, flip_v) = geometry(sp_pr.unwrap_or(pic));
    let r_id = pic
        .child("blipFill")
        .and_then(|b| b.child("blip"))
        .and_then(|b| b.attr("r:embed"))
        .unwrap_or_default();
    let target = rels.get(r_id).cloned();
    let media_part = target
        .as_deref()
        .filter(|t| t.starts_with("ppt/media/"))
        .and_then(|t| {
            if ptx.parts.contains_key(t) {
                Some(t.to_string())
            } else {
                None
            }
        });
    let src = media_part
        .as_deref()
        .map(|part| format!("media/{}", media_name(ptx, part)));
    if media_part.is_none() {
        ptx.warn("image with an unresolved relationship skipped");
    }
    let c_nv = pic.child("nvPicPr").and_then(|n| n.child("cNvPr"));
    Some(ShapeOut {
        ty: "image".into(),
        x,
        y,
        w,
        h,
        rotate,
        flip_h,
        flip_v,
        fill: None,
        line: None,
        shadow: parse_shadow(sp_pr.unwrap_or(pic), ptx),
        text: None,
        paragraphs: Vec::new(),
        font_size: None,
        color: None,
        font_face: None,
        valign: None,
        text_direction: None,
        margin: None,
        src,
        alt_text: c_nv.and_then(|c| c.attr("descr")).map(String::from),
        hyperlink: None,
        object_name: c_nv.and_then(|c| c.attr("name")).map(String::from),
        ph_key: None,
    })
}

fn parse_cxn(ptx: &mut Ptx, cxn: &XNode) -> Option<ShapeOut> {
    let sp_pr = cxn.child("spPr");
    let (x, y, w, h, rotate, flip_h, flip_v) = geometry(sp_pr.unwrap_or(cxn));
    let so = ShapeOut {
        ty: "line".into(),
        x,
        y,
        w,
        h,
        rotate,
        flip_h,
        flip_v,
        fill: None,
        line: parse_line(sp_pr.unwrap_or(cxn), ptx),
        shadow: None,
        text: None,
        paragraphs: Vec::new(),
        font_size: None,
        color: None,
        font_face: None,
        valign: None,
        text_direction: None,
        margin: None,
        src: None,
        alt_text: None,
        hyperlink: None,
        object_name: None,
        ph_key: None,
    };
    if so.line.is_none() {
        ptx.warn("connector without a line skipped");
        return None;
    }
    Some(so)
}

// ---- fills / lines / shadows ------------------------------------------------

fn parse_fill(sp_pr: &XNode, ptx: &mut Ptx) -> Option<toml::Value> {
    if let Some(sf) = sp_pr.child("solidFill") {
        if let Some(c) = color_of(ptx, sf) {
            return Some(T::Table(toml::Table::from_iter([(
                "color".into(),
                T::String(c),
            )])));
        }
        if sf.child("noFill").is_some() {
            return Some(T::Table(toml::Table::from_iter([(
                "type".into(),
                T::String("none".into()),
            )])));
        }
    }
    if sp_pr.child("noFill").is_some() {
        return Some(T::Table(toml::Table::from_iter([(
            "type".into(),
            T::String("none".into()),
        )])));
    }
    if sp_pr.child("gradFill").is_some() || sp_pr.child("pattFill").is_some() {
        ptx.warn("gradient/pattern fill approximated as none");
    }
    None
}

fn parse_line(sp_pr: &XNode, ptx: &Ptx) -> Option<toml::Value> {
    let ln = sp_pr.child("ln")?;
    let mut t = toml::Table::new();
    if let Some(w) = ln.attr("w").and_then(|v| v.parse::<f64>().ok()) {
        t.insert("width".into(), T::Float(w / 12_700.0));
    }
    if let Some(c) = color_of(ptx, ln) {
        t.insert("color".into(), T::String(c));
    }
    if ln.child("noFill").is_some() {
        t.insert("type".into(), T::String("none".into()));
    }
    if t.is_empty() {
        return None;
    }
    Some(T::Table(t))
}

fn parse_shadow(sp_pr: &XNode, ptx: &Ptx) -> Option<toml::Value> {
    let sh = sp_pr.child("effectLst")?.child("outerShdw")?;
    let mut t = toml::Table::new();
    t.insert("type".into(), T::String("outer".into()));
    if let Some(c) = color_of(ptx, sh) {
        t.insert("color".into(), T::String(c));
    }
    let f = |name: &str, div: f64| {
        sh.attr(name)
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v / div)
    };
    if let Some(blur) = f("blurRad", 12_700.0) {
        t.insert("blur".into(), T::Float(blur));
    }
    if let Some(offset) = f("dist", 12_700.0) {
        t.insert("offset".into(), T::Float(offset));
    }
    if let Some(angle) = f("dir", 60_000.0) {
        t.insert("angle".into(), T::Integer(angle as i64 % 360));
    }
    if let Some(alpha) = sh
        .child("alpha")
        .and_then(|a| a.attr("val"))
        .and_then(|v| v.parse::<f64>().ok())
    {
        t.insert("opacity".into(), T::Float(alpha / 100_000.0));
    }
    Some(T::Table(t))
}

/// `srgbClr` hex or `schemeClr` theme colour (mapped to gwen's theme names),
/// found either directly on the node or inside a `solidFill` child (as in
/// `<a:ln><a:solidFill><a:srgbClr .../></a:solidFill></a:ln>`).
fn color_of(ptx: &Ptx, node: &XNode) -> Option<String> {
    let direct = |n: &XNode| -> Option<String> {
        if let Some(c) = n.child("srgbClr").and_then(|c| c.attr("val")) {
            return Some(c.to_ascii_uppercase());
        }
        if let Some(c) = n.child("schemeClr").and_then(|c| c.attr("val")) {
            return scheme_color(ptx, c);
        }
        None
    };
    direct(node).or_else(|| node.child("solidFill").and_then(direct))
}

/// Resolve an OOXML scheme colour name to a hex from the active theme.
fn scheme_color(ptx: &Ptx, c: &str) -> Option<String> {
    if let Some(theme) = ptx.current_theme.as_ref().and_then(|t| ptx.themes.get(t))
        && let Some(hex) = theme.colors.get(c)
    {
        return Some(hex.clone());
    }
    // No theme part (defensive, malformed file): keep the theme name.
    match c {
        "lt1" => Some("bg1".into()),
        "lt2" => Some("bg2".into()),
        "dk1" => Some("tx1".into()),
        "dk2" => Some("tx2".into()),
        "hlink" | "folHlink" => None,
        other => Some(other.to_string()),
    }
}

// ---- text -------------------------------------------------------------------

/// Parsed text-body result: shape-level formatting plus the paragraphs.
#[derive(Debug, Default)]
struct TextOut {
    text: Option<String>,
    paragraphs: Vec<toml::Value>,
    font_size: Option<f64>,
    color: Option<String>,
    font_face: Option<String>,
    valign: Option<String>,
    text_direction: Option<String>,
    margin: Option<Vec<f64>>,
}

fn parse_text_body(ptx: &mut Ptx, rels: &HashMap<String, String>, body: &XNode) -> TextOut {
    let body_pr = body.child("bodyPr");
    let valign = body_pr.and_then(|b| b.attr("anchor")).map(anchor_to_valign);
    let text_direction = body_pr
        .and_then(|b| b.attr("vert"))
        .filter(|v| matches!(*v, "horz" | "vert" | "vert270" | "wordArtVert"))
        .map(String::from);
    let margin = body_pr.and_then(|b| {
        let at = |n: &str| {
            b.attr(n)
                .and_then(|v| v.parse::<f64>().ok())
                .map(|v| v / EMU_PER_IN)
        };
        match (at("tIns"), at("rIns"), at("bIns"), at("lIns")) {
            (Some(t), Some(r), Some(b), Some(l))
                if !(t == 0.0 && r == 0.0 && b == 0.0 && l == 0.0) =>
            {
                Some(vec![t, r, b, l])
            }
            _ => None,
        }
    });

    let mut paragraphs: Vec<toml::Value> = Vec::new();

    for p in body.children("p") {
        let (md, opts) = parse_paragraph(ptx, rels, p);
        paragraphs.push(T::Table({
            let mut t = toml::Table::new();
            t.insert("text".into(), T::String(md));
            for (k, v) in opts {
                t.insert(k, v);
            }
            t
        }));
    }

    // If every paragraph is plain, use the `text` shorthand instead.
    let text = if paragraphs.iter().all(|p| p.get("bullet").is_none()) {
        let joined = paragraphs
            .iter()
            .map(|p| p.get("text").and_then(|t| t.as_str()).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n\n");
        paragraphs.clear();
        Some(joined)
    } else {
        None
    };

    TextOut {
        text,
        paragraphs,
        font_size: None,
        color: None,
        font_face: None,
        valign,
        text_direction,
        margin,
    }
}

fn anchor_to_valign(a: &str) -> String {
    match a {
        "t" => "top".into(),
        "ctr" => "middle".into(),
        "b" => "bottom".into(),
        other => other.to_string(),
    }
}

fn run_color(ptx: &Ptx, rpr: &XNode) -> Option<String> {
    rpr.child("solidFill").and_then(|sf| color_of(ptx, sf))
}

fn parse_paragraph(
    ptx: &mut Ptx,
    rels: &HashMap<String, String>,
    p: &XNode,
) -> (String, toml::Table) {
    let mut md = String::new();
    let mut opts = toml::Table::new();
    if let Some(p_pr) = p.child("pPr") {
        if let Some(align) = p_pr.attr("algn") {
            opts.insert("align".into(), T::String(align.to_string()));
        }
        if let Some(ch) = p_pr.child("buChar").and_then(|b| b.attr("char")) {
            let code = ch
                .chars()
                .next()
                .map(|c| format!("{:X}", c as u32))
                .unwrap_or_default();
            opts.insert(
                "bullet".into(),
                T::Table(toml::Table::from_iter([
                    ("type".into(), T::String("bullet".into())),
                    ("character_code".into(), T::String(code)),
                ])),
            );
        } else if let Some(auto) = p_pr.child("buAutoNum") {
            let mut b = toml::Table::from_iter([("type".into(), T::String("number".into()))]);
            if let Some(nt) = auto.attr("type") {
                b.insert("number_type".into(), T::String(nt.to_string()));
            }
            if let Some(sa) = auto.attr("startAt").and_then(|v| v.parse::<i64>().ok()) {
                b.insert("number_start_at".into(), T::Integer(sa));
            }
            opts.insert("bullet".into(), T::Table(b));
        }
        if let Some(mar_l) = p_pr.attr("marL").and_then(|v| v.parse::<f64>().ok()) {
            let level = (mar_l / (EMU_PER_IN * 0.5)).round() as i64;
            if level > 0 {
                opts.insert("indent_level".into(), T::Integer(level));
            }
        }
        if let Some(sp) = p_pr.attr("spcBef").and_then(|v| v.parse::<f64>().ok()) {
            opts.insert("para_space_before".into(), T::Integer((sp / 100.0) as i64));
        }
    }
    for el in &p.children {
        match el.name.as_str() {
            "r" | "fld" => md.push_str(&parse_run(ptx, rels, el)),
            "br" => md.push('\n'),
            _ => {}
        }
    }
    (guard_list_start(&md), opts)
}

fn parse_run(ptx: &mut Ptx, rels: &HashMap<String, String>, r: &XNode) -> String {
    let rpr = r.child("rPr");
    let bold = rpr
        .and_then(|p| p.attr("b"))
        .map(|v| v == "1")
        .unwrap_or(false);
    let italic = rpr
        .and_then(|p| p.attr("i"))
        .map(|v| v == "1")
        .unwrap_or(false);
    let text = r.child("t").map(|t| t.text.clone()).unwrap_or_default();
    let escaped = escape_markdown(&text);
    // Bold/italic markers must hug non-whitespace: `**Copyright©2021 **` has its
    // closing `**` preceded by a space, which CommonMark treats as literal `*`.
    // Move the run's leading/trailing whitespace outside the markers.
    let lead_end = escaped
        .find(|c: char| !c.is_whitespace())
        .unwrap_or(escaped.len());
    let trail_start = escaped
        .rfind(|c: char| !c.is_whitespace())
        .map(|i| i + escaped[i..].chars().next().unwrap().len_utf8())
        .unwrap_or(0);
    let mut seg = if escaped[lead_end..trail_start].is_empty() {
        escaped
    } else {
        let lead = &escaped[..lead_end];
        let core = &escaped[lead_end..trail_start];
        let trail = &escaped[trail_start..];
        match (bold, italic) {
            (true, true) => format!("{lead}***{core}***{trail}"),
            (true, false) => format!("{lead}**{core}**{trail}"),
            (false, true) => format!("{lead}*{core}*{trail}"),
            (false, false) => escaped,
        }
    };
    if let Some(url) = rpr
        .and_then(|p| p.child("hlinkClick"))
        .and_then(|h| h.attr("r:id"))
        .and_then(|id| rels.get(id).cloned())
    {
        if url.starts_with("http") {
            seg = format!("[{seg}]({url})");
        } else {
            ptx.warn("slide-internal text hyperlink dropped");
        }
    }
    // Keep per-run formatting (colour, font, size, ...) via
    // `<span class="runN">` styles, for slides and masters alike.
    let opts = run_style_opts(ptx, rpr);
    if !opts.is_empty() {
        let class = span_class(ptx, &opts);
        seg = format!("<span class=\"{class}\">{seg}</span>");
    }
    seg
}

/// The non-markdown run options (`color`, `font_face`, `font_size`,
/// `underline`, ...) of an `<a:rPr>`, for a `<span class="runN">` style.
fn run_style_opts(ptx: &Ptx, rpr: Option<&XNode>) -> toml::Table {
    let mut t = toml::Table::new();
    let Some(rpr) = rpr else {
        return t;
    };
    if let Some(c) = run_color(ptx, rpr) {
        t.insert("color".into(), T::String(c));
    }
    if let Some(s) = rpr.attr("sz").and_then(|v| v.parse::<f64>().ok()) {
        t.insert("font_size".into(), T::Float(s / 100.0));
    }
    if let Some(face) = ["latin", "ea", "cs"]
        .iter()
        .find_map(|el| rpr.child(el).and_then(|f| f.attr("typeface")))
        && let Some(face) = resolve_font(ptx, face)
    {
        t.insert("font_face".into(), T::String(face));
    }
    if let Some(u) = rpr.attr("u").filter(|u| *u != "none") {
        t.insert("underline".into(), T::String(u.to_string()));
    }
    if let Some(s) = rpr.attr("strike").filter(|s| *s != "none") {
        t.insert("strike".into(), T::String(s.to_string()));
    }
    if let Some(h) = rpr.child("highlight").and_then(|h| color_of(ptx, h)) {
        t.insert("highlight".into(), T::String(h));
    }
    // lang/rtl_mode are per-run metadata with no visual effect; skipping them
    // keeps the generated styles meaningful.
    if let Some(base) = rpr.attr("baseline").and_then(|v| v.parse::<i64>().ok()) {
        // `baseline="0"` is the normal baseline (emitted on most runs); only a
        // non-zero value means superscript (positive) or subscript (negative).
        if base != 0 {
            let key = if base > 0 { "superscript" } else { "subscript" };
            t.insert(key.into(), T::Boolean(true));
        }
    }
    t
}

/// Resolve an OOXML theme font token (`+mn-lt`, `+mj-ea`, ...) to the theme's
/// actual typeface; a plain font name passes through.
fn resolve_font(ptx: &Ptx, face: &str) -> Option<String> {
    if face.starts_with("+m") {
        return ptx
            .current_theme
            .as_ref()
            .and_then(|t| ptx.themes.get(t))
            .and_then(|th| th.fonts.get(face))
            .cloned();
    }
    Some(face.to_string())
}

/// Register (or look up) the `[styles.named.runN]` style for a run-option set.
fn span_class(ptx: &mut Ptx, opts: &toml::Table) -> String {
    let key = toml::to_string(opts).unwrap_or_default();
    if let Some(name) = ptx.style_names.get(&key) {
        return name.clone();
    }
    let name = format!("run{}", ptx.run_styles.len() + 1);
    ptx.style_names.insert(key, name.clone());
    ptx.run_styles.insert(name.clone(), opts.clone());
    name
}

/// Escape characters that could change markdown meaning when the run is plain.
fn escape_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '*' | '_' | '`' | '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// A paragraph starting with a list marker would be re-parsed as a list; escape
/// the marker so a literal leading `- item` stays plain text.
fn guard_list_start(s: &str) -> String {
    if s.starts_with("- ") || s.starts_with("+ ") || s.starts_with("* ") {
        return format!("\\{s}");
    }
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0
        && i < bytes.len()
        && (bytes[i] == b'.' || bytes[i] == b')')
        && bytes.get(i + 1) == Some(&b' ')
    {
        return format!("\\{s}");
    }
    s.to_string()
}

// ---- serialisation -----------------------------------------------------------

fn shapes_array(shapes: &[ShapeOut]) -> toml::Value {
    T::Array(shapes.iter().map(shape_to_toml).collect())
}

fn shape_to_toml(s: &ShapeOut) -> toml::Value {
    let mut t = toml::Table::new();
    t.insert("type".into(), T::String(s.ty.clone()));
    t.insert("x".into(), T::Integer(s.x));
    t.insert("y".into(), T::Integer(s.y));
    t.insert("w".into(), T::Integer(s.w));
    t.insert("h".into(), T::Integer(s.h));
    if let Some(r) = s.rotate {
        t.insert("rotate".into(), T::Integer(r));
    }
    if s.flip_h {
        t.insert("flip_h".into(), T::Boolean(true));
    }
    if s.flip_v {
        t.insert("flip_v".into(), T::Boolean(true));
    }
    if let Some(v) = &s.fill {
        t.insert("fill".into(), v.clone());
    }
    if let Some(v) = &s.line {
        t.insert("line".into(), v.clone());
    }
    if let Some(v) = &s.shadow {
        t.insert("shadow".into(), v.clone());
    }
    if let Some(v) = &s.src {
        t.insert("src".into(), T::String(v.clone()));
    }
    if let Some(v) = &s.alt_text {
        t.insert("alt_text".into(), T::String(v.clone()));
    }
    if let Some(v) = &s.object_name {
        t.insert("object_name".into(), T::String(v.clone()));
    }
    if let Some(v) = &s.hyperlink {
        t.insert(
            "hyperlink".into(),
            T::Table(toml::Table::from_iter([(
                "url".into(),
                T::String(v.clone()),
            )])),
        );
    }
    if let Some(v) = &s.text {
        t.insert("text".into(), T::String(v.clone()));
    }
    if !s.paragraphs.is_empty() {
        t.insert("paragraphs".into(), T::Array(s.paragraphs.clone()));
    }
    if let Some(v) = s.font_size {
        t.insert("font_size".into(), T::Float(v));
    }
    if let Some(v) = &s.color {
        t.insert("color".into(), T::String(v.clone()));
    }
    if let Some(v) = &s.font_face {
        t.insert("font_face".into(), T::String(v.clone()));
    }
    if let Some(v) = &s.valign {
        t.insert("valign".into(), T::String(v.clone()));
    }
    if let Some(v) = &s.text_direction {
        t.insert("text_direction".into(), T::String(v.clone()));
    }
    if let Some(v) = &s.margin {
        t.insert(
            "margin".into(),
            T::Array(v.iter().map(|x| T::Float(*x)).collect()),
        );
    }
    T::Table(t)
}

fn dedupe_shapes(shapes: Vec<ShapeOut>) -> Vec<ShapeOut> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for s in shapes {
        let key = format!(
            "{}|{}|{}|{}|{}|{}",
            s.ty,
            s.x,
            s.y,
            s.w,
            s.h,
            s.text.as_deref().unwrap_or("")
        );
        if seen.insert(key) {
            out.push(s);
        }
    }
    out
}

fn main_toml(
    title: &str,
    width: i64,
    height: i64,
    major_font: &str,
    minor_font: &str,
    sections: &[(String, Vec<String>)],
    run_styles: &BTreeMap<String, toml::Table>,
) -> String {
    let mut root = toml::Table::new();
    let mut pres = toml::Table::new();
    pres.insert("title".into(), T::String(title.into()));
    pres.insert("width".into(), T::Integer(width));
    pres.insert("height".into(), T::Integer(height));
    root.insert("presentation".into(), T::Table(pres));
    let mut theme = toml::Table::new();
    theme.insert("major_font".into(), T::String(major_font.into()));
    theme.insert("minor_font".into(), T::String(minor_font.into()));
    root.insert("theme".into(), T::Table(theme));
    let secs: Vec<toml::Value> = sections
        .iter()
        .map(|(title, slides)| {
            T::Table(toml::Table::from_iter([
                ("title".into(), T::String(title.clone())),
                (
                    "slides".into(),
                    T::Array(slides.iter().map(|s| T::String(s.clone())).collect()),
                ),
            ]))
        })
        .collect();
    root.insert("sections".into(), T::Array(secs));
    if !run_styles.is_empty() {
        let named: toml::Table = run_styles
            .iter()
            .map(|(name, table)| (name.clone(), T::Table(table.clone())))
            .collect();
        root.insert(
            "styles".into(),
            T::Table(toml::Table::from_iter([("named".into(), T::Table(named))])),
        );
    }
    toml::to_string(&T::Table(root)).unwrap_or_default()
}

// ---- notes / media ----------------------------------------------------------

fn notes_of(ptx: &mut Ptx, rels: &HashMap<String, String>) -> Option<String> {
    let notes = rels
        .values()
        .find(|t| t.starts_with("ppt/notesSlides/"))
        .and_then(|t| ptx.parts.get(t.as_str()))
        .and_then(|b| parse_xml(b).ok())?;
    let mut paras: Vec<String> = Vec::new();
    let mut current = String::new();
    for p in notes
        .child("cSld")
        .and_then(|c| c.child("spTree"))
        .into_iter()
        .flat_map(|t| &t.children)
        .filter(|el| el.name == "sp")
        .flat_map(|sp| sp.child("txBody"))
        .flat_map(|b| b.children("p"))
    {
        for el in &p.children {
            match el.name.as_str() {
                "r" => {
                    if let Some(t) = el.child("t") {
                        current.push_str(&t.text);
                    }
                }
                "br" => current.push('\n'),
                _ => {}
            }
        }
        if !current.is_empty() {
            paras.push(current.trim().to_string());
            current.clear();
        }
    }
    if paras.is_empty() {
        None
    } else {
        Some(paras.join("\n\n"))
    }
}

fn media_name(ptx: &mut Ptx, part: &str) -> String {
    if let Some(existing) = ptx.media.get(part) {
        return existing.clone();
    }
    let base = Path::new(part)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".into());
    let name = unique_name(&base, &mut ptx.used_media);
    ptx.media.insert(part.to_string(), name.clone());
    name
}

/// snake_case for a camelCase preset id (`roundRect` -> `round_rect`).
fn snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}
