//! Exporting of layouts into _PDF_ documents.

use std::collections::HashMap;
use std::io::{self, Write};

use tide::{PdfWriter, Rect, Ref, Trailer, Version};
use tide::content::Content;
use tide::doc::{Catalog, Page, PageTree, Resource, Text};
use tide::font::{
    CIDFont, CIDFontType, CIDSystemInfo, FontDescriptor, FontFlags, Type0Font,
    CMap, CMapEncoding, FontStream, GlyphUnit, WidthRecord,
};

use fontdock::FaceId;
use ttf_parser::{name_id, GlyphId};

use crate::SharedFontLoader;
use crate::layout::{MultiLayout, Layout, LayoutAction};
use crate::length::Length;

/// Export a layouted list of boxes. The same font loader as used for
/// layouting needs to be passed in here since the layout only contains
/// indices referencing the loaded faces. The raw PDF ist written into the
/// target writable, returning the number of bytes written.
pub fn export<W: Write>(
    layout: &MultiLayout,
    loader: &SharedFontLoader,
    target: W,
) -> io::Result<usize> {
    PdfExporter::new(layout, loader, target)?.write()
}

/// The data relevant to the export of one document.
struct PdfExporter<'a, W: Write> {
    writer: PdfWriter<W>,
    layouts: &'a MultiLayout,
    loader: &'a SharedFontLoader,
    /// Since we cross-reference pages and faces with their IDs already in the
    /// document catalog, we need to know exactly which ID is used for what from
    /// the beginning. Thus, we compute a range for each category of object and
    /// stored these here.
    offsets: Offsets,
    // Font remapping, see below at `remap_fonts`.
    to_pdf: HashMap<FaceId, usize>,
    to_fontdock: Vec<FaceId>,
}

/// Indicates which range of PDF IDs will be used for which contents.
struct Offsets {
    catalog: Ref,
    page_tree: Ref,
    pages: (Ref, Ref),
    contents: (Ref, Ref),
    fonts: (Ref, Ref),
}

const NUM_OBJECTS_PER_FONT: u32 = 5;

impl<'a, W: Write> PdfExporter<'a, W> {
    /// Prepare the export. Only once [`ExportProcess::write`] is called the
    /// writing really happens.
    fn new(
        layouts: &'a MultiLayout,
        loader: &'a SharedFontLoader,
        target: W,
    ) -> io::Result<PdfExporter<'a, W>> {
        let (to_pdf, to_fontdock) = remap_fonts(layouts);
        let offsets = calculate_offsets(layouts.len(), to_pdf.len());

