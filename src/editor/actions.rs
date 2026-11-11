//! Semantic editor actions and their TOML mutations.

use std::collections::BTreeMap;
use std::path::Path;

use miette::{Result, miette};
use serde::Deserialize;
use serde_json::Value as Json;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::model::Project;

const EMU_PER_IN: f64 = 914_400.0;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Move {
        slide: usize,
        indices: Vec<usize>,
        dx: f64,
        dy: f64,
    },
    Resize {
        slide: usize,
        index: usize,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    SetShape {
        slide: usize,
        index: usize,
        keys: BTreeMap<String, Json>,
    },
    SetParagraph {
        slide: usize,
        index: usize,
        paragraph: usize,
        text: String,
    },
    AddText {
        slide: usize,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        #[serde(default)]
        text: Option<String>,
    },
    DeleteShapes {
        slide: usize,
        indices: Vec<usize>,
    },
    DuplicateShapes {
        slide: usize,
        indices: Vec<usize>,
    },
    ZOrder {
        slide: usize,
        indices: Vec<usize>,
        op: String,
    },
    AddSlide {
        after: usize,
    },
    DeleteSlide {
        slide: usize,
    },
    DuplicateSlide {
        slide: usize,
    },
    MoveSlide {
        from: usize,
        to: usize,
    },
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub slide: Option<usize>,
    pub select: Vec<usize>,
}

