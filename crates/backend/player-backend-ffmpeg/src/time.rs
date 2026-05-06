use std::time::Duration;

use ffmpeg_next as ffmpeg;

pub(crate) fn duration_from_micros(duration: i64) -> Option<Duration> {
    if duration <= 0 {
        return None;
    }

    Some(Duration::from_secs_f64(
        duration as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE),
    ))
}

pub(crate) fn duration_to_av_timestamp(duration: Duration) -> i64 {
    duration
        .as_micros()
        .min(i64::MAX as u128)
        .try_into()
        .expect("value clamped to i64")
}

pub(crate) fn rational_to_f64(value: ffmpeg::Rational) -> Option<f64> {
    if value.numerator() <= 0 || value.denominator() <= 0 {
        return None;
    }

    Some(f64::from(value))
}

pub(crate) fn timestamp_to_duration(
    timestamp: i64,
    time_base: ffmpeg::Rational,
) -> Option<Duration> {
    let seconds = (timestamp as f64) * f64::from(time_base);
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }

    Some(Duration::from_secs_f64(seconds))
}

pub(crate) fn timestamp_to_micros(timestamp: i64, time_base: ffmpeg::Rational) -> Option<i64> {
    let numerator = i128::from(time_base.numerator());
    let denominator = i128::from(time_base.denominator());
    if denominator <= 0 {
        return None;
    }
    let value = i128::from(timestamp)
        .saturating_mul(numerator)
        .saturating_mul(1_000_000)
        / denominator;
    Some(value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
}

pub(crate) fn frame_interval_from_stream(stream: &ffmpeg::Stream<'_>) -> Duration {
    let frame_rate = rational_to_f64(stream.avg_frame_rate())
        .or_else(|| rational_to_f64(stream.rate()))
        .filter(|value| *value > 0.0)
        .unwrap_or(30.0);

    Duration::from_secs_f64(1.0 / frame_rate)
}
