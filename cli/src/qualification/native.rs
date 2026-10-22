//! The observer's only owned native adapter. See CONTRACT.md for exact SDK/source scope.

use std::mem::{align_of, offset_of, size_of};

use super::protocol::{Error, Identity};

const BSD_INFO: i32 = 3;
const PATH_BYTES: usize = 4_096;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
struct Bsd {
    flags: u32,
    status: u32,
    exit_status: u32,
    pid: u32,
    parent: u32,
    uid: u32,
    gid: u32,
    real_uid: u32,
    real_gid: u32,
    saved_uid: u32,
    saved_gid: u32,
    reserved: u32,
    command: [u8; 16],
    name: [u8; 32],
    files: u32,
    group: u32,
    job_count: u32,
    terminal: u32,
    terminal_group: u32,
    nice: i32,
    birth_seconds: u64,
    birth_microseconds: u64,
}

const _: () = {
    assert!(size_of::<Bsd>() == 136);
    assert!(align_of::<Bsd>() == 8);
    assert!(offset_of!(Bsd, status) == 4);
    assert!(offset_of!(Bsd, pid) == 12);
    assert!(offset_of!(Bsd, parent) == 16);
    assert!(offset_of!(Bsd, uid) == 20);
    assert!(offset_of!(Bsd, group) == 100);
    assert!(offset_of!(Bsd, birth_seconds) == 120);
    assert!(offset_of!(Bsd, birth_microseconds) == 128);
};

#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        argument: u64,
        buffer: *mut std::ffi::c_void,
        size: i32,
    ) -> i32;
    fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, size: u32) -> i32;
}

/// Bracket the executable path with two complete live identity observations.
pub(super) fn observe(pid: u32) -> Result<Identity, Error> {
    let first = bsd(pid)?;
    let signed = i32::try_from(pid).map_err(|_| Error::Invalid("PID outside Darwin range"))?;
    let mut path = [0_u8; PATH_BYTES];
    // SAFETY: the exact SDK signature writes at most size bytes into this fully
    // initialized, aligned, live array. No pointer escapes the synchronous call.
    clear_errno();
    let result = unsafe { proc_pidpath(signed, path.as_mut_ptr().cast(), PATH_BYTES as u32) };
    let errno = if result <= 0 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    } else {
        0
    };
    let path = decode_path(&path, result, errno)?;
    let second = bsd(pid)?;
    if key(&first) != key(&second) {
        return Err(Error::Invalid(
            "process identity changed around executable path",
        ));
    }
    Ok(Identity {
        pid,
        parent: first.parent,
        group: first.group,
        uid: first.uid,
        birth_seconds: first.birth_seconds,
        birth_microseconds: first.birth_microseconds,
        path,
    })
}

fn clear_errno() {
    // SAFETY: Darwin's __error returns this thread's valid errno cell; it is
    // written only to distinguish a zero API result from stale prior errno.
    unsafe {
        *libc::__error() = 0;
    }
}

fn key(info: &Bsd) -> (u32, u32, u32, u32, u64, u64) {
    (
        info.pid,
        info.parent,
        info.group,
        info.uid,
        info.birth_seconds,
        info.birth_microseconds,
    )
}

fn bsd(pid: u32) -> Result<Bsd, Error> {
    let signed = i32::try_from(pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or(Error::Invalid("invalid Darwin PID"))?;
    let mut info = Bsd::default();
    // SAFETY: Bsd is the asserted 136-byte C layout of proc_bsdinfo. All bytes
    // are initialized before the call, the native buffer is exactly its size,
    // and no field is consumed unless the complete byte count is returned.
    clear_errno();
    let result = unsafe {
        proc_pidinfo(
            signed,
            BSD_INFO,
            0,
            (&mut info as *mut Bsd).cast(),
            size_of::<Bsd>() as i32,
        )
    };
    let errno = if result <= 0 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    } else {
        0
    };
    complete(result, errno)?;
    if info.pid != pid
        || !(1..=4).contains(&info.status)
        || info.parent == 0
        || info.group == 0
        || info.birth_seconds == 0
        || info.birth_microseconds >= 1_000_000
    {
        return Err(Error::Invalid(
            "incomplete or non-live BSD process identity",
        ));
    }
    Ok(info)
}

fn complete(result: i32, errno: i32) -> Result<(), Error> {
    if result == size_of::<Bsd>() as i32 {
        Ok(())
    } else {
        Err(Error::Native {
            operation: "proc_pidinfo/BSDInfo",
            errno: if result <= 0 { errno } else { 0 },
            result,
        })
    }
}

fn decode_path(buffer: &[u8; PATH_BYTES], result: i32, errno: i32) -> Result<Box<[u8]>, Error> {
    if result <= 0 {
        return Err(Error::Native {
            operation: "proc_pidpath",
            errno,
            result,
        });
    }
    let length = result as usize;
    if length >= PATH_BYTES
        || buffer[length] != 0
        || buffer[..length].contains(&0)
        || buffer[0] != b'/'
    {
        return Err(Error::Invalid(
            "proc_pidpath returned a truncated or malformed pathname",
        ));
    }
    let mut path = Vec::new();
    path.try_reserve_exact(length)
        .map_err(|_| Error::Invalid("native pathname allocation"))?;
    path.extend_from_slice(&buffer[..length]);
    Ok(path.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_complete_native_results_are_consumed() {
        for result in [-1, 0, 1, 135, 137] {
            assert!(super::complete(result, 5).is_err());
        }
        assert!(super::complete(136, 5).is_ok());
        let mut path = [0; super::PATH_BYTES];
        path[..7].copy_from_slice(b"/runner");
        assert_eq!(&*super::decode_path(&path, 7, 0).unwrap(), b"/runner");
        for length in [0, -1, 6, 8, 4096] {
            assert!(super::decode_path(&path, length, 5).is_err());
        }
        path[7] = b'x';
        assert!(super::decode_path(&path, 7, 0).is_err());
    }
}
