//! The Song, made runnable: the document goes in, a `GraphSpec` comes out.
//!
//! One direction only, and nothing here reads back — the same rule the
//! `daw` binary's own compiler keeps. What is different is the INPUT: that
//! one compiles the legacy arrangement, this one compiles
//! [`crate::sequencing::Song`] itself, which is why it can live in the
//! library where any frame can reach it.
//!
//! # What sounds, and what decides it
//!
//! A scene is "a place to keep clips before it is a thing to fire" — the
//! document does not record which one is playing, because which one is
//! playing is a fact about a PERFORMANCE and not about a song. So the
//! launched scene arrives here as an argument. A frame owns that choice;
//! the document never learns it.
//!
//! # What it does not do yet, stated rather than implied
//!
//! No device chains (nothing can add one), no sends or returns (the same),
//! no group summing (a group lane carries no clip, so it contributes no
//! sound of its own — muting THROUGH a group still works, because
//! [`Song::audible`] is what decides who reaches the graph at all), and no
//! p-locks or trig conditions: those are engine features the Song's own
//! `Trig` has no field for, and inventing one here would put a second
//! answer beside the model.
//!
//! Every one of those is an addition to this file, not a rewrite of it.

use crate::audio::graph::{GraphSpec, MAX_METERS, NodeId, NodeSpec, Note as GraphNote};
use crate::devices::DeviceKind;
use crate::pitch::nearest_midi;
use crate::sequencing::{
    Clip, Device, DeviceId, PATTERN_STEP_TICKS, PATTERN_STEPS, Pattern, Song, TICKS_PER_BEAT,
};

/// The master's meter slot: the LAST one, so the tracks can take theirs in
/// their own order from zero without either end having to know how many
/// the other used. The same convention the `daw` binary's compiler keeps.
pub const MASTER_METER: usize = MAX_METERS - 1;

/// The addressable nodes a build hands back, indexed BY TRACK.
///
/// This is what lets a fader send a letter instead of forcing a recompile:
/// every swap mints fresh ids, so the mapping is captured WITH the schedule
/// rather than derived from the song afterwards. A track that did not reach
/// the graph — silent, or with nothing to play — holds `None`, and a letter
/// addressed to it correctly has nowhere to go.
#[derive(Debug, Clone, PartialEq)]
pub struct SongNodes {
    /// Each track's output stage: the pan node that carries BOTH its fader
    /// and its pan, and the node its meter is tapped from.
    pub outputs: Vec<Option<NodeId>>,
    /// Each track's meter slot in [`crate::audio::BlockSnapshot::track_peaks`].
    pub meters: Vec<Option<usize>>,
    /// The master's output stage, always present — a song with nothing in
    /// it still has somewhere for the meter and the master gain to live.
    pub master: NodeId,
}

/// Compile `song` into a graph, playing whatever `playing` says.
///
/// `playing[track]` is the SCENE whose clip that track is sounding, and
/// `None` means the track is playing nothing. Per track rather than one
/// scene for the whole song, because that is what a session is: firing a
/// row is firing every clip in it, and a performer who could only ever
/// fire whole rows would be using less than the model already holds.
///
/// Nothing playing anywhere is a legitimate state and not an empty case:
/// it is the transport rolling with nothing fired, which must produce a
/// graph that runs and is silent rather than no graph at all.
pub fn build(song: &Song, playing: &[Option<usize>]) -> (GraphSpec, SongNodes) {
    let mut spec = GraphSpec::default();
    // The master exists before anything can feed it: a song with nothing
    // in it still needs somewhere for the master gain and its meter to
    // live, and a graph with no output is not a runnable graph.
    let master = spec.push(NodeSpec::Mixer { gain: song.master });
    let mut nodes = SongNodes {
        outputs: vec![None; song.tracks.len()],
        meters: vec![None; song.tracks.len()],
        master,
    };

    for (index, track) in song.tracks.iter().enumerate() {
        if !song.audible(index) {
            continue;
        }
        let Some(pattern) = playing
            .get(index)
            .copied()
            .flatten()
            .and_then(|scene| song.session.scenes.get(scene))
            .and_then(|scene| scene.clip(track.id))
            .and_then(|clip| match clip {
                Clip::Pattern(id) => song.patterns.iter().find(|pattern| pattern.id == id),
            })
        else {
            continue;
        };

        let notes = notes_of(song, pattern, &[]);
        if notes.is_empty() {
            // A voice with nothing to play is not silence worth paying a
            // node for — and a meter that never moves says the same thing
            // as no meter, which is what an absent slot already means.
            continue;
        }

        let Some(instrument) = voice_of(track, notes, Some(beats(pattern.length_ticks))) else {
            continue;
        };
        let voice = spec.push(instrument);
        // The effects, in signal order, each fed by the one before it. A
        // BYPASSED effect is simply not built: the signal passes it by,
        // which is what bypass means, and costs nothing to run.
        let mut effects: Vec<(DeviceId, NodeId)> = Vec::new();
        let mut tail = voice;
        for device in &track.chain {
            if device.is_instrument() || device.bypassed {
                continue;
            }
            let Some(node) = effect_of(device) else {
                continue;
            };
            let node = spec.push(node);
            spec.connect(tail, node);
            effects.push((device.id, node));
            tail = node;
        }
        // Now the effects have ids, the notes can name them: the voice's
        // notes are cut again with every effect lock addressed, and every
        // locked effect parameter registers the knob it returns to.
        if !effects.is_empty() {
            if let Some(notes) = spec.node_mut(voice).and_then(NodeSpec::notes_mut) {
                *notes = notes_of(song, pattern, &effects);
            }
            for step in 0..PATTERN_STEPS {
                for lock in &pattern.trig(step).locks {
                    let Some(id) = lock.device else {
                        continue;
                    };
                    if let Some((_, node)) = effects.iter().find(|(device, _)| *device == id)
                        && let Some(device) = track.chain.iter().find(|device| device.id == id)
                    {
                        spec.lock_base(*node, lock.param, device.value(lock.param));
                    }
                }
            }
        }
        // The fader and the pan are ONE node, which is why the meter tapped
        // from it reads post-fader and post-pan — what a mixer meter is
        // expected to show.
        let out = spec.push(NodeSpec::Pan {
            pan: track.pan,
            gain: track.volume,
        });
        spec.connect(tail, out);
        spec.connect(out, master);
        nodes.outputs[index] = Some(out);

        if index < MASTER_METER {
            spec.meter(index, out);
            nodes.meters[index] = Some(index);
        }
    }

    spec.meter(MASTER_METER, master);
    spec.set_output(master);
    (spec, nodes)
}

