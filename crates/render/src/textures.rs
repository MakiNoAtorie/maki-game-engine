//! Textures on the GPU: one sampled image with mip levels for each texture asset, and
//! the two samplers that a material can name.
//!
//! The mesh table names a texture by one number (`slot`): the bindless index of the
//! image and that of the sampler. `shaders/scene.glsl` reads it.

use std::collections::HashMap;

use anyhow::Result;
use maki_assets::{Handle, Texture, TextureRef, Textures};
use maki_gpu::{Format, Gpu, ImageDesc, ImageHandle, SamplerDesc, SamplerHandle, mip_level_count};

/// The slot of a material with no texture: `NO_TEXTURE` in `scene.glsl`.
pub(crate) const NO_TEXTURE: u32 = u32::MAX;

/// All textures that are on the GPU.
pub(crate) struct GpuTextures {
    images: HashMap<Handle<Texture>, ImageHandle>,
    /// Blend between texels, and hard texels (pixel art). The two blend between mip levels.
    linear: SamplerHandle,
    nearest: SamplerHandle,
}

impl GpuTextures {
    pub fn new(gpu: &mut Gpu) -> Result<Self> {
        let mut sampler = |linear| {
            gpu.create_sampler(&SamplerDesc {
                linear,
                repeat: true,
                mipmaps: true,
            })
        };
        Ok(Self {
            images: HashMap::new(),
            linear: sampler(true)?,
            nearest: sampler(false)?,
        })
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Makes the GPU images agree with the store: uploads new textures and frees
    /// those that were removed. `destroy_image` waits until no frame in flight can
    /// sample the image.
    pub fn sync(&mut self, gpu: &mut Gpu, textures: &Textures) {
        self.images.retain(|handle, image| {
            let keep = textures.get(*handle).is_some();
            if !keep {
                gpu.destroy_image(*image);
            }
            keep
        });
        for (handle, texture) in textures.iter() {
            if self.images.contains_key(&handle) {
                continue;
            }
            match upload(gpu, texture) {
                Ok(image) => {
                    tracing::info!(
                        name = texture.name,
                        width = texture.width,
                        height = texture.height,
                        "texture uploaded"
                    );
                    self.images.insert(handle, image);
                }
                // Tried again when the store changes the next time. The material
                // shows its color alone.
                Err(error) => {
                    tracing::error!("cannot upload texture `{}`: {error:#}", texture.name)
                }
            }
        }
    }

    /// The number that the mesh table has for a texture of a material: the image
    /// index in the low 16 bits, the sampler index above. [`NO_TEXTURE`] for no
    /// texture and for one that is not on the GPU (a failed load).
    pub fn slot(&self, gpu: &Gpu, texture: Option<TextureRef>) -> u32 {
        let slot = texture.and_then(|texture| {
            let image = gpu.image_index(*self.images.get(&texture.texture)?)?;
            let sampler = if texture.nearest {
                self.nearest
            } else {
                self.linear
            };
            Some(image | gpu.sampler_index(sampler)? << 16)
        });
        slot.unwrap_or(NO_TEXTURE)
    }
}

fn upload(gpu: &mut Gpu, texture: &Texture) -> Result<ImageHandle> {
    let levels = mip_chain(texture);
    debug_assert_eq!(
        levels.len() as u32,
        mip_level_count(texture.width, texture.height)
    );
    let image = gpu.create_image(&ImageDesc {
        name: &texture.name,
        width: texture.width,
        height: texture.height,
        mip_levels: levels.len() as u32,
        // sRGB: the GPU gives the shader linear color, and filters in linear color.
        format: if texture.srgb {
            Format::Rgba8Srgb
        } else {
            Format::Rgba8Unorm
        },
        storage: false,
        sampled: true,
        color_attachment: false,
    })?;
    let levels: Vec<&[u8]> = levels.iter().map(Vec::as_slice).collect();
    gpu.upload_image(image, &levels)
        .inspect_err(|_| gpu.destroy_image(image))?;
    Ok(image)
}

/// The pixels of each mip level, from the texture itself down to 1 x 1. A pixel of a
/// level is the mean of the (up to) four pixels of the level before that it covers.
///
/// The mean of colors is taken of the light, not of the sRGB numbers: a checker of
/// black and white is, from far away, a gray with half of the light (sRGB 188), not
/// the number in the middle (128), which is darker.
// ponytail: a box filter on the CPU at each upload. Enough for small, stylized
// textures. Store the levels in a cooked file when large textures make a room slow
// to load.
fn mip_chain(texture: &Texture) -> Vec<Vec<u8>> {
    // sRGB number to light, 0 to 1.
    let to_linear: Vec<f32> = (0..256)
        .map(|value| {
            let c = value as f32 / 255.0;
            if !texture.srgb {
                c
            } else if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
        .collect();
    let to_stored = |light: f32| {
        let c = if !texture.srgb {
            light
        } else if light <= 0.003_130_8 {
            light * 12.92
        } else {
            1.055 * light.powf(1.0 / 2.4) - 0.055
        };
        (c * 255.0).round() as u8
    };

    let mut levels = vec![texture.pixels.clone()];
    let (mut width, mut height) = (texture.width as usize, texture.height as usize);
    while width > 1 || height > 1 {
        let (next_width, next_height) = ((width / 2).max(1), (height / 2).max(1));
        let previous = levels.last().expect("the first level is there");
        let mut next = Vec::with_capacity(next_width * next_height * 4);
        for y in 0..next_height {
            for x in 0..next_width {
                // An odd last row or column is left out; a side of 1 pixel is used twice.
                let corners = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| {
                    let (x, y) = ((2 * x + dx).min(width - 1), (2 * y + dy).min(height - 1));
                    &previous[(y * width + x) * 4..][..4]
                });
                for channel in 0..3 {
                    let light: f32 = corners.iter().map(|c| to_linear[c[channel] as usize]).sum();
                    next.push(to_stored(light / 4.0));
                }
                // Alpha is a plain number.
                let alpha: u32 = corners.iter().map(|c| u32::from(c[3])).sum();
                next.push(((alpha + 2) / 4) as u8);
            }
        }
        levels.push(next);
        (width, height) = (next_width, next_height);
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mip_level_is_the_mean_of_the_light_of_four_pixels() {
        // 4 x 2: a black and white checker at the left, red with two alphas at the right.
        let (black, white) = ([0, 0, 0, 255], [255, 255, 255, 255]);
        let (red, clear) = ([255, 0, 0, 255], [255, 0, 0, 0]);
        let pixels = [black, white, red, clear, white, black, red, clear].concat();
        let texture = Texture::new("test", 4, 2, pixels).unwrap();
        let levels = mip_chain(&texture);
        assert_eq!(levels.len(), 3);
        assert_eq!(mip_level_count(4, 2), 3);
        // Half of the light is sRGB 188. The color of the red pixels stays; half of
        // them are there.
        assert_eq!(levels[1], [188, 188, 188, 255, 255, 0, 0, 128]);
        assert_eq!(levels[2].len(), 4);
        // A texture of one pixel has one level.
        let one = Texture::new("one", 1, 1, vec![9, 9, 9, 9]).unwrap();
        assert_eq!(mip_chain(&one), [vec![9, 9, 9, 9]]);
    }
}
