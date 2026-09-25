//! The methodology documents, rendered from Markdown and served as HTML.
//!
//! The two documents under `methodology/` are the tool's own account of what it
//! measures and how. They are prose, not data: they never change between
//! deploys, they have no rows, and nothing in the database refers to them.
//!
//! So they are **compiled into the binary** with `include_str!` rather than read
//! from disk. Reading them at runtime would mean a third path env var next to
//! `STATIC_DIR` and `SEED_DIR`, and the `.replit` comments record what a wrong
//! one costs — `SEED_DIR` is described there as a crash loop. Embedding has no
//! runtime failure mode at all: if a document is missing or renamed, the build
//! fails at the `include_str!` with the path in the error, which is the cheapest
//! possible place to find out. The cost is that editing prose needs a rebuild,
//! and every deploy already rebuilds.
//!
//! Markdown is rendered **once at startup** into `DOCS` and handed out as clones
//! thereafter, so a request is a string copy rather than a parse. Both documents
//! together are well under a megabyte, and the `CompressionLayer` in main.rs
//! gzips the response on the way out.
//!
//! Heading anchors are injected here rather than by comrak. comrak's own
//! `header_ids` emits an empty `<a>` *inside* each heading, which would have to
//! be parsed back out to build a table of contents; deriving the ids from the
//! Markdown source instead means the TOC and the anchors come from one function
//! and cannot drift apart.

