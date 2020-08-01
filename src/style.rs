//! Styles for text and pages.

use fontdock::{fallback, FallbackTree, FontVariant, FontStyle, FontWeight, FontWidth};
use crate::length::{Length, Size, Margins, Value4, ScaleLength};
use crate::paper::{Paper, PaperClass, PAPER_A4};

/// Defines properties of pages and text.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LayoutStyle {
    /// The style for text.
    pub text: TextStyle,
    /// The style for pages.
    pub page: PageStyle,
}

/// Defines which fonts to use and how to space text.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// A tree of font family names and generic class names.
    pub fallback: FallbackTree,
    /// The selected font variant.
    pub variant: FontVariant,
    /// Whether the bolder toggle is active or inactive. This determines
    /// whether the next `*` adds or removes font weight.
    pub bolder: bool,
    /// Whether the italic toggle is active or inactive. This determines
    /// whether the next `_` makes italic or non-italic.
    pub italic: bool,
    /// The base font size.
    pub base_font_size: Length,
    /// The font scale to apply on the base font size.
    pub font_scale: f64,
    /// The word spacing (as a multiple of the font size).
    pub word_spacing_scale: f64,
    /// The line spacing (as a multiple of the font size).
    pub line_spacing_scale: f64,
    /// The paragraphs spacing (as a multiple of the font size).
    pub paragraph_spacing_scale: f64,
}

impl TextStyle {
    /// The scaled font size.
    pub fn font_size(&self) -> Length {
        self.base_font_size * self.font_scale
    }

    /// The absolute word spacing.
    pub fn word_spacing(&self) -> Length {
        self.word_spacing_scale * self.font_size()
    }

    /// The absolute line spacing.
    pub fn line_spacing(&self) -> Length {
        (self.line_spacing_scale - 1.0) * self.font_size()
    }

    /// The absolute paragraph spacing.
    pub fn paragraph_spacing(&self) -> Length {
        (self.paragraph_spacing_scale - 1.0) * self.font_size()
    }
}

impl Default for TextStyle {
    fn default() -> TextStyle {
        TextStyle {
            fallback: fallback! {
                list: ["sans-serif"],
                classes: {
                    "serif" => ["source serif pro", "noto serif"],
                    "sans-serif" => ["source sans pro", "noto sans"],
                    "monospace" => ["source code pro", "noto sans mono"],
                    "math" => ["latin modern math", "serif"],
                },
                base: ["source sans pro", "noto sans",
                       "noto emoji", "latin modern math"],
            },
            variant: FontVariant {
                style: FontStyle::Normal,
                weight: FontWeight(400),
                width: FontWidth::Medium,
            },
            bolder: false,
            italic: false,
            base_font_size: Length::pt(11.0),
            font_scale: 1.0,
            word_spacing_scale: 0.25,
            line_spacing_scale: 1.2,
            paragraph_spacing_scale: 1.5,
        }
    }
}

/// Defines the size and margins of a page.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PageStyle {
    /// The class of this page.
    pub class: PaperClass,
    /// The width and height of the page.
    pub dimensions: Size,
    /// The amount of white space on each side. If a side is set to `None`, the
    /// default for the paper class is used.
    pub margins: Value4<Option<ScaleLength>>,
}

impl PageStyle {
    /// The default page style for the given paper.
    pub fn new(paper: Paper) -> PageStyle {
        PageStyle {
            class: paper.class,
            dimensions: paper.size(),
            margins: Value4::with_all(None),
        }
    }

    /// The absolute margins.
    pub fn margins(&self) -> Margins {
        let dims = self.dimensions;
        let default = self.class.default_margins();

        Margins {
            left: self.margins.left.unwrap_or(default.left).scaled(dims.x),
            top: self.margins.top.unwrap_or(default.top).scaled(dims.y),
            right: self.margins.right.unwrap_or(default.right).scaled(dims.x),
            bottom: self.margins.bottom.unwrap_or(default.bottom).scaled(dims.y),
        }
    }
}

impl Default for PageStyle {
    fn default() -> PageStyle {
        PageStyle::new(PAPER_A4)
    }
}