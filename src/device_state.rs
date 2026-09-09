//! A device instance, its editable state, and the knob adapters.
//!
//! `DeviceState` is what one device on a track is currently set to, in
//! ENGINE units; `DeviceInstance` is that state plus the stable id a
//! modulation wire names. The rest of the file is the translation layer
//! between those units and what a card draws — `device_norm` and
//! `device_value` are the two directions, and the `*_knobs` functions
//! build one card's worth of positions.
//!
//! Lifted out of `main.rs` unchanged.

use crate::devices::DeviceKind;
use daw::audio::graph::SynthParams;
use daw::ui::device;

/// The reverb's editable values in ENGINE units (`0..=1` each, the ranges
/// `daw::params::reverb` declares). The synth's equivalent is
/// `graph::SynthParams`, which the engine already owns.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ReverbParams {
    pub predelay_ms: f32,
    pub size: f32,
    pub decay: f32,
    pub damp: f32,
    pub low_cut: f32,
    pub diffusion: f32,
    pub modulation: f32,
    pub width: f32,
    pub mix: f32,
}

impl Default for ReverbParams {
    fn default() -> Self {
        use daw::params::{def, reverb};
        let at = |id: u32| def(reverb::TABLE, id).default;
        Self {
            predelay_ms: at(reverb::PREDELAY),
            size: at(reverb::SIZE),
            decay: at(reverb::DECAY),
            damp: at(reverb::DAMP),
            low_cut: at(reverb::LOWCUT),
            diffusion: at(reverb::DIFFUSION),
            modulation: at(reverb::MODULATION),
            width: at(reverb::WIDTH),
            mix: at(reverb::MIX),
        }
    }
}

/// The saturator's editable values in ENGINE units — the kernel's own
/// (`1..32` of drive, `-0.9..0.9` of bias, `0..4` of linear gain), as
/// `daw::params::sat` declares them.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EchoParams {
    pub sync: f32,
    pub time_ms: f32,
    pub feedback: f32,
    pub tone_hz: f32,
    pub drive: f32,
    pub wow: f32,
    pub spread: f32,
    pub mix: f32,
    /// 0 = an insert; above 0 the delay is an aux at this level. See
    /// `params::echo::SEND` — the knob decides the TOPOLOGY, so
    /// `shape_hash` watches whether it is zero.
    pub send: f32,
}

impl Default for EchoParams {
    fn default() -> Self {
        use daw::params::{def, echo};
        Self {
            sync: def(echo::TABLE, echo::SYNC).default,
            time_ms: def(echo::TABLE, echo::TIME).default,
            feedback: def(echo::TABLE, echo::FEEDBACK).default,
            tone_hz: def(echo::TABLE, echo::TONE).default,
            drive: def(echo::TABLE, echo::DRIVE).default,
            wow: def(echo::TABLE, echo::WOW).default,
            spread: def(echo::TABLE, echo::SPREAD).default,
            mix: def(echo::TABLE, echo::MIX).default,
            send: def(echo::TABLE, echo::SEND).default,
        }
    }
}

/// The saturator's editable values in ENGINE units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SatParams {
    /// The mode as its wire INDEX, kept as f32 like every other engine
    /// value here. A `u32` field would round on the way in, and a device
    /// state that cannot hand back exactly what was set to it is a device
    /// whose automation and modulation quietly disagree with its knobs.
    pub mode: f32,
    pub drive: f32,
    pub bias: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for SatParams {
    fn default() -> Self {
        use daw::params::{def, sat};
        Self {
            mode: def(sat::TABLE, sat::MODE).default,
            drive: def(sat::TABLE, sat::DRIVE).default,
            bias: def(sat::TABLE, sat::BIAS).default,
            mix: def(sat::TABLE, sat::MIX).default,
            out: def(sat::TABLE, sat::OUT).default,
        }
    }
}

/// The lo-fi converter's editable values in ENGINE units — hertz, a bit
/// count, a fraction and a linear gain, exactly as
/// [`daw::params::lofi`] declares them.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LofiParams {
    pub rate: f32,
    pub bits: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for LofiParams {
    fn default() -> Self {
        use daw::params::{def, lofi};
        Self {
            rate: def(lofi::TABLE, lofi::RATE).default,
            bits: def(lofi::TABLE, lofi::BITS).default,
            mix: def(lofi::TABLE, lofi::MIX).default,
            out: def(lofi::TABLE, lofi::OUT).default,
        }
    }
}

/// The sheen's editable values in ENGINE units — the kernel's own
/// multiplier, a corner in hertz, a fraction and a linear gain, exactly
/// as [`daw::params::sheen`] declares them.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SheenParams {
    pub amount: f32,
    pub edge: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for SheenParams {
    fn default() -> Self {
        use daw::params::{def, sheen};
        Self {
            amount: def(sheen::TABLE, sheen::AMOUNT).default,
            edge: def(sheen::TABLE, sheen::EDGE).default,
            mix: def(sheen::TABLE, sheen::MIX).default,
            out: def(sheen::TABLE, sheen::OUT).default,
        }
    }
}

/// The disperser's editable values in ENGINE units — a section count, a
/// corner in hertz and a Q, exactly as [`daw::params::disperser`]
/// declares them.
///
/// No mix and no trim, which is the device: see the params module for why
/// blending this one against the dry would break its flat-magnitude
/// promise.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DisperserParams {
    pub amount: f32,
    pub freq: f32,
    pub pinch: f32,
}

impl Default for DisperserParams {
    fn default() -> Self {
        use daw::params::{def, disperser};
        Self {
            amount: def(disperser::TABLE, disperser::AMOUNT).default,
            freq: def(disperser::TABLE, disperser::FREQ).default,
            pinch: def(disperser::TABLE, disperser::PINCH).default,
        }
    }
}

/// The tilt's editable values in ENGINE units — a lean in dB and a pivot
/// in hertz, exactly as [`daw::params::tilt`] declares them.
///
/// Two rows and no trim, which is the device: a see-saw is unity at its
/// pivot, so there is no make-up to make up.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TiltParams {
    pub tilt: f32,
    pub pivot: f32,
}

impl Default for TiltParams {
    fn default() -> Self {
        use daw::params::{def, tilt};
        Self {
            tilt: def(tilt::TABLE, tilt::TILT).default,
            pivot: def(tilt::TABLE, tilt::PIVOT).default,
        }
    }
}

/// The phaser's editable values in ENGINE units, exactly as
/// [`daw::params::phaser`] declares them.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PhaserParams {
    pub amount: f32,
    pub centre: f32,
    pub depth: f32,
    pub rate: f32,
    pub mix: f32,
}

impl Default for PhaserParams {
    fn default() -> Self {
        use daw::params::{def, phaser};
        Self {
            amount: def(phaser::TABLE, phaser::AMOUNT).default,
            centre: def(phaser::TABLE, phaser::CENTRE).default,
            depth: def(phaser::TABLE, phaser::DEPTH).default,
            rate: def(phaser::TABLE, phaser::RATE).default,
            mix: def(phaser::TABLE, phaser::MIX).default,
        }
    }
}

