#[path = "../src/encoded_output.rs"]
mod encoded_output;
use encoded_output::EncodedOutput;

#[test]
fn draining_output_does_not_turn_the_next_inter_frame_into_an_idr() {
    let mut output = EncodedOutput::default();
    output.configure(&[0, 0, 0, 1, 0x67]);
    output.frame(0.0, &[0, 0, 0, 1, 0x65], true);
    assert!(output.take()[0].2);
    output.frame(1.0 / 30.0, &[0, 0, 0, 1, 0x41], false);
    let frame = output.take().pop().unwrap();
    assert!(
        !frame.2,
        "a drained batch is not a codec synchronization boundary"
    );
    output.frame(2.0, &[0, 0, 0, 1, 0x65], true);
    assert!(output.take()[0].2);
    assert!(output.take().is_empty());
}
