//! A basic subtractive synthesizer

extern crate oxcable;

use oxcable::adsr::{self, Adsr};
use oxcable::filters::{first_order, second_order};
use oxcable::oscillator::{self, Oscillator, Waveform};
use oxcable::types::{AudioDevice, MidiDevice, MidiEvent, MidiMessage, Time, Sample};
use oxcable::utils::helpers::midi_note_to_freq;
use oxcable::voice_array::VoiceArray;


#[derive(Copy, Clone, Debug)]
pub enum SubtractiveSynthMessage {
    SetOsc1(Waveform),
    SetOsc2(Waveform),
    SetOsc1Transpose(f32),
    SetOsc2Transpose(f32),
    SetAttack(f32),
    SetDecay(f32),
    SetSustain(f32),
    SetRelease(f32),
    SetLFOFreq(f32),
    SetVibrato(f32),
    SetFilterFirstOrder(first_order::FilterMode),
    SetFilterSecondOrder(second_order::FilterMode),
}
pub use self::SubtractiveSynthMessage::*;

#[derive(Copy, Clone, Debug)]
enum FilterType { FirstOrder, SecondOrder }

/// A polyphonic subtractive synthesizer
pub struct SubtractiveSynth<M: MidiDevice> {
    voices: VoiceArray<SubtractiveSynthVoice>,
    controls: Option<Box<Fn(MidiEvent) -> Option<SubtractiveSynthMessage>>>,
    midi: M,
    lfo: Oscillator,
    filter: FilterType,
    first_filter: first_order::Filter,
    second_filter: second_order::Filter,
    lfo_buf: [Sample; 1],
    filter_input_buf: [Sample; 1],
    first_filter_buf: [Sample; 1],
    second_filter_buf: [Sample; 1],
    gain: f32,
}

impl<M> SubtractiveSynth<M> where M: MidiDevice {
    /// Returns a new subtractive synth that can play `num_voices` notes at one
    /// time.
    pub fn new(midi: M, num_voices: usize) -> SubtractiveSynth<M> {
        let mut voices = Vec::with_capacity(num_voices);
        for _i in (0 .. num_voices) {
            voices.push(SubtractiveSynthVoice::new());
        }
        let voice_array = VoiceArray::new(voices);

        SubtractiveSynth {
            voices: voice_array,
            controls: None,
            midi: midi,
            lfo: Oscillator::new(oscillator::Sine).freq(10.0),
            filter: FilterType::FirstOrder,
            first_filter: first_order::Filter::new(
                first_order::LowPass(20000.0), 1),
            second_filter: second_order::Filter::new(
                second_order::LowPass(20000.0), 1),
            lfo_buf: [0.0],
            filter_input_buf: [0.0],
            first_filter_buf: [0.0],
            second_filter_buf: [0.0],
            gain: -12.0,
        }
    }

