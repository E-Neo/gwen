use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq)]
pub enum PathSegment {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone)]
pub enum ResolvedPath {
    Presentation {
        remaining: Vec<PathSegment>,
    },
    Slide {
        slide_idx: Option<usize>,
        remaining: Vec<PathSegment>,
    },
    Shape {
        slide_idx: usize,
        shape_idx: Option<usize>,
        remaining: Vec<PathSegment>,
    },
    Notes {
        slide_idx: Option<usize>,
        remaining: Vec<PathSegment>,
    },
    NotesShape {
        slide_idx: usize,
        shape_idx: Option<usize>,
        remaining: Vec<PathSegment>,
    },
    Theme {
        remaining: Vec<PathSegment>,
    },
    Master {
        master_idx: Option<usize>,
        remaining: Vec<PathSegment>,
    },
}

#[cfg(test)]
pub fn parse_path(path_str: &str) -> AppResult<Vec<PathSegment>> {
    let raw = path_str.strip_prefix('p').unwrap_or(path_str);
    let raw = raw.strip_prefix('.').unwrap_or(raw);
    if raw.is_empty() {
        return Ok(Vec::new());
    }

    let mut segments = Vec::new();
    let mut rest = raw;

    while !rest.is_empty() {
        if let Some(name_end) = rest.find(['.', '[']) {
            let field = &rest[..name_end];
            if !field.is_empty() {
                segments.push(PathSegment::Field(field.to_string()));
            }
            if rest.as_bytes()[name_end] == b'.' {
                rest = &rest[name_end + 1..];
            } else if rest.as_bytes()[name_end] == b'[' {
                rest = &rest[name_end..];
            }
        } else {
            segments.push(PathSegment::Field(rest.to_string()));
            break;
        }

        if rest.starts_with('[') {
            let close = rest
                .find(']')
                .ok_or_else(|| AppError::PathParse("Unclosed bracket".to_string()))?;
            let idx_str = &rest[1..close];
            let idx: usize = idx_str
                .parse()
                .map_err(|_| AppError::PathParse(format!("Invalid index '{idx_str}'")))?;
            segments.push(PathSegment::Index(idx));
            rest = &rest[close + 1..];
            if rest.starts_with('.') {
                rest = &rest[1..];
            }
        }
    }

    Ok(segments)
}

