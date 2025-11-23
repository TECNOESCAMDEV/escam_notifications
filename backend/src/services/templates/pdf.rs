use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use genpdf::elements::{Break, Image as PdfImage, LinearLayout, Paragraph};
use genpdf::style::{Style, StyledString};
use genpdf::Document;
use image::imageops::FilterType;
use image::{load_from_memory, DynamicImage, GenericImageView};
use png::{BitDepth as PngBitDepth, ColorType as PngColorType, Encoder as PngEncoder};
use rusqlite::Connection;
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use tempfile::NamedTempFile;

const PAGE_WIDTH_INCH: f64 = 8.5;
const MARGIN_MM: f64 = 10.0;
const IMAGE_DPI: f64 = 150.0;

/// Entry point for HTTP handler: keeps previous behavior.
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

/// Push segments into a Paragraph converting each `TextSegment` into a `StyledString`.
/// Centralizes style mapping to avoid duplicated code across handlers.
fn push_segments_into_paragraph(p: &mut Paragraph, segments: &[TextSegment]) {
    for seg in segments {
        let styled = match seg.style {
            TextStyle::Regular => StyledString::new(seg.text.clone(), Style::new()),
            TextStyle::Bold => StyledString::new(seg.text.clone(), Style::new().bold()),
            TextStyle::Italic => StyledString::new(seg.text.clone(), Style::new().italic()),
            TextStyle::BoldItalic => {
                StyledString::new(seg.text.clone(), Style::new().bold().italic())
            }
        };
        p.push(styled);
    }
}

/// Parse simple markdown-like styles: \`***bolditalic***\`, \`**bold**\`, \`*italic*\`.
/// - Returns a sequence of text segments annotated with style.
fn parse_styles(line: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i: usize = 0;

    while i < chars.len() {
        // BoldItalic `***...***`
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
                // unmatched sequence -> literal
                segments.push(TextSegment {
                    text: "***".to_string(),
                    style: TextStyle::Regular,
                });
                i += 3;
            }
            continue;
        }

        // Bold `**...**`
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
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
            continue;
        }

        // Italic `*...*`
        if chars[i] == '*' {
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
            continue;
        }

        // Plain text until next '*'
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
    // Try to load Arial (if the Arial family TTFs were added to ./fonts).
    // If that fails, fall back to LiberationSans located in the same directory.
    if let Ok(family) = genpdf::fonts::from_files("./fonts", "Arial", None) {
        return Ok(family);
    }
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

/// Load images from DB for a template.
/// Returns a map id -> raw bytes.
fn load_images(
    conn: &Connection,
    template_id: &str,
) -> Result<HashMap<String, Vec<u8>>, Box<dyn Error>> {
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
    Ok(images_map)
}

/// Configure and return a genpdf Document with font and decorator set.
fn configure_document() -> Result<Document, Box<dyn Error>> {
    let font_family = load_font()?;
    let mut doc = Document::new(font_family);
    doc.set_title("Output from template");

    // Approximate the preview's `font-size: 11px`: 11px ≈ 8.25pt (1px = 0.75pt).
    // Use `f32` for the multiplication so `round()` is unambiguous, then cast to `u8`.
    let font_size_pt: u8 = (11.0_f32 * 0.75_f32).round() as u8;
    doc.set_font_size(font_size_pt);

    // `set_line_spacing` expects an `f32`.
    doc.set_line_spacing(1.0f64);

    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(10);
    doc.set_page_decorator(decorator);
    Ok(doc)
}

/// Handle a list item line starting with \- .
fn handle_list_item(doc: &mut Document, item_text: &str) {
    let segments = parse_styles(item_text);
    let mut p = Paragraph::new("");
    p.push(StyledString::new("• ", Style::new()));
    push_segments_into_paragraph(&mut p, &segments);
    let mut layout = LinearLayout::vertical();
    layout.push(p);
    doc.push(layout);
}