/// What a device IS, and its editable values — in ENGINE units, the same
/// numbers `src/params.rs` declares. One stored copy of one truth: the card
/// converts to normalized knob positions for drawing and back on the way
/// out, and `parameter_base`, `build_graph_spec` and the modulation plan all
/// read these directly.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DeviceState {
    SineSynth(SynthParams),
    Poly(daw::audio::poly::PolyParams),
    Loom(daw::audio::loom::LoomParams),
    Tine(daw::audio::tine::TineParams),
    Scomp(daw::scomp::ScompParams),
    Stab(daw::audio::stab::StabParams),
    Quad(daw::audio::quad::QuadParams),
    Brick(daw::audio::brick::BrickParams),
    Kit(daw::audio::kit::KitParams),
    Haze(daw::audio::haze::HazeParams),
    Sampler(daw::audio::sampler::SamplerParams),
    Kick(daw::audio::kick::KickParams),
    Snare(daw::audio::snare::SnareParams),
    Tom(daw::audio::tom::TomParams),
    Hat(daw::audio::hat::HatParams),
    Handclap(daw::audio::handclap::HandclapParams),
    Drum(daw::audio::drum::DrumParams),
    Thump(daw::audio::thump::ThumpParams),
    Clay(daw::audio::clay::ClayParams),
    Table(daw::audio::table::TableParams),
    Ring(daw::audio::ring::RingParams),
    PrismVoice(daw::audio::prism_voice::PrismVoiceParams),
    Mass(daw::audio::mass::MassParams),
    Pluck(daw::audio::pluck::PluckParams),
    Vox(daw::audio::vox::VoxParams),
    Pipe(daw::audio::pipe::PipeParams),
    Glass(daw::audio::glass::GlassParams),
    Rom(daw::audio::rom::RomParams),
    Reverb(ReverbParams),
    Sat(SatParams),
    Lofi(LofiParams),
    Sheen(SheenParams),
    Disperser(DisperserParams),
    Tilt(TiltParams),
    Phaser(PhaserParams),
    Echo(EchoParams),
    Eq(daw::audio::eq::EqParams),
    Filter(daw::audio::filter::FilterParams),
    Glue(daw::audio::glue::GlueParams),
    Clamp(daw::audio::clamp::ClampParams),
    Prism(daw::audio::prism::PrismParams),
    Gate(daw::audio::gate::GateParams),
    Strip(daw::audio::strip::StripParams),
    Resyn(daw::audio::resyn::ResynParams),
    Acid(daw::audio::acid::AcidParams),
    /// A RACK: a container, and nothing else.
    ///
    /// It has no parameters and makes no sound. Its children sit beside
    /// it in the same flat chain, pointing at it through
    /// `DeviceInstance::parent`, and the graph builder simply skips it —
    /// so a rack costs the audio path exactly one match arm that does
    /// nothing.
    ///
    /// Its macros are not here either: they carry a name and a list of
    /// targets, neither of which is `Copy`, so they live on the track in
    /// `Track::racks` the way a sampler's file does.
    Rack,
    Limiter(daw::audio::limiter::LimiterParams),
    Modulato(daw::audio::modulato::ModulatoParams),
    Flint(daw::audio::flint::FlintParams),
    Sibyl(daw::audio::sibyl::SibylParams),
    Ferric(daw::audio::ferric::FerricParams),
    Umbra(daw::audio::umbra::UmbraParams),
    Tone(daw::audio::tone::ToneParams),
    Sigil(daw::audio::sigil::SigilParams),
    Gauge(daw::audio::gauge::GaugeParams),
    Utility(daw::audio::utility::UtilityParams),
}

impl DeviceState {
    /// A device of `kind` at its table defaults.
    pub fn new(kind: DeviceKind) -> Self {
        match kind {
            DeviceKind::SineSynth => Self::SineSynth(SynthParams::default()),
            DeviceKind::Poly => Self::Poly(daw::audio::poly::PolyParams::default()),
            DeviceKind::Loom => Self::Loom(daw::audio::loom::LoomParams::default()),
            DeviceKind::Tine => Self::Tine(daw::audio::tine::TineParams::default()),
            DeviceKind::Scomp => Self::Scomp(daw::scomp::ScompParams::default()),
            DeviceKind::Stab => Self::Stab(daw::audio::stab::StabParams::default()),
            DeviceKind::Quad => Self::Quad(daw::audio::quad::QuadParams::default()),
            DeviceKind::Brick => Self::Brick(daw::audio::brick::BrickParams::default()),
            DeviceKind::Kit => Self::Kit(daw::audio::kit::KitParams::default()),
            DeviceKind::Haze => Self::Haze(daw::audio::haze::HazeParams::default()),
            DeviceKind::Sampler => Self::Sampler(daw::audio::sampler::SamplerParams::default()),
            DeviceKind::Kick => Self::Kick(daw::audio::kick::KickParams::default()),
            DeviceKind::Snare => Self::Snare(daw::audio::snare::SnareParams::default()),
            DeviceKind::Tom => Self::Tom(daw::audio::tom::TomParams::default()),
            DeviceKind::Hat => Self::Hat(daw::audio::hat::HatParams::default()),
            DeviceKind::Handclap => Self::Handclap(daw::audio::handclap::HandclapParams::default()),
            DeviceKind::Drum => Self::Drum(daw::audio::drum::DrumParams::default()),
            DeviceKind::Thump => Self::Thump(daw::audio::thump::ThumpParams::default()),
            DeviceKind::Clay => Self::Clay(daw::audio::clay::ClayParams::default()),
            DeviceKind::Table => Self::Table(daw::audio::table::TableParams::default()),
            DeviceKind::Ring => Self::Ring(daw::audio::ring::RingParams::default()),
            DeviceKind::PrismVoice => {
                Self::PrismVoice(daw::audio::prism_voice::PrismVoiceParams::default())
            }
            DeviceKind::Mass => Self::Mass(daw::audio::mass::MassParams::default()),
            DeviceKind::Pluck => Self::Pluck(daw::audio::pluck::PluckParams::default()),
            DeviceKind::Vox => Self::Vox(daw::audio::vox::VoxParams::default()),
            DeviceKind::Pipe => Self::Pipe(daw::audio::pipe::PipeParams::default()),
            DeviceKind::Glass => Self::Glass(daw::audio::glass::GlassParams::default()),
            DeviceKind::Rom => Self::Rom(daw::audio::rom::RomParams::default()),
            DeviceKind::Reverb => Self::Reverb(ReverbParams::default()),
            DeviceKind::Sat => Self::Sat(SatParams::default()),
            DeviceKind::Lofi => Self::Lofi(LofiParams::default()),
            DeviceKind::Sheen => Self::Sheen(SheenParams::default()),
            DeviceKind::Disperser => Self::Disperser(DisperserParams::default()),
            DeviceKind::Tilt => Self::Tilt(TiltParams::default()),
            DeviceKind::Phaser => Self::Phaser(PhaserParams::default()),
            DeviceKind::Echo => Self::Echo(EchoParams::default()),
            DeviceKind::Eq => Self::Eq(daw::audio::eq::EqParams::default()),
            DeviceKind::Filter => Self::Filter(daw::audio::filter::FilterParams::default()),
            DeviceKind::Glue => Self::Glue(daw::audio::glue::GlueParams::default()),
            DeviceKind::Clamp => Self::Clamp(daw::audio::clamp::ClampParams::default()),
            DeviceKind::Prism => Self::Prism(daw::audio::prism::PrismParams::default()),
            DeviceKind::Gate => Self::Gate(daw::audio::gate::GateParams::default()),
            DeviceKind::Strip => Self::Strip(daw::audio::strip::StripParams::default()),
            DeviceKind::Resyn => Self::Resyn(daw::audio::resyn::ResynParams::default()),
            DeviceKind::Acid => Self::Acid(daw::audio::acid::AcidParams::default()),
            DeviceKind::Rack => Self::Rack,
            // A console section never reaches this binary; it is the stage's.
            DeviceKind::Console(_) => Self::Rack,
            DeviceKind::Limiter => Self::Limiter(daw::audio::limiter::LimiterParams::default()),
            DeviceKind::Modulato => Self::Modulato(daw::audio::modulato::ModulatoParams::default()),
            DeviceKind::Flint => Self::Flint(daw::audio::flint::FlintParams::default()),
            DeviceKind::Sibyl => Self::Sibyl(daw::audio::sibyl::SibylParams::default()),
            DeviceKind::Ferric => Self::Ferric(daw::audio::ferric::FerricParams::default()),
            DeviceKind::Umbra => Self::Umbra(daw::audio::umbra::UmbraParams::default()),
            DeviceKind::Tone => Self::Tone(daw::audio::tone::ToneParams::default()),
            DeviceKind::Sigil => Self::Sigil(daw::audio::sigil::SigilParams::default()),
            DeviceKind::Gauge => Self::Gauge(daw::audio::gauge::GaugeParams::default()),
            DeviceKind::Utility => Self::Utility(daw::audio::utility::UtilityParams::default()),
        }
    }

