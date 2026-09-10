//! DRIVE — play a script of keys into the running app and photograph it.
//!
//! A dev harness, like `lab` and `shot`, and the one that was missing.
//! `shot` renders a POSE: a stage built in Rust, drawn once. This drives
//! the REAL running application — the one with the audio engine, the
//! library scan and the window someone is watching — by pushing synthetic
//! key events into the same `RawInput` the keyboard fills, one per frame.
//!
//! Why it exists: an agent driving the app through the compositor
//! (`wtype` and friends) is at the mercy of who holds focus. Keys land in
//! whatever window is in front, which is somebody else's chat window as
//! often as not, and a frame can only be seen if the window happens to be
//! on screen. Neither is true here: the keys go straight into the app's
//! own input queue, and the picture is read back off its own texture.
//!
//! Not shipped. Off unless `DAW_DRIVE` names a path:
//!
//!     DAW_DRIVE=/tmp/daw-drive ./target/debug/stage
//!     echo 'key ctrl+f' > /tmp/daw-drive
//!
//! One command per frame, so the app moves at frame rate and a person
//! can watch it happen. The grammar:
//!
//!     key ctrl+shift+t     a chord, through the real keymap
//!     text breaks/break1   typed text (a space here cannot reach the transport)
//!     wait 30              frames with no input
//!     until 10 slices      hold until the status says this — no guessing
//!     gone scanning        hold until the status stops saying it
//!     pace 4               frames between commands from here on
//!     shot out/03.png      read this frame back and write it
//!     echo laying the beat a line in the trace
//!
//! Every command is echoed to stdout with the app's own status beside it,
//! so the script leaves a readable trace even when nobody looks at a
//! single picture.

use std::collections::VecDeque;
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// Wall-clock watchdog, independent of UI frame rate; script-overridable.
const UNTIL_CAP: Duration = Duration::from_secs(10);

/// One line of the script.
#[derive(Debug, Clone)]
enum Cmd {
    Key {
        key: egui::Key,
        modifiers: egui::Modifiers,
        name: String,
    },
    Text(String),
    Wait(u32),
    /// Hold until the status says this, or gives up after a cap.
    Until {
        text: String,
        gone: bool,
        deadline: Option<Instant>,
    },
    Timeout(Duration),
    Fail(String),
    Pace(u32),
    Shot(PathBuf),
    Echo(String),
}

/// The script channel, and where the frame loop is up to in it.
pub struct Drive {
    rx: Receiver<Cmd>,
    queue: VecDeque<Cmd>,
    /// Frames to sit still before taking the next command.
    hold: u32,
    /// Frames between commands: 1 is frame rate, higher is a slower
    /// demonstration. The script can change it as it goes.
    pace: u32,
    /// A picture asked for this frame, taken after the render.
    shot: Option<PathBuf>,
    /// The command already played, waiting for the status it produced.
    ///
    /// A trace line is only worth reading if it says what the command
    /// DID, and the app cannot say that until the frame has run. So each
    /// line is held back one frame and printed with the state it left
    /// behind. Printing the state beforehand reads plausibly and is
    /// useless: it describes the world the command was about to change,
    /// which is how three rounds went into pressing Tab the wrong way.
    said: Option<String>,
    seq: usize,
    timeout: Duration,
    failed: bool,
}

