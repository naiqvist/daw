//! The wiring vocabulary: what a device widget knows about a parameter.
//!
//! A [`Param`] is a description, not a value — name, normalized↔natural
//! mapping, unit formatting, default, polarity. Panels own the normalized
//! value; the engine owns the natural one; a `Param` is how the two agree
//! without meeting.

/// Normalized (`0..=1`) ↔ natural value mapping. The natural side is
/// whatever the unit means: Hz, dB, ms, percent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mapping {
    /// Straight line between `min` and `max`.
    Linear { min: f32, max: f32 },
    /// Exponential sweep — equal normalized steps are equal *ratios*.
    /// The right mapping for frequency and time: the octave from 100 to
    /// 200 Hz gets as much travel as the one from 5 k to 10 k.
    /// Both endpoints must be positive.
    Log { min: f32, max: f32 },
    /// Linear in decibels. The natural value IS the dB figure; converting
    /// to amplitude is the engine's business.
    Db { min_db: f32, max_db: f32 },
}

impl Mapping {
    /// Normalized to natural. Input is clamped to `0..=1`.
    pub fn to_value(self, norm: f32) -> f32 {
        let t = norm.clamp(0.0, 1.0);
        match self {
            Self::Linear { min, max } => min + (max - min) * t,
            Self::Log { min, max } => min * (max / min).powf(t),
            Self::Db { min_db, max_db } => min_db + (max_db - min_db) * t,
        }
    }

    /// Natural to normalized, clamped to `0..=1`.
    pub fn to_norm(self, value: f32) -> f32 {
        let t = match self {
            Self::Linear { min, max }
            | Self::Db {
                min_db: min,
                max_db: max,
            } => {
                if max == min {
                    0.0
                } else {
                    (value - min) / (max - min)
                }
            }
            Self::Log { min, max } => {
                if min <= 0.0 || max <= 0.0 || max == min || value <= 0.0 {
                    0.0
                } else {
                    (value / min).ln() / (max / min).ln()
                }
            }
        };
        if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) }
    }
}

/// How a natural value prints. Formatting picks precision by magnitude so
/// readouts stay short: `18.5 Hz`, `1.25 kHz`, `-6.0 dB`, `35 ms`, `1.20 s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Hz,
    Db,
    /// Natural value is already in percent (map `Linear { 0.0, 100.0 }`).
    Percent,
    Ms,
    Seconds,
    Semitones,
    /// Bare number, two decimals.
    Plain,
}

impl Unit {
    pub fn format(self, v: f32) -> String {
        match self {
            Self::Hz if v >= 1000.0 => format!("{:.2} kHz", v / 1000.0),
            Self::Hz if v >= 100.0 => format!("{v:.0} Hz"),
            Self::Hz => format!("{v:.1} Hz"),
            Self::Db => format!("{v:+.1} dB"),
            Self::Percent => format!("{v:.0} %"),
            Self::Ms if v >= 1000.0 => format!("{:.2} s", v / 1000.0),
            Self::Ms if v >= 100.0 => format!("{v:.0} ms"),
            Self::Ms => format!("{v:.1} ms"),
            Self::Seconds => format!("{v:.2} s"),
            Self::Semitones => format!("{v:+.0} st"),
            Self::Plain => format!("{v:.2}"),
        }
    }
}

/// A parameter description. Widgets take one of these plus a `&mut f32`
/// normalized value; everything else (labeling, formatting, default,
/// bipolar rendering) follows from the description.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Param {
    pub name: &'static str,
    pub mapping: Mapping,
    pub unit: Unit,
    /// Where double-click-to-reset returns to, normalized.
    pub default_norm: f32,
    /// Rendering hint: fill from the center instead of the minimum
    /// (pan, pitch offset, EQ gain).
    pub bipolar: bool,
}

impl Param {
    pub fn new(name: &'static str, mapping: Mapping, unit: Unit) -> Self {
        Self {
            name,
            mapping,
            unit,
            default_norm: 0.0,
            bipolar: false,
        }
    }

    /// Log-mapped frequency parameter.
    pub fn hz(name: &'static str, min: f32, max: f32) -> Self {
        Self::new(name, Mapping::Log { min, max }, Unit::Hz)
    }

    /// Linear-in-dB gain parameter.
    pub fn db(name: &'static str, min_db: f32, max_db: f32) -> Self {
        Self::new(name, Mapping::Db { min_db, max_db }, Unit::Db)
    }

