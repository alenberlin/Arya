//! On-device text extraction for Knowledge Base documents.
//!
//! Every branch runs locally — no network, no cloud service. PDF text is
//! recovered per page (so each chunk keeps a citable page number); Office and
//! tabular formats are flattened to text. Images and scanned PDFs are handled by
//! the OCR path in [`super::ocr`]. A single unreadable file never fails the whole
//! ingest: the caller records the error on that document and moves on.

use std::io::{Cursor, Read};

/// One unit of extracted text with its source page when known (PDF); `None` for
/// formats without page structure.
pub struct Page {
    pub page: Option<i64>,
    pub text: String,
}

/// The result of extracting a document: its pages plus how the text was
/// recovered (surfaced to the user as a badge) and the page count.
pub struct Extracted {
    pub pages: Vec<Page>,
    pub extractor: &'static str,
    pub page_count: i64,
}

impl Extracted {
    /// Total non-whitespace characters recovered — used to decide whether a PDF
    /// is text-bearing or needs the OCR fallback.
    pub fn text_len(&self) -> usize {
        self.pages
            .iter()
            .map(|p| p.text.split_whitespace().map(str::len).sum::<usize>())
            .sum()
    }
}

/// One page/None-page unit built from plain text.
fn single(text: String) -> Extracted {
    Extracted {
        pages: vec![Page { page: None, text }],
        extractor: "text",
        page_count: 1,
    }
}

fn decode_utf8(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Extensions this text path understands (lowercase, no dot). Images are handled
/// by the OCR path instead.
pub fn is_text_ext(ext: &str) -> bool {
    matches!(
        ext,
        "pdf"
            | "docx"
            | "xlsx"
            | "xlsm"
            | "xls"
            | "csv"
            | "tsv"
            | "txt"
            | "md"
            | "markdown"
            | "text"
            | "log"
            | "html"
            | "htm"
            | "json"
    )
}

/// Extract text from `bytes` according to `ext` (lowercase, no leading dot).
pub fn extract(ext: &str, bytes: &[u8]) -> Result<Extracted, String> {
    match ext {
        "pdf" => extract_pdf(bytes),
        "docx" => extract_docx(bytes),
        "xlsx" | "xlsm" | "xls" => extract_spreadsheet(bytes),
        "csv" => Ok(single(extract_delimited(bytes, b','))),
        "tsv" => Ok(single(extract_delimited(bytes, b'\t'))),
        "html" | "htm" => Ok(single(strip_html(&decode_utf8(bytes)))),
        "txt" | "md" | "markdown" | "text" | "log" | "json" => Ok(single(decode_utf8(bytes))),
        other => Err(format!("unsupported file type: .{other}")),
    }
}

/// Per-page PDF text via `pdf-extract` (pure Rust, no external binary). A page
/// with no extractable text yields an empty string — the caller can then route
/// the document to OCR.
fn extract_pdf(bytes: &[u8]) -> Result<Extracted, String> {
    let pages = pdf_extract::extract_text_from_mem_by_pages(bytes)
        .map_err(|e| format!("could not read PDF: {e}"))?;
    let page_count = pages.len() as i64;
    let pages = pages
        .into_iter()
        .enumerate()
        .map(|(i, text)| Page {
            page: Some((i + 1) as i64),
            text,
        })
        .collect();
    Ok(Extracted {
        pages,
        extractor: "text",
        page_count,
    })
}

/// DOCX text: unzip and pull the runs out of `word/document.xml`. Paragraphs
/// become newlines so chunk boundaries land on real breaks.
fn extract_docx(bytes: &[u8]) -> Result<Extracted, String> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a valid .docx: {e}"))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|e| format!("missing word/document.xml: {e}"))?
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;
    Ok(single(docx_xml_to_text(&xml)))
}

fn docx_xml_to_text(xml: &str) -> String {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if e.name().as_ref() == b"w:t" => in_text = true,
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"w:t" => in_text = false,
                b"w:p" => out.push('\n'),
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"w:br" | b"w:cr" => out.push('\n'),
                b"w:tab" => out.push('\t'),
                _ => {}
            },
            // xml10_content decodes the bytes and unescapes XML entities
            // (quick-xml 0.41 renamed the old `unescape`).
            Ok(Event::Text(t)) if in_text => {
                out.push_str(&t.xml10_content().unwrap_or_default());
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Spreadsheet text via `calamine`: every sheet's cells joined row by row, so a
/// table stays queryable as "col | col | col" lines.
fn extract_spreadsheet(bytes: &[u8]) -> Result<Extracted, String> {
    use calamine::Reader;

    let mut wb = calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec()))
        .map_err(|e| format!("could not read spreadsheet: {e}"))?;
    let mut text = String::new();
    for name in wb.sheet_names() {
        let Ok(range) = wb.worksheet_range(&name) else {
            continue;
        };
        if range.is_empty() {
            continue;
        }
        text.push_str(&format!("# {name}\n"));
        for row in range.rows() {
            let cells: Vec<String> = row.iter().map(|c| c.to_string()).collect();
            if cells.iter().any(|c| !c.trim().is_empty()) {
                text.push_str(&cells.join(" | "));
                text.push('\n');
            }
        }
        text.push('\n');
    }
    Ok(single(text))
}

