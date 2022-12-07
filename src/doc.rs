//! Finished documents.

use std::fmt::{self, Debug, Formatter, Write};
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::Arc;

use crate::font::Font;
use crate::geom::{
    Abs, Align, Axes, Dir, Em, Numeric, Paint, Point, Shape, Size, Transform,
};
use crate::image::Image;
use crate::model::{dict, node, Content, Dict, Fold, StableId, StyleChain, Value};
use crate::util::EcoString;

/// A finished document with metadata and page frames.
#[derive(Debug, Default, Clone)]
pub struct Document {
    /// The page frames.
    pub pages: Vec<Frame>,
    /// The document's title.
    pub title: Option<EcoString>,
    /// The document's author.
    pub author: Option<EcoString>,
}

/// A partial layout result.
#[derive(Clone)]
pub struct Fragment(Vec<Frame>);

impl Fragment {
    /// Create a fragment from a single frame.
    pub fn frame(frame: Frame) -> Self {
        Self(vec![frame])
    }

    /// Create a fragment from multiple frames.
    pub fn frames(frames: Vec<Frame>) -> Self {
        Self(frames)
    }

    /// The number of frames in the fragment.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Extract the first and only frame.
    ///
    /// Panics if there are multiple frames.
    #[track_caller]
    pub fn into_frame(self) -> Frame {
        assert_eq!(self.0.len(), 1, "expected exactly one frame");
        self.0.into_iter().next().unwrap()
    }

    /// Extract the frames.
    pub fn into_frames(self) -> Vec<Frame> {
        self.0
    }

    /// Iterate over the contained frames.
    pub fn iter(&self) -> std::slice::Iter<Frame> {
        self.0.iter()
    }

    /// Iterate over the contained frames.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<Frame> {
        self.0.iter_mut()
    }
}

impl Debug for Fragment {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self.0.as_slice() {
            [frame] => frame.fmt(f),
            frames => frames.fmt(f),
        }
    }
}

impl IntoIterator for Fragment {
    type Item = Frame;
    type IntoIter = std::vec::IntoIter<Frame>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Fragment {
    type Item = &'a Frame;
    type IntoIter = std::slice::Iter<'a, Frame>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a mut Fragment {
    type Item = &'a mut Frame;
    type IntoIter = std::slice::IterMut<'a, Frame>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

/// A finished layout with elements at fixed positions.
#[derive(Default, Clone)]
pub struct Frame {
    /// The size of the frame.
    size: Size,
    /// The baseline of the frame measured from the top. If this is `None`, the
    /// frame's implicit baseline is at the bottom.
    baseline: Option<Abs>,
    /// The elements composing this layout.
    elements: Arc<Vec<(Point, Element)>>,
}

/// Constructor, accessors and setters.
impl Frame {
    /// Create a new, empty frame.
    ///
    /// Panics the size is not finite.
    #[track_caller]
    pub fn new(size: Size) -> Self {
        assert!(size.is_finite());
        Self { size, baseline: None, elements: Arc::new(vec![]) }
    }

    /// Whether the frame contains no elements.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// The size of the frame.
    pub fn size(&self) -> Size {
        self.size
    }

    /// The size of the frame, mutably.
    pub fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    /// Set the size of the frame.
    pub fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    /// The width of the frame.
    pub fn width(&self) -> Abs {
        self.size.x
    }

    /// The height of the frame.
    pub fn height(&self) -> Abs {
        self.size.y
    }

    /// The baseline of the frame.
    pub fn baseline(&self) -> Abs {
        self.baseline.unwrap_or(self.size.y)
    }

    /// Set the frame's baseline from the top.
    pub fn set_baseline(&mut self, baseline: Abs) {
        self.baseline = Some(baseline);
    }

    /// An iterator over the elements inside this frame alongside their
    /// positions relative to the top-left of the frame.
    pub fn elements(&self) -> std::slice::Iter<'_, (Point, Element)> {
        self.elements.iter()
    }

    /// Recover the text inside of the frame and its children.
    pub fn text(&self) -> EcoString {
        let mut text = EcoString::new();
        for (_, element) in self.elements() {
            match element {
                Element::Text(content) => {
                    for glyph in &content.glyphs {
                        text.push(glyph.c);
                    }
                }
                Element::Group(group) => text.push_str(&group.frame.text()),
                _ => {}
            }
        }
        text
    }
}

/// Insert elements and subframes.
impl Frame {
    /// The layer the next item will be added on. This corresponds to the number
    /// of elements in the frame.
    pub fn layer(&self) -> usize {
        self.elements.len()
    }

