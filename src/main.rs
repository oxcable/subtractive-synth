//! A binary that runs the subsynth as a standalone device.
//!
//! When run, the script will first ask which MIDI device to use, and will then
//! play audio to the default audio output.
//!
//! The synth is set up to use a reasonable default tone, and a control map
//! based on the Alesis Qx49 MIDI keyboard.

extern crate oxcable;
extern crate oxcable_subtractive_synth;

#[cfg(not(test))]
use oxcable_subtractive_synth as subsynth;

#[cfg(not(test))]
fn qx49_controls(controller: u8, value: u8) -> Option<subsynth::Message> {
    let range = value as f32 / 127.0;
    match controller {
        22 => Some(subsynth::SetAttack(5.0*range)),
        23 => Some(subsynth::SetDecay(5.0*range)),
        24 => Some(subsynth::SetSustain(range)),
        25 => Some(subsynth::SetRelease(5.0*range)),
        26 => Some(subsynth::SetLFOFreq(10.0*range)),
        27 => Some(subsynth::SetVibrato(range)),
        _ => None
    }
}

#[cfg(not(test))]
static BUFFER_SIZE: usize = 256;

#[cfg(not(test))]
fn main() {
    use oxcable::chain::{DeviceChain, Tick};
    use oxcable::dynamics::Limiter;
    use oxcable::io::audio::AudioEngine;
    use oxcable::io::midi::MidiEngine;
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
        Limiter::new(-3.0, 0.0, 1)
    ).into(
        audio_engine.default_output(1).unwrap()
    );

    println!("Playing. Press Enter to quit...");
    chain.tick_until_enter();
}