pub(crate) fn apply(dir: &Path, action: &Action) -> Result<Outcome> {
    let project = Project::load(dir)?;
    match action {
        Action::Move {
            slide,
            indices,
            dx,
            dy,
        } => {
            let (width, height) = slide_size(&project)?;
            edit_shapes(dir, &project, *slide, |shapes| {
                for &index in indices {
                    let shape = shapes
                        .get_mut(index)
                        .ok_or_else(|| miette!("shape {index} does not exist"))?;
                    if shape.get("placeholder").is_some() {
                        return Err(miette!("placeholder shapes cannot be moved"));
                    }
                    let x = coord_inches(shape.get("x"), width).unwrap_or(0.0) + dx;
                    let y = coord_inches(shape.get("y"), height).unwrap_or(0.0) + dy;
                    set_coord(shape, "x", x.max(0.0), width)?;
                    set_coord(shape, "y", y.max(0.0), height)?;
                }
                Ok(Outcome {
                    slide: Some(*slide),
                    select: indices.clone(),
                })
            })
        }
        Action::Resize {
            slide,
            index,
            x,
            y,
            w,
            h,
        } => {
            if *w < 0.0 || *h < 0.0 {
                return Err(miette!("width and height must not be negative"));
            }
            let (width, height) = slide_size(&project)?;
            edit_shapes(dir, &project, *slide, |shapes| {
                let shape = shapes
                    .get_mut(*index)
                    .ok_or_else(|| miette!("shape {index} does not exist"))?;
                if shape.get("placeholder").is_some() {
                    return Err(miette!("placeholder shapes cannot be resized"));
                }
                if shape.get("type").and_then(Item::as_str) == Some("table") {
                    return Err(miette!("tables can be moved in this phase, not resized"));
                }
                set_coord(shape, "x", x.max(0.0), width)?;
                set_coord(shape, "y", y.max(0.0), height)?;
                set_coord(shape, "w", *w, width)?;
                set_coord(shape, "h", *h, height)?;
                Ok(Outcome {
                    slide: Some(*slide),
                    select: vec![*index],
                })
            })
        }
        Action::SetShape { slide, index, keys } => edit_shapes(dir, &project, *slide, |shapes| {
            let shape = shapes
                .get_mut(*index)
                .ok_or_else(|| miette!("shape {index} does not exist"))?;
            if shape.get("placeholder").is_some() {
                for key in keys.keys() {
                    if matches!(key.as_str(), "x" | "y" | "w" | "h" | "rotate") {
                        return Err(miette!(
                            "placeholder geometry cannot be changed; edit the master"
                        ));
                    }
                }
            }
            for (key, value) in keys {
                if matches!(
                    key.as_str(),
                    "type"
                        | "styles"
                        | "x"
                        | "y"
                        | "w"
                        | "h"
                        | "src"
                        | "rows"
                        | "col_styles"
                        | "row_styles"
                        | "paragraphs"
                        | "index"
                        | "kind"
                ) {
                    return Err(miette!("`{key}` cannot be set through this action"));
                }
                if value.is_null() {
                    shape.remove(key);
                } else {
                    shape.insert(key, Item::Value(json_to_edit(value)?));
                }
            }
            Ok(Outcome {
                slide: Some(*slide),
                select: vec![*index],
            })
        }),
        Action::SetParagraph {
            slide,
            index,
            paragraph,
            text,
        } => edit_shapes(dir, &project, *slide, |shapes| {
            let shape = shapes
                .get_mut(*index)
                .ok_or_else(|| miette!("shape {index} does not exist"))?;
            let paras = shape
                .get_mut("paragraphs")
                .and_then(Item::as_array_of_tables_mut)
                .ok_or_else(|| miette!("shape {index} has no paragraphs"))?;
            let para = paras
                .get_mut(*paragraph)
                .ok_or_else(|| miette!("paragraph {paragraph} does not exist"))?;
            para.insert("text", Item::Value(Value::from(text.clone())));
            Ok(Outcome {
                slide: Some(*slide),
                select: vec![*index],
            })
        }),
        Action::AddText {
            slide,
            x,
            y,
            w,
            h,
            text,
        } => edit_shapes(dir, &project, *slide, |shapes| {
            let mut table = Table::new();
            table.insert("type", Item::Value(Value::from("text")));
            set_coord(&mut table, "x", *x, 13.333)?;
            set_coord(&mut table, "y", *y, 7.5)?;
            set_coord(&mut table, "w", *w, 13.333)?;
            set_coord(&mut table, "h", *h, 7.5)?;
            table.insert(
                "text",
                Item::Value(Value::from(
                    text.clone().unwrap_or_else(|| "New text".into()),
                )),
            );
            let index = shapes.len();
            shapes.push(table);
            Ok(Outcome {
                slide: Some(*slide),
                select: vec![index],
            })
        }),
        Action::DeleteShapes { slide, indices } => edit_shapes(dir, &project, *slide, |shapes| {
            let mut sorted = indices.clone();
            sorted.sort_unstable();
            sorted.dedup();
            for &index in sorted.iter().rev() {
                if index >= shapes.len() {
                    return Err(miette!("shape {index} does not exist"));
                }
                shapes.remove(index);
            }
            Ok(Outcome {
                slide: Some(*slide),
                select: Vec::new(),
            })
        }),
        Action::DuplicateShapes { slide, indices } => {
            let (width, height) = slide_size(&project)?;
            edit_shapes(dir, &project, *slide, |shapes| {
                let mut sorted = indices.clone();
                sorted.sort_unstable();
                sorted.dedup();
                let mut select = Vec::new();
                for &index in &sorted {
                    let mut table = shapes
                        .get(index)
                        .cloned()
                        .ok_or_else(|| miette!("shape {index} does not exist"))?;
                    let x = coord_inches(table.get("x"), width).unwrap_or(0.0) + 0.25;
                    let y = coord_inches(table.get("y"), height).unwrap_or(0.0) + 0.25;
                    if table.get("x").is_some() {
                        set_coord(&mut table, "x", x, width)?;
                    }
                    if table.get("y").is_some() {
                        set_coord(&mut table, "y", y, height)?;
                    }
                    select.push(shapes.len());
                    shapes.push(table);
                }
                Ok(Outcome {
                    slide: Some(*slide),
                    select,
                })
            })
        }
        Action::ZOrder { slide, indices, op } => edit_shapes(dir, &project, *slide, |shapes| {
            let mut tables: Vec<Table> = shapes.iter().cloned().collect();
            let mut selected: Vec<usize> = indices.clone();
            selected.sort_unstable();
            selected.dedup();
            let mut chosen = Vec::new();
            for &index in selected.iter().rev() {
                if index < tables.len() {
                    chosen.push(tables.remove(index));
                }
            }
            match op.as_str() {
                "front" => {
                    chosen.reverse();
                    tables.extend(chosen);
                }
                "back" => {
                    chosen.reverse();
                    chosen.extend(tables);
                    tables = chosen;
                }
                other => return Err(miette!("unknown z-order operation `{other}`")),
            }
            for (slot, table) in tables.iter_mut().enumerate() {
                table.set_position(Some(slot as isize));
            }
            shapes.clear();
            for table in tables {
                shapes.push(table);
            }
            let select = if op == "front" {
                (shapes.len().saturating_sub(selected.len())..shapes.len()).collect()
            } else {
                (0..selected.len()).collect()
            };
            Ok(Outcome {
                slide: Some(*slide),
                select,
            })
        }),
        Action::AddSlide { after } => {
            let files = slide_files(&project);
            if files.is_empty() {
                return Err(miette!("cannot add a slide to an empty deck"));
            }
            let after = (*after).min(files.len() - 1);
            let source_file = files[after].1.clone();
            let source: crate::model::Slide = load_slide(&project, &source_file)?;
            let master = source
                .master
                .or_else(|| {
                    project
                        .main
                        .sections
                        .iter()
                        .flat_map(|s| s.slides.iter())
                        .find_map(|file| load_slide(&project, file).ok()?.master)
                })
                .ok_or_else(|| miette!("cannot determine a master for the new slide"))?;
            let file = unique_slide_name(dir, "new-slide");
            let path = project.slide_path(&file);
            std::fs::write(&path, format!("master = \"{master}\"\n"))
                .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
            let (section, relative) = slide_position(&project, after)?;
            edit_main(dir, |doc| {
                let sections = sections_mut(doc)?;
                let section = sections
                    .get_mut(section)
                    .ok_or_else(|| miette!("section {section} does not exist"))?;
                let slides = slides_array(section)?;
                slides.insert(relative + 1, Value::from(file.as_str()));
                Ok(())
            })?;
            Ok(Outcome {
                slide: Some(after + 1),
                select: Vec::new(),
            })
        }
        Action::DeleteSlide { slide } => {
            let files = slide_files(&project);
            if files.len() <= 1 {
                return Err(miette!("cannot delete the last slide"));
            }
            let file = files
                .get(*slide)
                .ok_or_else(|| miette!("slide {slide} does not exist"))?
                .1
                .clone();
            let (section, relative) = slide_position(&project, *slide)?;
            edit_main(dir, |doc| {
                let sections = sections_mut(doc)?;
                let table = sections
                    .get_mut(section)
                    .ok_or_else(|| miette!("section {section} does not exist"))?;
                let slides = slides_array(table)?;
                if relative < slides.len() {
                    slides.remove(relative);
                }
                Ok(())
            })?;
            let path = project.slide_path(&file);
            std::fs::remove_file(&path)
                .map_err(|e| miette!("cannot remove `{}`: {e}", path.display()))?;
            Ok(Outcome {
                slide: Some((*slide).min(files.len() - 2)),
                select: Vec::new(),
            })
        }
        Action::DuplicateSlide { slide } => {
            let files = slide_files(&project);
            let file = files
                .get(*slide)
                .ok_or_else(|| miette!("slide {slide} does not exist"))?
                .1
                .clone();
            let contents = std::fs::read_to_string(project.slide_path(&file))
                .map_err(|e| miette!("cannot read `{file}`: {e}"))?;
            let stem = Path::new(&file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "slide".into());
            let new_file = unique_slide_name(dir, &format!("{stem}-copy"));
            let path = project.slide_path(&new_file);
            std::fs::write(&path, contents)
                .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
            let (section, relative) = slide_position(&project, *slide)?;
            edit_main(dir, |doc| {
                let sections = sections_mut(doc)?;
                let table = sections
                    .get_mut(section)
                    .ok_or_else(|| miette!("section {section} does not exist"))?;
                let slides = slides_array(table)?;
                slides.insert(relative + 1, Value::from(new_file.as_str()));
                Ok(())
            })?;
            Ok(Outcome {
                slide: Some(slide + 1),
                select: Vec::new(),
            })
        }
        Action::MoveSlide { from, to } => {
            let files = slide_files(&project);
            if *from >= files.len() || *to >= files.len() {
                return Err(miette!("slide index out of range"));
            }
            let mut flat = files;
            let (_, file) = flat.remove(*from);
            let target_section = flat
                .get(*to)
                .map(|(section, _)| *section)
                .unwrap_or_else(|| flat.last().map(|(section, _)| *section).unwrap_or(0));
            let insert_at = (*to).min(flat.len());
            flat.insert(insert_at, (target_section, file));
            edit_main(dir, |doc| {
                let sections = sections_mut(doc)?;
                for section in sections.iter_mut() {
                    slides_array(section)?.clear();
                }
                let count = sections.len();
                for (section, file) in flat {
                    let section = section.min(count.saturating_sub(1));
                    let table = sections
                        .get_mut(section)
                        .ok_or_else(|| miette!("section {section} does not exist"))?;
                    slides_array(table)?.push(Value::from(file));
                }
                Ok(())
            })?;
            Ok(Outcome {
                slide: Some(*to),
                select: Vec::new(),
            })
        }
    }
}