/// The instrument a track sounds, ready to push.
///
/// The head of its chain when there is one, and the default voice when
/// there is not — which is what every project written before devices
/// reached the Song carries, and what it has always sounded.
///
/// `None` means this track produces nothing: its instrument is bypassed,
/// or it is a kind this compiler cannot build yet. Silence is the honest
/// answer to both. Quietly substituting a different instrument would be a
/// track sounding something nobody chose.
///
/// The sampler is the one instrument whose patch is not only numbers: it
/// plays the file the device carries in `Device::sample`, and a sampler
/// with no file yet compiles to a SILENT sampler rather than a refused
/// graph — the engine's own rule, because a missing sample must not mute
/// a project.
///
/// **Not built yet:** `SineSynth`. An addition here.
fn voice_of(
    track: &crate::sequencing::Track,
    notes: Vec<GraphNote>,
    loop_len_beats: Option<f64>,
) -> Option<NodeSpec> {
    // Every instrument node has the same shape — the pattern's notes and
    // its own typed patch — so the arms differ only in which two names
    // they name. Written as one shape so a new instrument is one line and
    // cannot quietly acquire a different contract.
    macro_rules! voice {
        ($variant:ident, $params:path, $device:expr) => {{
            let mut params = <$params>::default();
            for (id, value) in &$device.overrides {
                params.set(*id, *value);
            }
            NodeSpec::$variant {
                notes,
                subloops: Vec::new(),
                loop_len_beats,
                params,
            }
        }};
    }

    let Some(head) = track.chain.first().filter(|device| device.is_instrument()) else {
        // No instrument on the chain: the default voice, at its defaults.
        return Some(NodeSpec::Poly {
            notes,
            subloops: Vec::new(),
            loop_len_beats,
            params: Default::default(),
        });
    };
    if head.bypassed {
        return None;
    }
    Some(match head.kind {
        DeviceKind::Poly => voice!(Poly, crate::audio::poly::PolyParams, head),
        DeviceKind::Haze => voice!(Haze, crate::audio::haze::HazeParams, head),
        DeviceKind::Loom => voice!(Loom, crate::audio::loom::LoomParams, head),
        DeviceKind::Tine => voice!(Tine, crate::audio::tine::TineParams, head),
        DeviceKind::Acid => voice!(Acid, crate::audio::acid::AcidParams, head),
        DeviceKind::Kick => voice!(Kick, crate::audio::kick::KickParams, head),
        DeviceKind::Snare => voice!(Snare, crate::audio::snare::SnareParams, head),
        DeviceKind::Hat => voice!(Hat, crate::audio::hat::HatParams, head),
        DeviceKind::Tom => voice!(Tom, crate::audio::tom::TomParams, head),
        DeviceKind::Handclap => voice!(Handclap, crate::audio::handclap::HandclapParams, head),
        DeviceKind::Sampler => {
            let mut params = crate::audio::sampler::SamplerParams::default();
            for (id, value) in &head.overrides {
                params.set(*id, *value);
            }
            NodeSpec::Sampler {
                notes,
                subloops: Vec::new(),
                loop_len_beats,
                path: head.sample.clone().unwrap_or_default(),
                params,
                // The device's authored slices, as fractions; an empty
                // table leaves the compiler to lay the grid the SLICES
                // knob asks for.
                slices: head.slices.clone(),
            }
        }
        _ => return None,
    })
}

