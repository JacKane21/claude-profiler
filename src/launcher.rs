use std::process::Command;

use anyhow::Result;

use crate::config::Profile;

/// Launch Claude Code with the specified profile's environment variables.
/// On Unix, this uses exec() to replace the current process entirely.
#[cfg(unix)]
pub fn exec_claude(profile: &Profile) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mut cmd = Command::new("claude");

    // Set all environment variables from the profile
    // This is additive - existing env vars are preserved
    for (key, value) in &profile.env {
        cmd.env(key, value);
    }

    // exec() replaces the current process - this never returns on success
    let err = cmd.exec();

    // If we get here, exec failed
    Err(err.into())
}

/// Launch Claude Code with the specified profile's environment variables.
/// On Windows, we spawn a child process since exec() isn't available.
#[cfg(windows)]
pub fn exec_claude(profile: &Profile) -> Result<()> {
    let mut cmd = Command::new("claude");

    // Set all environment variables from the profile
    for (key, value) in &profile.env {
        cmd.env(key, value);
    }

    // On Windows, we spawn and wait for the process
    let status = cmd.status()?;

    if !status.success() {
        anyhow::bail!("Claude Code exited with status: {}", status);
    }

    Ok(())
}
