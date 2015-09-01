//! A polyphonic subtractive synthesizer.
//!
//! This synthesizer uses building blocks from `oxcable` to perform basic
//! subtractive synthesis. The synth is controlled via MIDI and packaged as an
//! `AudioDevice`.
//!
//! # Signal Chain
//!
//! The synthesizer is polyphonic. The signal chain is composed of:
//!
//!  1. Two oscillators, operating independently. The oscillators may be
//!     transposed relative to each other.
//!  2. Independent ADSR envelopes for each voice.
//!  3. The signal is then passed through a multimode filter.
//!
//! Additionally, the synthesizer has an internal low frequency oscillator. This
//! LFO may be used to add vibrato to the oscillators, or tremolo to the output.
//!
//! # Controlling Tone
//!
//! The synthesizer provides three ways to configure its tone:
//!
//! 1. At initialization, using the builder pattern.
//! 2. During runtime, by passing it a [`Message`](enum.Message.html).
//! 3. With MIDI control signals, using a control map...
//!
//! ## MIDI Control Signals
//!
//! Many MIDI controllers feature additional knobs and sliders. These use
//! a special type of MIDI signal that is left open to instrument makers to
//! use as they desire.  To use these knobs for tone control, the synth uses an
//! optional closure to convert these control signals to synthesizer messages.
//! This closure takes the form:
//!
//! ```
//! # /*
//! fn control_map(controller: u8, value: u8) -> Option<Message>;
//! # */
//! ```
//!
//! The exact values used can vary from device to device, but `controller` will
//! always specify which knob is being used, and the `value` will range from 0 to
//! 127.
//!
//! If `Some(msg)` is returned, it will be used to update the synth immediately;
//! if `None` is returned, the MIDI message is ignored.
//!
//! The following example uses knobs on the Alesis QX49 keyboard to adjust the
//! ADSR envelope and vibrato:
//!
//! ```
//! use oxcable_subtractive_synth as subsynth;
//! fn qx49_controls(controller: u8, value: u8) -> Option<subsynth::Message> {
//!     let range = value as f32 / 127.0; // normalize to range [0.0, 1.0]
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
use oxcable::types::{AudioDevice, MessageReceiver, MidiDevice, MidiEvent,
        MidiMessage, Time, Sample};
use oxcable::utils::helpers::{midi_note_to_freq, decibel_to_ratio};
use oxcable::voice_array::VoiceArray;
use oxcable::wrappers::Buffered;


/// Defines the messages that the synthesizer supports.
#[derive(Copy, Clone, Debug)]
pub enum Message {
    /// Sets the output gain, in decibels.
    SetGain(f32),
    /// Sets the waveform for the first oscillator.
    SetOsc1(Waveform),
    /// Sets the waveform for the second oscillator.
    SetOsc2(Waveform),
    /// Sets the transposition of the first oscillator, in steps.
    SetOsc1Transpose(f32),
    /// Sets the transposition of the second oscillator, in steps.
    SetOsc2Transpose(f32),
    /// Sets the ADSR attack duration, in seconds.
    SetAttack(f32),
    /// Sets the ADSR decay duration, in seconds.
    SetDecay(f32),
    /// Sets the ADSR sustain level, from 0 to 1.
    SetSustain(f32),
    /// Sets the ADSR release duration, in seconds.
    SetRelease(f32),
    /// Sets the LFO frequency, in Hz.
    SetLFOFreq(f32),
    /// Sets the vibrato intensity, in steps.
    SetVibrato(f32),
    /// Sets the tremolo intensity, in decibels.
    SetTremolo(f32),
    /// Sets the filter to a first order filter of the specified mode.
    SetFilterFirstOrder(first_order::FilterMode),
    /// Sets the filter to a second order filter of the specified mode.
    SetFilterSecondOrder(second_order::FilterMode),
    /// Sends the provided MIDI event to the synth.
    SendMidiEvent(MidiEvent),
}
pub use self::Message::*;


