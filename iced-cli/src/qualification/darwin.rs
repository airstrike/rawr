//! Private observer safe CoreGraphics and Accessibility predicates.

use super::protocol::{Bounds, Window};
use core_foundation::base::CFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::geometry::CGRect;

const WINDOW_LIMIT: usize = 4_096;

/// Failure to copy or decode the bounded system window list.
#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("CoreGraphics returned no window list")]
    Unavailable,
    #[error("CoreGraphics returned more than {limit} window rows")]
    Limit { limit: usize },
    #[error("CoreGraphics window row omitted or mistyped {field}")]
    Field { field: &'static str },
    #[error(
        "Screen Recording permission is absent for the benchmark process; exact CoreGraphics window titles are unavailable"
    )]
    ScreenCaptureAccess,
    #[error("Accessibility permission is absent for the benchmark process")]
    AccessibilityTrust,
    #[error("Accessibility retirement proof failed: {reason}")]
    Accessibility { reason: Box<str> },
}

/// Check required machine permissions without requesting or mutating either one.
pub(super) fn preflight() -> Result<(), Error> {
    preflight_permissions(
        || core_graphics::access::ScreenCaptureAccess::default().preflight(),
        axuielement::api_enabled,
        axuielement::is_process_trusted,
    )
}

fn preflight_permissions(
    screen_capture: impl FnOnce() -> bool,
    accessibility_api: impl FnOnce() -> bool,
    accessibility_trusted: impl FnOnce() -> bool,
) -> Result<(), Error> {
    if !screen_capture() {
        Err(Error::ScreenCaptureAccess)
    } else if accessibility_api() && accessibility_trusted() {
        Ok(())
    } else {
        Err(Error::AccessibilityTrust)
    }
}

/// Copy exact-title rows without granting visibility authority.
pub(super) fn scan(exact_title: &str) -> Result<Box<[Window]>, Error> {
    use core_graphics::window::{
        create_description_from_array, create_window_list, kCGNullWindowID,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
    };

    let identifiers = create_window_list(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )
    .ok_or(Error::Unavailable)?;
    let identifier_count = usize::try_from(identifiers.len()).map_err(|_| Error::Limit {
        limit: WINDOW_LIMIT,
    })?;
    if identifier_count > WINDOW_LIMIT {
        return Err(Error::Limit {
            limit: WINDOW_LIMIT,
        });
    }
    let descriptions = create_description_from_array(identifiers).ok_or(Error::Unavailable)?;
    decode_title_rows(
        descriptions.iter(),
        descriptions.iter().len(),
        exact_title,
        |description| optional_string(description, "kCGWindowName").map(String::into_boxed_str),
        |description, title| decode_window(&description, title),
    )
}

pub(super) fn visible_candidate(window: &Window, title: &str) -> bool {
    window.title.as_ref() == title
        && window.layer == 0
        && window.alpha > 0.0
        && window.on_screen
        && window.bounds.width > 0.0
        && window.bounds.height > 0.0
}

