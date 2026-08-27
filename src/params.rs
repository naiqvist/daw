//! Parameter tables — the wire contract between widget, engine and app.
//!
//! Every knob a node exposes used to be numbered in three places that could
//! not see each other: the device widget's `P_*` consts, the bare literals in
//! `Node::apply` in `src/audio/graph.rs`, and the bare literals in the app's
//! `apply_*_edits`. Nothing connected them, and the failure mode was silent —
//! a renumbering sends a reverb's mix to a synth's gain and nothing fails to
//! compile. (The old `assert_eq!(P_MIX, 0)` test in the reverb widget was
//! this file trying to exist.)
//!
//! Now a device declares its knobs ONCE: id, name, engine-facing range,
//! default. All three sites read this table — the widget for names, ranges
//! and defaults; the engine for its red-zone clamp; the app for its match
//! arms. Adding a device means adding a module here and using its consts
//! everywhere; using a literal id anywhere else is the bug.
//!
//! Invariants (enforced by tests below): within a table, ids are dense and
//! equal to their index — so `TABLE[FOO as usize]` is the definition of
//! `FOO` — names are unique, and every default lies inside its range.

/// One knob of one device, in ENGINE units (Hz, ms, linear gain — never
/// normalized widget positions).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamDef {
    /// The wire id carried by `ParamChange` letters and `ParamEdit`s.
    pub id: u32,
    /// Display name, also the automation-facing name later.
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl ParamDef {
    /// Clamp an engine-facing value into this knob's range.
    pub fn clamp(self, value: f32) -> f32 {
        value.clamp(self.min, self.max)
    }
}

/// Red-zone lookup: clamp `value` into the range of `id`, or `None` for an
/// id the table does not know (a stale or misrouted letter — drop it).
/// Linear scan over a static table of at most a handful of entries; no
/// allocation, no panic.
pub fn clamp(table: &'static [ParamDef], id: u32, value: f32) -> Option<f32> {
    table
        .iter()
        .find(|def| def.id == id)
        .map(|def| def.clamp(value))
}

/// Green-zone lookup by id. Panics on an unknown id, which is a compile-time
/// mistake when callers use the named consts — never call this with a
/// runtime-provided id (that is what [`clamp`] is for).
pub fn def(table: &'static [ParamDef], id: u32) -> &'static ParamDef {
    &table[id as usize]
}

/// The built-in sequencer synth (`Node::Seq`, the SineSynth device).
pub mod seq {
    use super::ParamDef;

    pub const GAIN: u32 = 0;
    pub const ATTACK: u32 = 1;
    pub const RELEASE: u32 = 2;

    /// Defaults match the synth's original hardcoded voice: unity gain,
    /// ~1 ms attack, ~640 ms release.
    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 1.0,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: 0.05,
            max: 5_000.0,
            default: 1.0,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: 1.0,
            max: 30_000.0,
            default: 640.0,
        },
    ];
}

/// `Node::Reverb`.
/// The reverb: a feedback delay network, `dsp::fdn::Fdn`.
///
/// Nine rows, and the split is deliberate. The first five describe the
/// SPACE — where it starts, how big it is, how long it rings, how dark
/// it gets, and what the tail keeps of the low end. The last four
/// describe how that space is PRESENTED: how dense, how much it moves,
/// how wide, and how much of it you hear.
///
/// The old three-row table (mix, size, damp) is gone with the Freeverb
/// it drove. It had no pre-delay, so the tail started on top of the
/// source and every setting sounded like a wash; and it was mono, which
/// is the one thing a reverb cannot be.
pub mod reverb {
    use super::ParamDef;

    pub const PREDELAY: u32 = 0;
    pub const SIZE: u32 = 1;
    pub const DECAY: u32 = 2;
    pub const DAMP: u32 = 3;
    pub const LOWCUT: u32 = 4;
    pub const DIFFUSION: u32 = 5;
    pub const MODULATION: u32 = 6;
    pub const WIDTH: u32 = 7;
    pub const MIX: u32 = 8;

    /// The longest gap between the source and its room, in ms.
    ///
    /// Pre-delay is what separates a reverb from a wash: a few tens of
    /// milliseconds of silence lets the dry transient through before the
    /// room answers, which is why a vocal stays intelligible in a hall.
    pub const PREDELAY_MAX: f32 = 200.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: PREDELAY,
            name: "predelay",
            min: 0.0,
            max: PREDELAY_MAX,
            default: 20.0,
        },
        ParamDef {
            id: SIZE,
            name: "size",
            min: crate::dsp::fdn::SIZE_MIN,
            max: crate::dsp::fdn::SIZE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: crate::dsp::fdn::DECAY_MIN,
            max: crate::dsp::fdn::DECAY_MAX,
            // A medium room. Loading a reverb must not drown the track
            // it lands on.
            default: 1.8,
        },
        ParamDef {
            id: DAMP,
            name: "damp",
            // The corner of the one-pole in every feedback path. Named
            // in HERTZ rather than as a fraction, because "the tail goes
            // dark above 4 kHz" is a thing a person can picture.
            min: 400.0,
            max: 18_000.0,
            default: 5_000.0,
        },
        ParamDef {
            id: LOWCUT,
            name: "lowcut",
            // Off at the bottom. A reverb tail carrying the fundamental
            // of everything it is fed is how a mix turns to mud, and
            // this is the control that stops it.
            min: 20.0,
            max: 800.0,
            default: 120.0,
        },
        ParamDef {
            id: DIFFUSION,
            name: "diffusion",
            min: 0.0,
            max: 1.0,
            default: 0.8,
        },
        ParamDef {
            id: MODULATION,
            name: "modulation",
            // In samples of wander. Zero is a still network, which rings;
            // the top is a chorused tail.
            min: 0.0,
            max: 8.0,
            default: 2.0,
        },
        ParamDef {
            id: WIDTH,
            name: "width",
            // 0 is mono, 1 is the network's own spread, 2 over-widens.
            min: 0.0,
            max: 2.0,
            default: 1.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            default: 0.25,
        },
    ];
}

/// `Node::Sine`, the lab's tone generator.
pub mod sine {
    use super::ParamDef;

    pub const FREQ: u32 = 0;
    pub const AMP: u32 = 1;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: FREQ,
            name: "freq",
            min: 1.0,
            max: 20_000.0,
            default: 440.0,
        },
        ParamDef {
            id: AMP,
            name: "amp",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
    ];
}

/// `Node::Mixer`.
pub mod mixer {
    use super::ParamDef;

    pub const GAIN: u32 = 0;

    pub const TABLE: &[ParamDef] = &[ParamDef {
        id: GAIN,
        name: "gain",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    }];
}

/// `Node::Pan` — the per-track output stage: placement AND level.
///
/// Volume lives here rather than in a node of its own for the reason pan
/// does: every track already has this node, so a fader move is a param
/// letter to a permanent address instead of a schedule swap. The ceiling is
/// +6 dB in linear terms (1.995), the range every console's fader has above
/// unity.
pub mod pan {
    use super::ParamDef;

    pub const PAN: u32 = 0;
    pub const GAIN: u32 = 1;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: PAN,
            name: "pan",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: GAIN,
            name: "volume",
            min: 0.0,
            max: 1.995_262_3,
            default: 1.0,
        },
    ];
}

/// `Node::AudioClip`.
pub mod clip {
    use super::ParamDef;

    pub const GAIN: u32 = 0;
    pub const FADE_IN: u32 = 1;
    pub const FADE_OUT: u32 = 2;
    pub const FADE_IN_CURVE: u32 = 3;
    pub const FADE_OUT_CURVE: u32 = 4;

    /// The longest a fade may be, in FRAMES.
    ///
    /// Frames, not seconds, because every other length a clip carries is
    /// in frames and a fade has to be compared against them — and because
    /// the node has no sample rate of its own to convert with. The
    /// ceiling is generous rather than meaningful: what actually bounds a
    /// fade is the clip it lives on, which the app clamps against.
    pub const FADE_MAX_FRAMES: f32 = 4_800_000.0;

    /// A fade's SHAPE, as one continuous family rather than a menu.
    ///
    /// ```text
    /// y(x) = x * (1 + k) / (1 + k * x)      k > -1
    /// ```
    ///
    /// It passes through both endpoints, it is monotonic for every legal
    /// `k`, and it costs one multiply and one divide per sample. `k = 0`
    /// is exactly linear; above zero it rises fast then flattens, below
    /// zero the reverse.
    ///
    /// `powf` would give the same shapes and is neither unsafe nor
    /// unbounded — it is simply a transcendental per sample per channel
    /// on every clip that has a fade, which is a real cost for no
    /// difference anyone can hear. Hence the rational curve.
    ///
    /// Here rather than in the renderer or the node because BOTH need it
    /// and they must agree: a destructive fade and a clip fade of the
    /// same shape have to produce the same samples, or committing one
    /// would change the sound.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Curve {
        /// The shape, in `-1..=1`. Zero is linear.
        pub shape: f32,
    }

    impl Default for Curve {
        fn default() -> Self {
            Self::LINEAR
        }
    }

    impl Curve {
        pub const LINEAR: Self = Self { shape: 0.0 };

        pub fn new(shape: f32) -> Self {
            Self {
                shape: if shape.is_finite() {
                    shape.clamp(-1.0, 1.0)
                } else {
                    0.0
                },
            }
        }

        /// The curve's `k`, which must stay ABOVE −1.
        ///
        /// The two halves are not the same formula, and that is not an
        /// accident. Inverting this family — reflecting the curve about
        /// the diagonal, which is what "the opposite shape" means — sends
        /// `k` to `-k / (1 + k)`. Put `k₊ = a / (1 - a)` through that and
        /// it comes back as exactly `-a`. So the negative half IS the
        /// mirror of the positive half, and it is also the only mapping
        /// that keeps `k` above −1.
        ///
        /// The obvious symmetric-looking guess, `shape / (1 - |shape|)`
        /// on both sides, sends `k` to −4 at a shape of −0.8: the
        /// denominator crosses zero a quarter of the way along, the
        /// "fade" goes to infinity and then negative, and the shape is
        /// not a fade at all.
        ///
        /// The shape is clamped just inside ±1 so the mapping stays
        /// finite — at exactly ±1 it would be a vertical step.
        fn k(self) -> f32 {
            let shape = self.shape.clamp(-0.999, 0.999);
            if shape >= 0.0 {
                shape / (1.0 - shape)
            } else {
                shape
            }
        }

        /// `y` for an `x` in `0..=1`, both ends exact.
        pub fn at(self, x: f32) -> f32 {
            let x = x.clamp(0.0, 1.0);
            let k = self.k();
            if k == 0.0 {
                return x;
            }
            x * (1.0 + k) / (1.0 + k * x)
        }
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 1.0,
        },
        ParamDef {
            id: FADE_IN,
            name: "fadein",
            min: 0.0,
            max: FADE_MAX_FRAMES,
            default: 0.0,
        },
        ParamDef {
            id: FADE_OUT,
            name: "fadeout",
            min: 0.0,
            max: FADE_MAX_FRAMES,
            default: 0.0,
        },
        // The SHAPES ride letters beside the lengths, so dragging a
        // curve is heard while it is dragged rather than when the next
        // recompile catches up.
        ParamDef {
            id: FADE_IN_CURVE,
            name: "fadeincurve",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: FADE_OUT_CURVE,
            name: "fadeoutcurve",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
    ];
}

/// `Node::Filter` — the first kernel-backed effect. Beyond the knob table
/// this module owns the numbers BOTH the drawn curve and the audio path
/// derive from, so the display cannot drift from the sound: the resonance
/// mapping, the drive squash, and the slope list.
pub mod filter {
    use super::ParamDef;

    pub const MODE: u32 = 0;
    pub const SLOPE: u32 = 1;
    pub const CUTOFF: u32 = 2;
    pub const RES: u32 = 3;
    pub const DRIVE: u32 = 4;
    pub const CHARACTER: u32 = 5;
    pub const SPREAD: u32 = 6;

    /// Mode indices on the wire. Same order as the widget's mode strip.
    pub const MODE_LP: u32 = 0;
    pub const MODE_HP: u32 = 1;
    pub const MODE_BP: u32 = 2;
    pub const MODE_NOTCH: u32 = 3;

    /// Butterworth-flat: no peak at the corner. The resonance knob's
    /// audible floor — below this the resonant section stays flat, in the
    /// drawing and in the audio alike.
    pub const FLAT_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

    /// How hard drive squashes resonance in the CLEAN character. The
    /// saturation lives in the feedback path, so at full drive a peak
    /// that would be `1 + x` above flat is squashed to
    /// `1 + x / (1 + DRIVE_SQUASH)`.
    ///
    /// Every other character scales this — see [`CHARACTERS`].
    pub const DRIVE_SQUASH: f32 = 3.0;

    /// Character indices on the wire.
    pub const CHAR_CLEAN: u32 = 0;
    pub const CHAR_LADDER: u32 = 1;
    pub const CHAR_OTA: u32 = 2;
    pub const CHAR_DIODE: u32 = 3;
    pub const CHAR_MAX: u32 = CHAR_DIODE;

    /// WHAT KIND OF FILTER THIS IS, past the coefficients.
    ///
    /// Two filters with the same corner and the same slope can sound
    /// nothing alike, and the difference is almost never the response
    /// curve — it is where the nonlinearity sits, what shape it is, and
    /// what the resonance does when you lean on it. That is what a
    /// character is here: not a preset over the other knobs, but the
    /// handful of numbers the curve cannot show.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Character {
        pub name: &'static str,
        /// Multiplier on [`DRIVE_SQUASH`]. Higher means the resonance
        /// gives way sooner as the drive comes up — a ladder's peak
        /// collapses under drive, an OTA's holds on.
        pub squash: f32,
        /// Asymmetry fed to the shaper. Non-zero makes EVEN harmonics,
        /// which is what separates a buzz from a growl.
        pub bias: f32,
        /// Multiplier on the drive the shaper is given.
        pub drive_scale: f32,
        /// Which `dsp::shaper` curve, as an index into
        /// [`sat::MODE_*`](super::sat).
        pub shape: u32,
        /// How much low end the resonance eats, `0..=1` at full
        /// resonance. A real ladder thins out as it is turned up because
        /// the feedback subtracts the input; an OTA does not.
        pub bass_loss: f32,
    }

    /// The four, in wire order.
    ///
    /// These are voicings, not emulations of any particular circuit — the
    /// names say which tradition each one is reaching for, and the numbers
    /// are chosen so that the four are clearly different from one another
    /// at the same settings. That last part is the actual requirement: a
    /// character switch whose positions you cannot tell apart is a switch
    /// nobody will ever move twice.
    pub const CHARACTERS: &[Character] = &[
        // The filter as it was before there were characters: a symmetric
        // soft clip after the poles, moderate squash, no bass loss. The
        // reference, and the one to pick when the filter should get out
        // of the way.
        Character {
            name: "clean",
            squash: 1.0,
            bias: 0.0,
            drive_scale: 1.0,
            shape: super::sat::MODE_SOFT,
            bass_loss: 0.0,
        },
        // Fat, and it gives way. The peak collapses under drive and the
        // bottom thins as the resonance comes up, which together are why
        // a ladder sweep sounds like it is being played rather than set.
        Character {
            name: "ladder",
            squash: 1.7,
            bias: 0.0,
            drive_scale: 1.2,
            shape: super::sat::MODE_SOFT,
            bass_loss: 0.35,
        },
        // Bright and stubborn. Least squash of the four, so the peak
        // stays put however hard it is driven, and a cubic curve rather
        // than a tanh — a harder knee, more of the odd harmonics that
        // read as glassy.
        Character {
            name: "ota",
            squash: 0.6,
            bias: 0.0,
            drive_scale: 1.0,
            shape: super::sat::MODE_CUBIC,
            bass_loss: 0.0,
        },
        // The nasty one. Asymmetric, hardest squash, most drive: the
        // resonance folds over almost immediately and what is left is
        // buzz with a corner in it. Nobody reaches for this to be
        // tasteful.
        Character {
            name: "diode",
            squash: 2.4,
            bias: 0.30,
            drive_scale: 1.6,
            shape: super::sat::MODE_SOFT,
            bass_loss: 0.20,
        },
    ];

    pub const CHARACTER_NAMES: &[&str] = &["clean", "ladder", "ota", "diode"];

    /// The character a wire index names.
    ///
    /// Out of range gives `clean` — the FIRST, not the nearest. A stale
    /// or corrupt index must still filter, and the position it lands on
    /// should be the one that gets out of the way rather than the one
    /// that screams. Clamping to the last instead would make a truncated
    /// project file open sounding like a fuzz box.
    pub fn character(index: u32) -> &'static Character {
        CHARACTERS.get(index as usize).unwrap_or(&CHARACTERS[0])
    }

    /// The widest the two channels' corners may be pushed apart, in
    /// SEMITONES.
    ///
    /// Semitones rather than hertz, because the useful amount of spread
    /// is a musical interval and not a fixed distance: half an octave
    /// apart at 200 Hz and half an octave apart at 8 kHz are the same
    /// gesture, and 400 Hz apart is a different one at each.
    pub const SPREAD_MAX_ST: f32 = 12.0;

    /// One channel's corner, given the knob and how far to lean.
    ///
    /// `side` is -1 for left and +1 for right, so the two move in
    /// OPPOSITE directions around the cutoff the knob names — the corner
    /// you set stays the centre of what you hear, and turning spread up
    /// widens rather than detunes.
    pub fn spread_cutoff(cutoff_hz: f32, spread_st: f32, side: f32) -> f32 {
        let half = spread_st.clamp(0.0, SPREAD_MAX_ST) * 0.5 * side;
        (cutoff_hz * (half / 12.0).exp2()).clamp(20.0, 20_000.0)
    }

    /// Slope index -> filter order (poles). 6 dB per octave per pole.
    pub const SLOPE_ORDERS: &[u32] = &[1, 2, 3, 4, 6, 8];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MODE,
            name: "mode",
            min: 0.0,
            max: 3.0,
            default: 0.0, // lowpass
        },
        ParamDef {
            id: SLOPE,
            name: "slope",
            min: 0.0,
            max: 5.0,
            default: 3.0, // 24 dB/octave
        },
        ParamDef {
            id: CUTOFF,
            name: "cutoff",
            min: 20.0,
            max: 20_000.0,
            // Parked out of the way: loading a filter is transparent, and
            // the first knob touch is the first audible change — the
            // reverb's never-destroy-the-mix rule, applied to a filter.
            default: 20_000.0,
        },
        ParamDef {
            id: RES,
            name: "res",
            min: 0.3,
            // Just below scream: +24 dB or so of peak, matching the top of
            // the display window. Self-oscillation is a feature for a
            // later version that has a limiter behind it.
            max: 24.0,
            default: FLAT_Q,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: CHARACTER,
            name: "character",
            min: 0.0,
            max: CHAR_MAX as f32,
            // LADDER, not clean. The cutoff is parked open so the device
            // is silent on load whatever this says — and when the first
            // sweep does happen, it should sound like something rather
            // than like arithmetic. Clean is one position away for
            // anybody who wants the filter to get out of the way.
            default: CHAR_LADDER as f32,
        },
        ParamDef {
            id: SPREAD,
            name: "spread",
            min: 0.0,
            max: SPREAD_MAX_ST,
            // Zero: a stereo width effect nobody asked for is the one
            // thing worse than a mono filter, and `spread` is the knob
            // that says you did ask.
            default: 0.0,
        },
    ];

    /// The resonant section's effective Q after drive has had its say —
    /// the ONE resonance mapping. Drive squashes only the part of the
    /// resonance above flat, because that is the part living in the
    /// feedback path where the saturation is; a gentle filter driven hard
    /// does not lose its corner. Sub-flat requests floor at flat, which
    /// keeps the drawn curve and the audio identical there too.
    ///
    /// The engine feeds this straight to its resonant section; the display
    /// scales it by the section's Butterworth base via [`resonant_q`].
    /// Same function, so agreement is by construction, not by test alone.
    pub fn effective_q(q: f32, drive: f32) -> f32 {
        effective_q_for(q, drive, CHAR_CLEAN)
    }

    /// [`effective_q`] for a given character.
    ///
    /// The character scales how hard the drive squashes: a ladder's peak
    /// collapses under drive and an OTA's holds on, and that difference
    /// is most of what makes them recognisable. The clean character
    /// scales by one, so it is exactly the old behaviour and the
    /// pre-character projects that reach this path still sound the way
    /// they did.
    pub fn effective_q_for(q: f32, drive: f32, model: u32) -> f32 {
        let excess = (q.max(0.05) - FLAT_Q).max(0.0);
        let squash = DRIVE_SQUASH * character(model).squash;
        FLAT_Q + excess / (1.0 + drive.clamp(0.0, 1.0) * squash)
    }

    /// [`effective_q`] expressed against a cascade section's Butterworth
    /// base — the form the display's per-section math consumes.
    pub fn resonant_q(q: f32, base: f32, drive: f32) -> f32 {
        resonant_q_for(q, base, drive, CHAR_CLEAN)
    }

    /// [`resonant_q`] for a given character.
    pub fn resonant_q_for(q: f32, base: f32, drive: f32, model: u32) -> f32 {
        base * (effective_q_for(q, drive, model) / FLAT_Q)
    }

    /// Drive knob (0..=1) -> waveshaper drive. Full drive pushes ~24 dB
    /// into the soft clipper — growl territory, not bitcrush.
    pub fn shaper_drive(drive: f32) -> f32 {
        1.0 + drive.clamp(0.0, 1.0) * 15.0
    }

    /// [`shaper_drive`] scaled by the character.
    pub fn shaper_drive_for(drive: f32, model: u32) -> f32 {
        let scale = character(model).drive_scale;
        (1.0 + drive.clamp(0.0, 1.0) * 15.0 * scale)
            .clamp(crate::dsp::shaper::DRIVE_MIN, crate::dsp::shaper::DRIVE_MAX)
    }

    /// Slope index -> order, clamped to the steepest available.
    pub fn slope_order(index: u32) -> u32 {
        SLOPE_ORDERS[(index as usize).min(SLOPE_ORDERS.len() - 1)]
    }

    /// The level a resonant filter GIVES UP as the resonance comes up, as
    /// a linear gain — [`Character::bass_loss`] with the knob applied.
    ///
    /// A ladder thins out as its feedback subtracts the input, an OTA
    /// does not; only a lowpass has a bottom end to lose, so the other
    /// three modes come back at unity.
    ///
    /// The one place this arithmetic lives: the node multiplies its
    /// filtered block by it and the card's drawn curve shifts by the same
    /// number, so the picture and the audio agree by construction rather
    /// than by inspection. Before it was shared, a ladder at Q 8 measured
    /// a decibel under what the display drew and nothing in the app could
    /// have told you which one was lying.
    pub fn resonance_loss(res: f32, model: u32, mode: u32) -> f32 {
        let loss = character(model).bass_loss;
        if loss <= 0.0 || mode != MODE_LP {
            return 1.0;
        }
        // Against the knob's own top, so "full resonance" means the
        // stop and not an arbitrary Q.
        let span = TABLE[RES as usize].max;
        let excess = ((res.max(0.05) - FLAT_Q) / span).clamp(0.0, 1.0);
        1.0 - loss * excess
    }
}

