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
/// found in the `PATH`, then in the usual install folders. `None` when there
/// is none.
pub fn ffmpeg() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SAMPLR_FFMPEG") {
        return Some(PathBuf::from(path));
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(INSTALL_FOLDERS.iter().map(PathBuf::from))
        .map(|folder| folder.join(name))
        .find(|path| path.is_file())
}

/// Where package managers install ffmpeg, looked at after the `PATH`: an app
/// opened from the Finder or the Dock gets a minimal one
/// (`/usr/bin:/bin:/usr/sbin:/sbin`), without them, unlike one launched from
/// a terminal. Homebrew on Apple silicon, Homebrew on Intel (and manual
/// installs), MacPorts.
const INSTALL_FOLDERS: [&str; 3] = ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"];

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
    let target = target(source, options)?;

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

/// Where the copy of `source` goes: same name, the new extension, " mono"
/// added when mixed down. Refused when a file is already there.
fn target(source: &Path, options: &Options) -> Result<PathBuf, Box<dyn Error>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, write_sound};

    fn options(format: &str) -> Options {
        Options {
            format: format.to_string(),
            bits: None,
            mp3_bitrate: 320,
            ogg_quality: 8,
            mono: false,
        }
    }

    /// The codec arguments as one line, and the output format.
    fn args(options: &Options, source_bits: Option<u32>) -> (String, &'static str) {
        let (args, muxer) = codec(options, source_bits).unwrap();
        (args.join(" "), muxer)
    }

    #[test]
    fn lossless_formats_at_each_depth() {
        let mut wav = options("wav");
        wav.bits = Some(24);
        assert_eq!(args(&wav, None), ("-c:a pcm_s24le".to_string(), "wav"));
        let mut aiff = options("aiff");
        aiff.bits = Some(16);
        assert_eq!(
            args(&aiff, Some(24)),
            ("-c:a pcm_s16be".to_string(), "aiff")
        );
        let mut flac = options("flac");
        flac.bits = Some(24);
        assert_eq!(
            args(&flac, None),
            (
                "-c:a flac -sample_fmt s32 -bits_per_raw_sample 24".to_string(),
                "flac"
            )
        );
        flac.bits = Some(16);
        assert_eq!(args(&flac, None).0, "-c:a flac -sample_fmt s16");
    }

    #[test]
    fn keeping_the_depth() {
        let wav = options("wav");
        assert_eq!(args(&wav, Some(16)).0, "-c:a pcm_s16le");
        assert_eq!(args(&wav, Some(24)).0, "-c:a pcm_s24le");
        // 32 bits (float) goes down to 24, the most FLAC takes.
        assert_eq!(args(&wav, Some(32)).0, "-c:a pcm_s24le");
        // A lossy source has no depth: 16.
        assert_eq!(args(&wav, None).0, "-c:a pcm_s16le");
    }

    #[test]
    fn lossy_formats() {
        let mut mp3 = options("mp3");
        mp3.mp3_bitrate = 192;
        assert_eq!(
            args(&mp3, Some(24)),
            ("-c:a libmp3lame -b:a 192k".to_string(), "mp3")
        );
        let mut ogg = options("ogg");
        ogg.ogg_quality = 0;
        assert_eq!(
            args(&ogg, None),
            ("-c:a libvorbis -q:a 0".to_string(), "ogg")
        );
    }

    #[test]
    fn settings_not_offered_are_refused() {
        let mut wav = options("wav");
        wav.bits = Some(8);
        assert!(codec(&wav, None).is_err());
        let mut mp3 = options("mp3");
        mp3.mp3_bitrate = 31;
        assert!(codec(&mp3, None).is_err());
        mp3.mp3_bitrate = 321;
        assert!(codec(&mp3, None).is_err());
        let mut ogg = options("ogg");
        ogg.ogg_quality = 11;
        assert!(codec(&ogg, None).is_err());
        assert!(codec(&options("wma"), None).is_err());
    }

    #[test]
    fn the_copy_goes_next_to_the_original() {
        let dir = TempDir::new();
        let source = dir.path().join("Kick 01.wav");
        let mut flac = options("flac");
        assert_eq!(
            target(&source, &flac).unwrap(),
            dir.path().join("Kick 01.flac")
        );
        flac.mono = true;
        assert_eq!(
            target(&source, &flac).unwrap(),
            dir.path().join("Kick 01 mono.flac")
        );
        // Mono in the same format is a new file; as is, it would be the source.
        let mut wav = options("wav");
        assert!(target(&source, &wav).is_err());
        wav.mono = true;
        assert_eq!(
            target(&source, &wav).unwrap(),
            dir.path().join("Kick 01 mono.wav")
        );
    }

    #[test]
    fn the_copy_never_overwrites_a_file() {
        let dir = TempDir::new();
        let source = dir.path().join("kick.wav");
        fs::write(dir.path().join("kick.flac"), "already there").unwrap();
        assert!(target(&source, &options("flac")).is_err());
    }

    /// Runs the real ffmpeg; skipped, saying so, where it is not installed.
    #[test]
    fn converts_with_ffmpeg() {
        if ffmpeg().is_none() {
            eprintln!("ffmpeg not found: conversion not tested");
            return;
        }
        let dir = TempDir::new();
        let source = dir.path().join("kick.wav");
        let samples: Vec<i16> = (0..8820).map(|i| ((i % 100) * 300) as i16).collect();
        write_sound(&source, 2, &samples);

        let mut flac = options("flac");
        flac.mono = true;
        let converted = convert(&source, &flac, Some(16)).unwrap();
        assert_eq!(converted, dir.path().join("kick mono.flac"));
        let info = crate::read_info(&converted).unwrap();
        assert_eq!((info.channels, info.bits), (Some(1), Some(16)));
        assert!(source.is_file());
        // No `.part` left behind.
        let files = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(files, 2);
    }
}