fn edit_shapes<F>(dir: &Path, project: &Project, slide: usize, f: F) -> Result<Outcome>
where
    F: FnOnce(&mut ArrayOfTables) -> Result<Outcome>,
{
    let file = slide_files(project)
        .get(slide)
        .ok_or_else(|| miette!("slide {slide} does not exist"))?
        .1
        .clone();
    let path = project.slide_path(&file);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| miette!("cannot read `{}`: {e}", path.display()))?;
    let mut doc: DocumentMut = raw
        .parse()
        .map_err(|e| miette!("cannot parse `{}`: {e}", path.display()))?;
    let shapes = doc
        .entry("shapes")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or_else(|| miette!("`shapes` must be an array of tables"))?;
    let outcome = f(shapes)?;
    std::fs::write(&path, doc.to_string())
        .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
    let _ = dir;
    Ok(outcome)
}

fn edit_main<F>(dir: &Path, f: F) -> Result<()>
where
    F: FnOnce(&mut DocumentMut) -> Result<()>,
{
    let path = dir.join("main.toml");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| miette!("cannot read `{}`: {e}", path.display()))?;
    let mut doc: DocumentMut = raw
        .parse()
        .map_err(|e| miette!("cannot parse `{}`: {e}", path.display()))?;
    f(&mut doc)?;
    std::fs::write(&path, doc.to_string())
        .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
    Ok(())
}