/// `Node::Eq` — the eight-band equaliser.
///
/// # Why the table is eight identical groups and not eight of anything else
///
/// The band count is FIXED. Every target string, every modulation wire and
/// `shape_hash` itself are built on a static parameter table, so a band
/// that could come and go would be a parameter that could come and go —
/// and an automation lane pointed at a band that no longer exists is the
/// kind of silent failure this codebase spends its comments avoiding.
/// Eight permanent bands, each with an ON switch, costs one filter pair
/// per ENABLED band and nothing at all for the rest.
///
/// Ids are `band * PER_BAND + slot`, in table order, so `TABLE[id]` is
/// that id's row — the same property `echo` and `sat` rely on.
pub mod eq {
    use super::ParamDef;

    /// How many bands the equaliser has. Not a maximum: they all exist,
    /// all the time, and an unused one is switched off rather than absent.
    pub const BANDS: usize = 8;
    /// How many parameters each band carries.
    pub const PER_BAND: u32 = 5;

    // The slot within a band.
    pub const ON: u32 = 0;
    pub const TYPE: u32 = 1;
    pub const FREQ: u32 = 2;
    pub const GAIN: u32 = 3;
    pub const Q: u32 = 4;

    /// The `ParamChange` id of one band's one slot.
    pub const fn id(band: usize, slot: u32) -> u32 {
        band as u32 * PER_BAND + slot
    }

    /// Which band an id belongs to, and which slot of it. `None` for the
    /// output trim, which belongs to no band.
    pub const fn split(id: u32) -> Option<(usize, u32)> {
        if id >= OUT {
            return None;
        }
        Some(((id / PER_BAND) as usize, id % PER_BAND))
    }

    /// The output trim, after every band. The one row that is not a band.
    pub const OUT: u32 = BANDS as u32 * PER_BAND;

    // The shapes a band can take, as indices into `TYPE_NAMES`.
    pub const TYPE_LO_CUT_12: u32 = 0;
    pub const TYPE_LO_CUT_48: u32 = 1;
    pub const TYPE_LO_SHELF: u32 = 2;
    pub const TYPE_BELL: u32 = 3;
    pub const TYPE_NOTCH: u32 = 4;
    pub const TYPE_HI_SHELF: u32 = 5;
    pub const TYPE_HI_CUT_12: u32 = 6;
    pub const TYPE_HI_CUT_48: u32 = 7;

    /// The eight shapes, in the order a band's type cell steps through
    /// them: low end first, high end last, so stepping the control walks
    /// up the spectrum the way the picture does.
    pub const TYPE_NAMES: &[&str] = &[
        "lo cut 12",
        "lo cut 48",
        "lo shelf",
        "bell",
        "notch",
        "hi shelf",
        "hi cut 12",
        "hi cut 48",
    ];

    /// Whether this shape applies a GAIN. The cuts and the notch do not —
    /// they take away what they take away — so their gain cell is dead
    /// and their curve handle only moves sideways.
    pub const fn has_gain(shape: u32) -> bool {
        matches!(shape, TYPE_LO_SHELF | TYPE_BELL | TYPE_HI_SHELF)
    }

    /// The order of the cut this shape asks for, in poles. Zero for
    /// everything that is not a cut.
    pub const fn cut_order(shape: u32) -> u32 {
        match shape {
            TYPE_LO_CUT_12 | TYPE_HI_CUT_12 => 2,
            TYPE_LO_CUT_48 | TYPE_HI_CUT_48 => 8,
            _ => 0,
        }
    }

    /// Whether this shape is a cut that keeps the HIGH end.
    pub const fn is_highpass(shape: u32) -> bool {
        matches!(shape, TYPE_LO_CUT_12 | TYPE_LO_CUT_48)
    }

    /// The lowest and highest corner a band will accept. The audible band
    /// with a little either side, which is also exactly what the display
    /// draws — a band you cannot see is a band you cannot get back.
    pub const MIN_HZ: f32 = 20.0;
    pub const MAX_HZ: f32 = 20_000.0;

    /// The most a band will boost or cut, in dB.
    ///
    /// Eighteen rather than a wilder figure: past this an EQ band is
    /// being used as a filter or a fader, and both of those exist.
    pub const MAX_GAIN_DB: f32 = 18.0;

    /// The Q range. The bottom is a very wide, gentle shape; the top
    /// rings hard enough to use as a surgical notch.
    pub const MIN_Q: f32 = 0.1;
    pub const MAX_Q: f32 = 18.0;

    /// Butterworth, the Q that peaks at nothing. Every band's default,
    /// so a freshly switched-on band is the plain shape its name says.
    pub const FLAT_Q: f32 = core::f32::consts::FRAC_1_SQRT_2;

    /// The most the output trim will move, in dB.
    pub const MAX_OUT_DB: f32 = 24.0;