/// CSV/TSV: each row flattened to "field | field | field". Headers are kept as
/// the first row so column names stay searchable.
fn extract_delimited(bytes: &[u8], delimiter: u8) -> String {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(Cursor::new(bytes));
    let mut out = String::new();
    for record in rdr.records().flatten() {
        let joined = record.iter().collect::<Vec<_>>().join(" | ");
        if !joined.trim().is_empty() {
            out.push_str(&joined);
            out.push('\n');
        }
    }
    out
}

/// Strip HTML to text: drop `<script>`/`<style>` blocks whole, remove remaining
/// tags, and decode the handful of entities that matter. Script/style bodies are
/// skipped by jumping straight to their literal closing tag, so a raw `<` inside
/// JS (`if (a < b)`) can't confuse the parser. All slicing happens at ASCII byte
/// offsets (`<`, `>`, and the ASCII closing tag), so it is UTF-8-safe.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    loop {
        let Some(lt) = rest.find('<') else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..lt]);
        let after = &rest[lt + 1..];
        let closing = after.starts_with('/');
        let name: String = after
            .trim_start_matches('/')
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>')
            .collect::<String>()
            .to_ascii_lowercase();

        // A <script>/<style> element: skip its whole body by seeking the literal
        // closing tag rather than treating inner `<`/`>` as markup.
        if !closing && (name == "script" || name == "style") {
            let close = format!("</{name}");
            if let Some(pos) = find_ci_ascii(after, &close) {
                if let Some(gt) = after[pos..].find('>') {
                    out.push(' ');
                    rest = &after[pos + gt + 1..];
                    continue;
                }
            }
            out.push(' ');
            break; // unterminated block: drop the rest
        }

        // Ordinary tag: skip to its closing '>'.
        let Some(gt) = after.find('>') else {
            break;
        };
        out.push(' ');
        rest = &after[gt + 1..];
    }
    decode_entities(&out)
}

/// Case-insensitive ASCII substring search returning the byte offset in
/// `haystack`. `needle` must be ASCII (used only for closing tags like
/// `</script`), which guarantees the offset lands on a char boundary.
fn find_ci_ascii(haystack: &str, needle: &str) -> Option<usize> {
    let (hb, nb) = (haystack.as_bytes(), needle.as_bytes());
    if nb.is_empty() || hb.len() < nb.len() {
        return None;
    }
    (0..=hb.len() - nb.len()).find(|&i| hb[i..i + nb.len()].eq_ignore_ascii_case(nb))
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_plain_text_formats() {
        let e = extract("md", b"# Title\nbody text").unwrap();
        assert_eq!(e.extractor, "text");
        assert!(e.pages[0].text.contains("body text"));
        assert!(
            extract("png", b"x").is_err(),
            "images are not a text format"
        );
    }

    #[test]
    fn csv_flattens_rows_with_headers() {
        let out = extract_delimited(b"name,age\nAda,36\nGrace,45\n", b',');
        assert!(out.contains("name | age"));
        assert!(out.contains("Ada | 36"));
        assert!(out.contains("Grace | 45"));
    }

    #[test]
    fn tsv_uses_tab_delimiter() {
        let out = extract_delimited(b"a\tb\n1\t2\n", b'\t');
        assert!(out.contains("a | b"));
        assert!(out.contains("1 | 2"));
    }

    #[test]
    fn docx_xml_keeps_run_text_and_paragraph_breaks() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t xml:space="preserve"> world</w:t></w:r></w:p>
            <w:p><w:r><w:t>Second line</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let text = docx_xml_to_text(xml);
        assert!(text.contains("Hello world"), "runs joined: {text:?}");
        assert!(text.contains("Second line"));
        assert!(text.contains('\n'), "paragraphs become newlines");
    }

    #[test]
    fn strip_html_removes_tags_scripts_and_decodes_entities() {
        let html = "<html><head><style>.a{color:red}</style></head>\
            <body><script>var x = 1 < 2;</script>\
            <p>Tom &amp; Jerry &lt;3</p></body></html>";
        let text = strip_html(html);
        assert!(text.contains("Tom & Jerry <3"), "got: {text:?}");
        assert!(!text.contains("color:red"), "style dropped");
        assert!(!text.contains("var x"), "script dropped");
    }

    #[test]
    fn strip_html_is_utf8_safe_with_multibyte_before_tags() {
        // A multibyte char before a script block must not misalign slicing.
        let html = "café <script>junk</script> déjà <b>vu</b>";
        let text = strip_html(html);
        assert!(text.contains("café"));
        assert!(text.contains("déjà"));
        assert!(text.contains("vu"));
        assert!(!text.contains("junk"));
    }

    #[test]
    fn text_len_counts_non_whitespace() {
        let e = single("  hello   world  ".to_string());
        assert_eq!(e.text_len(), 10); // "hello" + "world"
    }
}
