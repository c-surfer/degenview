//! Short system sounds for events the user should hear, played through what the platform already
//! has: no audio crate, no output stream held open.
//!
//! [`SystemSounder::play`] and [`play_alert_sound`] return at once; the sound is started off the
//! calling thread (or asynchronously by the system), so a frame never waits on an audio device. A
//! platform with no way to play one, or a sound that fails to start, is silent: a missing chime is
//! never an error the user has to read.

use crate::model::alert::{AlertSound, AlertSoundRepeat};

const LITE_HAMMER_WAV: &[u8] = include_bytes!("../../../assets/sounds/lite_hammer.wav");
const PIANO_WAV: &[u8] = include_bytes!("../../../assets/sounds/piano.wav");
const YOUR_SUFFER_WAV: &[u8] = include_bytes!("../../../assets/sounds/your_suffer.wav");

pub fn alert_sound_bytes(sound: AlertSound) -> &'static [u8] {
    match sound {
        AlertSound::LiteHammer => LITE_HAMMER_WAV,
        AlertSound::Piano => PIANO_WAV,
        AlertSound::YourSuffer => YOUR_SUFFER_WAV,
    }
}

/// Plays a price-alert sound `repeat.count()` times sequentially in a background thread.
pub fn play_alert_sound(sound: AlertSound, repeat: AlertSoundRepeat) {
    platform::play_alert(sound, repeat.count());
}

const REPEAT_GAP: std::time::Duration = std::time::Duration::from_millis(180);

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct TempWav(std::path::PathBuf);

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl TempWav {
    fn write(bytes: &[u8]) -> Option<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("degenview-alert-{}-{seq}.wav", std::process::id()));
        std::fs::write(&path, bytes).ok()?;
        Some(Self(path))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// What happened, which decides the sound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sound {
    /// A paper order was filled.
    OrderFilled,
}

/// Plays sounds. A trait so the code deciding when to play can be tested without an audio device.
pub trait Sounder: Send + Sync {
    fn play(&self, sound: Sound);
}

/// The platform's own sound facility.
pub struct SystemSounder;

impl Sounder for SystemSounder {
    fn play(&self, sound: Sound) {
        platform::play(sound);
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Sound;
    use objc2_foundation::ns_string;

    fn name(sound: Sound) -> &'static objc2_foundation::NSString {
        match sound {
            Sound::OrderFilled => ns_string!("Glass"),
        }
    }

    pub fn play(sound: Sound) {
        // `NSSound` keeps a named sound cached; playing starts it and returns.
        std::thread::spawn(move || {
            if let Some(sound) = objc2_app_kit::NSSound::soundNamed(name(sound)) {
                // A sound still playing from the last fill restarts rather than being ignored.
                sound.stop();
                sound.play();
            }
        });
    }

    pub fn play_alert(sound: super::AlertSound, count: u8) {
        let bytes = super::alert_sound_bytes(sound);
        std::thread::spawn(move || {
            let Some(temp) = super::TempWav::write(bytes) else {
                return;
            };
            for i in 0..count {
                if let Ok(mut child) = std::process::Command::new("afplay").arg(&temp.0).spawn() {
                    let _ = child.wait();
                }
                if i + 1 < count {
                    std::thread::sleep(super::REPEAT_GAP);
                }
            }
        });
    }
}

#[cfg(windows)]
mod platform {
    use super::{AlertSound, Sound};

    /// A name from the user's Sound control panel, so it follows their scheme.
    fn alias(sound: Sound) -> &'static str {
        match sound {
            Sound::OrderFilled => "SystemNotification",
        }
    }

    pub fn play(sound: Sound) {
        use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
        let alias: Vec<u16> = alias(sound).encode_utf16().chain(Some(0)).collect();
        // SAFETY: `alias` is a NUL-terminated UTF-16 string that outlives the call (SND_ASYNC
        // copies an alias before returning); no module handle is needed for an alias.
        unsafe {
            PlaySoundW(
                alias.as_ptr(),
                std::ptr::null_mut(),
                SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }

    pub fn play_alert(sound: AlertSound, count: u8) {
        use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};
        let bytes = super::alert_sound_bytes(sound);
        std::thread::spawn(move || {
            for i in 0..count {
                // SAFETY: `bytes` is `'static` WAV data in memory and outlives the synchronous call.
                unsafe {
                    PlaySoundW(
                        bytes.as_ptr().cast(),
                        std::ptr::null_mut(),
                        SND_MEMORY | SND_SYNC | SND_NODEFAULT,
                    );
                }
                if i + 1 < count {
                    std::thread::sleep(super::REPEAT_GAP);
                }
            }
        });
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{AlertSound, Sound};
    use std::path::Path;
    use std::process::{Command, Stdio};

    /// An id from the freedesktop sound theme.
    fn theme_id(sound: Sound) -> &'static str {
        match sound {
            Sound::OrderFilled => "message",
        }
    }

    /// Asks `canberra-gtk-play` (present on most desktops) and stays silent where it is not, as in
    /// a Flatpak sandbox without it.
    pub fn play(sound: Sound) {
        std::thread::spawn(move || {
            let child = Command::new("canberra-gtk-play")
                .args(["-i", theme_id(sound)])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if let Ok(mut child) = child {
                // Reaped here so the finished player does not linger as a zombie.
                let _ = child.wait();
            }
        });
    }

    fn run_player(program: &str, args: &[&str], path: &Path) -> bool {
        let Ok(mut child) = Command::new(program)
            .args(args)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let _ = child.wait();
        true
    }

    pub fn play_alert(sound: AlertSound, count: u8) {
        let bytes = super::alert_sound_bytes(sound);
        std::thread::spawn(move || {
            let Some(temp) = super::TempWav::write(bytes) else {
                return;
            };
            for i in 0..count {
                let played = run_player("pw-play", &[], &temp.0)
                    || run_player("paplay", &[], &temp.0)
                    || run_player("aplay", &["-q"], &temp.0);
                if !played {
                    break;
                }
                if i + 1 < count {
                    std::thread::sleep(super::REPEAT_GAP);
                }
            }
        });
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod platform {
    use super::{AlertSound, Sound};

    pub fn play(_: Sound) {}
    pub fn play_alert(_: AlertSound, _: u8) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_sound_assets_are_valid_wav() {
        for sound in AlertSound::ALL {
            let bytes = alert_sound_bytes(sound);
            assert!(bytes.len() > 44, "{sound:?} wav too small");
            assert_eq!(&bytes[..4], b"RIFF", "{sound:?} missing RIFF header");
            assert_eq!(&bytes[8..12], b"WAVE", "{sound:?} missing WAVE header");
        }
        assert_eq!(AlertSoundRepeat::Once.count(), 1);
        assert_eq!(AlertSoundRepeat::Twice.count(), 2);
        assert_eq!(AlertSoundRepeat::ThreeTimes.count(), 3);
        assert_eq!(AlertSoundRepeat::FiveTimes.count(), 5);
    }
}
