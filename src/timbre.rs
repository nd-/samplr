// The fingerprint of a sound's timbre, to find samples that sound alike:
// how its spectrum is shaped (MFCC, the speech and music retrieval
// standard), how bright and how noisy it is, and how it starts and lasts.
// Fed during the waveform's decoding, like the pulse, nothing more is read;
// only running sums are kept, so memory does not depend on the length.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

/// Samples per analysed frame: 46 ms at 44.1 kHz, a bin every 21.5 Hz.
const FRAME: usize = 2048;

/// Frames overlap by half.
const HOP: usize = FRAME / 2;

/// The longest stretch looked at, as for the pulse: the start of a long
/// recording says enough of its timbre.
const MAX_SECONDS: f64 = 64.0;

/// Mel bands, from `LOWEST_HZ` to `HIGHEST_HZ` (or half the sample rate).
const BANDS: usize = 32;
const LOWEST_HZ: f64 = 40.0;
const HIGHEST_HZ: f64 = 16_000.0;

/// MFCC kept, the first (the overall level) left out: a sound played louder
/// has the same timbre.
const COEFFICIENTS: usize = 12;

/// A frame quieter than this (in mean square, -60 dBFS) is silence, which
/// says nothing of the timbre (a sample's tail, the gap in a loop).
const SILENCE: f32 = 1e-6;

/// Values measured on each audible frame: the MFCC, then the spectral
/// centroid (brightness, as log2 of Hz) and the flatness (0 a pure tone, 1
/// white noise).
const PER_FRAME: usize = COEFFICIENTS + 2;
pub const CENTROID: usize = COEFFICIENTS;
pub const FLATNESS: usize = COEFFICIENTS + 1;

/// The fingerprint's length: the mean of each value per frame, then its
/// standard deviation, then the attack and the length.
pub const DIMENSIONS: usize = 2 * PER_FRAME + 2;
/// Where the standard deviations start.
pub const DEVIATIONS: usize = PER_FRAME;
/// Seconds from the start to the loudness's peak, as log2(1 + seconds).
pub const ATTACK: usize = 2 * PER_FRAME;
/// The whole sound's length, as log2(1 + seconds).
pub const LENGTH: usize = 2 * PER_FRAME + 1;

/// A sound's timbre, `DIMENSIONS` numbers. Each is on a scale of its own:
/// the comparison weighs them after the whole library.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fingerprint(pub [f32; DIMENSIONS]);

impl Fingerprint {
    /// As stored in the database: each number as 4 bytes, little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    /// `None` for bytes of another length: a fingerprint of an older layout,
    /// computed again as stale.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != DIMENSIONS * 4 {
            return None;
        }
        let mut values = [0.0; DIMENSIONS];
        for (value, chunk) in values.iter_mut().zip(bytes.chunks_exact(4)) {
            *value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        Some(Fingerprint(values))
    }
}

/// The timbre of a sound, built frame by frame.
pub struct Timbre {
    sample_rate: f64,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// For each mel band, the spectrum bins it sums and their weights.
    filters: Vec<Vec<(usize, f32)>>,
    pending: Vec<f32>,
    frames: usize,
    analysed: usize,
    sums: [f64; PER_FRAME],
    squares: [f64; PER_FRAME],
    audible: usize,
    /// Each frame's mean square, for the attack (at most 64 s of them).
    powers: Vec<f32>,
}

