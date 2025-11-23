use actix_files::NamedFile;
use actix_web::http::header::{ContentDisposition, DispositionParam, DispositionType};
use actix_web::mime;
use actix_web::{web, Error as ActixError, HttpRequest, Responder};
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
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// Constants and helpers preserved from previous implementation
const PAGE_WIDTH_INCH: f64 = 8.5;
const MARGIN_MM: f64 = 10.0;
const IMAGE_DPI: f64 = 150.0;

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

/// Actix handler: generates and serves the PDF as `inline`.
pub async fn process(
    template_id: web::Path<String>,
    req: HttpRequest,
) -> Result<impl Responder, ActixError> {
    let id = template_id.into_inner();
    // Safe filename: use the id + .pdf
    let filename = format!("{}.pdf", id);
    let file_path = Path::new("./pdfs").join(&filename);

    // Generate the PDF in `./pdfs/<id>.pdf`
    if let Err(e) = generate_pdf_from_template_to_path(&id, &file_path) {
        // Map to HTTP error
        return Err(actix_web::error::ErrorServiceUnavailable(format!(
            "PDF generation failed: {}",
            e
        )));
    }

    // Serve the generated PDF
    if file_path.exists() {
        let named_file = NamedFile::open_async(&file_path)
            .await?
            .set_content_type(mime::APPLICATION_PDF)
            .set_content_disposition(ContentDisposition {
                disposition: DispositionType::Inline,
                parameters: vec![DispositionParam::Filename(filename)],
            });
        Ok(named_file.into_response(&req))
    } else {
        Err(actix_web::error::ErrorNotFound("File not found"))
    }
}


/// Generates the PDF and writes it to `output_path`.
pub fn generate_pdf_from_template_to_path(
    template_id: &str,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let conn = Connection::open("templify.sqlite")?;

    let mut stmt = conn.prepare("SELECT text FROM templates WHERE id = ?1")?;
    let template_text: String = stmt.query_row([template_id], |row| row.get(0))?;

    let images_map = load_images(&conn, template_id)?;

    let mut doc = configure_document()?;
    let mut temp_files: Vec<NamedTempFile> = Vec::new();

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

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut out_file = fs::File::create(output_path)?;
    doc.render(&mut out_file)?;

    Ok(())
}

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

fn decode_placeholder(ph: &str) -> Option<String> {
    let parts: Vec<&str> = ph.split(':').collect();
    parts.last().and_then(|last| {
        BASE64
            .decode(last)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    })
}

fn load_font() -> Result<genpdf::fonts::FontFamily<genpdf::fonts::FontData>, Box<dyn Error>> {
    if let Ok(family) = genpdf::fonts::from_files("./fonts", "Arial", None) {
        return Ok(family);
    }
    genpdf::fonts::from_files("./fonts", "LiberationSans", None).map_err(Into::into)
}

fn push_styled_text_with_breaks_to_doc(doc: &mut Document, text: &str) {
    let lines: Vec<&str> = text.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        doc.push(parse_styled_paragraph(line));
        if i < lines.len() - 1 {
            doc.push(Break::new(1));
        }
    }
}

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

fn configure_document() -> Result<Document, Box<dyn Error>> {
    let font_family = load_font()?;
    let mut doc = Document::new(font_family);
    doc.set_title("Output from template");

    let font_size_pt: u8 = (11.0_f32 * 0.75_f32).round() as u8;
    doc.set_font_size(font_size_pt);

    doc.set_line_spacing(1.0f64);

    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(10);
    doc.set_page_decorator(decorator);
    Ok(doc)
}

fn handle_list_item(doc: &mut Document, item_text: &str) {
    let segments = parse_styles(item_text);
    let mut p = Paragraph::new("");
    p.push(StyledString::new("• ", Style::new()));
    push_segments_into_paragraph(&mut p, &segments);
    let mut layout = LinearLayout::vertical();
    layout.push(p);
    doc.push(layout);
}

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

        let css_max_width_px: f64 = 200.0;
        let css_max_height_px: f64 = 200.0;
        let css_to_px = IMAGE_DPI / 96.0;
        let css_max_width_target_px = css_max_width_px * css_to_px;
        let css_max_height_target_px = css_max_height_px * css_to_px;

        let img = load_from_memory(bytes)?;
        let (orig_w, orig_h) = img.dimensions();
        let orig_w_f = orig_w as f64;
        let orig_h_f = orig_h as f64;

        let scale_by_content = (content_target_px / orig_w_f).min(1.0);
        let scale_by_css_w = (css_max_width_target_px / orig_w_f).min(1.0);
        let scale_by_css_h = (css_max_height_target_px / orig_h_f).min(1.0);

        let scale = scale_by_content.min(scale_by_css_w).min(scale_by_css_h);

        let resized: DynamicImage = if scale >= 1.0 {
            img
        } else {
            let new_w = (orig_w_f * scale).max(1.0).round() as u32;
            let new_h = (orig_h_f * scale).max(1.0).round() as u32;
            img.resize(new_w, new_h, FilterType::Lanczos3)
        };

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

fn handle_placeholder_line(line: &str, doc: &mut Document) {
    let inner = &line[4..line.len() - 1];
    if let Some(decoded) = decode_placeholder(inner) {
        push_styled_text_with_breaks_to_doc(doc, &decoded);
    } else {
        doc.push(Paragraph::new("[invalid placeholder]"));
    }
}

fn handle_normal_line(line: &str, doc: &mut Document) {
    let segments = parse_styles(line);
    let mut p = Paragraph::new("");
    push_segments_into_paragraph(&mut p, &segments);
    doc.push(p);
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
            paragraph.push(&rest[start..]);
            return paragraph;
        }
    }

    if !rest.is_empty() {
        paragraph.push(rest);
    }

    paragraph
}