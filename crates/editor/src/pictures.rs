//! Small pictures of image files and model files, for the file pickers.
//!
//! A picture is made one time for each file, on the CPU, and given to egui as a
//! texture (`Context::load_texture`). The picture of a model is its triangles, drawn
//! from one fixed direction: no pass of the renderer draws one model into an image.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use glam::{Vec2, Vec3};
use maki_assets::{Assets, Mesh, Model, Textures, load_gltf};
use maki_ui::egui::{self, Color32, ColorImage, Rgba, TextureHandle, TextureOptions, Ui};

use crate::widgets::{MODELS, PICTURES};

/// The longer side of a picture, in pixels.
const SIDE: usize = 96;
/// The side of a picture in a list, in points: half of `SIDE`, so it is sharp up to
/// a display scale of 200 %.
const SHOWN: f32 = 48.0;

/// The pictures that were made. In the memory of egui: the widgets get a `Ui` only.
#[derive(Clone, Default)]
struct Pictures(Arc<Mutex<Made>>);

#[derive(Default)]
struct Made {
    /// For each file: the time of its last change, and its picture. `None`: the
    /// file cannot be read (the loader logged the reason).
    files: HashMap<PathBuf, (SystemTime, Option<TextureHandle>)>,
    /// The egui frame that made the last picture.
    frame: u64,
}

/// The picture of an image file or a model file. `None`: a file of another kind, a
/// file that cannot be read, or a picture that a later frame makes.
// ponytail: a picture is made on the thread of the editor, one in each frame. On a
// thread when a model takes longer than a frame that the user accepts.
fn picture(ui: &Ui, path: &Path) -> Option<TextureHandle> {
    let extension = path.extension()?.to_str()?.to_lowercase();
    let model = MODELS.contains(&extension.as_str());
    if !model && !PICTURES.contains(&extension.as_str()) {
        return None;
    }
    // No file: no picture, and no try in each frame.
    let changed = std::fs::metadata(path)
        .and_then(|file| file.modified())
        .ok()?;
    let pictures: Pictures = ui.data_mut(|data| {
        data.get_temp_mut_or_default::<Pictures>(egui::Id::NULL)
            .clone()
    });
    let mut made = pictures.0.lock().expect("no holder panics");
    if let Some((time, picture)) = made.files.get(path)
        && *time == changed
    {
        return picture.clone();
    }
    let frame = ui.ctx().cumulative_frame_nr();
    if made.frame == frame {
        return None;
    }
    made.frame = frame;
    // The pictures of the files that are gone are freed here.
    made.files.retain(|path, _| path.exists());

    let start = std::time::Instant::now();
    let image = if model {
        of_model(path)
    } else {
        of_image(path)
    };
    let name = path.display().to_string();
    let picture = image.map(|image| ui.ctx().load_texture(name, image, TextureOptions::LINEAR));
    tracing::debug!(path = %path.display(), time = ?start.elapsed(), "picture made");
    made.files
        .insert(path.to_owned(), (changed, picture.clone()));
    picture
}

/// Shows the picture of a file, or an empty place of the same size: the names of a
/// list stay in one column.
pub(crate) fn show(ui: &mut Ui, path: &Path) {
    let size = egui::Vec2::splat(SHOWN);
    match picture(ui, path) {
        Some(picture) => {
            ui.add(egui::Image::new(&picture).fit_to_exact_size(size));
        }
        None => {
            ui.allocate_space(size);
        }
    }
}

/// True for a file that has a picture: a list shows a picture beside each name.
pub(crate) fn has_picture(extensions: &[&str]) -> bool {
    let known = |extension: &&str| MODELS.contains(extension) || PICTURES.contains(extension);
    extensions.iter().any(known)
}

/// An image file, made small. The sides keep their ratio.
// ponytail: the nearest texel. Pixel art keeps its hard edges; a photo shows noise.
// The mean of the texels of a pixel when a game has photos.
fn of_image(path: &Path) -> Option<ColorImage> {
    let mut textures = Textures::default();
    let handle = textures.load(path);
    let texture = textures.get(handle)?;
    let (width, height) = (texture.width as usize, texture.height as usize);
    let side = |length: usize| (length * SIDE / width.max(height)).max(1);
    let size = [side(width), side(height)];
    let mut pixels = Vec::with_capacity(size[0] * size[1] * 4);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let texel = (y * height / size[1]) * width + x * width / size[0];
            pixels.extend_from_slice(&texture.pixels[texel * 4..texel * 4 + 4]);
        }
    }
    Some(ColorImage::from_rgba_unmultiplied(size, &pixels))
}

/// A model file, drawn. The file is read into stores of this call: the stores of
/// the game do not get it.
fn of_model(path: &Path) -> Option<ColorImage> {
    let mut models = Assets::<Model>::default();
    let mut meshes = Assets::<Mesh>::default();
    let mut textures = Textures::default();
    let handle = load_gltf(path, &mut models, &mut meshes, &mut textures);
    let model = models.get(handle)?;
    let parts: Vec<&Mesh> = model
        .meshes
        .iter()
        .filter_map(|mesh| meshes.get(*mesh))
        .collect();
    Some(draw(&parts, &textures))
}

/// Two times the area of a triangle in the picture, with a sign.
fn edge(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    (b - a).perp_dot(c - a)
}

