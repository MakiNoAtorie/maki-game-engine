//! The texture asset, and the store that loads each image file one time.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use bevy_ecs::resource::Resource;

use crate::{Assets, Handle, Mesh};

/// An image for a material: 8 bits each for red, green, blue, and alpha.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// For the log: the file, or the model and the number of the image in it.
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Four bytes for each pixel, row by row from the top. Alpha is not multiplied
    /// into the color.
    pub pixels: Vec<u8>,
    /// True: the color is sRGB-encoded, as in each normal image file. False: the
    /// bytes are numbers (no texture of this kind exists yet).
    pub srgb: bool,
}

impl Texture {
    /// A color texture. `pixels` as in the field of that name.
    pub fn new(name: impl Into<String>, width: u32, height: u32, pixels: Vec<u8>) -> Result<Self> {
        let name = name.into();
        ensure!(width > 0 && height > 0, "texture `{name}` has no area");
        ensure!(
            pixels.len() as u64 == u64::from(width) * u64::from(height) * 4,
            "texture `{name}` of {width} x {height} has {} bytes",
            pixels.len()
        );
        Ok(Self {
            name,
            width,
            height,
            pixels,
            srgb: true,
        })
    }
}

/// A texture of a material, and how it is filtered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureRef {
    pub texture: Handle<Texture>,
    /// True: a texel that shows larger than a pixel keeps its hard edges (pixel art).
    /// False: texels blend.
    pub nearest: bool,
}

/// Where a texture came from: an image file, or image number `1` of a model file.
pub(crate) type Source = (PathBuf, Option<usize>);

/// Reads and decodes an image file (PNG or JPEG). It uses no store: for any thread.
pub(crate) fn decode_file(path: &Path) -> Result<Texture> {
    let image = image::open(path)?.into_rgba8();
    let (width, height) = image.dimensions();
    Texture::new(path.display().to_string(), width, height, image.into_raw())
}

/// The check of an image file for the watch (`AssetWatch`): it decodes.
pub fn check_image(path: &Path) -> Result<()> {
    decode_file(path).map(|_| ())
}

/// All textures. A source that is asked for two times gives the same handle: an
/// image that two materials or two models use is in memory one time.
#[derive(Resource, Default)]
pub struct Textures {
    store: Assets<Texture>,
    sources: HashMap<Source, Handle<Texture>>,
}

impl Textures {
    /// The texture of an image file (PNG or JPEG). A failure is logged, and the
    /// texture never becomes ready: the material then shows its color alone.
    // ponytail: the file is read and decoded on the calling thread. The images of a
    // model file are decoded with the model, on its thread (`ModelLoads`). For this
    // one, the renderer must write the table entry of a mesh again when its texture
    // arrives (`GpuMeshes::sync`).
    pub fn load(&mut self, path: &Path) -> Handle<Texture> {
        self.load_with((path.to_owned(), None), || decode_file(path))
    }

    /// The texture of `source`, made with `decode` if the store does not have it.
    pub(crate) fn load_with(
        &mut self,
        source: Source,
        decode: impl FnOnce() -> Result<Texture>,
    ) -> Handle<Texture> {
        if let Some(handle) = self.sources.get(&source) {
            return *handle;
        }
        let handle = self.store.reserve();
        let texture = decode().with_context(|| match &source {
            (path, None) => format!("cannot load texture {}", path.display()),
            (path, Some(image)) => format!("cannot load image {image} of {}", path.display()),
        });
        let texture = texture
            .inspect_err(|error| tracing::error!("{error:#}"))
            .ok();
        self.store.finish(handle, texture);
        self.sources.insert(source, handle);
        handle
    }

    /// The files that the store loaded, for the watch, each with its check: an
    /// image file, and a model file for an image in it.
    pub fn files(&self) -> Vec<(PathBuf, crate::Check)> {
        let check = |image: &Option<usize>| -> crate::Check {
            match image {
                None => check_image,
                Some(_) => crate::check_model,
            }
        };
        let sources = self.sources.keys();
        sources
            .map(|(path, image)| (path.clone(), check(image)))
            .collect()
    }

    /// Forgets a file that changed on the disk: the next load of it reads it again
    /// and gives a new handle. The texture of the old handle stays until no mesh
    /// uses it (`remove_unused`).
    pub fn forget(&mut self, path: &Path) {
        self.sources.retain(|(file, _), _| file != path);
    }

    /// Stores a texture that code made.
    pub fn add(&mut self, texture: Texture) -> Handle<Texture> {
        self.store.add(texture)
    }

    /// `None` after a failed load, and after the texture was removed.
    pub fn get(&self, handle: Handle<Texture>) -> Option<&Texture> {
        self.store.get(handle)
    }

    /// All textures that are loaded.
    pub fn iter(&self) -> impl Iterator<Item = (Handle<Texture>, &Texture)> {
        self.store.iter()
    }

    /// Removes each texture that no mesh of the store uses. Returns how many. The
    /// room loader calls this after a room change: the textures go with the meshes
    /// of the old room.
    pub fn remove_unused(&mut self, meshes: &Assets<Mesh>) -> usize {
        let used: HashSet<Handle<Texture>> = meshes
            .iter()
            .flat_map(|(_, mesh)| [mesh.base_color_texture(), mesh.emissive_texture()])
            .flatten()
            .map(|texture| texture.texture)
            .collect();
        let mut removed = 0;
        // Also a failed load: the next room that names the file tries again.
        let store = &mut self.store;
        self.sources.retain(|_, handle| {
            let keep = used.contains(handle);
            if !keep {
                store.remove(*handle);
                removed += 1;
            }
            keep
        });
        // The textures that code made have no source.
        let ready: Vec<_> = store.iter().map(|(handle, _)| handle).collect();
        for handle in ready.into_iter().filter(|handle| !used.contains(handle)) {
            store.remove(handle);
            removed += 1;
        }
        removed
    }
}
