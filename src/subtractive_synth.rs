//! A basic subtractive synthesizer

extern crate oxcable;

use oxcable::adsr::{Adsr, AdsrMessage};
use oxcable::oscillator::{Oscillator, OscillatorMessage, Waveform};
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
}

/// A polyphonic subtractive synthesizer
pub struct SubtractiveSynth<M: MidiDevice> {
    voices: VoiceArray<SubtractiveSynthVoice>,
    controls: Option<Box<Fn(MidiEvent) -> Option<SubtractiveSynthMessage>>>,
    midi: M,
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
            gain: -12.0,
        }
    }

    pub fn control_map<F>(mut self, map: F) -> SubtractiveSynth<M>
            where F: 'static+Fn(MidiEvent) -> Option<SubtractiveSynthMessage> {
        self.controls = Some(Box::new(map));
        self
    }

    pub fn waveform(mut self, waveform: Waveform) -> SubtractiveSynth<M> {
        self.set_waveform(waveform);
        self
    }

    fn set_waveform(&mut self, waveform: Waveform) {
        for voice in self.voices.iter_mut() {
            voice.osc.handle_message(OscillatorMessage::SetWaveform(waveform));
        }
    }

    pub fn adsr(mut self, attack_time: f32, decay_time: f32, sustain_level: f32,
               release_time: f32) -> SubtractiveSynth<M> {
        self.set_attack(attack_time);
        self.set_decay(decay_time);
        self.set_sustain(sustain_level);
        self.set_release(release_time);
        self
    }

    fn set_attack(&mut self, attack_time: f32) {
        for voice in self.voices.iter_mut() {
            voice.adsr.handle_message(AdsrMessage::SetAttack(attack_time));
        }
    }

    fn set_decay(&mut self, decay_time: f32) {
        for voice in self.voices.iter_mut() {
            voice.adsr.handle_message(AdsrMessage::SetDecay(decay_time));
        }
    }

    fn set_sustain(&mut self, sustain_level: f32) {
        for voice in self.voices.iter_mut() {
            voice.adsr.handle_message(AdsrMessage::SetSustain(sustain_level));
        }
    }

    fn set_release(&mut self, release_time: f32) {
        for voice in self.voices.iter_mut() {
            voice.adsr.handle_message(AdsrMessage::SetRelease(release_time));
        }
    }

    fn handle_message(&mut self, msg: SubtractiveSynthMessage) {
        println!("{:?}", msg);
        match msg {
            SubtractiveSynthMessage::SetWaveform(waveform) => {
                self.set_waveform(waveform);
            },
            SubtractiveSynthMessage::SetAttack(attack) => {
                self.set_attack(attack);
            },
            SubtractiveSynthMessage::SetDecay(decay) => {
                self.set_decay(decay);
            },
            SubtractiveSynthMessage::SetSustain(sustain) => {
                self.set_sustain(sustain);
            },
            SubtractiveSynthMessage::SetRelease(release) => {
                self.set_release(release);
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

        let mut s = 0.0;
        for voice in self.voices.iter_mut() {
            s += voice.tick(t);
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
    empty_buf: [Sample; 0],
    osc_buf: [Sample; 1],
    adsr_buf: [Sample; 1],
}

impl SubtractiveSynthVoice {
    fn new() -> SubtractiveSynthVoice {
        let osc = Oscillator::new(Waveform::Sine, 0.0);
        let adsr = Adsr::default(1);
        SubtractiveSynthVoice {
            key_held: false,
            sustain_held: false,
            osc: osc,
            adsr: adsr,
            empty_buf: [],
            osc_buf: [0.0],
            adsr_buf: [0.0],
        }
    }

    fn handle_event(&mut self, event: MidiEvent) {
        match event.payload {
            MidiMessage::NoteOn(note, _) => {
                self.key_held = true;
                self.osc.handle_message(OscillatorMessage::SetFreq(
                        midi_note_to_freq(note)));
                self.adsr.handle_message(AdsrMessage::NoteDown);
            },
            MidiMessage::NoteOff(_, _) => {
                self.key_held = false;
                if !self.sustain_held {
                    self.adsr.handle_message(AdsrMessage::NoteUp);
                }
            },
            MidiMessage::SustainPedal(true) => {
                self.sustain_held = true;
            },
            MidiMessage::SustainPedal(false) => {
                self.sustain_held = false;
                if !self.key_held {
                    self.adsr.handle_message(AdsrMessage::NoteUp);
                }
            },
            _ => ()
        }
    }

    fn tick(&mut self, t: Time) -> Sample {
        self.osc.tick(t, &self.empty_buf, &mut self.osc_buf);
        self.adsr.tick(t, &self.osc_buf, &mut self.adsr_buf);
        self.adsr_buf[0]
    }
}