/// One effect, ready to push, or `None` for a kind this compiler cannot
/// build.
///
/// Two shapes, and they are the engine's shapes rather than a choice made
/// here: most effects take their whole patch as one typed struct with an
/// id-addressed setter — the same shape every instrument has — and eight
/// take their settings as named fields. The field order of those eight is
/// their parameter table's order, which is checked by the fact that this
/// file names each id explicitly rather than counting positions.
///
/// `Rack` is the one absent kind: it is a container for other devices,
/// not a node, and nesting is not a thing the Song's chain models yet.
fn effect_of(device: &Device) -> Option<NodeSpec> {
    // Every struct-shaped effect is built the same way: the kind's own
    // defaults, then the device's edits applied by id. An id the table
    // does not know is dropped by the setter, exactly as a stale letter is.
    macro_rules! patch {
        ($variant:ident, $params:path) => {{
            let mut params = <$params>::default();
            for (id, edit) in &device.overrides {
                params.set(*id, *edit);
            }
            NodeSpec::$variant { params }
        }};
    }
    let value = |id: u32| device.value(id);
    // The one place an index stops being a float: the spec wants a
    // choice, and rounding is that single arithmetic step.
    let choice = |id: u32| device.value(id).round().max(0.0) as u32;

    Some(match device.kind {
        DeviceKind::Utility => patch!(Utility, crate::audio::utility::UtilityParams),
        DeviceKind::Modulato => patch!(Modulato, crate::audio::modulato::ModulatoParams),
        DeviceKind::Filter => patch!(Filter, crate::audio::filter::FilterParams),
        DeviceKind::Limiter => patch!(Limiter, crate::audio::limiter::LimiterParams),
        DeviceKind::Clamp => patch!(Clamp, crate::audio::clamp::ClampParams),
        DeviceKind::Prism => patch!(Prism, crate::audio::prism::PrismParams),
        DeviceKind::Glue => patch!(Glue, crate::audio::glue::GlueParams),
        DeviceKind::Eq => patch!(Eq, crate::audio::eq::EqParams),
        DeviceKind::Flint => patch!(Flint, crate::audio::flint::FlintParams),
        DeviceKind::Sibyl => patch!(Sibyl, crate::audio::sibyl::SibylParams),
        DeviceKind::Ferric => patch!(Ferric, crate::audio::ferric::FerricParams),
        DeviceKind::Umbra => patch!(Umbra, crate::audio::umbra::UmbraParams),
        DeviceKind::Tone => patch!(Tone, crate::audio::tone::ToneParams),
        DeviceKind::Sigil => patch!(Sigil, crate::audio::sigil::SigilParams),
        DeviceKind::Gauge => patch!(Gauge, crate::audio::gauge::GaugeParams),
        DeviceKind::Gate => patch!(Gate, crate::audio::gate::GateParams),
        DeviceKind::Strip => patch!(Strip, crate::audio::strip::StripParams),
        DeviceKind::Resyn => patch!(Resyn, crate::audio::resyn::ResynParams),
        DeviceKind::Reverb => NodeSpec::Reverb {
            predelay_ms: value(crate::params::reverb::PREDELAY),
            size: value(crate::params::reverb::SIZE),
            decay: value(crate::params::reverb::DECAY),
            damp: value(crate::params::reverb::DAMP),
            low_cut: value(crate::params::reverb::LOWCUT),
            diffusion: value(crate::params::reverb::DIFFUSION),
            modulation: value(crate::params::reverb::MODULATION),
            width: value(crate::params::reverb::WIDTH),
            mix: value(crate::params::reverb::MIX),
        },
        DeviceKind::Echo => NodeSpec::Echo {
            sync: choice(crate::params::echo::SYNC),
            time_ms: value(crate::params::echo::TIME),
            feedback: value(crate::params::echo::FEEDBACK),
            tone_hz: value(crate::params::echo::TONE),
            drive: value(crate::params::echo::DRIVE),
            wow: value(crate::params::echo::WOW),
            spread: value(crate::params::echo::SPREAD),
            mix: value(crate::params::echo::MIX),
            send: value(crate::params::echo::SEND),
        },
        DeviceKind::Sat => NodeSpec::Sat {
            mode: choice(crate::params::sat::MODE),
            drive: value(crate::params::sat::DRIVE),
            bias: value(crate::params::sat::BIAS),
            mix: value(crate::params::sat::MIX),
            out: value(crate::params::sat::OUT),
        },
        DeviceKind::Lofi => NodeSpec::Lofi {
            rate: value(crate::params::lofi::RATE),
            bits: value(crate::params::lofi::BITS),
            mix: value(crate::params::lofi::MIX),
            out: value(crate::params::lofi::OUT),
        },
        DeviceKind::Sheen => NodeSpec::Sheen {
            amount: value(crate::params::sheen::AMOUNT),
            edge_hz: value(crate::params::sheen::EDGE),
            mix: value(crate::params::sheen::MIX),
            out: value(crate::params::sheen::OUT),
        },
        DeviceKind::Disperser => NodeSpec::Disperser {
            amount: value(crate::params::disperser::AMOUNT),
            freq_hz: value(crate::params::disperser::FREQ),
            pinch: value(crate::params::disperser::PINCH),
        },
        DeviceKind::Tilt => NodeSpec::Tilt {
            tilt_db: value(crate::params::tilt::TILT),
            pivot_hz: value(crate::params::tilt::PIVOT),
        },
        DeviceKind::Phaser => NodeSpec::Phaser {
            amount: value(crate::params::phaser::AMOUNT),
            centre_hz: value(crate::params::phaser::CENTRE),
            depth_oct: value(crate::params::phaser::DEPTH),
            rate_hz: value(crate::params::phaser::RATE),
            mix: value(crate::params::phaser::MIX),
        },
        _ => return None,
    })
}

