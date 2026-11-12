//! Clips: the keys of an animation as a data asset (`<name>.clip.ron`).
//!
//! A clip has tracks; a track changes one channel of the object of the animator (its
//! position, rotation, scale, or a top-level field of one of its components) by keys
//! in time. The sampling is pure: [`clip_time`] gives the time in the clip, and
//! [`Track::sample`] the value of a track at that time.
//!
//! ```ron
//! (
//!     mode: Step,
//!     fps: 12.0,
//!     length: 1.0,
//!     tracks: [
//!         (channel: Position, keys: [
//!             (time: 0.0, value: Vector((0.0, 0.0, 0.0))),
//!             (time: 0.5, value: Vector((0.0, 1.0, 0.0))),
//!         ]),
//!         (channel: Field(component: "light", field: "intensity"), mode: Linear, keys: [
//!             (time: 0.0, value: Number(1.0)),
//!             (time: 1.0, value: Number(4.0)),
//!         ]),
//!     ],
//! )
//! ```

use glam::{EulerRot, Quat, Vec3};
use maki_core::Fields;
use maki_core::fields::Value;
use serde::{Deserialize, Serialize};

use crate::DataAsset;

/// An animation: tracks of keys.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct Clip {
    /// How a track with no mode of its own goes from one key to the next.
    #[serde(skip_serializing_if = "is_default")]
    pub mode: KeyMode,
    /// Frames in a second: the clip time is cut to its frames (animation on twos at
    /// 12). None: no cut.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[field(some = 12.0)]
    pub fps: Option<f32>,
    /// Seconds. 0: the time of the last key.
    #[field(range = 0.0..=3600.0)]
    pub length: f32,
    pub tracks: Vec<Track>,
}

impl DataAsset for Clip {
    const NAME: &'static str = "clip";
}

impl Clip {
    /// The length in seconds: `length`, or with 0 the time of the last key of all tracks.
    pub fn duration(&self) -> f32 {
        if self.length > 0.0 {
            return self.length;
        }
        let times = self.tracks.iter().flat_map(|track| &track.keys);
        times.map(|key| key.time).fold(0.0, f32::max)
    }
}

/// How a track goes from one key to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Fields)]
pub enum KeyMode {
    /// Each value is held until the next key.
    Step,
    /// Numbers and vectors blend in a line; a rotation turns the short way. A switch
    /// and a text step.
    #[default]
    Linear,
}

/// The keys of one channel of the object of the animator.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct Track {
    pub channel: Channel,
    /// None: the mode of the clip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<KeyMode>,
    /// In any order.
    pub keys: Vec<Key>,
}

/// What a track changes.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
pub enum Channel {
    /// A vector key: the offset from the rest pose.
    #[default]
    Position,
    /// A vector key: degrees around X, Y, and Z, as in the pose of a room file.
    Rotation,
    /// A vector key: the factor of the rest scale.
    Scale,
    /// A top-level field of a component of the object, as `SetField`.
    Field { component: String, field: String },
}

/// A value at a time.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct Key {
    /// Seconds from the start of the clip.
    #[field(range = 0.0..=3600.0)]
    pub time: f32,
    pub value: KeyValue,
}

/// The value of a key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum KeyValue {
    Number(f32),
    Vector([f32; 3]),
    Switch(bool),
    Text(String),
}

impl Default for KeyValue {
    fn default() -> Self {
        Self::Number(0.0)
    }
}

/// The value of a track at a time, in the form that its write needs.
#[derive(Debug, Clone, PartialEq)]
pub enum Sample {
    /// A position or a scale.
    Vector(Vec3),
    Rotation(Quat),
    /// A field: the tree of the value, as `SetField` writes it.
    Value(Value),
}

impl Track {
    /// The mode of this track in a clip of the mode `clip_mode`.
    pub fn mode_in(&self, clip_mode: KeyMode) -> KeyMode {
        self.mode.unwrap_or(clip_mode)
    }

