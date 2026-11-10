// Tempo and pitch of the preview, changed independently, by Signalsmith
// Stretch (MIT, C++ compiled with the app through the `ssstretch` crate,
// which needs a C++ compiler only; `signalsmith-stretch` generates its
// bindings with libclang, which needs clang installed): a rodio source wrapped around
// the decoder, reading the settings at each block so that they change while
// the sound plays. The sound is still read as it plays, never held in memory.
//
// A sound starts untouched: the stretcher only comes in once the settings
// leave their neutral values (tempo 1, no transposition), and stays for the
// rest of that sound, since going back would skip the input it holds.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;

/// The settings, shared by the page's commands and the sounds playing.
pub struct Shape {
    /// Tempo factor, as `f32` bits: 2.0 twice as fast, same pitch.
    tempo: AtomicU32,
    /// Transposition in semitones, as `f32` bits, same tempo.
    semitones: AtomicU32,
}

impl Shape {
    pub fn new() -> Arc<Self> {
        Arc::new(Shape {
            tempo: AtomicU32::new(1f32.to_bits()),
            semitones: AtomicU32::new(0f32.to_bits()),
        })
    }

    pub fn set(&self, tempo: f32, semitones: f32) {
        self.tempo.store(tempo.to_bits(), Ordering::Relaxed);
        self.semitones.store(semitones.to_bits(), Ordering::Relaxed);
    }

    fn get(&self) -> (f32, f32) {
        (
            f32::from_bits(self.tempo.load(Ordering::Relaxed)),
            f32::from_bits(self.semitones.load(Ordering::Relaxed)),
        )
    }
}

/// The C++ stretcher, with its buffers: it takes one buffer per channel,
/// rodio gives the channels interleaved.
struct Engine {
    stretch: ssstretch::Stretch,
    channels: usize,
    inputs: Vec<Vec<f32>>,
    outputs: Vec<Vec<f32>>,
}

// SAFETY: the C++ object is owned by the sound and moved with it to the
// audio output's thread; it is only ever used by one thread at a time.
unsafe impl Send for Engine {}

impl Engine {
    fn new(channels: usize, sample_rate: u32) -> Self {
        let mut stretch = ssstretch::Stretch::new();
        stretch.preset_default(channels as i32, sample_rate as f32);
        Engine {
            stretch,
            channels,
            inputs: vec![Vec::new(); channels],
            outputs: vec![Vec::new(); channels],
        }
    }

    fn input_latency(&self) -> usize {
        self.stretch.input_latency() as usize
    }

    fn output_latency(&self) -> usize {
        self.stretch.output_latency() as usize
    }

    fn deinterleave(&mut self, interleaved: &[f32]) -> i32 {
        let frames = interleaved.len() / self.channels;
        for (channel, buffer) in self.inputs.iter_mut().enumerate() {
            buffer.clear();
            buffer.extend(interleaved.iter().skip(channel).step_by(self.channels));
        }
        frames as i32
    }

    fn interleave(&self, interleaved: &mut [f32]) {
        for (i, sample) in interleaved.iter_mut().enumerate() {
            *sample = self.outputs[i % self.channels][i / self.channels];
        }
    }

    fn seek(&mut self, input: &[f32], playback_rate: f64) {
        let frames = self.deinterleave(input);
        self.stretch.seek_vec(&self.inputs, frames, playback_rate);
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let frames = self.deinterleave(input);
        let output_frames = (output.len() / self.channels) as i32;
        self.stretch
            .process_vec(&self.inputs, frames, &mut self.outputs, output_frames);
        self.interleave(output);
    }

    fn flush(&mut self, output: &mut [f32]) {
        let output_frames = (output.len() / self.channels) as i32;
        self.stretch.flush_vec(&mut self.outputs, output_frames);
        self.interleave(output);
    }
}

/// Frames of output computed at a time: about 12 ms at 44.1 kHz, short
/// enough for a change of the settings to be heard at once.
const BLOCK: usize = 512;

