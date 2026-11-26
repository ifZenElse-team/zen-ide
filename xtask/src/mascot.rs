use std::path::Path;

use zen_mascot::{FRAME_MS, FRAMES, SIZE, STILL, State, Variant};

const FOLDER: &str = "deploy/releases/site/img/kozo";

const ON_THE_SITE: [Variant; 6] =
    [Variant::Zen, Variant::GoGarden, Variant::RustRonin, Variant::WebWave, Variant::JvmJizo, Variant::Toolbox];

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    if let [flag, id, folder] = args
        && flag == "--sheet"
    {
        return sheet(id, Path::new(folder));
    }
    if let Some(unknown) = args.first() {
        return Err(format!("usage: cargo xtask mascot [--sheet <variant> <folder>], got `{unknown}`"));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("no workspace root")?;
    let folder = root.join(FOLDER);
    std::fs::create_dir_all(&folder).map_err(|error| format!("cannot create {}: {error}", folder.display()))?;
    for (name, bytes) in pictures()? {
        let path = folder.join(&name);
        std::fs::write(&path, &bytes).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        println!("{FOLDER}/{name} ({} bytes)", bytes.len());
    }
    Ok(())
}

fn pictures() -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut pictures = Vec::new();
    for variant in ON_THE_SITE {
        let frames: Vec<Vec<u8>> = (0..FRAMES).map(|t| rgba(variant, t)).collect();
        pictures.push((format!("{}.png", variant.id()), png(&frames)?));
        pictures.push((format!("{}-still.png", variant.id()), png(&frames[STILL..=STILL])?));
    }
    Ok(pictures)
}

fn rgba(variant: Variant, t: usize) -> Vec<u8> {
    zen_mascot::frame(variant, State::Idle, t)
        .iter()
        .flatten()
        .flat_map(|ink| ink.rgb(variant).map_or([0; 4], |[r, g, b]| [r, g, b, 0xFF]))
        .collect()
}

fn sheet(id: &str, folder: &Path) -> Result<(), String> {
    let variant = Variant::of(id);
    if variant.id() != id {
        let known: Vec<&str> = Variant::ALL.iter().map(|variant| variant.id()).collect();
        return Err(format!("unknown variant `{id}`; the variants are {}", known.join(", ")));
    }
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    const GAP: usize = 12;
    let strip = FRAMES * (SIZE * 4 + GAP) + GAP;
    let width = strip.max(GAP + (SIZE + GAP) + (SIZE * 2 + GAP) + (SIZE * 4 + GAP));
    let row = SIZE * 4 + GAP;
    let height = GAP + State::ALL.len() * 2 * row;
    for (name, background) in [("light", [0xF2, 0xF0, 0xEC]), ("dark", [0x2B, 0x2D, 0x30])] {
        let mut canvas: Vec<u8> =
            std::iter::repeat_n([background[0], background[1], background[2], 0xFF], width * height)
                .flatten()
                .collect();
        let mut paint = |grid: &zen_mascot::Grid, scale: usize, left: usize, top: usize| {
            for (y, line) in grid.iter().enumerate() {
                for (x, ink) in line.iter().enumerate() {
                    let Some([r, g, b]) = ink.rgb(variant) else { continue };
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let at = ((top + y * scale + dy) * width + left + x * scale + dx) * 4;
                            canvas[at..at + 4].copy_from_slice(&[r, g, b, 0xFF]);
                        }
                    }
                }
            }
        };
        for (index, state) in State::ALL.into_iter().enumerate() {
            let top = GAP + index * 2 * row;
            let still = zen_mascot::frame(variant, state, STILL);
            let mut left = GAP;
            for scale in [1, 2, 4] {
                paint(&still, scale, left, top);
                left += SIZE * scale + GAP;
            }
            for t in 0..FRAMES {
                paint(&zen_mascot::frame(variant, state, t), 4, GAP + t * (SIZE * 4 + GAP), top + row);
            }
        }
        let path = folder.join(format!("{id}-sheet-{name}.png"));
        let error = |error: png::EncodingError| format!("cannot encode the sheet: {error}");
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(error)?;
            writer.write_image_data(&canvas).map_err(error)?;
            writer.finish().map_err(error)?;
        }
        std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

fn png(frames: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let error = |error: png::EncodingError| format!("cannot encode a picture of Kozō: {error}");
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, SIZE as u32, SIZE as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::High);
        if frames.len() > 1 {
            encoder.set_animated(frames.len() as u32, 0).map_err(error)?;
            encoder.set_frame_delay(FRAME_MS as u16, 1000).map_err(error)?;
            encoder.set_blend_op(png::BlendOp::Source).map_err(error)?;
            encoder.set_dispose_op(png::DisposeOp::None).map_err(error)?;
        }
        let mut writer = encoder.write_header().map_err(error)?;
        for frame in frames {
            writer.write_image_data(frame).map_err(error)?;
        }
        writer.finish().map_err(error)?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_pictures_are_the_ones_the_grids_give() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        for (name, bytes) in pictures().unwrap() {
            let committed = std::fs::read(root.join(FOLDER).join(&name)).unwrap_or_default();
            assert!(committed == bytes, "{FOLDER}/{name} is out of date: run `cargo xtask mascot`");
        }
    }

    #[test]
    fn an_animation_has_eight_frames_and_a_still_picture_one() {
        let pictures = pictures().unwrap();
        let animated = &pictures.iter().find(|(name, _)| name == "zen.png").unwrap().1;
        let still = &pictures.iter().find(|(name, _)| name == "zen-still.png").unwrap().1;
        let decoder = |bytes: &[u8]| png::Decoder::new(std::io::Cursor::new(bytes.to_vec())).read_info().unwrap();
        let reader = decoder(animated);
        let control = reader.info().animation_control().expect("an animated PNG");
        assert_eq!((control.num_frames, control.num_plays), (8, 0), "eight frames, looping for ever");
        assert_eq!((reader.info().width, reader.info().height), (40, 40));
        assert!(decoder(still).info().animation_control().is_none());
    }
}