    /// 0–100 % parameter (mix, depth, resonance).
    pub fn percent(name: &'static str) -> Self {
        Self::new(
            name,
            Mapping::Linear {
                min: 0.0,
                max: 100.0,
            },
            Unit::Percent,
        )
    }

    /// Log-mapped time parameter in milliseconds.
    pub fn ms(name: &'static str, min: f32, max: f32) -> Self {
        Self::new(name, Mapping::Log { min, max }, Unit::Ms)
    }

    /// Set the default from a NATURAL value (`440.0`, `-6.0`).
    pub fn with_default(mut self, value: f32) -> Self {
        self.default_norm = self.mapping.to_norm(value);
        self
    }

    /// Mark bipolar and default to center unless a default was given.
    pub fn bipolar(mut self) -> Self {
        self.bipolar = true;
        self
    }

    /// Natural value at this normalized position.
    pub fn value(&self, norm: f32) -> f32 {
        self.mapping.to_value(norm)
    }

    /// Formatted natural value at this normalized position.
    pub fn format(&self, norm: f32) -> String {
        self.unit.format(self.mapping.to_value(norm))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_maps_and_roundtrips() {
        let m = Mapping::Linear {
            min: -12.0,
            max: 12.0,
        };
        assert_eq!(m.to_value(0.0), -12.0);
        assert_eq!(m.to_value(1.0), 12.0);
        assert_eq!(m.to_value(0.5), 0.0);
        for n in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            assert!((m.to_norm(m.to_value(n)) - n).abs() < 1e-6);
        }
        // Out-of-range input clamps rather than extrapolating.
        assert_eq!(m.to_value(2.0), 12.0);
        assert_eq!(m.to_norm(99.0), 1.0);
    }

    #[test]
    fn log_maps_equal_ratios_to_equal_travel() {
        let m = Mapping::Log {
            min: 20.0,
            max: 20_480.0, // ten octaves exactly
        };
        assert!((m.to_value(0.0) - 20.0).abs() < 1e-3);
        assert!((m.to_value(1.0) - 20_480.0).abs() < 1e-1);
        // One octave per tenth of travel.
        assert!((m.to_value(0.1) - 40.0).abs() < 1e-2);
        assert!((m.to_value(0.5) - 640.0).abs() < 1e-1);
        for n in [0.0f32, 0.3, 0.5, 0.9, 1.0] {
            assert!((m.to_norm(m.to_value(n)) - n).abs() < 1e-5);
        }
        // Degenerate inputs return 0, never NaN.
        assert_eq!(m.to_norm(0.0), 0.0);
        assert_eq!(m.to_norm(-5.0), 0.0);
    }

    #[test]
    fn db_is_linear_in_db() {
        let m = Mapping::Db {
            min_db: -60.0,
            max_db: 6.0,
        };
        assert_eq!(m.to_value(0.0), -60.0);
        assert_eq!(m.to_value(1.0), 6.0);
        assert!((m.to_norm(-27.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn units_format_by_magnitude() {
        assert_eq!(Unit::Hz.format(18.5), "18.5 Hz");
        assert_eq!(Unit::Hz.format(440.0), "440 Hz");
        assert_eq!(Unit::Hz.format(1250.0), "1.25 kHz");
        assert_eq!(Unit::Db.format(-6.0), "-6.0 dB");
        assert_eq!(Unit::Db.format(3.0), "+3.0 dB");
        assert_eq!(Unit::Percent.format(35.0), "35 %");
        assert_eq!(Unit::Ms.format(35.0), "35.0 ms");
        assert_eq!(Unit::Ms.format(350.0), "350 ms");
        assert_eq!(Unit::Ms.format(1200.0), "1.20 s");
        assert_eq!(Unit::Semitones.format(7.0), "+7 st");
        assert_eq!(Unit::Plain.format(0.5), "0.50");
    }

    #[test]
    fn param_builders_wire_defaults() {
        let cutoff = Param::hz("Cutoff", 20.0, 20_000.0).with_default(1_000.0);
        assert!((cutoff.value(cutoff.default_norm) - 1_000.0).abs() < 1.0);
        assert_eq!(cutoff.format(cutoff.default_norm), "1.00 kHz");

        let pan = Param::percent("Pan").bipolar().with_default(50.0);
        assert!(pan.bipolar);
        assert!((pan.default_norm - 0.5).abs() < 1e-6);
    }
}