    pub const TABLE: &[ParamDef] = &[
        // ---- band 1: a low cut, out of the way until you want it. ----
        ParamDef {
            id: id(0, ON),
            name: "on1",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(0, TYPE),
            name: "type1",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_LO_CUT_12 as f32,
        },
        ParamDef {
            id: id(0, FREQ),
            name: "freq1",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 80.0,
        },
        ParamDef {
            id: id(0, GAIN),
            name: "gain1",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(0, Q),
            name: "q1",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 2: the low shelf: weight, or the lack of it. ----
        ParamDef {
            id: id(1, ON),
            name: "on2",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(1, TYPE),
            name: "type2",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_LO_SHELF as f32,
        },
        ParamDef {
            id: id(1, FREQ),
            name: "freq2",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 150.0,
        },
        ParamDef {
            id: id(1, GAIN),
            name: "gain2",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(1, Q),
            name: "q2",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 3: the low mids, where a mix goes muddy. ----
        ParamDef {
            id: id(2, ON),
            name: "on3",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(2, TYPE),
            name: "type3",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_BELL as f32,
        },
        ParamDef {
            id: id(2, FREQ),
            name: "freq3",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 350.0,
        },
        ParamDef {
            id: id(2, GAIN),
            name: "gain3",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(2, Q),
            name: "q3",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 4: the box. ----
        ParamDef {
            id: id(3, ON),
            name: "on4",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(3, TYPE),
            name: "type4",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_BELL as f32,
        },
        ParamDef {
            id: id(3, FREQ),
            name: "freq4",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 800.0,
        },
        ParamDef {
            id: id(3, GAIN),
            name: "gain4",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(3, Q),
            name: "q4",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 5: presence. ----
        ParamDef {
            id: id(4, ON),
            name: "on5",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(4, TYPE),
            name: "type5",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_BELL as f32,
        },
        ParamDef {
            id: id(4, FREQ),
            name: "freq5",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 2000.0,
        },
        ParamDef {
            id: id(4, GAIN),
            name: "gain5",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(4, Q),
            name: "q5",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 6: edge. ----
        ParamDef {
            id: id(5, ON),
            name: "on6",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(5, TYPE),
            name: "type6",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_BELL as f32,
        },
        ParamDef {
            id: id(5, FREQ),
            name: "freq6",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 5000.0,
        },
        ParamDef {
            id: id(5, GAIN),
            name: "gain6",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(5, Q),
            name: "q6",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 7: air, as a shelf rather than a bump. ----
        ParamDef {
            id: id(6, ON),
            name: "on7",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(6, TYPE),
            name: "type7",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_HI_SHELF as f32,
        },
        ParamDef {
            id: id(6, FREQ),
            name: "freq7",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 10000.0,
        },
        ParamDef {
            id: id(6, GAIN),
            name: "gain7",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(6, Q),
            name: "q7",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 8: a high cut at the top of the band. ----
        ParamDef {
            id: id(7, ON),
            name: "on8",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(7, TYPE),
            name: "type8",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_HI_CUT_12 as f32,
        },
        ParamDef {
            id: id(7, FREQ),
            name: "freq8",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 16000.0,
        },
        ParamDef {
            id: id(7, GAIN),
            name: "gain8",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(7, Q),
            name: "q8",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- and the one row that belongs to no band. ----
        ParamDef {
            id: OUT,
            name: "out",
            min: -MAX_OUT_DB,
            max: MAX_OUT_DB,
            // Unity. An equaliser that changed the level before you
            // touched it would make every A/B a lie.
            default: 0.0,
        },
    ];
}

/// `Node::Glue` — the bus compressor.
///
/// Beyond the knob table this module owns the numbers BOTH the drawn
/// transfer curve and the audio path derive from, so the display cannot
/// drift from the sound: the switch positions, and the KNEE, which is not
/// a control at all but a consequence of the ratio.
///
/// # Why attack and release are switches
///
/// Because the unit this models has switches. A bus compressor's attack
/// is not a continuous search — it is a choice between "let the transient
/// through" and "catch it", and the six or seven positions on the panel
/// are the ones that have been useful for forty years. A continuous knob
/// here would be more freedom and less help: every position between 10
/// and 30 ms is a position nobody can hear the point of, and the ones
/// that matter would be harder to land on.
pub mod glue {
    use super::ParamDef;

    pub const THRESHOLD: u32 = 0;
    pub const RATIO: u32 = 1;
    pub const ATTACK: u32 = 2;
    pub const RELEASE: u32 = 3;
    pub const MAKEUP: u32 = 4;
    pub const DRY_WET: u32 = 5;
    pub const RANGE: u32 = 6;
    pub const CLIP: u32 = 7;
    pub const SC_HP: u32 = 8;

    /// The three ratios, and their names. A bus compressor offers a
    /// choice, not a sweep: 2:1 glues, 4:1 controls, 10:1 limits.
    pub const RATIO_VALUES: &[f32] = &[2.0, 4.0, 10.0];
    pub const RATIO_NAMES: &[&str] = &["2:1", "4:1", "10:1"];

    /// The attack positions, in MILLISECONDS.
    ///
    /// The fastest is half a sample at 48 kHz, which is not a mistake:
    /// it is what "instant" costs to write down, and the ballistics
    /// kernel treats any sub-sample time as instant.
    pub const ATTACK_MS: &[f32] = &[0.01, 0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
    /// Bare numbers: the unit rides the RAIL'S LABEL ("attack ms"), not
    /// every segment. Seven segments each carrying "0.01 ms" is a rail
    /// half again as wide, for a unit that cannot change between
    /// positions — and the card has two of these to fit side by side.
    /// With their unit, because a CELL prints the selected one ONCE.
    ///
    /// These were briefly bare, back when the card drew all seven at
    /// once on a rail and the unit would have been printed seven times
    /// for a figure that cannot change between positions. A cell shows
    /// only the position you are on, so it can afford to say what it is
    /// — and a bare "0.3" on a compressor is ambiguous in a way
    /// "0.3 ms" is not.
    pub const ATTACK_NAMES: &[&str] = &[
        "0.01 ms", "0.1 ms", "0.3 ms", "1 ms", "3 ms", "10 ms", "30 ms",
    ];

    /// The release positions, in SECONDS — and then AUTO, which is not a
    /// time at all.
    ///
    /// Indexed together because they are one switch on the panel, and
    /// the last position is the one that matters most: see
    /// [`is_auto`] and `dsp::dynamics::Ballistics`.
    pub const RELEASE_S: &[f32] = &[0.1, 0.2, 0.4, 0.6, 0.8, 1.2];
    /// Bare numbers for the reason [`ATTACK_NAMES`] gives — except the
    /// last, which is not a number and says so.
    /// With their unit, for the reason [`ATTACK_NAMES`] gives — except
    /// the last, which is not a time and says so.
    pub const RELEASE_NAMES: &[&str] =
        &["0.1 s", "0.2 s", "0.4 s", "0.6 s", "0.8 s", "1.2 s", "auto"];

    /// Which release position means "program dependent".
    pub const RELEASE_AUTO: u32 = 6;

    /// Whether this release position is AUTO rather than a time.
    pub const fn is_auto(index: u32) -> bool {
        index >= RELEASE_AUTO
    }

    /// The release time a position asks for, in ms. Auto has no time of
    /// its own — the kernel's two poles are the answer — so it reports
    /// the middle of the range, which is what the auto poles bracket and
    /// what a display should print if it prints anything.
    pub fn release_ms(index: u32) -> f32 {
        let i = (index as usize).min(RELEASE_S.len() - 1);
        RELEASE_S.get(i).copied().unwrap_or(0.4) * 1_000.0
    }

    /// The attack time a position asks for, in ms.
    pub fn attack_ms(index: u32) -> f32 {
        let i = (index as usize).min(ATTACK_MS.len() - 1);
        ATTACK_MS.get(i).copied().unwrap_or(10.0)
    }

    /// The ratio a position asks for.
    pub fn ratio(index: u32) -> f32 {
        let i = (index as usize).min(RATIO_VALUES.len() - 1);
        RATIO_VALUES.get(i).copied().unwrap_or(4.0)
    }

    /// THE KNEE IS NOT A CONTROL. It follows the ratio.
    ///
    /// A gentle ratio wants a wide, soft knee — that combination is what
    /// "glue" means, a compressor that is always slightly working and
    /// never announces itself. A hard ratio wants a narrow one, because
    /// at 10:1 the point IS the corner: you are asking it to stop the
    /// signal, and a soft knee would start stopping it long before the
    /// threshold you set.
    ///
    /// One switch changing two things is the unit's own behaviour, not a
    /// simplification of it — and it lives here rather than in the node
    /// so the curve on screen is drawn from the same figure the audio
    /// uses. A display that quietly disagreed about the knee would be
    /// wrong exactly where a compressor is hardest to hear.
    pub fn knee_db(ratio_index: u32) -> f32 {
        match ratio_index {
            0 => 18.0,
            1 => 10.0,
            _ => 4.0,
        }
    }

    /// The most gain reduction [`RANGE`] will allow, in dB. At the top
    /// the cap is off the end of anything audible; at 0 the compressor
    /// is switched off in all but name.
    pub const RANGE_MAX_DB: f32 = 60.0;

    /// The sidechain high-pass corner at which the filter is OFF.
    ///
    /// A 6 dB/octave corner at 20 Hz takes nothing audible off a
    /// detector, so the bottom of the range IS the off position and the
    /// control needs no separate switch beside it.
    pub const SC_HP_OFF_HZ: f32 = 20.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: THRESHOLD,
            name: "threshold",
            min: -60.0,
            max: 10.0,
            // ZERO, so a freshly loaded compressor is very nearly a wire:
            // it costs the CPU but it does not change the mix until you
            // ask it to. The same rule the equaliser's off bands follow.
            default: 0.0,
        },
        ParamDef {
            id: RATIO,
            name: "ratio",
            min: 0.0,
            max: (RATIO_VALUES.len() - 1) as f32,
            // 4:1, the middle position and the one an SSL sits at.
            default: 1.0,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: 0.0,
            max: (ATTACK_MS.len() - 1) as f32,
            // 10 ms: slow enough to let a transient through, which is
            // what a BUS compressor is for. A fast attack on a mix bus
            // flattens the drums and everyone blames the compressor.
            default: 5.0,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: 0.0,
            max: (RELEASE_NAMES.len() - 1) as f32,
            // AUTO, the classic starting point: it is the setting that
            // makes the unit forgiving of material it has never heard.
            default: RELEASE_AUTO as f32,
        },
        ParamDef {
            id: MAKEUP,
            name: "makeup",
            // Negative is allowed, unlike the unit this models, because
            // there is no console fader after it here — a device that
            // can only go up leaves you leaving the card to come back
            // down.
            min: -12.0,
            max: 24.0,
            default: 0.0,
        },
        ParamDef {
            id: DRY_WET,
            name: "drywet",
            min: 0.0,
            max: 100.0,
            // Fully wet: parallel compression is a thing you ask for.
            default: 100.0,
        },
        ParamDef {
            id: RANGE,
            name: "range",
            min: 0.0,
            max: RANGE_MAX_DB,
            // Uncapped. The cap is how the unit's character is dialled
            // BACK, so it starts out of the way.
            default: RANGE_MAX_DB,
        },
        ParamDef {
            id: CLIP,
            name: "clip",
            min: 0.0,
            max: 1.0,
            // Off. A soft clipper that arrived switched on would change
            // the sound of every project that loads one.
            default: 0.0,
        },
        ParamDef {
            id: SC_HP,
            name: "schp",
            min: SC_HP_OFF_HZ,
            max: 2_000.0,
            // Off, at the bottom of its own range.
            default: SC_HP_OFF_HZ,
        },
    ];
}

/// `Node::Sat` — the saturator. A transfer curve, oversampled.
///
/// # The table speaks the KERNEL's units
///
/// Drive is `1..32` and bias `-0.9..0.9` because those are
/// [`crate::dsp::shaper::DRIVE_MIN`]/[`DRIVE_MAX`](crate::dsp::shaper::DRIVE_MAX)
/// and [`BIAS_MAX`](crate::dsp::shaper::BIAS_MAX) — the same numbers the
/// widget draws its curve with. The filter's drive knob is `0..1` and
/// converts through [`filter::shaper_drive`] because it is a filter
/// SEASONING; this device IS the shaper, so a second mapping between the
/// knob and the curve would be one more place for the drawing and the
/// audio to disagree. The agreement test depends on the clamps agreeing,
/// not merely the arithmetic.
/// The kick drum synth.
///
/// One voice, one shot, fixed order:
///
/// ```text
/// pitch env A (fast) ─┐
/// pitch env B (slow) ─┴─▶ sine ──▶ amp env ─┐
/// white noise ── click env ─────────────────┴─▶ DISPERSER ─▶ SATURATOR ─▶ out
/// ```
///
/// TWO pitch envelopes because a kick needs two different drops at once:
/// a very fast one, a few milliseconds, that is heard as the beater
/// hitting the skin, and a slow one, a few dozen, that is heard as the
/// body falling to its tuned note. One envelope can be either but not
/// both — set it fast and the body has no weight, set it slow and the
/// attack turns to a woolly swoop.
///
/// The DISPERSER is last before the saturator and tuned to a HARMONIC of
/// the note, so the phase smear it adds sits on the drum rather than
/// beside it. See [`dsp::filters::Disperser`](crate::dsp::filters::Disperser).
pub mod kick {
    use super::ParamDef;

    pub const TUNE: u32 = 0;
    pub const AMP_DECAY: u32 = 1;
    pub const PITCH_A_DEPTH: u32 = 2;
    pub const PITCH_A_DECAY: u32 = 3;
    pub const PITCH_B_DEPTH: u32 = 4;
    pub const PITCH_B_DECAY: u32 = 5;
    pub const CLICK_LEVEL: u32 = 6;
    pub const CLICK_DECAY: u32 = 7;
    pub const DISP_STAGES: u32 = 8;
    pub const DISP_HARMONIC: u32 = 9;
    pub const DISP_Q: u32 = 10;
    pub const DRIVE: u32 = 11;
    pub const GAIN: u32 = 12;

    /// The tuned fundamental's window, in Hz. The bottom is below what
    /// most systems reproduce and the top is where a kick stops being one
    /// — a range wide enough for an 808 and a techno thump both.
    pub const TUNE_MIN: f32 = 20.0;
    pub const TUNE_MAX: f32 = 200.0;

    /// How far a pitch envelope can throw the fundamental, in semitones.
    ///
    /// The brief's kick recipe asks for ±48, and the fast envelope wants
    /// most of it: a 36-semitone drop from 50 Hz reaches 400 Hz, which is
    /// the click of a beater rather than a pitch.
    pub const PITCH_DEPTH_MAX: f32 = 48.0;

    /// The most allpass sections the disperser will run, as a float for
    /// the table. Mirrors the kernel's own cap so the knob cannot ask for
    /// a stage that does not exist.
    pub const DISP_STAGES_MAX: f32 = crate::dsp::filters::DISPERSER_MAX_STAGES as f32;

    /// Which harmonic of the tuned note the disperser sits on.
    ///
    /// 1 is the fundamental, where the smear is longest and reads as a
    /// pitch drop; higher harmonics move it up into the click, where it
    /// reads as a metallic zip. Integer harmonics rather than a free
    /// frequency because the point is to stay MUSICALLY attached to the
    /// drum — a free knob is a knob you have to re-tune every time the
    /// kick moves.
    pub const DISP_HARMONIC_MAX: f32 = 8.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: TUNE,
            name: "tune",
            min: TUNE_MIN,
            max: TUNE_MAX,
            // Around the low G that most kicks sit near.
            default: 50.0,
        },
        ParamDef {
            id: AMP_DECAY,
            name: "decay",
            min: 20.0,
            max: 2_000.0,
            default: 320.0,
        },
        ParamDef {
            id: PITCH_A_DEPTH,
            name: "punchdepth",
            min: 0.0,
            max: PITCH_DEPTH_MAX,
            default: 32.0,
        },
        ParamDef {
            id: PITCH_A_DECAY,
            name: "punchtime",
            // Down to a quarter of a millisecond: at the bottom this is a
            // click rather than a pitch drop, which is the point.
            min: 0.25,
            max: 60.0,
            default: 6.0,
        },
        ParamDef {
            id: PITCH_B_DEPTH,
            name: "sweepdepth",
            min: 0.0,
            max: PITCH_DEPTH_MAX,
            default: 12.0,
        },
        ParamDef {
            id: PITCH_B_DECAY,
            name: "sweeptime",
            min: 5.0,
            max: 500.0,
            default: 55.0,
        },
        ParamDef {
            id: CLICK_LEVEL,
            name: "click",
            min: 0.0,
            max: 1.0,
            default: 0.35,
        },
        ParamDef {
            id: CLICK_DECAY,
            // VERY short: the top of the range is still under a fiftieth
            // of a second. A noise burst longer than that stops being a
            // click and starts being a snare.
            name: "clicktime",
            min: 0.2,
            max: 20.0,
            default: 2.0,
        },
        ParamDef {
            id: DISP_STAGES,
            name: "disperse",
            min: 0.0,
            max: DISP_STAGES_MAX,
            // Off by default: the disperser is the flavour, not the drum.
            default: 0.0,
        },
        ParamDef {
            id: DISP_HARMONIC,
            name: "harmonic",
            min: 1.0,
            max: DISP_HARMONIC_MAX,
            default: 2.0,
        },
        ParamDef {
            id: DISP_Q,
            name: "spread",
            min: 0.3,
            max: 12.0,
            default: 2.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.9,
        },
    ];
}

/// The snare drum synth (`Node::Snare`).
///
/// A snare is TWO instruments struck at once and mixed: a tuned shell,
/// which is two detuned membrane modes, and the wire snares underneath,
/// which are band-passed noise. Every row here belongs to one or the
/// other, and the SNAP row is the balance between them — the single knob
/// that walks a drum from a tom to a rimshot.
pub mod snare {
    use super::ParamDef;

    pub const TUNE: u32 = 0;
    pub const RATIO: u32 = 1;
    pub const TONE_DECAY: u32 = 2;
    pub const BEND: u32 = 3;
    pub const BEND_TIME: u32 = 4;
    pub const SNAP: u32 = 5;
    pub const SNAP_DECAY: u32 = 6;
    pub const NOISE_TONE: u32 = 7;
    pub const NOISE_Q: u32 = 8;
    pub const DRIVE: u32 = 9;
    pub const GAIN: u32 = 10;

    /// The shell's fundamental, in Hz. A 14" snare sits near 180 and a
    /// piccolo near 300; the bottom of the range is a floor tom's
    /// territory and the top is a rim.
    pub const TUNE_MIN: f32 = 90.0;
    pub const TUNE_MAX: f32 = 400.0;

    /// How far above the fundamental the SECOND shell mode sits.
    ///
    /// A real drumhead's modes are not harmonic — the second circular
    /// mode of an ideal membrane is 1.59 times the first, not 2. That
    /// inharmonicity is why a snare reads as a drum rather than a pitch,
    /// and why this is a free ratio instead of a harmonic count. The
    /// default is the 180/330 pair most drum machines shipped.
    pub const RATIO_MIN: f32 = 1.0;
    pub const RATIO_MAX: f32 = 3.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: TUNE,
            name: "tune",
            min: TUNE_MIN,
            max: TUNE_MAX,
            default: 180.0,
        },
        ParamDef {
            id: RATIO,
            name: "ratio",
            min: RATIO_MIN,
            max: RATIO_MAX,
            default: 1.833,
        },
        ParamDef {
            id: TONE_DECAY,
            name: "shell",
            min: 20.0,
            max: 800.0,
            default: 120.0,
        },
        ParamDef {
            id: BEND,
            name: "bend",
            min: 0.0,
            max: 24.0,
            default: 6.0,
        },
        ParamDef {
            id: BEND_TIME,
            name: "bendtime",
            min: 1.0,
            max: 100.0,
            default: 20.0,
        },
        ParamDef {
            id: SNAP,
            name: "snap",
            min: 0.0,
            max: 1.0,
            default: 0.7,
        },
        ParamDef {
            id: SNAP_DECAY,
            // LONGER than the shell by default, and that is the sound: a
            // snare's wires rattle on after the head has stopped, and a
            // noise decay shorter than the shell's is a tom with a hiss
            // on the front.
            name: "snaptime",
            min: 20.0,
            max: 1_200.0,
            default: 180.0,
        },
        ParamDef {
            id: NOISE_TONE,
            name: "noise",
            min: 300.0,
            max: 8_000.0,
            default: 1_800.0,
        },
        ParamDef {
            id: NOISE_Q,
            name: "width",
            min: 0.3,
            max: 8.0,
            default: 0.9,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.9,
        },
    ];
}

/// The tom synth (`Node::Tom`).
///
/// The simplest drum in the rack, and deliberately: one sine, one pitch
/// bend, one short noise attack for the stick, one lowpass for the skin.
/// A tom that needs more rows than this is a kick with the wrong name.
pub mod tom {
    use super::ParamDef;

    pub const TUNE: u32 = 0;
    pub const DECAY: u32 = 1;
    pub const BEND: u32 = 2;
    pub const BEND_TIME: u32 = 3;
    pub const STICK: u32 = 4;
    pub const STICK_DECAY: u32 = 5;
    pub const TONE: u32 = 6;
    pub const DRIVE: u32 = 7;
    pub const GAIN: u32 = 8;

    /// The whole tom family in one range: a 16" floor tom at the bottom,
    /// a rack tom in the middle, a high timbale at the top.
    pub const TUNE_MIN: f32 = 40.0;
    pub const TUNE_MAX: f32 = 400.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: TUNE,
            name: "tune",
            min: TUNE_MIN,
            max: TUNE_MAX,
            default: 120.0,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: 40.0,
            max: 2_000.0,
            default: 400.0,
        },
        ParamDef {
            id: BEND,
            // SMALL by default. A tom bends — every struck drum does —
            // but a tom with a kick's 32 semitones on it is a kick.
            name: "bend",
            min: 0.0,
            max: 24.0,
            default: 5.0,
        },
        ParamDef {
            id: BEND_TIME,
            name: "bendtime",
            min: 5.0,
            max: 200.0,
            default: 40.0,
        },
        ParamDef {
            id: STICK,
            name: "stick",
            min: 0.0,
            max: 1.0,
            default: 0.15,
        },
        ParamDef {
            id: STICK_DECAY,
            name: "sticktime",
            min: 1.0,
            max: 80.0,
            default: 8.0,
        },
        ParamDef {
            id: TONE,
            name: "tone",
            min: 200.0,
            max: 12_000.0,
            default: 4_000.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.9,
        },
    ];
}

/// The 808 hi-hat (`Node::Hat`).
///
/// Modelled on the TR-808's actual circuit rather than on the usual
/// filtered-noise approximation, because the two do not sound alike. The
/// 808 has no noise source in its hat at all: it sums SIX SQUARE
/// OSCILLATORS at fixed, mutually inharmonic frequencies, and the metallic
/// clang everyone recognises is those six squares beating against each
/// other. Filtered white noise gives a "tss" with no pitch in it; this
/// gives the 808's.
///
/// The six frequencies are the machine's own, set by its oscillator
/// bank's timing components: see [`RATIOS`].
pub mod hat {
    use super::ParamDef;

    pub const TUNE: u32 = 0;
    pub const CLOSED_DECAY: u32 = 1;
    pub const OPEN_DECAY: u32 = 2;
    pub const BP_HZ: u32 = 3;
    pub const BP_Q: u32 = 4;
    pub const HP_HZ: u32 = 5;
    pub const DRIVE: u32 = 6;
    pub const GAIN: u32 = 7;

    /// The TR-808's six hi-hat oscillator frequencies, in Hz.
    ///
    /// These are the measured free-running frequencies of the machine's
    /// six square-wave oscillators. They are NOT harmonically related and
    /// that is the entire point — six harmonics would sum to a buzzy saw,
    /// while six inharmonic squares sum to metal. Changing one of these
    /// numbers is changing which machine this is.
    pub const RATIOS: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

    /// The tune knob, as a multiplier on the whole bank.
    ///
    /// A multiplier rather than six knobs, and rather than a frequency:
    /// the six ratios ARE the instrument, and anything that can move them
    /// against each other is a knob that can turn an 808 into something
    /// else. 1.0 is the machine exactly, and it is the default.
    pub const TUNE_MIN: f32 = 0.5;
    pub const TUNE_MAX: f32 = 2.0;

    /// The note that opens the hat: GM's A#1, open hi-hat.
    ///
    /// Below it — F#1 closed, G#1 pedal — the short envelope plays. The
    /// 808's own panel had two buttons and one voice; the note is how a
    /// sequence says which button.
    pub const OPEN_NOTE: u8 = 46;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: TUNE,
            name: "tune",
            min: TUNE_MIN,
            max: TUNE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: CLOSED_DECAY,
            name: "closed",
            min: 10.0,
            max: 400.0,
            default: 55.0,
        },
        ParamDef {
            id: OPEN_DECAY,
            name: "open",
            min: 50.0,
            max: 3_000.0,
            default: 500.0,
        },
        ParamDef {
            id: BP_HZ,
            // The 808's hat band sits around 10 kHz. Below about 6 the
            // squares stop being metal and start being a buzz.
            name: "band",
            min: 2_000.0,
            max: 16_000.0,
            default: 10_000.0,
        },
        ParamDef {
            id: BP_Q,
            name: "width",
            min: 0.5,
            max: 12.0,
            default: 2.0,
        },
        ParamDef {
            id: HP_HZ,
            name: "hp",
            min: 1_000.0,
            max: 12_000.0,
            default: 7_000.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.9,
        },
    ];
}

/// The hand clap (`Node::Handclap`).
///
/// `handclap`, not `clap`, and deliberately: CLAP in this codebase is the
/// PLUGIN FORMAT the host loads through clack. A module named `clap` next
/// to a plugin host that scans for CLAP plugins is a name that costs
/// somebody an afternoon.
///
/// A clap is not one sound. It is several hands not quite together,
/// followed by the room they are in — and that is exactly how it is
/// built: a short burst of band-passed noise retriggered a few times a
/// few milliseconds apart, over one longer decaying tail of the same
/// noise. The unevenness of the burst spacing is the whole realism.
pub mod handclap {
    use super::ParamDef;

    pub const BURSTS: u32 = 0;
    pub const SPREAD: u32 = 1;
    pub const BURST_DECAY: u32 = 2;
    pub const BODY: u32 = 3;
    pub const BODY_DECAY: u32 = 4;
    pub const TONE: u32 = 5;
    pub const WIDTH: u32 = 6;
    pub const HP_HZ: u32 = 7;
    pub const DRIVE: u32 = 8;
    pub const GAIN: u32 = 9;

    /// The most hands the clap will stack.
    pub const BURSTS_MAX: f32 = 4.0;

