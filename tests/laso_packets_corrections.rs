use futures_lite::future::block_on;
use laso_packet::{
    behavior::decode_with_breaks,
    laso::LasoPacketType,
    message::{Message, MessageVersion},
    rx::RxMessage,
    tx::MessageSender,
};

fn test_msg_reversal_w_corruption(msg: &Message<22>) {
    let mut wire_packets = Vec::new();
    let mut radio_packets = Vec::new();

    // Encoding and transmit
    let mut sender = MessageSender::new(msg.clone());
    while sender.data_to_send() {
        let wire_packet = sender.packet();
        wire_packets.push(wire_packet.clone());
        let mut radio_data = wire_packet.encode_for_transmit().data();

        // Simulate a burst error affecting 1/8 of the message
        // TODO more sofisticated test
        radio_data[3] = 0xff;
        radio_data[4] = 0xff;
        radio_data[5] = 0xff;
        radio_data[6] = 0xff;

        radio_packets.push(radio_data);
    }

    // Reception and decode
    let mut rx: RxMessage<22> = RxMessage::default();
    for from_radio in radio_packets {
        let p = block_on(decode_with_breaks(&from_radio));

        if let Err(err) = rx.append(&p) {
            panic!("Rx decode error: {:?}", err);
        }
    }

    assert_eq!(*msg, rx.msg);
}

#[test]
pub fn test_short_laso_reversal_w_noise() {
    let mut msg: Message<22> = Message::default();
    msg.source_address = 0x55;
    msg.packet_type = Some(LasoPacketType::GsmStatus.into());
    msg.add(0x01_u8);
    msg.add(0x0203_u16);
    // Padding
    for _ in 0..6 {
        msg.add(0x00_u8);
    }
    test_msg_reversal_w_corruption(&msg);
}

#[test]
pub fn test_long_laso_reversal_w_noise() {
    let mut msg: Message<22> = Message::default();
    msg.source_address = 0x55;
    msg.packet_type = Some(LasoPacketType::GsmStatus.into());
    msg.add(0x01_u8);
    msg.add(0x0203_u16);
    msg.add(0x0405_u16);
    msg.add(0x0607_u16);
    msg.add(0x0809_u16);
    msg.add(0x0a0b_u16);
    // Padding
    for _ in 0..9 {
        msg.add(0x00_u8);
    }
    test_msg_reversal_w_corruption(&msg);
}

#[test]
pub fn test_short_v2_reversal_w_noise() {
    let mut msg: Message<22> = Message::default();
    msg.source_address = 0x55;
    msg.packet_type = Some(LasoPacketType::GsmStatus.into());
    msg.version = MessageVersion::V2;
    msg.add(0x01_u8);
    msg.add(0x0203_u16);
    // Padding
    for _ in 0..6 {
        msg.add(0x00_u8);
    }
    test_msg_reversal_w_corruption(&msg);
}

#[test]
pub fn test_long_v2_reversal_w_noise() {
    let mut msg: Message<22> = Message::default();
    msg.source_address = 0x55;
    msg.packet_type = Some(LasoPacketType::GsmStatus.into());
    msg.version = MessageVersion::V2;
    msg.add(0x01_u8);
    msg.add(0x0203_u16);
    msg.add(0x0405_u16);
    msg.add(0x0607_u16);
    msg.add(0x0809_u16);
    msg.add(0x0a0b_u16);
    // Padding
    for _ in 0..9 {
        msg.add(0x00_u8);
    }
    test_msg_reversal_w_corruption(&msg);
}