    /// The value at the clip time `t`. Before the first key, the first key holds;
    /// after the last, the last. None: no keys, or a key value that the channel
    /// does not take (a number for a position).
    pub fn sample(&self, t: f32, clip_mode: KeyMode) -> Option<Sample> {
        let (key, next) = self.keys_around(t)?;
        let fraction = match (self.mode_in(clip_mode), next) {
            (KeyMode::Linear, Some(next)) => (t - key.time) / (next.time - key.time),
            _ => return self.sample_of(&key.value),
        };
        let next = next.map(|next| &next.value)?;
        match (&self.channel, &key.value, next) {
            (Channel::Rotation, KeyValue::Vector(a), KeyValue::Vector(b)) => {
                Some(Sample::Rotation(rotation(*a).slerp(rotation(*b), fraction)))
            }
            (_, KeyValue::Number(a), KeyValue::Number(b)) => {
                self.sample_of(&KeyValue::Number(a + (b - a) * fraction))
            }
            (_, KeyValue::Vector(a), KeyValue::Vector(b)) => {
                let value = Vec3::from(*a).lerp(Vec3::from(*b), fraction);
                self.sample_of(&KeyValue::Vector(value.into()))
            }
            // A switch, a text, or two keys of different kinds: a step.
            _ => self.sample_of(&key.value),
        }
    }

    /// The key at or before `t` (the first key when `t` is before all), and the key
    /// after `t`. A scan, not a search: the keys may be in any order. Of keys at one
    /// time, the last in the list is the one at that time.
    fn keys_around(&self, t: f32) -> Option<(&Key, Option<&Key>)> {
        let mut before: Option<&Key> = None;
        let mut after: Option<&Key> = None;
        for key in &self.keys {
            if key.time <= t {
                if before.is_none_or(|b| key.time >= b.time) {
                    before = Some(key);
                }
            } else if after.is_none_or(|a| key.time < a.time) {
                after = Some(key);
            }
        }
        match before {
            Some(key) => Some((key, after)),
            // Before the first key: it holds, with nothing to blend to.
            None => after.map(|key| (key, None)),
        }
    }

    fn sample_of(&self, value: &KeyValue) -> Option<Sample> {
        match (&self.channel, value) {
            (Channel::Position | Channel::Scale, KeyValue::Vector(v)) => {
                Some(Sample::Vector(Vec3::from(*v)))
            }
            (Channel::Rotation, KeyValue::Vector(v)) => Some(Sample::Rotation(rotation(*v))),
            (Channel::Field { .. }, value) => Some(Sample::Value(match value {
                KeyValue::Number(n) => n.tree(),
                KeyValue::Vector(v) => v.tree(),
                KeyValue::Switch(b) => b.tree(),
                KeyValue::Text(s) => s.tree(),
            })),
            _ => None,
        }
    }
}

/// Degrees around X, Y, and Z as a rotation, in the order of `PoseDef::transform`.
fn rotation([x, y, z]: [f32; 3]) -> Quat {
    let [x, y, z] = [x, y, z].map(f32::to_radians);
    Quat::from_euler(EulerRot::YXZ, y, x, z)
}

/// A part of a frame: a time that float sums put just below a frame start lands on it.
const FRAME_EPSILON: f32 = 1e-3;