    /// WHERE each burst falls, as a multiple of the spread.
    ///
    /// Not `0, 1, 2, 3`. Evenly spaced bursts sum to a flam — a machine
    /// gun, audibly periodic — because the ear hears equal intervals as a
    /// rhythm however short they are. Real hands are progressively closer
    /// together as they converge, so the gaps SHRINK: these offsets are
    /// the classic uneven pattern, and they are why this reads as one
    /// clap rather than four taps.
    pub const OFFSETS: [f32; 4] = [0.0, 1.0, 1.9, 2.7];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: BURSTS,
            name: "hands",
            min: 1.0,
            max: BURSTS_MAX,
            default: 3.0,
        },
        ParamDef {
            id: SPREAD,
            name: "spread",
            min: 2.0,
            max: 40.0,
            default: 10.0,
        },
        ParamDef {
            id: BURST_DECAY,
            name: "snap",
            min: 1.0,
            max: 60.0,
            default: 6.0,
        },
        ParamDef {
            id: BODY,
            name: "body",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
        ParamDef {
            id: BODY_DECAY,
            name: "tail",
            min: 50.0,
            max: 1_500.0,
            default: 280.0,
        },
        ParamDef {
            id: TONE,
            // The clap band: around 1 kHz, which is where a hand's slap
            // actually lives. Higher reads as a rimshot, lower as a thud.
            name: "tone",
            min: 300.0,
            max: 4_000.0,
            default: 1_000.0,
        },
        ParamDef {
            id: WIDTH,
            name: "width",
            min: 0.3,
            max: 8.0,
            default: 1.1,
        },
        ParamDef {
            id: HP_HZ,
            name: "hp",
            min: 100.0,
            max: 2_000.0,
            default: 500.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.9,
        },
    ];
}

/// Modulato: chorus, flanger and vibrato, which are one effect.
///
/// See `audio::modulato` for why they are one, and for what `mode`
/// actually does — it picks the base delay's WINDOW, because a single
/// knob spanning a flanger's tenth of a millisecond and a chorus's
/// thirty is a knob that can tune neither.
pub mod modulato {
    use super::ParamDef;

    pub const MODE: u32 = 0;
    pub const RATE: u32 = 1;
    pub const DEPTH: u32 = 2;
    pub const DELAY: u32 = 3;
    pub const FEEDBACK: u32 = 4;
    pub const SPREAD: u32 = 5;
    pub const MIX: u32 = 6;

    pub const MODE_CHORUS: f32 = 0.0;
    pub const MODE_FLANGER: f32 = 1.0;
    pub const MODE_VIBRATO: f32 = 2.0;
    pub const MODE_MAX: f32 = MODE_VIBRATO;

    /// The names the card's mode strip shows, indexed by the wire value.
    pub const MODE_NAMES: &[&str] = &["chorus", "flanger", "vibrato"];

    /// The deepest swing and the longest base delay, in milliseconds.
    /// The delay lines are sized for their sum.
    pub const DEPTH_MAX_MS: f32 = 10.0;
    pub const DELAY_MAX_MS: f32 = 40.0;

    /// The base delay's window for a mode, in milliseconds.
    ///
    /// UNDER TEN is where a delayed copy comb-filters the dry one, which
    /// is a flanger; past ten it is heard as a second voice, which is a
    /// chorus. Vibrato sits in between because it has no dry signal to
    /// beat against and only wants to stay one voice.
    pub fn window(mode: f32) -> (f32, f32) {
        match mode.round() {
            m if m == MODE_FLANGER => (0.2, 10.0),
            m if m == MODE_VIBRATO => (1.0, 15.0),
            _ => (8.0, DELAY_MAX_MS),
        }
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MODE,
            name: "mode",
            min: 0.0,
            max: MODE_MAX,
            default: MODE_CHORUS,
        },
        ParamDef {
            id: RATE,
            name: "rate",
            // Down to a cycle every fifty seconds, which is a slow drift
            // rather than an effect you can hear moving; up to where it
            // stops being modulation and starts being a sideband.
            min: 0.02,
            max: 20.0,
            default: 0.8,
        },
        ParamDef {
            id: DEPTH,
            name: "depth",
            min: 0.0,
            max: DEPTH_MAX_MS,
            default: 2.5,
        },
        ParamDef {
            id: DELAY,
            // A POSITION IN THE MODE'S WINDOW, `0..=1`, not a time — see
            // `window` above. The card prints the millisecond figure it
            // resolves to, so the reading is still honest.
            name: "delay",
            min: 0.0,
            max: 1.0,
            default: 0.35,
        },
        ParamDef {
            id: FEEDBACK,
            // BIPOLAR: negative feedback inverts the comb, so the
            // notches land where the peaks were. That is the difference
            // between the two flanger sounds everyone knows, and it
            // costs one sign.
            name: "feedback",
            min: -0.9,
            max: 0.9,
            default: 0.0,
        },
        ParamDef {
            id: SPREAD,
            // The right oscillator's phase offset, in TURNS. Half a turn
            // is opposition, which is as wide as it goes.
            name: "spread",
            min: 0.0,
            max: 0.5,
            default: 0.25,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
    ];
}

/// The character limiter (`Node::Limiter`).
///
/// NOT a transparent one, and that is the whole design. A limiter that
/// only prevents peaks is a safety device, and a safety device is
/// something you put on and forget; this is something you put on because
/// you want what it does. It makes things louder, it stops them getting
/// loud, and on the way it adds a little warmth, a little less high-end
/// fuzz, and a slew-driven lift that puts back the edge the gain
/// reduction takes off.
///
/// # Why there is no output trim
///
/// Because the CEILING is the output. A trim after the clipper would be
/// the one control able to push the signal back over the ceiling, which
/// would turn the device's one absolute promise — nothing leaves here
/// above the ceiling — into a promise with an asterisk. Drive with
/// [`PUSH`], land with [`CEILING`], and the guarantee holds at every
/// setting.
pub mod limiter {
    use super::ParamDef;

    pub const PUSH: u32 = 0;
    pub const CEILING: u32 = 1;
    pub const STYLE: u32 = 2;
    pub const RELEASE: u32 = 3;
    pub const WARMTH: u32 = 4;
    pub const FUZZ: u32 = 5;
    pub const BRIGHTEN: u32 = 6;

    /// How far into the ceiling the input can be driven, in dB.
    ///
    /// 24 dB is a lot, deliberately: this is the loudness control, and
    /// the interesting settings on a character limiter are the ones where
    /// it is working. A 6 dB range would make it a safety device again.
    pub const PUSH_MAX_DB: f32 = 24.0;

    /// The ceiling's window, in dBFS. Never above 0 — the point of a
    /// ceiling is that it is one.
    pub const CEILING_MIN_DB: f32 = -12.0;
    pub const CEILING_MAX_DB: f32 = 0.0;

    /// The styles, in wire order. Indices, as every other switch here is.
    pub const STYLE_WARM: u32 = 0;
    pub const STYLE_PUNCH: u32 = 1;
    pub const STYLE_SMASH: u32 = 2;
    pub const STYLE_MAX: u32 = STYLE_SMASH;

    /// What each style does to the release, as a MULTIPLIER on the knob.
    ///
    /// The style changes the recovery and nothing else, because the
    /// attack cannot move: it rides the lookahead, and the lookahead is
    /// the device's reported latency. A style that changed the latency
    /// would slide the track in time when you picked it off a menu.
    ///
    /// - `warm` recovers slowly, so the gain sits still and the device
    ///   reads as level rather than as movement.
    /// - `punch` is the knob as written.
    /// - `smash` recovers fast enough to pump audibly, which on the right
    ///   material is the effect people reach for a limiter to get.
    pub const STYLE_RELEASE_SCALE: &[f32] = &[2.5, 1.0, 0.35];
    pub const STYLE_NAMES: &[&str] = &["warm", "punch", "smash"];

    /// The style a wire index names. Out of range gives `punch`, because
    /// a stale index must still limit.
    pub fn style_release_scale(index: u32) -> f32 {
        STYLE_RELEASE_SCALE
            .get(index as usize)
            .copied()
            .unwrap_or(1.0)
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: PUSH,
            name: "push",
            min: 0.0,
            max: PUSH_MAX_DB,
            // Already working when you load it. The saturator's rule, for
            // the saturator's reason: you add a character limiter because
            // you want the character, and one that does nothing until you
            // turn a knob is a different kind of surprise.
            default: 3.0,
        },
        ParamDef {
            id: CEILING,
            name: "ceiling",
            min: CEILING_MIN_DB,
            max: CEILING_MAX_DB,
            // Not 0.0: a true-peak reconstruction of a signal sitting
            // exactly at full scale can overshoot a converter, and a
            // third of a dB is the cheapest insurance in audio.
            default: -0.3,
        },
        ParamDef {
            id: STYLE,
            name: "style",
            min: 0.0,
            max: STYLE_MAX as f32,
            default: STYLE_PUNCH as f32,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: 20.0,
            max: 1_000.0,
            default: 180.0,
        },
        ParamDef {
            id: WARMTH,
            name: "warmth",
            min: 0.0,
            max: 1.0,
            // SUBTLE, and the brief's word. This is the asymmetry that
            // makes even harmonics; past about half it stops being warmth
            // and starts being a fuzz box, so the default sits below.
            default: 0.35,
        },
        ParamDef {
            id: FUZZ,
            name: "fuzz",
            min: 0.0,
            max: 1.0,
            // Subtler again than the warmth, and deliberately: this one
            // lives in the top octaves where the ear is least forgiving.
            default: 0.15,
        },
        ParamDef {
            id: BRIGHTEN,
            name: "brighten",
            min: 0.0,
            max: 1.0,
            default: 0.3,
        },
    ];
}

pub mod sat {
    use super::ParamDef;

    pub const MODE: u32 = 0;
    pub const DRIVE: u32 = 1;
    pub const BIAS: u32 = 2;
    pub const MIX: u32 = 3;
    pub const OUT: u32 = 4;

    /// Mode indices on the wire — the order of `dsp::shaper::Mode`'s five
    /// shapes, and of the widget's mode strip. Indices, as
    /// [`filter::MODE`](super::filter::MODE) is.
    pub const MODE_HARD: u32 = 0;
    pub const MODE_SOFT: u32 = 1;
    pub const MODE_CUBIC: u32 = 2;
    pub const MODE_FOLD: u32 = 3;
    pub const MODE_CRUSH: u32 = 4;
    pub const MODE_MAX: u32 = MODE_CRUSH;

    /// The output trim's window, in dB, and the same figures as linear
    /// gain. Both forms are written down because the TABLE is in linear
    /// gain and the KNOB is in dB, and a widget deriving one from the
    /// other by hand is how the two ends of one range drift apart.
    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    /// `10^(-24/20)` and `10^(12/20)`, to f32 precision.
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MODE,
            name: "mode",
            min: 0.0,
            max: MODE_MAX as f32,
            default: MODE_SOFT as f32,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            // Unity gain into the curve — which is NOT the identity for
            // four of the five shapes (`SoftClip` at drive 1 is still
            // `tanh`, about 3% down at a normal level). Deliberately: the
            // reverb parks its default out of the way because a reverb
            // you did not ask for drowns a mix, and a gentle tanh does
            // not. You add a saturator because you want the character;
            // loading one that does nothing at all until you turn a knob
            // is a different kind of surprise.
            //
            // The identity case still EXISTS and is still tested —
            // `MODE_HARD` at this drive is a wire inside the rails, which
            // is what proves the oversampler round trip is unity gain and
            // phase-clean.
            default: crate::dsp::shaper::DRIVE_MIN,
        },
        ParamDef {
            id: BIAS,
            name: "bias",
            // Symmetric, because the kernel clamps symmetrically: bias
            // shifts the curve either way off centre, and which way is a
            // taste, not a magnitude.
            min: -crate::dsp::shaper::BIAS_MAX,
            max: crate::dsp::shaper::BIAS_MAX,
            default: 0.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Fully wet. A parallel blend is a choice you reach for —
            // it is how you keep a transient while crushing everything
            // under it — not the state you load into; a device that
            // arrives half-bypassed reads as broken before it reads as
            // subtle.
            default: 1.0,
        },
        ParamDef {
            id: OUT,
            name: "out",
            // Linear gain over exactly [`OUT_MIN_DB`]..[`OUT_MAX_DB`].
            // Saturation is a level change as much as a timbre change,
            // and the trim is how you A/B the two without reaching for
            // the fader behind it.
            //
            // The floor is -24 dB and NOT silence, deliberately: this row
            // is a trim, the track already has a fader and a mute, and a
            // range whose bottom is an infinite drop spends most of a
            // knob's travel — and most of an automation lane's useful
            // resolution — on the last inaudible decibel.
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];

    /// Mode index -> the kernel's shape, clamped. The ONE place the wire
    /// index becomes a curve — the widget's mode strip is the same list
    /// in the same order, so the drawing and the audio pick the same
    /// shape from the same number.
    pub fn mode(index: u32) -> crate::dsp::shaper::Mode {
        use crate::dsp::shaper::Mode;
        match index.min(MODE_MAX) {
            MODE_HARD => Mode::HardClip,
            MODE_CUBIC => Mode::Cubic,
            MODE_FOLD => Mode::Fold,
            MODE_CRUSH => Mode::Crush,
            _ => Mode::SoftClip,
        }
    }
}

/// `Node::Echo` — the analogue delay.
///
/// # Why the node is `Echo` and the device is "delay"
///
/// [`NodeSpec::Delay`](crate::audio::graph::NodeSpec::Delay) was taken
/// years-of-commits ago by plugin delay compensation: a pure wire that
/// arrives late, inserted by COMPILE and never by a user. Two different
/// things called Delay in one graph is a bug waiting for a careless
/// match arm, so the musical one is `Echo` in the engine and "delay" on
/// its face — the user-facing word is the widget's business.
///
/// # Time is either a division or a number, never both
///
/// [`SYNC`] picks which. At index 0 the echo is FREE and [`TIME`] is
/// read as milliseconds; at any other index the echo is locked to the
/// transport and [`TIME`] is ignored — the division is converted against
/// the tempo the segment is actually playing at, so a tempo ramp drags
/// the repeats with it.
pub mod echo {
    use super::ParamDef;

    pub const SYNC: u32 = 0;
    pub const TIME: u32 = 1;
    pub const FEEDBACK: u32 = 2;
    pub const TONE: u32 = 3;
    pub const DRIVE: u32 = 4;
    pub const WOW: u32 = 5;
    pub const SPREAD: u32 = 6;
    pub const MIX: u32 = 7;
    pub const SEND: u32 = 8;

    /// The longest echo the buffer is built for, in milliseconds.
    ///
    /// A hard ceiling and not a suggestion: the delay memory is
    /// allocated once at compile, in the green zone, and nothing in the
    /// red zone may ask for more than it. Four seconds covers a free
    /// time at its maximum and a whole bar at any tempo down to 60 BPM;
    /// slower than that, a synced whole-note clamps and the repeats come
    /// back early rather than the callback reaching for memory.
    pub const MAX_MS: f32 = 4_000.0;

    /// Sync divisions, in BEATS. Index 0 is free-running and has no
    /// division, which is why this list is indexed from `SYNC - 1`.
    ///
    /// Beats, not note names, because that is the unit the transport
    /// speaks: `ctx.beats_per_sample` converts one of these into samples
    /// with a single divide and no table of tempo maths.
    pub const DIVISION_BEATS: &[f32] = &[
        4.0,       // 1/1
        2.0,       // 1/2
        1.0,       // 1/4
        0.5,       // 1/8
        0.25,      // 1/16
        2.0 / 3.0, // 1/4T
        1.0 / 3.0, // 1/8T
        1.5,       // 1/4.
        0.75,      // 1/8.
    ];

    /// What the sync switch says. Index 0 is free; the rest line up with
    /// [`DIVISION_BEATS`], so the strip and the arithmetic cannot name
    /// different divisions.
    pub const SYNC_NAMES: &[&str] = &[
        "free", "1/1", "1/2", "1/4", "1/8", "1/16", "1/4t", "1/8t", "1/4.", "1/8.",
    ];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: SYNC,
            name: "sync",
            min: 0.0,
            max: (SYNC_NAMES.len() - 1) as f32,
            // An eighth: the division a delay is reached for most, and
            // audibly a delay rather than a doubler the moment it loads.
            default: 4.0,
        },
        ParamDef {
            id: TIME,
            name: "time",
            // Two milliseconds is where a delay stops being an echo and
            // starts being a comb filter — worth having, and the floor
            // the kernel's own fractional read enforces anyway.
            min: 2.0,
            max: MAX_MS,
            default: 350.0,
        },
        ParamDef {
            id: FEEDBACK,
            name: "feedback",
            min: 0.0,
            // The kernel's ceiling, as a percentage. At 99% a repeat is
            // "almost forever" and still provably decays; 100% would
            // integrate the buffer into the rails.
            max: 99.0,
            default: 35.0,
        },
        ParamDef {
            id: TONE,
            name: "tone",
            // The damping corner INSIDE the loop, so each trip loses
            // everything above it again. Down at 200 Hz the third repeat
            // is a thud; wide open it is a digital delay.
            min: 200.0,
            max: 20_000.0,
            default: 4_500.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 0.0,
            max: 100.0,
            // Clean on arrival: drive 0 is bit-exact bypass in the
            // kernel, so a delay you have not dirtied is arithmetically
            // the plain one.
            default: 0.0,
        },
        ParamDef {
            id: WOW,
            name: "wow",
            min: 0.0,
            max: 100.0,
            // A trace of movement by default. Dead-still repeats are the
            // one thing that never sounds like tape, and at this depth it
            // reads as warmth rather than as an effect.
            default: 12.0,
        },
        ParamDef {
            id: SPREAD,
            name: "spread",
            min: 0.0,
            max: 100.0,
            // How much later the right channel repeats than the left, as
            // a percentage of the echo time. A stereo picture from one
            // control, and 0 is a mono-compatible delay.
            default: 0.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 100.0,
            // Audible, never a wash — the reverb's rule. A delay that
            // arrives louder than the track it is on is a delay nobody
            // asked for.
            default: 30.0,
        },
        ParamDef {
            id: SEND,
            name: "send",
            min: 0.0,
            max: 100.0,
            // ZERO IS A TOPOLOGY, not just a quiet send. At 0 the delay
            // is an ordinary insert: the whole track runs through it and
            // `mix` blends. Above 0 it leaves the chain and becomes an
            // aux — the track is tapped at this level, post-fader, and
            // the delay's output returns to the master beside the dry.
            //
            // 0 is therefore also the only default that leaves every
            // project written before this parameter existed sounding
            // exactly as it did.
            default: 0.0,
        },
    ];

    /// The echo time in SAMPLES for this segment.
    ///
    /// One function, called by the node and by nothing else, so the
    /// free and synced cases cannot disagree about what "time" means.
    /// `beats_per_sample` comes from the segment being rendered, so a
    /// tempo change moves the repeats with it.
    ///
    /// Clamped to what the buffer was built for at both ends. A synced
    /// whole-note at a very slow tempo asks for more than [`MAX_MS`],
    /// and the honest answer in the red zone is an echo that comes back
    /// early — never a read past the end of the memory.
    pub fn time_samples(sync: u32, time_ms: f32, sample_rate: f32, beats_per_sample: f64) -> f32 {
        let max = MAX_MS * 1e-3 * sample_rate;
        let samples = if sync == 0 {
            time_ms * 1e-3 * sample_rate
        } else {
            let beats = DIVISION_BEATS
                .get((sync - 1) as usize)
                .copied()
                .unwrap_or(1.0);
            // beats / (beats per sample) = samples. A stopped transport
            // reports zero beats per sample; fall back to the free time
            // rather than dividing by zero.
            if beats_per_sample > 0.0 {
                (f64::from(beats) / beats_per_sample) as f32
            } else {
                time_ms * 1e-3 * sample_rate
            }
        };
        if samples.is_finite() {
            samples.clamp(2.0, max)
        } else {
            2.0
        }
    }
}

