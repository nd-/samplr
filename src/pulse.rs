// The tempo of a loop from its sound, as a last resort when neither its
// name, its ACID chunk nor a value set by hand gives it. A loop lasts a whole
// number of beats: its length gives a short list of possible tempos (2.5 s:
// 72, 96, 120, 144…), and the one whose pulse best matches the sound's
// attacks wins. Fed during the waveform's decoding, nothing more is read.
//
// Measured on the 1670 loops of the Linux library whose name gives the tempo
// (see CLAUDE.md): right 62.9 % of the time, half or double 15.1 %; the
// tempo from the length alone was right 44.1 % of the times it answered.
// The octave is the hard part: a dub groove labelled 140 is as much a 70,
// and the sound does not say which the pack's author chose.

/// Samples per step of the attack envelope: 5.8 ms at 44.1 kHz.
const HOP: usize = 256;

/// The longest loop looked at: 64 beats at 60 BPM. Longer, no tempo, and the
/// envelope stops growing (a recording of hours would otherwise fill memory).
const MAX_SECONDS: f64 = 64.0;

/// The attack envelope of a sound, built frame by frame: the rises of
/// energy in three bands (below 200 Hz, up to 2 kHz, above).
pub struct Onsets {
    sample_rate: f64,
    low_coefficient: f32,
    mid_coefficient: f32,
    low: f32,
    mid: f32,
    energy: [f32; 3],
    previous: [f32; 3],
    in_hop: usize,
    frames: usize,
    envelope: Vec<f32>,
}

impl Onsets {
    pub fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate);
        let coefficient =
            |cutoff: f64| (1.0 - (-2.0 * std::f64::consts::PI * cutoff / rate).exp()) as f32;
        Onsets {
            sample_rate: rate,
            low_coefficient: coefficient(200.0),
            mid_coefficient: coefficient(2000.0),
            low: 0.0,
            mid: 0.0,
            energy: [0.0; 3],
            previous: [0.0; 3],
            in_hop: 0,
            frames: 0,
            envelope: Vec::new(),
        }
    }

    /// One frame of sound, its channels mixed.
    pub fn push(&mut self, sample: f32) {
        self.frames += 1;
        if self.frames as f64 > MAX_SECONDS * self.sample_rate {
            return;
        }
        self.low += self.low_coefficient * (sample - self.low);
        self.mid += self.mid_coefficient * (sample - self.mid);
        let bands = [self.low, self.mid - self.low, sample - self.mid];
        for (energy, band) in self.energy.iter_mut().zip(bands) {
            *energy += band * band;
        }
        self.in_hop += 1;
        if self.in_hop < HOP {
            return;
        }
        let mut rise = 0.0;
        for (energy, previous) in self.energy.iter_mut().zip(self.previous.iter_mut()) {
            let level = (*energy / HOP as f32 + 1e-10).ln();
            rise += (level - *previous).max(0.0);
            *previous = level;
            *energy = 0.0;
        }
        // The first step has nothing before it: its rise is meaningless.
        self.envelope
            .push(if self.envelope.is_empty() { 0.0 } else { rise });
        self.in_hop = 0;
    }

    /// The loop's tempo, rounded; `None` for a sound too short or too long.
    pub fn tempo(&self) -> Option<u32> {
        let seconds = self.frames as f64 / self.sample_rate;
        if self.envelope.len() < 8 || seconds > MAX_SECONDS {
            return None;
        }
        // What stands above the mean: the attacks.
        let mean = self.envelope.iter().sum::<f32>() / self.envelope.len() as f32;
        let attacks: Vec<f32> = self.envelope.iter().map(|v| (v - mean).max(0.0)).collect();
        // No attack (silence, a steady drone): every tempo would score 0, and
        // the first candidate would win for nothing.
        if attacks.iter().all(|&attack| attack == 0.0) {
            return None;
        }
        let steps_per_second = self.sample_rate / HOP as f64;
        let mut best: Option<(f32, f64)> = None;
        for beats in 1..=64u32 {
            let tempo = 60.0 * f64::from(beats) / seconds;
            if !(60.0..=200.0).contains(&tempo) {
                continue;
            }
            let period = 60.0 / tempo * steps_per_second;
            let pulse = autocorrelation(&attacks, period)
                + 0.1 * on_grid(&attacks, period)
                + 0.5 * autocorrelation(&attacks, period / 2.0);
            let score = pulse * likelihood(beats, tempo);
            if best.is_none_or(|(best_score, _)| score > best_score) {
                best = Some((score, tempo));
            }
        }
        best.map(|(_, tempo)| tempo.round() as u32)
    }
}