    /// Add an element at a position in the foreground.
    pub fn push(&mut self, pos: Point, element: Element) {
        Arc::make_mut(&mut self.elements).push((pos, element));
    }

    /// Add a frame at a position in the foreground.
    ///
    /// Automatically decides whether to inline the frame or to include it as a
    /// group based on the number of elements in it.
    pub fn push_frame(&mut self, pos: Point, frame: Frame) {
        if self.should_inline(&frame) {
            self.inline(self.layer(), pos, frame);
        } else {
            self.push(pos, Element::Group(Group::new(frame)));
        }
    }

    /// Insert an element at the given layer in the frame.
    ///
    /// This panics if the layer is greater than the number of layers present.
    #[track_caller]
    pub fn insert(&mut self, layer: usize, pos: Point, element: Element) {
        Arc::make_mut(&mut self.elements).insert(layer, (pos, element));
    }

    /// Add an element at a position in the background.
    pub fn prepend(&mut self, pos: Point, element: Element) {
        Arc::make_mut(&mut self.elements).insert(0, (pos, element));
    }

    /// Add multiple elements at a position in the background.
    ///
    /// The first element in the iterator will be the one that is most in the
    /// background.
    pub fn prepend_multiple<I>(&mut self, elements: I)
    where
        I: IntoIterator<Item = (Point, Element)>,
    {
        Arc::make_mut(&mut self.elements).splice(0..0, elements);
    }

    /// Add a frame at a position in the background.
    pub fn prepend_frame(&mut self, pos: Point, frame: Frame) {
        if self.should_inline(&frame) {
            self.inline(0, pos, frame);
        } else {
            self.prepend(pos, Element::Group(Group::new(frame)));
        }
    }

    /// Whether the given frame should be inlined.
    fn should_inline(&self, frame: &Frame) -> bool {
        self.elements.is_empty() || frame.elements.len() <= 5
    }

    /// Inline a frame at the given layer.
    fn inline(&mut self, layer: usize, pos: Point, frame: Frame) {
        // Try to just reuse the elements.
        if pos.is_zero() && self.elements.is_empty() {
            self.elements = frame.elements;
            return;
        }

        // Try to transfer the elements without adjusting the position.
        // Also try to reuse the elements if the Arc isn't shared.
        let range = layer..layer;
        if pos.is_zero() {
            let sink = Arc::make_mut(&mut self.elements);
            match Arc::try_unwrap(frame.elements) {
                Ok(elements) => {
                    sink.splice(range, elements);
                }
                Err(arc) => {
                    sink.splice(range, arc.iter().cloned());
                }
            }
            return;
        }

        // We must adjust the element positions.
        // But still try to reuse the elements if the Arc isn't shared.
        let sink = Arc::make_mut(&mut self.elements);
        match Arc::try_unwrap(frame.elements) {
            Ok(elements) => {
                sink.splice(range, elements.into_iter().map(|(p, e)| (p + pos, e)));
            }
            Err(arc) => {
                sink.splice(range, arc.iter().cloned().map(|(p, e)| (p + pos, e)));
            }
        }
    }
}

/// Modify the frame.
impl Frame {
    /// Remove all elements from the frame.
    pub fn clear(&mut self) {
        if Arc::strong_count(&self.elements) == 1 {
            Arc::make_mut(&mut self.elements).clear();
        } else {
            self.elements = Arc::new(vec![]);
        }
    }

    /// Resize the frame to a new size, distributing new space according to the
    /// given alignments.
    pub fn resize(&mut self, target: Size, aligns: Axes<Align>) {
        if self.size != target {
            let offset = Point::new(
                aligns.x.position(target.x - self.size.x),
                aligns.y.position(target.y - self.size.y),
            );
            self.size = target;
            self.translate(offset);
        }
    }

