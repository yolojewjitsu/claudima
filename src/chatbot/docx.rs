//! DOCX text extraction.
//!
//! Extracts plain text from .docx files (Office Open XML format).
//! DOCX files are ZIP archives containing XML documents.

use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Extract plain text from a DOCX file.
///
/// DOCX structure:
/// - word/document.xml contains the main body text
/// - Text is in <w:t> elements within <w:p> (paragraph) elements
///
/// Returns the extracted text, or an error message if extraction fails.
pub fn extract_text(data: &[u8]) -> Result<String, String> {
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| format!("Invalid DOCX (not a valid ZIP): {e}"))?;

    // Find and read word/document.xml
    let mut document_xml = String::new();
    {
        let mut file = archive
            .by_name("word/document.xml")
            .map_err(|_| "Invalid DOCX: missing word/document.xml")?;
        file.read_to_string(&mut document_xml)
            .map_err(|e| format!("Failed to read document.xml: {e}"))?;
    }

    // Parse XML and extract text from <w:t> elements
    let text = extract_text_from_xml(&document_xml);

    if text.trim().is_empty() {
        return Err("DOCX appears to be empty or contains no text".to_string());
    }

    Ok(text)
}

/// Extract text content from Word XML.
///
/// Finds all <w:t> (text) elements and joins them, preserving paragraph breaks.
fn extract_text_from_xml(xml: &str) -> String {
    let mut result = String::new();
    let mut in_paragraph = false;
    let mut paragraph_text = String::new();

    // Simple state machine to extract text
    // We look for:
    // - <w:p ...> to start a paragraph
    // - </w:p> to end a paragraph (add newline)
    // - <w:t> or <w:t ...> to start text content
    // - </w:t> to end text content
    // - Content between <w:t> and </w:t>

    let mut chars = xml.chars().peekable();
    let mut in_text_element = false;

    while let Some(c) = chars.next() {
        if c == '<' {
            // Check if closing tag
            let is_closing = chars.peek() == Some(&'/');
            if is_closing {
                chars.next(); // consume '/'
            }

            // Read tag name
            let mut tag = String::new();
            while let Some(&next) = chars.peek() {
                if next == '>' || next == ' ' || next == '/' {
                    break;
                }
                tag.push(chars.next().unwrap());
            }

            // Skip to end of tag
            let mut is_self_closing = false;
            while let Some(&next) = chars.peek() {
                if next == '/' {
                    is_self_closing = true;
                }
                if chars.next() == Some('>') {
                    break;
                }
            }

            if is_closing {
                // Closing tag
                match tag.as_str() {
                    "w:p" => {
                        if in_paragraph && !paragraph_text.trim().is_empty() {
                            if !result.is_empty() {
                                result.push('\n');
                            }
                            result.push_str(paragraph_text.trim());
                        }
                        in_paragraph = false;
                        paragraph_text.clear();
                    }
                    "w:t" => {
                        in_text_element = false;
                    }
                    _ => {}
                }
            } else {
                // Opening tag
                match tag.as_str() {
                    "w:p" => {
                        in_paragraph = true;
                        paragraph_text.clear();
                    }
                    "w:t" => {
                        if !is_self_closing {
                            in_text_element = true;
                        }
                    }
                    // Handle line breaks within paragraphs
                    "w:br" => {
                        if in_paragraph {
                            paragraph_text.push('\n');
                        }
                    }
                    // Handle tabs
                    "w:tab" => {
                        if in_paragraph {
                            paragraph_text.push('\t');
                        }
                    }
                    _ => {}
                }
            }
        } else if in_text_element && in_paragraph {
            // Decode XML entities
            if c == '&' {
                let mut entity = String::new();
                while let Some(&next) = chars.peek() {
                    if next == ';' {
                        chars.next();
                        break;
                    }
                    entity.push(chars.next().unwrap());
                }
                match entity.as_str() {
                    "lt" => paragraph_text.push('<'),
                    "gt" => paragraph_text.push('>'),
                    "amp" => paragraph_text.push('&'),
                    "quot" => paragraph_text.push('"'),
                    "apos" => paragraph_text.push('\''),
                    _ => {
                        // Unknown entity, include as-is
                        paragraph_text.push('&');
                        paragraph_text.push_str(&entity);
                        paragraph_text.push(';');
                    }
                }
            } else {
                paragraph_text.push(c);
            }
        }
    }

    // Handle any remaining paragraph
    if in_paragraph && !paragraph_text.trim().is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(paragraph_text.trim());
    }

    result
}

/// Get a preview of document content (first N chars).
pub fn preview(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        let mut end = max_chars;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &text[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_from_xml_simple() {
        let xml = r"<w:document><w:body><w:p><w:r><w:t>Hello World</w:t></w:r></w:p></w:body></w:document>";
        let text = extract_text_from_xml(xml);
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn test_extract_text_from_xml_multiple_paragraphs() {
        let xml = r"<w:document><w:body><w:p><w:r><w:t>First paragraph</w:t></w:r></w:p><w:p><w:r><w:t>Second paragraph</w:t></w:r></w:p></w:body></w:document>";
        let text = extract_text_from_xml(xml);
        assert!(text.contains("First paragraph"));
        assert!(text.contains("Second paragraph"));
        assert!(text.contains('\n')); // Newline between paragraphs
    }

    #[test]
    fn test_extract_text_from_xml_with_entities() {
        let xml = r"<w:document><w:body><w:p><w:r><w:t>A &lt; B &amp; C &gt; D</w:t></w:r></w:p></w:body></w:document>";
        let text = extract_text_from_xml(xml);
        assert_eq!(text, "A < B & C > D");
    }

    #[test]
    fn test_preview_short() {
        let text = "Hello";
        assert_eq!(preview(text, 10), "Hello");
    }

    #[test]
    fn test_preview_truncated() {
        let text = "Hello World";
        assert_eq!(preview(text, 5), "Hello...");
    }
}