pub fn resolve_path(segments: &[PathSegment]) -> AppResult<ResolvedPath> {
    if segments.is_empty() {
        return Ok(ResolvedPath::Presentation {
            remaining: Vec::new(),
        });
    }

    let first = &segments[0];
    match first {
        PathSegment::Field(name) if name == "slides" => {
            let tail = &segments[1..];
            if tail.is_empty() {
                return Ok(ResolvedPath::Slide {
                    slide_idx: None,
                    remaining: Vec::new(),
                });
            }
            match &tail[0] {
                PathSegment::Index(idx) => {
                    let after_idx = &tail[1..];
                    if after_idx.is_empty() {
                        return Ok(ResolvedPath::Slide {
                            slide_idx: Some(*idx),
                            remaining: Vec::new(),
                        });
                    }
                    match &after_idx[0] {
                        PathSegment::Field(name) if name == "shapes" => {
                            let tail2 = &after_idx[1..];
                            if tail2.is_empty() {
                                return Ok(ResolvedPath::Shape {
                                    slide_idx: *idx,
                                    shape_idx: None,
                                    remaining: Vec::new(),
                                });
                            }
                            match &tail2[0] {
                                PathSegment::Index(shape_idx) => Ok(ResolvedPath::Shape {
                                    slide_idx: *idx,
                                    shape_idx: Some(*shape_idx),
                                    remaining: tail2[1..].to_vec(),
                                }),
                                _ => Err(AppError::PathParse(
                                    "Expected shape index after shapes".to_string(),
                                )),
                            }
                        }
                        PathSegment::Field(name) if name == "notes" => {
                            let tail2 = &after_idx[1..];
                            if tail2.is_empty() {
                                return Ok(ResolvedPath::Notes {
                                    slide_idx: Some(*idx),
                                    remaining: Vec::new(),
                                });
                            }
                            match &tail2[0] {
                                PathSegment::Field(n) if n == "shapes" => {
                                    let tail3 = &tail2[1..];
                                    if tail3.is_empty() {
                                        return Ok(ResolvedPath::NotesShape {
                                            slide_idx: *idx,
                                            shape_idx: None,
                                            remaining: Vec::new(),
                                        });
                                    }
                                    match &tail3[0] {
                                        PathSegment::Index(shape_idx) => {
                                            Ok(ResolvedPath::NotesShape {
                                                slide_idx: *idx,
                                                shape_idx: Some(*shape_idx),
                                                remaining: tail3[1..].to_vec(),
                                            })
                                        }
                                        _ => Err(AppError::PathParse(
                                            "Expected shape index after notes.shapes".to_string(),
                                        )),
                                    }
                                }
                                _ => Ok(ResolvedPath::Notes {
                                    slide_idx: Some(*idx),
                                    remaining: tail2.to_vec(),
                                }),
                            }
                        }
                        PathSegment::Field(_) => Ok(ResolvedPath::Slide {
                            slide_idx: Some(*idx),
                            remaining: after_idx.to_vec(),
                        }),
                        _ => Err(AppError::PathParse(
                            "Expected field after slide index".to_string(),
                        )),
                    }
                }
                PathSegment::Field(name) if name == "shapes" => Ok(ResolvedPath::Shape {
                    slide_idx: 0,
                    shape_idx: None,
                    remaining: Vec::new(),
                }),
                PathSegment::Field(name) if name == "notes" => {
                    let tail2 = &tail[1..];
                    if tail2.is_empty() {
                        return Ok(ResolvedPath::Notes {
                            slide_idx: None,
                            remaining: Vec::new(),
                        });
                    }
                    match &tail2[0] {
                        PathSegment::Field(n) if n == "shapes" => {
                            let tail3 = &tail2[1..];
                            if tail3.is_empty() {
                                return Ok(ResolvedPath::NotesShape {
                                    slide_idx: 0,
                                    shape_idx: None,
                                    remaining: Vec::new(),
                                });
                            }
                            match &tail3[0] {
                                PathSegment::Index(shape_idx) => Ok(ResolvedPath::NotesShape {
                                    slide_idx: 0,
                                    shape_idx: Some(*shape_idx),
                                    remaining: tail3[1..].to_vec(),
                                }),
                                _ => Err(AppError::PathParse(
                                    "Expected shape index after notes.shapes".to_string(),
                                )),
                            }
                        }
                        _ => Ok(ResolvedPath::Notes {
                            slide_idx: None,
                            remaining: tail2.to_vec(),
                        }),
                    }
                }
                PathSegment::Field(_) => Ok(ResolvedPath::Slide {
                    slide_idx: None,
                    remaining: tail.to_vec(),
                }),
            }
        }
        PathSegment::Field(name) if name == "theme" => Ok(ResolvedPath::Theme {
            remaining: segments[1..].to_vec(),
        }),
        PathSegment::Field(name) if name == "slide_masters" => {
            let tail = &segments[1..];
            if tail.is_empty() {
                return Ok(ResolvedPath::Master {
                    master_idx: None,
                    remaining: Vec::new(),
                });
            }
            match &tail[0] {
                PathSegment::Index(idx) => Ok(ResolvedPath::Master {
                    master_idx: Some(*idx),
                    remaining: tail[1..].to_vec(),
                }),
                _ => Err(AppError::PathParse(
                    "Expected index after slide_masters".to_string(),
                )),
            }
        }
        PathSegment::Field(_) => Ok(ResolvedPath::Presentation {
            remaining: segments.to_vec(),
        }),
        _ => Err(AppError::PathParse(format!(
            "Unexpected path segment {:?}",
            first
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(path_str: &str) -> ResolvedPath {
        resolve_path(&parse_path(path_str).unwrap()).unwrap()
    }

    #[test]
    fn parses_slide_shapes() {
        match resolve("slides[2].shapes[3]") {
            ResolvedPath::Shape {
                slide_idx: 2,
                shape_idx: Some(3),
                ..
            } => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_notes_slide() {
        match resolve("slides[1].notes") {
            ResolvedPath::Notes {
                slide_idx: Some(1),
                remaining,
            } => assert!(remaining.is_empty()),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_notes_shape() {
        match resolve("slides[1].notes.shapes[2].text_frame.paragraphs") {
            ResolvedPath::NotesShape {
                slide_idx: 1,
                shape_idx: Some(2),
                remaining,
            } => {
                assert!(matches!(&remaining[0], PathSegment::Field(n) if n == "text_frame"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn notes_requires_slide_index() {
        assert!(resolve_path(&parse_path("slides.notes").unwrap()).is_ok());
        assert!(resolve_path(&parse_path("slides[0].notes").unwrap()).is_ok());
    }

    #[test]
    fn theme_path_resolves() {
        match resolve_path(&parse_path("p.theme.colors.accent1").unwrap()).unwrap() {
            ResolvedPath::Theme { remaining } => {
                assert!(matches!(&remaining[0], PathSegment::Field(n) if n == "colors"));
                assert!(matches!(&remaining[1], PathSegment::Field(n) if n == "accent1"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn master_path_resolves() {
        match resolve_path(&parse_path("p.slide_masters[0].shapes[1].left").unwrap()).unwrap() {
            ResolvedPath::Master {
                master_idx: Some(0),
                remaining,
            } => {
                assert!(matches!(&remaining[0], PathSegment::Field(n) if n == "shapes"));
                assert!(matches!(&remaining[1], PathSegment::Index(1)));
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(matches!(
            resolve_path(&parse_path("p.slide_masters").unwrap()).unwrap(),
            ResolvedPath::Master {
                master_idx: None,
                ..
            }
        ));
    }

    #[test]
    fn master_layout_path_resolves() {
        match resolve_path(&parse_path("p.slide_masters[0].slide_layouts[2].shapes").unwrap())
            .unwrap()
        {
            ResolvedPath::Master {
                master_idx: Some(0),
                remaining,
            } => {
                assert!(matches!(&remaining[0], PathSegment::Field(n) if n == "slide_layouts"));
                assert!(matches!(&remaining[1], PathSegment::Index(2)));
                assert!(matches!(&remaining[2], PathSegment::Field(n) if n == "shapes"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}