    /// Move the baseline and contents of the frame by an offset.
    pub fn translate(&mut self, offset: Point) {
        if !offset.is_zero() {
            if let Some(baseline) = &mut self.baseline {
                *baseline += offset.y;
            }
            for (point, _) in Arc::make_mut(&mut self.elements) {
                *point += offset;
            }
        }
    }

    /// Attach the metadata from this style chain to the frame.
    pub fn meta(&mut self, styles: StyleChain) {
        for meta in styles.get(Meta::DATA) {
            self.push(Point::zero(), Element::Meta(meta, self.size));
        }
    }

    /// Arbitrarily transform the contents of the frame.
    pub fn transform(&mut self, transform: Transform) {
        self.group(|g| g.transform = transform);
    }

    /// Clip the contents of a frame to its size.
    pub fn clip(&mut self) {
        self.group(|g| g.clips = true);
    }

    /// Wrap the frame's contents in a group and modify that group with `f`.
    fn group<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Group),
    {
        let mut wrapper = Frame::new(self.size);
        wrapper.baseline = self.baseline;
        let mut group = Group::new(std::mem::take(self));
        f(&mut group);
        wrapper.push(Point::zero(), Element::Group(group));
        *self = wrapper;
    }
}

impl Debug for Frame {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_list()
            .entries(self.elements.iter().map(|(_, element)| element))
            .finish()
    }
}

/// The building block frames are composed of.
#[derive(Clone)]
pub enum Element {
    /// A group of elements.
    Group(Group),
    /// A run of shaped text.
    Text(Text),
    /// A geometric shape with optional fill and stroke.
    Shape(Shape),
    /// An image and its size.
    Image(Image, Size),
    /// Meta information and the region it applies to.
    Meta(Meta, Size),
}

impl Debug for Element {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Group(group) => group.fmt(f),
            Self::Text(text) => write!(f, "{text:?}"),
            Self::Shape(shape) => write!(f, "{shape:?}"),
            Self::Image(image, _) => write!(f, "{image:?}"),
            Self::Meta(meta, _) => write!(f, "{meta:?}"),
        }
    }
}

/// A group of elements with optional clipping.
#[derive(Clone)]
pub struct Group {
    /// The group's frame.
    pub frame: Frame,
    /// A transformation to apply to the group.
    pub transform: Transform,
    /// Whether the frame should be a clipping boundary.
    pub clips: bool,
}

impl Group {
    /// Create a new group with default settings.
    pub fn new(frame: Frame) -> Self {
        Self {
            frame,
            transform: Transform::identity(),
            clips: false,
        }
    }
}

impl Debug for Group {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("Group ")?;
        self.frame.fmt(f)
    }
}

/// A run of shaped text.
#[derive(Clone, Eq, PartialEq)]
pub struct Text {
    /// The font the glyphs are contained in.
    pub font: Font,
    /// The font size.
    pub size: Abs,
    /// Glyph color.
    pub fill: Paint,
    /// The natural language of the text.
    pub lang: Lang,
    /// The glyphs.
    pub glyphs: Vec<Glyph>,
}

impl Text {
    /// The width of the text run.
    pub fn width(&self) -> Abs {
        self.glyphs.iter().map(|g| g.x_advance).sum::<Em>().at(self.size)
    }
}

impl Debug for Text {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        // This is only a rough approxmiation of the source text.
        f.write_str("Text(\"")?;
        for glyph in &self.glyphs {
            for c in glyph.c.escape_debug() {
                f.write_char(c)?;
            }
        }
        f.write_str("\")")
    }
}

/// A glyph in a run of shaped text.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Glyph {
    /// The glyph's index in the font.
    pub id: u16,
    /// The advance width of the glyph.
    pub x_advance: Em,
    /// The horizontal offset of the glyph.
    pub x_offset: Em,
    /// The first character of the glyph's cluster.
    pub c: char,
}

/// An identifier for a natural language.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Lang([u8; 3], u8);