/// Internally used to track with filter type to use.
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
    /// Returns a new subtractive synth.
    ///
    /// * `midi`: the MIDI source to use.
    /// * `num_voices`: the maximum voices that can play at one time.
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

    /// Sets the control signal map to the provided closure, then return the
    /// same synth.
    ///
    /// For further details on control mappings, see the [main synth
    /// documentation](index.html#midi-control-signals).
    pub fn control_map<F>(mut self, map: F) -> Self
            where F: 'static+Fn(u8, u8) -> Option<Message> {
        self.controls = Some(Box::new(map));
        self
    }

    /// Sets the gain of the synth in decibels, then return the same synth.
    pub fn gain(mut self, gain: f32) -> Self {
        self.handle_message(SetGain(gain));
        self
    }

    /// Sets the waveform of the synth's first oscillator, then return the same
    /// synth.
    pub fn osc1(mut self, waveform: Waveform) -> Self {
        self.handle_message(SetOsc1(waveform));
        self
    }

    /// Sets the waveform of the synth's second oscillator, then return the same
    /// synth.
    pub fn osc2(mut self, waveform: Waveform) -> Self {
        self.handle_message(SetOsc2(waveform));
        self
    }

    /// Sets the transposition of the synth's first oscillator in steps, then
    /// return the same synth.
    pub fn osc1_transpose(mut self, steps: f32) -> Self {
        self.handle_message(SetOsc1Transpose(steps));
        self
    }

    /// Sets the transposition of the synth's second oscillator in steps, then
    /// return the same synth.
    pub fn osc2_transpose(mut self, steps: f32) -> Self {
        self.handle_message(SetOsc2Transpose(steps));
        self
    }

    /// Sets the synth's ADSR envelope, then return the same synth.
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

    /// Sets the synth's LFO frequency, then return the same synth.
    pub fn lfo(mut self, freq: f32) -> Self {
        self.handle_message(SetLFOFreq(freq));
        self
    }

    /// Sets the synth's vibrato intensity in steps, then return the same synth.
    pub fn vibrato(mut self, vibrato: f32) -> Self {
        self.handle_message(SetVibrato(vibrato));
        self
    }

    /// Sets the synth's tremolo intensity in decibels, then return the same synth.
    pub fn tremolo(mut self, tremolo: f32) -> Self {
        self.handle_message(SetTremolo(tremolo));
        self
    }

    /// Sets the synth's filter to a first order with the specified mode, then
    /// return the same synth.
    pub fn first_order(mut self, mode: first_order::FilterMode) -> Self {
        self.handle_message(SetFilterFirstOrder(mode));
        self
    }

    /// Sets the synth's filter to a second order with the specified mode, then
    /// return the same synth.
    pub fn second_order(mut self, mode: second_order::FilterMode) -> Self {
        self.handle_message(SetFilterSecondOrder(mode));
        self
    }

    // Handles MIDI events.
    fn handle_event(&mut self, event: MidiEvent) {
        match event.payload {
            MidiMessage::NoteOn(note, _) => {
                self.voices.note_on(note).handle_event(event);
            },
            MidiMessage::NoteOff(note, _) => {
                self.voices.note_off(note).map_or((), |d| d.handle_event(event));
            },
            MidiMessage::ControlChange(controller, value) => {
                let msg = match self.controls {
                    Some(ref f) => f(controller, value),
                    None => None
                };
                msg.map(|m| self.handle_message(m));
            },
            _ => {
                for voice in self.voices.iter_mut() {
                    voice.handle_event(event);
                }
            }
        }
    }
}

impl<M> MessageReceiver for SubtractiveSynth<M> where M: MidiDevice {
    type Msg = Message;
    fn handle_message(&mut self, msg: Message) {
        match msg {
            SetGain(gain) => {
                self.gain = decibel_to_ratio(gain);
            },
            SetOsc1(waveform) => {
                for voice in self.voices.iter_mut() {
                    voice.osc1.handle_message(oscillator::SetWaveform(waveform));
                }
            },
            SetOsc2(waveform) => {
                for voice in self.voices.iter_mut() {
                    voice.osc2.handle_message(oscillator::SetWaveform(waveform));
                }
            },
            SetOsc1Transpose(steps) => {
                for voice in self.voices.iter_mut() {
                    voice.osc1.handle_message(oscillator::SetTranspose(steps));
                }
            },
            SetOsc2Transpose(steps) => {
                for voice in self.voices.iter_mut() {
                    voice.osc2.handle_message(oscillator::SetTranspose(steps));
                }
            },
            SetAttack(attack) => {
                for voice in self.voices.iter_mut() {
                    voice.adsr.handle_message(adsr::SetAttack(attack));
                }
            },
            SetDecay(decay) => {
                for voice in self.voices.iter_mut() {
                    voice.adsr.handle_message(adsr::SetDecay(decay));
                }
            },
            SetSustain(sustain) => {
                for voice in self.voices.iter_mut() {
                    voice.adsr.handle_message(adsr::SetSustain(sustain));
                }
            },
            SetRelease(release) => {
                for voice in self.voices.iter_mut() {
                    voice.adsr.handle_message(adsr::SetRelease(release));
                }
            },
            SetLFOFreq(freq) => {
                self.lfo.handle_message(oscillator::SetFreq(freq));
            },
            SetVibrato(intensity) => {
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
            SendMidiEvent(event) => {
                self.handle_event(event);
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


/// The container for a single voice.
struct SubtractiveSynthVoice {
    key_held: bool,
    sustain_held: bool,
    osc1: Buffered<Oscillator>,
    osc2: Buffered<Oscillator>,
    adsr: Buffered<Adsr>,
}

impl SubtractiveSynthVoice {
    /// Creates a new voice.
    fn new() -> Self {
        SubtractiveSynthVoice {
            key_held: false,
            sustain_held: false,
            osc1: Buffered::from(Oscillator::new(oscillator::Sine)),
            osc2: Buffered::from(Oscillator::new(oscillator::Sine)),
            adsr: Buffered::from(Adsr::default(1)),
        }
    }

    /// Handles MIDI events.
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

    /// Processes a single timestep, then returns the voice's output for that
    /// timestep.
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