pub struct Stretched {
    input: Box<dyn Source + Send>,
    channels: rodio::ChannelCount,
    sample_rate: rodio::SampleRate,
    shape: Arc<Shape>,
    /// Created when first needed, kept for the rest of the sound.
    stretch: Option<Engine>,
    /// Whether the stretcher is on (see the module's comment).
    active: bool,
    /// Samples passed through untouched since the start, to switch on the
    /// stretcher at a frame's boundary only.
    passed: usize,
    semitones: f32,
    /// Fraction of an input frame carried over between blocks.
    carry: f64,
    input_block: Vec<f32>,
    output: Vec<f32>,
    position: usize,
    /// Output frames still to drop after the stretcher starts: its latency,
    /// so that the sound starts on time instead of after a silence.
    skip: usize,
    input_ended: bool,
    flushed: bool,
}

impl Stretched {
    pub fn new(input: Box<dyn Source + Send>, shape: Arc<Shape>) -> Self {
        Stretched {
            channels: input.channels(),
            sample_rate: input.sample_rate(),
            input,
            shape,
            stretch: None,
            active: false,
            passed: 0,
            semitones: 0.0,
            carry: 0.0,
            input_block: Vec::new(),
            output: Vec::new(),
            position: 0,
            skip: 0,
            input_ended: false,
            flushed: false,
        }
    }

    fn channel_count(&self) -> usize {
        usize::from(self.channels.get())
    }

    /// Reads up to `frames` frames of input, zeros past its end.
    fn read_input(&mut self, frames: usize) {
        let wanted = frames * self.channel_count();
        self.input_block.clear();
        while self.input_block.len() < wanted {
            match self.input.next() {
                Some(sample) => self.input_block.push(sample),
                None => {
                    self.input_ended = true;
                    self.input_block.resize(wanted, 0.0);
                }
            }
        }
    }

    /// Starts the stretcher on the input's current place: it is given its
    /// input latency's worth of sound first, and the same latency of output
    /// is dropped, as Signalsmith's own `exact` does.
    fn start(&mut self, tempo: f32) {
        let channels = self.channel_count();
        let sample_rate = self.sample_rate.get();
        let stretch = self
            .stretch
            .get_or_insert_with(|| Engine::new(channels, sample_rate));
        stretch.stretch.reset();
        let latency = stretch.input_latency();
        self.skip = stretch.output_latency();
        self.semitones = f32::NAN;
        self.read_input(latency);
        let stretch = self.stretch.as_mut().expect("created above");
        stretch.seek(&self.input_block, f64::from(tempo));
        self.active = true;
    }

    /// Computes the next block of output; `false` when there is none left.
    fn refill(&mut self, tempo: f32, semitones: f32) -> bool {
        self.output.clear();
        self.position = 0;
        let channels = self.channel_count();
        if self.input_ended {
            if self.flushed {
                return false;
            }
            self.flushed = true;
            let stretch = self.stretch.as_mut().expect("active");
            self.output.resize(stretch.output_latency() * channels, 0.0);
            stretch.flush(&mut self.output);
            return true;
        }
        if semitones != self.semitones {
            self.semitones = semitones;
            self.stretch
                .as_mut()
                .expect("active")
                .stretch
                .set_transpose_semitones(semitones, None);
        }
        let wanted = BLOCK as f64 * f64::from(tempo) + self.carry;
        let frames = wanted.floor() as usize;
        self.carry = wanted - frames as f64;
        self.read_input(frames);
        self.output.resize(BLOCK * channels, 0.0);
        let stretch = self.stretch.as_mut().expect("active");
        stretch.process(&self.input_block, &mut self.output);
        let dropped = self.skip.min(BLOCK);
        self.skip -= dropped;
        self.position = dropped * channels;
        true
    }
}

impl Iterator for Stretched {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        if !self.active {
            let at_frame_start = self.passed % self.channel_count() == 0;
            let (tempo, semitones) = self.shape.get();
            let neutral = tempo == 1.0 && semitones == 0.0;
            if neutral || !at_frame_start {
                self.passed += 1;
                return self.input.next();
            }
            self.start(tempo);
        }
        while self.position >= self.output.len() {
            let (tempo, semitones) = self.shape.get();
            if !self.refill(tempo, semitones) {
                return None;
            }
        }
        let sample = self.output[self.position];
        self.position += 1;
        Some(sample)
    }
}

