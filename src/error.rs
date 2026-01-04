use thiserror::Error;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum ProfilerError {
    #[error("Configuration file not found at {path}")]
    ConfigNotFound { path: String },

    #[error("Failed to parse configuration: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("Failed to create config directory: {0}")]
    ConfigDirCreate(std::io::Error),

    #[error("No profiles defined in configuration")]
    NoProfiles,

    #[error("Profile '{name}' not found")]
    ProfileNotFound { name: String },

    #[error("Claude Code command not found. Is it installed and in PATH?")]
    ClaudeNotFound,

    #[error("Failed to launch Claude Code: {0}")]
    LaunchFailed(std::io::Error),
}
