use crate::{Admission, Work};

#[derive(Default)]
pub(crate) struct Audit {
    pub(crate) events: Vec<Work>,
    pub(crate) reject_at: Option<usize>,
    pub(crate) visible_limit: Option<usize>,
}

impl Admission for Audit {
    type Error = ();

    fn admit(&mut self, work: Work) -> Result<(), Self::Error> {
        let index = self.events.len();
        self.events.push(work);
        if self.reject_at == Some(index)
            || matches!(work, Work::Visible { entries } if self.visible_limit.is_some_and(|limit| entries > limit))
        {
            Err(())
        } else {
            Ok(())
        }
    }
}

/// One native record in the target's layout. See ../SAFETY.md.
#[cfg(target_os = "macos")]
pub(crate) fn record(name: &[u8], inode: u64, kind: u8) -> Vec<u8> {
    let length = (21 + name.len() + 1 + 3) & !3;
    let mut bytes = vec![0; length];
    bytes[..8].copy_from_slice(&inode.to_ne_bytes());
    bytes[8..16].copy_from_slice(&0xfedc_ba98_7654_3210u64.to_ne_bytes());
    bytes[16..18].copy_from_slice(&u16::try_from(length).unwrap().to_ne_bytes());
    bytes[18..20].copy_from_slice(&u16::try_from(name.len()).unwrap().to_ne_bytes());
    bytes[20] = kind;
    bytes[21..21 + name.len()].copy_from_slice(name);
    bytes
}

/// One linux_dirent64, its padding after the terminator left nonzero as stale
/// buffer bytes may be. See ../SAFETY.md.
#[cfg(not(target_os = "macos"))]
pub(crate) fn record(name: &[u8], inode: u64, kind: u8) -> Vec<u8> {
    let length = (19 + name.len() + 1).next_multiple_of(8);
    let mut bytes = vec![0xa5; length];
    bytes[..8].copy_from_slice(&inode.to_ne_bytes());
    bytes[8..16].copy_from_slice(&0x7edc_ba98_7654_3210u64.to_ne_bytes());
    bytes[16..18].copy_from_slice(&u16::try_from(length).unwrap().to_ne_bytes());
    bytes[18] = kind;
    bytes[19..19 + name.len()].copy_from_slice(name);
    bytes[19 + name.len()] = 0;
    bytes
}
