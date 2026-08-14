use synth_core::{MidiNote, SynthEngine};
use synth_dsl::{CompileError, Compiler, Inputs};

fn main() -> Result<(), CompileError> {
    let program = Compiler::new()
        .compile("wave = sin(TAU * freq * t) * exp(-3 * t)\npan = 0\nl_limit = 2")?;
    let mut synth = SynthEngine::new(48_000.0, program);

    synth.note_on(MidiNote::new(60), 1.0);
    let mut left = [0.0; 16];
    let mut right = [0.0; 16];
    synth.render(&mut left, &mut right, Inputs::default());
    synth.note_off(MidiNote::new(60));

    println!("rendered {} samples", left.len());
    Ok(())
}
