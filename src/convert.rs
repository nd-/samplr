// Converting a sample to another format, next to the original, which is
// kept. The encoding is done by ffmpeg, run as a separate program: every
// format, and no sound held in samplr's memory, however long the file.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// What the conversion window asks for.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    /// "wav", "aiff", "flac", "mp3" or "ogg".
    pub format: String,
    /// WAV, AIFF, FLAC: 16 or 24 bits; `None` keeps the source's depth.
    pub bits: Option<u32>,
    /// MP3: constant bit rate, in kbit/s.
    pub mp3_bitrate: u32,
    /// OGG Vorbis: quality, from 0 to 10.
    pub ogg_quality: u32,
    /// Mixes every channel down to one.
    pub mono: bool,
}

/// The ffmpeg program: `SAMPLR_FFMPEG` if set, otherwise the first `ffmpeg`
/// found in the `PATH`. `None` when there is none.
pub fn ffmpeg() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SAMPLR_FFMPEG") {
        return Some(PathBuf::from(path));
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|folder| folder.join(name))
        .find(|path| path.is_file())
}

/// Writes `source` converted after `options`, next to it, and returns the
/// new path: same name, the new extension, " mono" added when mixed down.
/// Never overwrites a file. `source_bits`: the source's bit depth when known
/// (none for MP3 or OGG), for "keep the depth".
pub fn convert(
    source: &Path,
    options: &Options,
    source_bits: Option<u32>,
) -> Result<PathBuf, Box<dyn Error>> {
    let ffmpeg = ffmpeg().ok_or("ffmpeg was not found: install it to convert samples")?;
    let (codec_args, muxer) = codec(options, source_bits)?;

    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("file name with invalid characters")?;
    let mono = if options.mono { " mono" } else { "" };
    let target = source.with_file_name(format!("{stem}{mono}.{}", options.format));
    if target == source {
        return Err(format!("{} is already in that format", source.display()).into());
    }
    if target.exists() {
        return Err(format!("{} already exists", target.display()).into());
    }

    // Written under a name the folder watcher ignores (not an audio
    // extension), then renamed: the library never sees a half-written file.
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("converted");
    let partial = target.with_file_name(format!(".{file_name}.part"));

    let mut command = Command::new(ffmpeg);
    command
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-n", "-i"])
        .arg(source)
        // Sound only (no cover art), keeping the tags written in the file.
        .args(["-vn", "-map_metadata", "0"]);
    if options.mono {
        command.args(["-ac", "1"]);
    }
    command.args(&codec_args).args(["-f", muxer]).arg(&partial);
    let output = command.output()?;
    if !output.status.success() {
        let _ = fs::remove_file(&partial);
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg failed: {}", message.trim()).into());
    }
    fs::rename(&partial, &target)?;
    Ok(target)
}

/// ffmpeg's codec arguments and output format for `options`.
fn codec(
    options: &Options,
    source_bits: Option<u32>,
) -> Result<(Vec<String>, &'static str), Box<dyn Error>> {
    // Keeping the depth: 24 bits for anything above 16 (FLAC stops at 24),
    // 16 for a lossy source, which has none.
    let bits = options.bits.unwrap_or(match source_bits {
        Some(b) if b > 16 => 24,
        _ => 16,
    });
    if bits != 16 && bits != 24 {
        return Err(format!("{bits} bits is not offered: 16 or 24").into());
    }
    let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    Ok(match options.format.as_str() {
        "wav" => (
            args(&["-c:a", if bits == 24 { "pcm_s24le" } else { "pcm_s16le" }]),
            "wav",
        ),
        "aiff" => (
            args(&["-c:a", if bits == 24 { "pcm_s24be" } else { "pcm_s16be" }]),
            "aiff",
        ),
        "flac" => {
            // ffmpeg's FLAC encoder takes 16 or 32-bit samples; a 24-bit
            // stream is 32-bit samples of which 24 are used.
            let format = if bits == 24 { "s32" } else { "s16" };
            let mut a = args(&["-c:a", "flac", "-sample_fmt", format]);
            if bits == 24 {
                a.extend(args(&["-bits_per_raw_sample", "24"]));
            }
            (a, "flac")
        }
        "mp3" => {
            if !(32..=320).contains(&options.mp3_bitrate) {
                return Err(
                    format!("{} kbit/s is not an MP3 bit rate", options.mp3_bitrate).into(),
                );
            }
            let rate = format!("{}k", options.mp3_bitrate);
            (args(&["-c:a", "libmp3lame", "-b:a", &rate]), "mp3")
        }
        "ogg" => {
            if options.ogg_quality > 10 {
                return Err("OGG quality goes from 0 to 10".into());
            }
            let quality = options.ogg_quality.to_string();
            (args(&["-c:a", "libvorbis", "-q:a", &quality]), "ogg")
        }
        other => return Err(format!("“{other}” is not a format samplr converts to").into()),
    })
}
