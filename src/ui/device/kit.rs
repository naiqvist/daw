//! KIT's parameter mappings for the old binary's knobs.
//!
//! No card: the kit is built for the stage. These four functions exist
//! because the old rack's tables are matched exhaustively by kind, and
//! they defer to the brick's for every pad row.

use crate::params::kit as kp;
use crate::params::{self};
use crate::ui::device::brick;

fn linear_norm(param: u32, value: f32) -> f32 {
    let def = params::def(kp::TABLE, param);
    let span = def.max - def.min;
    if span > 0.0 {
        ((value - def.min) / span).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub fn kit_norm(param: u32, value: f32) -> f32 {
    match kp::pad_of(param) {
        Some((_, sub)) if sub < kp::PAN => brick::brick_norm(sub, value),
        _ => linear_norm(param, value),
    }
}

pub fn kit_value(param: u32, norm: f32) -> f32 {
    match kp::pad_of(param) {
        Some((_, sub)) if sub < kp::PAN => brick::brick_value(sub, norm),
        _ => {
            let def = params::def(kp::TABLE, param);
            def.clamp(def.min + (def.max - def.min) * norm.clamp(0.0, 1.0))
        }
    }
}

pub fn kit_is_discrete(param: u32) -> bool {
    match kp::pad_of(param) {
        Some((_, sub)) if sub < kp::PAN => brick::brick_is_discrete(sub),
        Some((_, sub)) => matches!(sub, kp::GROUP | kp::ON),
        None => matches!(param, kp::BASE | kp::PAD | kp::SOLO),
    }
}

pub fn kit_is_log(param: u32) -> bool {
    match kp::pad_of(param) {
        Some((_, sub)) if sub < kp::PAN => brick::brick_is_log(sub),
        Some(_) => false,
        None => param == kp::TIGHT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_rows_answer_as_the_brick_does_and_kit_rows_round_trip() {
        for def in crate::params::brick::TABLE {
            let id = kp::pad_param(7, def.id);
            assert_eq!(kit_is_log(id), brick::brick_is_log(def.id));
            assert_eq!(kit_is_discrete(id), brick::brick_is_discrete(def.id));
            let v = kit_value(id, 0.5);
            assert!((def.min..=def.max).contains(&v));
            assert!((kit_value(id, kit_norm(id, v)) - v).abs() <= v.abs() * 1e-3 + 1e-3);
        }
        for id in [kp::BASE, kp::TUNE, kp::TIGHT, kp::pad_param(0, kp::PAN)] {
            let def = params::def(kp::TABLE, id);
            assert_eq!(kit_value(id, 0.0), def.min);
            assert_eq!(kit_value(id, 1.0), def.max);
        }
        assert!(kit_is_discrete(kp::pad_param(2, kp::GROUP)) && !kit_is_discrete(kp::TUNE));
    }
}
