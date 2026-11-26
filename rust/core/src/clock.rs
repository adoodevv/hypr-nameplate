//! The bar clock's text.

const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// "Wednesday 22:00", Omarchy's bar clock.
pub fn now_text() -> String {
    match crate::ffi::local_time() {
        Some((wday, hour, min)) => format_clock(wday, hour, min),
        None => String::new(),
    }
}

fn format_clock(wday: u32, hour: u32, min: u32) -> String {
    let day = DAYS.get(wday as usize).copied().unwrap_or("");
    format!("{day} {hour:02}:{min:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_omarchy() {
        assert_eq!(format_clock(3, 9, 5), "Wednesday 09:05");
        assert_eq!(format_clock(0, 23, 59), "Sunday 23:59");
    }

    #[test]
    fn reads_the_real_clock() {
        let text = now_text();
        let (day, hm) = text.split_once(' ').unwrap();
        assert!(DAYS.contains(&day));
        assert_eq!(hm.len(), 5);
    }
}