    pub fn kind(self) -> DeviceKind {
        match self {
            Self::SineSynth(_) => DeviceKind::SineSynth,
            Self::Poly(_) => DeviceKind::Poly,
            Self::Loom(_) => DeviceKind::Loom,
            Self::Tine(_) => DeviceKind::Tine,
            Self::Scomp(_) => DeviceKind::Scomp,
            Self::Stab(_) => DeviceKind::Stab,
            Self::Quad(_) => DeviceKind::Quad,
            Self::Brick(_) => DeviceKind::Brick,
            Self::Kit(_) => DeviceKind::Kit,
            Self::Haze(_) => DeviceKind::Haze,
            Self::Sampler(_) => DeviceKind::Sampler,
            Self::Kick(_) => DeviceKind::Kick,
            Self::Snare(_) => DeviceKind::Snare,
            Self::Tom(_) => DeviceKind::Tom,
            Self::Hat(_) => DeviceKind::Hat,
            Self::Handclap(_) => DeviceKind::Handclap,
            Self::Drum(_) => DeviceKind::Drum,
            Self::Thump(_) => DeviceKind::Thump,
            Self::Clay(_) => DeviceKind::Clay,
            Self::Table(_) => DeviceKind::Table,
            Self::Ring(_) => DeviceKind::Ring,
            Self::PrismVoice(_) => DeviceKind::PrismVoice,
            Self::Mass(_) => DeviceKind::Mass,
            Self::Pluck(_) => DeviceKind::Pluck,
            Self::Vox(_) => DeviceKind::Vox,
            Self::Pipe(_) => DeviceKind::Pipe,
            Self::Glass(_) => DeviceKind::Glass,
            Self::Rom(_) => DeviceKind::Rom,
            Self::Reverb(_) => DeviceKind::Reverb,
            Self::Sat(_) => DeviceKind::Sat,
            Self::Lofi(_) => DeviceKind::Lofi,
            Self::Sheen(_) => DeviceKind::Sheen,
            Self::Disperser(_) => DeviceKind::Disperser,
            Self::Tilt(_) => DeviceKind::Tilt,
            Self::Phaser(_) => DeviceKind::Phaser,
            Self::Echo(_) => DeviceKind::Echo,
            Self::Eq(_) => DeviceKind::Eq,
            Self::Filter(_) => DeviceKind::Filter,
            Self::Glue(_) => DeviceKind::Glue,
            Self::Clamp(_) => DeviceKind::Clamp,
            Self::Prism(_) => DeviceKind::Prism,
            Self::Gate(_) => DeviceKind::Gate,
            Self::Strip(_) => DeviceKind::Strip,
            Self::Resyn(_) => DeviceKind::Resyn,
            Self::Acid(_) => DeviceKind::Acid,
            Self::Rack => DeviceKind::Rack,
            Self::Limiter(_) => DeviceKind::Limiter,
            Self::Modulato(_) => DeviceKind::Modulato,
            Self::Flint(_) => DeviceKind::Flint,
            Self::Sibyl(_) => DeviceKind::Sibyl,
            Self::Ferric(_) => DeviceKind::Ferric,
            Self::Umbra(_) => DeviceKind::Umbra,
            Self::Tone(_) => DeviceKind::Tone,
            Self::Sigil(_) => DeviceKind::Sigil,
            Self::Gauge(_) => DeviceKind::Gauge,
            Self::Utility(_) => DeviceKind::Utility,
        }
    }

    /// This device's value for `param`, or `None` for an id it does not
    /// have — which is how a target aimed at the wrong kind is refused.
    pub fn value(self, param: u32) -> Option<f32> {
        use daw::params::{disperser, echo, lofi, phaser, reverb, sat, seq, sheen, tilt};
        match self {
            Self::SineSynth(p) => match param {
                seq::GAIN => Some(p.gain),
                seq::ATTACK => Some(p.attack_ms),
                seq::RELEASE => Some(p.release_ms),
                _ => None,
            },
            // The poly synth has thirty-four rows and its own reader,
            // beside its writer, in the engine struct that owns them —
            // spelling them out twice here is how the two halves drift.
            Self::Poly(p) => p.get(param),
            Self::Loom(p) => p.get(param),
            Self::Tine(p) => p.get(param),
            Self::Scomp(p) => p.get(param),
            Self::Stab(p) => p.get(param),
            Self::Quad(p) => p.get(param),
            Self::Brick(p) => p.get(param),
            Self::Kit(p) => p.get(param),
            Self::Haze(p) => p.get(param),
            Self::Flint(p) => p.get(param),
            Self::Sibyl(p) => p.get(param),
            Self::Ferric(p) => p.get(param),
            Self::Umbra(p) => p.get(param),
            Self::Tone(p) => p.get(param),
            Self::Sigil(p) => p.get(param),
            Self::Gauge(p) => p.get(param),
            // Thirty-six rows, with a reader beside their writer in the
            // struct that owns them — spelling them out again here is how
            // the two halves drift.
            Self::Sampler(p) => p.get(param),
            // The kick has its own reader beside its writer, in the
            // struct that owns them — spelling thirteen rows out again
            // here is how the two halves drift.
            Self::Kick(p) => Some(p.get(param)),
            // Every drum after the kick keeps a reader beside its writer
            // in the struct that owns them, for the same reason: spelling
            // the rows out again here is how the two halves drift.
            Self::Snare(p) => Some(p.get(param)),
            Self::Tom(p) => Some(p.get(param)),
            Self::Hat(p) => Some(p.get(param)),
            Self::Handclap(p) => Some(p.get(param)),
            Self::Drum(p) => Some(p.get(param)),
            Self::Thump(p) => Some(p.get(param)),
            Self::Clay(p) => Some(p.get(param)),
            Self::Table(p) => daw::params::table::TABLE
                .get(param as usize)
                .map(|_| p.get(param)),
            Self::Ring(p) => daw::params::ring::TABLE
                .get(param as usize)
                .map(|_| p.get(param)),
            Self::PrismVoice(p) => daw::params::prism_voice::TABLE
                .get(param as usize)
                .map(|_| p.get(param)),
            Self::Mass(p) => daw::params::mass::TABLE
                .get(param as usize)
                .map(|_| p.get(param)),
            Self::Pluck(p) => daw::params::pluck::TABLE
                .get(param as usize)
                .map(|_| p.get(param)),
            Self::Vox(p) => daw::params::vox::TABLE
                .get(param as usize)
                .map(|_| p.get(param)),
            Self::Pipe(p) => daw::params::pipe::TABLE
                .get(param as usize)
                .map(|_| p.get(param)),
            Self::Glass(p) => daw::params::glass::TABLE
                .get(param as usize)
                .map(|_| p.get(param)),
            Self::Rom(p) => daw::params::rom::TABLE
                .get(param as usize)
                .map(|_| p.get(param)),
            Self::Reverb(p) => match param {
                reverb::PREDELAY => Some(p.predelay_ms),
                reverb::SIZE => Some(p.size),
                reverb::DECAY => Some(p.decay),
                reverb::DAMP => Some(p.damp),
                reverb::LOWCUT => Some(p.low_cut),
                reverb::DIFFUSION => Some(p.diffusion),
                reverb::MODULATION => Some(p.modulation),
                reverb::WIDTH => Some(p.width),
                reverb::MIX => Some(p.mix),
                _ => None,
            },
            Self::Lofi(p) => match param {
                lofi::RATE => Some(p.rate),
                lofi::BITS => Some(p.bits),
                lofi::MIX => Some(p.mix),
                lofi::OUT => Some(p.out),
                _ => None,
            },
            Self::Sheen(p) => match param {
                sheen::AMOUNT => Some(p.amount),
                sheen::EDGE => Some(p.edge),
                sheen::MIX => Some(p.mix),
                sheen::OUT => Some(p.out),
                _ => None,
            },
            Self::Disperser(p) => match param {
                disperser::AMOUNT => Some(p.amount),
                disperser::FREQ => Some(p.freq),
                disperser::PINCH => Some(p.pinch),
                _ => None,
            },
            Self::Tilt(p) => match param {
                tilt::TILT => Some(p.tilt),
                tilt::PIVOT => Some(p.pivot),
                _ => None,
            },
            Self::Phaser(p) => match param {
                phaser::AMOUNT => Some(p.amount),
                phaser::CENTRE => Some(p.centre),
                phaser::DEPTH => Some(p.depth),
                phaser::RATE => Some(p.rate),
                phaser::MIX => Some(p.mix),
                _ => None,
            },
            Self::Sat(p) => match param {
                sat::MODE => Some(p.mode),
                sat::DRIVE => Some(p.drive),
                sat::BIAS => Some(p.bias),
                sat::MIX => Some(p.mix),
                sat::OUT => Some(p.out),
                _ => None,
            },
            Self::Echo(p) => match param {
                echo::SYNC => Some(p.sync),
                echo::TIME => Some(p.time_ms),
                echo::FEEDBACK => Some(p.feedback),
                echo::TONE => Some(p.tone_hz),
                echo::DRIVE => Some(p.drive),
                echo::WOW => Some(p.wow),
                echo::SPREAD => Some(p.spread),
                echo::MIX => Some(p.mix),
                echo::SEND => Some(p.send),
                _ => None,
            },
            // Forty-one rows with their own reader, beside their own
            // writer, in the struct that owns them — spelling them out
            // twice here is how the two halves drift.
            Self::Eq(p) => p.get(param),
            // Seven rows with their own reader beside their own writer,
            // in the struct that owns them.
            Self::Filter(p) => Some(p.get(param)),
            Self::Glue(p) => p.get(param),
            Self::Clamp(p) => p.get(param),
            Self::Prism(p) => p.get(param),
            Self::Gate(p) => p.get(param),
            Self::Strip(p) => p.get(param),
            Self::Resyn(p) => p.get(param),
            Self::Acid(p) => p.get(param),
            // A rack has no parameters, so every id is unknown to it.
            Self::Rack => None,
            // Its own reader beside its own writer, in the struct that
            // owns them — spelling seven rows out again here is how the
            // two halves drift.
            Self::Limiter(p) => Some(p.get(param)),
            Self::Modulato(p) => Some(p.get(param)),
            // Seven rows with their own reader beside their own writer,
            // in the struct that owns them.
            Self::Utility(p) => p.get(param),
        }
    }

