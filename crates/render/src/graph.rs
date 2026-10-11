//! The frame graph: the list of render passes of one frame and the images they use.
//!
//! A pass declares each image that it uses and how ([`Access`]). From the declarations
//! the graph does the work that is easy to get wrong by hand:
//! - it makes the transient images and makes them again when their size changes;
//! - before each pass, it records the barriers that move each image from its last use
//!   to the use that the pass declared.
//!
//! One frame ([`FrameGraph::execute`]):
//! 1. Size the transient images.
//! 2. Plan the barriers ([`plan_barriers`], no GPU call).
//! 3. For each pass in the order of the list: record its barriers, then run it.
//! 4. Move the backbuffer to [`Access::Present`].
//!
//! The graph also measures the GPU time of each pass, with a GPU timestamp before the
//! first pass and after each pass ([`FrameGraph::timings`]).

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use maki_gpu::{Access, FRAMES_IN_FLIGHT, Format, Gpu, ImageDesc, ImageHandle, ImageTarget};

/// Names an image of the graph. Valid for the graph that made it.
// A plain index: graph resources are never removed. The GPU image behind a transient
// changes on resize and is held by generational handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceId(usize);

/// The swapchain image of the current frame. Its old content is dropped each frame.
pub const BACKBUFFER: ResourceId = ResourceId(0);

/// Which size a transient image follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeMode {
    /// The internal render resolution (from the config).
    Internal,
    /// The size of the swapchain images (the window).
    Output,
}

/// An image that the graph makes and owns.
#[derive(Debug, Clone, Copy)]
pub struct TransientDesc {
    pub name: &'static str,
    pub format: Format,
    pub size: SizeMode,
}

/// One step of the frame.
pub trait Pass: Send + Sync + 'static {
    /// Unique in the graph. Shown in the barrier log.
    fn name(&self) -> &'static str;
    /// Each image that `execute` uses, one entry per image. Read one time, when the pass
    /// is added. An image that is not declared here is not in a known state.
    fn accesses(&self) -> Vec<(ResourceId, Access)>;
    /// Records the GPU commands. The barriers for the declared images are already recorded.
    fn execute(&mut self, ctx: &mut PassContext);
}

/// What a pass can reach during `execute`.
pub struct PassContext<'a> {
    pub gpu: &'a mut Gpu,
    /// The render world: extracted data only. A pass has no access to the game world.
    pub world: &'a World,
    resources: &'a [GraphResource],
}

impl PassContext<'_> {
    /// The GPU image of a transient. Can change between frames.
    /// Panics for [`BACKBUFFER`]: the gpu crate has its own calls for the swapchain image.
    pub fn image(&self, id: ResourceId) -> ImageHandle {
        self.transient(id).0
    }

    /// Size in pixels of a transient.
    pub fn extent(&self, id: ResourceId) -> (u32, u32) {
        self.transient(id).1
    }

    /// The index of a transient in the bindless image arrays, for a push constant.
    pub fn bindless_index(&self, id: ResourceId) -> u32 {
        self.gpu
            .image_index(self.image(id))
            .expect("the graph keeps its images alive")
    }

    fn transient(&self, id: ResourceId) -> (ImageHandle, (u32, u32)) {
        let resource = &self.resources[id.0];
        resource
            .image
            .unwrap_or_else(|| panic!("`{}` is not a transient image", resource.name))
    }
}

struct GraphResource {
    name: &'static str,
    /// `None` for the backbuffer.
    desc: Option<TransientDesc>,
    /// The GPU image and its size. `None` for the backbuffer and before the first frame.
    image: Option<(ImageHandle, (u32, u32))>,
    /// True: `image` was made for use as a color attachment.
    attachment: bool,
    /// The last use of the image. `None`: the content is undefined.
    // Kept from one frame to the next: the first barrier of a frame starts at the last
    // use of the frame before, so it also waits for that frame.
    state: Option<Access>,
}

struct PassNode {
    pass: Box<dyn Pass>,
    accesses: Vec<(ResourceId, Access)>,
}

/// A change of use of one image. `before_pass` is an index in the pass list, or the
/// number of passes for the barrier at the end of the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Barrier {
    before_pass: usize,
    resource: ResourceId,
    from: Option<Access>,
    to: Access,
}

