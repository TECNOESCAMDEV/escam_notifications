use actix_web::http::header::ContentLength;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use genpdf::elements::{Break, Image as PdfImage, LinearLayout, Paragraph};
use genpdf::style::{Style, StyledString};
use genpdf::Document;
use rusqlite::Connection;
use std::collections::HashMap;
use std::error::Error;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

pub async fn process(template_id: actix_web::web::Path<String>) -> impl actix_web::Responder {
    let template_id = template_id.into_inner();
    match generate_pdf_from_template(&template_id) {
        Ok(_) => actix_web::HttpResponse::Ok().body("PDF generated successfully"),
        Err(e) => actix_web::HttpResponse::ServiceUnavailable()
            .body(format!("PDF generation failed: {}", e)),
    }
}

/// Fragments with detected styling.
enum TextStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

struct TextSegment {
    text: String,
    style: TextStyle,
}

/// Parses simple styles: \`***bolditalic***\`, \`**bold**\`, \`*italic*\`.
fn parse_styles(line: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut i: usize = 0;
    let chars: Vec<char> = line.chars().collect();
    while i < ContentLength::from(chars.len()) {
        if i + 2 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' && chars[i + 2] == '*' {
            let mut j = i + 3;
            while j + 2 < chars.len() {
                if chars[j] == '*' && chars[j + 1] == '*' && chars[j + 2] == '*' {
                    let text: String = chars[i + 3..j].iter().collect();
                    segments.push(TextSegment {
                        text,
                        style: TextStyle::BoldItalic,
                    });
                    i = j + 3;
                    break;
                }
                j += 1;
            }
            if i <= chars.len() && segments.last().map(|s| s.text.len()).unwrap_or(0) == 0 {
                segments.push(TextSegment {
                    text: "***".to_string(),
                    style: TextStyle::Regular,
                });
                i += 3;
            }
        } else if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            let mut j = i + 2;
            while j + 1 < chars.len() {
                if chars[j] == '*' && chars[j + 1] == '*' {
                    let text: String = chars[i + 2..j].iter().collect();
                    segments.push(TextSegment {
                        text,
                        style: TextStyle::Bold,
                    });
                    i = j + 2;
                    break;
                }
                j += 1;
            }
            if i <= chars.len() && segments.last().map(|s| s.text.len()).unwrap_or(0) == 0 {
                segments.push(TextSegment {
                    text: "**".to_string(),
                    style: TextStyle::Regular,
                });
                i += 2;
            }
        } else if chars[i] == '*' {
            let mut j = i + 1;
            while j < chars.len() {
                if chars[j] == '*' {
                    let text: String = chars[i + 1..j].iter().collect();
                    segments.push(TextSegment {
                        text,
                        style: TextStyle::Italic,
                    });
                    i = j + 1;
                    break;
                }
                j += 1;
            }
            if i <= chars.len() && segments.last().map(|s| s.text.len()).unwrap_or(0) == 0 {
                segments.push(TextSegment {
                    text: "*".to_string(),
                    style: TextStyle::Regular,
                });
                i += 1;
            }
        } else {
            let mut j = i;
            while j < chars.len() && chars[j] != '*' {
                j += 1;
            }
            let text: String = chars[i..j].iter().collect();
            segments.push(TextSegment {
                text,
                style: TextStyle::Regular,
            });
            i = j;
        }
    }
    segments
}

/// Decodes the last base64 segment in \`[ph:...:BASE64]\`.
fn decode_placeholder(ph: &str) -> Option<String> {
    let parts: Vec<&str> = ph.split(':').collect();
    parts.last().and_then(|last| {
        BASE64
            .decode(last)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    })
}

/// Load the font family (adjust path/name if needed).
fn load_font() -> Result<genpdf::fonts::FontFamily<genpdf::fonts::FontData>, Box<dyn Error>> {
    genpdf::fonts::from_files("./fonts", "LiberationSans", None).map_err(Into::into)
}

/// Push text that may contain internal newlines into \`doc\`, preserving breaks.
fn push_styled_text_with_breaks_to_doc(doc: &mut Document, text: &str) {
    let lines: Vec<&str> = text.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        doc.push(parse_styled_paragraph(line));
        if i < lines.len() - 1 {
            doc.push(Break::new(1));
        }
    }
}

