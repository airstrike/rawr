//! Keep a background hot-reload owner at full speed while it prepares and
//! installs one edit. macOS otherwise naps an unfocused app after about half a
//! minute, which slows the compile several times over.

/// Held from a reload's begin to its installation; dropping it ends it.
pub(super) struct Activity {
    #[cfg(target_os = "macos")]
    _token: macos::Token,
}

impl Activity {
    pub(super) fn begin() -> Option<Self> {
        #[cfg(target_os = "macos")]
        return macos::Token::begin().map(|token| Self { _token: token });
        #[cfg(not(target_os = "macos"))]
        return Some(Self {});
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_char, c_void, CStr};

    type Object = *mut c_void;
    type Selector = *mut c_void;

    #[link(name = "Foundation", kind = "framework")]
    unsafe extern "C" {}

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Object;
        fn sel_registerName(name: *const c_char) -> Selector;
        fn objc_msgSend();
        fn objc_autoreleasePoolPush() -> *mut c_void;
        fn objc_autoreleasePoolPop(pool: *mut c_void);
    }

    /// `NSActivityUserInitiatedAllowingIdleSystemSleep`.
    const USER_INITIATED: u64 = 0x00FF_FFFF & !(1 << 20);

    pub(super) struct Token(Object);

    // The token is an immutable Foundation object; NSProcessInfo is thread safe.
    unsafe impl Send for Token {}

    fn selector(name: &CStr) -> Selector {
        // SAFETY: a NUL-terminated selector name; registration is idempotent.
        unsafe { sel_registerName(name.as_ptr()) }
    }

    impl Token {
        pub(super) fn begin() -> Option<Self> {
            // SAFETY: each call matches the Objective-C method signature it
            // sends; the autoreleased results are used inside this pool, and
            // the token is retained before the pool drains.
            unsafe {
                let pool = objc_autoreleasePoolPush();
                let send: unsafe extern "C" fn(Object, Selector) -> Object =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                let string: unsafe extern "C" fn(Object, Selector, *const c_char) -> Object =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                let begin: unsafe extern "C" fn(Object, Selector, u64, Object) -> Object =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                let info = send(
                    objc_getClass(c"NSProcessInfo".as_ptr()),
                    selector(c"processInfo"),
                );
                let reason = string(
                    objc_getClass(c"NSString".as_ptr()),
                    selector(c"stringWithUTF8String:"),
                    c"Rawr hot reload".as_ptr(),
                );
                let token = if info.is_null() || reason.is_null() {
                    std::ptr::null_mut()
                } else {
                    let token = begin(
                        info,
                        selector(c"beginActivityWithOptions:reason:"),
                        USER_INITIATED,
                        reason,
                    );
                    if token.is_null() {
                        token
                    } else {
                        send(token, selector(c"retain"))
                    }
                };
                objc_autoreleasePoolPop(pool);
                (!token.is_null()).then_some(Self(token))
            }
        }
    }

    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: the token came from beginActivityWithOptions:reason: and
            // was retained once; it is ended once and released once.
            unsafe {
                let pool = objc_autoreleasePoolPush();
                let send: unsafe extern "C" fn(Object, Selector) -> Object =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                let end: unsafe extern "C" fn(Object, Selector, Object) =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                let info = send(
                    objc_getClass(c"NSProcessInfo".as_ptr()),
                    selector(c"processInfo"),
                );
                if !info.is_null() {
                    end(info, selector(c"endActivity:"), self.0);
                }
                send(self.0, selector(c"release"));
                objc_autoreleasePoolPop(pool);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn activity_begins_and_ends() {
        let activity = super::Activity::begin();
        assert!(activity.is_some());
        drop(activity);
    }
}