/// The GPU time of each pass of one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PassTimings {
    /// The frame that was measured (the number given to [`FrameGraph::execute`]). The
    /// GPU finishes a frame later than the CPU records it, so this frame is some
    /// frames older than the current one.
    pub frame: u64,
    /// Name and GPU milliseconds of each pass, in execution order. The time of a pass
    /// has the barriers before it. A pass that skips its work shows near zero.
    pub passes: Vec<(&'static str, f32)>,
}

#[derive(Resource)]
pub struct FrameGraph {
    resources: Vec<GraphResource>,
    /// In execution order.
    passes: Vec<PassNode>,
    /// The barriers of the frame before, to log only a changed plan.
    last_plan: Vec<Barrier>,
    /// The frame that used each GPU frame slot last. Its timestamps are ready when
    /// the slot is used again.
    slot_frames: [Option<u64>; FRAMES_IN_FLIGHT],
    timings: Option<PassTimings>,
}

impl Default for FrameGraph {
    fn default() -> Self {
        Self {
            // Index 0 is `BACKBUFFER`.
            resources: vec![GraphResource {
                name: "backbuffer",
                desc: None,
                image: None,
                attachment: false,
                state: None,
            }],
            passes: Vec::new(),
            last_plan: Vec::new(),
            slot_frames: [None; FRAMES_IN_FLIGHT],
            timings: None,
        }
    }
}

impl FrameGraph {
    /// Declares an image that the graph makes. Panics if the name is in use.
    pub fn add_transient(&mut self, desc: TransientDesc) -> ResourceId {
        assert!(
            self.resource(desc.name).is_none(),
            "graph resource `{}` was added two times",
            desc.name
        );
        self.resources.push(GraphResource {
            name: desc.name,
            desc: Some(desc),
            image: None,
            attachment: false,
            state: None,
        });
        ResourceId(self.resources.len() - 1)
    }

    /// Finds a resource by name. A module uses this to reach an image of another module.
    pub fn resource(&self, name: &str) -> Option<ResourceId> {
        self.resources
            .iter()
            .position(|resource| resource.name == name)
            .map(ResourceId)
    }

    /// The description of a transient image. `None` for [`BACKBUFFER`].
    pub fn transient_desc(&self, id: ResourceId) -> Option<TransientDesc> {
        self.resources[id.0].desc
    }

    /// The pass timings of the newest frame that the GPU has finished.
    /// `None` in the first frames, and on a GPU without timestamps.
    pub fn timings(&self) -> Option<&PassTimings> {
        self.timings.as_ref()
    }

    /// Adds a pass at the end of the frame.
    pub fn add_pass(&mut self, pass: impl Pass) {
        self.insert_pass(self.passes.len(), Box::new(pass));
    }

    /// Adds a pass directly before the pass with the given name. Panics if no pass has it.
    pub fn add_pass_before(&mut self, before: &str, pass: impl Pass) {
        let index = self
            .passes
            .iter()
            .position(|node| node.pass.name() == before)
            .unwrap_or_else(|| panic!("the graph has no pass `{before}`"));
        self.insert_pass(index, Box::new(pass));
    }

    fn insert_pass(&mut self, index: usize, pass: Box<dyn Pass>) {
        let name = pass.name();
        assert!(
            self.passes.iter().all(|node| node.pass.name() != name),
            "pass `{name}` was added two times"
        );
        let accesses = pass.accesses();
        for (i, (id, access)) in accesses.iter().enumerate() {
            // Two uses in one pass have no order, so no barrier can go between them.
            assert!(
                accesses[..i].iter().all(|(other, _)| other != id),
                "pass `{name}` declares `{}` two times",
                self.resources[id.0].name
            );
            assert!(
                *access != Access::Present,
                "pass `{name}`: the graph sets Access::Present itself"
            );
        }
        self.passes.insert(index, PassNode { pass, accesses });
    }

