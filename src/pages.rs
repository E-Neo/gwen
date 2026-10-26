//! Slide page selections: `N[,N-R]` (1-based, whitespace tolerated).
//!
//! `gwen render --page` uses this parser, so a selection means the same thing
//! everywhere. Selected pages are deduplicated and returned in ascending deck
//! order.

use std::collections::BTreeSet;
use std::fmt;

/// A parsed `--page` value: single pages and inclusive ranges, in the order
/// written (selection always normalises to deck order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSelection {
    parts: Vec<Part>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    One(usize),
    Range(usize, usize),
}

impl PageSelection {
    /// Parse `1`, `1,3-5`, ` 2 , 4 ` and the like.
    pub fn parse(input: &str) -> Result<Self, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err("`--page` is empty; use slide numbers such as `1,3-5`".into());
        }
        let mut parts = Vec::new();
        for raw in trimmed.split(',') {
            let item = raw.trim();
            if item.is_empty() {
                return Err(format!(
                    "`--page` has an empty item in `{}`; use slide numbers such as `1,3-5`",
                    input.trim()
                ));
            }
            let part = match item.split_once('-') {
                Some((from, to)) => {
                    let from = parse_number(from, input)?;
                    let to = parse_number(to, input)?;
                    if from > to {
                        return Err(format!(
                            "`--page` range `{item}` is reversed; write `{to}-{from}`"
                        ));
                    }
                    Part::Range(from, to)
                }
                None => Part::One(parse_number(item, input)?),
            };
            parts.push(part);
        }
        Ok(PageSelection { parts })
    }

    /// Resolve against the deck size, 1-based, deduplicated and ascending.
    pub fn select(&self, total: usize) -> Result<Vec<usize>, String> {
        let mut out = BTreeSet::new();
        for part in &self.parts {
            match *part {
                Part::One(n) => {
                    check(n, total)?;
                    out.insert(n);
                }
                Part::Range(from, to) => {
                    check(from, total)?;
                    check(to, total)?;
                    out.extend(from..=to);
                }
            }
        }
        Ok(out.into_iter().collect())
    }

    /// Format a page list as `1, 3-5` (ascending runs collapsed).
    pub fn format(pages: &[usize]) -> String {
        let mut out = String::new();
        let mut iter = pages.iter().copied().peekable();
        while let Some(start) = iter.next() {
            let mut end = start;
            while let Some(next) = iter.peek().copied()
                && next == end + 1
            {
                end = next;
                iter.next();
            }
            if !out.is_empty() {
                out.push_str(", ");
            }
            if end == start {
                out.push_str(&start.to_string());
            } else {
                out.push_str(&format!("{start}-{end}"));
            }
        }
        out
    }
}

fn parse_number(text: &str, whole: &str) -> Result<usize, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(format!(
            "`--page` is missing a number in `{}`; use slide numbers such as `1,3-5`",
            whole.trim()
        ));
    }
    let n: usize = text.parse().map_err(|_| {
        format!("`--page` item `{text}` is not a slide number; use `1,3-5` style ranges")
    })?;
    if n == 0 {
        return Err("`--page` is 1-based; slide `0` does not exist".into());
    }
    Ok(n)
}

fn check(n: usize, total: usize) -> Result<(), String> {
    if n > total {
        return Err(format!(
            "`--page` {n} is out of range; the deck has {total} slide{}",
            if total == 1 { "" } else { "s" }
        ));
    }
    Ok(())
}

impl fmt::Display for PageSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered: Vec<String> = self
            .parts
            .iter()
            .map(|p| match *p {
                Part::One(n) => n.to_string(),
                Part::Range(a, b) => format!("{a}-{b}"),
            })
            .collect();
        f.write_str(&rendered.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_singles_and_ranges() {
        let sel = PageSelection::parse("1,3-5").unwrap();
        assert_eq!(sel.select(8).unwrap(), vec![1, 3, 4, 5]);
        assert_eq!(PageSelection::format(&[1, 3, 4, 5]), "1, 3-5");
    }

    #[test]
    fn normalises_order_and_duplicates() {
        let sel = PageSelection::parse("5,1,3-4,3").unwrap();
        assert_eq!(sel.select(5).unwrap(), vec![1, 3, 4, 5]);
    }

    #[test]
    fn trims_whitespace() {
        let sel = PageSelection::parse("  2 , 4 - 5 ").unwrap();
        assert_eq!(sel.select(5).unwrap(), vec![2, 4, 5]);
    }

    #[test]
    fn rejects_zero_empty_and_reversed() {
        assert!(PageSelection::parse("0").unwrap_err().contains("1-based"));
        assert!(PageSelection::parse("1,,2").is_err());
        assert!(PageSelection::parse("").is_err());
        let err = PageSelection::parse("5-3").unwrap_err();
        assert!(err.contains("reversed") && err.contains("3-5"), "{err}");
        assert!(PageSelection::parse("1-x").is_err());
        assert!(PageSelection::parse("-2").is_err());
        assert!(PageSelection::parse("2-").is_err());
    }

    #[test]
    fn out_of_range_names_the_deck_size() {
        let sel = PageSelection::parse("9").unwrap();
        let err = sel.select(4).unwrap_err();
        assert!(err.contains("the deck has 4 slides"), "{err}");
        let err = PageSelection::parse("1-9").unwrap().select(1).unwrap_err();
        assert!(err.contains("the deck has 1 slide"), "{err}");
    }

    #[test]
    fn full_selection_formats_like_written() {
        let sel = PageSelection::parse("1-3,5").unwrap();
        assert_eq!(sel.to_string(), "1-3, 5");
        assert_eq!(PageSelection::format(&[]), "");
    }
}