impl Lang {
    pub const ENGLISH: Self = Self(*b"en ", 2);
    pub const GERMAN: Self = Self(*b"de ", 2);

    /// Return the language code as an all lowercase string slice.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0[..usize::from(self.1)]).unwrap_or_default()
    }

    /// The default direction for the language.
    pub fn dir(self) -> Dir {
        match self.as_str() {
            "ar" | "dv" | "fa" | "he" | "ks" | "pa" | "ps" | "sd" | "ug" | "ur"
            | "yi" => Dir::RTL,
            _ => Dir::LTR,
        }
    }
}

impl FromStr for Lang {
    type Err = &'static str;

    /// Construct a language from a two- or three-byte ISO 639-1/2/3 code.
    fn from_str(iso: &str) -> Result<Self, Self::Err> {
        let len = iso.len();
        if matches!(len, 2..=3) && iso.is_ascii() {
            let mut bytes = [b' '; 3];
            bytes[..len].copy_from_slice(iso.as_bytes());
            bytes.make_ascii_lowercase();
            Ok(Self(bytes, len as u8))
        } else {
            Err("expected two or three letter language code (ISO 639-1/2/3)")
        }
    }
}

/// An identifier for a region somewhere in the world.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Region([u8; 2]);

impl Region {
    /// Return the region code as an all uppercase string slice.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl FromStr for Region {
    type Err = &'static str;

    /// Construct a region from its two-byte ISO 3166-1 alpha-2 code.
    fn from_str(iso: &str) -> Result<Self, Self::Err> {
        if iso.len() == 2 && iso.is_ascii() {
            let mut bytes: [u8; 2] = iso.as_bytes().try_into().unwrap();
            bytes.make_ascii_uppercase();
            Ok(Self(bytes))
        } else {
            Err("expected two letter region code (ISO 3166-1 alpha-2)")
        }
    }
}

/// Meta information that isn't visible or renderable.
#[derive(Debug, Clone, Hash)]
pub enum Meta {
    /// An internal or external link.
    Link(Destination),
    /// An identifiable piece of content that produces something within the
    /// area this metadata is attached to.
    Node(StableId, Content),
}

#[node]
impl Meta {
    /// Metadata that should be attached to all elements affected by this style
    /// property.
    #[property(fold, skip)]
    pub const DATA: Vec<Meta> = vec![];
}

impl Fold for Vec<Meta> {
    type Output = Self;

    fn fold(mut self, outer: Self::Output) -> Self::Output {
        self.extend(outer);
        self
    }
}

/// A link destination.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum Destination {
    /// A link to a point on a page.
    Internal(Location),
    /// A link to a URL.
    Url(EcoString),
}

/// A physical location in a document.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct Location {
    /// The page, starting at 1.
    pub page: NonZeroUsize,
    /// The exact coordinates on the page (from the top left, as usual).
    pub pos: Point,
}

impl Location {
    /// Encode into a user-facing dictionary.
    pub fn encode(&self) -> Dict {
        dict! {
            "page" => Value::Int(self.page.get() as i64),
            "x" => Value::Length(self.pos.x.into()),
            "y" => Value::Length(self.pos.y.into()),
        }
    }
}

/// Standard semantic roles.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Role {
    /// A paragraph.
    Paragraph,
    /// A heading of the given level and whether it should be part of the
    /// outline.
    Heading { level: NonZeroUsize, outlined: bool },
    /// A generic block-level subdivision.
    GenericBlock,
    /// A generic inline subdivision.
    GenericInline,
    /// A list and whether it is ordered.
    List { ordered: bool },
    /// A list item. Must have a list parent.
    ListItem,
    /// The label of a list item. Must have a list item parent.
    ListLabel,
    /// The body of a list item. Must have a list item parent.
    ListItemBody,
    /// A mathematical formula.
    Formula,
    /// A table.
    Table,
    /// A table row. Must have a table parent.
    TableRow,
    /// A table cell. Must have a table row parent.
    TableCell,
    /// A code fragment.
    Code,
    /// A page header.
    Header,
    /// A page footer.
    Footer,
    /// A page background.
    Background,
    /// A page foreground.
    Foreground,
}