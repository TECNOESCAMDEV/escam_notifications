//! # PDF Generation Service
//!
//! This module is responsible for generating and serving PDF documents based on templates
//! stored in the database. It exposes an Actix web endpoint that takes a template ID,
//! fetches the corresponding template content, renders it into a PDF using the `genpdf`
//! crate, and serves the file to the client.
//!
//! ## Core Features:
//! - **Template Parsing**: It processes template text line by line, interpreting different formatting cues.
//! - **Styled Text**: Supports Markdown-like syntax for bold (`**text**`), italic (`*text*`),
//!   and bold-italic (`***text***`) styling.
//! - **Image Handling**: Embeds images referenced in the template (e.g., `[img:image_id]`).
//!   It performs resizing to fit page constraints and converts images to a PDF-compatible format.
//! - **Placeholder Substitution**: Decodes and inserts Base64-encoded content from placeholders
//!   (e.g., `[ph:BASE64_DATA]`), which may themselves contain simple `<b>` and `<i>` tags for styling.
//! - **List Formatting**: Renders lines starting with `- ` as bulleted list items.
//!
//! ## Workflow:
//! 1.  A `GET` request is made to `/api/templates/pdf/{template_id}`.
//! 2.  The `process` handler is invoked.
//! 3.  `generate_pdf_from_template_to_path` is called, which orchestrates the PDF creation.
//! 4.  It connects to the database to fetch the template's text and associated images (as Base64).
//! 5.  The template text is parsed. Each line is processed based on its format (image, placeholder, list, or plain text).
//! 6.  Images are decoded, resized, converted to RGB PNG, and saved to temporary files.
//! 7.  The `genpdf` `Document` is assembled with all elements (paragraphs, images, breaks).
//! 8.  The document is rendered and saved to a file in the `./pdfs` directory.
//! 9.  The `process` handler serves the generated file with a `Content-Disposition: inline` header,
//!     allowing browsers to display it directly.

use actix_files::NamedFile;
use actix_web::http::header::{ContentDisposition, DispositionParam, DispositionType};
use actix_web::mime;
use actix_web::{web, Error as ActixError, HttpRequest, Responder};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use genpdf::elements::{Break, Image as PdfImage, Paragraph};
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

// --- Constants ---

/// The width of the PDF page in inches (standard US Letter size).
const PAGE_WIDTH_INCH: f64 = 8.5;
/// The margin for the PDF page in millimeters.
const MARGIN_MM: f64 = 10.0;
/// The DPI (dots per inch) used for scaling images within the PDF to ensure print quality.
const IMAGE_DPI: f64 = 150.0;

/// Represents the text style for a segment of text within a paragraph.
enum TextStyle {
    /// Standard, unstyled text.
    Regular,
    /// Bold text.
    Bold,
    /// Italic text.
    Italic,
    /// Bold and italic text.
    BoldItalic,
}

/// Represents a segment of text with a specific style.
/// This is used to construct paragraphs with mixed styling (e.g., "This is **bold** text.").
pub struct TextSegment {
    text: String,
    style: TextStyle,
}

/// Actix web handler for `GET /api/templates/pdf/{template_id}`.
///
/// Generates a PDF from a template and serves it for inline display in the browser.
///
/// # Arguments
/// * `template_id` - The ID of the template to use, extracted from the URL path.
/// * `req` - The incoming `HttpRequest`, used to build the response.
///
/// # Returns
/// A `Result` containing an `impl Responder` (the PDF file response) on success,
/// or an `ActixError` on failure (e.g., PDF generation error or file not found).
pub async fn process(
    template_id: web::Path<String>,
    req: HttpRequest,
) -> Result<impl Responder, ActixError> {
    let id = template_id.into_inner();
    let filename = format!("{}.pdf", id);
    let file_path = Path::new("./pdfs").join(&filename);

    // Generate the PDF file and save it to the designated path.
    if let Err(e) = generate_pdf_from_template_to_path(&id, &file_path) {
        return Err(actix_web::error::ErrorServiceUnavailable(format!(
            "PDF generation failed: {}",
            e
        )));
    }

    // Serve the generated PDF file.
    if file_path.exists() {
        let named_file = NamedFile::open_async(&file_path)
            .await?
            .set_content_type(mime::APPLICATION_PDF)
            .set_content_disposition(ContentDisposition {
                disposition: DispositionType::Inline, // Suggests the browser should display the file.
                parameters: vec![DispositionParam::Filename(filename)],
            });
        Ok(named_file.into_response(&req))
    } else {
        Err(actix_web::error::ErrorNotFound("File not found"))
    }
}