    pub fn control_map<F>(mut self, map: F) -> SubtractiveSynth<M>
            where F: 'static+Fn(MidiEvent) -> Option<SubtractiveSynthMessage> {
        self.controls = Some(Box::new(map));
        self
    }

    pub fn osc1(mut self, waveform: Waveform) -> SubtractiveSynth<M> {
        self.handle_message(SetOsc1(waveform));
        self
    }

    pub fn osc2(mut self, waveform: Waveform) -> SubtractiveSynth<M> {
        self.handle_message(SetOsc2(waveform));
        self
    }

    pub fn osc1_transpose(mut self, steps: f32) -> SubtractiveSynth<M> {
        self.handle_message(SetOsc1Transpose(steps));
        self
    }

    pub fn osc2_transpose(mut self, steps: f32) -> SubtractiveSynth<M> {
        self.handle_message(SetOsc2Transpose(steps));
        self
    }

    pub fn adsr(mut self, attack_time: f32, decay_time: f32, sustain_level: f32,
               release_time: f32) -> SubtractiveSynth<M> {
        self.handle_message(SetAttack(attack_time));
        self.handle_message(SetDecay(decay_time));
        self.handle_message(SetSustain(sustain_level));
        self.handle_message(SetRelease(release_time));
        self
    }

    pub fn lfo(mut self, freq: f32, vibrato: f32) -> SubtractiveSynth<M> {
        self.handle_message(SetLFOFreq(freq));
        self.handle_message(SetVibrato(vibrato));
        self
    }

    pub fn first_order(mut self, mode: first_order::FilterMode)
            -> SubtractiveSynth<M> {
        self.handle_message(SetFilterFirstOrder(mode));
        self
    }

    pub fn second_order(mut self, mode: second_order::FilterMode)
            -> SubtractiveSynth<M> {
        self.handle_message(SetFilterSecondOrder(mode));
        self
    }

    fn handle_message(&mut self, msg: SubtractiveSynthMessage) {
        match msg {
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
                    voice.osc1.handle_message(oscillator::SetLFOIntensity(intensity));
                    voice.osc2.handle_message(oscillator::SetLFOIntensity(intensity));
                }
            },
            SubtractiveSynthMessage::SetFilterFirstOrder(mode) => {
                self.filter = FilterType::FirstOrder;
                self.first_filter.set_mode(mode);
            },
            SubtractiveSynthMessage::SetFilterSecondOrder(mode) => {
                self.filter = FilterType::SecondOrder;
                self.second_filter.set_mode(mode);
            },
        }
    }

    fn handle_event(&mut self, event: MidiEvent) {
        match event.payload {
            MidiMessage::NoteOn(note, _) => {
                self.voices.note_on(note).handle_event(event);
            },
            MidiMessage::NoteOff(note, _) => {
                self.voices.note_off(note).map_or((), |d| d.handle_event(event));
            },
            MidiMessage::ControlChange(_, _) => {
                let msg = match self.controls {
                    Some(ref f) => f(event),
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

        self.lfo.tick(t, &[0.0;0], &mut self.lfo_buf);
        self.filter_input_buf[0] = 0.0;
        for voice in self.voices.iter_mut() {
            self.filter_input_buf[0] += voice.tick(t, &self.lfo_buf);
        }
        self.first_filter.tick(t, &self.filter_input_buf,
                              &mut self.first_filter_buf);
        self.second_filter.tick(t, &self.filter_input_buf,
                               &mut self.second_filter_buf);
        let s = match self.filter {
            FilterType::FirstOrder => self.first_filter_buf[0],
            FilterType::SecondOrder => self.second_filter_buf[0],
        };
        outputs[0] = self.gain * s;
    }
}


/// The container for a single voice
struct SubtractiveSynthVoice {
    key_held: bool,
    sustain_held: bool,
    osc1: Oscillator,
    osc2: Oscillator,
    adsr: Adsr,
    osc1_buf: [Sample; 1],
    osc2_buf: [Sample; 1],
    osc_out: [Sample; 1],
    adsr_buf: [Sample; 1],
}

impl SubtractiveSynthVoice {
    fn new() -> SubtractiveSynthVoice {
        SubtractiveSynthVoice {
            key_held: false,
            sustain_held: false,
            osc1: Oscillator::new(oscillator::Sine),
            osc2: Oscillator::new(oscillator::Sine),
            adsr: Adsr::default(1),
            osc1_buf: [0.0],
            osc2_buf: [0.0],
            osc_out: [0.0],
            adsr_buf: [0.0],
        }
    }

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

    fn tick(&mut self, t: Time, lfo: &[Sample]) -> Sample {
        self.osc1.tick(t, lfo, &mut self.osc1_buf);
        self.osc2.tick(t, lfo, &mut self.osc2_buf);
        self.osc_out[0] = self.osc1_buf[0] + self.osc2_buf[0];
        self.adsr.tick(t, &self.osc_out, &mut self.adsr_buf);
        self.adsr_buf[0]
    }
}
