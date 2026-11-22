//! # nmc-preview
//!
//! Native previews of files that are neither plain text nor a picture,
//! for the viewer (F3) and the quick view of every interface:
//!
//! * [`ebook`]: EPUB and FictionBook e-books as formatted text.
//! * [`media`]: an information sheet for audio and video files.
//! * [`markdown`]: Markdown as formatted text.
//! * [`office`]: Word, PowerPoint, Excel and OpenDocument files as
//!   formatted text and tables.
//! * [`pdf`]: PDF pages as pictures (pure Rust, `hayro`).
//! * [`model`]: STL, OBJ, PLY and glTF models drawn shaded from any angle
//!   by a small software renderer.
//! * [`syntax`]: colours for source and data files (shell, YAML, JSON,
//!   XML, HTML, SQL, TypeScript, Rust, Python) from a small hand-written
//!   lexer.
//!
//! Formatted text is laid out by [`rich`] into lines of styled spans that
//! a terminal or a window draws with one fixed-width font. Everything here
//! is plain data in, plain data out: no interface, no files, no threads.

pub mod ebook;
pub mod markdown;
pub mod media;
pub mod model;
pub mod office;
pub mod pdf;
pub mod rich;
pub mod samples;
pub mod syntax;

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
    /// An e-book.
    Ebook(ebook::Format),
    /// An audio file (its tags and properties).
    Audio,
    /// A video file (its container information).
    Video,
}

impl Kind {
    /// The preview for a file named `name`, if it gets one.
    pub fn of(name: &str) -> Option<Kind> {
        let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
        match extension.as_str() {
            "md" | "markdown" | "mdown" | "mkd" => Some(Kind::Markdown),
            "pdf" => Some(Kind::Pdf),
            _ if model::is_model_name(name) => Some(Kind::Model),
            _ if media::is_audio_name(name) => Some(Kind::Audio),
            _ if media::is_video_name(name) => Some(Kind::Video),
            _ if ebook::Format::of(name).is_some() => ebook::Format::of(name).map(Kind::Ebook),
            _ => office::Format::of(name).map(Kind::Office),
        }
    }

    /// A short name for status lines, such as `Word document` or `PDF`.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Markdown => "Markdown",
            Kind::Office(office::Format::Sheet) => "Spreadsheet",
            Kind::Office(office::Format::Pptx) => "PowerPoint slides (text)",
            Kind::Office(office::Format::Odp) => "OpenDocument slides (text)",
            Kind::Office(office::Format::Docx) => "Word document",
            Kind::Office(office::Format::Odt) => "OpenDocument text",
            Kind::Pdf => "PDF",
            Kind::Model => "3D model",
            Kind::Ebook(ebook::Format::Epub) => "EPUB e-book",
            Kind::Ebook(ebook::Format::Fb2) => "FictionBook e-book",
            Kind::Audio => "Audio file (information)",
            Kind::Video => "Video file (information)",
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
        assert_eq!(Kind::of("book.epub"), Some(Kind::Ebook(ebook::Format::Epub)));
        assert_eq!(Kind::of("book.FB2"), Some(Kind::Ebook(ebook::Format::Fb2)));
        assert_eq!(Kind::of("song.mp3"), Some(Kind::Audio));
        assert_eq!(Kind::of("film.mkv"), Some(Kind::Video));
        assert_eq!(Kind::of("notes.txt"), None);
        assert_eq!(Kind::of("Makefile"), None);
    }
}
