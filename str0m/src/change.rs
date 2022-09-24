use std::ops::Deref;

use net_::Id;
use rtp::{Direction, Mid, Ssrc};
use sdp::{MediaLine, Msid, Offer};

use crate::media::{CodecConfig, CodecParams, MediaKind};
use crate::session::MediaOrChannel;
use crate::Rtc;

pub struct Changes(pub Vec<Change>);

#[derive(Debug)]
pub enum Change {
    AddMedia(AddMedia),
    AddChannel(Mid),
    ChangeDir(Mid, Direction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddMedia {
    pub mid: Mid,
    pub msid: Msid,
    pub kind: MediaKind,
    pub dir: Direction,
    pub ssrcs: Vec<(Ssrc, bool)>,

    // These are filled in when creating a Media from AddMedia
    pub params: Vec<CodecParams>,
    pub index: usize,
}

pub struct ChangeSet<'a> {
    rtc: &'a mut Rtc,
    changes: Changes,
}

impl<'a> ChangeSet<'a> {
    pub(crate) fn new(rtc: &'a mut Rtc) -> Self {
        ChangeSet {
            rtc,
            changes: Changes(vec![]),
        }
    }

    pub fn add_media(&mut self, kind: MediaKind, dir: Direction) -> Mid {
        let mid = self.rtc.new_mid();

        let ssrcs = {
            // For video we do RTX channels.
            let has_rtx = kind == MediaKind::Video;

            let ssrc_base = if has_rtx { 2 } else { 1 };

            // TODO: allow configuring simulcast
            let simulcast_count = 1;

            let ssrc_count = ssrc_base * simulcast_count;
            let mut v = Vec::with_capacity(ssrc_count);

            for i in 0..ssrc_count {
                // Allocate SSRC that are not in use in the session already.
                let is_rtx = has_rtx && i % 2 == 1;
                v.push((self.rtc.new_ssrc(), is_rtx));
            }

            v
        };

        // TODO: let user configure stream/track name.
        let msid = Msid {
            stream_id: Id::<30>::random().to_string(),
            track_id: Id::<30>::random().to_string(),
        };

        let add = AddMedia {
            mid,
            msid,
            kind,
            dir,
            ssrcs,

            // Added later
            params: vec![],
            index: 0,
        };

        self.changes.0.push(Change::AddMedia(add));
        mid
    }

    pub fn add_channel(&mut self) -> Mid {
        let mid = self.rtc.new_mid();
        self.changes.0.push(Change::AddChannel(mid));
        mid
    }

    pub fn set_direction(&mut self, mid: Mid, dir: Direction) {
        self.changes.0.push(Change::ChangeDir(mid, dir));
    }

    pub fn apply(self) -> Offer {
        self.rtc.set_changes(self.changes)
    }
}

impl Changes {
    /// Tests the given lines (from answer) corresponds to changes.
    pub fn ensure_correct_answer(&self, lines: &[&MediaLine]) -> Option<String> {
        if self.count_new_m_lines() != lines.len() {
            return Some(format!(
                "Differing m-line count in offer vs answer: {} != {}",
                self.count_new_m_lines(),
                lines.len()
            ));
        }

        'next: for l in lines {
            let mid = l.mid();

            for m in &self.0 {
                use Change::*;
                match m {
                    AddMedia(v) if v.mid == mid => {
                        if !l.typ.is_media() {
                            return Some(format!(
                                "Answer m-line for mid ({}) is not of media type: {:?}",
                                mid, l.typ
                            ));
                        }
                        continue 'next;
                    }
                    AddChannel(v) if *v == mid => {
                        if !l.typ.is_channel() {
                            return Some(format!(
                                "Answer m-line for mid ({}) is not a data channel: {:?}",
                                mid, l.typ
                            ));
                        }
                        continue 'next;
                    }
                    _ => {}
                }
            }

            return Some(format!("Mid in answer is not in offer: {}", mid));
        }

        None
    }

    fn count_new_m_lines(&self) -> usize {
        self.0
            .iter()
            .filter(|c| matches!(c, Change::AddMedia(_) | Change::AddChannel(_)))
            .count()
    }

    pub fn as_new_m_lines<'a, 'b: 'a>(
        &'a self,
        index_start: usize,
        config: &'b CodecConfig,
    ) -> impl Iterator<Item = MediaOrChannel> + '_ {
        self.0
            .iter()
            .enumerate()
            .filter_map(move |(idx, c)| c.as_new_m_line(index_start + idx, config))
    }

    pub(crate) fn apply_to(&self, lines: &mut [MediaLine]) {
        for change in &self.0 {
            use Change::*;
            match change {
                ChangeDir(mid, dir) => {
                    if let Some(line) = lines.iter_mut().find(|l| l.mid() == *mid) {
                        if let Some(dir_pos) = line.attrs.iter().position(|a| a.is_direction()) {
                            line.attrs[dir_pos] = (*dir).into();
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl Change {
    fn as_new_m_line(&self, index: usize, config: &CodecConfig) -> Option<MediaOrChannel> {
        use Change::*;
        match self {
            AddMedia(v) => {
                // TODO can we avoid all this cloning?
                let mut add = v.clone();
                add.params = config.all_for_kind(v.kind).map(|c| c.clone()).collect();
                add.index = index;

                let media = add.into();
                Some(MediaOrChannel::Media(media))
            }
            AddChannel(mid) => {
                let channel = (*mid, index).into();
                Some(MediaOrChannel::Channel(channel))
            }
            _ => None,
        }
    }
}

impl Deref for Changes {
    type Target = [Change];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}