    /// Store an engine-unit value. Unknown ids are dropped, exactly as the
    /// engine's own clamp drops them.
    pub fn set(&mut self, param: u32, value: f32) {
        use daw::params::{disperser, echo, lofi, phaser, reverb, sat, seq, sheen, tilt};
        match self {
            Self::SineSynth(p) => match param {
                seq::GAIN => p.gain = value,
                seq::ATTACK => p.attack_ms = value,
                seq::RELEASE => p.release_ms = value,
                _ => {}
            },
            Self::Poly(p) => p.set(param, value),
            Self::Loom(p) => p.set(param, value),
            Self::Tine(p) => p.set(param, value),
            Self::Scomp(p) => p.set(param, value),
            Self::Stab(p) => p.set(param, value),
            Self::Quad(p) => p.set(param, value),
            Self::Brick(p) => p.set(param, value),
            Self::Kit(p) => p.set(param, value),
            Self::Haze(p) => p.set(param, value),
            Self::Flint(p) => p.set(param, value),
            Self::Sibyl(p) => p.set(param, value),
            Self::Ferric(p) => p.set(param, value),
            Self::Umbra(p) => p.set(param, value),
            Self::Tone(p) => p.set(param, value),
            Self::Sigil(p) => p.set(param, value),
            Self::Gauge(p) => p.set(param, value),
            Self::Sampler(p) => p.set(param, value),
            Self::Kick(p) => p.set(param, value),
            Self::Snare(p) => p.set(param, value),
            Self::Tom(p) => p.set(param, value),
            Self::Hat(p) => p.set(param, value),
            Self::Handclap(p) => p.set(param, value),
            Self::Drum(p) => p.set(param, value),
            Self::Thump(p) => p.set(param, value),
            Self::Clay(p) => p.set(param, value),
            Self::Table(p) => p.set(param, value),
            Self::Ring(p) => p.set(param, value),
            Self::PrismVoice(p) => p.set(param, value),
            Self::Mass(p) => p.set(param, value),
            Self::Pluck(p) => p.set(param, value),
            Self::Vox(p) => p.set(param, value),
            Self::Pipe(p) => p.set(param, value),
            Self::Glass(p) => p.set(param, value),
            Self::Rom(p) => p.set(param, value),
            Self::Reverb(p) => match param {
                reverb::PREDELAY => p.predelay_ms = value,
                reverb::SIZE => p.size = value,
                reverb::DECAY => p.decay = value,
                reverb::DAMP => p.damp = value,
                reverb::LOWCUT => p.low_cut = value,
                reverb::DIFFUSION => p.diffusion = value,
                reverb::MODULATION => p.modulation = value,
                reverb::WIDTH => p.width = value,
                reverb::MIX => p.mix = value,
                _ => {}
            },
            Self::Lofi(p) => match param {
                lofi::RATE => p.rate = value,
                lofi::BITS => p.bits = value,
                lofi::MIX => p.mix = value,
                lofi::OUT => p.out = value,
                _ => {}
            },
            Self::Sheen(p) => match param {
                sheen::AMOUNT => p.amount = value,
                sheen::EDGE => p.edge = value,
                sheen::MIX => p.mix = value,
                sheen::OUT => p.out = value,
                _ => {}
            },
            Self::Disperser(p) => match param {
                disperser::AMOUNT => p.amount = value,
                disperser::FREQ => p.freq = value,
                disperser::PINCH => p.pinch = value,
                _ => {}
            },
            Self::Tilt(p) => match param {
                tilt::TILT => p.tilt = value,
                tilt::PIVOT => p.pivot = value,
                _ => {}
            },
            Self::Phaser(p) => match param {
                phaser::AMOUNT => p.amount = value,
                phaser::CENTRE => p.centre = value,
                phaser::DEPTH => p.depth = value,
                phaser::RATE => p.rate = value,
                phaser::MIX => p.mix = value,
                _ => {}
            },
            Self::Sat(p) => match param {
                sat::MODE => p.mode = value,
                sat::DRIVE => p.drive = value,
                sat::BIAS => p.bias = value,
                sat::MIX => p.mix = value,
                sat::OUT => p.out = value,
                _ => {}
            },
            Self::Echo(p) => match param {
                echo::SYNC => p.sync = value,
                echo::TIME => p.time_ms = value,
                echo::FEEDBACK => p.feedback = value,
                echo::TONE => p.tone_hz = value,
                echo::DRIVE => p.drive = value,
                echo::WOW => p.wow = value,
                echo::SPREAD => p.spread = value,
                echo::MIX => p.mix = value,
                echo::SEND => p.send = value,
                _ => {}
            },
            Self::Eq(p) => p.set(param, value),
            Self::Filter(p) => p.set(param, value),
            Self::Glue(p) => p.set(param, value),
            Self::Clamp(p) => p.set(param, value),
            Self::Prism(p) => p.set(param, value),
            Self::Gate(p) => p.set(param, value),
            Self::Strip(p) => p.set(param, value),
            Self::Resyn(p) => p.set(param, value),
            Self::Acid(p) => p.set(param, value),
            Self::Rack => {}
            Self::Limiter(p) => p.set(param, value),
            Self::Modulato(p) => p.set(param, value),
            Self::Utility(p) => p.set(param, value),
        }
    }
}

