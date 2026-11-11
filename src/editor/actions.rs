//! Semantic editor actions and their TOML mutations.

use std::collections::BTreeMap;
use std::path::Path;

use miette::{Result, miette};
use serde::{Deserialize, Deserializer};
use serde_json::Value as Json;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::model::Project;

const EMU_PER_IN: f64 = 914_400.0;
/// Decoded image payloads are capped at 20 MB. The HTTP body limit is larger
/// to allow for base64 overhead.
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

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
    SetSlide {
        slide: usize,
        /// `None` means keep the current background, `Some(None)` clears it,
        /// and `Some(Some(value))` sets a color string or background table.
        #[serde(default, deserialize_with = "double_option")]
        background: Option<Option<Json>>,
        /// `None` means keep the current notes; an empty string clears them.
        #[serde(default)]
        notes: Option<String>,
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
    AddShape {
        slide: usize,
        shape_type: String,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        #[serde(default)]
        text: Option<String>,
    },
    AddImage {
        slide: usize,
        /// Original file name; its extension chooses the media type.
        name: String,
        /// Base64 payload, with or without a `data:` prefix.
        data: String,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
        #[serde(default)]
        w: Option<f64>,
        #[serde(default)]
        h: Option<f64>,
    },
    ReplaceImage {
        slide: usize,
        index: usize,
        name: String,
        data: String,
    },
    Align {
        slide: usize,
        indices: Vec<usize>,
        /// `left`, `center`, `right`, `top`, `middle` or `bottom`.
        edge: String,
    },
    Distribute {
        slide: usize,
        indices: Vec<usize>,
        /// `horizontal` or `vertical`.
        axis: String,
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
        #[serde(default)]
        after: Option<usize>,
        #[serde(default)]
        section: Option<usize>,
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
        #[serde(default)]
        section: Option<usize>,
    },
    AddSection {
        #[serde(default)]
        after: Option<usize>,
        #[serde(default)]
        title: Option<String>,
    },
    RenameSection {
        section: usize,
        title: String,
    },
    DeleteSection {
        section: usize,
    },
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub slide: Option<usize>,
    pub select: Vec<usize>,
    pub section: Option<usize>,
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
                    section: None,
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
                    section: None,
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
                section: None,
            })
        }),
        Action::SetSlide {
            slide,
            background,
            notes,
        } => edit_slide_doc(&project, *slide, |doc| {
            if let Some(background) = background {
                match background {
                    None => {
                        doc.remove("background");
                    }
                    Some(value) => {
                        doc.insert("background", Item::Value(json_to_edit(value)?));
                    }
                }
            }
            if let Some(notes) = notes {
                if notes.trim().is_empty() {
                    doc.remove("notes");
                } else {
                    doc.insert("notes", Item::Value(Value::from(notes.as_str())));
                }
            }
            Ok(Outcome {
                slide: Some(*slide),
                select: Vec::new(),
                section: None,
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
                section: None,
            })
        }),
        Action::AddText {
            slide,
            x,
            y,
            w,
            h,
            text,
        } => {
            let (width, height) = slide_size(&project)?;
            edit_shapes(dir, &project, *slide, |shapes| {
                let mut table = Table::new();
                table.insert("type", Item::Value(Value::from("text")));
                set_coord(&mut table, "x", *x, width)?;
                set_coord(&mut table, "y", *y, height)?;
                set_coord(&mut table, "w", *w, width)?;
                set_coord(&mut table, "h", *h, height)?;
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
                    section: None,
                })
            })
        }
        Action::AddShape {
            slide,
            shape_type,
            x,
            y,
            w,
            h,
            text,
        } => {
            let preset = crate::opts::canonical_preset(shape_type)
                .ok_or_else(|| miette!("`{shape_type}` is not a shape preset"))?;
            if *w <= 0.0 || *h < 0.0 {
                return Err(miette!(
                    "width must be positive and height must not be negative"
                ));
            }
            let (width, height) = slide_size(&project)?;
            edit_shapes(dir, &project, *slide, |shapes| {
                let mut table = Table::new();
                table.insert("type", Item::Value(Value::from(preset)));
                set_coord(&mut table, "x", x.max(0.0), width)?;
                set_coord(&mut table, "y", y.max(0.0), height)?;
                set_coord(&mut table, "w", *w, width)?;
                set_coord(&mut table, "h", *h, height)?;
                if let Some(text) = text.as_deref().filter(|text| !text.trim().is_empty()) {
                    table.insert("text", Item::Value(Value::from(text)));
                }
                let index = shapes.len();
                shapes.push(table);
                Ok(Outcome {
                    slide: Some(*slide),
                    select: vec![index],
                    section: None,
                })
            })
        }
        Action::AddImage {
            slide,
            name,
            data,
            x,
            y,
            w,
            h,
        } => {
            if slide_files(&project).get(*slide).is_none() {
                return Err(miette!("slide {slide} does not exist"));
            }
            if w.is_some_and(|w| w <= 0.0) || h.is_some_and(|h| h <= 0.0) {
                return Err(miette!("image width and height must be positive"));
            }
            let (src, aspect) = store_image(dir, name, data)?;
            let (slide_w, slide_h) = slide_size(&project)?;
            let (w, h) = match (*w, *h) {
                (Some(w), Some(h)) => (w, h),
                (Some(w), None) => (w, w / aspect),
                (None, Some(h)) => (h * aspect, h),
                (None, None) => {
                    let w = (slide_w * 0.45).clamp(1.0, 6.0);
                    (w, w / aspect)
                }
            };
            if w <= 0.0 || h <= 0.0 {
                return Err(miette!("image width and height must be positive"));
            }
            let x = x.unwrap_or_else(|| ((slide_w - w) / 2.0).max(0.0));
            let y = y.unwrap_or_else(|| ((slide_h - h) / 2.0).max(0.0));
            edit_shapes(dir, &project, *slide, |shapes| {
                let mut table = Table::new();
                table.insert("type", Item::Value(Value::from("image")));
                table.insert("src", Item::Value(Value::from(src.as_str())));
                set_coord(&mut table, "x", x.max(0.0), slide_w)?;
                set_coord(&mut table, "y", y.max(0.0), slide_h)?;
                set_coord(&mut table, "w", w, slide_w)?;
                set_coord(&mut table, "h", h, slide_h)?;
                let index = shapes.len();
                shapes.push(table);
                Ok(Outcome {
                    slide: Some(*slide),
                    select: vec![index],
                    section: None,
                })
            })
        }
        Action::ReplaceImage {
            slide,
            index,
            name,
            data,
        } => {
            if slide_files(&project).get(*slide).is_none() {
                return Err(miette!("slide {slide} does not exist"));
            }
            let (src, _) = store_image(dir, name, data)?;
            edit_shapes(dir, &project, *slide, |shapes| {
                let shape = shapes
                    .get_mut(*index)
                    .ok_or_else(|| miette!("shape {index} does not exist"))?;
                if shape.get("type").and_then(Item::as_str) != Some("image") {
                    return Err(miette!("shape {index} is not an image"));
                }
                shape.insert("src", Item::Value(Value::from(src.as_str())));
                Ok(Outcome {
                    slide: Some(*slide),
                    select: vec![*index],
                    section: None,
                })
            })
        }
        Action::Align {
            slide,
            indices,
            edge,
        } => align_shapes(dir, &project, *slide, indices, edge),
        Action::Distribute {
            slide,
            indices,
            axis,
        } => distribute_shapes(dir, &project, *slide, indices, axis),
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
                section: None,
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
                    section: None,
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
                section: None,
            })
        }),
        Action::AddSlide { after, section } => {
            let files = slide_files(&project);
            if files.is_empty() {
                return Err(miette!("cannot add a slide to an empty deck"));
            }
            let (target_section, insert_at, source_index) = if let Some(section) = section {
                let section = *section;
                if section >= project.main.sections.len() {
                    return Err(miette!("section {section} does not exist"));
                }
                let source_index = after.unwrap_or_else(|| {
                    if project.main.sections[section].slides.is_empty() {
                        0
                    } else {
                        project
                            .main
                            .sections
                            .iter()
                            .take(section)
                            .map(|section| section.slides.len())
                            .sum::<usize>()
                    }
                });
                (
                    section,
                    project.main.sections[section].slides.len(),
                    source_index.min(files.len() - 1),
                )
            } else {
                let after = after.unwrap_or(files.len() - 1).min(files.len() - 1);
                let (section, relative) = slide_position(&project, after)?;
                (section, relative + 1, after)
            };
            let source_file = files[source_index].1.clone();
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
            edit_main(dir, |doc| {
                let sections = sections_mut(doc)?;
                let section = sections
                    .get_mut(target_section)
                    .ok_or_else(|| miette!("section {target_section} does not exist"))?;
                let slides = slides_array(section)?;
                let at = insert_at.min(slides.len());
                slides.insert(at, Value::from(file.as_str()));
                Ok(())
            })?;
            let flat = project
                .main
                .sections
                .iter()
                .take(target_section)
                .map(|section| section.slides.len())
                .sum::<usize>()
                + insert_at;
            Ok(Outcome {
                slide: Some(flat),
                select: Vec::new(),
                section: Some(target_section),
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
                section: None,
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
                section: None,
            })
        }
        Action::MoveSlide { from, to, section } => {
            let files = slide_files(&project);
            if *from >= files.len() {
                return Err(miette!("slide index out of range"));
            }
            if let Some(target_section) = section {
                let target_section = *target_section;
                if target_section >= project.main.sections.len() {
                    return Err(miette!("section {target_section} does not exist"));
                }
                let (source_section, source_relative) = slide_position(&project, *from)?;
                let file = files[*from].1.clone();
                let mut groups: Vec<Vec<String>> = project
                    .main
                    .sections
                    .iter()
                    .map(|section| section.slides.clone())
                    .collect();
                groups[source_section].remove(source_relative);
                let at = (*to).min(groups[target_section].len());
                groups[target_section].insert(at, file);
                edit_main(dir, |doc| {
                    let sections = sections_mut(doc)?;
                    let count = sections.len();
                    if count != groups.len() {
                        return Err(miette!("section count changed while editing"));
                    }
                    for (index, group) in groups.iter().enumerate() {
                        let table = sections
                            .get_mut(index)
                            .ok_or_else(|| miette!("section {index} does not exist"))?;
                        let slides = slides_array(table)?;
                        slides.clear();
                        for file in group {
                            slides.push(Value::from(file.as_str()));
                        }
                    }
                    Ok(())
                })?;
                let flat = groups
                    .iter()
                    .take(target_section)
                    .map(|group| group.len())
                    .sum::<usize>()
                    + at;
                return Ok(Outcome {
                    slide: Some(flat),
                    select: Vec::new(),
                    section: Some(target_section),
                });
            }
            if *to >= files.len() {
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
                section: None,
            })
        }
        Action::AddSection { after, title } => {
            let at = after
                .map(|section| (section + 1).min(project.main.sections.len()))
                .unwrap_or(project.main.sections.len());
            let title = title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .unwrap_or("New section")
                .to_string();
            edit_main(dir, |doc| {
                let sections = sections_mut(doc)?;
                let mut table = Table::new();
                table.insert("title", Item::Value(Value::from(title.as_str())));
                table.insert("slides", Item::Value(Value::Array(Array::new())));
                sections.insert(at.min(sections.len()), table);
                Ok(())
            })?;
            Ok(Outcome {
                slide: None,
                select: Vec::new(),
                section: Some(at),
            })
        }
        Action::RenameSection { section, title } => {
            let title = title.trim();
            if title.is_empty() {
                return Err(miette!("a section title must not be empty"));
            }
            edit_main(dir, |doc| {
                let sections = sections_mut(doc)?;
                let table = sections
                    .get_mut(*section)
                    .ok_or_else(|| miette!("section {section} does not exist"))?;
                table.insert("title", Item::Value(Value::from(title)));
                Ok(())
            })?;
            Ok(Outcome {
                slide: None,
                select: Vec::new(),
                section: Some(*section),
            })
        }
        Action::DeleteSection { section } => {
            if project.main.sections.len() <= 1 {
                return Err(miette!("cannot delete the last section"));
            }
            let section = *section;
            if section >= project.main.sections.len() {
                return Err(miette!("section {section} does not exist"));
            }
            edit_main(dir, |doc| {
                let sections = sections_mut(doc)?;
                let removed = sections.remove(section);
                let moved: Vec<Value> = removed
                    .get("slides")
                    .and_then(Item::as_array)
                    .map(|array| array.iter().cloned().collect())
                    .unwrap_or_default();
                if !moved.is_empty() {
                    let destination = if section > 0 { section - 1 } else { 0 };
                    let table = sections
                        .get_mut(destination)
                        .ok_or_else(|| miette!("section {destination} does not exist"))?;
                    let slides = slides_array(table)?;
                    if section == 0 {
                        let existing: Vec<Value> = slides.iter().cloned().collect();
                        slides.clear();
                        for file in moved {
                            slides.push(file);
                        }
                        for file in existing {
                            slides.push(file);
                        }
                    } else {
                        for file in moved {
                            slides.push(file);
                        }
                    }
                }
                Ok(())
            })?;
            Ok(Outcome {
                slide: None,
                select: Vec::new(),
                section: Some(section.min(project.main.sections.len().saturating_sub(2))),
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

fn edit_slide_doc<F>(project: &Project, slide: usize, f: F) -> Result<Outcome>
where
    F: FnOnce(&mut DocumentMut) -> Result<Outcome>,
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
    let outcome = f(&mut doc)?;
    std::fs::write(&path, doc.to_string())
        .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
    Ok(outcome)
}

#[derive(Clone, Copy)]
struct Bounds {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn shape_bounds(shapes: &ArrayOfTables, index: usize, width: f64, height: f64) -> Result<Bounds> {
    let shape = shapes
        .get(index)
        .ok_or_else(|| miette!("shape {index} does not exist"))?;
    if shape.get("placeholder").is_some() {
        return Err(miette!(
            "placeholder shapes get their geometry from the master and cannot be moved"
        ));
    }
    let x = coord_inches(shape.get("x"), width)
        .ok_or_else(|| miette!("shape {index} needs an explicit x coordinate"))?;
    let y = coord_inches(shape.get("y"), height)
        .ok_or_else(|| miette!("shape {index} needs an explicit y coordinate"))?;
    let w = coord_inches(shape.get("w"), width)
        .ok_or_else(|| miette!("shape {index} needs an explicit width"))?;
    let h = coord_inches(shape.get("h"), height)
        .ok_or_else(|| miette!("shape {index} needs an explicit height"))?;
    Ok(Bounds { x, y, w, h })
}

fn align_shapes(
    dir: &Path,
    project: &Project,
    slide: usize,
    indices: &[usize],
    edge: &str,
) -> Result<Outcome> {
    let (width, height) = slide_size(project)?;
    let mut chosen: Vec<usize> = indices.to_vec();
    chosen.sort_unstable();
    chosen.dedup();
    if chosen.len() < 2 {
        return Err(miette!("select at least two shapes to align"));
    }
    edit_shapes(dir, project, slide, |shapes| {
        let rects = chosen
            .iter()
            .map(|&index| shape_bounds(shapes, index, width, height).map(|bounds| (index, bounds)))
            .collect::<Result<Vec<_>>>()?;
        let left = rects.iter().fold(f64::INFINITY, |acc, (_, b)| acc.min(b.x));
        let right = rects
            .iter()
            .fold(f64::NEG_INFINITY, |acc, (_, b)| acc.max(b.x + b.w));
        let top = rects.iter().fold(f64::INFINITY, |acc, (_, b)| acc.min(b.y));
        let bottom = rects
            .iter()
            .fold(f64::NEG_INFINITY, |acc, (_, b)| acc.max(b.y + b.h));
        let center_x = (left + right) / 2.0;
        let center_y = (top + bottom) / 2.0;
        for &(index, bounds) in &rects {
            let shape = shapes
                .get_mut(index)
                .ok_or_else(|| miette!("shape {index} does not exist"))?;
            match edge {
                "left" => set_coord(shape, "x", left, width)?,
                "center" => set_coord(shape, "x", center_x - bounds.w / 2.0, width)?,
                "right" => set_coord(shape, "x", right - bounds.w, width)?,
                "top" => set_coord(shape, "y", top, height)?,
                "middle" => set_coord(shape, "y", center_y - bounds.h / 2.0, height)?,
                "bottom" => set_coord(shape, "y", bottom - bounds.h, height)?,
                other => {
                    return Err(miette!(
                        "unknown align edge `{other}`; use left, center, right, top, middle or bottom"
                    ));
                }
            }
        }
        Ok(Outcome {
            slide: Some(slide),
            select: chosen,
            section: None,
        })
    })
}

fn distribute_shapes(
    dir: &Path,
    project: &Project,
    slide: usize,
    indices: &[usize],
    axis: &str,
) -> Result<Outcome> {
    let (width, height) = slide_size(project)?;
    let mut chosen: Vec<usize> = indices.to_vec();
    chosen.sort_unstable();
    chosen.dedup();
    if chosen.len() < 3 {
        return Err(miette!(
            "select at least three shapes to distribute them evenly"
        ));
    }
    if axis != "horizontal" && axis != "vertical" {
        return Err(miette!(
            "unknown distribute axis `{axis}`; use horizontal or vertical"
        ));
    }
    edit_shapes(dir, project, slide, |shapes| {
        let mut rects = chosen
            .iter()
            .map(|&index| shape_bounds(shapes, index, width, height).map(|bounds| (index, bounds)))
            .collect::<Result<Vec<_>>>()?;
        let horizontal = axis == "horizontal";
        if horizontal {
            rects.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
        } else {
            rects.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
        }
        let first = rects
            .first()
            .map(|(_, b)| *b)
            .ok_or_else(|| miette!("no shapes to distribute"))?;
        let last = rects
            .last()
            .map(|(_, b)| *b)
            .ok_or_else(|| miette!("no shapes to distribute"))?;
        let span = if horizontal {
            (last.x + last.w) - first.x
        } else {
            (last.y + last.h) - first.y
        };
        let total: f64 = rects
            .iter()
            .map(|(_, b)| if horizontal { b.w } else { b.h })
            .sum();
        let gap = (span - total) / (rects.len() - 1) as f64;
        let mut cursor = if horizontal {
            first.x + first.w + gap
        } else {
            first.y + first.h + gap
        };
        for (position, &(index, bounds)) in rects.iter().enumerate() {
            if position == 0 || position == rects.len() - 1 {
                continue;
            }
            let shape = shapes
                .get_mut(index)
                .ok_or_else(|| miette!("shape {index} does not exist"))?;
            if horizontal {
                set_coord(shape, "x", cursor, width)?;
                cursor += bounds.w + gap;
            } else {
                set_coord(shape, "y", cursor, height)?;
                cursor += bounds.h + gap;
            }
        }
        Ok(Outcome {
            slide: Some(slide),
            select: chosen,
            section: None,
        })
    })
}

fn store_image(dir: &Path, name: &str, data: &str) -> Result<(String, f64)> {
    let clean_name = Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| miette!("`{name}` is not a valid image file name"))?;
    crate::media::mime_for(clean_name).map_err(|e| miette!("{e}"))?;
    let payload = data.split_once(',').map(|(_, bytes)| bytes).unwrap_or(data);
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|e| miette!("invalid image data: {e}"))?;
    if bytes.is_empty() {
        return Err(miette!("the image is empty"));
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(miette!("the image is too large (maximum 20 MB)"));
    }
    let media_dir = dir.join("media");
    std::fs::create_dir_all(&media_dir)
        .map_err(|e| miette!("cannot create `{}`: {e}", media_dir.display()))?;
    let file = unique_media_name(&media_dir, clean_name);
    let path = media_dir.join(&file);
    std::fs::write(&path, &bytes).map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
    let aspect = match crate::media::image_aspect(&path) {
        Ok(aspect) => aspect,
        Err(err) => {
            let _ = std::fs::remove_file(&path);
            return Err(miette!("{err}"));
        }
    };
    Ok((format!("media/{file}"), aspect))
}

fn unique_media_name(dir: &Path, name: &str) -> String {
    let path = Path::new(name);
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("image");
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
    let base = if sane.is_empty() { "image" } else { sane };
    for n in 1.. {
        let name = if n == 1 {
            format!("{base}.{ext}")
        } else {
            format!("{base}-{n}.{ext}")
        };
        if !dir.join(&name).exists() {
            return name;
        }
    }
    unreachable!()
}

fn double_option<'de, D>(deserializer: D) -> std::result::Result<Option<Option<Json>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Json>::deserialize(deserializer).map(Some)
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
