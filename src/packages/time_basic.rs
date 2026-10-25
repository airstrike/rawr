use super::arithmetic::make_err as make_arithmetic_err;
use crate::plugin::*;
use crate::{def_package, RawrResult, RawrResultOf, INT};
use std::convert::TryFrom;

use crate::FLOAT;

#[cfg(any(not(target_family = "wasm"), not(target_os = "unknown")))]
use std::time::{Duration, Instant};

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use web_time::{Duration, Instant};

def_package! {
    /// Package of basic timing utilities.
    pub BasicTimePackage(lib) {
        lib.set_standard_lib(true);

        // Register date/time functions
        combine_with_exported_module!(lib, "time", time_functions);
    }
}

#[export_module]
mod time_functions {
    /// Create a timestamp containing the current system time.
    ///
    /// # Example
    ///
    /// ```rawr
    /// let now = timestamp();
    ///
    /// sleep(10.0);            // sleep for 10 seconds
    ///
    /// print(now.elapsed);     // prints 10.???
    /// ```
    #[rawr_fn(volatile)]
    pub fn timestamp() -> Instant {
        Instant::now()
    }

    /// Return the number of seconds between the current system time and the timestamp.
    ///
    /// # Example
    ///
    /// ```rawr
    /// let now = timestamp();
    ///
    /// sleep(10.0);            // sleep for 10 seconds
    ///
    /// print(now.elapsed);     // prints 10.???
    /// ```
    #[rawr_fn(name = "elapsed", get = "elapsed", return_raw)]
    pub fn elapsed(timestamp: Instant) -> RawrResult {
        if timestamp > Instant::now() {
            Err(make_arithmetic_err("Time-stamp is later than now"))
        } else {
            Ok((timestamp.elapsed().as_secs_f64() as FLOAT).into())
        }
    }

    /// Return the number of seconds between two timestamps.
    #[rawr_fn(return_raw, name = "-")]
    #[allow(clippy::unnecessary_wraps)]
    pub fn time_diff(timestamp1: Instant, timestamp2: Instant) -> RawrResult {
        Ok(if timestamp2 > timestamp1 {
            -(timestamp2 - timestamp1).as_secs_f64() as FLOAT
        } else {
            (timestamp1 - timestamp2).as_secs_f64() as FLOAT
        }
        .into())
    }

    pub mod float_functions {
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        fn add_impl(timestamp: Instant, seconds: FLOAT) -> RawrResultOf<Instant> {
            if seconds < 0.0 {
                return subtract_impl(timestamp, -seconds);
            }
            #[allow(clippy::cast_precision_loss)]
            if seconds > (INT::MAX as FLOAT).min(u64::MAX as FLOAT) {
                return Err(make_arithmetic_err(format!(
                    "Integer overflow for timestamp add: {seconds}"
                )));
            }

            timestamp
                .checked_add(Duration::from_millis((seconds * 1000.0) as u64))
                .ok_or_else(|| {
                    make_arithmetic_err(format!(
                        "Timestamp overflow when adding {seconds} second(s)"
                    ))
                })
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        fn subtract_impl(timestamp: Instant, seconds: FLOAT) -> RawrResultOf<Instant> {
            if seconds < 0.0 {
                return add_impl(timestamp, -seconds);
            }
            #[allow(clippy::cast_precision_loss)]
            if seconds > (INT::MAX as FLOAT).min(u64::MAX as FLOAT) {
                return Err(make_arithmetic_err(format!(
                    "Integer overflow for timestamp subtract: {seconds}"
                )));
            }

            timestamp
                .checked_sub(Duration::from_millis((seconds * 1000.0) as u64))
                .ok_or_else(|| {
                    make_arithmetic_err(format!(
                        "Timestamp overflow when subtracting {seconds} second(s)"
                    ))
                })
        }

        /// Add the specified number of `seconds` to the timestamp and return it as a new timestamp.
        #[rawr_fn(return_raw, name = "+")]
        pub fn add(timestamp: Instant, seconds: FLOAT) -> RawrResultOf<Instant> {
            add_impl(timestamp, seconds)
        }
        /// Add the specified number of `seconds` to the timestamp.
        #[rawr_fn(return_raw, name = "+=")]
        pub fn add_assign(timestamp: &mut Instant, seconds: FLOAT) -> RawrResultOf<()> {
            *timestamp = add_impl(*timestamp, seconds)?;
            Ok(())
        }
        /// Subtract the specified number of `seconds` from the timestamp and return it as a new timestamp.
        #[rawr_fn(return_raw, name = "-")]
        pub fn subtract(timestamp: Instant, seconds: FLOAT) -> RawrResultOf<Instant> {
            subtract_impl(timestamp, seconds)
        }
        /// Subtract the specified number of `seconds` from the timestamp.
        #[rawr_fn(return_raw, name = "-=")]
        pub fn subtract_assign(timestamp: &mut Instant, seconds: FLOAT) -> RawrResultOf<()> {
            *timestamp = subtract_impl(*timestamp, seconds)?;
            Ok(())
        }
    }