impl Timbre {
    pub fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate);
        // Hann window: the frame's edges fade, so that cutting the sound
        // into frames adds no frequencies of its own.
        let window = (0..FRAME)
            .map(|i| {
                let phase = 2.0 * std::f64::consts::PI * i as f64 / FRAME as f64;
                (0.5 - 0.5 * phase.cos()) as f32
            })
            .collect();
        Timbre {
            sample_rate: rate,
            fft: RealFftPlanner::<f32>::new().plan_fft_forward(FRAME),
            window,
            filters: mel_filters(rate),
            pending: Vec::with_capacity(FRAME),
            frames: 0,
            analysed: 0,
            sums: [0.0; PER_FRAME],
            squares: [0.0; PER_FRAME],
            audible: 0,
            powers: Vec::new(),
        }
    }

    /// One frame of sound, its channels mixed.
    pub fn push(&mut self, sample: f32) {
        self.frames += 1;
        if self.frames as f64 > MAX_SECONDS * self.sample_rate {
            return;
        }
        // A float WAV may hold NaN or infinite values: one would turn every
        // number of the fingerprint into NaN.
        self.pending
            .push(if sample.is_finite() { sample } else { 0.0 });
        if self.pending.len() == FRAME {
            self.analyse();
            self.pending.drain(..HOP);
        }
    }

    /// The fingerprint, or `None` for a silent sound.
    pub fn finish(mut self) -> Option<Fingerprint> {
        // A sound shorter than a frame is analysed padded with silence.
        if self.analysed == 0 && !self.pending.is_empty() {
            self.pending.resize(FRAME, 0.0);
            self.analyse();
        }
        if self.audible == 0 {
            return None;
        }
        let count = self.audible as f64;
        let mut values = [0.0; DIMENSIONS];
        for (i, (sum, square)) in self.sums.iter().zip(self.squares).enumerate() {
            let mean = sum / count;
            values[i] = mean as f32;
            values[DEVIATIONS + i] = (square / count - mean * mean).max(0.0).sqrt() as f32;
        }
        // The attack ends at the first frame within 3 dB of the loudest: the
        // loudest itself may come late in a sustained sound.
        let loudest = self.powers.iter().copied().fold(0.0, f32::max);
        let peak = self
            .powers
            .iter()
            .position(|&p| p >= loudest / 2.0)
            .unwrap_or(0);
        let attack = (peak * HOP) as f64 / self.sample_rate;
        let length = self.frames as f64 / self.sample_rate;
        values[ATTACK] = (1.0 + attack).log2() as f32;
        values[LENGTH] = (1.0 + length).log2() as f32;
        Some(Fingerprint(values))
    }

    fn analyse(&mut self) {
        self.analysed += 1;
        let power = self.pending.iter().map(|s| s * s).sum::<f32>() / FRAME as f32;
        self.powers.push(power);
        if power < SILENCE {
            return;
        }

        let mut input: Vec<f32> = self
            .pending
            .iter()
            .zip(&self.window)
            .map(|(sample, weight)| sample * weight)
            .collect();
        let mut spectrum = self.fft.make_output_vec();
        // Fails only on buffers of the wrong length, which these are not.
        self.fft
            .process(&mut input, &mut spectrum)
            .expect("FFT buffers of the planned length");
        let powers: Vec<f64> = spectrum
            .iter()
            .map(|bin| f64::from(bin.norm_sqr()))
            .collect();

        let mut values = [0.0; PER_FRAME];
        let energies: Vec<f64> = self
            .filters
            .iter()
            .map(|filter| {
                filter
                    .iter()
                    .map(|&(bin, weight)| powers[bin] * f64::from(weight))
                    .sum()
            })
            .collect();
        // Floors relative to the frame's own level, 80 dB below its loudest
        // band (and 120 dB below its mean bin): a fixed floor would weigh more
        // in a quiet sound, and its timbre would change with its level.
        let floor = energies.iter().copied().fold(0.0, f64::max) * 1e-8;
        let bands: Vec<f64> = energies.iter().map(|e| (e + floor).ln()).collect();
        // DCT-II of the bands' log energies, without the first coefficient.
        for (k, value) in values.iter_mut().take(COEFFICIENTS).enumerate() {
            let k = (k + 1) as f64;
            *value = bands
                .iter()
                .enumerate()
                .map(|(n, band)| {
                    band * (std::f64::consts::PI * k * (n as f64 + 0.5) / BANDS as f64).cos()
                })
                .sum::<f64>()
                / BANDS as f64;
        }

        // Bin 0 (the constant offset) left out of both.
        let bin_hz = self.sample_rate / FRAME as f64;
        let total: f64 = powers[1..].iter().sum();
        // A constant offset alone, nothing to measure.
        if total == 0.0 {
            return;
        }
        let centroid = powers[1..]
            .iter()
            .enumerate()
            .map(|(i, power)| (i + 1) as f64 * bin_hz * power)
            .sum::<f64>()
            / total;
        values[CENTROID] = centroid.max(1.0).log2();
        let mean = total / (powers.len() - 1) as f64;
        let log_mean = powers[1..]
            .iter()
            .map(|p| (p + mean * 1e-12).ln())
            .sum::<f64>()
            / (powers.len() - 1) as f64;
        values[FLATNESS] = (log_mean.exp() / mean).min(1.0);

        for (i, value) in values.iter().enumerate() {
            self.sums[i] += value;
            self.squares[i] += value * value;
        }
        self.audible += 1;
    }
}