/// `Node::Poly` — the workhorse synth of `notes/20260825-synth-brief.md`.
///
/// # Naming convention
///
/// A name here is `<group> <label>`: the group is the section of the
/// voice path the parameter belongs to, the label is what the knob says.
/// Names must be unique across a device, but a knob standing in a well
/// already titled "osc a" must not repeat itself — so the table carries
/// the full, automation-facing name and the widget shows everything after
/// the first space. One string, both jobs, and `poly::label` is the only
/// place that knows the split.
///
/// # The table speaks the unit the knob shows
///
/// Percent parameters run `0..100`, times are ms, frequencies Hz,
/// resonance is a Q on the Filter node's scale, gain is linear. A widget
/// `Param` built from a row uses the row's range AS its mapping, so
/// `Param::value()` is always the engine-facing number and there is no
/// per-parameter bridging step to get wrong. Where a kernel wants a
/// different form — `shaper_drive` wants `0..=1`, not percent — the node
/// converts, once, at the point of use.
///
/// # Discrete parameters are INDICES
///
/// Wave, octave, mode, slope, drive position, voice mode and unison ride
/// the wire as indices into the widget's choice list, exactly as
/// [`filter::MODE`] and [`filter::SLOPE`] do. Where the index is not the
/// value, the offset is written down beside it — there is no second
/// convention to remember, only the one arithmetic step.
pub mod poly {
    use super::ParamDef;

    // osc a
    pub const A_WAVE: u32 = 0;
    pub const A_OCT: u32 = 1;
    pub const A_SEMI: u32 = 2;
    pub const A_FINE: u32 = 3;
    pub const A_LEVEL: u32 = 4;
    pub const A_PENV: u32 = 5;
    // osc b
    pub const B_WAVE: u32 = 6;
    pub const B_OCT: u32 = 7;
    pub const B_SEMI: u32 = 8;
    pub const B_FINE: u32 = 9;
    pub const B_LEVEL: u32 = 10;
    pub const B_PENV: u32 = 11;
    // noise
    pub const N_COLOR: u32 = 12;
    pub const N_LEVEL: u32 = 13;
    pub const N_DECAY: u32 = 14;
    // filter
    pub const F_MODE: u32 = 15;
    pub const F_SLOPE: u32 = 16;
    pub const F_CUTOFF: u32 = 17;
    pub const F_RES: u32 = 18;
    pub const F_ENV: u32 = 19;
    pub const F_KEY: u32 = 20;
    pub const F_DRIVE: u32 = 21;
    pub const F_POS: u32 = 22;
    // amp
    pub const AMP_A: u32 = 23;
    pub const AMP_D: u32 = 24;
    pub const AMP_S: u32 = 25;
    pub const AMP_R: u32 = 26;
    pub const GAIN: u32 = 27;
    pub const VEL: u32 = 28;
    // voices
    pub const V_MODE: u32 = 29;
    pub const V_GLIDE: u32 = 30;
    pub const V_UNISON: u32 = 31;
    pub const V_DETUNE: u32 = 32;
    pub const V_SPREAD: u32 = 33;
    // filter envelope — its OWN times, not the amp's
    pub const FENV_A: u32 = 34;
    pub const FENV_D: u32 = 35;
    pub const FENV_S: u32 = 36;
    pub const FENV_R: u32 = 37;
    // pitch envelope — a one-shot decay, the kick's clock
    pub const PENV_D: u32 = 38;
    // the audio-rate mod matrix: four wires of (source, dest, depth)
    pub const W1_SRC: u32 = 39;
    pub const W1_DST: u32 = 40;
    pub const W1_AMT: u32 = 41;
    pub const W2_SRC: u32 = 42;
    pub const W2_DST: u32 = 43;
    pub const W2_AMT: u32 = 44;
    pub const W3_SRC: u32 = 45;
    pub const W3_DST: u32 = 46;
    pub const W3_AMT: u32 = 47;

    /// The four `MipOsc` shapes, then the four sparse spectral tables.
    /// Index order IS the wire order of [`A_WAVE`] and [`B_WAVE`].
    pub const WAVES: &[&str] = &[
        "sine", "tri", "saw", "square", "bell", "glass", "metal", "air",
    ];
    pub const NOISE_COLORS: &[&str] = &["white", "pink"];
    /// Whether the drive stage sits before or after the filter.
    pub const DRIVE_POS: &[&str] = &["pre", "post"];
    pub const VOICE_MODES: &[&str] = &["poly", "mono", "legato"];
    /// Octave transpose choices. An octave is a discrete musical fact, so
    /// it rides as an index rather than as a continuous number a knob
    /// could land between: `octave = index - OCT_CENTER`.
    pub const OCTAVES: &[&str] = &["-4", "-3", "-2", "-1", "0", "+1", "+2", "+3", "+4"];
    /// Index of "no transpose" in [`OCTAVES`].
    pub const OCT_CENTER: u32 = 4;
    /// Unison voices per note: `voices = index + 1`.
    pub const UNISON: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8"];

    /// Wires the matrix holds. Fixed, because the wire rows are param
    /// rows: automation, projects and letters all address them the same
    /// way they address a knob, which is the entire reason the matrix is
    /// in the TABLE rather than in a side structure only the UI knows.
    ///
    /// THREE, down from four (2026-08-26): the matrix presents as a 3×3
    /// grid — three wires of (src, dst, depth) — and a fourth wire the
    /// grid could not show would be a knob without a face. A project
    /// saved with four loads with its fourth wire dropped.
    pub const WIRES: usize = 3;
    /// `(src, dst, depth)` row ids of each wire, in wire order.
    pub const WIRE_IDS: [(u32, u32, u32); WIRES] = [
        (W1_SRC, W1_DST, W1_AMT),
        (W2_SRC, W2_DST, W2_AMT),
        (W3_SRC, W3_DST, W3_AMT),
    ];

    /// What a wire can listen to. Index order IS the wire value of the
    /// `W*_SRC` rows. "off" first, so a defaulted wire is silent.
    pub const MOD_SRC: &[&str] = &[
        "off", "osc a", "osc b", "noise", "amp", "fenv", "penv", "vel",
    ];
    pub const SRC_OFF: u32 = 0;
    pub const SRC_OSC_A: u32 = 1;
    pub const SRC_OSC_B: u32 = 2;
    pub const SRC_NOISE: u32 = 3;
    pub const SRC_AMP: u32 = 4;
    pub const SRC_FENV: u32 = 5;
    pub const SRC_PENV: u32 = 6;
    pub const SRC_VEL: u32 = 7;

    /// What a wire can move. Phase and level are the audio-rate pair —
    /// PM IS FM here (phase is the integral of frequency, and the phase
    /// accumulator makes it an add); cutoff, res and pan ride the
    /// control chunk.
    pub const MOD_DST: &[&str] = &[
        "off",
        "a phase",
        "a level",
        "b phase",
        "b level",
        "noise lvl",
        "cutoff",
        "res",
        "pan",
    ];
    pub const DST_OFF: u32 = 0;
    pub const DST_A_PHASE: u32 = 1;
    pub const DST_A_LEVEL: u32 = 2;
    pub const DST_B_PHASE: u32 = 3;
    pub const DST_B_LEVEL: u32 = 4;
    pub const DST_N_LEVEL: u32 = 5;
    pub const DST_CUTOFF: u32 = 6;
    pub const DST_RES: u32 = 7;
    pub const DST_PAN: u32 = 8;

    /// Total voices the allocator owns, before unison multiplies them.
    /// Two lane groups of eight — one zmm register each.
    pub const VOICES: usize = 16;

