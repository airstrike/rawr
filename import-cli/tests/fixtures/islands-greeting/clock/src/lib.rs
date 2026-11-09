//! A stand-in for `chrono` with its shapes: `Local::now()` gives a
//! `DateTime<Local>` by value, whose `hour` is the `Timelike` trait's,
//! implemented for every time zone. Its time is fixed.

pub trait TimeZone {}

#[derive(Debug, Clone, Copy)]
pub struct Local;

impl TimeZone for Local {}

#[derive(Debug, Clone, Copy)]
pub struct DateTime<Tz: TimeZone> {
    seconds: u32,
    zone: Tz,
}

impl Local {
    pub fn now() -> DateTime<Local> {
        DateTime {
            seconds: 14 * 3600 + 5 * 60,
            zone: Local,
        }
    }
}

impl<Tz: TimeZone> DateTime<Tz> {
    pub fn timezone(&self) -> &Tz {
        &self.zone
    }
}

pub trait Timelike {
    fn hour(&self) -> u32;
    fn minute(&self) -> u32;
}

impl<Tz: TimeZone> Timelike for DateTime<Tz> {
    fn hour(&self) -> u32 {
        self.seconds / 3600
    }

    fn minute(&self) -> u32 {
        self.seconds / 60 % 60
    }
}