/// The samples of `library` ranked by how close their timbre is to
/// `target`'s, nearest first, with their distance. Each number is first
/// brought to the library's own scale (its mean and standard deviation over
/// the library), so that none weighs more for its unit: a distance of 1 is a
/// difference of one standard deviation on every number.
pub fn rank<K: Copy>(target: &Fingerprint, library: &[(K, Fingerprint)]) -> Vec<(K, f32)> {
    // A fingerprint holding NaN (measured by an older samplr, see `push`)
    // would make the library's scale NaN, hence every distance 0.
    let library: Vec<&(K, Fingerprint)> = library
        .iter()
        .filter(|(_, fingerprint)| fingerprint.0.iter().all(|value| value.is_finite()))
        .collect();
    let count = library.len().max(1) as f64;
    let mut means = [0.0f64; DIMENSIONS];
    let mut spreads = [0.0f64; DIMENSIONS];
    for (_, fingerprint) in &library {
        for (mean, value) in means.iter_mut().zip(fingerprint.0) {
            *mean += f64::from(value) / count;
        }
    }
    for (_, fingerprint) in &library {
        for ((spread, mean), value) in spreads.iter_mut().zip(means).zip(fingerprint.0) {
            *spread += (f64::from(value) - mean).powi(2) / count;
        }
    }
    // A number the same for the whole library tells no sample apart.
    let scales: Vec<f32> = spreads
        .iter()
        .map(|spread| {
            if *spread > 1e-12 {
                (1.0 / spread.sqrt()) as f32
            } else {
                0.0
            }
        })
        .collect();
    let mut ranked: Vec<(K, f32)> = library
        .iter()
        .map(|(key, fingerprint)| {
            let squares: f32 = target
                .0
                .iter()
                .zip(fingerprint.0)
                .zip(&scales)
                .map(|((a, b), scale)| ((a - b) * scale).powi(2))
                .sum();
            (*key, (squares / DIMENSIONS as f32).sqrt())
        })
        .collect();
    ranked.sort_by(|a, b| a.1.total_cmp(&b.1));
    ranked
}