/// Press exactly one enabled close button on the exact authenticated window.
pub(super) fn press_close(
    expected: &Window,
    before_press: impl FnOnce() -> Result<(), Box<str>>,
) -> Result<(), Error> {
    use axuielement::ax_action::AX_PRESS_ACTION;
    use axuielement::ax_attribute::{
        AX_CLOSE_BUTTON_ATTRIBUTE, AX_ENABLED_ATTRIBUTE, AX_POSITION_ATTRIBUTE, AX_SIZE_ATTRIBUTE,
        AX_TITLE_ATTRIBUTE, AX_WINDOWS_ATTRIBUTE,
    };
    use axuielement::AXUIElement;

    let current = exact_current(expected)?;
    if current != *expected {
        return Err(accessibility(
            "CoreGraphics window identity changed before AX press",
        ));
    }
    let pid = i32::try_from(expected.pid)
        .map_err(|_| accessibility("runner PID does not fit the AX process type"))?;
    let application = AXUIElement::from_pid(pid)
        .ok_or_else(|| accessibility("AX application could not be created from runner PID"))?;
    application
        .set_timeout(1.0)
        .map_err(|error| accessibility(error.to_string()))?;
    if application
        .pid()
        .map_err(|error| accessibility(error.to_string()))?
        != pid
    {
        return Err(accessibility(
            "AX application PID differs from authenticated runner",
        ));
    }
    let windows = application
        .element_array_attribute(AX_WINDOWS_ATTRIBUTE)
        .map_err(|error| accessibility(error.to_string()))?;
    let mut matching = None;
    for window in windows {
        if window
            .pid()
            .map_err(|error| accessibility(error.to_string()))?
            != pid
        {
            return Err(accessibility(
                "AX window PID differs from authenticated runner",
            ));
        }
        let title = window
            .string_attribute(AX_TITLE_ATTRIBUTE)
            .map_err(|error| accessibility(error.to_string()))?;
        let position = window
            .point_attribute(AX_POSITION_ATTRIBUTE)
            .map_err(|error| accessibility(error.to_string()))?;
        let size = window
            .size_attribute(AX_SIZE_ATTRIBUTE)
            .map_err(|error| accessibility(error.to_string()))?;
        if title.as_deref() == Some(expected.title.as_ref())
            && position.is_some_and(|position| {
                position.x == expected.bounds.x && position.y == expected.bounds.y
            })
            && size.is_some_and(|size| {
                size.width == expected.bounds.width && size.height == expected.bounds.height
            })
        {
            if matching.replace(window).is_some() {
                return Err(accessibility("AX title/bounds window is ambiguous"));
            }
        }
    }
    let Some(window) = matching else {
        return Err(accessibility(
            "AX application did not expose exactly one title/bounds window",
        ));
    };
    let close = window
        .element_attribute(AX_CLOSE_BUTTON_ATTRIBUTE)
        .map_err(|error| accessibility(error.to_string()))?
        .ok_or_else(|| accessibility("AX window omitted its close button"))?;
    if close
        .pid()
        .map_err(|error| accessibility(error.to_string()))?
        != pid
        || close
            .bool_attribute(AX_ENABLED_ATTRIBUTE)
            .map_err(|error| accessibility(error.to_string()))?
            != Some(true)
    {
        return Err(accessibility(
            "AX close button has the wrong PID or is not enabled",
        ));
    }
    let actions = close
        .action_names()
        .map_err(|error| accessibility(error.to_string()))?;
    if actions
        .iter()
        .filter(|action| action.as_str() == AX_PRESS_ACTION)
        .count()
        != 1
    {
        return Err(accessibility(
            "AX close button does not support exactly one press action",
        ));
    }
    before_press().map_err(accessibility)?;
    close
        .perform_action(AX_PRESS_ACTION)
        .map_err(|error| accessibility(error.to_string()))
}

/// Observe whether the exact window remains, rejecting replacement or ambiguity.
pub(super) fn remains(expected: &Window) -> Result<bool, Error> {
    let windows = scan(&expected.title)?;
    remains_in(&windows, expected)
}

fn remains_in(windows: &[Window], expected: &Window) -> Result<bool, Error> {
    let exact = windows.iter().filter(|window| *window == expected).count();
    let named = windows
        .iter()
        .filter(|window| window.pid == expected.pid && window.title == expected.title)
        .count();
    match (exact, named, windows.len()) {
        (1, 1, 1) => Ok(true),
        (0, 0, 0) => Ok(false),
        _ => Err(accessibility(
            "CoreGraphics window was duplicated or replaced after AX press",
        )),
    }
}

fn exact_current(expected: &Window) -> Result<Window, Error> {
    let windows = scan(&expected.title)?;
    current_in(&windows, expected)
}

fn current_in(windows: &[Window], expected: &Window) -> Result<Window, Error> {
    let [window] = windows.as_ref() else {
        return Err(accessibility(
            "CoreGraphics did not retain one exact PID/title window",
        ));
    };
    if window.pid != expected.pid || window.title != expected.title {
        return Err(accessibility(
            "CoreGraphics did not retain one exact PID/title window",
        ));
    }
    Ok(window.clone())
}

fn decode_title_rows<T>(
    rows: impl IntoIterator<Item = T>,
    capacity: usize,
    exact_title: &str,
    mut title: impl FnMut(&T) -> Option<Box<str>>,
    mut decode: impl FnMut(T, Box<str>) -> Result<Window, Error>,
) -> Result<Box<[Window]>, Error> {
    let mut windows = Vec::new();
    windows
        .try_reserve_exact(capacity)
        .map_err(|_| Error::Limit {
            limit: WINDOW_LIMIT,
        })?;
    for row in rows {
        let Some(title) = title(&row) else {
            continue;
        };
        if title.as_ref() != exact_title {
            continue;
        }
        windows.push(decode(row, title)?);
    }
    Ok(windows.into_boxed_slice())
}

