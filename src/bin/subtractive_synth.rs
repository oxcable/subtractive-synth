//! Sets up a subtractive synth listening to the default MIDI input

extern crate oxcable;
use oxcable::types::{MidiEvent, MidiMessage};

extern crate oxcable_subtractive_synth;
use oxcable_subtractive_synth as subsynth;

fn qx49_controls(event: MidiEvent) -> Option<subsynth::SubtractiveSynthMessage> {
    let (byte1, byte2) = match event.payload {
        MidiMessage::ControlChange(byte1, byte2) => (byte1, byte2),
        _ => panic!("impossible midi event")
    };
    let range = byte2 as f32 / 127.0;
    match byte1 {
        22 => Some(subsynth::SetAttack(5.0*range)),
        23 => Some(subsynth::SetDecay(5.0*range)),
        24 => Some(subsynth::SetSustain(range)),
        25 => Some(subsynth::SetRelease(5.0*range)),
        26 => Some(subsynth::SetLFOFreq(10.0*range)),
        27 => Some(subsynth::SetVibrato(range)),
        _ => None
    }
}

static BUFFER_SIZE: usize = 256;

fn main() {
    use oxcable::chain::{DeviceChain, Tick};
    use oxcable::dynamics::Limiter;
    use oxcable::io::audio::AudioEngine;
    use oxcable::io::midi::MidiEngine;
    use oxcable::mixers::Gain;
    use oxcable::oscillator::{Saw, PolyBlep};

    println!("Initializing signal chain...");
    let audio_engine = AudioEngine::with_buffer_size(BUFFER_SIZE).unwrap();
    let midi_engine = MidiEngine::open().unwrap();
    let midi = midi_engine.choose_input().unwrap();
    let mut chain = DeviceChain::from(
        subsynth::SubtractiveSynth::new(midi, 10)
            .osc1(Saw(PolyBlep)).osc2(Saw(PolyBlep))
            .control_map(qx49_controls)
    ).into(
        Gain::new(-12.0, 1)
    ).into(
        Limiter::new(-3.0, 0.0, 1)
    ).into(
        audio_engine.default_output(1).unwrap()
    );

    println!("Playing. Press Enter to quit...");
    chain.tick_until_enter();
}
