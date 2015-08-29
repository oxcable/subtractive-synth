//! A polyphonic subtractive synthesizer.
//!
//! This synthesizer uses building blocks from `oxcable` to perform simple but
//! fully featured subtractive synthesis, with polyphony. It is controlled via
//! MIDI and packaged as an `AudioDevice`.
//!
//! Each voice is composed of two seperate oscillators, with ADSR enveloping.
//! The notes are then passed through an adjustable, multimode filter for
//! shaping.
//!
//! The synthesizer additionally supports vibrato and tremolo using an LFO.
//!
//! ## Controlling tone
//!
//! The synthesizer provides three ways to configure its tone:
//!
//! 1. At initialization, using the builder pattern.
//! 2. During runtime, by passing it a `SubtractiveSynthMessage`.
//! 3. With MIDI conrol signals, using a control map (see below).
//!
//! ## MIDI Control Signals
//!
//! In order to allow for tone control using the knobs and sliders present on
//! many MIDI instruments, the synth allows providing a closure to convert these
//! control signals to synthesizer messages. This closure takes the form:
//!
//! ```
//! # /*
//! fn control_map(controller: u8, value: u8) -> Option<SubtractiveSynthMessage>;
//! # */
//! ```
//!
//! The exact values used can vary from device to device, but controller will
//! always specify which knob is being used, and the value will range from 0 to
//! 127.
//!
//! If `Some(msg)` is returned, it will be used to update the synth immediately;
//! if `None` is returned, the MIDI message is ignored.
//!
//! The following example provides ADSR and LFO mappings for the Alesis QX49
//! keyboard:
//!
//! ```
//! use oxcable_subtractive_synth as subsynth;
//! fn qx49_controls(controller: u8, value: u8) -> Option<subsynth::SubtractiveSynthMessage> {
//!     let range = value as f32 / 127.0;
//!     match controller {
//!         22 => Some(subsynth::SetAttack(5.0*range)),
//!         23 => Some(subsynth::SetDecay(5.0*range)),
//!         24 => Some(subsynth::SetSustain(range)),
//!         25 => Some(subsynth::SetRelease(5.0*range)),
//!         26 => Some(subsynth::SetLFOFreq(10.0*range)),
//!         27 => Some(subsynth::SetVibrato(range)),
//!         _ => None
//!     }
//! }
//! ```

extern crate oxcable;

use oxcable::adsr::{self, Adsr};
use oxcable::filters::{first_order, second_order};
use oxcable::oscillator::{self, Oscillator, Waveform};
use oxcable::tremolo::{self, Tremolo};
use oxcable::types::{AudioDevice, MidiDevice, MidiEvent, MidiMessage, Time, Sample};
use oxcable::utils::helpers::{midi_note_to_freq, decibel_to_ratio};
use oxcable::voice_array::VoiceArray;


/// The messages that the synthesizer responds to.
#[derive(Copy, Clone, Debug)]
pub enum SubtractiveSynthMessage {
    /// Set the gain, in decibels
    SetGain(f32),
    /// Set the waveform for the first oscillator
    SetOsc1(Waveform),
    /// Set the waveform for the second oscillator
    SetOsc2(Waveform),
    /// Set the tranposition of the first oscillator
    SetOsc1Transpose(f32),
    /// Set the tranposition of the second oscillator
    SetOsc2Transpose(f32),
    /// Set the ADSR attack, in seconds
    SetAttack(f32),
    /// Set the ADSR decay, in seconds
    SetDecay(f32),
    /// Set the ADSR sustain level, from 0 to 1
    SetSustain(f32),
    /// Set the ADSR release, in seconds
    SetRelease(f32),
    /// Set the LFO frequency, in Hz
    SetLFOFreq(f32),
    /// Set the vibrato intensity, in steps
    SetVibrato(f32),
    /// Set the tremolo intensity, in decibels
    SetTremolo(f32),
    /// Set the filter to a first order filter of the specified mode
    SetFilterFirstOrder(first_order::FilterMode),
    /// Set the filter to a second order filter of the specified mode
    SetFilterSecondOrder(second_order::FilterMode),
}
pub use self::SubtractiveSynthMessage::*;


/// Internally used to track with filter type to use
#[derive(Copy, Clone, Debug)]
enum FilterType { FirstOrder, SecondOrder }

/// A polyphonic subtractive synthesizer.
pub struct SubtractiveSynth<M: MidiDevice> {
    voices: VoiceArray<SubtractiveSynthVoice>,
    controls: Option<Box<Fn(u8, u8) -> Option<Message>>>,
    midi: M,
    gain: f32,

    // audio devices
    lfo: Buffered<Oscillator>,
    filter: FilterType,
    first_filter: Buffered<first_order::Filter>,
    second_filter: Buffered<second_order::Filter>,
    tremolo: Buffered<Tremolo>,
}