    /// Records the frame. Call between `Gpu::begin_frame` and `Gpu::end_frame`.
    /// `world` is the render world. `internal_resolution` is the size of
    /// [`SizeMode::Internal`] images. `frame` is the number of the app frame: it names
    /// the frame in [`PassTimings`].
    pub fn execute(
        &mut self,
        gpu: &mut Gpu,
        world: &World,
        internal_resolution: (u32, u32),
        frame: u64,
    ) {
        // The GPU has finished the frame that used this frame slot before, so its
        // timestamps are ready: one before the first pass and one after each pass.
        let stamps = gpu.timestamps();
        if let Some(frame) = self.slot_frames[gpu.frame_slot()].replace(frame)
            // False if a pass was added after that frame.
            && stamps.len() == self.passes.len() + 1
        {
            let spans = stamps.windows(2).map(|pair| (pair[1] - pair[0]) as f32);
            let names = self.passes.iter().map(|node| node.pass.name());
            self.timings = Some(PassTimings {
                frame,
                passes: names.zip(spans).collect(),
            });
        }

        // Extension point, dead-pass culling: walk the passes from last to first and keep
        // a pass only if it writes the backbuffer or an image that a kept pass reads.
        // Then skip the passes that are not kept in steps 1 to 3.

        // 1. Transient images.
        // Extension point, memory aliasing: with the first and the last pass that use
        // each transient, two transients whose ranges do not overlap and whose format
        // and size agree can share one image. A shared image must start each range with
        // `state: None`, because the other user changed its content.
        let output = gpu.swapchain_extent();
        for (index, resource) in self.resources.iter_mut().enumerate() {
            let Some(desc) = resource.desc else {
                // The swapchain gives a different image each frame.
                resource.state = None;
                continue;
            };
            let size = match desc.size {
                SizeMode::Internal => internal_resolution,
                SizeMode::Output => output,
            };
            // An image gets the color attachment use only if a pass draws into it. A
            // pass that was added after the image was made causes a new image.
            let attachment = self.passes.iter().any(|node| {
                node.accesses
                    .contains(&(ResourceId(index), Access::ColorAttachment))
            });
            if resource.image.is_some_and(|(_, current)| current == size)
                && resource.attachment == attachment
            {
                continue;
            }
            if let Some((old, _)) = resource.image {
                // Deferred: frames in flight can still use the old image.
                gpu.destroy_image(old);
            }
            let image = gpu
                .create_image(&ImageDesc {
                    name: desc.name,
                    width: size.0,
                    height: size.1,
                    format: desc.format,
                    // ponytail: every transient permits the two shader uses. Take the
                    // flags from the declared accesses if a format does not permit both.
                    storage: true,
                    sampled: true,
                    color_attachment: attachment,
                })
                .unwrap_or_else(|error| panic!("transient image `{}`: {error:#}", desc.name));
            tracing::debug!(name = desc.name, ?size, "transient image created");
            resource.image = Some((image, size));
            resource.attachment = attachment;
            resource.state = None;
        }

        // 2. Barrier plan.
        let mut states: Vec<_> = self.resources.iter().map(|r| r.state).collect();
        let accesses: Vec<_> = self.passes.iter().map(|n| n.accesses.as_slice()).collect();
        let plan = plan_barriers(&accesses, &mut states);
        for (resource, state) in self.resources.iter_mut().zip(states) {
            resource.state = state;
        }
        if plan != self.last_plan {
            self.log_plan(&plan);
        }

        // 3. and 4. Barriers and passes. The plan is in pass order.
        let mut barriers = plan.iter().peekable();
        gpu.write_timestamp().expect("timestamp");
        for index in 0..=self.passes.len() {
            while let Some(barrier) = barriers.next_if(|b| b.before_pass == index) {
                let resource = &self.resources[barrier.resource.0];
                let target = match resource.image {
                    Some((image, _)) => ImageTarget::Image(image),
                    None => ImageTarget::Swapchain,
                };
                gpu.image_barrier(target, barrier.from, barrier.to)
                    .expect("image barrier");
            }
            if let Some(node) = self.passes.get_mut(index) {
                // A profiler shows the CPU time of each pass (Tracy, with the `profile`
                // feature of the game).
                let _span = tracing::trace_span!("pass", name = node.pass.name()).entered();
                node.pass.execute(&mut PassContext {
                    gpu,
                    world,
                    resources: &self.resources,
                });
                gpu.write_timestamp().expect("timestamp");
            }
        }
        self.last_plan = plan;
    }

    /// One debug line per barrier. Called only when the plan changed, so a steady
    /// state logs nothing.
    fn log_plan(&self, plan: &[Barrier]) {
        tracing::debug!(barriers = plan.len(), "frame graph: barrier plan changed");
        for barrier in plan {
            let before = self
                .passes
                .get(barrier.before_pass)
                .map_or("end of frame", |node| node.pass.name());
            tracing::debug!(
                before,
                image = self.resources[barrier.resource.0].name,
                from = ?barrier.from,
                to = ?barrier.to,
                "barrier"
            );
        }
    }
}