    /// The part of a name a knob shows: everything after the group word.
    ///
    /// A subslice of a `&'static str` is still `&'static`, so this is the
    /// whole implementation — no second table of short names to drift
    /// against the first.
    pub fn label(name: &'static str) -> &'static str {
        name.split_once(' ').map_or(name, |(_group, rest)| rest)
    }

    /// Defaults are ONE audible oscillator and nothing else: osc A at
    /// full level, osc B and the noise source silent, the filter parked
    /// open, unity gain out. Loading the synth makes a sound the moment a
    /// note arrives, and it makes exactly one — the reverb table's
    /// never-destroy-what-it-lands-on rule, aimed at a patch the user
    /// starts by ADDING to rather than by subtracting from.
    pub const TABLE: &[ParamDef] = &[
        // ---------------------------------------------------- osc a ---
        ParamDef {
            id: A_WAVE,
            name: "a wave",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: A_OCT,
            name: "a oct",
            min: 0.0,
            max: 8.0,
            default: 4.0,
        },
        ParamDef {
            id: A_SEMI,
            name: "a semi",
            min: -12.0,
            max: 12.0,
            default: 0.0,
        },
        ParamDef {
            id: A_FINE,
            name: "a fine",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: A_LEVEL,
            name: "a level",
            min: 0.0,
            max: 100.0,
            default: 100.0,
        },
        // +-48 semitones: the brief's kick recipe sweeps the whole range.
        ParamDef {
            id: A_PENV,
            name: "a p.env",
            min: -48.0,
            max: 48.0,
            default: 0.0,
        },
        // ---------------------------------------------------- osc b ---
        ParamDef {
            id: B_WAVE,
            name: "b wave",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: B_OCT,
            name: "b oct",
            min: 0.0,
            max: 8.0,
            default: 4.0,
        },
        ParamDef {
            id: B_SEMI,
            name: "b semi",
            min: -12.0,
            max: 12.0,
            default: 0.0,
        },
        ParamDef {
            id: B_FINE,
            name: "b fine",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: B_LEVEL,
            name: "b level",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: B_PENV,
            name: "b p.env",
            min: -48.0,
            max: 48.0,
            default: 0.0,
        },
        // ---------------------------------------------------- noise ---
        ParamDef {
            id: N_COLOR,
            name: "noise color",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: N_LEVEL,
            name: "noise level",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        // The noise source's OWN decay, so a kick's click and a hat's
        // body are two settings rather than two devices.
        ParamDef {
            id: N_DECAY,
            name: "noise decay",
            min: 1.0,
            max: 4_000.0,
            default: 120.0,
        },
        // --------------------------------------------------- filter ---
        ParamDef {
            id: F_MODE,
            name: "filter mode",
            min: 0.0,
            max: 3.0,
            default: 0.0,
        },
        ParamDef {
            id: F_SLOPE,
            name: "filter slope",
            min: 0.0,
            max: 5.0,
            default: 3.0,
        },
        // Down to 20 Hz: a sub bass has to keep its fundamental.
        ParamDef {
            id: F_CUTOFF,
            name: "filter cutoff",
            min: 20.0,
            max: 20_000.0,
            default: 20_000.0,
        },
        // Q, on the same scale the Filter node uses — one resonance
        // meaning across the app, so `filter::effective_q` applies here
        // unchanged.
        ParamDef {
            id: F_RES,
            name: "filter res",
            min: 0.3,
            max: 24.0,
            default: super::filter::FLAT_Q,
        },
        // Bipolar, in percent of the cutoff sweep.
        ParamDef {
            id: F_ENV,
            name: "filter env",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: F_KEY,
            name: "filter key",
            min: 0.0,
            max: 200.0,
            default: 0.0,
        },
        ParamDef {
            id: F_DRIVE,
            name: "filter drive",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: F_POS,
            name: "filter pos",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        // ------------------------------------------------------ amp ---
        // Attacks down to 1 ms and below, per the brief: a click is an
        // envelope, not a special case.
        ParamDef {
            id: AMP_A,
            name: "amp attack",
            min: 0.05,
            max: 5_000.0,
            default: 1.0,
        },
        ParamDef {
            id: AMP_D,
            name: "amp decay",
            min: 1.0,
            max: 10_000.0,
            default: 200.0,
        },
        ParamDef {
            id: AMP_S,
            name: "amp sustain",
            min: 0.0,
            max: 100.0,
            default: 70.0,
        },
        ParamDef {
            id: AMP_R,
            name: "amp release",
            min: 1.0,
            max: 30_000.0,
            default: 640.0,
        },
        // Linear gain, matching `seq::GAIN` — the table speaks amplitude
        // and the widget decides how to show it.
        ParamDef {
            id: GAIN,
            name: "amp gain",
            min: 0.0,
            max: 2.0,
            default: 1.0,
        },
        ParamDef {
            id: VEL,
            name: "amp vel",
            min: 0.0,
            max: 100.0,
            default: 100.0,
        },
        // --------------------------------------------------- voices ---
        ParamDef {
            id: V_MODE,
            name: "voice mode",
            min: 0.0,
            max: 2.0,
            default: 0.0,
        },
        ParamDef {
            id: V_GLIDE,
            name: "voice glide",
            min: 1.0,
            max: 2_000.0,
            default: 1.0,
        },
        ParamDef {
            id: V_UNISON,
            name: "voice unison",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: V_DETUNE,
            name: "voice detune",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: V_SPREAD,
            name: "voice spread",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        // ------------------------------------------- filter envelope ---
        // Its own ADSR: two voices of one patch may share a shape, but
        // the filter's contour and the amp's are different musical ideas
        // and borrowing one's times for the other made them one knob.
        ParamDef {
            id: FENV_A,
            name: "fenv attack",
            min: 0.05,
            max: 5_000.0,
            default: 1.0,
        },
        ParamDef {
            id: FENV_D,
            name: "fenv decay",
            min: 1.0,
            max: 10_000.0,
            default: 200.0,
        },
        ParamDef {
            id: FENV_S,
            name: "fenv sustain",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: FENV_R,
            name: "fenv release",
            min: 1.0,
            max: 30_000.0,
            default: 300.0,
        },
        // -------------------------------------------- pitch envelope ---
        // One-shot decay. ~50 ms is the brief's kick recipe, which is
        // what this envelope exists for.
        ParamDef {
            id: PENV_D,
            name: "penv decay",
            min: 1.0,
            max: 2_000.0,
            default: 50.0,
        },
        // ------------------------------------------------ mod matrix ---
        // Four wires, each three rows. Depth is bipolar percent; at 100%
        // a phase wire swings a full turn and a level wire doubles or
        // silences. Src/dst are indices into MOD_SRC / MOD_DST, "off"
        // first so a defaulted wire is silent.
        ParamDef {
            id: W1_SRC,
            name: "wire1 src",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: W1_DST,
            name: "wire1 dest",
            min: 0.0,
            max: 8.0,
            default: 0.0,
        },
        ParamDef {
            id: W1_AMT,
            name: "wire1 depth",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: W2_SRC,
            name: "wire2 src",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: W2_DST,
            name: "wire2 dest",
            min: 0.0,
            max: 8.0,
            default: 0.0,
        },
        ParamDef {
            id: W2_AMT,
            name: "wire2 depth",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: W3_SRC,
            name: "wire3 src",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: W3_DST,
            name: "wire3 dest",
            min: 0.0,
            max: 8.0,
            default: 0.0,
        },
        ParamDef {
            id: W3_AMT,
            name: "wire3 depth",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
    ];

    /// Octave transpose of a wire index.
    pub fn octave(index: u32) -> i32 {
        index as i32 - OCT_CENTER as i32
    }

    /// Unison voice count of a wire index.
    pub fn unison(index: u32) -> u32 {
        index + 1
    }
}

/// `Node::Utility` — gain, placement and the stereo field.
///
/// The device with no tone of its own. Everything here is a thing you
/// reach for when a track is RIGHT but sits wrong: it is three dB too
/// loud, it is centred when it should lean, its stereo is too wide for
/// the mix or too narrow for the part, its bass is smeared across the
/// image, one side arrived with its phase flipped, or the file has an
/// offset in it.
///
/// # The order the stages run in is a decision, not an accident
///
/// ```text
///  in ── dc ── channel ── phase ── width ── bass mono ── pan ── gain ── out
/// ```
///
/// - **DC first.** An offset is damage to be cleaned before anything
///   reads the signal; left in, it rides into `mid` and the width stage
///   spreads it across both sides.
/// - **Channel, then phase.** Swapping the sides and inverting one are
///   both routing, and doing them the other way round would make "ø L"
///   mean whichever channel `swap` had just moved there.
/// - **Width, then bass mono.** Both are mid/side, and bass mono is a
///   correction ON the width: it is the low end you keep centred no
///   matter how wide the rest is told to be.
/// - **Pan and gain last.** They are level, and level is what the stages
///   above must not have to compensate for.
///
/// # The discretes are INDICES
///
/// [`PHASE`], [`CHANNEL`] and [`DC`] ride the wire as indices into their
/// name lists, exactly as [`filter::MODE`](super::filter::MODE) does.
pub mod utility {
    use super::ParamDef;

    pub const GAIN: u32 = 0;
    pub const PAN: u32 = 1;
    pub const WIDTH: u32 = 2;
    pub const MONO_HZ: u32 = 3;
    pub const PHASE: u32 = 4;
    pub const CHANNEL: u32 = 5;
    pub const DC: u32 = 6;

    /// The trim's window, in dB. Wider than the saturator's output trim
    /// on purpose: that one is a make-up for a curve, and this is the
    /// row you reach for when a stem arrives at the wrong level
    /// entirely. Still not silence at the bottom, for the reason
    /// [`sat::OUT`](super::sat::OUT) gives — the track has a fader and a
    /// mute already, and a range whose floor is an infinite drop spends
    /// most of its travel on the last inaudible decibel.
    pub const GAIN_MIN_DB: f32 = -36.0;
    pub const GAIN_MAX_DB: f32 = 36.0;

    /// Full width is 1.0 and the ceiling is twice that. Zero is mono —
    /// the side signal gone entirely — and it is a real setting rather
    /// than an edge case: "make this mono" is half of why the device
    /// exists.
    pub const WIDTH_MAX: f32 = 2.0;

    /// The bass-mono crossover's window. At [`MONO_MIN_HZ`] the stage is
    /// OFF: the corner has walked below the band, there is nothing under
    /// it to centre, and paying for a filter that does nothing is worse
    /// than saying so — see [`mono_off`].
    pub const MONO_MIN_HZ: f32 = 20.0;
    pub const MONO_MAX_HZ: f32 = 500.0;

    pub const PHASE_NONE: u32 = 0;
    pub const PHASE_L: u32 = 1;
    pub const PHASE_R: u32 = 2;
    pub const PHASE_BOTH: u32 = 3;
    /// What the cell prints, indexed by [`PHASE`]. `ø` is the console
    /// marking, and it is worth the non-ascii: "inv L" and "ø L" cost
    /// the same width and only one of them is what the desk says.
    pub const PHASE_NAMES: &[&str] = &["off", "ø L", "ø R", "ø L+R"];

    pub const CHANNEL_STEREO: u32 = 0;
    pub const CHANNEL_SWAP: u32 = 1;
    pub const CHANNEL_LEFT: u32 = 2;
    pub const CHANNEL_RIGHT: u32 = 3;
    /// `left` and `right` mean "this side, on both outputs" — a mono
    /// FOLD to one source, not a mute of the other. Soloing one side of
    /// a stereo file to check it is the ordinary use, and hearing it out
    /// of one speaker while you do is not.
    pub const CHANNEL_NAMES: &[&str] = &["stereo", "swap", "left", "right"];

    /// The two-position switches' faces. One list, because a switch that
    /// printed `on`/`off` in one place and `in`/`out` in another would be
    /// two grammars for one control.
    pub const SWITCH_NAMES: &[&str] = &["off", "on"];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: GAIN,
            name: "gain",
            // dB, not linear gain: this row IS the number the user is
            // thinking in, and every readout, automation lane and
            // modulation sweep over it should move in decibels because
            // that is what "3 dB down" means.
            min: GAIN_MIN_DB,
            max: GAIN_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: PAN,
            name: "pan",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: WIDTH,
            name: "width",
            min: 0.0,
            max: WIDTH_MAX,
            // Untouched. A utility that opened anywhere but unity would
            // change a mix by being inserted, which is the one thing this
            // device must never do.
            default: 1.0,
        },
        ParamDef {
            id: MONO_HZ,
            name: "mono",
            min: MONO_MIN_HZ,
            max: MONO_MAX_HZ,
            // Off, by the same rule: inserted and untouched, this device
            // is a wire.
            default: MONO_MIN_HZ,
        },
        ParamDef {
            id: PHASE,
            name: "phase",
            min: 0.0,
            max: PHASE_BOTH as f32,
            default: PHASE_NONE as f32,
        },
        ParamDef {
            id: CHANNEL,
            name: "channel",
            min: 0.0,
            max: CHANNEL_RIGHT as f32,
            default: CHANNEL_STEREO as f32,
        },
        ParamDef {
            id: DC,
            name: "dc",
            min: 0.0,
            max: 1.0,
            // Off. A DC blocker is a highpass at 5 Hz, and one that
            // arrived switched on would quietly change the bottom of
            // every track it was dropped onto.
            default: 0.0,
        },
    ];

    /// A switch position, clamped into its list. RON round-trips NaN, so
    /// a hand-edited project can smuggle one in and this is where it
    /// stops — the same door [`glue`](super::glue) uses, for the same
    /// reason.
    pub fn index(value: f32, count: usize) -> u32 {
        if value.is_finite() {
            (value.round().max(0.0) as u32).min(count.saturating_sub(1) as u32)
        } else {
            0
        }
    }

    /// The sign each side is multiplied by, for a [`PHASE`] index. The
    /// ONE place the wire index becomes arithmetic — the card's cell
    /// reads the same list in the same order, so the drawing and the
    /// audio flip the same channel.
    pub fn phase_signs(index: u32) -> (f32, f32) {
        match index.min(PHASE_BOTH) {
            PHASE_L => (-1.0, 1.0),
            PHASE_R => (1.0, -1.0),
            PHASE_BOTH => (-1.0, -1.0),
            _ => (1.0, 1.0),
        }
    }

    /// Whether the bass-mono crossover does anything at this corner.
    ///
    /// A single place, because the node skipping the filter and the card
    /// printing "off" have to agree: a stage that is drawn as bypassed
    /// and still running is how a null test stops nulling.
    pub fn mono_off(hz: f32) -> bool {
        !hz.is_finite() || hz <= MONO_MIN_HZ
    }
}

/// The sampler — the device that plays a file, and has an opinion about
/// how it should sound coming back out.
///
/// Thirty-six rows across five pages. Design and reasoning:
/// `notes/20260827-sampler-brief.md`.
///
/// Everything here is in ENGINE units — hertz, milliseconds, semitones,
/// decibels — except the four positions ([`START`], [`END`],
/// [`LOOP_START`], and the `start` modulation destination), which are
/// FRACTIONS OF THE FILE. That is deliberate: a position stored in frames
/// would point somewhere else the moment a different sample was dropped
/// on the card, and a sampler is a device you drop samples on.
pub mod sampler {
    use super::ParamDef;

    pub const MODE: u32 = 0;
    pub const START: u32 = 1;
    pub const END: u32 = 2;
    pub const REVERSE: u32 = 3;
    pub const FADE_IN: u32 = 4;
    pub const FADE_OUT: u32 = 5;
    pub const ROOT: u32 = 6;
    pub const TUNE: u32 = 7;
    pub const FINE: u32 = 8;

    pub const LOOP_MODE: u32 = 9;
    pub const LOOP_START: u32 = 10;
    pub const LOOP_XFADE: u32 = 11;
    pub const SLICES: u32 = 12;
    pub const SLICE_SOURCE: u32 = 13;
    pub const CHOKE: u32 = 14;

    pub const AMP_A: u32 = 15;
    pub const AMP_D: u32 = 16;
    pub const AMP_S: u32 = 17;
    pub const AMP_R: u32 = 18;
    pub const FILT_MODE: u32 = 19;
    pub const CUTOFF: u32 = 20;
    pub const RES: u32 = 21;
    pub const KEYTRACK: u32 = 22;

    pub const MOD_A: u32 = 23;
    pub const MOD_D: u32 = 24;
    pub const MOD_S: u32 = 25;
    pub const MOD_R: u32 = 26;
    pub const MOD_DEST: u32 = 27;
    pub const MOD_DEPTH: u32 = 28;
    pub const VELOCITY: u32 = 29;

    pub const DRIVE: u32 = 30;
    pub const RATE: u32 = 31;
    pub const BITS: u32 = 32;
    pub const PREAMP: u32 = 33;
    pub const GAIN: u32 = 34;
    pub const PAN: u32 = 35;

    // ------------------------------------------------------- discretes ---

    /// What a note MEANS. See the brief's mode table.
    pub const MODE_CLASSIC: f32 = 0.0;
    pub const MODE_ONE_SHOT: f32 = 1.0;
    pub const MODE_SLICE: f32 = 2.0;
    pub const MODE_MAX: f32 = MODE_SLICE;
    pub const MODE_NAMES: &[&str] = &["classic", "1-shot", "slice"];

    pub const LOOP_OFF: f32 = 0.0;
    pub const LOOP_FORWARD: f32 = 1.0;
    pub const LOOP_PINGPONG: f32 = 2.0;
    pub const LOOP_MODE_MAX: f32 = LOOP_PINGPONG;
    pub const LOOP_NAMES: &[&str] = &["off", "fwd", "ping"];

    pub const SLICE_GRID: f32 = 0.0;
    pub const SLICE_TRANSIENTS: f32 = 1.0;
    /// The table was authored BY HAND and nothing may re-cut it.
    ///
    /// Not a third way of slicing — a latch. Dragging a marker sets it,
    /// because a grid that is re-derived from the count every frame would
    /// otherwise walk over the drag on the very next one. Making that
    /// visible in a cell is the point: when the count knob stops re-cutting
    /// the file, the card can say why.
    pub const SLICE_CUSTOM: f32 = 2.0;
    pub const SLICE_SOURCE_MAX: f32 = SLICE_CUSTOM;
    pub const SLICE_SOURCE_NAMES: &[&str] = &["grid", "onset", "custom"];

    pub const OFF_ON_NAMES: &[&str] = &["off", "on"];

    /// Filter shapes, in the order [`crate::dsp::filters::Mode`] wants
    /// them. One filter, four faces — the brief asks for a single filter
    /// and this is what "single" costs.
    pub const FILT_LOWPASS: f32 = 0.0;
    pub const FILT_HIGHPASS: f32 = 1.0;
    pub const FILT_BANDPASS: f32 = 2.0;
    pub const FILT_NOTCH: f32 = 3.0;
    pub const FILT_MODE_MAX: f32 = FILT_NOTCH;
    pub const FILT_NAMES: &[&str] = &["lp", "hp", "bp", "notch"];

    /// Where the one modulation envelope goes.
    pub const DEST_CUTOFF: f32 = 0.0;
    pub const DEST_PITCH: f32 = 1.0;
    pub const DEST_START: f32 = 2.0;
    pub const DEST_RATE: f32 = 3.0;
    pub const DEST_DRIVE: f32 = 4.0;
    pub const DEST_PAN: f32 = 5.0;
    pub const DEST_MAX: f32 = DEST_PAN;
    pub const DEST_NAMES: &[&str] = &["cutoff", "pitch", "start", "rate", "drive", "pan"];

    // ------------------------------------------------------------ spans ---

    /// The most slices one file can be cut into. Sixty-four is the
    /// Octatrack's number and it is the right one: past that the notes
    /// run off the top of a five-octave keyboard.
    pub const SLICES_MAX: f32 = 64.0;

    /// How far the pitch knobs reach. Two octaves either way covers
    /// re-tuning a one-shot into a bassline without turning the range
    /// into a place you cannot find zero in.
    pub const TUNE_MAX: f32 = 24.0;

    /// Full-scale throw of each modulation destination, at `depth = 1`.
    /// The card prints these, so they live here rather than inside the
    /// voice where the card could not see them.
    pub const DEST_CUTOFF_OCTAVES: f32 = 5.0;
    pub const DEST_PITCH_SEMITONES: f32 = 24.0;
    pub const DEST_START_FRACTION: f32 = 0.25;
    pub const DEST_RATE_OCTAVES: f32 = 4.0;

    /// The MIDI note that plays slice 0, and every semitone above it the
    /// next slice.
    pub const SLICE_BASE_NOTE: u8 = 36;

    /// Which page each row belongs to, for the card's tab strip. Parallel
    /// to [`TABLE`] and checked against it by a test, because a row that
    /// no page claims is a row nobody can reach.
    pub const PAGES: &[&str] = &["sample", "loop", "shape", "mod", "dirt"];

    pub fn page_of(id: u32) -> usize {
        match id {
            MODE..=FINE => 0,
            LOOP_MODE..=CHOKE => 1,
            AMP_A..=KEYTRACK => 2,
            MOD_A..=VELOCITY => 3,
            _ => 4,
        }
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MODE,
            name: "mode",
            min: 0.0,
            max: MODE_MAX,
            default: MODE_CLASSIC,
        },
        ParamDef {
            id: START,
            name: "start",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: END,
            name: "end",
            min: 0.0,
            max: 1.0,
            default: 1.0,
        },
        ParamDef {
            id: REVERSE,
            name: "reverse",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: FADE_IN,
            // NOT zero by default. A sample cut mid-cycle clicks, and a
            // device that clicks out of the box is a broken device.
            name: "fadein",
            min: 0.0,
            max: 2_000.0,
            default: 2.0,
        },
        ParamDef {
            id: FADE_OUT,
            // Also the choke ramp and the voice-stealing ramp, which is
            // why it is longer than the fade in: what it mostly does is
            // get out of the way of the next note.
            name: "fadeout",
            min: 0.0,
            max: 2_000.0,
            default: 5.0,
        },
        ParamDef {
            id: ROOT,
            name: "root",
            min: 0.0,
            max: 127.0,
            default: 60.0,
        },
        ParamDef {
            id: TUNE,
            name: "tune",
            min: -TUNE_MAX,
            max: TUNE_MAX,
            default: 0.0,
        },
        ParamDef {
            id: FINE,
            name: "fine",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: LOOP_MODE,
            name: "loop",
            min: 0.0,
            max: LOOP_MODE_MAX,
            default: LOOP_OFF,
        },
        ParamDef {
            id: LOOP_START,
            name: "loopstart",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: LOOP_XFADE,
            name: "xfade",
            min: 0.0,
            max: 500.0,
            default: 10.0,
        },
        ParamDef {
            id: SLICES,
            name: "slices",
            min: 1.0,
            max: SLICES_MAX,
            default: 16.0,
        },
        ParamDef {
            id: SLICE_SOURCE,
            name: "slicefrom",
            min: 0.0,
            max: SLICE_SOURCE_MAX,
            default: SLICE_GRID,
        },
        ParamDef {
            id: CHOKE,
            name: "choke",
            min: 0.0,
            max: 1.0,
            // On: a new slice cuts the last one. That is what makes a
            // chopped break sit still instead of smearing.
            default: 1.0,
        },
        ParamDef {
            id: AMP_A,
            name: "attack",
            min: 0.1,
            max: 8_000.0,
            default: 1.0,
        },
        ParamDef {
            id: AMP_D,
            name: "decay",
            min: 1.0,
            max: 16_000.0,
            default: 800.0,
        },
        ParamDef {
            id: AMP_S,
            name: "sustain",
            min: 0.0,
            max: 1.0,
            // Held open, so a fresh sampler plays the file rather than an
            // envelope's opinion of it.
            default: 1.0,
        },
        ParamDef {
            id: AMP_R,
            name: "release",
            min: 1.0,
            max: 16_000.0,
            default: 40.0,
        },
        ParamDef {
            id: FILT_MODE,
            name: "filter",
            min: 0.0,
            max: FILT_MODE_MAX,
            default: FILT_LOWPASS,
        },
        ParamDef {
            id: CUTOFF,
            name: "cutoff",
            min: 20.0,
            max: 20_000.0,
            default: 20_000.0,
        },
        ParamDef {
            id: RES,
            name: "res",
            min: 0.5,
            max: 20.0,
            default: std::f32::consts::FRAC_1_SQRT_2,
        },
        ParamDef {
            id: KEYTRACK,
            name: "keytrack",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: MOD_A,
            name: "modatk",
            min: 0.1,
            max: 8_000.0,
            default: 1.0,
        },
        ParamDef {
            id: MOD_D,
            name: "moddec",
            min: 1.0,
            max: 16_000.0,
            default: 200.0,
        },
        ParamDef {
            id: MOD_S,
            name: "modsus",
            min: 0.0,
            max: 1.0,
            // Zero: this is an envelope, not a level. A mod envelope that
            // sustains at full is a knob offset wearing a disguise.
            default: 0.0,
        },
        ParamDef {
            id: MOD_R,
            name: "modrel",
            min: 1.0,
            max: 16_000.0,
            default: 100.0,
        },
        ParamDef {
            id: MOD_DEST,
            name: "dest",
            min: 0.0,
            max: DEST_MAX,
            default: DEST_CUTOFF,
        },
        ParamDef {
            id: MOD_DEPTH,
            name: "depth",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: VELOCITY,
            name: "vel",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 0.0,
            max: 1.0,
            // On by default, and only just. Enough to round a transient's
            // tip; not enough to hear as distortion on a pad.
            default: 0.15,
        },
        ParamDef {
            id: RATE,
            name: "rate",
            min: crate::dsp::lofi::RATE_MIN,
            max: 48_000.0,
            // The S900's neighbourhood. Subtle, not crushed: what it
            // mostly does is take the air off.
            default: 32_000.0,
        },
        ParamDef {
            id: BITS,
            name: "bits",
            min: crate::dsp::lofi::BITS_MIN,
            max: crate::dsp::lofi::BITS_MAX,
            // Twelve, because that is what the machines this borrows from
            // had. Continuous, because thirteen and a half is a perfectly
            // good lattice and sometimes the right one.
            default: 12.0,
        },
        ParamDef {
            id: PREAMP,
            name: "preamp",
            min: 0.0,
            max: 1.0,
            default: 0.25,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: -60.0,
            max: 12.0,
            default: 0.0,
        },
        ParamDef {
            id: PAN,
            name: "pan",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
    ];
}

/// The lo-fi converter — an early sampler's front end, as a device.
///
/// Two knobs of real character and two of housekeeping. `RATE` is the
/// converter's own clock and `BITS` its word length, and both are the
/// kernel's: `dsp::lofi::Downsampler` owns the tracking anti-alias
/// filter, the zero-order hold and the quantiser, and this table only
/// says what a musician is allowed to ask for.
///
/// # The top of each range is OFF, and exactly off
///
/// The kernel promises an EXACT identity at `rate >= sample_rate` and at
/// `bits >= 16`, checked per block rather than per sample. Both ends are
/// reachable here on purpose, so "how much of this is the device?" is
/// answerable by turning two knobs to their stops rather than by
/// bypassing the card. A colour you cannot remove is a colour you cannot
/// measure.
///
/// [`RATE_MAX`] is 48 kHz rather than the running sample rate because a
/// TABLE is `&'static` — `params::clamp` scans it inside the callback —
/// and a bound that moved with the device's preparation could not be.
/// At any session rate at or below 48 kHz the top of the knob is at or
/// above it, so the off switch stays reachable.
/// The sheen — a slew-driven brightener, as a device.
///
/// `dsp::dynamics::SlewBrighten` owns the whole idea: a high-frequency
/// lift whose size is driven by how fast the signal is actually MOVING,
/// so a snare edge gets it and a held pad does not. This table only says
/// what a musician is allowed to ask for.
///
/// # The bottom of `AMOUNT` is off, and exactly off
///
/// The kernel promises that amount 0 is a bit-exact wire rather than
/// merely a quiet one, which is what lets a device leave the stage
/// permanently in its path. That end is reachable here on purpose, for
/// the reason [`lofi`](super::lofi)'s two off switches are: a colour you
/// cannot remove is a colour you cannot measure.
///
/// # What is NOT here
///
/// The envelope's attack and release, and the knee that decides where
/// "fast" starts, are the kernel's own constants and stay there. They are
/// what make this a brightener rather than a general-purpose exciter, and
/// the kernel's doc argues each figure. A device that exposed them would
/// be offering the user a way to turn it into the harshness it exists to
/// avoid.
/// The disperser — a chain of allpasses, as a device.
///
/// `dsp::filters::Disperser` owns the whole idea: sections that pass every
/// frequency at unity gain and do nothing but delay each one differently,
/// so a transient stops arriving all at once and a click becomes a
/// descending chirp. This table only says what a musician may ask for.
///
/// Modelled on Kilohearts' Disperser, which is the reference everyone has
/// heard, and whose three controls map exactly onto what our kernel
/// already takes: how many sections, where they are tuned, and how
/// tightly the phase turns there.
///
/// # There is no MIX, and that is the device
///
/// Every other effect here blends against the dry. This one must not.
/// The kernel's load-bearing property is that its magnitude response is
/// FLAT at every setting — that is what makes it safe across a drum — and
/// flatness is a property of the wet signal alone. Sum it with the dry
/// and the two disagree in phase, which is a comb filter: notches that
/// move with the frequency knob. That is a phaser, and a good one, but it
/// is a different device and it does not keep this one's promise.
///
/// # Zero stages is the off switch
///
/// The kernel says so plainly, and the range starts there on purpose, for
/// the reason [`lofi`](super::lofi) and [`sheen`](super::sheen) both give:
/// a colour you cannot remove is a colour you cannot measure. The
/// reference starts at one section; we start at none, because ours can.
/// The tilt — a see-saw around a pivot, as a device.
///
/// `dsp::filters::Tilt` owns the whole idea: highs up and lows down by
/// the same amount, or the reverse, and unity exactly AT the pivot. One
/// gesture for "brighter" or "darker" that keeps the level where it was.
///
/// The kernel has been in the tree since the sampler's preamp, which uses
/// it with the pivot nailed to 1 kHz and the direction fixed. This table
/// is what it takes to hand both to the user.
///
/// # Two rows, and why there is no third
///
/// No output trim, unlike every other colour device here. A see-saw is
/// unity at its pivot and moves the two ends in opposite directions, so
/// broadband material comes out at about the level it went in — there is
/// no make-up to make up. A third row would be a third thing to set on a
/// device whose entire appeal is that it is one gesture.
///
/// # Flat is the DEFAULT, which is the opposite of the lo-fi's argument
///
/// [`lofi`](super::lofi) opens audibly coloured, because a colour device
/// that does nothing until you turn a knob is a surprise. This one opens
/// FLAT, and the difference is what the device is for. You reach for a
/// tilt to make a decision about a track; a tilt that arrived having
/// already made one would be making it on your behalf, and you would
/// have to undo it before you could start. Corrective devices open
/// neutral. Colouring devices open coloured.
///
/// Flat here is the kernel's exact wire — zero tilt is `g_hi = 1` and
/// `g_delta = 0`, which is `x` — so the off setting is exact in the way
/// [`lofi`](super::lofi) and [`sheen`](super::sheen) both insist on.
/// The phaser — the disperser's allpass chain, swept and MIXED.
///
/// [`disperser`](super::disperser) refuses a mix knob, and its module
/// says exactly why: summing a phase-shifted copy with the dry is a comb
/// filter whose notches move with the frequency, which breaks the flat
/// magnitude that device promises. "That is a phaser, and a good one, but
/// it is a different device."
///
/// This is that device. Same kernel, same sections; the mix is the point
/// rather than the mistake, and an LFO walks the corner so the notches
/// sweep.
///
/// # What it does not expose
///
/// The sections' Q. The disperser offers it as `pinch` because a
/// disperser is a tuning instrument — you aim it at a harmonic. A phaser
/// is a sweep, and a narrow Q turns the notches into a ringing pitch that
/// fights the sweep rather than riding it. It is fixed at
/// `audio::graph`'s `PHASER_Q`.
///
/// # Zero stages is the off switch
///
/// With no sections the wet path IS the dry path, so the blend has
/// nothing to cancel against and the device passes through whatever the
/// mix says. The same exact-off the rest of these devices insist on,
/// arriving for free out of the kernel's own wire.
/// The gate — downward expansion, as a device.
///
/// `dsp::dynamics::Mode::Expand` has existed since the dynamics family
/// landed, and its own doc calls it "a gate at high ratio". Until now
/// nothing in the audio path used it: `audio::glue` is a compressor, and
/// the only caller of `Expand` anywhere was the display widget that draws
/// its curve. This table is what it takes to make it a device.
///
/// # Attack and release mean the opposite of what they mean on the glue
///
/// A compressor ATTACKS by pulling the gain DOWN. A gate attacks by
/// letting it UP — the signal crossed the threshold and the gate opens.
/// `dsp::dynamics::Ballistics` uses the compressor's convention (attack
/// is whichever direction adds reduction), so `audio::gate` hands it the
/// two times SWAPPED. See `GateCore::process`; a test measures the
/// opening and closing times so the swap cannot quietly come undone.
///
/// # And it listens to its INPUT, not its output
///
/// The glue is a feedback compressor: its detector reads its own output,
/// which is what gives it its character. A gate must not be. Once a
/// feedback gate closed, its detector would hear the silence it had just
/// made, decide the signal was still below the threshold, and stay shut
/// forever. Feed-forward is not a preference here, it is the only
/// topology that reopens.
pub mod gate {
    use super::ParamDef;

    pub const THRESHOLD: u32 = 0;
    pub const RATIO: u32 = 1;
    pub const ATTACK: u32 = 2;
    pub const RELEASE: u32 = 3;
    pub const RANGE: u32 = 4;

    /// Where the gate decides, in dBFS.
    ///
    /// # These two ranges are the DISPLAY's, deliberately
    ///
    /// `ui::device::dynamics` draws its transfer curve across
    /// `VIEW_MIN_DB..VIEW_MAX_DB` with a ratio axis of
    /// `RATIO_MIN..RATIO_MAX`, and on the gate's card that curve is not
    /// an illustration — it is the control, dragged to set both. A knob
    /// that reached past the axis it is drawn on would put the threshold
    /// cell and the threshold handle in disagreement, and the card's
    /// whole premise is that they cannot disagree.
    ///
    /// So the table takes the widget's figures rather than the kernel's
    /// wider ones. Nothing is lost that a gate wants: a threshold under
    /// -60 dBFS is below the noise it would be gating, and 60:1 is
    /// already a closed door — `dsp::dynamics`' own doc says past about a
    /// hundred the curve is "a gate to more digits than a float holds".
    pub const THRESHOLD_MIN_DB: f32 = crate::ui::device::dynamics::VIEW_MIN_DB;
    pub const THRESHOLD_MAX_DB: f32 = crate::ui::device::dynamics::VIEW_MAX_DB;

    /// How steeply it expands below the threshold. The display's axis —
    /// see [`THRESHOLD_MIN_DB`].
    pub const RATIO_MIN: f32 = crate::ui::device::dynamics::RATIO_MIN;
    pub const RATIO_MAX: f32 = crate::ui::device::dynamics::RATIO_MAX;

    /// How fast it OPENS, in ms. Fast at the bottom, because a gate that
    /// takes even a millisecond to open has already eaten the transient
    /// it was let through for.
    pub const ATTACK_MIN_MS: f32 = 0.05;
    pub const ATTACK_MAX_MS: f32 = 100.0;

    /// How fast it CLOSES, in ms.
    pub const RELEASE_MIN_MS: f32 = 5.0;
    pub const RELEASE_MAX_MS: f32 = 2_000.0;

    /// The most it will ever shut, in dB.
    ///
    /// A gate that closes completely is the special case, not the
    /// default: leaving a little of the room in is what makes gated drums
    /// sound gated rather than chopped. Zero range is the device switched
    /// off in all but name, which is what the bottom of the control is
    /// for — and it is exact, because a range of nothing is a gain of
    /// one.
    pub const RANGE_MAX_DB: f32 = 80.0;

    /// The detector's window, in ms. Not a knob: a gate is a decision
    /// about whether a sound has started, and a slow window blurs the one
    /// question it exists to answer. Fast enough to catch a stick, slow
    /// enough not to chatter on a waveform's own zero crossings.
    pub const WINDOW_MS: f32 = 3.0;

    /// The knee, in dB, centred on the threshold. Not a knob either —
    /// five rows is already the biggest of the small devices, and a
    /// gate's knee is a refinement rather than a decision. Soft enough
    /// that programme sitting near the threshold breathes instead of
    /// stuttering.
    pub const KNEE_DB: f32 = 6.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: THRESHOLD,
            name: "threshold",
            min: THRESHOLD_MIN_DB,
            max: THRESHOLD_MAX_DB,
            default: -40.0,
        },
        ParamDef {
            id: RATIO,
            name: "ratio",
            min: RATIO_MIN,
            max: RATIO_MAX,
            // Eight to one. Unmistakably a gate and still an EXPANDER —
            // it leans on quiet material rather than deleting it, which
            // is the setting that flatters most sources. The top of the
            // range is there for when you want the chop.
            default: 8.0,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: ATTACK_MIN_MS,
            max: ATTACK_MAX_MS,
            default: 1.0,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: RELEASE_MIN_MS,
            max: RELEASE_MAX_MS,
            default: 150.0,
        },
        ParamDef {
            id: RANGE,
            name: "range",
            min: 0.0,
            max: RANGE_MAX_DB,
            // Sixty dB down is closed to any ear, and still short of the
            // silence that makes a gate sound like an edit.
            default: 60.0,
        },
    ];
}

pub mod phaser {
    use super::ParamDef;

