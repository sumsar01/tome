//! Compact, token-efficient formatting for MCP tool responses.

use crate::db::Db;
use crate::sources::SearchResult;

pub const DEFAULT_SEARCH_LIMIT: usize = 8;
pub const DEFAULT_MAX_CHARS: usize = 16_000;
const SNIPPET_CHARS: usize = 120;
const MAX_OUTLINE_ENTRIES: usize = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub level: usize,
    pub title: String,
    /// Byte offset of the heading line start.
    pub start: usize,
}

/// Markdown headings outside fenced code blocks.
pub fn headings(content: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut pos = 0;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end();
        let t = trimmed.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            let level = t.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&level) && t[level..].starts_with(' ') {
                let title = t[level..].trim().trim_end_matches('#').trim().to_string();
                if !title.is_empty() {
                    out.push(Heading { level, title, start: pos });
                }
            }
        }
        pos += line.len();
    }
    out
}

/// First H1 title, if any.
pub fn title(content: &str) -> Option<String> {
    headings(content).into_iter().find(|h| h.level == 1).map(|h| h.title)
}

/// Extract a section (heading through the next heading of same or higher level).
/// Exact case-insensitive title match wins over substring match.
pub fn section<'a>(content: &'a str, query: &str) -> Option<(&'a str, String)> {
    let hs = headings(content);
    let q = query.trim().trim_start_matches('#').trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let idx = hs
        .iter()
        .position(|h| h.title.to_lowercase() == q)
        .or_else(|| hs.iter().position(|h| h.title.to_lowercase().contains(&q)))?;
    let h = &hs[idx];
    let end = hs[idx + 1..]
        .iter()
        .find(|n| n.level <= h.level)
        .map(|n| n.start)
        .unwrap_or(content.len());
    Some((&content[h.start..end], h.title.clone()))
}

/// Compact heading list for navigation hints.
pub fn outline(content: &str) -> String {
    let hs = headings(content);
    let max_level = if hs.iter().filter(|h| h.level <= 2).count() >= 3 { 2 } else { 3 };
    let picked: Vec<&Heading> = hs.iter().filter(|h| h.level <= max_level).collect();
    let mut names: Vec<String> = picked
        .iter()
        .take(MAX_OUTLINE_ENTRIES)
        .map(|h| truncate_chars(&h.title, 50))
        .collect();
    if picked.len() > MAX_OUTLINE_ENTRIES {
        names.push(format!("+{} more", picked.len() - MAX_OUTLINE_ENTRIES));
    }
    names.join(" | ")
}

/// Apply section scoping and char-window truncation to fetched content.
pub fn get_view(
    alias: &str,
    content: &str,
    section_query: Option<&str>,
    max_chars: Option<usize>,
    offset: Option<usize>,
) -> String {
    let (body, scope) = match section_query.filter(|s| !s.trim().is_empty()) {
        Some(q) => match section(content, q) {
            Some((s, title)) => (s, Some(title)),
            None => {
                return format!(
                    "No section matching '{q}' in '{alias}'. Sections: {}",
                    outline(content)
                )
            }
        },
        None => (content, None),
    };

    let max = max_chars.unwrap_or(DEFAULT_MAX_CHARS);
    let offset = offset.unwrap_or(0);
    let total = body.chars().count();
    if offset == 0 && (max == 0 || total <= max) {
        return body.to_string();
    }
    if offset >= total {
        return format!("Offset {offset} is past the end of '{alias}' ({total} chars).");
    }

    let start_b = byte_at(body, offset);
    let rest = &body[start_b..];
    let window = if max == 0 { rest } else { &rest[..byte_at(rest, max)] };
    let window = if window.len() < rest.len() {
        match window.rfind('\n') {
            Some(i) if i > window.len() / 2 => &window[..=i],
            _ => window,
        }
    } else {
        window
    };
    let end = offset + window.chars().count();

    let mut out = window.to_string();
    let section_arg = scope
        .as_ref()
        .map(|t| format!(" section=\"{t}\""))
        .unwrap_or_default();
    if end < total {
        out.push_str(&format!(
            "\n\n[tome: chars {offset}-{end} of {total}. More: tome_get alias=\"{alias}\"{section_arg} offset={end}"
        ));
        if scope.is_none() {
            out.push_str(&format!(", or section=\"<heading>\". Sections: {}", outline(body)));
        }
        out.push(']');
    } else {
        out.push_str(&format!("\n\n[tome: chars {offset}-{end} of {total}, end of doc]"));
    }
    out
}

