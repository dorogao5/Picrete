use time::{format_description::well_known::Rfc3339, OffsetDateTime, PrimitiveDateTime, UtcOffset};

pub(crate) fn primitive_now_utc() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}

pub(crate) fn to_primitive_utc(value: OffsetDateTime) -> PrimitiveDateTime {
    let utc = value.to_offset(UtcOffset::UTC);
    PrimitiveDateTime::new(utc.date(), utc.time())
}

pub(crate) fn format_primitive(value: PrimitiveDateTime) -> String {
    value.assume_utc().format(&Rfc3339).unwrap_or_else(|_| value.assume_utc().to_string())
}

pub(crate) fn format_offset(value: OffsetDateTime) -> String {
    value.format(&Rfc3339).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Date, Time, UtcOffset};

    #[test]
    fn format_primitive_outputs_utc_z() {
        let date = Date::from_calendar_date(2025, time::Month::January, 2).unwrap();
        let time = Time::from_hms(10, 20, 30).unwrap();
        let value = PrimitiveDateTime::new(date, time);
        assert_eq!(format_primitive(value), "2025-01-02T10:20:30Z");
    }

    #[test]
    fn format_offset_preserves_offset() {
        let date = Date::from_calendar_date(2025, time::Month::January, 2).unwrap();
        let time = Time::from_hms(10, 20, 30).unwrap();
        let utc = PrimitiveDateTime::new(date, time).assume_utc();
        let offset = UtcOffset::from_hms(3, 0, 0).unwrap();
        let shifted = utc.to_offset(offset);
        assert_eq!(format_offset(shifted), "2025-01-02T13:20:30+03:00");
    }
}
