// Waveforms: a summary of the sound, small enough to be stored in the
// database and drawn without decoding the file again.

use std::error::Error;
use std::fs::File;
use std::path::Path;

use rodio::Source;

/// Number of points kept per waveform: about one per pixel of a wide
/// display. Each point holds the lowest and highest value of the sound over
/// its share of the file, which is what a waveform drawing needs.
pub const POINTS: usize = 1024;

/// The lowest and highest sample value (between -1 and 1) over a stretch of sound.
pub type Peak = (f32, f32);

/// What is measured on the sound with its waveform.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Measures {
    /// Integrated loudness (EBU R128), in LUFS; `None` for a sound too short
    /// (under 0.4 s) or too quiet to be measured.
    pub lufs: Option<f64>,
    /// Highest sample, in dBFS (0 is full scale); `None` for silence.
    pub peak_db: Option<f64>,
    /// The tempo it would have as a loop (see `pulse`); whether it is one is
    /// decided elsewhere.
    pub tempo: Option<u32>,
    /// To find the samples that sound alike; `None` for silence.
    pub timbre: Option<crate::timbre::Fingerprint>,
}

/// Decodes the whole file and summarises it into at most `POINTS` peaks,
/// measuring its loudness, its pulse and its timbre on the way (the same
/// decoding serves them all).
/// Every channel goes into the same peaks: the drawing shows the loudest
/// channel at each moment.
pub fn compute(path: &Path) -> Result<(Vec<Peak>, Measures), Box<dyn Error>> {
    let decoder = rodio::Decoder::try_from(File::open(path)?)?;
    // `get()` turns the `NonZero<u16>` returned by rodio into a plain number.
    let channels = decoder.channels().get() as usize;
    let mut meter = ebur128::EbuR128::new(
        channels as u32,
        decoder.sample_rate().get(),
        ebur128::Mode::I | ebur128::Mode::SAMPLE_PEAK,
    )?;
    // Fed by blocks of frames rather than sample by sample.
    let mut block: Vec<f32> = Vec::with_capacity(4096 * channels);
    let mut onsets = crate::pulse::Onsets::new(decoder.sample_rate().get());
    let mut timbre = crate::timbre::Timbre::new(decoder.sample_rate().get());
    let mut frame_sum = 0.0;

    // The file length is not always known in advance (some MP3s), so the
    // peaks are built on the fly: each bucket starts by covering a single
    // frame (one sample per channel), and every time the list reaches twice
    // `POINTS`, neighbouring buckets are merged two by two and each bucket
    // covers twice as many frames. Memory use stays the same whatever the
    // file length.
    let mut buckets: Vec<Peak> = Vec::with_capacity(2 * POINTS);
    let mut frames_per_bucket = 1;
    let mut frames_in_current = 0;

    for (index, sample) in decoder.enumerate() {
        block.push(sample);
        if block.len() == block.capacity() {
            meter.add_frames_f32(&block)?;
            block.clear();
        }
        // A new bucket opens on the first sample of its first frame.
        if index % channels == 0 && frames_in_current == 0 {
            buckets.push((sample, sample));
        }
        // `last_mut` lends the last bucket for modification. It always
        // exists here, since one was pushed on the very first sample.
        if let Some((low, high)) = buckets.last_mut() {
            *low = low.min(sample);
            *high = high.max(sample);
        }

        frame_sum += sample;
        // End of a frame: count it, and close the bucket once it is full.
        if index % channels == channels - 1 {
            onsets.push(frame_sum / channels as f32);
            timbre.push(frame_sum / channels as f32);
            frame_sum = 0.0;
            frames_in_current += 1;
            if frames_in_current == frames_per_bucket {
                frames_in_current = 0;
                if buckets.len() == 2 * POINTS {
                    buckets = merge_pairs(&buckets);
                    frames_per_bucket *= 2;
                }
            }
        }
    }

    if buckets.is_empty() {
        return Err("no sound decoded".into());
    }
    // A last, incomplete frame (one sample per channel) is left out.
    block.truncate(block.len() - block.len() % channels);
    meter.add_frames_f32(&block)?;
    let (lufs, peak_db) = loudness(&meter, channels);
    let measures = Measures {
        lufs,
        peak_db,
        tempo: onsets.tempo(),
        timbre: timbre.finish(),
    };
    Ok((resample(&buckets, POINTS), measures))
}

