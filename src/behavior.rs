use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::packet::{GolayDecoderResult, PacketWithGolay, PacketWithInterleave, PacketWithoutDC};

pub async fn decode_with_breaks(packet: &[u8; 32]) -> GolayDecoderResult {
    let p = PacketWithoutDC::new(packet);
    let p2 = PacketWithInterleave::from(&p);

    yield_now().await;

    let p3 = PacketWithGolay::from(&p2);

    yield_now().await;

    GolayDecoderResult::from(&p3)
}

struct Yield(bool);

async fn yield_now() {
    Yield::default().await;
}

impl Yield {
    fn default() -> Yield {
        Yield(false)
    }
}

impl Future for Yield {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.0 {
            self.get_mut().0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}