/// Render compact ranked search results, enriched with title and category.
pub fn search_results(db: &Db, query: &str, results: &[SearchResult], limit: Option<usize>) -> String {
    if results.is_empty() {
        return format!("No results for '{query}'");
    }
    let limit = limit.filter(|l| *l > 0).unwrap_or(DEFAULT_SEARCH_LIMIT);
    let shown = results.len().min(limit);
    let mut out = format!(
        "{shown} of {} for '{query}' (alias | title | category | score):\n",
        results.len()
    );
    for (i, r) in results.iter().take(limit).enumerate() {
        let doc = db.find_doc(&r.alias).ok().flatten();
        let title = doc.as_ref().and_then(|d| d.content.as_deref()).and_then(title);
        let category = doc.as_ref().and_then(|d| d.category.clone());
        let snippet = match r.snippet.strip_prefix("content match: ") {
            Some(s) => {
                let s = compact_snippet(s);
                match title.as_deref().and_then(|t| s.strip_prefix(t)) {
                    Some(rest) => rest.trim_start().to_string(),
                    None => s,
                }
            }
            None => String::new(),
        };
        out.push_str(&format!(
            "{}. {} | {} | {} | {}\n",
            i + 1,
            r.alias,
            title.as_deref().map(|t| truncate_chars(t, 80)).unwrap_or_else(|| "-".into()),
            category.as_deref().unwrap_or("-"),
            r.score
        ));
        if !snippet.is_empty() {
            out.push_str(&format!("   {snippet}\n"));
        }
    }
    if results.len() > shown {
        out.push_str(&format!("(+{} more: pass limit)\n", results.len() - shown));
    }
    out
}

/// Single-line snippet without markdown emphasis/heading markers.
pub fn compact_snippet(s: &str) -> String {
    let cleaned = s.replace("**", "");
    let joined = cleaned
        .lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .filter(|l| !l.is_empty() && !l.starts_with("```"))
        .collect::<Vec<_>>()
        .join(" ");
    let collapsed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, SNIPPET_CHARS)
}

fn truncate_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
    t.push('…');
    t
}