fn decode_window(
    description: &CFDictionary<CFString, CFType>,
    title: Box<str>,
) -> Result<Window, Error> {
    Ok(Window {
        number: number(description, "kCGWindowNumber")?
            .to_i64()
            .ok_or(Error::Field {
                field: "kCGWindowNumber",
            })?,
        pid: number(description, "kCGWindowOwnerPID")?
            .to_i64()
            .ok_or(Error::Field {
                field: "kCGWindowOwnerPID",
            })?,
        title,
        layer: number(description, "kCGWindowLayer")?
            .to_i64()
            .ok_or(Error::Field {
                field: "kCGWindowLayer",
            })?,
        alpha: number(description, "kCGWindowAlpha")?
            .to_f64()
            .ok_or(Error::Field {
                field: "kCGWindowAlpha",
            })?,
        on_screen: boolean(description, "kCGWindowIsOnscreen")?,
        bounds: bounds(description)?,
    })
}

pub(super) fn provisional_candidate(
    windows: &[Window],
    title: &str,
) -> Result<Option<Window>, Error> {
    if windows.len() > 1 {
        return Err(accessibility(
            "more than one provisional title window became visible",
        ));
    }
    Ok(windows
        .iter()
        .find(|window| visible_candidate(window, title))
        .cloned())
}

fn bounds(description: &CFDictionary<CFString, CFType>) -> Result<Bounds, Error> {
    let dictionary = value(description, "kCGWindowBounds")
        .and_then(|value| value.downcast::<CFDictionary>())
        .ok_or(Error::Field {
            field: "kCGWindowBounds",
        })?;
    let rectangle = CGRect::from_dict_representation(&dictionary).ok_or(Error::Field {
        field: "kCGWindowBounds",
    })?;
    let bounds = Bounds {
        x: rectangle.origin.x,
        y: rectangle.origin.y,
        width: rectangle.size.width,
        height: rectangle.size.height,
    };
    if !bounds.x.is_finite()
        || !bounds.y.is_finite()
        || !bounds.width.is_finite()
        || !bounds.height.is_finite()
        || bounds.width <= 0.0
        || bounds.height <= 0.0
    {
        return Err(Error::Field {
            field: "kCGWindowBounds",
        });
    }
    Ok(bounds)
}

fn accessibility(reason: impl Into<Box<str>>) -> Error {
    Error::Accessibility {
        reason: reason.into(),
    }
}

fn value(description: &CFDictionary<CFString, CFType>, key: &str) -> Option<CFType> {
    description
        .find(&CFString::new(key))
        .map(|value| (*value).clone())
}

fn number(
    description: &CFDictionary<CFString, CFType>,
    key: &'static str,
) -> Result<CFNumber, Error> {
    value(description, key)
        .and_then(|value| value.downcast::<CFNumber>())
        .ok_or(Error::Field { field: key })
}

fn boolean(description: &CFDictionary<CFString, CFType>, key: &'static str) -> Result<bool, Error> {
    value(description, key)
        .and_then(|value| value.downcast::<CFBoolean>())
        .map(bool::from)
        .ok_or(Error::Field { field: key })
}

