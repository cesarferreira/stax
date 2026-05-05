use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SUCCESS_ALERT_WAV: &[u8] = include_bytes!("../assets/ci-alert-success.wav");
const ERROR_ALERT_WAV: &[u8] = include_bytes!("../assets/ci-alert-error.wav");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltInSound {
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sound {
    BuiltIn(BuiltInSound),
    Path(PathBuf),
}

pub fn play_sound(sound: &Sound) -> Result<(), String> {
    match sound {
        Sound::BuiltIn(kind) => {
            let file = write_built_in_sound(*kind)?;
            play_sound_file(file.path())
        }
        Sound::Path(path) => {
            if !path.exists() {
                return Err(format!("sound file does not exist: {}", path.display()));
            }

            play_sound_file(path)
        }
    }
}

fn write_built_in_sound(kind: BuiltInSound) -> Result<tempfile::NamedTempFile, String> {
    let (prefix, bytes) = match kind {
        BuiltInSound::Success => ("stax-ci-alert-success-", SUCCESS_ALERT_WAV),
        BuiltInSound::Error => ("stax-ci-alert-error-", ERROR_ALERT_WAV),
    };

    let mut file = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(".wav")
        .tempfile()
        .map_err(|err| format!("could not create built-in CI alert sound file: {err}"))?;
    file.write_all(bytes).map_err(|err| {
        format!(
            "could not write built-in CI alert sound to {}: {}",
            file.path().display(),
            err
        )
    })?;
    Ok(file)
}

#[allow(clippy::needless_return)]
fn play_sound_file(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return run_status(Command::new("afplay").arg(path)).map_err(|err| err.to_string());
    }

    #[cfg(target_os = "linux")]
    {
        for program in ["paplay", "pw-play", "aplay", "ffplay"] {
            let mut command = Command::new(program);
            if program == "ffplay" {
                command.args(["-nodisp", "-autoexit", "-loglevel", "quiet"]);
            }
            command.arg(path);

            match run_status(&mut command) {
                Ok(()) => return Ok(()),
                Err(SoundCommandError::NotFound) => continue,
                Err(SoundCommandError::Failed(_)) => continue,
            }
        }

        return Err(
            "no supported audio player found (tried paplay, pw-play, aplay, ffplay)".to_string(),
        );
    }

    #[cfg(target_os = "windows")]
    {
        let path = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string()
            .replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName PresentationCore; \
             $p = New-Object System.Windows.Media.MediaPlayer; \
             $p.Open([Uri]::new('{path}')); \
             while (-not $p.NaturalDuration.HasTimeSpan) {{ Start-Sleep -Milliseconds 50 }}; \
             $ms = [Math]::Ceiling($p.NaturalDuration.TimeSpan.TotalMilliseconds) + 100; \
             $p.Play(); \
             Start-Sleep -Milliseconds $ms;"
        );
        let mut command = Command::new("powershell");
        command.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ]);
        return run_status(&mut command).map_err(|err| err.to_string());
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = path;
        Err("CI alert sounds are not supported on this platform".to_string())
    }
}

enum SoundCommandError {
    NotFound,
    Failed(String),
}

impl std::fmt::Display for SoundCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "audio player command was not found"),
            Self::Failed(message) => write!(f, "{message}"),
        }
    }
}

fn run_status(command: &mut Command) -> Result<(), SoundCommandError> {
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match output {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(SoundCommandError::Failed(format!(
            "audio player exited with {}",
            status
        ))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(SoundCommandError::NotFound),
        Err(err) => Err(SoundCommandError::Failed(err.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_alert_sounds_are_wav_files() {
        assert_wav(SUCCESS_ALERT_WAV);
        assert_wav(ERROR_ALERT_WAV);
    }

    fn assert_wav(bytes: &[u8]) {
        assert!(bytes.starts_with(b"RIFF"));
        assert_eq!(&bytes[8..12], b"WAVE");
        assert!(bytes.windows(4).any(|chunk| chunk == b"data"));
        assert!(bytes.len() > 44);
    }
}
