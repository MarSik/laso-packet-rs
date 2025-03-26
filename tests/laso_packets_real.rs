pub(crate) use futures_lite::future::block_on;
use laso_packet::{
    behavior::decode_with_breaks,
    laso::LasoPacketType,
    message::{Message, MessageVersion},
    packet::PacketStatus,
    rx::{RxDecodeError, RxMessage},
    tx::MessageSender,
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

fn test_msg_reversal_w_rx_length<const N: usize>(msg: &Message<N>, tx_len: usize, rx_len: usize) {
    let mut wire_packets = Vec::new();
    let mut radio_packets: Vec<[u8; 32]> = Vec::new();

    // Encoding and transmit
    let mut sender = MessageSender::new(msg.clone());
    while sender.data_to_send() {
        let wire_packet = sender.packet();
        wire_packets.push(wire_packet.clone());
        radio_packets.push(wire_packet.encode_for_transmit().data());
    }

    assert_eq!(
        tx_len,
        radio_packets.len(),
        "Should have sent only {} radio packets (sent {})",
        tx_len,
        radio_packets.len()
    );

    while rx_len > radio_packets.len() {
        radio_packets.push([0xaa_u8; 32]);
    }

    // Reception and decode
    let mut rx: RxMessage<N> = RxMessage::default();
    for from_radio in radio_packets {
        let p = block_on(decode_with_breaks(&from_radio));
        assert!(p.parity_errors == 0);
        assert!(p.errors == 0);

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

    assert_eq!(*msg, rx.msg);
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
    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(res.msg.version, MessageVersion::V2Short);
}

#[test]
#[cfg(feature = "legacy")]
pub fn test_short_laso_rx_reversal_2packets() {
    let mut msg: Message<22> = Message::default();
    msg.source_address = 0x55;
    msg.packet_type = Some(LasoPacketType::GsmStatus.into());
    msg.version = MessageVersion::LegacyLaso;
    msg.add(0x01_u8);
    msg.add(0x0203_u16);
    // Padding
    for _ in 0..6 {
        msg.add(0x00_u8);
    }
    test_msg_reversal_w_rx_length(&msg, 1, 2);
}

#[test]
pub fn test_short_v2_rx_reversal_2packets() {
    let mut msg: Message<22> = Message::default();
    msg.source_address = 0x55;
    msg.packet_type = Some(LasoPacketType::GsmStatus.into());
    msg.version = MessageVersion::V2Short;
    msg.add(0x01_u8);
    msg.add(0x0203_u16);
    // Padding
    for _ in 0..5 {
        msg.add(0x00_u8);
    }
    test_msg_reversal_w_rx_length(&msg, 1, 2);
}