impl Drive {
    /// Open the channel named by `DAW_DRIVE`, or nothing at all.
    ///
    /// The path is a FIFO, made if it is not there already. A writer that
    /// opens and closes it — every `echo > fifo` does — would hand a
    /// plain reader EOF, so the reader reopens rather than stopping: the
    /// channel outlives any one script.
    pub fn open() -> Option<Self> {
        let path = std::env::var_os("DAW_DRIVE").map(PathBuf::from)?;
        make_fifo(&path)?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("drive".into())
            .spawn(move || {
                loop {
                    let Ok(file) = std::fs::File::open(&path) else {
                        return;
                    };
                    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
                        if let Some(cmd) = parse_script_line(&line)
                            && tx.send(cmd).is_err()
                        {
                            return;
                        }
                    }
                }
            })
            .ok()?;
        eprintln!("drive: listening");
        Some(Self {
            rx,
            queue: VecDeque::new(),
            hold: 0,
            pace: 1,
            shot: None,
            said: None,
            seq: 0,
            timeout: UNTIL_CAP,
            failed: false,
        })
    }

    /// The events for this frame, and the trace line for what was played.
    ///
    /// At most ONE command per frame. The stage's grammar takes at most
    /// one key from a frame — the discipline a real keyboard has — so a
    /// script that pushed six keys into one frame would lose five of them
    /// and lie about having pressed them.
    pub fn events(&mut self, status: &str) -> Vec<egui::Event> {
        self.events_at(status, Instant::now())
    }

    fn events_at(&mut self, status: &str, now: Instant) -> Vec<egui::Event> {
        // Last frame's command, now that the app has answered it.
        if let Some(line) = self.said.take() {
            println!("{line}{status}");
        }
        // Freeze before consuming another command, not when an external
        // runner eventually notices the trace. A new run needs a fresh Drive.
        if !self.failed && status.contains("REFUSED") {
            println!("drive GAVE UP: application refused · {status}");
            self.failed = true;
        }
        if self.failed {
            self.queue.clear();
            return Vec::new();
        }
        loop {
            match self.rx.try_recv() {
                Ok(cmd) => self.queue.push_back(cmd),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        if self.hold > 0 {
            self.hold -= 1;
            return Vec::new();
        }
        let Some(cmd) = self.queue.pop_front() else {
            return Vec::new();
        };
        self.seq += 1;
        self.hold = self.pace.saturating_sub(1);
        let seq = self.seq;
        let mut line = None;
        let mut say = |what: &str| line = Some(format!("drive {seq:>4} {what:<28} "));
        let events = match cmd {
            Cmd::Key {
                key,
                modifiers,
                name,
            } => {
                say(&format!("key {name}"));
                // Down AND up in the same frame: the stage reads presses,
                // and a key left down would read as held — which some of
                // the grammar (X to extend a selection) takes to mean
                // something else entirely.
                [true, false]
                    .map(|pressed| egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed,
                        repeat: false,
                        modifiers,
                    })
                    .to_vec()
            }
            Cmd::Text(text) => {
                say(&format!("text {text}"));
                vec![egui::Event::Text(text)]
            }
            Cmd::Until {
                text,
                gone,
                deadline,
            } => {
                let deadline = deadline.unwrap_or(now + self.timeout);
                let there = status.contains(text.as_str());
                if there != gone {
                    say(&format!("{} {text}", if gone { "gone" } else { "until" }));
                } else if now >= deadline {
                    say(&format!("GAVE UP waiting for {text}"));
                    self.failed = true;
                    self.queue.clear();
                } else {
                    // Not yet: put it back and try the next frame.
                    self.queue.push_front(Cmd::Until {
                        text,
                        gone,
                        deadline: Some(deadline),
                    });
                    self.seq -= 1;
                }
                Vec::new()
            }
            Cmd::Timeout(duration) => {
                self.timeout = duration;
                say(&format!("timeout {}s", duration.as_secs_f64()));
                Vec::new()
            }
            Cmd::Fail(error) => {
                say(&format!("GAVE UP {error}"));
                self.failed = true;
                self.queue.clear();
                Vec::new()
            }
            Cmd::Wait(frames) => {
                say(&format!("wait {frames}"));
                self.hold = frames;
                Vec::new()
            }
            Cmd::Pace(frames) => {
                self.pace = frames.max(1);
                say(&format!("pace {frames}"));
                Vec::new()
            }
            Cmd::Shot(path) => {
                say(&format!("shot {}", path.display()));
                self.shot = Some(path);
                Vec::new()
            }
            Cmd::Echo(text) => {
                say(&format!("· {text}"));
                Vec::new()
            }
        };
        self.said = line;
        events
    }

    /// The picture this frame owes, if the script asked for one.
    pub fn taking(&mut self) -> Option<PathBuf> {
        self.shot.take()
    }
}

/// Read a rendered texture back and write it as a PNG.
///
/// The offscreen target, before the screen treatment — the same picture
/// `shot` writes, and the legible one: the CRT pass is a filter over the
/// text, not the text.
pub fn capture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    size: [u32; 2],
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Rows in a copy destination start on a 256-byte boundary.
    let unpadded = size[0] * 4;
    let padded = unpadded.div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("drive_readback"),
        size: u64::from(padded) * u64::from(size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("drive_capture"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(size[1]),
            },
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()??;
    let mapped = slice.get_mapped_range()?;
    let mut rgba = Vec::with_capacity((unpadded * size[1]) as usize);
    for row in 0..size[1] {
        let start = (row * padded) as usize;
        let end = start + unpadded as usize;
        rgba.extend_from_slice(mapped.get(start..end).ok_or("short readback")?);
    }
    drop(mapped);
    buffer.unmap();

    // The native shell favours BGRA swapchains. A PNG always stores RGBA;
    // otherwise a captured teal visual becomes yellow despite correct GPU
    // rendering. This applies to every native TAKE screenshot, not just video.
    capture_rgba_order(&mut rgba, texture.format());

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, png(size[0], size[1], &rgba))?;
    Ok(())
}