        Ok(PdfExporter {
            writer: PdfWriter::new(target),
            layouts,
            offsets,
            to_pdf,
            to_fontdock,
            loader,
        })
    }

    /// Write everything (writing entry point).
    fn write(&mut self) -> io::Result<usize> {
        self.writer.write_header(Version::new(1, 7))?;
        self.write_preface()?;
        self.write_pages()?;
        self.write_fonts()?;
        self.writer.write_xref_table()?;
        self.writer.write_trailer(Trailer::new(self.offsets.catalog))?;
        Ok(self.writer.written())
    }

    /// Write the document catalog and page tree.
    fn write_preface(&mut self) -> io::Result<()> {
        // The document catalog.
        self.writer.write_obj(self.offsets.catalog, &Catalog::new(self.offsets.page_tree))?;

        // The font resources.
        let start = self.offsets.fonts.0;
        let fonts = (0 .. self.to_pdf.len() as u32).map(|i| {
            Resource::Font(i + 1, start + (NUM_OBJECTS_PER_FONT * i))
        });

        // The root page tree.
        self.writer.write_obj(
            self.offsets.page_tree,
            PageTree::new()
                .kids(ids(self.offsets.pages))
                .resources(fonts),
        )?;

        // The page objects (non-root nodes in the page tree).
        let iter = ids(self.offsets.pages)
            .zip(ids(self.offsets.contents))
            .zip(self.layouts);

        for ((page_id, content_id), page) in iter {
            let rect = Rect::new(
                0.0,
                0.0,
                page.dimensions.x.to_pt() as f32,
                page.dimensions.y.to_pt() as f32,
            );

            self.writer.write_obj(
                page_id,
                Page::new(self.offsets.page_tree)
                    .media_box(rect)
                    .content(content_id),
            )?;
        }

        Ok(())
    }

    /// Write the contents of all pages.
    fn write_pages(&mut self) -> io::Result<()> {
        for (id, page) in ids(self.offsets.contents).zip(self.layouts) {
            self.write_page(id, &page)?;
        }
        Ok(())
    }

    /// Write the content of a page.
    fn write_page(&mut self, id: u32, page: &Layout) -> io::Result<()> {
        // Moves and face switches are always cached and only flushed once
        // needed.
        let mut text = Text::new();
        let mut face_id = FaceId::MAX;
        let mut font_size = Length::ZERO;
        let mut next_pos = None;

        for action in &page.actions {
            match action {
                LayoutAction::MoveAbsolute(pos) => {
                    next_pos = Some(*pos);
                },

                &LayoutAction::SetFont(id, size) => {
                    face_id = id;
                    font_size = size;
                    text.tf(self.to_pdf[&id] as u32 + 1, font_size.to_pt() as f32);
                }

                LayoutAction::WriteText(string) => {
                    if let Some(pos) = next_pos.take() {
                        let x = pos.x.to_pt();
                        let y = (page.dimensions.y - pos.y - font_size).to_pt();
                        text.tm(1.0, 0.0, 0.0, 1.0, x as f32, y as f32);
                    }

                    let loader = self.loader.borrow();
                    let face = loader.get_loaded(face_id);
                    text.tj(face.encode_text(&string));
                },

                LayoutAction::DebugBox(_) => {}
            }
        }

        self.writer.write_obj(id, &text.to_stream())?;

        Ok(())
    }

    /// Write all the fonts.
    fn write_fonts(&mut self) -> io::Result<()> {
        let mut id = self.offsets.fonts.0;

        for &face_id in &self.to_fontdock {
            let loader = self.loader.borrow();
            let face = loader.get_loaded(face_id);

            let name = face
                .names()
                .find(|entry| {
                    entry.name_id() == name_id::POST_SCRIPT_NAME
                    && entry.is_unicode()
                })
                .map(|entry| entry.to_string())
                .flatten()
                .unwrap_or_else(|| "unknown".to_string());

            let base_font = format!("ABCDEF+{}", name);
            let system_info = CIDSystemInfo::new("Adobe", "Identity", 0);

            let units_per_em = face.units_per_em().unwrap_or(1000);
            let ratio = 1.0 / (units_per_em as f64);
            let to_length = |x| Length::pt(ratio * x as f64);
            let to_glyph_unit = |font_unit| {
                let length = to_length(font_unit);
                (1000.0 * length.to_pt()).round() as GlyphUnit
            };

            let global_bbox = face.global_bounding_box();
            let bbox = Rect::new(
                to_glyph_unit(global_bbox.x_min as f64),
                to_glyph_unit(global_bbox.y_min as f64),
                to_glyph_unit(global_bbox.x_max as f64),
                to_glyph_unit(global_bbox.y_max as f64),
            );

            let monospace = face.is_monospaced();
            let italic = face.is_italic();
            let italic_angle = face.italic_angle().unwrap_or(0.0);
            let ascender = face.typographic_ascender().unwrap_or(0);
            let descender = face.typographic_descender().unwrap_or(0);
            let cap_height = face.capital_height().unwrap_or(ascender);
            let stem_v = 10.0 + 0.244 * (face.weight().to_number() as f32 - 50.0);

            let mut flags = FontFlags::empty();
            flags.set(FontFlags::SERIF, name.contains("Serif"));
            flags.set(FontFlags::FIXED_PITCH, monospace);
            flags.set(FontFlags::ITALIC, italic);
            flags.insert(FontFlags::SYMBOLIC);
            flags.insert(FontFlags::SMALL_CAP);

            // Write the base font object referencing the CID font.
            self.writer.write_obj(
                id,
                Type0Font::new(
                    base_font.clone(),
                    CMapEncoding::Predefined("Identity-H".to_string()),
                    id + 1,
                )
                .to_unicode(id + 3),
            )?;

            let num_glyphs = face.number_of_glyphs();
            let widths: Vec<_> = (0 .. num_glyphs)
                .map(|g| face.glyph_hor_advance(GlyphId(g)).unwrap_or(0))
                .map(|w| to_glyph_unit(w as f64))
                .collect();

            // Write the CID font referencing the font descriptor.
            self.writer.write_obj(
                id + 1,
                CIDFont::new(
                    CIDFontType::Type2,
                    base_font.clone(),
                    system_info.clone(),
                    id + 2,
                )
                .widths(vec![WidthRecord::Start(0, widths)]),
            )?;

            // Write the font descriptor (contains the global information about
            // the font).
            self.writer.write_obj(id + 2,
                FontDescriptor::new(base_font, flags, italic_angle)
                    .font_bbox(bbox)
                    .ascent(to_glyph_unit(ascender as f64))
                    .descent(to_glyph_unit(descender as f64))
                    .cap_height(to_glyph_unit(cap_height as f64))
                    .stem_v(stem_v as GlyphUnit)
                    .font_file_2(id + 4)
            )?;

            let mut mapping = vec![];
            for subtable in face.character_mapping_subtables() {
                subtable.codepoints(|n| {
                    if let Some(c) = std::char::from_u32(n) {
                        if let Some(g) = face.glyph_index(c) {
                            mapping.push((g.0, c));
                        }
                    }
                })
            }

            // Write the CMap, which maps glyph ID's to unicode codepoints.
            self.writer.write_obj(id + 3, &CMap::new(
                "Custom",
                system_info,
                mapping,
            ))?;

            // Finally write the subsetted font bytes.
            self.writer.write_obj(id + 4, &FontStream::new(face.data()))?;

            id += NUM_OBJECTS_PER_FONT;
        }

        Ok(())
    }
}