use axum::{Json, extract::Path, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use std::sync::LazyLock;

/// A document as authored. `title` and `subtitle` live here rather than in the
/// Markdown because they label the *chooser card*, where "BLACKOUT —
/// Methodology" (the value document's own H1) would say nothing to someone
/// picking between two of them.
struct Source {
    slug: &'static str,
    title: &'static str,
    subtitle: &'static str,
    /// Short all-caps tag on the card: which audience the document is for.
    kind: &'static str,
    markdown: &'static str,
}

/// The registry. Adding a document is a line here plus the file; the route,
/// the chooser page and the table of contents all follow from it.
const SOURCES: &[Source] = &[
    Source {
        slug: "value",
        title: "Policy Methodology",
        subtitle: "What BLACKOUT measures, where every number comes from, and what it can \
                   and cannot be used to claim.",
        kind: "POLICY",
        markdown: include_str!("../../../methodology/METHODOLOGY-value.md"),
    },
    Source {
        slug: "technical",
        title: "Technical Methodology",
        subtitle: "How the system acquires, stores, computes and serves the data behind \
                   the globe.",
        kind: "TECHNICAL",
        markdown: include_str!("../../../methodology/METHODOLOGY-technical.md"),
    },
];

/// One entry in a document's table of contents. `id` matches the `id`
/// attribute injected into the corresponding heading in `html`, so the
/// frontend can link to it without generating slugs of its own.
#[derive(Serialize, Clone)]
pub struct TocEntry {
    level: u8,
    id: String,
    text: String,
}

/// A rendered document.
#[derive(Serialize, Clone)]
pub struct Doc {
    slug: &'static str,
    title: &'static str,
    subtitle: &'static str,
    kind: &'static str,
    html: String,
    toc: Vec<TocEntry>,
    word_count: usize,
}

/// What the chooser page needs: everything except the body, which is the
/// entire weight of the response.
#[derive(Serialize)]
pub struct DocSummary {
    slug: &'static str,
    title: &'static str,
    subtitle: &'static str,
    kind: &'static str,
    word_count: usize,
    /// Top-level (`##`) section titles, in document order. The chooser lists
    /// these so a reader can see what a document covers before opening it.
    /// Its length is the section count, so there is no separate counter that
    /// could come to disagree with it.
    sections: Vec<String>,
}

/// Rendered once, on first access, and shared from then on.
static DOCS: LazyLock<Vec<Doc>> = LazyLock::new(|| SOURCES.iter().map(render).collect());

/// `GET /api/methodology` — the chooser index, in registry order.
pub async fn list_methodology() -> impl IntoResponse {
    let summaries: Vec<DocSummary> = DOCS
        .iter()
        .map(|d| DocSummary {
            slug: d.slug,
            title: d.title,
            subtitle: d.subtitle,
            kind: d.kind,
            word_count: d.word_count,
            sections: d
                .toc
                .iter()
                .filter(|t| t.level == 2)
                .map(|t| t.text.clone())
                .collect(),
        })
        .collect();
    Json(summaries)
}

/// `GET /api/methodology/:slug` — one rendered document.
///
/// 404 on an unknown slug is a real not-found, unlike `/api/countries/:code`:
/// the set of documents is fixed and known at compile time, so a miss means a
/// bad URL rather than an absent row, and the frontend surfaces it as an error.
pub async fn get_methodology(Path(slug): Path<String>) -> Result<Json<Doc>, StatusCode> {
    DOCS.iter()
        .find(|d| d.slug == slug)
        .map(|d| Json(d.clone()))
        .ok_or(StatusCode::NOT_FOUND)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render(source: &Source) -> Doc {
    // The leading H1 is dropped: the page renders `title` in its own header,
    // above the back link, and a second title at the top of the prose would
    // read as a duplicate.
    let (body, headings) = scan(source.markdown);

    let mut options = comrak::Options::default();
    // GFM. The value document uses tables heavily (the source register, the
    // index inputs); footnotes and strikethrough cost nothing to allow and are
    // the other two things a prose author reaches for.
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.footnotes = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    // Raw HTML in the source is escaped rather than passed through. These
    // documents are first-party and compiled in, so this is not a security
    // boundary — it is a guarantee that a stray `<` in prose renders as a `<`
    // instead of silently opening a tag and swallowing the paragraph.
    options.render.r#unsafe = false;

    let html = inject_ids(&comrak::markdown_to_html(&body, &options), &headings);

    // Word count over the source rather than the rendered HTML, so table
    // pipes, list markers and tag names are not counted as words.
    let word_count = body.split_whitespace().count();

    Doc {
        slug: source.slug,
        title: source.title,
        subtitle: source.subtitle,
        kind: source.kind,
        html,
        toc: headings,
        word_count,
    }
}

/// Walk the Markdown once: strip a leading H1, and collect `##`/`###` headings
/// in document order with unique slugified ids.
///
/// Returns the body to render and the table of contents.
fn scan(markdown: &str) -> (String, Vec<TocEntry>) {
    let mut body = String::with_capacity(markdown.len());
    let mut toc: Vec<TocEntry> = Vec::new();
    // A `#` at the start of a line inside a fenced block is a comment in some
    // shell snippet, not a heading. Tracking the fence keeps it out of the TOC
    // — and, more importantly, keeps the id count aligned with the `<h2>`/
    // `<h3>` tags comrak actually emits, which `inject_ids` depends on.
    let mut in_fence = false;
    let mut h1_dropped = false;

    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            // Only the first H1, and only before any other content, so a
            // stray `#` heading later in the document is left alone.
            if !h1_dropped && trimmed.starts_with("# ") && body.trim().is_empty() {
                h1_dropped = true;
                continue;
            }
            let level = if let Some(rest) = trimmed.strip_prefix("### ") {
                Some((3u8, rest))
            } else if let Some(rest) = trimmed.strip_prefix("## ") {
                Some((2u8, rest))
            } else {
                None
            };
            if let Some((level, text)) = level {
                let text = plain_text(text);
                let id = unique_id(slugify(&text), &toc);
                toc.push(TocEntry { level, id, text });
            }
        }
        body.push_str(line);
        body.push('\n');
    }

    (body, toc)
}

/// Strip the inline emphasis and code markers a heading is likely to carry, so
/// the TOC shows `What the tool is deliberately not` rather than
/// `What the tool is deliberately *not*`. Link syntax is left alone — no
/// heading in these documents uses it, and half-handling it would be worse
/// than not handling it.
fn plain_text(text: &str) -> String {
    text.replace(['*', '`', '_'], "").trim().to_string()
}

/// GitHub-style anchor slug: lowercase, alphanumerics kept, every other run
/// collapsed to a single dash. Em dashes and section numbers ("3.1") both fall
/// out of this correctly — `3.1 Is it ours?` becomes `3-1-is-it-ours`.
fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_dash = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.extend(ch.to_lowercase());
        } else {
            pending_dash = true;
        }
    }
    out
}