fn capture_rgba_order(bytes: &mut [u8], format: wgpu::TextureFormat) {
    if matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb) {
        for pixel in bytes.chunks_exact_mut(4) { pixel.swap(0, 2); }
    }
}

#[test]
fn visual_capture_preserves_colour_for_bgra_and_rgba() {
    let mut bgra = [10, 20, 200, 255, 8, 50, 90, 128];
    capture_rgba_order(&mut bgra, wgpu::TextureFormat::Bgra8Unorm);
    assert_eq!(bgra, [200, 20, 10, 255, 90, 50, 8, 128]);
    let original = bgra;
    capture_rgba_order(&mut bgra, wgpu::TextureFormat::Rgba8UnormSrgb);
    assert_eq!(bgra, original);
}

/// `mkfifo`, without a crate for it.
fn make_fifo(path: &std::path::Path) -> Option<()> {
    if let Ok(meta) = std::fs::metadata(path) {
        // Already there. A regular file would be read once and never
        // again, which is a script that runs once and then goes quiet —
        // say so rather than pretending to listen.
        use std::os::unix::fs::FileTypeExt;
        if meta.file_type().is_fifo() {
            return Some(());
        }
        eprintln!("drive: {} exists and is not a fifo", path.display());
        return None;
    }
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .ok()?;
    status.success().then_some(())
}

/// One script line, or nothing for a blank line or a comment.
fn parse_script_line(line: &str) -> Option<Cmd> {
    let trimmed = line.trim();
    parse(line).or_else(|| {
        (!trimmed.is_empty() && !trimmed.starts_with('#'))
            .then(|| Cmd::Fail(format!("invalid script line: {trimmed}")))
    })
}

fn parse(line: &str) -> Option<Cmd> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let rest = rest.trim();
    match word {
        "key" => {
            let (modifiers, name) = chord(rest)?;
            // A key egui cannot name must SAY so. Dropped in silence it
            // would read as a key that was pressed and did nothing, and
            // the whole script after it would be diagnosed against a
            // state it never reached.
            let Some(key) = egui::Key::from_name(name) else {
                eprintln!("drive: no such key: {name}");
                return None;
            };
            Some(Cmd::Key {
                key,
                modifiers,
                name: rest.to_owned(),
            })
        }
        "text" => Some(Cmd::Text(rest.to_owned())),
        "wait" => Some(Cmd::Wait(rest.parse().ok()?)),
        // WAIT ON THE APP, NOT ON THE CLOCK. A fixed sleep is either a
        // guess that is too long (most of them, most of the time) or one
        // that is too short exactly once, on the run that matters.
        "until" if !rest.is_empty() => Some(Cmd::Until {
            text: rest.to_owned(),
            gone: false,
            deadline: None,
        }),
        "gone" if !rest.is_empty() => Some(Cmd::Until {
            text: rest.to_owned(),
            gone: true,
            deadline: None,
        }),
        "timeout" => rest
            .parse::<f64>()
            .ok()
            .filter(|s| s.is_finite() && (0.01..=3600.0).contains(s))
            .map(|s| Cmd::Timeout(Duration::from_secs_f64(s))),
        "pace" => Some(Cmd::Pace(rest.parse().ok()?)),
        "shot" => Some(Cmd::Shot(PathBuf::from(rest))),
        "echo" => Some(Cmd::Echo(rest.to_owned())),
        _ => {
            eprintln!("drive: no such command: {word}");
            None
        }
    }
}