impl<M> SubtractiveSynth<M> where M: MidiDevice {
    /// Returns a new subtractive synth that can play `num_voices` notes at one
    /// time.
    pub fn new(midi: M, num_voices: usize) -> Self {
        let mut voices = Vec::with_capacity(num_voices);
        for _i in (0 .. num_voices) {
            voices.push(SubtractiveSynthVoice::new());
        }
        let voice_array = VoiceArray::new(voices);

        SubtractiveSynth {
            voices: voice_array,
            controls: None,
            midi: midi,
            gain: 1.0/num_voices as f32,
            lfo: Buffered::from(Oscillator::new(oscillator::Sine).freq(10.0)),
            filter: FilterType::FirstOrder,
            first_filter: Buffered::from(first_order::Filter::new(
                first_order::LowPass(20000.0), 1)),
            second_filter: Buffered::from(second_order::Filter::new(
                second_order::LowPass(20000.0), 1)),
            tremolo: Buffered::from(Tremolo::new(0.0)),
        }
    }

    /// Set the control signal map to the provided closure, then return the same
    /// synth.
    ///
    /// For further details on control mappings, see the main synth
    /// documentation.
    pub fn control_map<F>(mut self, map: F) -> Self
            where F: 'static+Fn(u8, u8) -> Option<SubtractiveSynthMessage> {
        self.controls = Some(Box::new(map));
        self
    }

    /// Set the gain of the synth in decibels, then return the same synth.
    pub fn gain(mut self, gain: f32) -> Self {
        self.handle_message(SetGain(gain));
        self
    }

    /// Set the waveform of the synth's first oscillator, then return the same
    /// synth.
    pub fn osc1(mut self, waveform: Waveform) -> Self {
        self.handle_message(SetOsc1(waveform));
        self
    }

    /// Set the waveform of the synth's second oscillator, then return the same
    /// synth.
    pub fn osc2(mut self, waveform: Waveform) -> Self {
        self.handle_message(SetOsc2(waveform));
        self
    }

    /// Set the transposition of the synth's first oscillator, then return the
    /// same synth.
    pub fn osc1_transpose(mut self, steps: f32) -> Self {
        self.handle_message(SetOsc1Transpose(steps));
        self
    }

    /// Set the transposition of the synth's second oscillator, then return the
    /// same synth.
    pub fn osc2_transpose(mut self, steps: f32) -> Self {
        self.handle_message(SetOsc2Transpose(steps));
        self
    }

    /// Set the synth's ADSR envelope, then return the same synth.
    ///
    /// * `attack_time` specifies the length of the attack in seconds.
    /// * `decay_time` specifies the length of the decay in seconds.
    /// * `sustain_level` specifies the amplitude of the sustain from 0 to 1.
    /// * `release_time` specifies the length of the release in seconds.
    pub fn adsr(mut self, attack_time: f32, decay_time: f32, sustain_level: f32,
               release_time: f32) -> Self {
        self.handle_message(SetAttack(attack_time));
        self.handle_message(SetDecay(decay_time));
        self.handle_message(SetSustain(sustain_level));
        self.handle_message(SetRelease(release_time));
        self
    }

    /// Set the synth's LFO frequency, then return the same synth.
    pub fn lfo(mut self, freq: f32) -> Self {
        self.handle_message(SetLFOFreq(freq));
        self
    }

    /// Set the synth's vibrato intensity in steps, then return the same synth.
    pub fn vibrato(mut self, vibrato: f32) -> Self {
        self.handle_message(SetVibrato(vibrato));
        self
    }

    /// Set the synth's tremolo intensity in decibels, then return the same synth.
    pub fn tremolo(mut self, tremolo: f32) -> Self {
        self.handle_message(SetTremolo(tremolo));
        self
    }

    /// Set the synth's filter to a first order with the specified mode, then
    /// return the same synth.
    pub fn first_order(mut self, mode: first_order::FilterMode) -> Self {
        self.handle_message(SetFilterFirstOrder(mode));
        self
    }

    /// Set the synth's filter to a second order with the specified mode, then
    /// return the same synth.
    pub fn second_order(mut self, mode: second_order::FilterMode) -> Self {
        self.handle_message(SetFilterSecondOrder(mode));
        self
    }

    /// Perform the action specified by the message
    fn handle_message(&mut self, msg: SubtractiveSynthMessage) {
        match msg {
            SubtractiveSynthMessage::SetGain(gain) => {
                self.gain = decibel_to_ratio(gain);
            },
            SubtractiveSynthMessage::SetOsc1(waveform) => {
                for voice in self.voices.iter_mut() {
                    voice.osc1.handle_message(oscillator::SetWaveform(waveform));
                }
            },
            SubtractiveSynthMessage::SetOsc2(waveform) => {
                for voice in self.voices.iter_mut() {
                    voice.osc2.handle_message(oscillator::SetWaveform(waveform));
                }
            },
            SubtractiveSynthMessage::SetOsc1Transpose(steps) => {
                for voice in self.voices.iter_mut() {
                    voice.osc1.handle_message(oscillator::SetTranspose(steps));
                }
            },
            SubtractiveSynthMessage::SetOsc2Transpose(steps) => {
                for voice in self.voices.iter_mut() {
                    voice.osc2.handle_message(oscillator::SetTranspose(steps));
                }
            },
            SubtractiveSynthMessage::SetAttack(attack) => {
                for voice in self.voices.iter_mut() {
                    voice.adsr.handle_message(adsr::SetAttack(attack));
                }
            },
            SubtractiveSynthMessage::SetDecay(decay) => {
                for voice in self.voices.iter_mut() {
                    voice.adsr.handle_message(adsr::SetDecay(decay));
                }
            },
            SubtractiveSynthMessage::SetSustain(sustain) => {
                for voice in self.voices.iter_mut() {
                    voice.adsr.handle_message(adsr::SetSustain(sustain));
                }
            },
            SubtractiveSynthMessage::SetRelease(release) => {
                for voice in self.voices.iter_mut() {
                    voice.adsr.handle_message(adsr::SetRelease(release));
                }
            },
            SubtractiveSynthMessage::SetLFOFreq(freq) => {
                self.lfo.handle_message(oscillator::SetFreq(freq));
            },
            SubtractiveSynthMessage::SetVibrato(intensity) => {
                for voice in self.voices.iter_mut() {
                    voice.osc1.handle_message(
                        oscillator::SetLFOIntensity(intensity));
                    voice.osc2.handle_message(
                        oscillator::SetLFOIntensity(intensity));
                }
            },
            SetTremolo(intensity) => {
                self.tremolo.handle_message(tremolo::SetIntensity(intensity));
            },
            SetFilterFirstOrder(mode) => {
                self.filter = FilterType::FirstOrder;
                self.first_filter.handle_message(first_order::SetMode(mode));
            },
            SetFilterSecondOrder(mode) => {
                self.filter = FilterType::SecondOrder;
                self.second_filter.handle_message(second_order::SetMode(mode));
            },
        }
    }
}

