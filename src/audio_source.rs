//! Green-zone metadata for audio clip source regions.

/// Green-zone metadata for an audio clip's source region. The path and
/// musical placement persist; disk streams are created only while compiling
/// a schedule and never enter the arrangement model.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AudioSource {
    pub path: std::path::PathBuf,
    pub sample_rate: u32,
    pub source_offset: u64,
    pub source_frames: u64,
    pub gain: f32,
    pub looped: bool,
    /// Live's Transpose, in semitones. VARISPEED: no live resampler
    /// exists, so a non-zero pitch plays a cached render of the ORIGINAL
    /// file at the ratio — `transposed_from` — and the clip's duration
    /// follows the rate. Zero is the file as it is, bit for bit.
    #[serde(default)]
    pub transpose: f32,
    /// Live's Detune, in cents, added to `transpose` before the ratio.
    #[serde(default)]
    pub detune: f32,
    /// The file the pitch render was made FROM — the original. `None`
    /// while the clip plays its own file (transpose and detune zero).
    /// Returning the knobs to zero hands the clip back this file, so
    /// the pitch change is reversible where a destructive verb is not.
    #[serde(default)]
    pub transposed_from: Option<std::path::PathBuf>,
    /// The pitch ratio currently baked into `path`: 1.0 = the original,
    /// 2.0 = one octave up. The clip's frame-space numbers (fades,
    /// envelope) live in the CURRENT file's space, so every change
    /// scales them by new/old.
    #[serde(default)]
    pub applied_ratio: f32,
    /// Frames in the WHOLE FILE, which `source_offset`/`source_frames`
    /// name a region of.
    ///
    /// Needed to play backwards: the region `[o, o + n)` of a file is
    /// `[F - o - n, F - o)` of its reversal, and there is no way to work
    /// that out without `F`. Zero means "not recorded" — a project
    /// written before this existed — and is read as the region's own end,
    /// which is exact for the untrimmed clip that most of them are.
    #[serde(default)]
    pub file_frames: u64,
    /// Plays backwards. The FILE is reversed into a cache and the clip
    /// points at it; nothing in the audio callback knows, which is why
    /// this costs the red zone nothing at all.
    #[serde(default)]
    pub reversed: bool,
    /// A gain ramp at each end of the clip, in FRAMES of its timeline
    /// span — the same units the node applies them in, so the handle you
    /// drag and the envelope you hear are the same number.
    #[serde(default)]
    pub fade_in: u64,
    #[serde(default)]
    pub fade_out: u64,
    /// Each fade's SHAPE, in `-1..=1`; zero is linear.
    ///
    /// Defaulted, so a project written before shapes existed loads with
    /// the straight ramps it was made with and sounds identical.
    #[serde(default)]
    pub fade_in_curve: f32,
    #[serde(default)]
    pub fade_out_curve: f32,
    /// A gain ride over the clip's timeline span: `(frame, dB)`, sorted.
    ///
    /// dB in the MODEL and linear in the node, because dB is what a
    /// fader is marked in and what a breakpoint should be read as, while
    /// the callback wants a number it can multiply by. The conversion is
    /// one place: the graph builder.
    ///
    /// Empty is no envelope, which is what every clip written before this
    /// carries — so a project from before it loads sounding identical.
    #[serde(default)]
    pub envelope: Vec<(u64, f32)>,
}

impl AudioSource {
    /// The file's length, falling back to this region's end for a
    /// project written before the field existed.
    pub fn file_frames(&self) -> u64 {
        if self.file_frames > 0 {
            self.file_frames
        } else {
            self.source_offset.saturating_add(self.source_frames)
        }
    }

    /// Is there more of the file BEFORE this clip's left edge, and after
    /// its right?
    ///
    /// Invisible until now, and it is the one thing about an audio clip
    /// you cannot work out by looking: a clip trimmed to a quarter of
    /// its file and one that IS its file are drawn identically, so
    /// "can I pull this edge out further" was a question you answered by
    /// trying.
    ///
    /// A looped clip is never trimmed in this sense — it repeats its
    /// region rather than running out of one.
    pub fn spare(&self) -> (bool, bool) {
        if self.looped {
            return (false, false);
        }
        let end = self.source_offset.saturating_add(self.source_frames);
        (self.source_offset > 0, end < self.file_frames())
    }

    /// Where this clip's region starts in whichever file will actually be
    /// streamed — the original, or its reversal.
    pub fn playing_offset(&self) -> u64 {
        if self.reversed {
            self.file_frames()
                .saturating_sub(self.source_offset.saturating_add(self.source_frames))
        } else {
            self.source_offset
        }
    }

    /// The file the node should open. `None` while a reversal has been
    /// asked for and not yet built — the caller decides what silence
    /// means, rather than this quietly handing back the forward file and
    /// playing the clip the wrong way round.
    pub fn playing_path(&self) -> Option<std::path::PathBuf> {
        if !self.reversed {
            return Some(self.path.clone());
        }
        let cache = crate::library::reverse_cache_path(&self.path)?;
        cache.exists().then_some(cache)
    }
}

#[cfg(test)]
mod tests {
    use super::AudioSource;
    use std::path::PathBuf;

    #[test]
    fn serde_round_trip_preserves_every_field_and_file_bits() {
        let source = AudioSource {
            path: PathBuf::from("/projects/session/audio/take-03.wav"),
            sample_rate: 96_000,
            source_offset: 1_337,
            source_frames: 82_901,
            gain: 0.625,
            looped: true,
            transpose: -7.0,
            detune: 13.5,
            transposed_from: Some(PathBuf::from("/projects/session/audio/take.wav")),
            applied_ratio: 0.671_875,
            file_frames: 192_000,
            reversed: true,
            fade_in: 480,
            fade_out: 960,
            fade_in_curve: -0.25,
            fade_out_curve: 0.75,
            envelope: vec![(0, -3.0), (48_000, 1.5), (82_900, -6.0)],
        };

        let encoded = ron::to_string(&source).expect("AudioSource should serialize");
        let decoded: AudioSource =
            ron::from_str(&encoded).expect("serialized AudioSource should load");

        assert_eq!(decoded, source);
        assert_eq!(
            ron::to_string(&decoded).expect("round-tripped AudioSource should serialize"),
            encoded
        );
    }

    #[test]
    fn pre_existing_project_gets_the_original_file_defaults() {
        let encoded = r#"(
            path: "/projects/old-song/audio/take.wav",
            sample_rate: 48000,
            source_offset: 120,
            source_frames: 880,
            gain: 0.75,
            looped: false,
        )"#;

        let source: AudioSource =
            ron::from_str(encoded).expect("pre-existing AudioSource should still load");

        assert_eq!(source.transpose, 0.0);
        assert_eq!(source.detune, 0.0);
        assert_eq!(source.transposed_from, None);
        assert_eq!(source.applied_ratio, 0.0);
        assert_eq!(source.file_frames, 0, "zero means not recorded");
        assert_eq!(
            source.file_frames(),
            1_000,
            "the region end is the fallback"
        );
        assert_eq!(source.playing_path(), Some(source.path.clone()));
    }
}
