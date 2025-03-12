use heapless::Vec;

// Raw data received from the radio
#[derive(Clone)]
pub struct RawReceiveData<const N: usize> {
    pub packet: Vec<u8, N>,
    pub lna: u8,
    pub rssi: u8,
}

impl<const N: usize> RawReceiveData<N> {
    pub fn init() -> Self {
        Self {
            packet: Vec::new(),
            lna: 0,
            rssi: 0,
        }
    }

    pub fn clear(&mut self) {
        self.packet.clear();
    }
}