/// Triangular filters evenly spread on the mel scale, which follows the ear:
/// fine in the bass, coarse in the treble.
fn mel_filters(sample_rate: f64) -> Vec<Vec<(usize, f32)>> {
    let mel = |hz: f64| 2595.0 * (1.0 + hz / 700.0).log10();
    let hz = |mel: f64| 700.0 * (10f64.powf(mel / 2595.0) - 1.0);
    let highest = HIGHEST_HZ.min(sample_rate / 2.0);
    let (low, high) = (mel(LOWEST_HZ), mel(highest));
    let edges: Vec<f64> = (0..BANDS + 2)
        .map(|i| hz(low + (high - low) * i as f64 / (BANDS + 1) as f64))
        .collect();
    let bin_hz = sample_rate / FRAME as f64;
    let bins = FRAME / 2 + 1;
    (0..BANDS)
        .map(|band| {
            let (start, centre, end) = (edges[band], edges[band + 1], edges[band + 2]);
            let mut filter: Vec<(usize, f32)> = (1..bins)
                .filter_map(|bin| {
                    let f = bin as f64 * bin_hz;
                    let weight = if f > start && f <= centre {
                        (f - start) / (centre - start)
                    } else if f > centre && f < end {
                        (end - f) / (end - centre)
                    } else {
                        0.0
                    };
                    (weight > 0.0).then_some((bin, weight as f32))
                })
                .collect();
            // A low band narrower than a bin takes the bin nearest its centre.
            if filter.is_empty() {
                filter.push((((centre / bin_hz).round() as usize).clamp(1, bins - 1), 1.0));
            }
            filter
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44100;

    fn fingerprint(samples: impl IntoIterator<Item = f32>) -> Option<Fingerprint> {
        let mut timbre = Timbre::new(RATE);
        for sample in samples {
            timbre.push(sample);
        }
        timbre.finish()
    }

    fn sine(hz: f64, seconds: f64, level: f32) -> Vec<f32> {
        let frames = (seconds * f64::from(RATE)) as usize;
        (0..frames)
            .map(|i| {
                level * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(RATE)).sin() as f32
            })
            .collect()
    }

    /// White noise, the same at each run.
    fn noise(seconds: f64, level: f32) -> Vec<f32> {
        let mut state: u32 = 12345;
        (0..(seconds * f64::from(RATE)) as usize)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                level * (state as f32 / u32::MAX as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    fn distance(a: &Fingerprint, b: &Fingerprint) -> f32 {
        a.0.iter()
            .zip(&b.0)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            .sqrt()
    }

    #[test]
    fn two_close_tones_are_nearer_than_a_tone_and_a_noise() {
        let a = fingerprint(sine(440.0, 1.0, 0.5)).unwrap();
        let b = fingerprint(sine(466.0, 1.0, 0.5)).unwrap();
        let hiss = fingerprint(noise(1.0, 0.5)).unwrap();
        assert_eq!(a.0.len(), DIMENSIONS);
        assert!(distance(&a, &b) * 5.0 < distance(&a, &hiss));
    }

    #[test]
    fn a_noise_is_flat_and_a_tone_is_not() {
        let tone = fingerprint(sine(440.0, 1.0, 0.5)).unwrap();
        let hiss = fingerprint(noise(1.0, 0.5)).unwrap();
        assert!(tone.0[FLATNESS] < 0.1, "tone: {}", tone.0[FLATNESS]);
        assert!(hiss.0[FLATNESS] > 0.5, "noise: {}", hiss.0[FLATNESS]);
    }

    #[test]
    fn a_higher_tone_is_brighter() {
        let low = fingerprint(sine(200.0, 1.0, 0.5)).unwrap();
        let high = fingerprint(sine(3200.0, 1.0, 0.5)).unwrap();
        // Four octaves apart: about 4 on a log2 scale.
        let octaves = high.0[CENTROID] - low.0[CENTROID];
        assert!((octaves - 4.0).abs() < 0.3, "{octaves}");
    }

    #[test]
    fn the_level_does_not_change_the_timbre() {
        let loud = fingerprint(sine(440.0, 1.0, 0.5)).unwrap();
        let quiet = fingerprint(sine(440.0, 1.0, 0.05)).unwrap();
        assert!(
            distance(&loud, &quiet) < 1e-3,
            "{}",
            distance(&loud, &quiet)
        );
    }

    #[test]
    fn the_attack_and_the_length_are_measured() {
        // Half a second of silence, then a second of tone.
        let mut late = vec![0.0; RATE as usize / 2];
        late.extend(sine(440.0, 1.0, 0.5));
        let late = fingerprint(late).unwrap();
        let attack = 2f32.powf(late.0[ATTACK]) - 1.0;
        let length = 2f32.powf(late.0[LENGTH]) - 1.0;
        assert!((attack - 0.5).abs() < 0.05, "{attack}");
        assert!((length - 1.5).abs() < 0.01, "{length}");
    }

    #[test]
    fn a_sound_shorter_than_a_frame_has_a_fingerprint() {
        // 20 ms, less than half a frame.
        let short = fingerprint(sine(440.0, 0.02, 0.5)).unwrap();
        assert!(short.0.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn the_nearest_come_first_whatever_the_units() {
        let low = fingerprint(sine(200.0, 1.0, 0.5)).unwrap();
        let near = fingerprint(sine(220.0, 1.0, 0.5)).unwrap();
        let high = fingerprint(sine(3000.0, 1.0, 0.5)).unwrap();
        let hiss = fingerprint(noise(1.0, 0.5)).unwrap();
        let library = [("hiss", hiss), ("high", high), ("near", near), ("low", low)];
        let ranked = rank(&low, &library);
        let order: Vec<&str> = ranked.iter().map(|(key, _)| *key).collect();
        assert_eq!(order[..2], ["low", "near"]);
        assert_eq!(ranked[0].1, 0.0);
        assert!(ranked.windows(2).all(|pair| pair[0].1 <= pair[1].1));
    }

    #[test]
    fn a_number_the_same_everywhere_is_ignored() {
        let tone = fingerprint(sine(440.0, 1.0, 0.5)).unwrap();
        let mut other = tone;
        other.0[CENTROID] += 1.0;
        // Only the centroid differs, and only between the two.
        let ranked = rank(&tone, &[(1, tone), (2, other)]);
        assert!(ranked.iter().all(|(_, distance)| distance.is_finite()));
        assert!(ranked[1].1 > 0.0);
        assert!(rank(&tone, &[(1, tone)])[0].1 == 0.0);
        assert!(rank::<u8>(&tone, &[]).is_empty());
    }

    #[test]
    fn stored_as_bytes_and_read_back() {
        let tone = fingerprint(sine(440.0, 1.0, 0.5)).unwrap();
        let bytes = tone.to_bytes();
        assert_eq!(bytes.len(), DIMENSIONS * 4);
        assert_eq!(Fingerprint::from_bytes(&bytes), Some(tone));
        assert_eq!(Fingerprint::from_bytes(&bytes[4..]), None);
    }

    #[test]
    fn silence_has_no_fingerprint() {
        assert_eq!(fingerprint(vec![0.0; RATE as usize]), None);
        assert_eq!(fingerprint(Vec::new()), None);
    }

    #[test]
    fn only_the_first_64_seconds_are_analysed() {
        let mut timbre = Timbre::new(8000);
        for sample in noise(70.0 * 8000.0 / f64::from(RATE), 0.5) {
            timbre.push(sample);
        }
        // 64 s at 8 kHz, in frames overlapping by half.
        assert_eq!(timbre.analysed, (64 * 8000 - FRAME) / HOP + 1);
        let length = 2f32.powf(timbre.finish().unwrap().0[LENGTH]) - 1.0;
        assert!((length - 70.0).abs() < 0.01, "{length}");
    }

    fn finite(fingerprint: &Fingerprint) -> bool {
        fingerprint.0.iter().all(|value| value.is_finite())
    }

    #[test]
    fn sounds_around_one_frame_long() {
        // A single frame falls where the window is zero: nothing to measure.
        if let Some(one) = fingerprint([0.5]) {
            assert!(finite(&one), "{one:?}");
        }
        assert!(finite(&fingerprint(vec![0.5; 44]).unwrap()));
        assert_eq!(fingerprint([0.0]), None);
        // Under the silence threshold once padded to a frame.
        assert_eq!(fingerprint([0.001]), None);
        for frames in [FRAME - 1, FRAME, FRAME + 1, FRAME + HOP] {
            let sound = fingerprint(noise(frames as f64 / f64::from(RATE), 0.5)).unwrap();
            assert!(finite(&sound), "{frames}: {sound:?}");
        }
    }

    #[test]
    fn a_constant_offset_is_measured_without_nan() {
        for level in [0.5, -1.0, 1e-3] {
            if let Some(dc) = fingerprint(vec![level; RATE as usize]) {
                assert!(finite(&dc), "{level}: {dc:?}");
            }
        }
    }

    #[test]
    fn odd_sample_rates_do_not_panic() {
        for rate in [0, 1, 100, 8000, 192_000, u32::MAX] {
            let mut timbre = Timbre::new(rate);
            for sample in noise(0.1, 0.5) {
                timbre.push(sample);
            }
            if let Some(fingerprint) = timbre.finish() {
                assert!(finite(&fingerprint), "{rate}: {fingerprint:?}");
            }
        }
    }

    #[test]
    fn full_scale_and_clipped_sounds_are_finite() {
        let square = (0..RATE).map(|i| if (i / 50) % 2 == 0 { 1.0 } else { -1.0 });
        assert!(finite(&fingerprint(square).unwrap()));
        assert!(finite(&fingerprint(noise(1.0, 1000.0)).unwrap()));
    }

    #[test]
    fn a_non_finite_sample_does_not_poison_the_fingerprint() {
        // Expected: a NaN or infinite sample (a float WAV may hold one) is
        // skipped or zeroed, so that the fingerprint stays finite.
        for bad in [f32::NAN, f32::INFINITY] {
            let mut sound = noise(1.0, 0.5);
            sound[10_000] = bad;
            let fingerprint = fingerprint(sound).unwrap();
            assert!(finite(&fingerprint), "{bad}: {fingerprint:?}");
        }
    }

    /// Fingerprints spread around, the same at each run.
    fn random_fingerprints(count: usize, seed: u32) -> Vec<(usize, Fingerprint)> {
        let mut state = seed;
        (0..count)
            .map(|key| {
                let mut values = [0.0; DIMENSIONS];
                for value in values.iter_mut() {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    *value = (state as f32 / u32::MAX as f32 - 0.5) * 20.0;
                }
                (key, Fingerprint(values))
            })
            .collect()
    }

    fn assert_well_ranked(target: &Fingerprint, library: &[(usize, Fingerprint)]) {
        let ranked = rank(target, library);
        assert_eq!(ranked, rank(target, library));
        assert_eq!(ranked.len(), library.len());
        assert!(
            ranked
                .iter()
                .all(|(_, distance)| distance.is_finite() && *distance >= 0.0),
            "{ranked:?}"
        );
        assert!(ranked.windows(2).all(|pair| pair[0].1 <= pair[1].1));
        let mut keys: Vec<usize> = ranked.iter().map(|(key, _)| *key).collect();
        keys.sort();
        let mut expected: Vec<usize> = library.iter().map(|(key, _)| *key).collect();
        expected.sort();
        assert_eq!(keys, expected);
    }

    #[test]
    fn rank_is_sorted_finite_and_the_same_at_each_call() {
        for seed in 0..20 {
            let library = random_fingerprints(50, seed);
            for (_, target) in &library {
                assert_well_ranked(target, &library);
            }
            // A target from outside the library.
            let outside = random_fingerprints(1, seed + 1000)[0].1;
            assert_well_ranked(&outside, &library);
        }
    }

    #[test]
    fn rank_of_a_lone_or_repeated_fingerprint() {
        let one = random_fingerprints(1, 3)[0].1;
        assert_eq!(rank(&one, &[(0, one)]), [(0, 0.0)]);
        // Every number the same over the library: nothing tells them apart.
        let same: Vec<(usize, Fingerprint)> = (0..5).map(|key| (key, one)).collect();
        let ranked = rank(&one, &same);
        assert_well_ranked(&one, &same);
        assert!(ranked.iter().all(|(_, distance)| *distance == 0.0));
        // Ties keep the library's order.
        let keys: Vec<usize> = ranked.iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, [0, 1, 2, 3, 4]);
        let other = random_fingerprints(1, 4)[0].1;
        assert_well_ranked(&other, &same);
        assert!(rank(&other, &same).iter().all(|(_, d)| *d == 0.0));
    }

    #[test]
    fn rank_with_extreme_finite_numbers() {
        let mut low = Fingerprint([0.0; DIMENSIONS]);
        let mut high = Fingerprint([0.0; DIMENSIONS]);
        low.0[CENTROID] = -f32::MAX;
        high.0[CENTROID] = f32::MAX;
        let mut tiny = Fingerprint([0.0; DIMENSIONS]);
        tiny.0[FLATNESS] = f32::MIN_POSITIVE;
        let library = [(0, low), (1, high), (2, tiny)];
        for (_, target) in &library {
            let ranked = rank(target, &library);
            assert!(ranked.iter().all(|(_, d)| !d.is_nan()), "{ranked:?}");
            assert!(ranked.windows(2).all(|pair| pair[0].1 <= pair[1].1));
        }
    }

    #[test]
    fn a_nan_fingerprint_does_not_blind_the_others() {
        // Expected: a fingerprint holding NaN (see above) is left out of the
        // library's scale, so the others are still told apart.
        let mut library = random_fingerprints(10, 5);
        library.push((10, Fingerprint([f32::NAN; DIMENSIONS])));
        let ranked = rank(&library[0].1, &library);
        let others: Vec<f32> = ranked
            .iter()
            .filter(|(key, _)| *key != 0 && *key != 10)
            .map(|(_, distance)| *distance)
            .collect();
        assert!(others.iter().all(|d| *d > 0.0), "{ranked:?}");
    }

    #[test]
    fn bytes_of_odd_lengths() {
        assert_eq!(Fingerprint::from_bytes(&[]), None);
        assert_eq!(Fingerprint::from_bytes(&[0; DIMENSIONS * 4 + 1]), None);
        assert_eq!(Fingerprint::from_bytes(&[0; DIMENSIONS * 4 - 1]), None);
        let zero = Fingerprint::from_bytes(&[0; DIMENSIONS * 4]).unwrap();
        assert_eq!(zero, Fingerprint([0.0; DIMENSIONS]));
    }
}
