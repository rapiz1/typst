use std::io;

use super::prelude::*;
use crate::diag::Error;
use crate::image::ImageId;

/// `image`: An image.
pub fn image(ctx: &mut EvalContext, args: &mut Args) -> TypResult<Value> {
    let path = args.expect::<Spanned<EcoString>>("path to image file")?;
    let width = args.named("width")?;
    let height = args.named("height")?;
    let fit = args.named("fit")?.unwrap_or_default();

    // Load the image.
    let full = ctx.make_path(&path.v);
    let id = ctx.images.load(&full).map_err(|err| {
        Error::boxed(path.span, match err.kind() {
            io::ErrorKind::NotFound => "file not found".into(),
            _ => format!("failed to load image ({})", err),
        })
    })?;

    Ok(Value::Template(Template::from_inline(move |_| {
        ImageNode { id, fit }.pack().sized(Spec::new(width, height))
    })))
}

/// An image node.
#[derive(Debug, Hash)]
pub struct ImageNode {
    /// The id of the image file.
    pub id: ImageId,
    /// How the image should adjust itself to a given area.
    pub fit: ImageFit,
}

impl Layout for ImageNode {
    fn layout(
        &self,
        ctx: &mut LayoutContext,
        regions: &Regions,
    ) -> Vec<Constrained<Rc<Frame>>> {
        let &Regions { current, expand, .. } = regions;

        let img = ctx.images.get(self.id);
        let pxw = img.width() as f64;
        let pxh = img.height() as f64;

        let pixel_ratio = pxw / pxh;
        let current_ratio = current.w / current.h;
        let wide = pixel_ratio > current_ratio;

        // The space into which the image will be placed according to its fit.
        let canvas = if expand.x && expand.y {
            current
        } else if expand.x || (wide && current.w.is_finite()) {
            Size::new(current.w, current.h.min(current.w.safe_div(pixel_ratio)))
        } else if current.h.is_finite() {
            Size::new(current.w.min(current.h * pixel_ratio), current.h)
        } else {
            Size::new(Length::pt(pxw), Length::pt(pxh))
        };

        // The actual size of the fitted image.
        let size = match self.fit {
            ImageFit::Contain | ImageFit::Cover => {
                if wide == (self.fit == ImageFit::Contain) {
                    Size::new(canvas.w, canvas.w / pixel_ratio)
                } else {
                    Size::new(canvas.h * pixel_ratio, canvas.h)
                }
            }
            ImageFit::Stretch => canvas,
        };

        // First, place the image in a frame of exactly its size and then resize
        // the frame to the canvas size, center aligning the image in the
        // process.
        let mut frame = Frame::new(size);
        frame.push(Point::zero(), Element::Image(self.id, size));
        frame.resize(canvas, Spec::new(Align::Center, Align::Horizon));

        // Create a clipping group if the fit mode is "cover".
        if self.fit == ImageFit::Cover {
            frame.clip();
        }

        vec![frame.constrain(Constraints::tight(regions))]
    }
}

/// How an image should adjust itself to a given area.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ImageFit {
    /// The image should be fully contained in the area.
    Contain,
    /// The image should completely cover the area.
    Cover,
    /// The image should be stretched so that it exactly fills the area.
    Stretch,
}

castable! {
    ImageFit,
    Expected: "string",
    Value::Str(string) => match string.as_str() {
        "contain" => Self::Contain,
        "cover" => Self::Cover,
        "stretch" => Self::Stretch,
        _ => Err(r#"expected "contain", "cover" or "stretch""#)?,
    },
}

impl Default for ImageFit {
    fn default() -> Self {
        Self::Contain
    }
}