/// One device on a track, with a STABLE identity.
///
/// Automation and modulation address a device by `id`, never by position:
/// reordering the chain must not break a wire. The id is minted from the
/// arrangement's one counter, so it is unique across the whole document.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceInstance {
    pub id: u64,
    pub state: DeviceState,
    /// The RACK this device lives inside, if any.
    ///
    /// The chain stays a flat `Vec` and nesting is a pointer upward, not
    /// a `Vec` downward. That is the whole reason a rack was affordable:
    /// `DeviceInstance` and `DeviceState` are both `Copy`, and putting a
    /// list of children inside either would have rippled through every
    /// `match instance.state` in the app. An `Option<u64>` is `Copy`, and
    /// the order a rack's devices run in is the order they already sit in
    /// the chain.
    ///
    /// A rack's own instance carries `None` — racks do not nest yet, and
    /// the day they do this field is already the shape for it.
    #[serde(default)]
    pub parent: Option<u64>,
    /// Which page of this device's card is open — the poly synth's tab,
    /// and the equaliser's SELECTED BAND, which is the same idea wearing
    /// a different hat: the one part of a card that is about where you
    /// are working rather than about the sound.
    ///
    /// It lives on the INSTANCE because a card is rebuilt from engine
    /// units every frame, so anything the card alone remembers is
    /// forgotten before the next one. And it is stored, because which
    /// section you were working in is part of the patch you come back to.
    #[serde(default)]
    pub page: u8,
    /// How far a card's display is zoomed in, as a multiple of the whole
    /// thing, and where its left edge sits as a fraction of it.
    ///
    /// On the INSTANCE for the reason `page` is: a card is rebuilt from
    /// engine units every frame, so anything it alone remembered would be
    /// forgotten before the next frame drew. Zoom is not a parameter —
    /// nothing engine-facing changes — but it IS part of where you were
    /// working, which is worth coming back to.
    ///
    /// Only the sampler reads them today. They are named for what they
    /// are rather than for it, because the next display that needs to be
    /// looked into closely will want exactly this pair.
    #[serde(default = "unit_zoom")]
    pub view_zoom: f32,
    #[serde(default)]
    pub view_scroll: f32,
    /// Bypassed devices stay in the chain and leave the SCHEDULE, the same
    /// way a muted track does.
    #[serde(default)]
    pub bypass: bool,
}

/// A display showing all of itself. The serde default, so a project
/// written before zoom existed opens zoomed out rather than at 0x.
pub fn unit_zoom() -> f32 {
    1.0
}

impl DeviceInstance {
    pub fn kind(&self) -> DeviceKind {
        self.state.kind()
    }
}

/// A synth's engine values as the card's KNOB POSITIONS. The mapping is the
/// card's own in both directions, so what is drawn and what is stored
/// cannot drift; `engine_values_survive_the_trip_through_the_knobs` pins
/// the round trip.
pub fn synth_knobs(params: SynthParams) -> device::SineSynthUi {
    use daw::params::seq;
    let at = |param, value| device_norm(DeviceKind::SineSynth, param, value);
    device::SineSynthUi {
        gain: at(seq::GAIN, params.gain),
        attack: at(seq::ATTACK, params.attack_ms),
        release: at(seq::RELEASE, params.release_ms),
    }
}

/// A poly patch's engine values as the card's KNOB POSITIONS.
///
/// Built by walking the TABLE and writing through the card's own slot map,
/// rather than by naming thirty-four fields: the card already knows which
/// field each id belongs to, and a second copy of that map is a second
/// thing to get wrong.
pub fn loom_knobs(params: daw::audio::loom::LoomParams, page: u8) -> device::loom::LoomUi {
    let mut ui = device::loom::LoomUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::loom::TABLE, id).default)
    });
    ui.restore_view(page);
    ui
}

pub fn poly_knobs(params: daw::audio::poly::PolyParams, page: u8) -> device::PolyUi {
    let mut ui = device::PolyUi {
        page: usize::from(page),
        ..device::PolyUi::default()
    };
    for def in daw::params::poly::TABLE {
        let Some(value) = params.get(def.id) else {
            continue;
        };
        let norm = device_norm(DeviceKind::Poly, def.id, value);
        if let Some(slot) = ui.slot(def.id) {
            *slot = norm;
        }
    }
    ui
}

pub fn reverb_knobs(params: ReverbParams) -> device::ReverbUi {
    use daw::params::reverb;
    // The card reads through a closure rather than being handed the
    // engine's struct: a widget module must not know `crate::audio`, and
    // the app is the layer that knows both sides.
    device::ReverbUi::from_engine(|id| match id {
        reverb::PREDELAY => params.predelay_ms,
        reverb::SIZE => params.size,
        reverb::DECAY => params.decay,
        reverb::DAMP => params.damp,
        reverb::LOWCUT => params.low_cut,
        reverb::DIFFUSION => params.diffusion,
        reverb::MODULATION => params.modulation,
        reverb::WIDTH => params.width,
        _ => params.mix,
    })
}

pub fn phaser_knobs(params: PhaserParams) -> device::PhaserUi {
    use daw::params::phaser;
    let at = |param, value| device_norm(DeviceKind::Phaser, param, value);
    device::PhaserUi {
        amount: at(phaser::AMOUNT, params.amount),
        centre: at(phaser::CENTRE, params.centre),
        depth: at(phaser::DEPTH, params.depth),
        rate: at(phaser::RATE, params.rate),
        mix: at(phaser::MIX, params.mix),
    }
}

pub fn tilt_knobs(params: TiltParams) -> device::TiltUi {
    use daw::params::tilt;
    let at = |param, value| device_norm(DeviceKind::Tilt, param, value);
    device::TiltUi {
        tilt: at(tilt::TILT, params.tilt),
        pivot: at(tilt::PIVOT, params.pivot),
    }
}

pub fn disperser_knobs(params: DisperserParams) -> device::DisperserUi {
    use daw::params::disperser;
    let at = |param, value| device_norm(DeviceKind::Disperser, param, value);
    device::DisperserUi {
        amount: at(disperser::AMOUNT, params.amount),
        freq: at(disperser::FREQ, params.freq),
        pinch: at(disperser::PINCH, params.pinch),
    }
}

pub fn sheen_knobs(params: SheenParams) -> device::SheenUi {
    use daw::params::sheen;
    let at = |param, value| device_norm(DeviceKind::Sheen, param, value);
    device::SheenUi {
        amount: at(sheen::AMOUNT, params.amount),
        edge: at(sheen::EDGE, params.edge),
        mix: at(sheen::MIX, params.mix),
        out: at(sheen::OUT, params.out),
    }
}

pub fn lofi_knobs(params: LofiParams) -> device::LofiUi {
    use daw::params::lofi;
    let at = |param, value| device_norm(DeviceKind::Lofi, param, value);
    device::LofiUi {
        rate: at(lofi::RATE, params.rate),
        bits: at(lofi::BITS, params.bits),
        mix: at(lofi::MIX, params.mix),
        out: at(lofi::OUT, params.out),
    }
}

pub fn sat_knobs(params: SatParams) -> device::SatUi {
    use daw::params::sat;
    let at = |param, value| device_norm(DeviceKind::Sat, param, value);
    device::SatUi {
        mode: at(sat::MODE, params.mode),
        drive: at(sat::DRIVE, params.drive),
        bias: at(sat::BIAS, params.bias),
        mix: at(sat::MIX, params.mix),
        out: at(sat::OUT, params.out),
    }
}

pub fn utility_knobs(params: daw::audio::utility::UtilityParams) -> device::UtilityUi {
    let mut knobs = device::UtilityUi::default();
    for def in daw::params::utility::TABLE {
        if let Some(value) = params.get(def.id) {
            knobs.set_norm(def.id, device_norm(DeviceKind::Utility, def.id, value));
        }
    }
    knobs
}

pub fn acid_knobs(params: daw::audio::acid::AcidParams) -> device::AcidUi {
    let mut knobs = device::AcidUi::default();
    for def in daw::params::acid::TABLE {
        if let Some(value) = params.get(def.id)
            && let Some(slot) = knobs.slot_mut(def.id)
        {
            *slot = device_norm(DeviceKind::Acid, def.id, value);
        }
    }
    knobs
}

pub fn resyn_knobs(params: daw::audio::resyn::ResynParams, page: u8) -> device::ResynUi {
    let mut knobs = device::ResynUi::default();
    for def in daw::params::resyn::TABLE {
        if let Some(value) = params.get(def.id)
            && let Some(slot) = knobs.slot_mut(def.id)
        {
            *slot = device_norm(DeviceKind::Resyn, def.id, value);
        }
    }
    // The picked band has no engine counterpart, so it arrives from the
    // instance — `eq_knobs` takes the same road, for the same reason.
    knobs.selected = (page as usize).min(daw::params::resyn::BAND_COUNT - 1);
    knobs
}