fn optional_string(
    description: &CFDictionary<CFString, CFType>,
    key: &'static str,
) -> Option<String> {
    value(description, key)
        .and_then(|value| value.downcast::<CFString>())
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    #[test]
    fn permission_preflight_is_ordered_and_fail_closed() {
        let ax_calls = Cell::new(0);
        assert!(matches!(
            super::preflight_permissions(
                || false,
                || {
                    ax_calls.set(ax_calls.get() + 1);
                    false
                },
                || {
                    ax_calls.set(ax_calls.get() + 1);
                    false
                },
            ),
            Err(super::Error::ScreenCaptureAccess)
        ));
        assert_eq!(ax_calls.get(), 0);
        assert!(matches!(
            super::preflight_permissions(|| true, || false, || true),
            Err(super::Error::AccessibilityTrust)
        ));
        assert!(matches!(
            super::preflight_permissions(|| true, || true, || false),
            Err(super::Error::AccessibilityTrust)
        ));
        assert!(super::preflight_permissions(|| true, || true, || true).is_ok());
    }

    struct Row {
        title: Option<&'static str>,
        fields: Fields,
    }

    enum Fields {
        Valid { on_screen: bool },
        Malformed(&'static str),
    }

    fn decode(rows: Vec<Row>, exact_title: &str) -> Result<Box<[super::Window]>, super::Error> {
        let capacity = rows.len();
        super::decode_title_rows(
            rows,
            capacity,
            exact_title,
            |row| row.title.map(Into::into),
            |row, title| match row.fields {
                Fields::Valid { on_screen } => Ok(super::Window {
                    number: 1,
                    pid: 2,
                    title,
                    layer: 0,
                    alpha: 1.0,
                    on_screen,
                    bounds: super::Bounds {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                }),
                Fields::Malformed(field) => Err(super::Error::Field { field }),
            },
        )
    }

    #[test]
    fn unrelated_malformed_rows_are_ignored_before_candidate_decode() {
        let windows = decode(
            vec![
                Row {
                    title: Some("unrelated"),
                    fields: Fields::Malformed("kCGWindowIsOnscreen"),
                },
                Row {
                    title: None,
                    fields: Fields::Malformed("kCGWindowBounds"),
                },
                Row {
                    title: Some("exact"),
                    fields: Fields::Valid { on_screen: true },
                },
            ],
            "exact",
        )
        .expect("only the exact-title row enters governed field decoding");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].title.as_ref(), "exact");
    }

    #[test]
    fn exact_title_malformed_field_fails_closed() {
        let error = decode(
            vec![Row {
                title: Some("exact"),
                fields: Fields::Malformed("kCGWindowIsOnscreen"),
            }],
            "exact",
        )
        .expect_err("an exact-title row must decode every governed field");
        assert!(matches!(
            error,
            super::Error::Field {
                field: "kCGWindowIsOnscreen"
            }
        ));
    }

    #[test]
    fn exact_title_false_onscreen_is_retained_then_predicate_rejected() {
        let windows = decode(
            vec![Row {
                title: Some("exact"),
                fields: Fields::Valid { on_screen: false },
            }],
            "exact",
        )
        .expect("false on-screen is a decoded fact, not an inferred one");
        assert_eq!(windows.len(), 1);
        assert!(!windows[0].on_screen);
        assert!(super::provisional_candidate(&windows, "exact")
            .expect("one decoded title row is unambiguous")
            .is_none());
    }

    #[test]
    fn multiple_exact_title_rows_remain_ambiguous() {
        let windows = decode(
            vec![
                Row {
                    title: Some("exact"),
                    fields: Fields::Valid { on_screen: true },
                },
                Row {
                    title: Some("exact"),
                    fields: Fields::Valid { on_screen: false },
                },
            ],
            "exact",
        )
        .expect("all exact-title rows decode before ambiguity is evaluated");
        assert_eq!(windows.len(), 2);
        let expected = windows[0].clone();
        assert!(super::provisional_candidate(&windows, "exact").is_err());
        assert!(super::current_in(&windows, &expected).is_err());
        assert!(super::remains_in(&windows, &expected).is_err());
    }

    #[test]
    fn both_timing_modes_use_one_exact_window_predicate_and_poll_cadence() {
        let candidate = super::Window {
            number: 1,
            pid: 2,
            title: "exact".into(),
            layer: 0,
            alpha: 1.0,
            on_screen: true,
            bounds: super::Bounds {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        };
        assert!(super::visible_candidate(&candidate, "exact"));
        assert!(!super::visible_candidate(&candidate, "wrong"));

        let mut rejected = candidate.clone();
        rejected.layer = 1;
        assert!(!super::visible_candidate(&rejected, "exact"));

        let mut rejected = candidate.clone();
        rejected.alpha = 0.0;
        assert!(!super::visible_candidate(&rejected, "exact"));
        rejected.alpha = -1.0;
        assert!(!super::visible_candidate(&rejected, "exact"));

        let mut rejected = candidate.clone();
        rejected.on_screen = false;
        assert!(!super::visible_candidate(&rejected, "exact"));

        let mut rejected = candidate.clone();
        rejected.bounds.width = 0.0;
        assert!(!super::visible_candidate(&rejected, "exact"));
        rejected.bounds.width = -1.0;
        assert!(!super::visible_candidate(&rejected, "exact"));

        let mut rejected = candidate;
        rejected.bounds.height = 0.0;
        assert!(!super::visible_candidate(&rejected, "exact"));
        rejected.bounds.height = -1.0;
        assert!(!super::visible_candidate(&rejected, "exact"));
    }
}
