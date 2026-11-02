//! Feature flags for unfinished work. Each flag is a row in the `settings`
//! table of `app.db`, `feature.<name>`, holding `true` or `false`, and is
//! off unless it holds `true`. A flag hides its UI and leaves its MCP tools
//! out; migrations always run, whatever the flags say. Each flag is deleted
//! once its work ships.

use std::fmt;
use std::str::FromStr;

/// A piece of unfinished work behind a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    /// Milestones, epics, dependencies and the new statuses.
    Structure,
    Canvas,
    Assistant,
    Shaping,
    Automation,
}

impl Feature {
    /// Every feature, in the order Settings lists them.
    pub const ALL: [Self; 5] = [
        Self::Structure,
        Self::Canvas,
        Self::Assistant,
        Self::Shaping,
        Self::Automation,
    ];

    /// The feature's name, such as `structure`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Structure => "structure",
            Self::Canvas => "canvas",
            Self::Assistant => "assistant",
            Self::Shaping => "shaping",
            Self::Automation => "automation",
        }
    }

    /// The key of its row in `settings`, such as `feature.structure`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Structure => "feature.structure",
            Self::Canvas => "feature.canvas",
            Self::Assistant => "feature.assistant",
            Self::Shaping => "feature.shaping",
            Self::Automation => "feature.automation",
        }
    }

    /// The feature whose row is `key`, if any.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|feature| feature.key() == key)
    }
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Feature {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|feature| feature.as_str() == s)
            .ok_or_else(|| {
                format!(
                    "`{s}` is not a feature; use structure, canvas, assistant, shaping or \
                     automation"
                )
            })
    }
}

/// Which features are on. The default has every one off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Features {
    on: [bool; Feature::ALL.len()],
}

impl Features {
    /// Every feature off.
    pub const OFF: Self = Self {
        on: [false; Feature::ALL.len()],
    };

    pub fn is_on(&self, feature: Feature) -> bool {
        self.on[feature as usize]
    }

    /// Turns `feature` on or off.
    pub fn set(&mut self, feature: Feature, on: bool) {
        self.on[feature as usize] = on;
    }

    /// These features with `feature` turned on or off.
    pub fn with(mut self, feature: Feature, on: bool) -> Self {
        self.set(feature, on);
        self
    }

    /// The features that are on, in [`Feature::ALL`] order.
    pub fn enabled(&self) -> impl Iterator<Item = Feature> + '_ {
        Feature::ALL
            .into_iter()
            .filter(|&feature| self.is_on(feature))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_keys_round_trip() {
        for (index, feature) in Feature::ALL.into_iter().enumerate() {
            assert_eq!(feature as usize, index);
            assert_eq!(feature.as_str().parse::<Feature>(), Ok(feature));
            assert_eq!(feature.key(), format!("feature.{feature}"));
            assert_eq!(Feature::from_key(feature.key()), Some(feature));
        }
        assert!("Structure".parse::<Feature>().is_err());
        assert_eq!(Feature::from_key("structure"), None);
        assert_eq!(Feature::from_key("feature.later"), None);
    }

    #[test]
    fn features_start_off() {
        let features = Features::default();
        assert_eq!(features, Features::OFF);
        assert_eq!(features.enabled().count(), 0);
        for feature in Feature::ALL {
            assert!(!features.is_on(feature));
        }
    }

    #[test]
    fn features_turn_on_and_off_one_at_a_time() {
        let features = Features::OFF
            .with(Feature::Canvas, true)
            .with(Feature::Automation, true);
        assert_eq!(
            features.enabled().collect::<Vec<_>>(),
            [Feature::Canvas, Feature::Automation]
        );
        assert!(!features.is_on(Feature::Structure));
        let features = features.with(Feature::Canvas, false);
        assert_eq!(
            features.enabled().collect::<Vec<_>>(),
            [Feature::Automation]
        );
    }
}
