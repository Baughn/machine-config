pub mod commands;

#[derive(Debug, Clone)]
pub struct Config {
    pub channel: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channel: "nixos-unstable".to_string(),
        }
    }
}