fn sections_mut(doc: &mut DocumentMut) -> Result<&mut ArrayOfTables> {
    doc.entry("sections")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or_else(|| miette!("`[[sections]]` must be an array of tables"))
}

fn slides_array(section: &mut Table) -> Result<&mut Array> {
    section
        .entry("slides")
        .or_insert(Item::Value(Value::Array(Array::new())))
        .as_array_mut()
        .ok_or_else(|| miette!("`sections.slides` must be an array"))
}

fn slide_files(project: &Project) -> Vec<(usize, String)> {
    project
        .main
        .sections
        .iter()
        .enumerate()
        .flat_map(|(section, s)| s.slides.iter().cloned().map(move |file| (section, file)))
        .collect()
}

fn slide_position(project: &Project, index: usize) -> Result<(usize, usize)> {
    let mut at = 0;
    for (section, s) in project.main.sections.iter().enumerate() {
        if index < at + s.slides.len() {
            return Ok((section, index - at));
        }
        at += s.slides.len();
    }
    Err(miette!("slide {index} does not exist"))
}

fn load_slide(project: &Project, file: &str) -> Result<crate::model::Slide> {
    let path = project.slide_path(file);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| miette!("cannot read `{}`: {e}", path.display()))?;
    toml::from_str(&raw).map_err(|e| miette!("cannot parse `{}`: {e}", path.display()))
}