/// Finds the barriers of one frame. `passes` holds the declared accesses of each pass,
/// in execution order. `states` holds the last use of each resource (index =
/// `ResourceId`); on return it holds the use at the end of the frame.
fn plan_barriers(
    passes: &[&[(ResourceId, Access)]],
    states: &mut [Option<Access>],
) -> Vec<Barrier> {
    let mut plan = Vec::new();
    let mut change = |before_pass, resource: ResourceId, to: Access| {
        let from = states[resource.0];
        // No barrier between two equal read-only uses: reads have no order, and the
        // layout is the same. A write needs one also after an equal write, so that
        // the two writes run in order.
        if from != Some(to) || to.writes() {
            plan.push(Barrier {
                before_pass,
                resource,
                from,
                to,
            });
        }
        states[resource.0] = Some(to);
    };
    for (index, accesses) in passes.iter().enumerate() {
        for &(resource, access) in *accesses {
            change(index, resource, access);
        }
    }
    // The swapchain must be in the present layout when the frame ends.
    change(passes.len(), BACKBUFFER, Access::Present);
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use Access::*;

    const HDR: ResourceId = ResourceId(1);
    const OUTPUT: ResourceId = ResourceId(2);

    /// (before_pass, resource, from, to), shorter to write than `Barrier`.
    fn plan(
        passes: &[&[(ResourceId, Access)]],
        states: &mut [Option<Access>],
    ) -> Vec<(usize, ResourceId, Option<Access>, Access)> {
        plan_barriers(passes, states)
            .into_iter()
            .map(|b| (b.before_pass, b.resource, b.from, b.to))
            .collect()
    }

    #[test]
    fn barriers_of_the_stage_4_graph() {
        let passes: [&[(ResourceId, Access)]; 3] = [
            &[(HDR, StorageWrite)],
            &[(HDR, Sampled), (OUTPUT, StorageWrite)],
            &[(OUTPUT, TransferSrc), (BACKBUFFER, TransferDst)],
        ];
        let mut states = [None; 3];

        // First frame: all images are new.
        assert_eq!(
            plan(&passes, &mut states),
            [
                (0, HDR, None, StorageWrite),
                (1, HDR, Some(StorageWrite), Sampled),
                (1, OUTPUT, None, StorageWrite),
                (2, OUTPUT, Some(StorageWrite), TransferSrc),
                (2, BACKBUFFER, None, TransferDst),
                (3, BACKBUFFER, Some(TransferDst), Present),
            ]
        );

        // Next frame: the transients start at their last use of the frame before.
        // `execute` sets the backbuffer state to `None` each frame.
        states[BACKBUFFER.0] = None;
        assert_eq!(
            plan(&passes, &mut states),
            [
                (0, HDR, Some(Sampled), StorageWrite),
                (1, HDR, Some(StorageWrite), Sampled),
                (1, OUTPUT, Some(TransferSrc), StorageWrite),
                (2, OUTPUT, Some(StorageWrite), TransferSrc),
                (2, BACKBUFFER, None, TransferDst),
                (3, BACKBUFFER, Some(TransferDst), Present),
            ]
        );
    }

    #[test]
    fn equal_reads_need_no_barrier_but_equal_writes_do() {
        let passes: [&[(ResourceId, Access)]; 4] = [
            &[(HDR, Sampled)],
            &[(HDR, Sampled)],
            &[(HDR, StorageWrite)],
            &[(HDR, StorageWrite)],
        ];
        let mut states = [None, Some(Sampled)];
        assert_eq!(
            plan(&passes, &mut states),
            [
                (2, HDR, Some(Sampled), StorageWrite),
                (3, HDR, Some(StorageWrite), StorageWrite),
                (4, BACKBUFFER, None, Present),
            ]
        );
    }

    struct Named(&'static str);

    impl Pass for Named {
        fn name(&self) -> &'static str {
            self.0
        }
        fn accesses(&self) -> Vec<(ResourceId, Access)> {
            Vec::new()
        }
        fn execute(&mut self, _ctx: &mut PassContext) {}
    }

    #[test]
    fn a_pass_can_go_before_a_named_pass() {
        let mut graph = FrameGraph::default();
        graph.add_pass(Named("a"));
        graph.add_pass(Named("c"));
        graph.add_pass_before("c", Named("b"));
        let names: Vec<_> = graph.passes.iter().map(|n| n.pass.name()).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }
}