pub fn strip_knobs(params: daw::audio::strip::StripParams) -> device::StripUi {
    let mut knobs = device::StripUi::default();
    for def in daw::params::strip::TABLE {
        if let Some(value) = params.get(def.id)
            && let Some(slot) = knobs.slot_mut(def.id)
        {
            *slot = device_norm(DeviceKind::Strip, def.id, value);
        }
    }
    knobs
}

pub fn gate_knobs(params: daw::audio::gate::GateParams) -> device::GateUi {
    let mut knobs = device::GateUi::default();
    for def in daw::params::gate::TABLE {
        if let Some(value) = params.get(def.id)
            && let Some(slot) = knobs.slot_mut(def.id)
        {
            *slot = device_norm(DeviceKind::Gate, def.id, value);
        }
    }
    knobs
}

pub fn glue_knobs(params: daw::audio::glue::GlueParams) -> device::GlueUi {
    let mut knobs = device::GlueUi::default();
    for def in daw::params::glue::TABLE {
        if let Some(value) = params.get(def.id) {
            knobs.set_norm(def.id, device_norm(DeviceKind::Glue, def.id, value));
        }
    }
    knobs
}

/// `page` is which band the card is showing — UI-only state that rides
/// `DeviceInstance`, since a card is rebuilt from engine units every
/// frame and would forget it otherwise.
pub fn prism_knobs(params: daw::audio::prism::PrismParams, page: u8) -> device::prism::PrismUi {
    device::prism::PrismUi::from_engine(usize::from(page), |id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::prism::TABLE, id).default)
    })
}

/// How many PAGE DOTS a kind's card draws in its title strip.
///
/// Only cards built on `card::tabbed_card*` have them; the equaliser and
/// the prism page with a cell in their footer instead, and the poly
/// synth has a named rail. Everything else is a single page.
///
/// This exists so the card's grip can keep clear of the dots — see
/// `card::Handle::keep_clear`.
pub fn card_pages(kind: DeviceKind) -> usize {
    match kind {
        DeviceKind::Sampler => device::sampler::pages(),
        DeviceKind::Scomp => device::scomp::pages(),
        DeviceKind::Stab => device::stab::pages(),
        DeviceKind::Quad => device::quad::pages(),
        DeviceKind::Brick => device::brick::pages(),
        _ => 1,
    }
}

pub fn tine_knobs(params: daw::audio::tine::TineParams) -> device::tine::TineUi {
    device::tine::TineUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::tine::TABLE, id).default)
    })
}

pub fn scomp_knobs(params: daw::scomp::ScompParams) -> device::scomp::ScompUi {
    device::scomp::ScompUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::scomp::TABLE, id).default)
    })
}

pub fn stab_knobs(params: daw::audio::stab::StabParams) -> device::stab::StabUi {
    device::stab::StabUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::stab::TABLE, id).default)
    })
}

pub fn quad_knobs(params: daw::audio::quad::QuadParams) -> device::quad::QuadUi {
    device::quad::QuadUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::quad::TABLE, id).default)
    })
}

pub fn brick_knobs(params: daw::audio::brick::BrickParams) -> device::brick::BrickUi {
    device::brick::BrickUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::brick::TABLE, id).default)
    })
}

pub fn tone_knobs(params: daw::audio::tone::ToneParams) -> device::tone::ToneUi {
    device::tone::ToneUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::tone::TABLE, id).default)
    })
}

pub fn sigil_knobs(params: daw::audio::sigil::SigilParams) -> device::sigil::SigilUi {
    device::sigil::SigilUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::sigil::TABLE, id).default)
    })
}

pub fn gauge_knobs(params: daw::audio::gauge::GaugeParams) -> device::gauge::GaugeUi {
    device::gauge::GaugeUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::gauge::TABLE, id).default)
    })
}

pub fn umbra_knobs(params: daw::audio::umbra::UmbraParams) -> device::umbra::UmbraUi {
    device::umbra::UmbraUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::umbra::TABLE, id).default)
    })
}

pub fn ferric_knobs(params: daw::audio::ferric::FerricParams) -> device::ferric::FerricUi {
    device::ferric::FerricUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::ferric::TABLE, id).default)
    })
}

pub fn sibyl_knobs(params: daw::audio::sibyl::SibylParams) -> device::sibyl::SibylUi {
    device::sibyl::SibylUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::sibyl::TABLE, id).default)
    })
}

pub fn flint_knobs(params: daw::audio::flint::FlintParams) -> device::flint::FlintUi {
    device::flint::FlintUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::flint::TABLE, id).default)
    })
}

pub fn clamp_knobs(params: daw::audio::clamp::ClampParams) -> device::clamp::ClampUi {
    device::clamp::ClampUi::from_engine(|id| {
        params
            .get(id)
            .unwrap_or_else(|| daw::params::def(daw::params::clamp::TABLE, id).default)
    })
}

pub fn eq_knobs(params: daw::audio::eq::EqParams, page: u8) -> device::EqUi {
    let mut knobs = device::EqUi {
        selected: usize::from(page).min(daw::params::eq::BANDS - 1),
        ..device::EqUi::default()
    };
    // Straight off the table: every row the engine has, put where the
    // card's own mapping says that value lives. One loop rather than
    // forty-one lines, and it cannot miss a row.
    for def in daw::params::eq::TABLE {
        if let Some(value) = params.get(def.id) {
            knobs.set_norm(def.id, device_norm(DeviceKind::Eq, def.id, value));
        }
    }
    knobs
}

pub fn echo_knobs(params: EchoParams) -> device::EchoUi {
    use daw::params::echo;
    let at = |param, value| device_norm(DeviceKind::Echo, param, value);
    device::EchoUi {
        sync: at(echo::SYNC, params.sync),
        time: at(echo::TIME, params.time_ms),
        feedback: at(echo::FEEDBACK, params.feedback),
        tone: at(echo::TONE, params.tone_hz),
        drive: at(echo::DRIVE, params.drive),
        wow: at(echo::WOW, params.wow),
        spread: at(echo::SPREAD, params.spread),
        mix: at(echo::MIX, params.mix),
        send: at(echo::SEND, params.send),
    }
}