fn unique_slide_name(dir: &Path, stem: &str) -> String {
    let sane: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let sane = sane.trim_matches('-');
    let base = if sane.is_empty() { "slide" } else { sane };
    for n in 1.. {
        let name = if n == 1 {
            format!("{base}.toml")
        } else {
            format!("{base}-{n}.toml")
        };
        if !dir.join("slides").join(&name).exists() {
            return name;
        }
    }
    unreachable!()
}

fn slide_size(project: &Project) -> Result<(f64, f64)> {
    let width = project
        .main
        .presentation
        .width
        .inches(0)
        .map_err(|e| miette!("invalid slide width: {e}"))?;
    let height = project
        .main
        .presentation
        .height
        .inches(0)
        .map_err(|e| miette!("invalid slide height: {e}"))?;
    Ok((width, height))
}

fn json_to_edit(value: &Json) -> Result<Value> {
    match value {
        Json::Null => Err(miette!(
            "null values remove a key and are handled separately"
        )),
        Json::Bool(b) => Ok(Value::from(*b)),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::from(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::from(f))
            } else {
                Err(miette!("unsupported number"))
            }
        }
        Json::String(s) => Ok(Value::from(s.as_str())),
        Json::Array(items) => {
            let mut out = Array::new();
            for item in items {
                out.push(json_to_edit(item)?);
            }
            Ok(Value::Array(out))
        }
        Json::Object(map) => {
            let mut out = InlineTable::new();
            for (key, item) in map {
                out.insert(key.as_str(), json_to_edit(item)?);
            }
            Ok(Value::InlineTable(out))
        }
    }
}

fn coord_inches(item: Option<&Item>, slide_inches: f64) -> Option<f64> {
    let value = item?.as_value()?;
    match value {
        Value::Integer(emu) => Some(*emu.value() as f64 / EMU_PER_IN),
        Value::Float(inches) => Some(*inches.value()),
        Value::String(text) => parse_coord_str(text.value(), slide_inches),
        _ => None,
    }
}

fn parse_coord_str(text: &str, slide_inches: f64) -> Option<f64> {
    let text = text.trim();
    if let Some(pct) = text.strip_suffix('%') {
        return pct
            .trim()
            .parse::<f64>()
            .ok()
            .map(|p| p / 100.0 * slide_inches);
    }
    for (unit, factor) in [
        ("in", 1.0),
        ("cm", 1.0 / 2.54),
        ("mm", 1.0 / 25.4),
        ("pt", 1.0 / 72.0),
    ] {
        if let Some(number) = text.strip_suffix(unit) {
            return number
                .trim()
                .parse::<f64>()
                .ok()
                .map(|value| value * factor);
        }
    }
    text.parse::<f64>().ok().map(|emu| emu / EMU_PER_IN)
}

fn set_coord(table: &mut Table, key: &str, inches: f64, slide_inches: f64) -> Result<()> {
    let inches = inches.max(0.0);
    let value = match table.get(key).and_then(Item::as_value) {
        Some(Value::Integer(_)) => Value::from((inches * EMU_PER_IN).round() as i64),
        Some(Value::String(raw)) => {
            let raw = raw.value().trim();
            if raw.ends_with('%') {
                Value::from(format!("{}%", fmt_num(inches / slide_inches * 100.0)))
            } else if let Some(unit) = ["in", "cm", "mm", "pt"]
                .into_iter()
                .find(|unit| raw.ends_with(unit))
            {
                let value = match unit {
                    "cm" => inches * 2.54,
                    "mm" => inches * 25.4,
                    "pt" => inches * 72.0,
                    _ => inches,
                };
                Value::from(format!("{}{unit}", fmt_num(value)))
            } else {
                Value::from(format_emu(inches))
            }
        }
        _ => Value::from(format!("{}in", fmt_num(inches))),
    };
    table.insert(key, Item::Value(value));
    Ok(())
}

fn format_emu(inches: f64) -> String {
    format!("{}", (inches * EMU_PER_IN).round() as i64)
}

fn fmt_num(value: f64) -> String {
    let mut text = format!("{value:.3}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" { "0".into() } else { text }
}