/// Handle an image placeholder line like \`[img:ID]\`.
/// Loads image bytes, rescales to fit the printable width of a Letter page
/// preserving aspect ratio, writes a temporary PNG and embeds it.
fn handle_image_line(
    line: &str,
    images_map: &HashMap<String, Vec<u8>>,
    temp_files: &mut Vec<NamedTempFile>,
    doc: &mut Document,
) -> Result<(), Box<dyn Error>> {
    let inner = &line[5..line.len() - 1];
    if let Some(bytes) = images_map.get(inner) {
        let margin_in = MARGIN_MM / 25.4_f64;
        let content_width_in = PAGE_WIDTH_INCH - 2.0 * margin_in;
        let content_target_px = content_width_in * IMAGE_DPI;

        // Simulate front-end CSS limits: max-width:200px; max-height:200px;
        let css_max_width_px: f64 = 200.0;
        let css_max_height_px: f64 = 200.0;
        // Convert CSS px -> image pixels at IMAGE_DPI assuming 96 CSS px per inch
        let css_to_px = IMAGE_DPI / 96.0;
        let css_max_width_target_px = css_max_width_px * css_to_px;
        let css_max_height_target_px = css_max_height_px * css_to_px;

        let img = load_from_memory(bytes)?;
        let (orig_w, orig_h) = img.dimensions();
        let orig_w_f = orig_w as f64;
        let orig_h_f = orig_h as f64;

        // Compute scales (<= 1.0) for each constraint
        let scale_by_content = (content_target_px / orig_w_f).min(1.0);
        let scale_by_css_w = (css_max_width_target_px / orig_w_f).min(1.0);
        let scale_by_css_h = (css_max_height_target_px / orig_h_f).min(1.0);

        // Final scale is the most restrictive (smallest) of the three
        let scale = scale_by_content.min(scale_by_css_w).min(scale_by_css_h);

        let resized: DynamicImage = if scale >= 1.0 {
            img
        } else {
            let new_w = (orig_w_f * scale).max(1.0).round() as u32;
            let new_h = (orig_h_f * scale).max(1.0).round() as u32;
            img.resize(new_w, new_h, FilterType::Lanczos3)
        };

        // Flatten alpha channel over white background and convert to RGB
        let rgba = resized.to_rgba8();
        let (w, h) = rgba.dimensions();
        let mut background = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
        image::imageops::overlay(&mut background, &rgba, 0, 0);
        let rgb_image = DynamicImage::ImageRgba8(background).to_rgb8();
        let raw = rgb_image.into_raw();

        let mut tmp = NamedTempFile::new()?;
        {
            let file = tmp.as_file_mut();
            let mut encoder = PngEncoder::new(file, w, h);
            encoder.set_color(PngColorType::Rgb);
            encoder.set_depth(PngBitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&raw)?;
        }

        let path: PathBuf = tmp.path().to_path_buf();
        let mut img_elem = PdfImage::from_path(path)?;
        img_elem.set_dpi(IMAGE_DPI);
        temp_files.push(tmp);
        doc.push(img_elem);
    } else {
        doc.push(Paragraph::new(format!("[imagen no encontrada: {}]", inner)));
    }
    Ok(())
}

/// Handle a placeholder line like \`[ph:...:BASE64]\`.
fn handle_placeholder_line(line: &str, doc: &mut Document) {
    let inner = &line[4..line.len() - 1];
    if let Some(decoded) = decode_placeholder(inner) {
        push_styled_text_with_breaks_to_doc(doc, &decoded);
    } else {
        doc.push(Paragraph::new("[invalid placeholder]"));
    }
}

/// Handle a normal text line (may contain inline styles).
fn handle_normal_line(line: &str, doc: &mut Document) {
    let segments = parse_styles(line);
    let mut p = Paragraph::new("");
    push_segments_into_paragraph(&mut p, &segments);
    doc.push(p);
}

/// Generates a PDF from the template \`template_id\` and saves it to \`output.pdf\`.
/// This function orchestrates reading the template, images and streaming the processed lines to the document.
pub fn generate_pdf_from_template(template_id: &str) -> Result<(), Box<dyn Error>> {
    let conn = Connection::open("templify.sqlite")?;

    // 1) Retrieve template text
    let mut stmt = conn.prepare("SELECT text FROM templates WHERE id = ?1")?;
    let template_text: String = stmt.query_row([template_id], |row| row.get(0))?;

    // 2) Load images
    let images_map = load_images(&conn, template_id)?;

    // 3) Configure document
    let mut doc = configure_document()?;

    // Keep temporary files alive until rendering finishes
    let mut temp_files: Vec<NamedTempFile> = Vec::new();

    // 4) Process lines - DO NOT trim to preserve empty lines
    for raw_line in template_text.lines() {
        let line = raw_line;
        if line.is_empty() {
            doc.push(Break::new(1));
            continue;
        }

        if line.starts_with("- ") {
            handle_list_item(&mut doc, &line[2..]);
            continue;
        }

        if line.starts_with("[img:") && line.ends_with(']') {
            handle_image_line(line, &images_map, &mut temp_files, &mut doc)?;
            continue;
        }

        if line.starts_with("[ph:") && line.ends_with(']') {
            handle_placeholder_line(line, &mut doc);
            continue;
        }

        handle_normal_line(line, &mut doc);
    }

    // 5) Render to file
    let mut out_file = std::fs::File::create("output.pdf")?;
    doc.render(&mut out_file)?;

    // temp_files dropped and cleaned up here
    Ok(())
}

/// Find next HTML-like tag \`<b>\` or \`<i>\` in text, returning tag name and index.
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

/// Parse a single logical line containing \`<b>...</b>\` and/or \`<i>...</i>\` tags into a Paragraph.
/// This preserves remaining plain text and gracefully handles missing closing tags.
fn parse_styled_paragraph(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::new("");
    let mut rest = text;

    while let Some((next_tag, start)) = find_next_tag(rest) {
        if start > 0 {
            paragraph.push(&rest[..start]);
        }

        let (tag_open, tag_close) = match next_tag {
            "b" => ("<b>", "</b>"),
            "i" => ("<i>", "</i>"),
            _ => unreachable!(),
        };

        if let Some(rel_end) = rest[start + tag_open.len()..].find(tag_close) {
            let styled_text = &rest[start + tag_open.len()..start + tag_open.len() + rel_end];
            let styled = match next_tag {
                "b" => StyledString::new(styled_text, Style::new().bold()),
                "i" => StyledString::new(styled_text, Style::new().italic()),
                _ => StyledString::new(styled_text, Style::new()),
            };
            paragraph.push(styled);
            rest = &rest[start + tag_open.len() + rel_end + tag_close.len()..];
        } else {
            // If no closing tag, push remainder as plain text
            paragraph.push(&rest[start..]);
            return paragraph;
        }
    }

    if !rest.is_empty() {
        paragraph.push(rest);
    }

    paragraph
}
