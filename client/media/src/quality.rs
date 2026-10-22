//! Screen-share quality presets.
//!
//! The person who shares picks one before the next share starts. The publisher
//! uses that preset as the encoder's bitrate ceiling. Changing it on a live
//! share, and a server or admin cap, are later steps.

/// A fixed screen-share bitrate the sharer can choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenShareBitrate {
    /// 3.5 Mbps. The smaller ceiling, for a constrained link.
    Economical,
    /// 5 Mbps. Used when the sharer has not chosen another preset.
    Standard,
    /// 6 Mbps.
    High,
    /// 8 Mbps. The ceiling aimed at sharp text.
    SharpText,
}

impl ScreenShareBitrate {
    /// The preset used when the sharer has not chosen one.
    pub const DEFAULT: Self = Self::Standard;

    /// The presets, in the order the shell shows them.
    pub const ALL: [Self; 4] = [
        Self::Economical,
        Self::Standard,
        Self::High,
        Self::SharpText,
    ];

    /// Encoder ceiling in bits per second.
    pub const fn bits_per_second(self) -> u64 {
        match self {
            Self::Economical => 3_500_000,
            Self::Standard => 5_000_000,
            Self::High => 6_000_000,
            Self::SharpText => 8_000_000,
        }
    }

    /// Encoder ceiling in kilobits per second.
    ///
    /// These are the four rates the sharer chooses among: 3500, 5000, 6000
    /// and 8000.
    pub const fn kilobits_per_second(self) -> u32 {
        (self.bits_per_second() / 1_000) as u32
    }

    /// The shell label: a short name and the rate.
    ///
    /// The Phase 0 shell is English, so the names follow that language and
    /// the rate sits in parentheses.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Economical => "Economical (3.5 Mbps)",
            Self::Standard => "Standard (5 Mbps)",
            Self::High => "High (6 Mbps)",
            Self::SharpText => "Sharp text (8 Mbps)",
        }
    }

    /// The preset named by a shell label.
    ///
    /// An unknown label is the default, so a share still starts when the
    /// label cannot be read.
    pub fn from_label(label: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|preset| preset.label() == label)
            .unwrap_or(Self::DEFAULT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_five_megabits() {
        assert_eq!(ScreenShareBitrate::DEFAULT, ScreenShareBitrate::Standard);
        assert_eq!(ScreenShareBitrate::DEFAULT.bits_per_second(), 5_000_000);
        assert_eq!(ScreenShareBitrate::DEFAULT.kilobits_per_second(), 5_000);
        assert_eq!(ScreenShareBitrate::DEFAULT.label(), "Standard (5 Mbps)");
    }

    #[test]
    fn the_presets_are_the_four_fixed_rates() {
        let rates: Vec<u32> = ScreenShareBitrate::ALL
            .iter()
            .copied()
            .map(ScreenShareBitrate::kilobits_per_second)
            .collect();
        assert_eq!(rates, vec![3_500, 5_000, 6_000, 8_000]);
        assert_eq!(ScreenShareBitrate::Economical.bits_per_second(), 3_500_000);
        assert_eq!(ScreenShareBitrate::High.bits_per_second(), 6_000_000);
        assert_eq!(ScreenShareBitrate::SharpText.bits_per_second(), 8_000_000);
    }

    #[test]
    fn each_label_selects_its_preset_and_an_unknown_label_is_the_default() {
        for preset in ScreenShareBitrate::ALL {
            assert_eq!(ScreenShareBitrate::from_label(preset.label()), preset);
            assert!(preset.label().ends_with("Mbps)"));
        }
        assert_eq!(
            ScreenShareBitrate::from_label("not a preset"),
            ScreenShareBitrate::DEFAULT
        );
        assert_eq!(
            ScreenShareBitrate::from_label(""),
            ScreenShareBitrate::DEFAULT
        );
    }
}
