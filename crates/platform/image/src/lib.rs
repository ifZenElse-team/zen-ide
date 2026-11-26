pub mod icon;

use std::path::Path;

const SVG_MAX_SIDE: f32 = 4096.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub format: &'static str,
    pub bits_per_pixel: Option<u16>,
    pub vector_size: Option<(f32, f32)>,
}

#[derive(Debug)]
pub struct ImageError(String);

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImageError {}

pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg"];

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
}

pub fn is_svg(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

pub fn opens_as_picture(path: &Path) -> bool {
    is_image(path) && !is_svg(path)
}

pub fn decode(path: &Path, bytes: &[u8]) -> Result<Picture, ImageError> {
    if is_svg(path) { decode_svg(bytes) } else { decode_raster(bytes) }
}

fn decode_raster(bytes: &[u8]) -> Result<Picture, ImageError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| ImageError(error.to_string()))?;
    let format = match reader.format() {
        Some(image::ImageFormat::Png) => "PNG",
        Some(image::ImageFormat::Jpeg) => "JPEG",
        Some(image::ImageFormat::Gif) => "GIF",
        Some(image::ImageFormat::WebP) => "WebP",
        Some(image::ImageFormat::Bmp) => "BMP",
        Some(image::ImageFormat::Ico) => "ICO",
        _ => return Err(ImageError("the image format is not supported".to_owned())),
    };
    let decoded = reader.decode().map_err(|error| ImageError(error.to_string()))?;
    let bits_per_pixel = Some(decoded.color().bits_per_pixel());
    let rgba = decoded.into_rgba8();
    let (width, height) = (rgba.width() as usize, rgba.height() as usize);
    Ok(Picture { width, height, rgba: rgba.into_raw(), format, bits_per_pixel, vector_size: None })
}

pub fn decode_svg(bytes: &[u8]) -> Result<Picture, ImageError> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
        .map_err(|error| ImageError(error.to_string()))?;
    let size = tree.size();
    let scale = (4.0f32).min(SVG_MAX_SIDE / size.width().max(size.height()));
    render_tree(&tree, scale)
}

pub fn render_svg(bytes: &[u8], scale: f32) -> Result<Picture, ImageError> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
        .map_err(|error| ImageError(error.to_string()))?;
    let size = tree.size();
    render_tree(&tree, scale.min(SVG_MAX_SIDE / size.width().max(size.height())))
}

#[derive(Clone)]
pub struct SvgFonts {
    faces: std::sync::Arc<resvg::usvg::fontdb::Database>,
    family: String,
}

impl SvgFonts {
    pub fn new(faces: &[&[u8]], family: &str) -> Self {
        let mut database = resvg::usvg::fontdb::Database::new();
        for face in faces {
            database.load_font_data(face.to_vec());
        }
        Self { faces: std::sync::Arc::new(database), family: family.to_owned() }
    }

    pub fn family(&self) -> &str {
        &self.family
    }
}

pub fn render_svg_with_fonts(bytes: &[u8], scale: f32, fonts: &SvgFonts) -> Result<Picture, ImageError> {
    let options = resvg::usvg::Options {
        font_family: fonts.family.clone(),
        fontdb: std::sync::Arc::clone(&fonts.faces),
        ..resvg::usvg::Options::default()
    };
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|error| ImageError(error.to_string()))?;
    let size = tree.size();
    render_tree(&tree, scale.min(SVG_MAX_SIDE / size.width().max(size.height())))
}

fn render_tree(tree: &resvg::usvg::Tree, scale: f32) -> Result<Picture, ImageError> {
    let size = tree.size();
    let (width, height) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width.max(1), height.max(1))
        .ok_or_else(|| ImageError("the SVG has no size".to_owned()))?;
    resvg::render(tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let rgba = pixmap.pixels().iter().flat_map(|pixel| {
        let pixel = pixel.demultiply();
        [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
    });
    Ok(Picture {
        width: pixmap.width() as usize,
        height: pixmap.height() as usize,
        rgba: rgba.collect(),
        format: "SVG",
        bits_per_pixel: None,
        vector_size: Some((size.width(), size.height())),
    })
}

impl Picture {
    pub fn natural_size(&self) -> (f32, f32) {
        self.vector_size.unwrap_or((self.width as f32, self.height as f32))
    }

    pub fn shrunk_to(&self, max_side: usize) -> Picture {
        let factor = self.width.max(self.height).div_ceil(max_side.max(1)).max(1);
        if factor == 1 {
            return self.clone();
        }
        let (width, height) = (self.width.div_ceil(factor), self.height.div_ceil(factor));
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0u32; 4];
                let mut count = 0;
                for sy in y * factor..((y + 1) * factor).min(self.height) {
                    for sx in x * factor..((x + 1) * factor).min(self.width) {
                        let at = (sy * self.width + sx) * 4;
                        for (channel, total) in sum.iter_mut().enumerate() {
                            *total += u32::from(self.rgba[at + channel]);
                        }
                        count += 1;
                    }
                }
                rgba.extend(sum.iter().map(|total| (total / count.max(1)) as u8));
            }
        }
        Picture { width, height, rgba, ..self.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let pixels = image::RgbaImage::from_fn(width, height, |x, _| image::Rgba([x as u8, 0, 0, 255]));
        pixels.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        bytes
    }

    #[test]
    fn decodes_png() {
        let picture = decode(Path::new("a.png"), &png(3, 2)).unwrap();
        assert_eq!((picture.width, picture.height, picture.format), (3, 2, "PNG"));
        assert_eq!(picture.rgba.len(), 3 * 2 * 4);
        assert_eq!(picture.bits_per_pixel, Some(32));
        assert_eq!(&picture.rgba[4..8], &[1, 0, 0, 255]);
    }

    #[test]
    fn decodes_svg_at_a_larger_scale() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8"><rect width="16" height="8" fill="red"/></svg>"#;
        let picture = decode(Path::new("icon.SVG"), svg).unwrap();
        assert_eq!((picture.width, picture.height, picture.format), (64, 32, "SVG"));
        assert_eq!(picture.natural_size(), (16.0, 8.0));
        assert_eq!(&picture.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn rejects_what_is_not_an_image() {
        assert!(decode(Path::new("a.png"), b"hello").is_err());
        assert!(decode(Path::new("a.svg"), b"hello").is_err());
    }

    #[test]
    fn knows_image_files() {
        assert!(is_image(Path::new("/p/Logo.PNG")));
        assert!(is_image(Path::new("icon.svg")));
        assert!(!is_image(Path::new("main.go")));
        assert!(!is_image(Path::new("png")));
        assert!(
            opens_as_picture(Path::new("a.PNG"))
                && !opens_as_picture(Path::new("icon.SVG"))
                && is_svg(Path::new("x.svg"))
        );
    }

    #[test]
    fn shrinks_by_averaging() {
        let picture = Picture {
            width: 4,
            height: 2,
            rgba: [[0, 0, 0, 255], [100, 0, 0, 255], [0, 0, 0, 255], [0, 0, 0, 255]].repeat(2).concat(),
            format: "PNG",
            bits_per_pixel: Some(32),
            vector_size: None,
        };
        let small = picture.shrunk_to(2);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(&small.rgba[..4], &[50, 0, 0, 255]);
        assert_eq!(picture.shrunk_to(8), picture);
    }
}