/// Generates a PDF from the template \`template_id\` and saves it to \`output.pdf\`.
pub fn generate_pdf_from_template(template_id: &str) -> Result<(), Box<dyn Error>> {
    let conn = Connection::open("templify.sqlite")?;
    // 1) Retrieve template text
    let mut stmt = conn.prepare("SELECT text FROM templates WHERE id = ?1")?;
    let template_text: String = stmt.query_row([template_id], |row| row.get(0))?;

    // 2) Load images (base64 -> bytes)
    let mut images_stmt = conn.prepare("SELECT id, base64 FROM images WHERE template_id = ?1")?;
    let mut rows = images_stmt.query([template_id])?;
    let mut images_map: HashMap<String, Vec<u8>> = HashMap::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let b64: String = row.get(1)?;
        if let Ok(bytes) = BASE64.decode(b64) {
            images_map.insert(id, bytes);
        }
    }

    // 3) Configure document
    let font_family = load_font()?;
    let mut doc = Document::new(font_family);
    doc.set_title("Salida desde plantilla");
    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(10);
    doc.set_page_decorator(decorator);

    // Keep temporary files alive until rendering finishes
    let mut temp_files: Vec<NamedTempFile> = Vec::new();

    // 4) Process lines \- use \`lines()\` but DO NOT trim so empty lines are preserved
    for raw_line in template_text.lines() {
        // preserve blank lines: represent as a Break
        let line = raw_line; // do not call \`trim()\`
        if line.is_empty() {
            doc.push(Break::new(1));
            continue;
        }

        // separator `--`
        if line == "--" {
            doc.push(Paragraph::new("------------------------------"));
            continue;
        }

        // List item
        if line.starts_with("- ") {
            let mut layout = LinearLayout::vertical();
            let item_text = &line[2..];
            let segments = parse_styles(item_text);
            let mut p = Paragraph::new("");
            // list bullet prefix
            p.push(StyledString::new("• ", Style::new()));
            for seg in segments {
                let styled = match seg.style {
                    TextStyle::Regular => StyledString::new(seg.text, Style::new()),
                    TextStyle::Bold => StyledString::new(seg.text, Style::new().bold()),
                    TextStyle::Italic => StyledString::new(seg.text, Style::new().italic()),
                    TextStyle::BoldItalic => {
                        StyledString::new(seg.text, Style::new().bold().italic())
                    }
                };
                p.push(styled);
            }
            layout.push(p);
            doc.push(layout);
            continue;
        }

        // Image: `[img:ID]`
        if line.starts_with("[img:") && line.ends_with(']') {
            let inner = &line[5..line.len() - 1];
            if let Some(bytes) = images_map.get(inner) {
                let mut tmp = NamedTempFile::new()?;
                tmp.write_all(bytes)?;
                // keep tmp so it is not removed before rendering
                let path: PathBuf = tmp.path().to_path_buf();
                // create image element from path
                let mut img_elem = PdfImage::from_path(path)?;
                img_elem.set_dpi(150.0);
                // push tmp into vector to keep it alive
                temp_files.push(tmp);
                doc.push(img_elem);
            } else {
                doc.push(Paragraph::new(format!("[imagen no encontrada: {}]", inner)));
            }
            continue;
        }

        // Placeholder: `[ph:...:BASE64]`
        if line.starts_with("[ph:") && line.ends_with(']') {
            let inner = &line[4..line.len() - 1];
            if let Some(decoded) = decode_placeholder(inner) {
                // decoded may contain multiple lines: preserve them
                push_styled_text_with_breaks_to_doc(&mut doc, &decoded);
            } else {
                doc.push(Paragraph::new("[invalid placeholder]"));
            }
            continue;
        }

        // Normal line (single logical line): may still contain no internal \n
        let segments = parse_styles(line);
        let mut p = Paragraph::new("");
        for seg in segments {
            let styled = match seg.style {
                TextStyle::Regular => StyledString::new(seg.text, Style::new()),
                TextStyle::Bold => StyledString::new(seg.text, Style::new().bold()),
                TextStyle::Italic => StyledString::new(seg.text, Style::new().italic()),
                TextStyle::BoldItalic => StyledString::new(seg.text, Style::new().bold().italic()),
            };
            p.push(styled);
        }
        doc.push(p);
    }

    // 5) Render to file
    let mut out_file = std::fs::File::create("output.pdf")?;
    doc.render(&mut out_file)?;

    // temp_files removed on function exit (NamedTempFile cleanup)
    Ok(())
}

fn find_next_tag(text: &str) -> Option<(&str, usize)> {
    let b_pos = text.find("<b>");
    let i_pos = text.find("<i>");
    match (b_pos, i_pos) {
        (Some(b), Some(i)) if b < i => Some(("b", b)),
        (Some(_), Some(i)) => Some(("i", i)),
        (Some(b), None) => Some(("b", b)),
        (None, Some(i)) => Some(("i", i)),
        (None, None) => None,
    }
}

fn parse_styled_paragraph(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::new("");
    let mut rest = text;

    while let Some((next_tag, start)) = find_next_tag(rest) {
        // push plain text before the tag
        if start > 0 {
            paragraph.push(&rest[..start]);
        }

        let (tag_open, tag_close) = match next_tag {
            "b" => ("<b>", "</b>"),
            "i" => ("<i>", "</i>"),
            _ => unreachable!(),
        };

        // find closing tag after the opening tag
        if let Some(rel_end) = rest[start + tag_open.len()..].find(tag_close) {
            let styled_text = &rest[start + tag_open.len()..start + tag_open.len() + rel_end];
            let styled = match next_tag {
                "b" => StyledString::new(styled_text, Style::new().bold()),
                "i" => StyledString::new(styled_text, Style::new().italic()),
                _ => StyledString::new(styled_text, Style::new()),
            };
            paragraph.push(styled);
            // advance rest past the closing tag
            rest = &rest[start + tag_open.len() + rel_end + tag_close.len()..];
        } else {
            // no closing tag found: push the rest as plain text and finish
            paragraph.push(&rest[start..]);
            return paragraph;
        }
    }

    // push any remaining plain text
    if !rest.is_empty() {
        paragraph.push(rest);
    }

    paragraph
}
