/// Extract an attribute value from a tag attribute string.
pub fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    let pattern = format!("{}=", name);
    let pos = attrs.to_ascii_lowercase().find(&pattern)?;
    let after = &attrs[pos + pattern.len()..];
    if let Some(stripped) = after.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else if let Some(stripped) = after.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(stripped[..end].to_string())
    } else {
        let end = after
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(after.len());
        Some(after[..end].to_string())
    }
}

/// Decode common HTML entities.
pub fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&ndash;", "–")
        .replace("&mdash;", "—")
        .replace("&hellip;", "…")
}

/// Convert HTML to readable markdown.
///
/// Handles: headings, paragraphs, lists, code blocks, inline code, links,
/// bold/italic, and strips nav/script/style elements.
pub fn html_to_markdown(html: &str) -> String {
    let mut out = String::new();
    let mut chars = html.chars().peekable();
    let mut in_skip = false; // inside <script>, <style>, <nav>, <header>, <footer>
    let mut in_pre = false; // inside <pre>
    let mut list_depth: usize = 0;
    let mut ordered_counters: Vec<usize> = Vec::new();
    let mut pending_nl = 0usize; // deferred newlines (avoids leading blanks)

    let flush_nl = |out: &mut String, n: usize| {
        for _ in 0..n {
            out.push('\n');
        }
    };

    while let Some(ch) = chars.next() {
        if ch != '<' {
            if !in_skip {
                if pending_nl > 0 {
                    flush_nl(&mut out, pending_nl);
                    pending_nl = 0;
                }
                if in_pre {
                    out.push(ch);
                } else {
                    // Collapse whitespace in normal text
                    if ch == '\n' || ch == '\r' {
                        if !out.ends_with(' ') && !out.ends_with('\n') {
                            out.push(' ');
                        }
                    } else {
                        out.push(ch);
                    }
                }
            }
            continue;
        }

        // Inside a tag — collect tag name and attributes
        let mut tag = String::new();
        let mut closing = false;
        if chars.peek() == Some(&'/') {
            closing = true;
            chars.next();
        }
        // Read tag name
        while let Some(&c) = chars.peek() {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ':' {
                tag.push(c.to_ascii_lowercase());
                chars.next();
            } else {
                break;
            }
        }
        // Drain rest of tag to '>'
        let mut attrs = String::new();
        let mut depth = 0i32;
        for c in chars.by_ref() {
            if c == '<' {
                depth += 1;
            }
            if c == '>' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            attrs.push(c);
        }

        let self_closing = attrs.trim_end().ends_with('/');

        // Skip tags
        let skip_tags = [
            "script", "style", "nav", "header", "footer", "aside", "form", "button", "input",
            "svg", "iframe", "noscript",
        ];
        if skip_tags.contains(&tag.as_str()) {
            if !closing && !self_closing {
                in_skip = true;
            }
            if closing {
                in_skip = false;
            }
            continue;
        }

        if in_skip {
            continue;
        }

        match (tag.as_str(), closing) {
            ("h1", false) => {
                pending_nl = pending_nl.max(2);
                if pending_nl > 0 && !out.is_empty() {
                    flush_nl(&mut out, pending_nl);
                    pending_nl = 0;
                }
                out.push_str("# ");
            }
            ("h2", false) => {
                pending_nl = pending_nl.max(2);
                if !out.is_empty() {
                    flush_nl(&mut out, pending_nl);
                    pending_nl = 0;
                }
                out.push_str("## ");
            }
            ("h3", false) => {
                pending_nl = pending_nl.max(2);
                if !out.is_empty() {
                    flush_nl(&mut out, pending_nl);
                    pending_nl = 0;
                }
                out.push_str("### ");
            }
            ("h4", false) => {
                pending_nl = pending_nl.max(2);
                if !out.is_empty() {
                    flush_nl(&mut out, pending_nl);
                    pending_nl = 0;
                }
                out.push_str("#### ");
            }
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => {
                pending_nl = 2;
            }
            ("p", false) => {
                if !out.is_empty() {
                    pending_nl = pending_nl.max(2);
                }
            }
            ("p", true) => {
                pending_nl = pending_nl.max(2);
            }
            ("br", _) => {
                out.push('\n');
            }
            ("hr", _) => {
                out.push_str("\n\n---\n\n");
            }
            ("pre", false) => {
                in_pre = true;
                pending_nl = pending_nl.max(2);
                if !out.is_empty() {
                    flush_nl(&mut out, pending_nl);
                    pending_nl = 0;
                }
                out.push_str("```\n");
            }
            ("pre", true) => {
                in_pre = false;
                out.push_str("\n```");
                pending_nl = 2;
            }
            ("code", false) if !in_pre => {
                out.push('`');
            }
            ("code", true) if !in_pre => {
                out.push('`');
            }
            ("strong" | "b", false) => {
                out.push_str("**");
            }
            ("strong" | "b", true) => {
                out.push_str("**");
            }
            ("em" | "i", false) => {
                out.push('_');
            }
            ("em" | "i", true) => {
                out.push('_');
            }
            ("ul", false) => {
                list_depth += 1;
                ordered_counters.push(0);
                pending_nl = pending_nl.max(1);
            }
            ("ul", true) => {
                list_depth = list_depth.saturating_sub(1);
                ordered_counters.pop();
                pending_nl = pending_nl.max(1);
            }
            ("ol", false) => {
                list_depth += 1;
                ordered_counters.push(0);
                pending_nl = pending_nl.max(1);
            }
            ("ol", true) => {
                list_depth = list_depth.saturating_sub(1);
                ordered_counters.pop();
                pending_nl = pending_nl.max(1);
            }
            ("li", false) => {
                flush_nl(&mut out, pending_nl.max(1));
                pending_nl = 0;
                let indent = "  ".repeat(list_depth.saturating_sub(1));
                if let Some(counter) = ordered_counters.last_mut() {
                    *counter += 1;
                    out.push_str(&format!("{}{}. ", indent, *counter));
                } else {
                    out.push_str(&format!("{}- ", indent));
                }
            }
            ("li", true) => {
                pending_nl = 1;
            }
            ("a", false) => {
                // Extract href from attrs
                if let Some(href) = extract_attr(&attrs, "href") {
                    out.push('[');
                    // We'll close the link on </a>; stash href using a sentinel
                    out.push_str("\x00HREF=");
                    out.push_str(&href);
                    out.push('\x00');
                }
            }
            ("a", true) => {
                // Close link bracket if we opened one
                // Look back for sentinel and restructure: [text](href)
                if let Some(start) = out.rfind("\x00HREF=") {
                    let sentinel_end = out[start..]
                        .find('\x00')
                        .map(|i| start + i + 1)
                        .unwrap_or(out.len());
                    let href: String = out[start + 6..sentinel_end - 1].to_string();
                    let text: String = out[sentinel_end..].to_string();
                    out.truncate(start);
                    if text.trim().is_empty() {
                        out.push_str(&href);
                    } else {
                        out.push('[');
                        out.push_str(text.trim());
                        out.push_str("](");
                        out.push_str(&href);
                        out.push(')');
                    }
                }
            }
            ("div" | "section" | "article" | "main", false) => {
                pending_nl = pending_nl.max(1);
            }
            ("div" | "section" | "article" | "main", true) => {
                pending_nl = pending_nl.max(1);
            }
            ("blockquote", false) => {
                pending_nl = pending_nl.max(2);
            }
            ("blockquote", true) => {
                pending_nl = pending_nl.max(2);
            }
            _ => {}
        }
    }

    // Decode HTML entities in the output
    decode_entities(out.trim())
}