/// Assigns a new PDF-internal index to each used face and returns two mappings:
/// - Forwards from the old face ids to the new pdf indices (hash map)
/// - Backwards from the pdf indices to the old ids (vec)
fn remap_fonts(layouts: &MultiLayout) -> (HashMap<FaceId, usize>, Vec<FaceId>) {
    let mut to_pdf = HashMap::new();
    let mut to_fontdock = vec![];

    // We want to find out which fonts are used at all. To do that, look at each
    // text element to find out which font is uses.
    for layout in layouts {
        for action in &layout.actions {
            if let &LayoutAction::SetFont(id, _) = action {
                to_pdf.entry(id).or_insert_with(|| {
                    let next_id = to_fontdock.len();
                    to_fontdock.push(id);
                    next_id
                });
            }
        }
    }

    (to_pdf, to_fontdock)
}

/// We need to know in advance which IDs to use for which objects to
/// cross-reference them. Therefore, we calculate the indices in the beginning.
fn calculate_offsets(layout_count: usize, font_count: usize) -> Offsets {
    let catalog = 1;
    let page_tree = catalog + 1;
    let pages = (page_tree + 1, page_tree + layout_count as Ref);
    let contents = (pages.1 + 1, pages.1 + layout_count as Ref);
    let font_offsets = (contents.1 + 1, contents.1 + 5 * font_count as Ref);

    Offsets {
        catalog,
        page_tree,
        pages,
        contents,
        fonts: font_offsets,
    }
}

/// Create an iterator from a reference pair.
fn ids((start, end): (Ref, Ref)) -> impl Iterator<Item=Ref> {
    start ..= end
}