/// Generates a PDF from a template and saves it to the specified output path.
///
/// This is the main orchestration function. It connects to the database, fetches template
/// content, parses it line by line, and uses `genpdf` to build and render the document.
///
/// # Arguments
/// * `template_id` - The ID of the template to retrieve from the database.
/// * `output_path` - The file system path where the generated PDF will be saved.
///
/// # Returns
/// An empty `Result` on success, or a `Box<dyn Error>` on failure.
fn generate_pdf_from_template_to_path(
    template_id: &str,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let conn = Connection::open("templify.sqlite")?;

    let mut stmt = conn.prepare("SELECT text FROM templates WHERE id = ?1")?;
    let template_text: String = stmt.query_row([template_id], |row| row.get(0))?;

    let images_map = load_images(&conn, template_id)?;

    let mut doc = configure_document()?;
    let mut temp_files: Vec<NamedTempFile> = Vec::new(); // Holds temp files for images to ensure they live long enough.

    // Process the template content line by line.
    for raw_line in template_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            doc.push(Break::new(1)); // Add vertical space for empty lines.
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

        // If no other format matches, treat it as a normal paragraph.
        handle_normal_line(line, &mut doc);
    }

    // Ensure the output directory exists.
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Render the document to the output file.
    let mut out_file = fs::File::create(output_path)?;
    doc.render(&mut out_file)?;

    Ok(())
}

/// Pushes a slice of `TextSegment`s into a `genpdf::Paragraph`.
///
/// This function iterates through styled text segments and adds them to a `genpdf`
/// paragraph, applying the correct bold/italic styling for each part.
///
/// # Arguments
/// * `p` - The `Paragraph` to which the styled text will be added.
/// * `segments` - A slice of `TextSegment`s to add.
pub(crate) fn push_segments_into_paragraph(p: &mut Paragraph, segments: &[TextSegment]) {
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

/// Parses a line of text for Markdown-like styling (`*`, `**`, `***`) and returns a vector of `TextSegment`s.
///
/// # Arguments
/// * `line` - The string slice to parse.
///
/// # Returns
/// A `Vec<TextSegment>` representing the parsed line with styles.
pub(crate) fn parse_styles(line: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i: usize = 0;

    while i < chars.len() {
        // Check for BoldItalic `***...***` first, as it's the longest token.
        if i + 5 < chars.len() && chars[i..i + 3] == ['*', '*', '*'] {
            if let Some(end_pos) = line[i + 3..].find("***") {
                let text = line[i + 3..i + 3 + end_pos].to_string();
                segments.push(TextSegment {
                    text,
                    style: TextStyle::BoldItalic,
                });
                i += 3 + end_pos + 3;
                continue;
            }
        }

        // Check for Bold `**...**`.
        if i + 3 < chars.len() && chars[i..i + 2] == ['*', '*'] {
            if let Some(end_pos) = line[i + 2..].find("**") {
                let text = line[i + 2..i + 2 + end_pos].to_string();
                segments.push(TextSegment {
                    text,
                    style: TextStyle::Bold,
                });
                i += 2 + end_pos + 2;
                continue;
            }
        }

        // Check for Italic `*...*`.
        if i + 1 < chars.len() && chars[i] == '*' {
            if let Some(end_pos) = line[i + 1..].find('*') {
                let text = line[i + 1..i + 1 + end_pos].to_string();
                segments.push(TextSegment {
                    text,
                    style: TextStyle::Italic,
                });
                i += 1 + end_pos + 1;
                continue;
            }
        }

        // Find the next segment of plain text (up to the next '*').
        let mut j = i;
        while j < chars.len() && chars[j] != '*' {
            j += 1;
        }
        let text: String = chars[i..j].iter().collect();
        if !text.is_empty() {
            segments.push(TextSegment {
                text,
                style: TextStyle::Regular,
            });
        }
        i = j;
    }

    segments
}

/// Decodes a Base64 string from a placeholder tag.
///
/// The placeholder format is expected to be `[ph:BASE64_STRING]`.
/// This function extracts and decodes the `BASE64_STRING`.
///
/// # Arguments
/// * `ph` - The inner content of the placeholder tag (e.g., `ph:BASE64_STRING`).
///
/// # Returns
/// An `Option<String>` containing the decoded text, or `None` if decoding fails.
fn decode_placeholder(ph: &str) -> Option<String> {
    let parts: Vec<&str> = ph.split(':').collect();
    parts.last().and_then(|last| {
        BASE64
            .decode(last)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    })
}

/// Loads the font family for the PDF document.
///
/// Tries to load "Arial", falling back to "LiberationSans" if not found.
///
/// # Returns
/// A `Result` containing the `FontFamily` or a `Box<dyn Error>` on failure.
fn load_font() -> Result<genpdf::fonts::FontFamily<genpdf::fonts::FontData>, Box<dyn Error>> {
    // Attempt to load Arial first, as it's a common and preferred font.
    if let Ok(family) = genpdf::fonts::from_files("./fonts", "Arial", None) {
        return Ok(family);
    }
    // Fall back to LiberationSans, a common open-source alternative.
    genpdf::fonts::from_files("./fonts", "LiberationSans", None).map_err(Into::into)
}

/// Parses text containing `<b>` and `<i>` tags and adds it to the document, preserving line breaks.
///
/// This is used for content from placeholders, which may contain simple HTML-like tags.
///
/// # Arguments
/// * `doc` - The `Document` to which the text will be added.
/// * `text` - The text to parse and add.
fn push_styled_text_with_breaks_to_doc(doc: &mut Document, text: &str) {
    let lines: Vec<&str> = text.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        doc.push(parse_styled_paragraph(line));
        // Add a line break after each line except the last one.
        if i < lines.len() - 1 {
            doc.push(Break::new(1));
        }
    }
}