/// Integrated loudness in LUFS and sample peak in dBFS.
fn loudness(meter: &ebur128::EbuR128, channels: usize) -> (Option<f64>, Option<f64>) {
    // Too short or too quiet, the measure is -inf: no value rather than a
    // number that means nothing.
    let lufs = meter.loudness_global().ok().filter(|lufs| lufs.is_finite());
    let peak = (0..channels as u32)
        .filter_map(|channel| meter.sample_peak(channel).ok())
        .fold(0.0, f64::max);
    let peak_db = (peak > 0.0).then(|| 20.0 * peak.log10());
    (lufs, peak_db)
}

/// Merges neighbouring peaks two by two: half as many, each covering twice as long.
fn merge_pairs(peaks: &[Peak]) -> Vec<Peak> {
    peaks.chunks(2).map(merge).collect()
}

/// The single peak covering all the given peaks.
fn merge(peaks: &[Peak]) -> Peak {
    peaks
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), &(l, h)| {
            (low.min(l), high.max(h))
        })
}

/// Brings `peaks` down to exactly `count` peaks, each covering an equal
/// share. Fewer peaks than `count` (a file shorter than `count` frames) are
/// returned as they are.
pub fn resample(peaks: &[Peak], count: usize) -> Vec<Peak> {
    if peaks.len() <= count {
        return peaks.to_vec();
    }
    (0..count)
        .map(|i| merge(&peaks[i * peaks.len() / count..(i + 1) * peaks.len() / count]))
        .collect()
}

/// Peaks stored as bytes, two per peak (low then high), each value mapped from
/// -1..1 to -127..127. A byte is plenty for a drawing, and a 1024-point
/// waveform takes 2 KB in the database.
pub fn to_bytes(peaks: &[Peak]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(peaks.len() * 2);
    for &(low, high) in peaks {
        bytes.push(to_byte(low));
        bytes.push(to_byte(high));
    }
    bytes
}

pub fn from_bytes(bytes: &[u8]) -> Vec<Peak> {
    bytes
        .chunks_exact(2)
        .map(|pair| (from_byte(pair[0]), from_byte(pair[1])))
        .collect()
}

// `as i8` reads the same 8 bits as a signed number: the byte 255 becomes -1.
// That is how a value between -127 and 127 travels as a byte.

fn to_byte(value: f32) -> u8 {
    (value.clamp(-1.0, 1.0) * 127.0).round() as i8 as u8
}

fn from_byte(byte: u8) -> f32 {
    byte as i8 as f32 / 127.0
}