/// `ctrl+shift+t` into the modifiers and the key's name.
///
/// COMMAND travels with CTRL. The stage's own `Mods::COMMAND` is what
/// every ctrl chord in the keymap is written with, and egui fills
/// `command` from ctrl on this platform — a chord that set only `ctrl`
/// would miss every one of them.
fn chord(text: &str) -> Option<(egui::Modifiers, &str)> {
    let mut modifiers = egui::Modifiers::NONE;
    let mut rest = text;
    while let Some((head, tail)) = rest.split_once('+') {
        match head.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "cmd" | "command" => {
                modifiers.ctrl = true;
                modifiers.command = true;
            }
            "shift" => modifiers.shift = true,
            "alt" | "option" => modifiers.alt = true,
            other => {
                eprintln!("drive: no such modifier: {other}");
                return None;
            }
        }
        rest = tail;
    }
    Some((modifiers, rest))
}

// ---- PNG, with no encoder --------------------------------------------
//
// Deflate STORED blocks: a valid zlib stream, trivially correct, and the
// file is a snapshot nobody keeps. The same writer `shot` uses.

fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((h * (w * 4 + 1)) as usize);
    for row in 0..h {
        raw.push(0); // filter: none
        let start = (row * w * 4) as usize;
        let end = start + (w * 4) as usize;
        raw.extend_from_slice(&rgba[start..end]);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut head = Vec::with_capacity(13);
    head.extend_from_slice(&w.to_be_bytes());
    head.extend_from_slice(&h.to_be_bytes());
    head.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(&mut out, b"IHDR", &head);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut crc = crc32(0xFFFF_FFFF, kind);
    crc = crc32(crc, body);
    out.extend_from_slice(&(crc ^ 0xFFFF_FFFF).to_be_bytes());
}

fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = data.chunks(0xFFFF).peekable();
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = chunks.next() {
        let last = u8::from(chunks.peek().is_none());
        out.push(last);
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn crc32(mut crc: u32, data: &[u8]) -> u32 {
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chord_carries_command_with_ctrl() {
        let (modifiers, key) = chord("ctrl+shift+t").expect("a chord");
        assert!(modifiers.ctrl && modifiers.command && modifiers.shift);
        assert!(!modifiers.alt);
        assert_eq!(key, "t");
    }

    #[test]
    fn the_grammar_reads_every_line_it_offers() {
        assert!(matches!(parse("key ctrl+f"), Some(Cmd::Key { .. })));
        assert!(matches!(parse("key ArrowDown"), Some(Cmd::Key { .. })));
        assert!(matches!(parse("key F2"), Some(Cmd::Key { .. })));
        // Text keeps its spaces: that is the whole point of not typing it
        // as keys, where a space reaches the transport instead.
        assert!(matches!(parse("text break 1.wav"), Some(Cmd::Text(t)) if t == "break 1.wav"));
        assert!(matches!(parse("wait 30"), Some(Cmd::Wait(30))));
        assert!(matches!(
            parse("until 10 slices"),
            Some(Cmd::Until { gone: false, .. })
        ));
        assert!(matches!(
            parse("gone scanning"),
            Some(Cmd::Until { gone: true, .. })
        ));
        assert!(matches!(parse("pace 4"), Some(Cmd::Pace(4))));
        assert!(matches!(parse("shot out/a.png"), Some(Cmd::Shot(_))));
        assert!(parse("").is_none());
        assert!(parse("# a comment").is_none());
        assert!(parse("frobnicate").is_none());
    }

    /// A picture nothing can open is worse than no picture: check the
    /// bytes are a PNG a decoder would accept, header, size and all.
    #[test]
    fn the_written_bytes_are_a_png() {
        let rgba = vec![0xAB; 4 * 4 * 4];
        let bytes = png(4, 4, &rgba);
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&bytes[12..16], b"IHDR");
        assert_eq!(
            u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            4
        );
        assert_eq!(bytes[24], 8, "bit depth");
        assert_eq!(bytes[25], 6, "rgba");
        assert!(bytes.ends_with(&[0xAE, 0x42, 0x60, 0x82]), "IEND crc");
    }
}

#[cfg(test)]
mod trace_tests {
    use super::*;

    /// A LINE SAYS WHAT ITS COMMAND DID, not what was true before it.
    ///
    /// The status is held back a frame deliberately: the app cannot
    /// answer for a key until the frame carrying it has run. Printing
    /// the state beforehand reads plausibly and describes the world the
    /// command was about to change.
    #[test]
    fn a_trace_line_carries_the_state_its_command_produced() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut drive = Drive {
            rx,
            queue: VecDeque::new(),
            hold: 0,
            pace: 1,
            shot: None,
            said: None,
            seq: 0,
            timeout: UNTIL_CAP,
            failed: false,
        };
        tx.send(parse("key Enter").expect("a key")).expect("sent");

