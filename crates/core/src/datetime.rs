use jiff::{fmt::strtime, tz::TimeZone, ZonedDateTime};

pub(crate) fn parse_local_datetime(
    input: &str,
    local_formats: &[&str],
    offset_formats: &[&str],
    time_zone: &TimeZone,
) -> Option<ZonedDateTime> {
    let with_offset = offset_formats.iter().find_map(|format| {
        let (parsed, length) = strtime::BrokenDownTime::parse_prefix(format, input).ok()?;
        let datetime = parsed.to_datetime().ok()?;
        let remainder = input.get(length..)?.trim_start();
        let offset = strtime::parse("%:z", remainder)
            .or_else(|_| strtime::parse("%z", remainder))
            .ok()?
            .offset()?;
        let timestamp = offset.to_timestamp(datetime).ok()?;
        Some(timestamp.to_zoned_datetime(time_zone.clone()))
    });

    with_offset.or_else(|| {
        local_formats.iter().find_map(|format| {
            let datetime = jiff::civil::DateTime::strptime(format, input).ok()?;
            // A wall-clock time can be missing or repeated during a DST transition.
            time_zone
                .to_ambiguous_zoned_datetime(datetime)
                .unambiguous()
                .ok()
        })
    })
}

pub(crate) mod local_rfc3339 {
    use jiff::{tz::TimeZone, Timestamp, ZonedDateTime};
    use serde::{de, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(date: &ZonedDateTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Keep the CLI and GUI JSON format used by previously serialized plans.
        let nanos = date.subsec_nanosecond();
        let format = if nanos == 0 {
            "%Y-%m-%dT%H:%M:%S"
        } else if nanos % 1_000_000 == 0 {
            "%Y-%m-%dT%H:%M:%S%.3f"
        } else if nanos % 1_000 == 0 {
            "%Y-%m-%dT%H:%M:%S%.6f"
        } else {
            "%Y-%m-%dT%H:%M:%S%.9f"
        };
        let datetime = date.strftime(format);
        let seconds = date.offset().seconds();
        if seconds == 0 {
            return serializer.collect_str(&format_args!("{datetime}Z"));
        }

        // The previous RFC 3339 formatter rounded historical offsets to whole minutes.
        let sign = if seconds < 0 { '-' } else { '+' };
        let minutes = (seconds.abs() + 30) / 60;
        serializer.collect_str(&format_args!(
            "{datetime}{sign}{:02}:{:02}",
            minutes / 60,
            minutes % 60,
        ))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ZonedDateTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let date = String::deserialize(deserializer)?;
        let timestamp = date.parse::<Timestamp>().map_err(de::Error::custom)?;
        Ok(timestamp.to_zoned_datetime(TimeZone::system()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn interprets_offset_and_naive_metadata_in_the_local_zone() {
        let zone = TimeZone::get("Asia/Tokyo").unwrap();
        let formats = ["%Y-%m-%dT%H:%M:%S%.f"];
        for input in [
            "2026-02-08T00:20:30.123-05:00",
            "2026-02-08T00:20:30.123-0500",
            "2026-02-08T14:20:30.123",
        ] {
            let result = parse_local_datetime(input, &formats, &formats, &zone).unwrap();
            assert_eq!(
                result.datetime(),
                date(2026, 2, 8).at(14, 20, 30, 123_000_000)
            );
        }
    }

    #[test]
    fn rejects_invalid_and_ambiguous_wall_clock_times() {
        let zone = TimeZone::get("America/New_York").unwrap();
        let formats = ["%Y-%m-%dT%H:%M:%S%.f"];
        for input in [
            "2026-02-30T01:30:00",
            "2026-03-08T02:30:00",
            "2026-11-01T01:30:00",
            "2026-11-01T01:30:00+invalid",
        ] {
            assert!(
                parse_local_datetime(input, &formats, &formats, &zone).is_none(),
                "{input}"
            );
        }
        assert!(
            parse_local_datetime("2026-11-01T01:30:00-05:00", &formats, &formats, &zone).is_some()
        );
    }
}
