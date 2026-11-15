//! # sc-preview
//!
//! Native previews of files that are neither plain text nor a picture,
//! for the viewer (F3) and the quick view of every interface:
//!
//! * [`markdown`]: Markdown as formatted text.
//! * [`office`]: Word, PowerPoint, Excel and OpenDocument files as
//!   formatted text and tables.
//! * [`pdf`]: PDF pages as pictures (pure Rust, `hayro`).
//! * [`model`]: STL, OBJ, PLY and glTF models drawn shaded from any angle
//!   by a small software renderer.
//!
//! Formatted text is laid out by [`rich`] into lines of styled spans that
//! a terminal or a window draws with one fixed-width font. Everything here
//! is plain data in, plain data out: no interface, no files, no threads.

pub mod markdown;
pub mod model;
pub mod office;
pub mod pdf;
pub mod rich;
pub mod samples;

/// What kind of preview a file gets, from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Markdown text.
    Markdown,
    /// An Office document or spreadsheet.
    Office(office::Format),
    /// A PDF file.
    Pdf,
    /// A 3D model.
    Model,
}

impl Kind {
    /// The preview for a file named `name`, if it gets one.
    pub fn of(name: &str) -> Option<Kind> {
        let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
        match extension.as_str() {
            "md" | "markdown" | "mdown" | "mkd" => Some(Kind::Markdown),
            "pdf" => Some(Kind::Pdf),
            _ if model::is_model_name(name) => Some(Kind::Model),
            _ => office::Format::of(name).map(Kind::Office),
        }
    }

    /// A short name for status lines: `markdown`, `pdf`, `document`,
    /// `model`.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Markdown => "markdown",
            Kind::Office(office::Format::Sheet) => "spreadsheet",
            Kind::Office(office::Format::Pptx | office::Format::Odp) => "slides",
            Kind::Office(_) => "document",
            Kind::Pdf => "pdf",
            Kind::Model => "3d model",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_the_extension() {
        assert_eq!(Kind::of("README.md"), Some(Kind::Markdown));
        assert_eq!(Kind::of("a.PDF"), Some(Kind::Pdf));
        assert_eq!(Kind::of("part.stl"), Some(Kind::Model));
        assert_eq!(Kind::of("scene.glb"), Some(Kind::Model));
        assert_eq!(Kind::of("letter.docx"), Some(Kind::Office(office::Format::Docx)));
        assert_eq!(Kind::of("budget.ods"), Some(Kind::Office(office::Format::Sheet)));
        assert_eq!(Kind::of("notes.txt"), None);
        assert_eq!(Kind::of("Makefile"), None);
    }
}
