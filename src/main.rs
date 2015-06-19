//! Sets up a subtractive synth listening to the default MIDI input

extern crate oxcable;
use oxcable::types::{MidiEvent, MidiMessage};

mod subtractive_synth;
use subtractive_synth::{SubtractiveSynth, SubtractiveSynthMessage};

fn qx49_controls(event: MidiEvent) -> Option<SubtractiveSynthMessage> {
    let (byte1, byte2) = match event.payload {
        MidiMessage::ControlChange(byte1, byte2) => (byte1, byte2),
        _ => panic!("impossible midi event")
    };
    let range = byte2 as f32 / 127.0;
    match byte1 {
        22 => Some(SubtractiveSynthMessage::SetAttack(5.0*range)),
        23 => Some(SubtractiveSynthMessage::SetDecay(5.0*range)),
        24 => Some(SubtractiveSynthMessage::SetSustain(range)),
        25 => Some(SubtractiveSynthMessage::SetRelease(5.0*range)),
        _ => None
    }
}

fn main() {
    use oxcable::chain::DeviceChain;
    use oxcable::dynamics::Limiter;
    use oxcable::io::audio::AudioEngine;
    use oxcable::io::midi::MidiEngine;
    use oxcable::mixers::Gain;
    use oxcable::oscillator::{AntialiasType, Waveform};
    use oxcable::utils::tick::tick_until_enter;

    println!("Initializing signal chain...");
    let audio_engine = AudioEngine::open().unwrap();
    let midi_engine = MidiEngine::open().unwrap();
    let mut chain = DeviceChain::from(
        SubtractiveSynth::new(midi_engine.choose_input(), 10)
            .waveform(Waveform::Saw(AntialiasType::PolyBlep))
            .control_map(qx49_controls)
    ).into(
        Gain::new(-6.0, 1)
    ).into(
        Limiter::new(-3.0, 0.0, 1)
    ).into(
        audio_engine.default_output(1)
    );

    println!("Playing. Press Enter to quit...");
    tick_until_enter(&mut chain);
}