    #[inline(always)]
    fn add_inner(timestamp: Instant, seconds: u64) -> Option<Instant> {
        timestamp.checked_add(Duration::from_secs(seconds))
    }
    #[inline]
    fn add_impl(timestamp: Instant, seconds: INT) -> RawrResultOf<Instant> {
        if seconds < 0 {
            #[allow(clippy::useless_conversion)]
            subtract_inner(timestamp, u64::try_from(seconds.unsigned_abs()).unwrap())
        } else {
            add_inner(timestamp, u64::try_from(seconds).unwrap())
        }
        .ok_or_else(|| {
            make_arithmetic_err(format!(
                "Timestamp overflow when adding {seconds} second(s)"
            ))
        })
    }
    #[inline(always)]
    fn subtract_inner(timestamp: Instant, seconds: u64) -> Option<Instant> {
        timestamp.checked_sub(Duration::from_secs(seconds))
    }
    #[inline]
    fn subtract_impl(timestamp: Instant, seconds: INT) -> RawrResultOf<Instant> {
        if seconds < 0 {
            add_inner(timestamp, seconds.unsigned_abs())
        } else {
            subtract_inner(timestamp, u64::try_from(seconds).unwrap())
        }
        .ok_or_else(|| {
            make_arithmetic_err(format!(
                "Timestamp overflow when subtracting {seconds} second(s)"
            ))
        })
    }

    /// Add the specified number of `seconds` to the timestamp and return it as a new timestamp.
    #[rawr_fn(return_raw, name = "+")]
    pub fn add(timestamp: Instant, seconds: INT) -> RawrResultOf<Instant> {
        add_impl(timestamp, seconds)
    }
    /// Add the specified number of `seconds` to the timestamp.
    #[rawr_fn(return_raw, name = "+=")]
    pub fn add_assign(timestamp: &mut Instant, seconds: INT) -> RawrResultOf<()> {
        *timestamp = add_impl(*timestamp, seconds)?;
        Ok(())
    }
    /// Subtract the specified number of `seconds` from the timestamp and return it as a new timestamp.
    #[rawr_fn(return_raw, name = "-")]
    pub fn subtract(timestamp: Instant, seconds: INT) -> RawrResultOf<Instant> {
        subtract_impl(timestamp, seconds)
    }
    /// Subtract the specified number of `seconds` from the timestamp.
    #[rawr_fn(return_raw, name = "-=")]
    pub fn subtract_assign(timestamp: &mut Instant, seconds: INT) -> RawrResultOf<()> {
        *timestamp = subtract_impl(*timestamp, seconds)?;
        Ok(())
    }

    /// Return `true` if two timestamps are equal.
    #[rawr_fn(name = "==")]
    pub fn eq(timestamp1: Instant, timestamp2: Instant) -> bool {
        timestamp1 == timestamp2
    }
    /// Return `true` if two timestamps are not equal.
    #[rawr_fn(name = "!=")]
    pub fn ne(timestamp1: Instant, timestamp2: Instant) -> bool {
        timestamp1 != timestamp2
    }
    /// Return `true` if the first timestamp is earlier than the second.
    #[rawr_fn(name = "<")]
    pub fn lt(timestamp1: Instant, timestamp2: Instant) -> bool {
        timestamp1 < timestamp2
    }
    /// Return `true` if the first timestamp is earlier than or equals to the second.
    #[rawr_fn(name = "<=")]
    pub fn lte(timestamp1: Instant, timestamp2: Instant) -> bool {
        timestamp1 <= timestamp2
    }
    /// Return `true` if the first timestamp is later than the second.
    #[rawr_fn(name = ">")]
    pub fn gt(timestamp1: Instant, timestamp2: Instant) -> bool {
        timestamp1 > timestamp2
    }
    /// Return `true` if the first timestamp is later than or equals to the second.
    #[rawr_fn(name = ">=")]
    pub fn gte(timestamp1: Instant, timestamp2: Instant) -> bool {
        timestamp1 >= timestamp2
    }
}
