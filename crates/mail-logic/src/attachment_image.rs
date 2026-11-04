//! What a preview of an attachment is made of, as values: what a part's first bytes say it is, the
//! room a picture is fitted into, a picture ready to draw, a PDF's page and why a part has none.
//! Reading the stored bytes and decoding them is `mail_core::attachment_image`'s.

/// What a stored part's first bytes say it is, of what can be previewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Png,
    Jpeg,
    Gif,
    WebP,
    Pdf,
}

/// What `head`, a part's first bytes, is: one of the kinds drawn, or `None` for anything else,
/// SVG included.
pub fn sniff(head: &[u8]) -> Option<Kind> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Kind::Png)
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Kind::Jpeg)
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some(Kind::Gif)
    } else if head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"WEBP" {
        Some(Kind::WebP)
    } else if head.starts_with(b"%PDF-") {
        Some(Kind::Pdf)
    } else {
        None
    }
}

/// The room a picture is fitted into, in device pixels. The aspect is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    pub width: u32,
    pub height: u32,
}

/// A strip thumbnail: 56 logical pixels square, drawn at up to 2x.
pub const SMALL: Fit = Fit {
    width: 112,
    height: 112,
};

/// The viewer's picture.
pub const LARGE: Fit = Fit {
    width: 1600,
    height: 1200,
};

/// The viewer's PDF page.
pub const PDF_PAGE: Fit = Fit {
    width: 1200,
    height: 1600,
};

/// A picture ready to draw: a `data:` URI of a PNG, and its size in pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub uri: String,
    pub width: u32,
    pub height: u32,
}

/// Why a part here is not drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Its header claims more pixels than are decoded to preview anything.
    TooLarge { width: u32, height: u32 },
    /// More bytes than are read to preview a part of its kind.
    TooManyBytes(u64),
    /// A PDF that needs a password.
    Locked,
    /// A PDF with no pages.
    Empty,
    /// Damaged, or not what its first bytes said.
    Unreadable,
}

impl Refusal {
    /// What the strip and the viewer say instead of a picture.
    pub fn sentence(self) -> String {
        match self {
            Refusal::TooLarge { width, height } => {
                format!("Too large to preview ({width} × {height})")
            }
            Refusal::TooManyBytes(size) => {
                format!("Too large to preview ({})", mail_domain::human_size(size))
            }
            Refusal::Locked => "Locked PDF, no preview".to_owned(),
            Refusal::Empty => "A PDF with no pages".to_owned(),
            Refusal::Unreadable => "No preview: the file could not be read".to_owned(),
        }
    }
}

/// Why a part has no preview at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// Still on the server: nothing is fetched to preview it.
    NotHere,
    /// Not a kind that is drawn.
    NotAPicture,
    /// A kind that is drawn, refused.
    Refused(Refusal),
    /// The store could not answer.
    Store(String),
}

/// One page of a PDF, drawn, and how many it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub picture: Picture,
    /// Which page, from 0: the one asked for, held to the last there is.
    pub number: u32,
    pub count: u32,
}
