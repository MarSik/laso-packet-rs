pub(crate) use futures_lite::future::block_on;
use laso_packet::{
    behavior::decode_with_breaks,
    packet::PacketStatus,
    rx::{RxDecodeError, RxMessage},
};

fn test_msg_decode(from_radio: &[u8; 64]) -> Result<RxMessage<23>, RxDecodeError> {
    let mut rx = RxMessage::default();
    for i in 0..=1 {
        let p = block_on(decode_with_breaks(&from_radio[i * 32..(i + 1) * 32]));
        println!("Packet: {:?}", p);
        if let PacketStatus::Raw(status) = p.data.status {
            println!("Status after decoding: {:?}", rx.decode_status(status));
        }

        match rx.append(&p) {
            Ok(status) => {
                if status.finished() {
                    break;
                }
            }
            Err(err) => {
                panic!("Rx decode error: {:?}", err);
            }
        }
    }

    Ok(rx)
}

#[test]
pub fn test_beacon_1() {
    let rx = [
        0x6a_u8, 0xd4, 0x34, 0x9b, 0x2a, 0x24, 0x26, 0xa9, 0x58, 0xa4, 0x66, 0xe4, 0x34, 0xc9,
        0x64, 0x29, 0x9b, 0x9a, 0x2a, 0x72, 0xb2, 0x33, 0xa3, 0x72, 0x66, 0xd8, 0xaa, 0xa4, 0xa4,
        0x5a, 0x65, 0x71, 0x55, 0x55, 0x50, 0xc1, 0x5c, 0x29, 0x25, 0x72, 0xe1, 0x8c, 0xda, 0x5d,
        0x2a, 0xef, 0x6c, 0xf6, 0x12, 0x11, 0x4f, 0x48, 0x2f, 0x8f, 0x31, 0xb5, 0x4a, 0x0d, 0x65,
        0x71, 0xaf, 0x73, 0x44, 0x55,
    ];
    let res = test_msg_decode(&rx);
}
