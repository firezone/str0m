use std::collections::VecDeque;
use std::time::Duration;

use str0m::format::Codec;
use str0m::media::MediaKind;
use str0m::rtp::{ExtensionValues, Ssrc};
use str0m::{Event, RtcError};

mod common;
use common::{connect_l_r, init_crypto_default, init_log, progress};

#[test]
pub fn rtp_direct_mid_rid() -> Result<(), RtcError> {
    init_log();
    init_crypto_default();

    let (mut l, mut r) = connect_l_r();

    let mid = "aud".into();
    let rid = "hi".into();

    // In this example we are using MID and RID to identify the incoming media.
    let ssrc_tx: Ssrc = 42.into();

    l.direct_api().declare_media(mid, MediaKind::Audio);

    l.direct_api()
        .declare_stream_tx(ssrc_tx, None, mid, Some(rid));

    r.direct_api()
        .declare_media(mid, MediaKind::Audio)
        .expect_rid_rx(rid);

    let max = l.last.max(r.last);
    l.last = max;
    r.last = max;

    let params = l.params_opus();
    let ssrc = l.direct_api().stream_tx_by_mid(mid, None).unwrap().ssrc();
    assert_eq!(params.spec().codec, Codec::Opus);
    let pt = params.pt();

    let to_write: Vec<&[u8]> = vec![
        // 1
        &[0x1, 0x2, 0x3, 0x4],
        // 3
        &[0x9, 0xa, 0xb, 0xc],
        // 2
        &[0x5, 0x6, 0x7, 0x8],
    ];

    let mut to_write: VecDeque<_> = to_write.into();

    let mut write_at = l.last + Duration::from_millis(300);

    let mut counts: Vec<u64> = vec![0, 3, 1];

    loop {
        if l.start + l.duration() > write_at {
            write_at = l.last + Duration::from_millis(300);
            if let Some(packet) = to_write.pop_front() {
                let wallclock = l.start + l.duration();

                let mut direct = l.direct_api();
                let stream = direct.stream_tx(&ssrc).unwrap();

                let count = counts.remove(0);
                let time = (count * 1000 + 47_000_000) as u32;
                let seq_no = (47_000 + count).into();

                let exts = ExtensionValues {
                    audio_level: Some(-42 - count as i8),
                    voice_activity: Some(false),
                    ..Default::default()
                };

                stream
                    .write_rtp(
                        pt,
                        seq_no,
                        time,
                        wallclock,
                        false,
                        exts,
                        false,
                        packet.to_vec(),
                    )
                    .expect("clean write");
            }
        }

        progress(&mut l, &mut r)?;

        if l.duration() > Duration::from_secs(10) {
            break;
        }
    }

    let media: Vec<_> = r
        .events
        .iter()
        .filter_map(|(_, e)| {
            if let Event::RtpPacket(v) = e {
                Some(v)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(media.len(), 3);

    assert!(l.media(mid).is_some());
    assert!(l.direct_api().stream_tx_by_mid(mid, None).is_some());
    l.direct_api().remove_media(mid);
    assert!(l.media(mid).is_none());
    assert!(l.direct_api().stream_tx_by_mid(mid, None).is_none());

    assert!(r.media(mid).is_some());
    assert!(r.direct_api().stream_rx_by_mid(mid, None).is_some());
    r.direct_api().remove_media(mid);
    assert!(r.media(mid).is_none());
    assert!(r.direct_api().stream_rx_by_mid(mid, None).is_none());

    Ok(())
}