    pub const AMOUNT: u32 = 0;
    pub const CENTRE: u32 = 1;
    pub const DEPTH: u32 = 2;
    pub const RATE: u32 = 3;
    pub const MIX: u32 = 4;

    /// The most sections a phaser runs.
    ///
    /// Sixteen, not the disperser's thirty-two. Past this the notches are
    /// packed closer than a sweep can separate them and the effect stops
    /// being a phaser and starts being the disperser next door — which is
    /// available, and better at it.
    pub const AMOUNT_MAX: f32 = 16.0;

    /// Where the sweep is centred.
    pub const CENTRE_MIN_HZ: f32 = 100.0;
    pub const CENTRE_MAX_HZ: f32 = 4_000.0;

    /// How far the corner travels either side of centre, in OCTAVES.
    ///
    /// Octaves and not hertz, because that is what the ear hears and what
    /// the log-mapped centre knob already speaks. A depth in hertz would
    /// mean a different sweep at every centre setting.
    pub const DEPTH_MAX_OCT: f32 = 4.0;

    /// The sweep's speed. Slow enough at the bottom to take half a minute
    /// over one pass, fast enough at the top to wobble.
    pub const RATE_MIN_HZ: f32 = 0.02;
    pub const RATE_MAX_HZ: f32 = 8.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: AMOUNT,
            name: "amount",
            min: 0.0,
            max: AMOUNT_MAX,
            // Four sections: two notches, which is the classic phaser and
            // the one everybody has heard.
            default: 4.0,
        },
        ParamDef {
            id: CENTRE,
            name: "centre",
            min: CENTRE_MIN_HZ,
            max: CENTRE_MAX_HZ,
            default: 800.0,
        },
        ParamDef {
            id: DEPTH,
            name: "depth",
            min: 0.0,
            max: DEPTH_MAX_OCT,
            // Two octaves either side. Wide enough to hear the notches
            // travel, narrow enough that they stay in the band the centre
            // knob was pointed at.
            default: 2.0,
        },
        ParamDef {
            id: RATE,
            name: "rate",
            min: RATE_MIN_HZ,
            max: RATE_MAX_HZ,
            default: 0.4,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // HALF, and this one is not a shrug. A phaser's notch is the
            // dry and the wet cancelling, and cancellation is deepest
            // when the two are equal. Fully wet is an allpass — flat, and
            // silent as an effect. The knob's TOP is the setting that
            // does nothing here, which is the reverse of every other mix
            // in the rack and worth knowing before you reach for it.
            default: 0.5,
        },
    ];
}

pub mod tilt {
    use super::ParamDef;

    pub const TILT: u32 = 0;
    pub const PIVOT: u32 = 1;

    /// How far the see-saw leans, in dB at the HIGH extreme; the low end
    /// mirrors it.
    ///
    /// TWELVE, and not the kernel's own twenty-four. This is the one
    /// place in the rack where the device's range is deliberately
    /// narrower than the kernel's, and the reason is measured rather
    /// than tasteful — see [`PIVOT_MAX_HZ`].
    pub const TILT_MAX_DB: f32 = 12.0;

    /// Where the plank balances, and the other half of one decision.
    ///
    /// # Why both ranges stop short of the kernel's
    ///
    /// `Tilt::prepare` puts the unity crossing on the pivot by placing
    /// the one-pole's corner at `pivot × g_hi`. That correction is what
    /// makes the pivot mean anything — without it the crossing slides an
    /// octave at ±6 dB — but it walks the corner UP as the lean
    /// increases, and a bilinear corner loses accuracy as it approaches
    /// Nyquist. At a high pivot and a big positive lean the two multiply
    /// and the crossing comes off the pivot after all.
    ///
    /// Measured, at 48 kHz, as the crossing's error in dB:
    ///
    /// ```text
    ///   pivot:    200    500   1000   2000   3000   4000   8000
    ///  +24 dB:  -0.12  -0.86  -4.22 -20.82 -18.69 -16.60 -10.58
    ///  +12 dB:  -0.01  -0.07  -0.18  -0.70  -1.83  -3.55 -11.65
    ///   +6 dB:  -0.00  -0.03  -0.03  -0.09  -0.29  -0.37  -2.00
    /// ```
    ///
    /// So the ranges are drawn around the region where the promise
    /// HOLDS: ±12 dB and a pivot up to 2 kHz keeps the crossing within
    /// 0.7 dB of where the knob points, and within 0.2 dB over most of
    /// it. That is a conventional tilt anyway — most are ±6 — and the
    /// alternative was shipping two knobs that combine into a lie.
    ///
    /// The failure is not the kernel's: its doc already says a
    /// first-order see-saw is the wrong tool past its ceiling, and this
    /// is the same argument arriving one stage earlier. Nothing here
    /// clamps behind the user's back, which would be the other way to
    /// hide it and a worse one — a knob that silently stops moving is
    /// harder to diagnose than one that was never offered.
    pub const PIVOT_MIN_HZ: f32 = 100.0;
    pub const PIVOT_MAX_HZ: f32 = 2_000.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: TILT,
            name: "tilt",
            min: -TILT_MAX_DB,
            max: TILT_MAX_DB,
            // FLAT. See the module header: this one is corrective, and a
            // corrective device that opens with an opinion is a device
            // you have to argue with before you can use it.
            default: 0.0,
        },
        ParamDef {
            id: PIVOT,
            name: "pivot",
            min: PIVOT_MIN_HZ,
            max: PIVOT_MAX_HZ,
            // A kilohertz: the preamp's own figure, and where a tilt
            // pivot conventionally sits — near enough the middle of the
            // band by ear that leaning either way reads as "brighter" or
            // "darker" rather than as a bass or a treble control.
            default: 1_000.0,
        },
    ];
}

pub mod disperser {
    use super::ParamDef;

    pub const AMOUNT: u32 = 0;
    pub const FREQ: u32 = 1;
    pub const PINCH: u32 = 2;

    /// The most sections on offer, and the kernel's own ceiling —
    /// `dsp::filters::DISPERSER_MAX_STAGES`, not a second opinion about
    /// it. Restated as an f32 because a TABLE row is f32.
    pub const AMOUNT_MAX: f32 = crate::dsp::filters::DISPERSER_MAX_STAGES as f32;

    /// Where the sections are tuned. The full audible span, because the
    /// whole gesture with this device is sweeping the smear from a
    /// sub-bass boing up to a metallic tick.
    pub const FREQ_MIN_HZ: f32 = 20.0;
    pub const FREQ_MAX_HZ: f32 = 20_000.0;

    /// How tightly the phase turns at the corner — the sections' Q.
    ///
    /// Low spreads the group delay over octaves and reads as a soft
    /// smear; high packs it into a narrow band and reads as a ringing
    /// pitch. The floor stays clear of the filter module's own `MIN_Q` so
    /// no setting here lands on a clamp.
    pub const PINCH_MIN: f32 = 0.1;
    pub const PINCH_MAX: f32 = 8.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: AMOUNT,
            name: "amount",
            min: 0.0,
            max: AMOUNT_MAX,
            // Eight sections: sixteen poles of phase. Unmistakably the
            // effect and still a smeared transient rather than the
            // pitched "pew" the top of the range gives, which is a sound
            // you go looking for rather than one you want on load.
            default: 8.0,
        },
        ParamDef {
            id: FREQ,
            name: "freq",
            min: FREQ_MIN_HZ,
            max: FREQ_MAX_HZ,
            // Low-mid, where a kick's body lives — the case the kernel's
            // own doc is written around.
            default: 500.0,
        },
        ParamDef {
            id: PINCH,
            name: "pinch",
            min: PINCH_MIN,
            max: PINCH_MAX,
            // The neutral turn. Butterworth-ish, and the setting at which
            // the smear reads as a softened transient rather than as a
            // note of its own.
            default: 1.0,
        },
    ];
}

pub mod sheen {
    use super::ParamDef;

    pub const AMOUNT: u32 = 0;
    pub const EDGE: u32 = 1;
    pub const MIX: u32 = 2;
    pub const OUT: u32 = 3;

    /// The kernel's own ceiling on how much a fully-triggered lift adds.
    /// Taken from `SlewBrighten::prepare`'s clamp rather than invented
    /// here, so the top of the knob is the top of the kernel.
    pub const AMOUNT_MAX: f32 = 4.0;

    /// The edge band's corner: everything above it is what gets lifted.
    ///
    /// The floor is well clear of the kernel's own 20 Hz minimum because
    /// a brightener whose band starts in the bass is a volume knob with
    /// extra steps.
    pub const EDGE_MIN_HZ: f32 = 200.0;
    pub const EDGE_MAX_HZ: f32 = 8_000.0;

    /// The output trim's window, in dB, and the same figures as linear
    /// gain — both forms written down for the reason
    /// [`sat::OUT_MIN_DB`](super::sat::OUT_MIN_DB) gives.
    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    /// `10^(-24/20)` and `10^(12/20)`, to f32 precision.
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: AMOUNT,
            name: "amount",
            min: 0.0,
            max: AMOUNT_MAX,
            // The figure the limiter's own brighten knob reaches at full
            // travel (`audio::limiter::BRIGHTEN_AMOUNT`). Known-good and
            // clearly audible, and well short of the kernel's ceiling —
            // a device that opens at its maximum leaves nowhere to go.
            default: 1.2,
        },
        ParamDef {
            id: EDGE,
            name: "edge",
            min: EDGE_MIN_HZ,
            max: EDGE_MAX_HZ,
            // `SlewBrighten`'s own default corner, and near the 1.8 kHz
            // the limiter picked: presence rather than air.
            default: 1_500.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Fully wet. The amount knob is already the "how much" — a
            // second one at less than full would mean the device opens
            // quieter than either control admits.
            default: 1.0,
        },
        ParamDef {
            id: OUT,
            name: "out",
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];
}

