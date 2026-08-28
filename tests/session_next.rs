#![allow(dead_code)]

#[path = "../src/ui/session_next.rs"]
mod session_next;

// Lets the isolated module's pointer tests use the app's established probe
// path now and keep the same path when the module is registered in `ui`.
mod ui {
    pub mod device {
        pub use daw::ui::device::probe;
    }
    // The affordance rule is the app's, not this module's — the same
    // shim the probe rides, for the same reason.
    pub use daw::ui::affordance;
}

#[test]
fn isolated_session_module_is_in_the_test_graph() {
    let document = session_next::SessionDocument::new(vec![session_next::SessionTrack {
        id: session_next::TrackId(1),
        name: "Synth".to_owned(),
        ..session_next::SessionTrack::default()
    }]);
    assert_eq!(document.tracks.len(), 1);
    assert_eq!(document.scenes.len(), session_next::DEFAULT_SCENES);
}