/// Draws the waveform in the terminal: `width` columns, `rows` lines. Each
/// column is a bar as high as the loudest value of its stretch, built from
/// the block characters ▁▂▃▄▅▆▇█ which give eight heights per line.
pub fn draw(peaks: &[Peak], width: usize, rows: usize) -> String {
    const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    let columns = resample(peaks, width);
    // Bar height of each column, in eighths of a line.
    let heights: Vec<usize> = columns
        .iter()
        .map(|&(low, high)| {
            let loudest = low.abs().max(high.abs()).min(1.0);
            (loudest * (rows * 8) as f32).round() as usize
        })
        .collect();

    let mut drawing = String::new();
    // Lines are written from the top: the line `row` (0 at the bottom) is
    // full where the bar goes above it, empty where it stays below, and
    // partly filled in between.
    for row in (0..rows).rev() {
        for &height in &heights {
            let filled = height.saturating_sub(row * 8).min(8);
            drawing.push(BLOCKS[filled]);
        }
        drawing.push('\n');
    }
    drawing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, write_sound};

    /// Half of full scale, as a 16-bit sample: 0.5 once decoded.
    const HALF: i16 = 16_384;

    /// `frames` frames swinging between +HALF and -HALF at every sample:
    /// each stretch of the waveform reaches both.
    fn swing(frames: usize) -> Vec<i16> {
        (0..frames)
            .map(|i| if i % 2 == 0 { HALF } else { -HALF })
            .collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn bytes_give_the_peaks_back() {
        let peaks = [(-1.0, 1.0), (-0.5, 0.5), (0.0, 0.0), (-0.25, 0.75)];
        let bytes = to_bytes(&peaks);
        assert_eq!(bytes.len(), 2 * peaks.len());
        for ((low, high), (l, h)) in from_bytes(&bytes).into_iter().zip(peaks) {
            // A byte keeps the value within 1/254.
            assert!((low - l).abs() <= 1.0 / 254.0 && (high - h).abs() <= 1.0 / 254.0);
        }
    }

    #[test]
    fn values_beyond_full_scale_are_clamped() {
        assert_eq!(from_bytes(&to_bytes(&[(-3.0, 2.0)])), [(-1.0, 1.0)]);
    }

    #[test]
    fn merging_keeps_the_extremes() {
        let peaks = [(-0.1, 0.2), (-0.5, 0.1), (0.0, 0.9)];
        assert_eq!(merge(&peaks), (-0.5, 0.9));
        // An odd number: the last peak is kept alone.
        assert_eq!(merge_pairs(&peaks), [(-0.5, 0.2), (0.0, 0.9)]);
    }

    #[test]
    fn resampling_gives_exactly_the_count_asked() {
        let peaks: Vec<Peak> = (0..3000).map(|i| (-(i as f32) / 3000.0, 0.0)).collect();
        let fewer = resample(&peaks, POINTS);
        assert_eq!(fewer.len(), POINTS);
        assert_eq!(merge(&fewer), merge(&peaks));
        // Fewer peaks than asked: returned as they are.
        assert_eq!(resample(&peaks[..10], POINTS), &peaks[..10]);
    }

    #[test]
    fn drawing_fills_the_bars_to_their_height() {
        let drawing = draw(&[(0.0, 0.0), (-1.0, 1.0), (-0.5, 0.25)], 3, 2);
        assert_eq!(drawing, " █ \n ██\n");
    }

    #[test]
    fn a_long_sound_gives_every_point_its_peaks() {
        let dir = TempDir::new();
        let path = dir.path().join("swing.wav");
        // One second: far more frames than points, merged on the way.
        write_sound(&path, 1, &swing(44_100));
        let (peaks, measures) = compute(&path).unwrap();
        assert_eq!(peaks.len(), POINTS);
        assert!(
            peaks
                .iter()
                .all(|&(low, high)| close(low, -0.5) && close(high, 0.5))
        );
        // Half of full scale: -6 dBFS.
        assert!((measures.peak_db.unwrap() + 6.02).abs() < 0.05);
        assert!(measures.lufs.is_some());
    }

    #[test]
    fn every_channel_goes_into_the_peaks() {
        let dir = TempDir::new();
        let path = dir.path().join("right.wav");
        // Left silent, right swinging.
        let samples: Vec<i16> = swing(44_100).into_iter().flat_map(|s| [0, s]).collect();
        write_sound(&path, 2, &samples);
        let (peaks, _) = compute(&path).unwrap();
        assert!(
            peaks
                .iter()
                .all(|&(low, high)| close(low, -0.5) && close(high, 0.5))
        );
    }

    #[test]
    fn a_very_short_sound_keeps_one_peak_per_frame_and_no_loudness() {
        let dir = TempDir::new();
        let path = dir.path().join("click.wav");
        write_sound(&path, 1, &swing(100));
        let (peaks, measures) = compute(&path).unwrap();
        assert_eq!(peaks.len(), 100);
        // Under 0.4 s, the EBU R128 block: no LUFS, but a peak.
        assert_eq!(measures.lufs, None);
        assert!(measures.peak_db.is_some());
    }

    #[test]
    fn silence_has_no_loudness() {
        let dir = TempDir::new();
        let path = dir.path().join("silence.wav");
        write_sound(&path, 1, &[0; 44_100]);
        let (peaks, measures) = compute(&path).unwrap();
        assert!(peaks.iter().all(|&peak| peak == (0.0, 0.0)));
        assert_eq!((measures.lufs, measures.peak_db), (None, None));
    }

    #[test]
    fn a_file_that_is_not_sound_is_refused() {
        let dir = TempDir::new();
        let path = dir.path().join("text.wav");
        std::fs::write(&path, "not a sound").unwrap();
        assert!(compute(&path).is_err());
    }
}
