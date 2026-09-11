//! Orchestration: turn a validated config + parsed slides into an OPC package.

use std::collections::HashMap;
use std::path::Path;

use crate::config::Config;
use crate::content::Slide;
use crate::error::Result;
use crate::ooxml::package::Package;
use crate::ooxml::rels;
use crate::parts::{core, layout, master, notes, presentation, props, slide, theme};

/// Build the whole package for a project rooted at `project`.
pub fn build(project: &Path, config: &Config, slides: &[Slide]) -> Result<Package> {
    let mut pkg = Package::new();

    // Theme.
    pkg.add_part(theme::URI, theme::build(&config.theme));

    // Layouts, in deterministic (sorted) order. Layout ids start after the
    // master id and stay unique across the package.
    let mut layout_ids: Vec<(String, String, u32)> = Vec::new(); // (key, uri, id)
    for (i, (key, layout_def)) in config.layouts.iter().enumerate() {
        let uri = layout::uri(i + 1);
        let id = 2147483649 + i as u32;
        pkg.add_part(&uri, layout::build(key, layout_def));
        pkg.relate(&uri, rels::SLIDE_MASTER, master::URI);
        layout_ids.push((key.clone(), uri, id));
    }

    // Master: theme rel, then one rel per layout (referenced by r:id).
    pkg.relate(master::URI, rels::THEME, theme::URI);
    let layout_refs: Vec<(u32, String)> = layout_ids
        .iter()
        .map(|(_, uri, id)| (*id, pkg.relate(master::URI, rels::SLIDE_LAYOUT, uri)))
        .collect();
    pkg.add_part(
        master::URI,
        master::build(&config.presentation.name, &layout_refs),
    );

    // Presentation-level rel to the master (rId1). Optional parts are added
    // after the slides further down.
    let master_rid = pkg.relate(presentation::URI, rels::SLIDE_MASTER, master::URI);

    // Slides.
    let media_dir = project.join("src").join("media");
    let mut slide_refs: Vec<(u32, String)> = Vec::new();
    let mut any_notes = false;
    let mut notes_slide_num = 0usize;

    for (i, content) in slides.iter().enumerate() {
        let uri = slide::uri(i + 1);
        let layout_key = &content.layout;
        let layout_uri = layout_ids
            .iter()
            .find(|(key, _, _)| key == layout_key)
            .map(|(_, uri, _)| uri.clone())
            .expect("layout resolved during load");
        pkg.relate(&uri, rels::SLIDE_LAYOUT, &layout_uri);

        // Pictures: add the media part once and a rel from this slide.
        let mut image_rids: HashMap<String, String> = HashMap::new();
        for filename in &content.pictures {
            if !image_rids.contains_key(filename) {
                let media_uri = format!("ppt/media/{filename}");
                if pkg.part(&media_uri).is_none() {
                    let bytes = std::fs::read(media_dir.join(filename)).map_err(|e| {
                        miette::miette!(
                            "slide `{}`: cannot read `src/media/{filename}`: {e}",
                            content.source
                        )
                    })?;
                    pkg.add_part(&media_uri, bytes);
                }
                let rid = pkg.relate(&uri, rels::IMAGE, &media_uri);
                image_rids.insert(filename.clone(), rid);
            }
        }

        let layout_def = config.layout(layout_key)?;
        pkg.add_part(
            &uri,
            slide::build(content, layout_key, layout_def, &config.theme, &image_rids)?,
        );

        // Notes.
        if let Some(text) = &content.notes {
            any_notes = true;
            notes_slide_num += 1;
            let notes_uri = notes::slide_uri(notes_slide_num);
            pkg.relate(&uri, rels::NOTES_SLIDE, &notes_uri);
            pkg.relate(&notes_uri, rels::SLIDE, &uri);
            pkg.relate(&notes_uri, rels::NOTES_MASTER, notes::MASTER_URI);
            pkg.add_part(&notes_uri, notes::build_slide(text));
        }

        let rid = pkg.relate(presentation::URI, rels::SLIDE, &uri);
        slide_refs.push((256 + i as u32, rid));
    }

    // Optional presentation parts come after the slides, so the first slide is
    // rId2 (right after the master) and theme precedes tableStyles — the
    // ordering PowerPoint (and its validators) expect.
    pkg.relate(presentation::URI, rels::THEME, theme::URI);
    pkg.add_part(props::PRES_PROPS_URI, props::pres_props());
    pkg.relate(presentation::URI, rels::PRES_PROPS, props::PRES_PROPS_URI);
    pkg.add_part(props::VIEW_PROPS_URI, props::view_props());
    pkg.relate(presentation::URI, rels::VIEW_PROPS, props::VIEW_PROPS_URI);
    pkg.add_part(props::TABLE_STYLES_URI, props::table_styles());
    pkg.relate(
        presentation::URI,
        rels::TABLE_STYLES,
        props::TABLE_STYLES_URI,
    );

    let notes_master_rid = if any_notes {
        pkg.add_part(theme::NOTES_URI, theme::build(&config.theme));
        pkg.add_part(notes::MASTER_URI, notes::build_master());
        pkg.relate(notes::MASTER_URI, rels::THEME, theme::NOTES_URI);
        Some(pkg.relate(presentation::URI, rels::NOTES_MASTER, notes::MASTER_URI))
    } else {
        None
    };

    pkg.add_part(
        presentation::URI,
        presentation::build(
            config.presentation.slide_width,
            config.presentation.slide_height,
            &master_rid,
            notes_master_rid.as_deref(),
            &slide_refs,
        ),
    );
    pkg.add_part(core::URI, core::build(&config.presentation.name));
    pkg.add_part(props::APP_URI, props::app_props(slides.len()));

    // Package root rels.
    pkg.relate("", rels::OFFICE_DOCUMENT, presentation::URI);
    pkg.relate("", rels::CORE_PROPERTIES, core::URI);
    pkg.relate("", rels::EXTENDED_PROPERTIES, props::APP_URI);

    Ok(pkg)
}
