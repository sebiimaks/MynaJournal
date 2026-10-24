use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct SecuritySettings {
    /// Zero disables timed locking; passwords are still forgotten on exit.
    pub auto_lock_seconds: u64,
    pub lock_on_leave: bool,
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            auto_lock_seconds: 300,
            lock_on_leave: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_defaults_and_partial_overrides_are_backward_compatible() {
        let defaults: crate::settings::Settings = toml::from_str("").unwrap();
        assert_eq!(defaults.security, SecuritySettings::default());
        let partial: crate::settings::Settings =
            toml::from_str("[security]\nauto_lock_seconds = 0\n").unwrap();
        assert_eq!(partial.security.auto_lock_seconds, 0);
        assert!(partial.security.lock_on_leave);
        let roundtrip: crate::settings::Settings =
            toml::from_str(&toml::to_string(&partial).unwrap()).unwrap();
        assert_eq!(roundtrip.security, partial.security);
    }
}