/// Draws meshes into a picture of `SIDE` x `SIDE` pixels: seen from the front, a
/// little from the right and from above, with no perspective. Each triangle has
/// one brightness, from its direction. Clear where no mesh is.
fn draw(meshes: &[&Mesh], textures: &Textures) -> ColorImage {
    // The axes of the picture, and the direction away from the eye.
    let forward = Vec3::new(-0.45, -0.5, -0.74).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    let light = (right * 0.35 + up * 0.6 - forward * 0.7).normalize();
    let view = |point: Vec3| Vec3::new(point.dot(right), point.dot(up), point.dot(forward));

    // The model fills the picture, with a margin of 2 pixels.
    let points = meshes.iter().flat_map(|mesh| mesh.positions());
    let (low, high) = points.fold((Vec3::MAX, Vec3::MIN), |(low, high), point| {
        (low.min(view(*point)), high.max(view(*point)))
    });
    let center = (low + high) / 2.0;
    let scale = (SIDE as f32 - 4.0) / (high - low).truncate().max_element().max(1e-6);
    let half = SIDE as f32 / 2.0;
    // Pixels in x and y (y down), the distance in z.
    let to_picture = |point: Vec3| {
        let at = (view(point) - center) * scale;
        Vec3::new(half + at.x, half - at.y, at.z)
    };

    let mut depths = vec![f32::MAX; SIDE * SIDE];
    let mut pixels = vec![Color32::TRANSPARENT; SIDE * SIDE];
    for mesh in meshes {
        let texture = mesh.base_color_texture();
        let texture = texture.and_then(|texture| textures.get(texture.texture));
        let texture = texture.filter(|_| !mesh.uvs().is_empty());
        let [r, g, b, alpha] = mesh.base_color();
        let base = Rgba::from_rgb(r, g, b);
        for triangle in mesh.indices().as_chunks::<3>().0 {
            let corner = |index: usize| mesh.positions()[triangle[index] as usize];
            let [a, b, c] = [0, 1, 2].map(|index| to_picture(corner(index)));
            let area = edge(a.truncate(), b.truncate(), c.truncate());
            if area.abs() < 1e-6 {
                continue;
            }
            let normal = (corner(1) - corner(0)).cross(corner(2) - corner(0));
            // The two sides of a triangle get the same light: a card of a plant.
            let shade = 0.3 + 0.7 * normal.normalize_or_zero().dot(light).abs();

            let low = a.min(b).min(c).max(Vec3::ZERO);
            let high = a.max(b).max(c).min(Vec3::splat(SIDE as f32 - 1.0));
            for y in low.y as usize..=high.y as usize {
                for x in low.x as usize..=high.x as usize {
                    let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    // The share of each corner at the point. All from 0 to 1: the
                    // point is in the triangle.
                    let shares = [
                        edge(b.truncate(), c.truncate(), point) / area,
                        edge(c.truncate(), a.truncate(), point) / area,
                        edge(a.truncate(), b.truncate(), point) / area,
                    ];
                    let distance = shares[0] * a.z + shares[1] * b.z + shares[2] * c.z;
                    let pixel = y * SIDE + x;
                    if shares.iter().any(|share| *share < 0.0) || distance >= depths[pixel] {
                        continue;
                    }
                    let (mut color, mut alpha) = (base, alpha);
                    if let Some(texture) = texture {
                        let uv = (0..3).fold(Vec2::ZERO, |uv, index| {
                            uv + Vec2::from(mesh.uvs()[triangle[index] as usize]) * shares[index]
                        });
                        // The texture repeats.
                        let size = Vec2::new(texture.width as f32, texture.height as f32);
                        let texel = ((uv - uv.floor()) * size).min(size - 1.0);
                        let texel = texel.y as usize * texture.width as usize + texel.x as usize;
                        let [r, g, b, a] = texture.pixels[texel * 4..texel * 4 + 4] else {
                            continue;
                        };
                        color = color * Rgba::from(Color32::from_rgb(r, g, b));
                        alpha *= f32::from(a) / 255.0;
                    }
                    // A hole of a texture (leaves, a fence).
                    if mesh.alpha_cutoff().is_some_and(|cutoff| alpha < cutoff) {
                        continue;
                    }
                    depths[pixel] = distance;
                    // Not `color * shade`: that changes the alpha too.
                    let [r, g, b, _] = color.to_array().map(|part| part * shade);
                    pixels[pixel] = Color32::from(Rgba::from_rgb(r, g, b));
                }
            }
        }
    }
    ColorImage::new([SIDE, SIDE], pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_drawn_with_its_color_and_with_a_brightness_for_each_side() {
        let cube = Mesh::cube(1.0).with_base_color([1.0, 0.0, 0.0, 1.0]);
        let image = draw(&[&cube], &Textures::default());
        assert_eq!(image.size, [SIDE, SIDE]);
        // Clear in a corner, the cube at the center.
        assert_eq!(image.pixels[0], Color32::TRANSPARENT);
        let center = image.pixels[SIDE / 2 * SIDE + SIDE / 2];
        assert!(center.r() > 60 && center.g() == 0 && center.a() == 255);
        // Three sides show, each with its brightness.
        let reds: std::collections::HashSet<u8> = image
            .pixels
            .iter()
            .filter(|pixel| pixel.a() == 255)
            .map(|pixel| pixel.r())
            .collect();
        assert_eq!(reds.len(), 3);
    }
}
