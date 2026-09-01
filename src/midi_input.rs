//! Hardware MIDI input — the green-zone half.
//!
//! `midi_typing` turns the computer keyboard into a piano, which is a fine
//! authoring affordance and is not a MIDI port. This is the port: ALSA seq
//! on Linux, CoreMIDI on macOS, WinMM on Windows, through `midir`. The
//! reasoning for that choice over the `jack` crate — and the cost it
//! accepts — is `notes/20260831-midi-and-tempo-decisions.md`.
//!
//! **Nothing here reaches the audio callback.** `midir` runs its own
//! thread and hands bytes to a channel; the app drains that channel from
//! the green zone, exactly as it drains every other worker. Streaming
//! live events from here INTO the callback would violate the sequencing
//! contract's rule 4 — sequences ride compiled immutable chunks, and a
//! laggy thread must never be able to delay a note.

use std::sync::mpsc::{Receiver, Sender, channel};

/// One thing a controller said.
///
/// Only notes, deliberately. A controller says a great many things; the
/// ones a sequencer can act on today are note-on and note-off, and a
/// variant nothing consumes is a variant that rots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MidiEvent {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
}

/// Decode one MIDI message into the events this app can act on.
///
/// Returns `None` for everything else — clock, aftertouch, CC, sysex —
/// rather than guessing. A note-on with velocity zero is a note-OFF, which
/// is not a quirk but how most controllers release a key; missing it is
/// the classic stuck-note bug.
pub fn decode(message: &[u8]) -> Option<MidiEvent> {
    let (status, data) = message.split_first()?;
    // Channel messages: the low nibble is the channel, which we accept
    // from any source. A running-status message (no status byte) is not
    // decoded — ALSA and CoreMIDI both deliver complete messages.
    match (status & 0xF0, data) {
        (0x90, [note, velocity, ..]) if *velocity > 0 => Some(MidiEvent::NoteOn {
            note: *note & 0x7F,
            velocity: *velocity & 0x7F,
        }),
        (0x90, [note, _, ..]) | (0x80, [note, _, ..]) => {
            Some(MidiEvent::NoteOff { note: *note & 0x7F })
        }
        _ => None,
    }
}

/// The input service: which ports exist, which one is open, and what it
/// has said since the last drain.
pub struct MidiInput {
    ports: Vec<String>,
    connected: Option<String>,
    /// Held so the connection stays open; dropping it closes the port.
    connection: Option<midir::MidiInputConnection<Sender<(u64, MidiEvent)>>>,
    sender: Sender<(u64, MidiEvent)>,
    events: Receiver<(u64, MidiEvent)>,
}

impl Default for MidiInput {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            ports: Vec::new(),
            connected: None,
            connection: None,
            sender,
            events,
        }
    }
}

impl MidiInput {
    /// Re-read the port list. Cheap, and the only way to notice a
    /// controller that was plugged in after start-up.
    pub fn refresh(&mut self) {
        self.ports = match midir::MidiInput::new("daw") {
            Ok(input) => input
                .ports()
                .iter()
                .filter_map(|port| input.port_name(port).ok())
                .collect(),
            Err(_) => Vec::new(),
        };
    }

    pub fn ports(&self) -> &[String] {
        &self.ports
    }

    /// The open port's name, if one is open.
    pub fn connected(&self) -> Option<&str> {
        self.connected.as_deref()
    }

    /// Open a port by index into [`ports`].
    ///
    /// Refuses out loud rather than silently doing nothing, because a
    /// controller that appears connected and says nothing is the single
    /// most confusing failure this feature can have.
    pub fn connect(&mut self, index: usize) -> Result<(), String> {
        let input = midir::MidiInput::new("daw").map_err(|error| error.to_string())?;
        let ports = input.ports();
        let port = ports
            .get(index)
            .ok_or_else(|| format!("MIDI: no port {index}"))?;
        let name = input
            .port_name(port)
            .unwrap_or_else(|_| format!("port {index}"));
        let sender = self.sender.clone();
        // This closure runs on midir's OWN thread, never the audio
        // callback. A channel send here allocates, which is exactly why
        // it must never move into the red zone.
        let connection = input
            .connect(
                port,
                "daw-in",
                move |stamp, message, sender: &mut Sender<(u64, MidiEvent)>| {
                    if let Some(event) = decode(message) {
                        // A closed receiver means the app is going away;
                        // dropping the event is correct and silent.
                        let _ = sender.send((stamp, event));
                    }
                },
                sender,
            )
            .map_err(|error| format!("MIDI: cannot open {name}: {error}"))?;
        self.connection = Some(connection);
        self.connected = Some(name);
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.connection = None;
        self.connected = None;
    }

    /// Everything the controller has said since the last drain, oldest
    /// first. Never blocks.
    pub fn drain(&mut self) -> Vec<(u64, MidiEvent)> {
        self.events.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The decode table. This is the half that can be tested without
    /// hardware, and it is where the bugs live.
    #[test]
    fn note_on_and_note_off_decode() {
        assert_eq!(
            decode(&[0x90, 60, 100]),
            Some(MidiEvent::NoteOn {
                note: 60,
                velocity: 100
            })
        );
        assert_eq!(
            decode(&[0x80, 60, 0]),
            Some(MidiEvent::NoteOff { note: 60 })
        );
    }

    /// A note-on with velocity zero is a note-OFF. Most controllers
    /// release a key that way, and missing it is the classic stuck-note
    /// bug — the one that leaves a synth screaming after the hands have
    /// left the keys.
    #[test]
    fn a_zero_velocity_note_on_is_a_note_off() {
        assert_eq!(
            decode(&[0x90, 64, 0]),
            Some(MidiEvent::NoteOff { note: 64 })
        );
    }

    /// Any channel is accepted: the low nibble is the channel and this
    /// app listens to all of them.
    #[test]
    fn every_channel_is_heard() {
        for channel in 0..16u8 {
            assert_eq!(
                decode(&[0x90 | channel, 48, 64]),
                Some(MidiEvent::NoteOn {
                    note: 48,
                    velocity: 64
                }),
                "channel {channel}"
            );
        }
    }

    /// Everything else is ignored rather than guessed at. A variant
    /// nothing consumes is a variant that rots.
    #[test]
    fn other_messages_are_not_guessed_at() {
        for message in [
            &[0xB0, 7, 100][..], // CC
            &[0xE0, 0, 64][..],  // pitch bend
            &[0xF8][..],         // clock
            &[0xD0, 64][..],     // channel pressure
            &[][..],             // nothing at all
            &[0x90][..],         // truncated
        ] {
            assert_eq!(decode(message), None, "{message:?} must not decode");
        }
    }

    /// Data bytes are masked to seven bits, so a malformed message can
    /// never produce a note number outside the MIDI range.
    #[test]
    fn data_bytes_stay_inside_seven_bits() {
        let Some(MidiEvent::NoteOn { note, velocity }) = decode(&[0x90, 0xFF, 0xFF]) else {
            panic!("decodes");
        };
        assert!(note <= 127);
        assert!(velocity <= 127);
    }

    /// A service with no port open drains nothing and does not block.
    #[test]
    fn draining_an_unconnected_service_is_empty_and_instant() {
        let mut input = MidiInput::default();
        assert!(input.connected().is_none());
        assert!(input.drain().is_empty());
    }
}