pub mod lofi {
    use super::ParamDef;

    pub const RATE: u32 = 0;
    pub const BITS: u32 = 1;
    pub const MIX: u32 = 2;
    pub const OUT: u32 = 3;

    /// The converter clock's window. The floor is the kernel's own
    /// [`RATE_MIN`](crate::dsp::lofi::RATE_MIN) — below about a kilohertz
    /// the hold period is heard as a buzz at its own pitch rather than as
    /// a texture — and the ceiling is where the hold switches off.
    pub const RATE_MAX: f32 = 48_000.0;

    /// The output trim's window, in dB, and the same figures as linear
    /// gain. Both forms are written down for the reason
    /// [`sat::OUT_MIN_DB`](super::sat::OUT_MIN_DB) gives: the TABLE is in
    /// linear gain and the KNOB is in dB, and a widget deriving one from
    /// the other by hand is how the two ends of one range drift apart.
    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    /// `10^(-24/20)` and `10^(12/20)`, to f32 precision.
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: RATE,
            name: "rate",
            min: crate::dsp::lofi::RATE_MIN,
            max: RATE_MAX,
            // Half of CD, which is the rate the twelve-bit machines
            // people mean by "lo-fi" actually ran near. Audibly the
            // device rather than a polite nod at it: you add this
            // because you want the grain, and one that does nothing
            // until you turn a knob is the surprise the saturator's
            // table already argues against.
            default: 22_050.0,
        },
        ParamDef {
            id: BITS,
            name: "bits",
            min: crate::dsp::lofi::BITS_MIN,
            max: crate::dsp::lofi::BITS_MAX,
            // TWELVE. The number in every advert for the machines this
            // models, and far enough from the kernel's sixteen-bit off
            // switch to be heard.
            default: 12.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Fully wet. A converter is a thing signal goes THROUGH, and
            // parallel lo-fi is the special case rather than the default
            // — unlike the reverb, which parks itself out of the way
            // because a reverb nobody asked for drowns a mix.
            default: 1.0,
        },
        ParamDef {
            id: OUT,
            name: "out",
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[(&str, &[ParamDef])] = &[
        ("seq", seq::TABLE),
        ("reverb", reverb::TABLE),
        ("sine", sine::TABLE),
        ("mixer", mixer::TABLE),
        ("pan", pan::TABLE),
        ("clip", clip::TABLE),
        ("filter", filter::TABLE),
        ("sat", sat::TABLE),
        ("echo", echo::TABLE),
        ("poly", poly::TABLE),
        ("eq", eq::TABLE),
        ("glue", glue::TABLE),
        ("kick", kick::TABLE),
        ("snare", snare::TABLE),
        ("tom", tom::TABLE),
        ("hat", hat::TABLE),
        ("handclap", handclap::TABLE),
        ("limiter", limiter::TABLE),
        ("lofi", lofi::TABLE),
        ("sheen", sheen::TABLE),
        ("disperser", disperser::TABLE),
        ("tilt", tilt::TABLE),
        ("phaser", phaser::TABLE),
        ("gate", gate::TABLE),
    ];

    /// The invariant `def()` and every `TABLE[FOO as usize]` rely on.
    /// The bus compressor's switches: every position has a name, every
    /// name has a position, and the table's range covers exactly the
    /// list it indexes.
    ///
    /// A names list one longer than its values list is how a switch ends
    /// up with a position that reads "30 ms" and sets 10 — nothing fails
    /// to compile, and nothing sounds obviously wrong either.
    #[test]
    fn the_glue_switches_and_their_names_are_the_same_length() {
        use glue as g;
        assert_eq!(g::RATIO_VALUES.len(), g::RATIO_NAMES.len());
        assert_eq!(g::ATTACK_MS.len(), g::ATTACK_NAMES.len());
        // The release is the odd one: one more NAME than time, because
        // the last position is auto and has no time of its own.
        assert_eq!(g::RELEASE_S.len() + 1, g::RELEASE_NAMES.len());
        assert_eq!(g::RELEASE_AUTO as usize, g::RELEASE_S.len());

        // Each switch's row spans exactly its list.
        for (id, count) in [
            (g::RATIO, g::RATIO_NAMES.len()),
            (g::ATTACK, g::ATTACK_NAMES.len()),
            (g::RELEASE, g::RELEASE_NAMES.len()),
        ] {
            let def = def(g::TABLE, id);
            assert_eq!(def.min, 0.0, "{}: switches start at 0", def.name);
            assert_eq!(
                def.max,
                (count - 1) as f32,
                "{}: the row does not cover its list",
                def.name
            );
        }

        // Every position resolves, and out-of-range indices clamp to the
        // end rather than panicking or wrapping to the start.
        for i in 0..g::ATTACK_MS.len() as u32 {
            assert_eq!(g::attack_ms(i), g::ATTACK_MS[i as usize]);
        }
        assert_eq!(g::attack_ms(999), *g::ATTACK_MS.last().unwrap());
        for i in 0..g::RATIO_VALUES.len() as u32 {
            assert_eq!(g::ratio(i), g::RATIO_VALUES[i as usize]);
        }
        assert_eq!(g::ratio(999), *g::RATIO_VALUES.last().unwrap());

        // Auto is the last position and nothing before it.
        for i in 0..g::RELEASE_AUTO {
            assert!(!g::is_auto(i), "position {i} must be a time");
            assert_eq!(g::release_ms(i), g::RELEASE_S[i as usize] * 1_000.0);
        }
        assert!(g::is_auto(g::RELEASE_AUTO));
        assert!(g::is_auto(999), "past the end is still auto, never a time");

        // The knee follows the ratio, and does so MONOTONICALLY: gentler
        // ratio, wider knee. This is the unit's "one switch, two things"
        // behaviour and the display reads the same function.
        let knees: Vec<f32> = (0..g::RATIO_VALUES.len() as u32).map(g::knee_db).collect();
        assert!(
            knees.windows(2).all(|w| w[0] > w[1]),
            "a harder ratio must not have a wider knee: {knees:?}"
        );
        assert!(
            knees.iter().all(|k| *k > 0.0),
            "a zero knee is a hard corner"
        );
        assert_eq!(
            g::knee_db(999),
            *knees.last().unwrap(),
            "past the end clamps"
        );
    }

    #[test]
    fn ids_are_dense_and_equal_to_their_index() {
        for (device, table) in ALL {
            for (i, p) in table.iter().enumerate() {
                assert_eq!(p.id, i as u32, "{device}:{}", p.name);
            }
        }
    }

    #[test]
    fn names_are_unique_within_a_device() {
        for (device, table) in ALL {
            for (i, a) in table.iter().enumerate() {
                for b in &table[i + 1..] {
                    assert_ne!(a.name, b.name, "{device}");
                }
            }
        }
    }

    #[test]
    fn ranges_are_ordered_and_defaults_lie_inside_them() {
        for (device, table) in ALL {
            for p in *table {
                assert!(p.min < p.max, "{device}:{}", p.name);
                assert!(
                    (p.min..=p.max).contains(&p.default),
                    "{device}:{} default {} outside [{}, {}]",
                    p.name,
                    p.default,
                    p.min,
                    p.max
                );
            }
        }
    }

    /// The one resonance mapping: flat stays flat at any drive, drive
    /// squashes the excess by exactly 1 + DRIVE_SQUASH at full tilt, and
    /// sub-flat requests floor at flat.
    #[test]
    fn effective_q_squashes_excess_and_floors_at_flat() {
        use filter::{DRIVE_SQUASH, FLAT_Q, effective_q};
        assert_eq!(effective_q(FLAT_Q, 0.0), FLAT_Q);
        assert_eq!(effective_q(FLAT_Q, 1.0), FLAT_Q);
        assert_eq!(effective_q(0.3, 0.7), FLAT_Q);
        let clean = effective_q(8.0, 0.0) - FLAT_Q;
        let driven = effective_q(8.0, 1.0) - FLAT_Q;
        assert!((clean / driven - (1.0 + DRIVE_SQUASH)).abs() < 1e-5);
    }

    /// The `<group> <label>` split the poly card leans on: every name
    /// has a group, and what is left is short enough to sit under a knob
    /// in a well that already carries the group as its title.
    #[test]
    fn poly_names_split_into_a_group_and_a_short_label() {
        for p in poly::TABLE {
            let label = poly::label(p.name);
            assert_ne!(label, p.name, "{} has no group word", p.name);
            assert!(!label.contains(' '), "{} has a two-word label", p.name);
            assert!(!label.is_empty(), "{} has an empty label", p.name);
        }
    }

    /// The two places a poly index is not the value it means. Both are
    /// written down beside their tables; these pin the arithmetic.
    #[test]
    fn poly_indices_convert_to_what_they_mean() {
        use poly::{OCT_CENTER, OCTAVES, UNISON, octave, unison};
        assert_eq!(octave(0), -4);
        assert_eq!(octave(OCT_CENTER), 0);
        assert_eq!(octave(OCTAVES.len() as u32 - 1), 4);
        assert_eq!(unison(0), 1);
        assert_eq!(unison(UNISON.len() as u32 - 1), 8);
    }

    /// The matrix rows and their vocabularies agree: every wire's three
    /// ids exist, src/dst ranges are exactly their name lists, and depth
    /// is bipolar. A list that outgrew its range is a choice the switch
    /// offers and the engine clamps away.
    #[test]
    fn poly_wire_rows_match_their_vocabularies() {
        use poly::{MOD_DST, MOD_SRC, TABLE, WIRE_IDS};
        for (src, dst, amt) in WIRE_IDS {
            let s = def(TABLE, src);
            let d = def(TABLE, dst);
            let a = def(TABLE, amt);
            assert_eq!(s.max as usize + 1, MOD_SRC.len(), "{}", s.name);
            assert_eq!(d.max as usize + 1, MOD_DST.len(), "{}", d.name);
            assert_eq!((s.min, d.min, s.default, d.default), (0.0, 0.0, 0.0, 0.0));
            assert!(a.min < 0.0 && a.max > 0.0, "{} is not bipolar", a.name);
        }
    }

    /// The red-zone lookup: known ids clamp, unknown ids drop.
    #[test]
    fn clamp_bounds_known_ids_and_refuses_unknown_ones() {
        assert_eq!(clamp(reverb::TABLE, reverb::MIX, 2.0), Some(1.0));
        assert_eq!(clamp(reverb::TABLE, reverb::MIX, -1.0), Some(0.0));
        assert_eq!(clamp(pan::TABLE, pan::PAN, 0.3), Some(0.3));
        assert_eq!(clamp(reverb::TABLE, 99, 0.5), None);
    }

    /// THE FADE CURVE IS MONOTONIC AND HITS BOTH ENDS, at every shape.
    ///
    /// Both properties are what makes it a fade rather than a shape: one
    /// that dipped would get louder halfway through a fade out, and one
    /// that missed an endpoint would either click at the start or never
    /// reach silence at the end.
    #[test]
    fn the_fade_curve_is_monotonic_and_hits_both_endpoints() {
        use clip::Curve;
        for step in -20..=20 {
            let shape = step as f32 / 20.0;
            let curve = Curve::new(shape);
            assert_eq!(curve.at(0.0), 0.0, "shape {shape} left endpoint");
            assert!(
                (curve.at(1.0) - 1.0).abs() < 1e-5,
                "shape {shape} right endpoint: {}",
                curve.at(1.0)
            );
            let mut previous = 0.0;
            for i in 0..=200 {
                let value = curve.at(i as f32 / 200.0);
                assert!(value.is_finite(), "shape {shape} at {i} is {value}");
                assert!(
                    (-1e-6..=1.0 + 1e-6).contains(&value),
                    "shape {shape} at {i} left the unit interval: {value}"
                );
                assert!(
                    value >= previous - 1e-6,
                    "shape {shape} fell at {i}: {value} after {previous}"
                );
                previous = value;
            }
        }
    }

    /// A shape of zero is EXACTLY linear, so a curve that has never been
    /// touched cannot change a fade that already sounded right.
    #[test]
    fn a_shape_of_zero_is_exactly_linear() {
        let curve = clip::Curve::LINEAR;
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            assert_eq!(curve.at(x), x);
        }
        assert_eq!(clip::Curve::default(), clip::Curve::LINEAR);
    }

    /// Opposite shapes are mirror images about the diagonal. This is the
    /// property the two-branch `k` mapping exists to keep, and the reason
    /// the obvious one-line version of it is wrong.
    #[test]
    fn opposite_shapes_are_mirror_images() {
        use clip::Curve;
        for step in 1..=18 {
            let shape = step as f32 / 20.0;
            let up = Curve::new(shape);
            let down = Curve::new(-shape);
            for i in 0..=50 {
                let x = i as f32 / 50.0;
                // Reflecting about the diagonal: y = f(x) becomes
                // x = g(y), so g(f(x)) is x again.
                let back = down.at(up.at(x));
                assert!(
                    (back - x).abs() < 1e-4,
                    "shape {shape} at {x} came back as {back}"
                );
            }
        }
    }

    /// A nonsense shape is a linear fade, not a NaN that silences a clip.
    #[test]
    fn a_nonsense_shape_falls_back_to_linear() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(clip::Curve::new(bad), clip::Curve::LINEAR);
        }
        assert_eq!(clip::Curve::new(9.0).shape, 1.0, "and out of range clamps");
        assert_eq!(clip::Curve::new(-9.0).shape, -1.0);
    }

    /// THE CHARACTER SWITCH: every position has a name, every name has a
    /// position, and the table's range covers exactly the list it
    /// indexes. The glue switches' rule, for the same reason — a names
    /// list one longer than its values is a position that reads one thing
    /// and does another, and nothing fails to compile.
    #[test]
    fn the_filter_characters_and_their_names_line_up() {
        use filter as f;
        assert_eq!(f::CHARACTERS.len(), f::CHARACTER_NAMES.len());
        assert_eq!(f::CHAR_MAX as usize + 1, f::CHARACTERS.len());
        for (i, name) in f::CHARACTER_NAMES.iter().enumerate() {
            assert_eq!(&f::character(i as u32).name, name, "position {i}");
        }
        // The row spans exactly the list.
        let def = def(f::TABLE, f::CHARACTER);
        assert_eq!(def.min, 0.0);
        assert_eq!(def.max, f::CHAR_MAX as f32);
        // A stale index still filters rather than reading past the end.
        assert_eq!(f::character(99).name, "clean");
    }

    /// THE CHARACTERS ARE ACTUALLY DIFFERENT. A switch whose positions
    /// cannot be told apart is a switch nobody moves twice — so the thing
    /// worth pinning is that they diverge, not that any one of them holds
    /// a particular number.
    #[test]
    fn the_filter_characters_do_different_things() {
        use filter as f;
        let q = 12.0;
        let driven: Vec<f32> = (0..f::CHARACTERS.len())
            .map(|i| f::effective_q_for(q, 1.0, i as u32))
            .collect();

        // Every pair differs audibly at full drive.
        for (i, a) in driven.iter().enumerate() {
            for (j, b) in driven.iter().enumerate().skip(i + 1) {
                assert!(
                    (a - b).abs() > 0.05,
                    "characters {i} and {j} squash the same: {a} vs {b}"
                );
            }
        }

        // The ordering the docs claim: an OTA holds its peak, a diode
        // gives it up first.
        let ota = f::effective_q_for(q, 1.0, f::CHAR_OTA);
        let ladder = f::effective_q_for(q, 1.0, f::CHAR_LADDER);
        let diode = f::effective_q_for(q, 1.0, f::CHAR_DIODE);
        assert!(ota > ladder, "the OTA should hold on: {ota} vs {ladder}");
        assert!(ladder > diode, "the diode should fold first");

        // CLEAN IS THE OLD BEHAVIOUR, exactly — a project saved before
        // there were characters must still sound the way it did.
        for drive in [0.0f32, 0.5, 1.0] {
            for res in [0.3f32, 1.0, 24.0] {
                assert_eq!(
                    f::effective_q_for(res, drive, f::CHAR_CLEAN),
                    f::effective_q(res, drive),
                    "clean drifted from the original at {res}/{drive}"
                );
                assert_eq!(
                    f::shaper_drive_for(drive, f::CHAR_CLEAN),
                    f::shaper_drive(drive)
                );
            }
        }

        // Nothing any character does makes the resonance non-finite or
        // takes it below flat.
        for i in 0..f::CHARACTERS.len() as u32 {
            for drive in [0.0f32, 0.5, 1.0] {
                for res in [0.05f32, f::FLAT_Q, 24.0] {
                    let q = f::effective_q_for(res, drive, i);
                    assert!(q.is_finite() && q >= f::FLAT_Q - 1e-6, "{i}: {q}");
                }
                let d = f::shaper_drive_for(drive, i);
                assert!(d >= crate::dsp::shaper::DRIVE_MIN && d <= crate::dsp::shaper::DRIVE_MAX);
            }
        }
    }

    /// THE SPREAD WIDENS AROUND THE CORNER RATHER THAN DETUNING IT. The
    /// cutoff you set stays the centre of what you hear, so turning
    /// spread up never moves the filter — it only opens it out.
    #[test]
    fn the_spread_opens_symmetrically_around_the_cutoff() {
        use filter as f;
        for cutoff in [100.0f32, 1_000.0, 8_000.0] {
            // At zero it is exactly the knob, both sides.
            assert!((f::spread_cutoff(cutoff, 0.0, -1.0) - cutoff).abs() < 1e-3);
            assert!((f::spread_cutoff(cutoff, 0.0, 1.0) - cutoff).abs() < 1e-3);

            for spread in [1.0f32, 6.0, f::SPREAD_MAX_ST] {
                let lo = f::spread_cutoff(cutoff, spread, -1.0);
                let hi = f::spread_cutoff(cutoff, spread, 1.0);
                assert!(lo < cutoff && cutoff < hi, "at {cutoff}/{spread}");
                // The GEOMETRIC mean is the corner: equal musical
                // intervals either side, which is what makes this a width
                // control rather than a detune.
                let centre = (lo * hi).sqrt();
                assert!(
                    (centre - cutoff).abs() < cutoff * 1e-3,
                    "the spread moved the corner: {centre} against {cutoff}"
                );
            }
        }

        // Both sides stay inside the table's own cutoff range, however
        // hard the corner is pushed against a rail.
        let row = def(f::TABLE, f::CUTOFF);
        for cutoff in [row.min, row.max] {
            for side in [-1.0f32, 1.0] {
                let hz = f::spread_cutoff(cutoff, f::SPREAD_MAX_ST, side);
                assert!(hz >= row.min - 1e-3 && hz <= row.max + 1e-3, "{hz}");
            }
        }
    }
}