impl<M> AudioDevice for SubtractiveSynth<M> where M: MidiDevice {
    fn num_inputs(&self) -> usize {
        0
    }

    fn num_outputs(&self) -> usize {
        1
    }

    fn tick(&mut self, t: Time, _: &[Sample], outputs: &mut[Sample]) {
        for event in self.midi.get_events(t) {
            self.handle_event(event);
        }

        self.lfo.tick(t);
        let mut voice_out = 0.0;
        for voice in self.voices.iter_mut() {
            voice_out += voice.tick(t, &self.lfo.outputs);
        }

        self.first_filter.inputs[0] = voice_out;
        self.second_filter.inputs[0] = voice_out;
        self.first_filter.tick(t);
        self.second_filter.tick(t);

        self.tremolo.inputs[0] = match self.filter {
            FilterType::FirstOrder => self.first_filter.outputs[0],
            FilterType::SecondOrder => self.second_filter.outputs[0]
        };
        self.tremolo.inputs[1] = self.lfo.outputs[0];
        self.tremolo.tick(t);

        outputs[0] = self.gain * self.tremolo.outputs[0];
    }
}


/// The container for a single voice
struct SubtractiveSynthVoice {
    key_held: bool,
    sustain_held: bool,
    osc1: Buffered<Oscillator>,
    osc2: Buffered<Oscillator>,
    adsr: Buffered<Adsr>,
}

impl SubtractiveSynthVoice {
    /// Create a new voice
    fn new() -> Self {
        SubtractiveSynthVoice {
            key_held: false,
            sustain_held: false,
            osc1: Buffered::from(Oscillator::new(oscillator::Sine)),
            osc2: Buffered::from(Oscillator::new(oscillator::Sine)),
            adsr: Buffered::from(Adsr::default(1)),
        }
    }

    /// Handle MIDI event
    fn handle_event(&mut self, event: MidiEvent) {
        match event.payload {
            MidiMessage::NoteOn(note, _) => {
                self.key_held = true;
                let freq = midi_note_to_freq(note);
                self.osc1.handle_message(oscillator::SetFreq(freq));
                self.osc2.handle_message(oscillator::SetFreq(freq));
                self.adsr.handle_message(adsr::NoteDown);
            },
            MidiMessage::NoteOff(_, _) => {
                self.key_held = false;
                if !self.sustain_held {
                    self.adsr.handle_message(adsr::NoteUp);
                }
            },
            MidiMessage::SustainPedal(true) => {
                self.sustain_held = true;
            },
            MidiMessage::SustainPedal(false) => {
                self.sustain_held = false;
                if !self.key_held {
                    self.adsr.handle_message(adsr::NoteUp);
                }
            },
            MidiMessage::PitchBend(value) => {
                let bend = 2.0*value;
                self.osc1.handle_message(oscillator::SetBend(bend));
                self.osc2.handle_message(oscillator::SetBend(bend));
            },
            _ => ()
        }
    }

    /// Process a single timestep, and return the voice's output for that
    /// timestep
    fn tick(&mut self, t: Time, lfo: &[Sample]) -> Sample {
        self.osc1.inputs[0] = lfo[0];
        self.osc2.inputs[0] = lfo[0];
        self.osc1.tick(t);
        self.osc2.tick(t);
        self.adsr.inputs[0] = (self.osc1.outputs[0] + self.osc2.outputs[0]) / 2.0;
        self.adsr.tick(t);
        self.adsr.outputs[0]
    }
}