impl Source for Stretched {
    fn current_span_len(&self) -> Option<usize> {
        // Untouched, the input's spans; stretched, one span per block. rodio
        // reads the sample rate again only at a span's start: a span to the
        // end (`None`) made it ignore every later change of the player's
        // speed (Tape turned on during the sound).
        if !self.active {
            return self.input.current_span_len();
        }
        Some(if self.position < self.output.len() {
            self.output.len() - self.position
        } else {
            BLOCK * self.channel_count()
        })
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        self.input.try_seek(position)?;
        // Started again from the new place at the next sample.
        self.output.clear();
        self.position = 0;
        self.input_ended = false;
        self.flushed = false;
        if self.active {
            let (tempo, _) = self.shape.get();
            self.start(tempo);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    fn sine(seconds: f32) -> Box<dyn Source + Send> {
        let rate = 44_100;
        let samples: Vec<f32> = (0..(seconds * rate as f32) as usize)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5)
            .collect();
        Box::new(SamplesBuffer::new(
            NonZero::new(1).unwrap(),
            NonZero::new(rate).unwrap(),
            samples,
        ))
    }

    #[test]
    fn neutral_is_untouched() {
        let shape = Shape::new();
        let out: Vec<f32> = Stretched::new(sine(0.1), shape).collect();
        let expected: Vec<f32> = sine(0.1).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn tempo_changes_the_length() {
        let shape = Shape::new();
        shape.set(2.0, 0.0);
        let out = Stretched::new(sine(1.0), shape).count();
        // Half a second, give or take a block and the latency flushed.
        assert!((out as f32 / 44_100.0 - 0.5).abs() < 0.1, "{out}");
    }

    #[test]
    fn transposes_without_changing_the_length() {
        let shape = Shape::new();
        shape.set(1.0, 12.0);
        let out: Vec<f32> = Stretched::new(sine(1.0), shape).collect();
        assert!(
            (out.len() as f32 / 44_100.0 - 1.0).abs() < 0.1,
            "{}",
            out.len()
        );
        // An octave up: 880 Hz, counted by the sign changes of the middle
        // half second (two per period).
        let middle = &out[11_025..33_075];
        let changes = middle
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        assert!(
            (changes as f32 - 880.0).abs() < 30.0,
            "{changes} sign changes"
        );
    }

    #[test]
    fn spans_end_with_each_block() {
        let shape = Shape::new();
        shape.set(1.0, 3.0);
        let mut stretched = Stretched::new(sine(1.0), shape);
        stretched.next();
        let span = stretched.current_span_len().expect("a span per block");
        assert!(span > 0 && span <= BLOCK);
    }

    /// Sign changes of a stretch of sound: two per period.
    fn sign_changes(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    }

    #[test]
    fn player_speed_changes_while_stretched() {
        // The case found by hand: the stretcher on, then Tape changes the
        // player's speed during the sound.
        let shape = Shape::new();
        shape.set(1.0, 3.0);
        let (player, queue) = rodio::Player::new();
        player.append(Stretched::new(sine(4.0), Arc::clone(&shape)));
        let mut output = rodio::source::UniformSourceIterator::new(
            queue,
            NonZero::new(1).unwrap(),
            NonZero::new(44_100).unwrap(),
        );
        let before: Vec<f32> = output.by_ref().take(44_100).collect();
        shape.set(1.0, 0.0);
        player.set_speed(2.0);
        // Past the player's 5 ms of polling and a block or two.
        let _: Vec<f32> = output.by_ref().take(4_410).collect();
        let after: Vec<f32> = output.by_ref().take(22_050).collect();
        // 440 Hz up 3 semitones, then 440 Hz twice as fast: 523 then 880 Hz.
        // Half a second each: its sign changes are its frequency.
        let before = sign_changes(&before[22_050..]) as f32;
        let after = sign_changes(&after) as f32;
        assert!((before - 523.0).abs() < 30.0, "before: {before} Hz");
        assert!((after - 880.0).abs() < 40.0, "after: {after} Hz");
    }

    #[test]
    fn starts_at_once() {
        let shape = Shape::new();
        shape.set(1.0, 3.0);
        let out: Vec<f32> = Stretched::new(sine(1.0), shape).collect();
        // No silence before the sound: the latency is dropped.
        let first_loud = out.iter().position(|s| s.abs() > 0.1).unwrap();
        assert!(first_loud < 441, "sound starts after {first_loud} samples");
    }
}
