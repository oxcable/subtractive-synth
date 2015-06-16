//! Sets up a subtractive synth listening to the default MIDI input

extern crate oxcable;

mod subtractive_synth;

fn main() {
    use oxcable::chain::DeviceChain;
    use oxcable::dynamics::Limiter;
    use oxcable::io::audio::AudioEngine;
    use oxcable::io::midi::MidiEngine;
    use oxcable::mixers::Gain;
    use oxcable::utils::tick::tick_until_enter;

    use subtractive_synth::SubtractiveSynth;

    println!("Initializing signal chain...");
    let audio_engine = AudioEngine::open().unwrap();
    let midi_engine = MidiEngine::open().unwrap();
    let mut chain = DeviceChain::from(
        SubtractiveSynth::new(midi_engine.choose_input(), 2)
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