/// How usual a loop of `beats` beats at `tempo` is: tempos around 110
/// (within about half an octave), a number of beats in whole bars of four,
/// better a power of two. These choose between octaves, which the sound
/// alone cannot.
fn likelihood(beats: u32, tempo: f64) -> f32 {
    let octaves = (tempo / 110.0).log2();
    let mut likelihood = (-(octaves * octaves) / (2.0 * 0.4 * 0.4)).exp() as f32;
    if beats.is_multiple_of(4) {
        likelihood *= 1.3;
    }
    if beats.is_power_of_two() {
        likelihood *= 1.2;
    }
    likelihood
}

/// The envelope at a fractional step, the loop going round.
fn at(envelope: &[f32], position: f64) -> f32 {
    let length = envelope.len();
    let position = position.rem_euclid(length as f64);
    let index = position.floor() as usize;
    let fraction = (position - position.floor()) as f32;
    envelope[index % length] * (1.0 - fraction) + envelope[(index + 1) % length] * fraction
}

/// How much the attacks repeat after `lag` steps, from 0 to 1, round the
/// loop.
fn autocorrelation(envelope: &[f32], lag: f64) -> f32 {
    let norm: f32 = envelope.iter().map(|v| v * v).sum::<f32>() + 1e-9;
    let sum: f32 = envelope
        .iter()
        .enumerate()
        .map(|(step, value)| value * at(envelope, step as f64 + lag))
        .sum();
    sum / norm
}

/// How strong the attacks are on the beats counted from the loop's start,
/// against their mean.
fn on_grid(envelope: &[f32], period: f64) -> f32 {
    let mean = envelope.iter().sum::<f32>() / envelope.len() as f32 + 1e-9;
    let beats = (envelope.len() as f64 / period).round().max(1.0) as usize;
    let sum: f32 = (0..beats)
        .map(|beat| {
            let centre = beat as f64 * period;
            (-2..=2)
                .map(|offset| at(envelope, centre + f64::from(offset)))
                .fold(0.0, f32::max)
        })
        .sum();
    sum / beats as f32 / mean
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click on every beat, louder on the first of each bar of four, and
    /// a quieter one between beats, as a hi-hat would.
    fn clicks(tempo: f64, beats: u32) -> Onsets {
        let rate = 44_100;
        let mut onsets = Onsets::new(rate);
        let beat = 60.0 / tempo * f64::from(rate);
        let length = (beat * f64::from(beats)) as usize;
        for frame in 0..length {
            let in_beat = frame as f64 % beat;
            let in_half = frame as f64 % (beat / 2.0);
            let index = (frame as f64 / beat) as u32;
            let sample = if in_beat < 400.0 {
                let level = if index.is_multiple_of(4) { 0.9 } else { 0.6 };
                level * ((frame as f32) * 0.3).sin()
            } else if in_half < 200.0 {
                0.2 * ((frame as f32) * 1.7).sin()
            } else {
                0.0
            };
            onsets.push(sample);
        }
        onsets
    }

    #[test]
    fn finds_the_tempo() {
        assert_eq!(clicks(120.0, 8).tempo(), Some(120));
        assert_eq!(clicks(95.0, 8).tempo(), Some(95));
        assert_eq!(clicks(140.0, 16).tempo(), Some(140));
    }

    #[test]
    fn silence_has_no_tempo() {
        let mut onsets = Onsets::new(44_100);
        for _ in 0..105_840 {
            onsets.push(0.0);
        }
        assert_eq!(onsets.tempo(), None);
    }

    #[test]
    fn too_short_or_too_long() {
        let mut short = Onsets::new(44_100);
        for _ in 0..1000 {
            short.push(0.5);
        }
        assert_eq!(short.tempo(), None);
        let mut long = Onsets::new(1000);
        for _ in 0..70_000 {
            long.push(0.0);
        }
        assert_eq!(long.tempo(), None);
    }
}