        // The frame that plays it has nothing to report yet.
        let events = drive.events("scope SESSION");
        assert_eq!(events.len(), 2, "a press and a release");
        let line = drive.said.clone().expect("the line waits for its status");
        assert!(line.contains("key Enter"), "{line}");

        // The NEXT frame prints it, against the state it produced.
        let _ = drive.events("scope CLIP");
        assert!(
            drive.said.is_none(),
            "the held line was printed and not held twice"
        );
    }
}

#[cfg(test)]
mod until_tests {
    use super::*;

    fn driven(script: &[&str]) -> (Drive, std::sync::mpsc::Sender<Cmd>) {
        let (tx, rx) = std::sync::mpsc::channel();
        for line in script {
            tx.send(parse(line).expect("a command")).expect("sent");
        }
        (
            Drive {
                rx,
                queue: VecDeque::new(),
                hold: 0,
                pace: 1,
                shot: None,
                said: None,
                seq: 0,
                timeout: UNTIL_CAP,
                failed: false,
            },
            tx,
        )
    }

    /// `until` costs NOTHING once the thing has happened, and holds the
    /// script back until it has. That is the whole point: a fixed sleep
    /// is a guess that is too long every time but one.
    #[test]
    fn until_holds_until_the_app_says_so_and_then_goes_at_once() {
        let (mut drive, _tx) = driven(&["until 10 slices", "key Enter"]);

        // Nothing while the app is still working.
        for _ in 0..5 {
            assert!(drive.events("scope SAMPLE | scanning").is_empty());
        }
        // The frame it lands, the wait is over — and the NEXT frame
        // plays the key, rather than some number of frames later.
        assert!(
            drive
                .events("scope SAMPLE | 10 slices on onsets")
                .is_empty()
        );
        assert_eq!(
            drive.events("scope SAMPLE | 10 slices on onsets").len(),
            2,
            "the key follows immediately"
        );
    }

    /// And it gives up rather than hanging a run forever.
    #[test]
    fn until_gives_up_rather_than_hanging() {
        let (mut drive, _tx) = driven(&["until never happens", "key Enter"]);
        let start = Instant::now();
        assert!(drive.events_at("scope SESSION", start).is_empty());
        assert!(
            drive
                .events_at("scope SESSION", start + UNTIL_CAP)
                .is_empty()
        );
        assert!(drive.failed);
        assert!(
            drive
                .events_at("scope SESSION", start + UNTIL_CAP + Duration::from_secs(1))
                .is_empty()
        );
        assert!(drive.queue.is_empty(), "no later command can run");
    }

    #[test]
    fn timeout_is_elapsed_time_not_frame_count() {
        let (mut drive, _tx) = driven(&["timeout 30", "until ready", "key Enter"]);
        let start = Instant::now();
        drive.events_at("loading", start);
        for _ in 0..2000 {
            drive.events_at("loading", start);
        }
        assert!(!drive.failed);
        drive.events_at("ready", start + Duration::from_secs(29));
        assert_eq!(
            drive
                .events_at("ready", start + Duration::from_secs(29))
                .len(),
            2
        );
    }

    #[test]
    fn refusal_and_bad_script_lines_stop_before_the_next_edit() {
        let (mut drive, _tx) = driven(&["key Enter", "key ArrowUp"]);
        assert_eq!(drive.events("ready").len(), 2);
        assert!(drive.events("REFUSED unavailable").is_empty());
        assert!(drive.events("ready").is_empty());
        for bad in [
            "wait junk",
            "key NoSuchKey",
            "until",
            "timeout NaN",
            "timeout 0",
            "typo",
        ] {
            assert!(
                matches!(parse_script_line(bad), Some(Cmd::Fail(_))),
                "{bad}"
            );
        }
    }

    /// `gone` is the other half: hold while the app is still saying it.
    #[test]
    fn gone_waits_for_a_word_to_leave() {
        let (mut drive, _tx) = driven(&["gone scanning"]);
        assert!(
            drive
                .events("scope BROWSER | scanning the library")
                .is_empty()
        );
        assert!(
            drive
                .events("scope BROWSER | scanning the library")
                .is_empty()
        );
        let _ = drive.events("scope BROWSER | ready");
        assert!(drive.queue.is_empty(), "the wait finished");
    }
}