/// Disambiguate a repeated heading the way GitHub does, with a numeric suffix,
/// so two sections both called "Limitations" get distinct anchors instead of
/// one link that always jumps to the first.
fn unique_id(base: String, taken: &[TocEntry]) -> String {
    // An all-punctuation heading slugifies to nothing; give it something to
    // anchor to rather than an empty `id=""`.
    let base = if base.is_empty() {
        format!("section-{}", taken.len() + 1)
    } else {
        base
    };
    if !taken.iter().any(|t| t.id == base) {
        return base;
    }
    (1..).map(|n| format!("{base}-{n}")).find(|c| !taken.iter().any(|t| &t.id == c)).unwrap()
}

/// Add `id` attributes to the rendered `<h2>`/`<h3>` tags, in order.
///
/// comrak emits headings in source order, and `scan` collected them in source
/// order from the same text, so the nth heading tag in the HTML is the nth TOC
/// entry. The tag itself is copied from the HTML rather than rebuilt from
/// `entry.level`, so the output stays well-formed even if that assumption ever
/// breaks — the id would land on the wrong heading, but no tag would be
/// renamed and no document would fail to render.
///
/// `<h2>` cannot appear in the HTML from any source but a heading: raw HTML in
/// the Markdown is escaped (`options.render.unsafe_` is false), so a literal
/// `<h2>` in prose or a code block arrives here as `&lt;h2&gt;`.
fn inject_ids(html: &str, toc: &[TocEntry]) -> String {
    let mut out = String::with_capacity(html.len() + toc.len() * 24);
    let mut rest = html;
    let mut entries = toc.iter();

    while let Some((at, tag)) = ["<h2>", "<h3>"]
        .iter()
        .filter_map(|tag| rest.find(tag).map(|at| (at, *tag)))
        .min_by_key(|(at, _)| *at)
    {
        let Some(entry) = entries.next() else { break };
        out.push_str(&rest[..at]);
        // `tag` is "<h2>" or "<h3>"; everything but the closing bracket, then
        // the id, then the bracket.
        out.push_str(&tag[..tag.len() - 1]);
        out.push_str(" id=\"");
        out.push_str(&entry.id);
        out.push_str("\">");
        rest = &rest[at + tag.len()..];
    }

    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_drops_the_leading_h1_and_collects_headings() {
        let (body, toc) = scan("# Title\n\n## First\n\ntext\n\n### Nested\n");
        assert!(!body.contains("# Title"));
        assert_eq!(toc.len(), 2);
        assert_eq!((toc[0].level, toc[0].id.as_str()), (2, "first"));
        assert_eq!((toc[1].level, toc[1].id.as_str()), (3, "nested"));
    }

    #[test]
    fn scan_ignores_hashes_inside_fenced_code() {
        let (_, toc) = scan("## Real\n\n```sh\n## not a heading\n```\n\n## Also real\n");
        assert_eq!(toc.len(), 2);
        assert_eq!(toc[1].text, "Also real");
    }

    #[test]
    fn slugs_are_github_shaped_and_unique() {
        assert_eq!(slugify("3.1 Is it ours?"), "3-1-is-it-ours");
        assert_eq!(slugify("Limitations — read before citing"), "limitations-read-before-citing");
        let taken = vec![TocEntry { level: 2, id: "limits".into(), text: "Limits".into() }];
        assert_eq!(unique_id("limits".into(), &taken), "limits-1");
    }

    #[test]
    fn ids_land_on_the_matching_headings() {
        let (body, toc) = scan("## Alpha\n\ntext\n\n### Beta\n");
        let html = inject_ids(&comrak::markdown_to_html(&body, &comrak::Options::default()), &toc);
        assert!(html.contains(r#"<h2 id="alpha">Alpha</h2>"#), "{html}");
        assert!(html.contains(r#"<h3 id="beta">Beta</h3>"#), "{html}");
    }

    /// The real documents are the input this code actually has to survive, and
    /// they are compiled in, so assert against them directly.
    #[test]
    fn both_shipped_documents_render() {
        for doc in DOCS.iter() {
            assert!(!doc.html.is_empty(), "{} rendered empty", doc.slug);
            assert!(!doc.toc.is_empty(), "{} has no headings", doc.slug);
            // Every TOC id must exist as an anchor in the body, or the
            // sidebar links go nowhere.
            for entry in &doc.toc {
                assert!(
                    doc.html.contains(&format!(" id=\"{}\">", entry.id)),
                    "{}: no anchor for {:?}",
                    doc.slug,
                    entry.id
                );
            }
        }
    }
}