/// The time in a clip of `length` seconds after `time` seconds of play: wrapped
/// when `looping`, else held at the end; with `fps`, cut to the start of its frame,
/// `floor(t * fps) / fps`. A length of 0 or less: always 0.
pub fn clip_time(time: f32, length: f32, looping: bool, fps: Option<f32>) -> f32 {
    if length <= 0.0 {
        return 0.0;
    }
    if !looping && time >= length {
        return length;
    }
    let t = if looping {
        time.rem_euclid(length)
    } else {
        time.max(0.0)
    };
    let Some(fps) = fps.filter(|fps| *fps > 0.0) else {
        return t;
    };
    let t = (t * fps + FRAME_EPSILON).floor() / fps;
    // The epsilon can lift the end of a loop to `length`: that is the frame at 0.
    if looping && t >= length {
        0.0
    } else {
        t.min(length)
    }
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{data_text, read_data};

    fn track(channel: Channel, mode: Option<KeyMode>, keys: &[(f32, KeyValue)]) -> Track {
        let keys = keys
            .iter()
            .cloned()
            .map(|(time, value)| Key { time, value });
        Track {
            channel,
            mode,
            keys: keys.collect(),
        }
    }

    fn number(sample: Option<Sample>) -> f64 {
        match sample {
            Some(Sample::Value(value)) => value.as_f64().unwrap(),
            other => panic!("not a number: {other:?}"),
        }
    }

    fn field() -> Channel {
        Channel::Field {
            component: "light".into(),
            field: "intensity".into(),
        }
    }

    #[test]
    fn a_clip_file_reads_and_writes_with_no_defaults_written() {
        let clip = Clip {
            length: 2.0,
            tracks: vec![
                track(
                    Channel::Position,
                    None,
                    &[(0.5, KeyValue::Vector([1.0, 2.0, 3.0]))],
                ),
                track(
                    field(),
                    Some(KeyMode::Step),
                    &[(0.0, KeyValue::Switch(true))],
                ),
            ],
            ..Clip::default()
        };
        // The tree of the inspector reads back as the clip.
        assert_eq!(maki_core::fields::read::<Clip>(clip.tree()).unwrap(), clip);
        let text = data_text(&clip);
        assert!(
            !text.contains("mode: Linear") && !text.contains("fps"),
            "{text}"
        );
        let dir = std::env::temp_dir().join(format!("maki-clip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("jump.clip.ron");
        std::fs::write(&path, &text).unwrap();
        assert_eq!(read_data::<Clip>(&path).unwrap(), clip);
        // A written mode and frame rate read back.
        let stepped = Clip {
            mode: KeyMode::Step,
            fps: Some(12.0),
            ..clip
        };
        std::fs::write(&path, data_text(&stepped)).unwrap();
        assert_eq!(read_data::<Clip>(&path).unwrap(), stepped);
        // `fps: 12.0` with no `Some`: the RON of the files of a person.
        std::fs::write(&path, "(fps: 12.0, tracks: [])").unwrap();
        assert_eq!(read_data::<Clip>(&path).unwrap().fps, Some(12.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn length_zero_is_the_time_of_the_last_key() {
        let clip = Clip {
            tracks: vec![
                track(Channel::Scale, None, &[(1.5, KeyValue::Vector([1.0; 3]))]),
                track(field(), None, &[(0.2, KeyValue::Number(1.0))]),
            ],
            ..Clip::default()
        };
        assert_eq!(clip.duration(), 1.5);
        assert_eq!(
            Clip {
                length: 3.0,
                ..clip
            }
            .duration(),
            3.0
        );
        assert_eq!(Clip::default().duration(), 0.0);
    }

    #[test]
    fn clip_time_loops_and_holds_at_the_end() {
        assert_eq!(clip_time(2.5, 2.0, true, None), 0.5);
        assert_eq!(clip_time(4.0, 2.0, true, None), 0.0);
        assert_eq!(clip_time(2.5, 2.0, false, None), 2.0);
        assert_eq!(clip_time(1.25, 2.0, false, None), 1.25);
        assert_eq!(clip_time(5.0, 0.0, true, None), 0.0);
        // The end holds also with a frame rate that does not divide the length.
        assert_eq!(clip_time(9.0, 1.05, false, Some(12.0)), 1.05);
    }

    /// The frame of each fixed step of 1/60 s, with the time summed in f32 as a run does.
    fn frames_of_steps(fps: f32, steps: usize, looping: bool) -> Vec<u32> {
        let mut time = 0.0f32;
        let mut frames = Vec::new();
        for _ in 0..steps {
            let t = clip_time(time, 1.0, looping, Some(fps));
            frames.push((t * fps).round() as u32);
            time += 1.0 / 60.0;
        }
        frames
    }

    #[test]
    fn fixed_steps_land_on_the_frames_of_12_fps() {
        // 60 steps of 1/60 s in a second: 5 steps for each frame of 12, in each loop.
        let frames = frames_of_steps(12.0, 600, true);
        for (step, frame) in frames.iter().enumerate() {
            assert_eq!(*frame as usize, (step % 60) / 5, "step {step}");
        }
        // Held: the last frame is the end, 12.
        assert_eq!(*frames_of_steps(12.0, 120, false).last().unwrap(), 12);
    }

    #[test]
    fn clip_time_is_on_twos_at_12_and_on_threes_at_8() {
        // The frames of 24 in a second: 12 changes on each second one, 8 on each third.
        for (fps, hold) in [(12.0, 2), (8.0, 3)] {
            let times: Vec<f32> = (0..48)
                .map(|frame| clip_time(frame as f32 / 24.0, 2.0, true, Some(fps)))
                .collect();
            for (frame, t) in times.iter().enumerate() {
                assert_eq!(*t, (frame / hold) as f32 / fps, "fps {fps} frame {frame}");
            }
        }
        // 8 values in a second of 8 fps.
        let mut values: Vec<f32> = (0..60)
            .map(|step| clip_time(step as f32 / 60.0, 1.0, true, Some(8.0)))
            .collect();
        values.dedup();
        assert_eq!(values.len(), 8);
    }

    #[test]
    fn a_step_track_holds_and_jumps() {
        let keys = [(0.0, KeyValue::Number(1.0)), (0.5, KeyValue::Number(3.0))];
        let step = track(field(), Some(KeyMode::Step), &keys);
        assert_eq!(number(step.sample(0.0, KeyMode::Linear)), 1.0);
        assert_eq!(number(step.sample(0.49, KeyMode::Linear)), 1.0);
        assert_eq!(number(step.sample(0.5, KeyMode::Linear)), 3.0);
        assert_eq!(number(step.sample(9.0, KeyMode::Linear)), 3.0);
        // No mode on the track: that of the clip.
        let of_clip = track(field(), None, &keys);
        assert_eq!(number(of_clip.sample(0.25, KeyMode::Step)), 1.0);
    }

    #[test]
    fn a_linear_track_lerps() {
        let keys = [(0.0, KeyValue::Number(1.0)), (0.5, KeyValue::Number(3.0))];
        let linear = track(field(), None, &keys);
        assert_eq!(number(linear.sample(0.25, KeyMode::Linear)), 2.0);
        assert_eq!(number(linear.sample(0.75, KeyMode::Linear)), 3.0);
        let keys = [
            (1.0, KeyValue::Vector([0.0; 3])),
            (2.0, KeyValue::Vector([2.0, 4.0, -2.0])),
        ];
        let position = track(Channel::Position, None, &keys);
        let middle = Sample::Vector(Vec3::new(1.0, 2.0, -1.0));
        assert_eq!(position.sample(1.5, KeyMode::Linear), Some(middle));
        // Before the first key: the first key holds.
        let start = Sample::Vector(Vec3::ZERO);
        assert_eq!(position.sample(0.0, KeyMode::Linear), Some(start));
    }

    #[test]
    fn a_rotation_slerps_the_short_way() {
        // 170 to -170 around Y: 20 degrees through 180, not 340 through 0.
        let keys = [
            (0.0, KeyValue::Vector([0.0, 170.0, 0.0])),
            (1.0, KeyValue::Vector([0.0, -170.0, 0.0])),
        ];
        let turn = track(Channel::Rotation, None, &keys);
        let Some(Sample::Rotation(middle)) = turn.sample(0.5, KeyMode::Linear) else {
            panic!("not a rotation");
        };
        let half_turn = Quat::from_rotation_y(180f32.to_radians());
        assert!(middle.angle_between(half_turn) < 1e-3, "{middle:?}");
        // A key is degrees around X, Y, Z in the order of the pose of a room file.
        let Some(Sample::Rotation(start)) = turn.sample(0.0, KeyMode::Linear) else {
            panic!("not a rotation");
        };
        assert!(start.angle_between(Quat::from_rotation_y(170f32.to_radians())) < 1e-4);
    }

    #[test]
    fn a_switch_or_a_text_track_is_a_step() {
        let keys = [
            (0.0, KeyValue::Switch(false)),
            (1.0, KeyValue::Switch(true)),
        ];
        let switch = track(field(), Some(KeyMode::Linear), &keys);
        assert_eq!(
            switch.sample(0.9, KeyMode::Linear),
            Some(Sample::Value(false.into()))
        );
        assert_eq!(
            switch.sample(1.0, KeyMode::Linear),
            Some(Sample::Value(true.into()))
        );
        let keys = [
            (0.0, KeyValue::Text("a".into())),
            (1.0, KeyValue::Text("b".into())),
        ];
        let text = track(field(), None, &keys);
        assert_eq!(
            text.sample(0.9, KeyMode::Linear),
            Some(Sample::Value("a".into()))
        );
    }

    #[test]
    fn keys_out_of_order_an_empty_track_and_a_wrong_value() {
        let keys = [
            (1.0, KeyValue::Number(3.0)),
            (0.0, KeyValue::Number(1.0)),
            (0.5, KeyValue::Number(5.0)),
        ];
        let linear = track(field(), None, &keys);
        assert_eq!(number(linear.sample(0.25, KeyMode::Linear)), 3.0);
        assert_eq!(number(linear.sample(0.75, KeyMode::Linear)), 4.0);
        assert_eq!(number(linear.sample(0.75, KeyMode::Step)), 5.0);
        assert_eq!(track(field(), None, &[]).sample(0.5, KeyMode::Step), None);
        // A number for a position: no value, not a panic.
        let wrong = track(Channel::Position, None, &[(0.0, KeyValue::Number(1.0))]);
        assert_eq!(wrong.sample(0.0, KeyMode::Step), None);
    }
}
