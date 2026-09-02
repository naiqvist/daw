//! Project save/load. The serialization rule from transport.rs holds: only
//! musical time and node specs are stored — never sample positions, never
//! runtime state. A project saved at one sample rate opens at any other.

use crate::audio::graph::{GraphSpec, NodeId, NodeSpec};
use std::path::Path;

/// The on-disk document: nodes in order, wires by position, RON-encoded.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub bpm: f64,
    pub nodes: Vec<NodeSpec>,
    /// Indices into `nodes`.
    pub wires: Vec<(usize, usize)>,
    pub output: Option<usize>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("encode: {0}")]
    Encode(#[from] ron::Error),
    #[error("decode: {0}")]
    Decode(#[from] ron::de::SpannedError),
    #[error("a wire or output references a node index that does not exist")]
    BadIndex,
}

impl Project {
    /// Snapshot a graph. NodeIds are runtime-only (generational); the doc
    /// stores positions in insertion order instead.
    pub fn from_spec(spec: &GraphSpec, bpm: f64) -> Self {
        let ordered: Vec<(NodeId, &NodeSpec)> = spec.iter_ordered().collect();
        let pos_of = |id: NodeId| ordered.iter().position(|(i, _)| *i == id);
        Self {
            bpm,
            nodes: ordered.iter().map(|(_, n)| (*n).clone()).collect(),
            wires: spec
                .wires()
                .iter()
                .filter_map(|(a, b)| Some((pos_of(*a)?, pos_of(*b)?)))
                .collect(),
            output: spec.output().and_then(pos_of),
        }
    }

    /// Rebuild a live graph. Returns the spec plus the fresh NodeIds in doc
    /// order, so callers can rebind parameter addressing.
    pub fn into_spec(&self) -> Result<(GraphSpec, Vec<NodeId>), ProjectError> {
        let mut spec = GraphSpec::default();
        let ids: Vec<NodeId> = self.nodes.iter().map(|n| spec.push(n.clone())).collect();
        for (a, b) in &self.wires {
            let (Some(fa), Some(fb)) = (ids.get(*a), ids.get(*b)) else {
                return Err(ProjectError::BadIndex);
            };
            spec.connect(*fa, *fb);
        }
        if let Some(o) = self.output {
            spec.set_output(*ids.get(o).ok_or(ProjectError::BadIndex)?);
        }
        Ok((spec, ids))
    }

    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, ProjectError> {
        Ok(ron::from_str(&std::fs::read_to_string(path)?)?)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;
    use crate::audio::bounce::{BounceOptions, bounce};
    use crate::audio::graph::{NodeSpec, Note, SubLoop};

    fn demo_spec() -> GraphSpec {
        let mut spec = GraphSpec::default();
        let seq = spec.push(NodeSpec::Seq {
            notes: vec![
                Note {
                    start_beats: 0.0,
                    len_beats: 0.4,
                    pitch: 60,
                    vel: 100,
                    plocks: Vec::new(),
                    fx_locks: Vec::new(),
                    prob: 1.0,
                    cond: None,
                },
                Note {
                    start_beats: 1.0,
                    len_beats: 0.4,
                    pitch: 64,
                    vel: 90,
                    plocks: Vec::new(),
                    fx_locks: Vec::new(),
                    prob: 1.0,
                    cond: None,
                },
            ],
            subloops: vec![SubLoop {
                start_beats: 0.0,
                end_beats: 2.0,
                repeats: 2,
            }],
            loop_len_beats: Some(4.0),
            params: Default::default(),
        });
        let pan = spec.push(NodeSpec::Pan {
            pan: -0.3,
            gain: 1.0,
        });
        let mix = spec.push(NodeSpec::Mixer { gain: 0.5 });
        spec.connect(seq, pan);
        spec.connect(pan, mix);
        spec.set_output(mix);
        spec
    }

    #[test]
    fn project_round_trips_through_ron() {
        let spec = demo_spec();
        let doc = Project::from_spec(&spec, 133.7);
        let path = std::env::temp_dir().join("daw-test-project.ron");
        doc.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();
        assert_eq!(doc, loaded, "document must survive the disk");

        let (spec2, ids) = loaded.into_spec().unwrap();
        assert_eq!(ids.len(), 3);
        let doc2 = Project::from_spec(&spec2, loaded.bpm);
        assert_eq!(
            doc, doc2,
            "spec -> doc -> spec -> doc must be a fixed point"
        );
    }

    #[test]
    fn bad_indices_are_refused() {
        let mut doc = Project::from_spec(&demo_spec(), 120.0);
        doc.wires.push((0, 99));
        assert!(matches!(doc.into_spec(), Err(ProjectError::BadIndex)));
    }

    #[test]
    fn saved_project_bounces_identically_to_the_original() {
        // The test that makes save/load mean something: the ORIGINAL graph
        // and the SAVED-AND-RELOADED graph must render byte-identical audio.
        let spec = demo_spec();
        let doc = Project::from_spec(&spec, 120.0);
        let path = std::env::temp_dir().join("daw-test-project2.ron");
        doc.save(&path).unwrap();
        let (spec2, _) = Project::load(&path).unwrap().into_spec().unwrap();

        let opts = BounceOptions {
            length_beats: 6.0,
            ..Default::default()
        };
        let wav_a = std::env::temp_dir().join("daw-test-bounce-a.wav");
        let wav_b = std::env::temp_dir().join("daw-test-bounce-b.wav");
        bounce(&spec, &opts, &wav_a).unwrap();
        bounce(&spec2, &opts, &wav_b).unwrap();

        let a = std::fs::read(&wav_a).unwrap();
        let b = std::fs::read(&wav_b).unwrap();
        assert!(!a.is_empty());
        assert_eq!(a, b, "reloaded project must render byte-identical audio");
    }
}