/// The knob position of one engine value, by device kind.
pub fn device_norm(kind: DeviceKind, param: u32, value: f32) -> f32 {
    match kind {
        DeviceKind::Flint => device::flint::flint_norm(param, value),
        DeviceKind::Sibyl => device::sibyl::sibyl_norm(param, value),
        DeviceKind::Ferric => device::ferric::ferric_norm(param, value),
        DeviceKind::Umbra => device::umbra::umbra_norm(param, value),
        DeviceKind::Tone => device::tone::tone_norm(param, value),
        DeviceKind::Sigil => device::sigil::sigil_norm(param, value),
        DeviceKind::Gauge => device::gauge::gauge_norm(param, value),
        DeviceKind::SineSynth => device::sine_synth_norm(param, value),
        DeviceKind::Poly => device::poly_norm(param, value),
        DeviceKind::Loom => device::loom::loom_norm(param, value),
        DeviceKind::Tine => device::tine::tine_norm(param, value),
        DeviceKind::Scomp => device::scomp::scomp_norm(param, value),
        DeviceKind::Stab => device::stab::stab_norm(param, value),
        DeviceKind::Quad => device::quad::quad_norm(param, value),
        DeviceKind::Brick => device::brick::brick_norm(param, value),
        DeviceKind::Kit => device::kit::kit_norm(param, value),
        DeviceKind::Haze => device::haze::haze_norm(param, value),
        DeviceKind::Sampler => device::sampler_norm(param, value),
        DeviceKind::Kick => device::kick::kick_norm(param, value),
        DeviceKind::Snare => device::snare_norm(param, value),
        DeviceKind::Tom => device::tom_norm(param, value),
        DeviceKind::Hat => device::hat_norm(param, value),
        DeviceKind::Handclap => device::handclap_norm(param, value),
        DeviceKind::Drum => device::drum_norm(param, value),
        DeviceKind::Thump => device::thump_norm(param, value),
        DeviceKind::Clay => device::clay_norm(param, value),
        DeviceKind::Table => device::table_norm(param, value),
        DeviceKind::Ring => device::ring_norm(param, value),
        DeviceKind::PrismVoice => device::prism_voice_norm(param, value),
        DeviceKind::Mass => device::mass_norm(param, value),
        DeviceKind::Pluck => device::pluck_norm(param, value),
        DeviceKind::Vox => device::vox_norm(param, value),
        DeviceKind::Pipe => device::pipe_norm(param, value),
        DeviceKind::Glass => device::glass_norm(param, value),
        DeviceKind::Rom => device::rom_norm(param, value),
        DeviceKind::Limiter => device::limiter_norm(param, value),
        DeviceKind::Reverb => device::reverb_norm(param, value),
        DeviceKind::Sat => device::sat_norm(param, value),
        DeviceKind::Lofi => device::lofi_norm(param, value),
        DeviceKind::Sheen => device::sheen_norm(param, value),
        DeviceKind::Disperser => device::disperser_norm(param, value),
        DeviceKind::Tilt => device::tilt_norm(param, value),
        DeviceKind::Phaser => device::phaser_norm(param, value),
        DeviceKind::Echo => device::echo_norm(param, value),
        DeviceKind::Eq => device::eq_norm(param, value),
        DeviceKind::Filter => device::filter_norm(param, value),
        DeviceKind::Glue => device::glue_norm(param, value),
        DeviceKind::Clamp => device::clamp::clamp_norm(param, value),
        DeviceKind::Prism => device::prism::prism_norm(param, value),
        DeviceKind::Gate => device::gate_norm(param, value),
        DeviceKind::Strip => device::strip_norm(param, value),
        DeviceKind::Resyn => device::resyn_norm(param, value),
        DeviceKind::Acid => device::acid_norm(param, value),
        // A rack has no parameters; nothing ever asks, and this is what
        // it would be told if it did.
        DeviceKind::Rack => 0.0,
        // A console section never reaches this binary; it is the stage's.
        DeviceKind::Console(_) => 0.0,
        DeviceKind::Modulato => device::modulato::modulato_norm(param, value),
        DeviceKind::Utility => device::utility_norm(param, value),
    }
}

/// Whether a parameter snaps to segments rather than moving continuously.
///
/// The tables `seq` and `reverb` are continuous end to end; the poly
/// synth is the first device with choices in it, so it is the first that
/// can answer anything but `false`.
///
/// A card picks its own widget from the `Param` directly, so most of the
/// app never asks. What needs the answer is anything working from OUTSIDE
/// a card: the round-trip tests, and the parameter-lock editor — which
/// has to know whether a row steps whole choices or sweeps.
pub fn device_is_discrete(kind: DeviceKind, param: u32) -> bool {
    match kind {
        // Nothing on the transient shaper snaps: every row is a sweep.
        DeviceKind::Flint => false,
        DeviceKind::Sibyl => device::sibyl::sibyl_is_discrete(param),
        DeviceKind::Ferric => device::ferric::ferric_is_discrete(param),
        DeviceKind::Umbra => false,
        DeviceKind::Tone => device::tone::tone_is_discrete(param),
        DeviceKind::Sigil => device::sigil::sigil_is_discrete(param),
        DeviceKind::Gauge => device::gauge::gauge_is_discrete(param),
        DeviceKind::Poly => device::poly_is_discrete(param),
        DeviceKind::Loom => device::loom::loom_is_discrete(param),
        // Nothing on the resonator snaps: every row is a sweep.
        DeviceKind::Tine => false,
        DeviceKind::Scomp => device::scomp::scomp_is_discrete(param),
        DeviceKind::Stab => device::stab::stab_is_discrete(param),
        DeviceKind::Quad => device::quad::quad_is_discrete(param),
        DeviceKind::Brick => device::brick::brick_is_discrete(param),
        DeviceKind::Kit => device::kit::kit_is_discrete(param),
        // Nothing on the pad synth snaps: every row is a sweep.
        DeviceKind::Haze => false,
        DeviceKind::Sampler => device::sampler_is_discrete(param),
        DeviceKind::Kick => device::kick::kick_is_discrete(param),
        DeviceKind::Snare => device::snare_is_discrete(param),
        DeviceKind::Tom => device::tom_is_discrete(param),
        DeviceKind::Hat => device::hat_is_discrete(param),
        DeviceKind::Handclap => device::handclap_is_discrete(param),
        DeviceKind::Drum => device::drum_is_discrete(param),
        DeviceKind::Thump => device::thump_is_discrete(param),
        DeviceKind::Clay => device::clay_is_discrete(param),
        DeviceKind::Table => device::table_is_discrete(param),
        DeviceKind::Ring => device::ring_is_discrete(param),
        DeviceKind::PrismVoice => device::prism_voice_is_discrete(param),
        DeviceKind::Mass => device::mass_is_discrete(param),
        DeviceKind::Pluck => device::pluck_is_discrete(param),
        DeviceKind::Vox => device::vox_is_discrete(param),
        DeviceKind::Pipe => device::pipe_is_discrete(param),
        DeviceKind::Glass => device::glass_is_discrete(param),
        DeviceKind::Rom => device::rom_is_discrete(param),
        DeviceKind::Limiter => device::limiter_is_discrete(param),
        DeviceKind::Sat => device::sat_is_discrete(param),
        DeviceKind::Lofi => device::lofi_is_discrete(param),
        DeviceKind::Sheen => device::sheen_is_discrete(param),
        DeviceKind::Disperser => device::disperser_is_discrete(param),
        DeviceKind::Tilt => device::tilt_is_discrete(param),
        DeviceKind::Phaser => device::phaser_is_discrete(param),
        DeviceKind::Echo => device::echo_is_discrete(param),
        DeviceKind::Eq => device::eq_is_discrete(param),
        DeviceKind::Filter => device::filter_is_discrete(param),
        DeviceKind::Glue => device::glue_is_discrete(param),
        // Nothing on the surgical compressor snaps: the ratio is
        // continuous, which is the point of it beside glue's detents.
        DeviceKind::Clamp => false,
        DeviceKind::Prism => false,
        DeviceKind::Gate => device::gate_is_discrete(param),
        DeviceKind::Strip => device::strip_is_discrete(param),
        DeviceKind::Resyn => device::resyn_is_discrete(param),
        DeviceKind::Acid => device::acid_is_discrete(param),
        // A rack has no parameters; nothing ever asks, and this is what
        // it would be told if it did.
        DeviceKind::Rack => false,
        // A console section never reaches this binary; it is the stage's.
        DeviceKind::Console(_) => false,
        DeviceKind::Modulato => device::modulato::modulato_is_discrete(param),
        DeviceKind::Utility => device::utility_is_discrete(param),
        DeviceKind::SineSynth | DeviceKind::Reverb => false,
    }
}