fn byte_at(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map(|(i, _)| i).unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{DocRecord, SOURCE_INLINE};

    const DOC: &str = "# Title\n\nintro\n\n## Setup\n\nsetup body\n\n```sh\n# not a heading\n```\n\n### Sub\n\nsub body\n\n## Usage\n\nusage body\n";

    #[test]
    fn headings_skip_code_fences() {
        let hs: Vec<_> = headings(DOC).into_iter().map(|h| (h.level, h.title)).collect();
        assert_eq!(
            hs,
            vec![
                (1, "Title".into()),
                (2, "Setup".into()),
                (3, "Sub".into()),
                (2, "Usage".into())
            ]
        );
        assert_eq!(title(DOC).as_deref(), Some("Title"));
    }

    #[test]
    fn section_includes_subsections_and_stops_at_sibling() {
        let (s, t) = section(DOC, "setup").unwrap();
        assert_eq!(t, "Setup");
        assert!(s.starts_with("## Setup"));
        assert!(s.contains("sub body"));
        assert!(!s.contains("usage body"));
        let (s, _) = section(DOC, "## usage").unwrap();
        assert!(s.ends_with("usage body\n"));
    }

    #[test]
    fn section_prefers_exact_match() {
        let doc = "## Setup extras\n\na\n\n## Setup\n\nb\n";
        let (s, _) = section(doc, "Setup").unwrap();
        assert_eq!(s, "## Setup\n\nb\n");
    }

    #[test]
    fn missing_section_lists_outline() {
        let out = get_view("d", DOC, Some("nope"), None, None);
        assert!(out.contains("No section matching 'nope'"));
        assert!(out.contains("Setup | Usage"));
    }

    #[test]
    fn small_doc_is_returned_verbatim() {
        assert_eq!(get_view("d", DOC, None, None, None), DOC);
        assert_eq!(get_view("d", DOC, None, Some(0), None), DOC);
    }

    #[test]
    fn truncation_adds_continuation_hint() {
        let doc = format!("# T\n\n## A\n\n{}\n## B\n\nend\n", "line of text\n".repeat(50));
        let out = get_view("d", &doc, None, Some(100), None);
        assert!(out.contains("[tome: chars 0-"));
        assert!(out.contains("offset="));
        assert!(out.contains("Sections: T | A | B") || out.contains("Sections: T"));
        let body = out.split("\n\n[tome:").next().unwrap();
        assert!(body.chars().count() <= 100);
        assert!(body.ends_with('\n'));

        let end: usize = out.split("offset=").nth(1).unwrap().split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
        let next = get_view("d", &doc, None, Some(100_000), Some(end));
        assert!(next.contains("end of doc"));
        assert!(next.contains("end\n"));
    }

    #[test]
    fn truncation_is_utf8_safe() {
        let doc = "æøå".repeat(100);
        let out = get_view("d", &doc, None, Some(10), Some(5));
        assert!(out.starts_with("åæøåæøåæøå\n\n[tome: chars 5-15 of 300."));
    }

    #[test]
    fn offset_past_end() {
        assert!(get_view("d", "abc", None, None, Some(10)).contains("past the end"));
    }

    #[test]
    fn compact_snippet_flattens() {
        let s = compact_snippet("# Avoiding **Terraform** races\n\n## Symptom\n\n```\nError\n");
        assert_eq!(s, "Avoiding Terraform races Symptom Error");
        assert!(compact_snippet(&"x ".repeat(200)).chars().count() <= SNIPPET_CHARS);
    }

    #[test]
    fn search_results_are_compact_and_limited() {
        let db = Db::open_in_memory().unwrap();
        for i in 0..3 {
            db.add_doc(&DocRecord {
                alias: format!("doc-{i}"),
                source: SOURCE_INLINE.into(),
                page_id: None,
                path: None,
                tags: vec!["t".into()],
                content: Some(format!("# Doc {i}\n\nbody")),
                namespace: None,
                category: Some("Notes".into()),
            })
            .unwrap();
        }
        let results: Vec<SearchResult> = (0..3)
            .map(|i| SearchResult {
                alias: format!("doc-{i}"),
                snippet: format!("content match: # Doc {i}\n\n**body**"),
                score: 10 - i,
            })
            .collect();
        let out = search_results(&db, "body", &results, Some(2));
        assert!(out.starts_with("2 of 3 for 'body'"));
        assert!(out.contains("1. doc-0 | Doc 0 | Notes | 10\n   body\n"));
        assert!(!out.contains("doc-2"));
        assert!(out.contains("(+1 more: pass limit)"));
        assert_eq!(search_results(&db, "x", &[], None), "No results for 'x'");

        let fuzzy = [SearchResult { alias: "doc-1".into(), snippet: "[inline] t".into(), score: 5 }];
        assert_eq!(
            search_results(&db, "doc", &fuzzy, None),
            "1 of 1 for 'doc' (alias | title | category | score):\n1. doc-1 | Doc 1 | Notes | 5\n"
        );
    }
}
