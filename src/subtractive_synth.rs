//! A basic subtractive synthesizer

extern crate oxcable;

use oxcable::adsr::{self, Adsr};
use oxcable::oscillator::{self, Oscillator, Waveform};
use oxcable::types::{AudioDevice, MidiDevice, MidiEvent, MidiMessage, Time, Sample};
use oxcable::utils::helpers::midi_note_to_freq;
use oxcable::voice_array::VoiceArray;


#[derive(Copy, Clone, Debug)]
pub enum SubtractiveSynthMessage {
    SetWaveform(Waveform),
    SetAttack(f32),
    SetDecay(f32),
    SetSustain(f32),
    SetRelease(f32),
    SetLFOFreq(f32),
    SetVibrato(f32),
}
pub use self::SubtractiveSynthMessage::*;

/// A polyphonic subtractive synthesizer
pub struct SubtractiveSynth<M: MidiDevice> {
    voices: VoiceArray<SubtractiveSynthVoice>,
    controls: Option<Box<Fn(MidiEvent) -> Option<SubtractiveSynthMessage>>>,
    midi: M,
    lfo: Oscillator,
    lfo_buf: [Sample; 1],
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
            lfo_buf: [0.0],
            gain: -12.0,
        }
    }

    pub fn control_map<F>(mut self, map: F) -> SubtractiveSynth<M>
            where F: 'static+Fn(MidiEvent) -> Option<SubtractiveSynthMessage> {
        self.controls = Some(Box::new(map));
        self
    }

    pub fn waveform(mut self, waveform: Waveform) -> SubtractiveSynth<M> {
        self.handle_message(SetWaveform(waveform));
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

    fn handle_message(&mut self, msg: SubtractiveSynthMessage) {
        match msg {
            SubtractiveSynthMessage::SetWaveform(waveform) => {
                for voice in self.voices.iter_mut() {
                    voice.osc.handle_message(oscillator::SetWaveform(waveform));
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
                    voice.osc.handle_message(oscillator::SetLFOIntensity(intensity));
                }
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
        let mut s = 0.0;
        for voice in self.voices.iter_mut() {
            s += voice.tick(t, &self.lfo_buf);
        }
        outputs[0] = self.gain * s;
    }
}


/// The container for a single voice
struct SubtractiveSynthVoice {
    key_held: bool,
    sustain_held: bool,
    osc: Oscillator,
    adsr: Adsr,
    osc_buf: [Sample; 1],
    adsr_buf: [Sample; 1],
}

impl SubtractiveSynthVoice {
    fn new() -> SubtractiveSynthVoice {
        let osc = Oscillator::new(oscillator::Sine);
        let adsr = Adsr::default(1);
        SubtractiveSynthVoice {
            key_held: false,
            sustain_held: false,
            osc: osc,
            adsr: adsr,
            osc_buf: [0.0],
            adsr_buf: [0.0],
        }
    }

    fn handle_event(&mut self, event: MidiEvent) {
        match event.payload {
            MidiMessage::NoteOn(note, _) => {
                self.key_held = true;
                self.osc.handle_message(oscillator::SetFreq(
                        midi_note_to_freq(note)));
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
                self.osc.handle_message(oscillator::SetBend(2.0*value));
            },
            _ => ()
        }
    }

    fn tick(&mut self, t: Time, lfo: &[Sample]) -> Sample {
        self.osc.tick(t, lfo, &mut self.osc_buf);
        self.adsr.tick(t, &self.osc_buf, &mut self.adsr_buf);
        self.adsr_buf[0]
    }
}