/// Loads all images associated with a template from the database.
///
/// Images are stored as Base64 strings and are decoded into byte vectors.
///
/// # Arguments
/// * `conn` - A reference to the `rusqlite::Connection`.
/// * `template_id` - The ID of the template whose images should be loaded.
///
/// # Returns
/// A `Result` containing a `HashMap` mapping image IDs to their raw byte data,
/// or a `Box<dyn Error>` on failure.
pub(crate) fn load_images(
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

/// Creates and configures a new `genpdf::Document` with default settings.
///
/// Sets the font, title, font size, line spacing, and page margins.
///
/// # Returns
/// A `Result` containing the configured `Document` or a `Box<dyn Error>` on failure.
pub(crate) fn configure_document() -> Result<Document, Box<dyn Error>> {
    let font_family = load_font()?;
    let mut doc = Document::new(font_family);
    doc.set_title("Output from template");

    let font_size_pt: u8 = 11;
    doc.set_font_size(font_size_pt);

    doc.set_line_spacing(1.25);

    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(MARGIN_MM);
    doc.set_page_decorator(decorator);
    Ok(doc)
}

/// Handles a line representing a list item (e.g., "- Item text").
///
/// It adds a bullet point and the item text (with styling) to the document.
///
/// # Arguments
/// * `doc` - The `Document` to which the list item will be added.
/// * `item_text` - The text of the list item (without the "- " prefix).
pub(crate) fn handle_list_item(doc: &mut Document, item_text: &str) {
    let segments = parse_styles(item_text);
    let mut p = Paragraph::new("");
    p.push("• "); // Add a bullet point prefix.
    push_segments_into_paragraph(&mut p, &segments);
    doc.push(p);
}

/// Handles a line representing an image tag (e.g., `[img:image_id]`).
///
/// This function retrieves the image data, resizes it to fit page width and
/// CSS-like constraints, converts it to a compatible format (RGB PNG), saves it
/// to a temporary file, and adds it to the PDF document.
///
/// # Arguments
/// * `line` - The full line containing the image tag.
/// * `images_map` - A map of image IDs to their byte data.
/// * `temp_files` - A vector to hold `NamedTempFile`s, ensuring they are not deleted prematurely.
/// * `doc` - The `Document` to which the image will be added.
///
/// # Returns
/// An empty `Result` on success, or a `Box<dyn Error>` on failure.
pub(crate) fn handle_image_line(
    line: &str,
    images_map: &HashMap<String, Vec<u8>>,
    temp_files: &mut Vec<NamedTempFile>,
    doc: &mut Document,
) -> Result<(), Box<dyn Error>> {
    let inner = &line[5..line.len() - 1];
    if let Some(bytes) = images_map.get(inner) {
        // Calculate the maximum available width on the page in pixels.
        let margin_in = MARGIN_MM / 25.4_f64;
        let content_width_in = PAGE_WIDTH_INCH - 2.0 * margin_in;
        let content_target_px = content_width_in * IMAGE_DPI;

        // These values simulate max-width/max-height from CSS for consistent rendering.
        let css_max_width_px: f64 = 200.0;
        let css_max_height_px: f64 = 200.0;
        let css_to_px = IMAGE_DPI / 96.0; // Convert CSS pixels (96 DPI) to PDF pixels (IMAGE_DPI).
        let css_max_width_target_px = css_max_width_px * css_to_px;
        let css_max_height_target_px = css_max_height_px * css_to_px;

        let img = load_from_memory(bytes)?;
        let (orig_w, orig_h) = img.dimensions();
        let (orig_w_f, orig_h_f) = (orig_w as f64, orig_h as f64);

        // Determine the final scaling factor by respecting all constraints (page width, css max-width, css max-height).
        let scale_by_content = (content_target_px / orig_w_f).min(1.0);
        let scale_by_css_w = (css_max_width_target_px / orig_w_f).min(1.0);
        let scale_by_css_h = (css_max_height_target_px / orig_h_f).min(1.0);
        let scale = scale_by_content.min(scale_by_css_w).min(scale_by_css_h);

        // Resize the image only if it's larger than the target dimensions.
        let resized = if scale < 1.0 {
            let new_w = (orig_w_f * scale).max(1.0).round() as u32;
            let new_h = (orig_h_f * scale).max(1.0).round() as u32;
            img.resize(new_w, new_h, FilterType::Lanczos3)
        } else {
            img
        };

        // Convert to RGB PNG, as genpdf has better support for it.
        // This involves overlaying RGBA images on a white background to remove transparency.
        let rgba = resized.to_rgba8();
        let (w, h) = rgba.dimensions();
        let mut background = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
        image::imageops::overlay(&mut background, &rgba, 0, 0);
        let rgb_image = DynamicImage::ImageRgba8(background).to_rgb8();
        let raw = rgb_image.into_raw();

        // Write the processed image to a temporary file.
        let mut tmp = NamedTempFile::new()?;
        {
            let file = tmp.as_file_mut();
            let mut encoder = PngEncoder::new(file, w, h);
            encoder.set_color(PngColorType::Rgb);
            encoder.set_depth(PngBitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&raw)?;
        }

        // Add the image from the temp file to the document.
        let path: PathBuf = tmp.path().to_path_buf();
        let mut img_elem = PdfImage::from_path(path)?;
        img_elem.set_dpi(IMAGE_DPI);
        doc.push(img_elem);
        temp_files.push(tmp); // Keep the temp file alive until the function scope ends.
    } else {
        doc.push(Paragraph::new(format!("[image not found: {}]", inner)));
    }
    Ok(())
}

/// Handles a line representing a placeholder tag (e.g., `[ph:BASE64_STRING]`).
///
/// Decodes the Base64 content and adds it to the document, parsing any nested
/// `<b>` or `<i>` tags within the decoded text.
///
/// # Arguments
/// * `line` - The full line containing the placeholder tag.
/// * `doc` - The `Document` to which the decoded content will be added.
fn handle_placeholder_line(line: &str, doc: &mut Document) {
    let inner = &line[4..line.len() - 1];
    if let Some(decoded) = decode_placeholder(inner) {
        push_styled_text_with_breaks_to_doc(doc, &decoded);
    } else {
        doc.push(Paragraph::new("[invalid placeholder]"));
    }
}

/// Handles a normal line of text without special formatting prefixes.
///
/// Parses the line for Markdown-like styles and adds it to the document as a paragraph.
///
/// # Arguments
/// * `line` - The line of text to process.
/// * `doc` - The `Document` to which the paragraph will be added.
pub(crate) fn handle_normal_line(line: &str, doc: &mut Document) {
    let segments = parse_styles(line);
    let mut p = Paragraph::new("");
    push_segments_into_paragraph(&mut p, &segments);
    doc.push(p);
}

/// Finds the first occurrence of a `<b>` or `<i>` tag in a string.
///
/// # Arguments
/// * `text` - The string to search in.
///
/// # Returns
/// An `Option` containing the tag name ("b" or "i") and its starting position,
/// or `None` if no tags are found.
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

/// Parses a string with `<b>` and `<i>` tags into a `genpdf::Paragraph`.
///
/// This is used for content from placeholders which may contain simple HTML-like tags.
///
/// # Arguments
/// * `text` - The string to parse.
///
/// # Returns
/// A `Paragraph` with styled text.
fn parse_styled_paragraph(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::new("");
    let mut rest = text;

    while let Some((next_tag, start)) = find_next_tag(rest) {
        // Add any plain text before the tag.
        if start > 0 {
            paragraph.push(&rest[..start]);
        }

        let (tag_open, tag_close) = match next_tag {
            "b" => ("<b>", "</b>"),
            "i" => ("<i>", "</i>"),
            _ => unreachable!(),
        };

        // Find the corresponding closing tag.
        if let Some(rel_end) = rest[start + tag_open.len()..].find(tag_close) {
            let styled_text = &rest[start + tag_open.len()..start + tag_open.len() + rel_end];
            let styled = match next_tag {
                "b" => StyledString::new(styled_text, Style::new().bold()),
                "i" => StyledString::new(styled_text, Style::new().italic()),
                _ => StyledString::new(styled_text, Style::new()),
            };
            paragraph.push(styled);
            // Move past the processed segment.
            rest = &rest[start + tag_open.len() + rel_end + tag_close.len()..];
        } else {
            // If a tag is unclosed, treat the rest of the line as plain text.
            paragraph.push(&rest[start..]);
            return paragraph;
        }
    }

    // Add any remaining plain text at the end.
    if !rest.is_empty() {
        paragraph.push(rest);
    }

    paragraph
}
