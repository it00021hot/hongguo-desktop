#[path = "../src/encoded_output.rs"]
mod encoded_output;
use encoded_output::EncodedOutput;

#[test]
fn draining_output_does_not_turn_the_next_inter_frame_into_an_idr() {
    let mut output = EncodedOutput::default();
    output
        .configure(&[
            0, 0, 0, 1, 0x40, 1, 0, 0, 0, 1, 0x42, 1, 0, 0, 0, 1, 0x44, 1,
        ])
        .unwrap();
    output.frame(0.0, &[0, 0, 0, 1, 0x26, 1], true).unwrap();
    assert!(output.take()[0].2);
    output
        .frame(1.0 / 30.0, &[0, 0, 0, 1, 0x02, 1], false)
        .unwrap();
    let frame = output.take().pop().unwrap();
    assert!(
        !frame.2,
        "a drained batch is not a codec synchronization boundary"
    );
    output.frame(2.0, &[0, 0, 0, 1, 0x26, 1], true).unwrap();
    assert!(output.take()[0].2);
    assert!(output.take().is_empty());
}

#[test]
fn irap_without_platform_sync_flag_receives_one_complete_configuration() {
    let mut output = EncodedOutput::default();
    let csd = [
        0, 0, 0, 1, 0x40, 1, 0, 0, 0, 1, 0x42, 1, 0, 0, 0, 1, 0x44, 1,
    ];
    output.configure(&csd).unwrap();
    output.configure(&csd).unwrap();
    let cra = [0, 0, 0, 1, 0x2a, 1];
    output.frame(0.5, &cra, false).unwrap();
    let (pts, frame, sync) = output.take().pop().unwrap();
    assert_eq!(pts, 0.5);
    assert!(sync);
    assert_eq!(frame, [csd.as_slice(), cra.as_slice()].concat());
}

#[test]
fn invalid_configuration_or_missing_parameter_sets_never_publish_a_frame() {
    for bytes in [
        &[][..],
        &[0x40, 1],
        &[0, 0, 0, 1, 0x40],
        &[0, 0, 0, 1, 0x40, 0],
    ] {
        let mut output = EncodedOutput::default();
        assert!(output.configure(bytes).is_err());
        assert!(output.take().is_empty());
    }
    let mut output = EncodedOutput::default();
    assert!(output.frame(0.0, &[0, 0, 0, 1, 0x26, 1], true).is_err());
    assert!(output.take().is_empty());
}

#[test]
fn inline_three_byte_configuration_is_normalized_once() {
    let mut output = EncodedOutput::default();
    output
        .frame(
            1.0,
            &[
                0, 0, 1, 0x40, 1, 0, 0, 1, 0x42, 1, 0, 0, 1, 0x44, 1, 0, 0, 1, 0x26, 1,
            ],
            false,
        )
        .unwrap();
    let (pts, bytes, sync) = output.take().pop().unwrap();
    assert_eq!(pts, 1.0);
    assert!(sync);
    assert_eq!(
        bytes,
        [
            0, 0, 0, 1, 0x40, 1, 0, 0, 0, 1, 0x42, 1, 0, 0, 0, 1, 0x44, 1, 0, 0, 0, 1, 0x26, 1
        ]
    );
}
