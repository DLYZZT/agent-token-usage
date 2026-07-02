use agent_token_usage::time::{parse_filter_time, parse_timestamp};
use chrono::{TimeZone, Utc};

#[test]
fn parses_rfc3339_variants() {
    let expected = Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap();
    for text in [
        "2026-06-30T10:00:00Z",
        "2026-06-30T10:00:00.000Z",
        "2026-06-30T18:00:00+08:00",
        "2026-06-30T18:00:00+0800",
    ] {
        assert_eq!(parse_timestamp(Some(text)), Some(expected), "input {text}");
    }
}

#[test]
fn parses_space_separated_and_naive_timestamps() {
    let expected = Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap();
    for text in [
        "2026-06-30 10:00:00",
        "2026-06-30 10:00:00.000",
        "2026-06-30T10:00:00",
        "2026-06-30 18:00:00+08:00",
    ] {
        assert_eq!(parse_timestamp(Some(text)), Some(expected), "input {text}");
    }
}

#[test]
fn rejects_invalid_timestamps() {
    assert_eq!(parse_timestamp(None), None);
    assert_eq!(parse_timestamp(Some("")), None);
    assert_eq!(parse_timestamp(Some("   ")), None);
    assert_eq!(parse_timestamp(Some("not a time")), None);
    assert_eq!(parse_timestamp(Some("2026-06-30")), None);
}

#[test]
fn filter_time_expands_bare_dates() {
    let start = parse_filter_time(Some("2026-06-30"), false).unwrap();
    assert_eq!(
        start,
        Some(Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap())
    );

    // 作为 --until 时按整天含尾处理：加一天。
    let end = parse_filter_time(Some("2026-06-30"), true).unwrap();
    assert_eq!(
        end,
        Some(Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap())
    );
}

#[test]
fn filter_time_accepts_full_timestamps_and_rejects_garbage() {
    assert_eq!(parse_filter_time(None, false).unwrap(), None);
    assert_eq!(
        parse_filter_time(Some("2026-06-30T10:00:00Z"), true).unwrap(),
        Some(Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap())
    );
    assert!(parse_filter_time(Some("garbage"), false).is_err());
}