/// Whether a parameter lives on a LOG scale, by device kind — asked of
/// the card's own mapping, so the modulation plan sweeps cutoffs and
/// times in OCTAVES exactly where the knob does.
pub fn device_is_log(kind: DeviceKind, param: u32) -> bool {
    match kind {
        // Nothing on the transient shaper is a ratio: both amounts are
        // already in dB and the split reads in milliseconds.
        DeviceKind::Flint => false,
        // Semitones and degrees are already logarithmic units.
        DeviceKind::Sibyl => false,
        DeviceKind::Ferric => false,
        DeviceKind::Umbra => false,
        // A test tone's frequency is heard in ratios, like every other.
        DeviceKind::Tone => param == daw::params::tone::FREQ,
        // The carrier is pitched: the seal is drawn in octaves.
        DeviceKind::Sigil => param == daw::params::sigil::FREQ,
        DeviceKind::Gauge => false,
        DeviceKind::Poly => device::poly_is_log(param),
        DeviceKind::Loom => device::loom::loom_is_log(param),
        DeviceKind::Tine => false,
        DeviceKind::Scomp => device::scomp::scomp_is_log(param),
        DeviceKind::Stab => device::stab::stab_is_log(param),
        DeviceKind::Quad => device::quad::quad_is_log(param),
        DeviceKind::Brick => device::brick::brick_is_log(param),
        DeviceKind::Kit => device::kit::kit_is_log(param),
        DeviceKind::Haze => device::haze::haze_is_log(param),
        DeviceKind::Sampler => device::sampler_is_log(param),
        DeviceKind::Kick => device::kick::kick_is_log(param),
        DeviceKind::Snare => device::snare_is_log(param),
        DeviceKind::Tom => device::tom_is_log(param),
        DeviceKind::Hat => device::hat_is_log(param),
        DeviceKind::Handclap => device::handclap_is_log(param),
        DeviceKind::Drum => device::drum_is_log(param),
        DeviceKind::Thump => device::thump_is_log(param),
        DeviceKind::Clay => device::clay_is_log(param),
        DeviceKind::Table => device::table_is_log(param),
        DeviceKind::Ring => device::ring_is_log(param),
        DeviceKind::PrismVoice => device::prism_voice_is_log(param),
        DeviceKind::Mass => device::mass_is_log(param),
        DeviceKind::Pluck => device::pluck_is_log(param),
        DeviceKind::Vox => device::vox_is_log(param),
        DeviceKind::Pipe => device::pipe_is_log(param),
        DeviceKind::Glass => device::glass_is_log(param),
        DeviceKind::Rom => device::rom_is_log(param),
        DeviceKind::Limiter => device::limiter_is_log(param),
        DeviceKind::SineSynth => device::sine_synth_is_log(param),
        DeviceKind::Sat => device::sat_is_log(param),
        DeviceKind::Lofi => device::lofi_is_log(param),
        DeviceKind::Sheen => device::sheen_is_log(param),
        DeviceKind::Disperser => device::disperser_is_log(param),
        DeviceKind::Tilt => device::tilt_is_log(param),
        DeviceKind::Phaser => device::phaser_is_log(param),
        DeviceKind::Echo => device::echo_is_log(param),
        DeviceKind::Eq => device::eq_is_log(param),
        DeviceKind::Filter => device::filter_is_log(param),
        DeviceKind::Glue => device::glue_is_log(param),
        DeviceKind::Clamp => device::clamp::clamp_is_log(param),
        DeviceKind::Prism => device::prism::prism_is_log(param),
        DeviceKind::Gate => device::gate_is_log(param),
        DeviceKind::Strip => device::strip_is_log(param),
        DeviceKind::Resyn => device::resyn_is_log(param),
        DeviceKind::Acid => device::acid_is_log(param),
        // A rack has no parameters; nothing ever asks, and this is what
        // it would be told if it did.
        DeviceKind::Rack => false,
        // A console section never reaches this binary; it is the stage's.
        DeviceKind::Console(_) => false,
        DeviceKind::Modulato => device::modulato::modulato_is_log(param),
        DeviceKind::Utility => device::utility_is_log(param),
        DeviceKind::Reverb => false,
    }
}

/// The engine value at a knob position — the inverse of [`device_norm`],
/// and the direction a card applies on the way out.
///
/// It was `#[cfg(test)]` until racks arrived, because a card emits engine
/// units itself and nothing in the app needed the other direction. A
/// MACRO does: it is a position pointed at somebody else's parameter, and
/// turning it means asking that device what its own units call this much.
pub fn device_value(kind: DeviceKind, param: u32, norm: f32) -> f32 {
    match kind {
        DeviceKind::Flint => device::flint::flint_value(param, norm),
        DeviceKind::Sibyl => device::sibyl::sibyl_value(param, norm),
        DeviceKind::Ferric => device::ferric::ferric_value(param, norm),
        DeviceKind::Umbra => device::umbra::umbra_value(param, norm),
        DeviceKind::Tone => device::tone::tone_value(param, norm),
        DeviceKind::Sigil => device::sigil::sigil_value(param, norm),
        DeviceKind::Gauge => device::gauge::gauge_value(param, norm),
        DeviceKind::SineSynth => device::sine_synth_value(param, norm),
        DeviceKind::Poly => device::poly_value(param, norm),
        DeviceKind::Loom => device::loom::loom_value(param, norm),
        DeviceKind::Tine => device::tine::tine_value(param, norm),
        DeviceKind::Scomp => device::scomp::scomp_value(param, norm),
        DeviceKind::Stab => device::stab::stab_value(param, norm),
        DeviceKind::Quad => device::quad::quad_value(param, norm),
        DeviceKind::Brick => device::brick::brick_value(param, norm),
        DeviceKind::Kit => device::kit::kit_value(param, norm),
        DeviceKind::Haze => device::haze::haze_value(param, norm),
        DeviceKind::Sampler => device::sampler_value(param, norm),
        DeviceKind::Kick => device::kick::kick_value(param, norm),
        DeviceKind::Snare => device::snare_value(param, norm),
        DeviceKind::Tom => device::tom_value(param, norm),
        DeviceKind::Hat => device::hat_value(param, norm),
        DeviceKind::Handclap => device::handclap_value(param, norm),
        DeviceKind::Drum => device::drum_value(param, norm),
        DeviceKind::Thump => device::thump_value(param, norm),
        DeviceKind::Clay => device::clay_value(param, norm),
        DeviceKind::Table => device::table_value(param, norm),
        DeviceKind::Ring => device::ring_value(param, norm),
        DeviceKind::PrismVoice => device::prism_voice_value(param, norm),
        DeviceKind::Mass => device::mass_value(param, norm),
        DeviceKind::Pluck => device::pluck_value(param, norm),
        DeviceKind::Vox => device::vox_value(param, norm),
        DeviceKind::Pipe => device::pipe_value(param, norm),
        DeviceKind::Glass => device::glass_value(param, norm),
        DeviceKind::Rom => device::rom_value(param, norm),
        DeviceKind::Limiter => device::limiter_value(param, norm),
        DeviceKind::Reverb => device::reverb_value(param, norm),
        DeviceKind::Sat => device::sat_value(param, norm),
        DeviceKind::Lofi => device::lofi_value(param, norm),
        DeviceKind::Sheen => device::sheen_value(param, norm),
        DeviceKind::Disperser => device::disperser_value(param, norm),
        DeviceKind::Tilt => device::tilt_value(param, norm),
        DeviceKind::Phaser => device::phaser_value(param, norm),
        DeviceKind::Echo => device::echo_value(param, norm),
        DeviceKind::Eq => device::eq_value(param, norm),
        DeviceKind::Filter => device::filter_value(param, norm),
        DeviceKind::Glue => device::glue_value(param, norm),
        DeviceKind::Clamp => device::clamp::clamp_value(param, norm),
        DeviceKind::Prism => device::prism::prism_value(param, norm),
        DeviceKind::Gate => device::gate_value(param, norm),
        DeviceKind::Strip => device::strip_value(param, norm),
        DeviceKind::Resyn => device::resyn_value(param, norm),
        DeviceKind::Acid => device::acid_value(param, norm),
        // A rack has no parameters; nothing ever asks, and this is what
        // it would be told if it did.
        DeviceKind::Rack => 0.0,
        // A console section never reaches this binary; it is the stage's.
        DeviceKind::Console(_) => 0.0,
        DeviceKind::Modulato => device::modulato::modulato_value(param, norm),
        DeviceKind::Utility => device::utility_value(param, norm),
    }
}

/// Every parameter of a device state as an edit — what a reset, a preset
/// recall or a freshly loaded device sends, since a card only emits what
/// the user just moved.
pub fn device_edits(state: DeviceState) -> Vec<device::ParamEdit> {
    state
        .kind()
        .spec()
        .params
        .iter()
        .filter_map(|def| {
            state.value(def.id).map(|value| device::ParamEdit {
                param: def.id,
                value,
            })
        })
        .collect()
}