/// Ticks in the Song's own resolution, as beats — the unit the graph
/// speaks. Tempo is applied when the spec is compiled, never here.
fn beats(ticks: usize) -> f64 {
    ticks as f64 / TICKS_PER_BEAT as f64
}

/// One pattern's trigs as graph notes, in the key the song is in.
///
/// A pitch in this model is an ANCHOR — a degree of the song's key, or an
/// absolute frequency — so resolving it needs the key and cannot be done
/// by the caller. The engine speaks MIDI numbers, so the resolution ends
/// at the nearest one.
fn notes_of(song: &Song, pattern: &Pattern, effects: &[(DeviceId, NodeId)]) -> Vec<GraphNote> {
    let mut notes = Vec::new();
    for step in 0..PATTERN_STEPS {
        let trig = pattern.trig(step);
        if !trig.enabled {
            continue;
        }
        let at = step * PATTERN_STEP_TICKS;
        for note in &trig.notes {
            if note.muted {
                continue;
            }
            // Micro-timing is a displacement in ticks and may point either
            // way; the first step's early note lands at the top of the
            // pattern rather than before it.
            let start = at.saturating_add_signed(note.micro_ticks as isize);
            notes.push(GraphNote {
                start_beats: beats(start),
                len_beats: beats(note.length_ticks),
                pitch: nearest_midi(note.pitch.resolve(&song.key)),
                vel: note.velocity,
                // The trig's locks ride every note of the chord: the
                // graph locks per note, the trig locks per firing, and a
                // chord is one firing.
                // The voice's locks, and the effects' locks each addressed to
                // the node its device became. A lock on a device that is not
                // on the chain now — bypassed, or gone — is left out, not
                // misdelivered.
                plocks: trig
                    .locks
                    .iter()
                    .filter(|lock| lock.device.is_none())
                    .map(|lock| (lock.param, lock.value))
                    .collect(),
                fx_locks: trig
                    .locks
                    .iter()
                    .filter_map(|lock| {
                        let device = lock.device?;
                        let (_, node) = effects.iter().find(|(id, _)| *id == device)?;
                        Some((node.to_bits(), lock.param, lock.value))
                    })
                    .collect(),
                prob: trig.probability,
                cond: None,
            });
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::{Note, Scene, Slot, TrackKind};

    /// A song with one instrument track holding one pattern, and a scene
    /// that fires it.
    /// Every track playing scene zero — the table the harness wants
    /// whenever it just needs the clips to sound.
    fn playing(song: &Song) -> Vec<Option<usize>> {
        vec![Some(0); song.tracks.len()]
    }

    /// A song whose first scene holds a clip with one note in it. The
    /// clip lives IN the session, because that is where the compiler
    /// looks — a scene handed in from the side would be a scene no
    /// performer could have fired.
    fn song_with_a_clip() -> Song {
        let mut song = Song::default();
        let pattern = song.patterns[0].id;
        song.patterns[0].toggle(0, Note::new(60, PATTERN_STEP_TICKS, 100));
        song.session.scenes[0] = Scene {
            slots: vec![Slot {
                track: song.tracks[0].id,
                clip: Clip::Pattern(pattern),
            }],
        };
        song
    }

    /// A sampler's authored slices ride into its node spec as the
    /// fractions the device holds.
    #[test]
    fn a_samplers_slices_ride_into_its_spec() {
        let mut song = song_with_a_clip();
        let id = song
            .add_device(0, DeviceKind::Sampler)
            .expect("a sampler on the track");
        song.device_mut(id)
            .expect("the device")
            .set_slices([0.5, 0.25, 0.0]);
        let (spec, _) = build(&song, &playing(&song));
        let slices = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Sampler { slices, .. } => Some(slices.clone()),
                _ => None,
            })
            .expect("a sampler node");
        assert_eq!(slices, vec![0.0, 0.25, 0.5]);
    }

    /// A lock on an effect rides into the voice's notes addressed to the
    /// effect's node, and the effect's knob is registered so the lock
    /// can be restored.
    #[test]
    fn an_effect_lock_names_its_node_and_registers_its_knob() {
        use crate::params::sat;
        let mut song = song_with_a_clip();
        let sat_id = song
            .add_device(0, DeviceKind::Sat)
            .expect("an effect on the track");
        song.device_mut(sat_id)
            .expect("device")
            .set(sat::DRIVE, 0.3);
        let knob = song.device(sat_id).expect("device").value(sat::DRIVE);
        song.patterns[0]
            .trig_mut(0)
            .set_lock_on(Some(sat_id), sat::DRIVE, 0.9);
        let (spec, _) = build(&song, &playing(&song));
        let sat_node = spec
            .iter_ordered()
            .find_map(|(id, node)| matches!(node, NodeSpec::Sat { .. }).then_some(id))
            .expect("the effect is in the graph");
        let notes = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Poly { notes, .. } => Some(notes.clone()),
                _ => None,
            })
            .expect("the voice is in the graph");
        assert_eq!(
            notes[0].fx_locks,
            vec![(sat_node.to_bits(), sat::DRIVE, 0.9)]
        );
        assert!(
            notes[0].plocks.is_empty(),
            "an effect lock leaked onto the voice"
        );
        let bases = spec.lock_bases();
        assert!(
            bases.iter().any(|(node, param, base)| *node == sat_node
                && *param == sat::DRIVE
                && (*base - knob).abs() < 1e-6),
            "the knob was not registered: {bases:?}"
        );
    }

    /// A trig's locks ride into the graph verbatim, on every note of
    /// the chord, as the (param, value) pairs the engine applies.
    #[test]
    fn a_trigs_locks_ride_its_notes_into_the_graph() {
        let mut song = song_with_a_clip();
        song.patterns[0].trig_mut(0).set_lock(3, 0.25);
        song.patterns[0].trig_mut(0).set_lock(7, 0.9);
        let pattern = song.patterns[0].clone();
        let notes = notes_of(&song, &pattern, &[]);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].plocks, vec![(3, 0.25), (7, 0.9)]);
    }

    /// Put `track`'s clip for scene zero into the session.
    fn also_playing(song: &mut Song, track: usize) {
        let (id, pattern) = (song.tracks[track].id, song.patterns[0].id);
        song.session.scenes[0].slots.push(Slot {
            track: id,
            clip: Clip::Pattern(pattern),
        });
    }

    fn voices(spec: &GraphSpec) -> usize {
        spec.iter_ordered()
            .filter(|(_, node)| matches!(node, NodeSpec::Poly { .. }))
            .count()
    }

    #[test]
    fn a_song_with_nothing_launched_still_runs_and_is_silent() {
        let song = song_with_a_clip();
        let (spec, nodes) = build(&song, &[]);
        assert_eq!(voices(&spec), 0, "something sounded with nothing launched");
        assert!(nodes.outputs.iter().all(Option::is_none));
        spec.compile(48_000, 128)
            .expect("a silent graph must still be runnable");
    }

    #[test]
    fn a_launched_scene_gives_its_track_a_voice_that_compiles() {
        let song = song_with_a_clip();
        let (spec, nodes) = build(&song, &playing(&song));
        assert_eq!(voices(&spec), 1);
        assert!(nodes.outputs[0].is_some(), "the track has no output stage");
        assert_eq!(nodes.meters[0], Some(0));
        spec.compile(48_000, 128).expect("the graph must run");
    }

    #[test]
    fn a_muted_track_never_reaches_the_graph() {
        let mut song = song_with_a_clip();
        song.tracks[0].muted = true;
        let (spec, nodes) = build(&song, &playing(&song));
        assert_eq!(voices(&spec), 0);
        assert!(nodes.outputs[0].is_none());
        assert!(nodes.meters[0].is_none(), "a silent track kept a meter");
    }

    #[test]
    fn solo_elsewhere_keeps_a_track_out_of_the_graph() {
        let mut song = song_with_a_clip();
        song.add_track(TrackKind::Instrument);
        also_playing(&mut song, 1);
        song.tracks[1].solo = true;

        let (spec, nodes) = build(&song, &playing(&song));
        assert_eq!(voices(&spec), 1, "the unsoloed track was still heard");
        assert!(nodes.outputs[0].is_none());
        assert!(nodes.outputs[1].is_some());
    }

    #[test]
    fn a_track_playing_a_scene_it_has_no_clip_in_is_silent() {
        let mut song = song_with_a_clip();
        // Point the track at a scene that holds nothing for it.
        song.session.scenes[1] = Scene { slots: Vec::new() };
        let (spec, nodes) = build(&song, &[Some(1)]);
        assert_eq!(voices(&spec), 0);
        assert!(nodes.outputs[0].is_none());
    }

    #[test]
    fn the_fader_and_the_pan_ride_one_output_stage() {
        let mut song = song_with_a_clip();
        song.tracks[0].volume = 0.25;
        song.tracks[0].pan = -0.5;
        let (spec, nodes) = build(&song, &playing(&song));
        let out = nodes.outputs[0].expect("a sounding track has an output stage");
        let (_, node) = spec
            .iter_ordered()
            .find(|(id, _)| *id == out)
            .expect("the output stage is in the graph");
        match node {
            NodeSpec::Pan { pan, gain } => {
                assert_eq!(*gain, 0.25, "the fader did not reach the graph");
                assert_eq!(*pan, -0.5, "the pan did not reach the graph");
            }
            other => panic!("the output stage is a {other:?}, not a pan"),
        }
    }

    #[test]
    fn the_master_keeps_the_last_meter_slot_for_itself() {
        let song = song_with_a_clip();
        let (_, nodes) = build(&song, &playing(&song));
        assert_eq!(nodes.meters[0], Some(0), "the track took the master's slot");
        assert_ne!(nodes.meters[0], Some(MASTER_METER));
    }

    /// The one that matters: a song, a launched scene, and actual sound
    /// out of the schedule the engine would be handed.
    ///
    /// Every other test here checks a claim about the graph's SHAPE. This
    /// one runs it, because a graph that compiles and is silent would
    /// pass all of them — and silence with no explanation is the failure
    /// this project spends the most effort avoiding.
    #[test]
    fn a_launched_scene_actually_makes_sound() {
        use crate::audio::graph::ProcessCtx;

        let song = song_with_a_clip();
        let (spec, _) = build(&song, &playing(&song));
        let mut schedule = spec.compile(48_000, 256).expect("the graph must run");

        // Two seconds of blocks at 120bpm, rolling from the top. The
        // first block carries the discontinuity, as a first play does.
        let beats_per_sample = 120.0 / 60.0 / 48_000.0;
        let silence = [0.0f32; 512];
        let mut loudest = 0.0f32;
        for block in 0..64 {
            let mut out = vec![0.0f32; 512];
            let ctx = ProcessCtx {
                device_input: &silence,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: (block * 256) as u64,
                beat: (block * 256) as f64 * beats_per_sample,
                beats_per_sample,
                discontinuity: block == 0,
            };
            schedule.run(&mut out, &ctx);
            for sample in &out {
                loudest = loudest.max(sample.abs());
            }
        }
        assert!(
            loudest > 0.01,
            "the graph compiled and stayed silent (loudest {loudest})"
        );
        assert!(
            loudest <= 1.5,
            "the graph is not an amplitude any more ({loudest})"
        );
    }

    /// And the other half of the same claim: a track the mixer says is
    /// silent IS silent, all the way through the engine rather than only
    /// in the shape of the graph.
    #[test]
    fn a_muted_track_makes_no_sound_at_all() {
        use crate::audio::graph::ProcessCtx;

        let mut song = song_with_a_clip();
        song.tracks[0].muted = true;
        let (spec, _) = build(&song, &playing(&song));
        let mut schedule = spec.compile(48_000, 256).expect("the graph must run");

        let beats_per_sample = 120.0 / 60.0 / 48_000.0;
        let silence = [0.0f32; 512];
        let mut loudest = 0.0f32;
        for block in 0..64 {
            let mut out = vec![0.0f32; 512];
            let ctx = ProcessCtx {
                device_input: &silence,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: (block * 256) as u64,
                beat: (block * 256) as f64 * beats_per_sample,
                beats_per_sample,
                discontinuity: block == 0,
            };
            schedule.run(&mut out, &ctx);
            for sample in &out {
                loudest = loudest.max(sample.abs());
            }
        }
        assert_eq!(loudest, 0.0, "a muted track was heard");
    }

    #[test]
    fn the_chains_head_is_the_voice_the_track_sounds() {
        let mut song = song_with_a_clip();
        // The default: no chain, the default voice.
        let (spec, _) = build(&song, &playing(&song));
        assert!(
            spec.iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Poly { .. })),
            "an empty chain did not sound the default voice"
        );

        // Choose another instrument and the track sounds THAT.
        song.add_device(0, DeviceKind::Haze).expect("an instrument");
        let (spec, _) = build(&song, &playing(&song));
        assert!(
            spec.iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Haze { .. })),
            "the chosen instrument did not reach the graph"
        );
        assert!(
            !spec
                .iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Poly { .. })),
            "the default voice sounded alongside the chosen one"
        );
    }

    #[test]
    fn a_devices_settings_reach_the_voice() {
        let mut song = song_with_a_clip();
        let id = song.add_device(0, DeviceKind::Poly).expect("an instrument");
        let gain = crate::params::poly::GAIN;
        let quiet = {
            let device = song.device_mut(id).expect("there");
            assert!(device.set(gain, 0.2));
            device.value(gain)
        };

        let (spec, _) = build(&song, &playing(&song));
        let params = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Poly { params, .. } => Some(*params),
                _ => None,
            })
            .expect("the voice is in the graph");
        assert_eq!(
            params.gain, quiet,
            "an edited parameter did not reach the engine's patch"
        );
    }

    #[test]
    fn a_sampler_plays_the_file_its_device_carries() {
        let mut song = song_with_a_clip();
        let id = song
            .add_device(0, DeviceKind::Sampler)
            .expect("an instrument");

        // No file yet: a silent sampler, not a refused graph and not the
        // default voice standing in.
        let (spec, nodes) = build(&song, &playing(&song));
        let path = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Sampler { path, .. } => Some(path.clone()),
                _ => None,
            })
            .expect("the sampler is in the graph");
        assert!(path.as_os_str().is_empty());
        assert!(
            nodes.outputs[0].is_some(),
            "a sampler with no file lost its output"
        );

        let device = song.device_mut(id).expect("there");
        device.sample = Some("/kits/break.wav".into());
        assert!(device.set(crate::params::sampler::START, 0.25));
        let (spec, _) = build(&song, &playing(&song));
        let (path, params) = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Sampler { path, params, .. } => Some((path.clone(), *params)),
                _ => None,
            })
            .expect("the sampler is in the graph");
        assert_eq!(path, std::path::PathBuf::from("/kits/break.wav"));
        assert_eq!(
            params.start, 0.25,
            "an edit did not reach the sampler's patch"
        );
    }

    #[test]
    fn a_bypassed_instrument_is_silence_and_not_a_substitute() {
        let mut song = song_with_a_clip();
        let id = song.add_device(0, DeviceKind::Haze).expect("an instrument");
        song.device_mut(id).expect("there").bypassed = true;

        let (spec, nodes) = build(&song, &playing(&song));
        assert!(
            nodes.outputs[0].is_none(),
            "a bypassed voice still had an output"
        );
        assert_eq!(voices(&spec), 0);
        assert!(
            !spec
                .iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Poly { .. })),
            "bypassing an instrument quietly substituted the default one"
        );
        spec.compile(48_000, 128)
            .expect("a silent graph must still run");
    }

    /// Render one graph and report the loudest sample it produced.
    fn loudest(song: &Song) -> f32 {
        use crate::audio::graph::ProcessCtx;
        let (spec, _) = build(song, &playing(song));
        let mut schedule = spec.compile(48_000, 256).expect("the graph must run");
        let beats_per_sample = 120.0 / 60.0 / 48_000.0;
        let silence = [0.0f32; 512];
        let mut peak = 0.0f32;
        for block in 0..64 {
            let mut out = vec![0.0f32; 512];
            schedule.run(
                &mut out,
                &ProcessCtx {
                    device_input: &silence,
                    in_channels: 2,
                    block_frames: 256,
                    offset: 0,
                    len: 256,
                    playing: true,
                    position: (block * 256) as u64,
                    beat: (block * 256) as f64 * beats_per_sample,
                    beats_per_sample,
                    discontinuity: block == 0,
                },
            );
            for sample in &out {
                peak = peak.max(sample.abs());
            }
        }
        peak
    }

    #[test]
    fn effects_reach_the_graph_in_signal_order_and_it_still_runs() {
        let mut song = song_with_a_clip();
        song.add_device(0, DeviceKind::Poly).expect("instrument");
        song.add_device(0, DeviceKind::Reverb).expect("effect");
        song.add_device(0, DeviceKind::Sat).expect("effect");

        let (spec, _) = build(&song, &playing(&song));
        let order: Vec<&str> = spec
            .iter_ordered()
            .filter_map(|(_, node)| match node {
                NodeSpec::Poly { .. } => Some("poly"),
                NodeSpec::Reverb { .. } => Some("reverb"),
                NodeSpec::Sat { .. } => Some("sat"),
                _ => None,
            })
            .collect();
        assert_eq!(
            order,
            ["poly", "reverb", "sat"],
            "the chain did not reach the graph in the order it was built"
        );
        assert!(loudest(&song) > 0.01, "a chained track went silent");
    }

    #[test]
    fn a_bypassed_effect_is_passed_by_rather_than_built() {
        let mut song = song_with_a_clip();
        song.add_device(0, DeviceKind::Poly).expect("instrument");
        let sat = song.add_device(0, DeviceKind::Sat).expect("effect");
        song.device_mut(sat).expect("there").bypassed = true;

        let (spec, _) = build(&song, &playing(&song));
        assert!(
            !spec
                .iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Sat { .. })),
            "a bypassed effect was still built"
        );
        // And the signal still arrives: bypass is a pass, not a cut.
        assert!(
            loudest(&song) > 0.01,
            "bypassing an effect silenced the track"
        );
    }

    #[test]
    fn an_effects_settings_reach_its_node() {
        let mut song = song_with_a_clip();
        let id = song.add_device(0, DeviceKind::Sat).expect("effect");
        let device = song.device_mut(id).expect("there");
        assert!(device.set(crate::params::sat::DRIVE, 9.0));
        assert!(device.set(crate::params::sat::MODE, 3.0));
        let (drive, mode) = (
            device.value(crate::params::sat::DRIVE),
            device.value(crate::params::sat::MODE),
        );

        let (spec, _) = build(&song, &playing(&song));
        let built = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Sat { drive, mode, .. } => Some((*drive, *mode)),
                _ => None,
            })
            .expect("the effect is in the graph");
        assert_eq!(built.0, drive, "an edited value did not reach the node");
        assert_eq!(
            built.1,
            mode.round() as u32,
            "a choice did not survive becoming an index"
        );
    }

    #[test]
    fn an_effect_shapes_what_it_is_given() {
        // The claim a chain exists to make: the same notes through a
        // different chain are a different sound. A closed output trim is
        // the least ambiguous shape an effect can impose.
        let mut song = song_with_a_clip();
        song.add_device(0, DeviceKind::Poly).expect("instrument");
        let bare = loudest(&song);
        assert!(bare > 0.01, "the bare track was silent to begin with");

        let sat = song.add_device(0, DeviceKind::Sat).expect("effect");
        let device = song.device_mut(sat).expect("there");
        assert!(device.set(crate::params::sat::OUT, 0.0));
        assert!(device.set(crate::params::sat::MIX, 1.0));
        assert!(
            loudest(&song) < bare * 0.5,
            "the effect made no difference to the sound"
        );
    }

    #[test]
    fn every_effect_kind_this_song_can_hold_actually_builds() {
        // A device the model accepts and the compiler silently drops is
        // a chain that lies. Rack is the one absent kind, and it says so.
        use crate::devices::DEVICES;
        let mut missing = Vec::new();
        for spec in DEVICES.iter().filter(|spec| !spec.instrument) {
            let mut song = Song::default();
            let Some(id) = song.add_device(0, spec.kind) else {
                continue;
            };
            let device = song.device(id).expect("there");
            if effect_of(device).is_none() {
                missing.push(spec.name);
            }
        }
        assert_eq!(
            missing,
            ["rack"],
            "effects the chain accepts but cannot build"
        );
    }

    #[test]
    fn an_empty_trig_contributes_no_note() {
        let mut song = song_with_a_clip();
        song.patterns[0].clear(0);
        let (spec, _) = build(&song, &playing(&song));
        assert_eq!(voices(&spec), 0, "a pattern of rests still made a voice");
    }

    #[test]
    fn a_muted_note_is_not_played_while_its_trig_still_is() {
        let mut song = song_with_a_clip();
        song.patterns[0].toggle(4, Note::new(64, PATTERN_STEP_TICKS, 100));
        let heard = |song: &Song| {
            let (spec, _) = build(song, &playing(song));
            spec.iter_ordered()
                .find_map(|(_, node)| match node {
                    NodeSpec::Poly { notes, .. } => Some(notes.len()),
                    _ => None,
                })
                .unwrap_or(0)
        };
        assert_eq!(heard(&song), 2);
        song.patterns[0].trig_mut(4).notes[0].muted = true;
        assert_eq!(heard(&song), 1, "a muted note was played anyway");
    }
}
