use std::num::NonZeroU16;

use super::*;

struct Session {
    inner: Box<dyn AudioProcessor>,
    input: VesperAudioFormat,
    output: VesperAudioFormat,
    poisoned: bool,
}

pub(super) fn vtable(modes: AudioProcessorCapabilities) -> VesperAudioProcessorVtable {
    VesperAudioProcessorVtable {
        struct_size: size_of::<VesperAudioProcessorVtable>() as u32,
        query_capabilities_json: Some(capabilities),
        open: Some(open),
        set_playback_policy: Some(set_playback_policy),
        set_param: Some(set_param),
        process_block: modes.fixed_block.then_some(process_block),
        push: modes.buffered.then_some(push),
        pull: modes.buffered.then_some(pull),
        latency_frames: Some(latency_frames),
        flush: Some(flush),
        destroy: Some(destroy),
        get_last_error: None,
    }
}

unsafe extern "C" fn capabilities(ctx: *mut c_void, out: *mut VesperBytes) -> VesperStatus {
    guarded(|| {
        if !sized_pointer(out) {
            return Err(PluginError::Contract("invalid capabilities output"));
        }
        // SAFETY: the caller provides its live root owner and writable result.
        let factory = unsafe { owner(ctx) }?
            .plugin
            .audio
            .as_ref()
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        // SAFETY: output points to a VesperBytes and the factory owns the slice.
        unsafe { out.write(bytes(factory.capabilities_json().as_bytes())) };
        Ok(VESPER_OK)
    })
}

unsafe extern "C" fn open(
    ctx: *mut c_void,
    input: *const VesperAudioFormat,
    params: VesperBytes,
    output: *mut VesperAudioFormat,
    latency: *mut u32,
    session: *mut *mut VesperSession,
) -> VesperStatus {
    guarded(|| {
        if !sized_pointer(session) || !sized_pointer(latency) {
            return Err(PluginError::Contract("invalid audio open outputs"));
        }
        // SAFETY: the caller provides readable inputs and distinct writable outputs.
        let (input, params) = unsafe {
            session.write(ptr::null_mut());
            latency.write(0);
            require_size(output, size_of::<VesperAudioFormat>())?;
            (read_sized(input)?, read_bytes(params, MAX_JSON_BYTES)?)
        };
        validate_f32_audio_format(&input)?;
        // SAFETY: the caller provides the live owner from its root.
        let factory = unsafe { owner(ctx) }?
            .plugin
            .audio
            .as_ref()
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        let opened = factory.open(input, params)?;
        validate_f32_audio_format(&opened.output_format)?;
        if factory.capabilities() != opened.processor.capabilities() {
            return Err(PluginError::Contract(
                "audio session modes differ from the factory",
            ));
        }
        // SAFETY: outputs are writable. The plugin transfers this Box once.
        unsafe {
            output.write(opened.output_format);
            latency.write(opened.processor.latency_frames());
            session.write(
                Box::into_raw(Box::new(Session {
                    inner: opened.processor,
                    input,
                    output: opened.output_format,
                    poisoned: false,
                }))
                .cast(),
            );
        }
        Ok(VESPER_OK)
    })
}

unsafe fn session_call(
    raw: *mut VesperSession,
    call: impl FnOnce(&mut Session) -> PluginResult<VesperStatus>,
) -> VesperStatus {
    let raw = raw.cast::<Session>();
    if !sized_pointer(raw) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the host owns this live session and serializes all calls.
    let session = unsafe { &mut *raw };
    if session.poisoned {
        return VESPER_ERR_PANIC;
    }
    match catch(|| call(session)) {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => status(error),
        Err(()) => {
            session.poisoned = true;
            VESPER_ERR_PANIC
        }
    }
}

unsafe extern "C" fn set_playback_policy(
    raw: *mut VesperSession,
    rate: f32,
    pitch: u32,
    epoch: u64,
) -> VesperStatus {
    // SAFETY: the ABI caller supplies its live, exclusive session handle.
    unsafe {
        session_call(raw, |session| {
            if !rate.is_finite() || rate <= 0.0 || epoch == 0 {
                return Err(PluginError::Contract("invalid playback rate or epoch"));
            }
            session.inner.set_playback_policy(rate, pitch, epoch)?;
            Ok(VESPER_OK)
        })
    }
}

unsafe extern "C" fn set_param(
    raw: *mut VesperSession,
    key: VesperBytes,
    value: VesperBytes,
) -> VesperStatus {
    // SAFETY: the caller borrows the parameter bytes for its exclusive call.
    unsafe {
        session_call(raw, |session| {
            session.inner.set_param(
                read_bytes(key, MAX_JSON_BYTES)?,
                read_bytes(value, MAX_JSON_BYTES)?,
            )?;
            Ok(VESPER_OK)
        })
    }
}

fn time(buffer: &VesperAudioBuffer) -> AudioTime {
    AudioTime {
        pts_us: buffer.pts_us,
        rate_epoch: buffer.rate_epoch,
    }
}

unsafe fn input_view<'a>(
    buffer: &VesperAudioBuffer,
    channels: NonZeroU16,
    samples: usize,
) -> PluginResult<AudioBufferView<'a>> {
    let pcm = if samples == 0 {
        &[]
    } else {
        // SAFETY: the caller validated the span and guarantees readable PCM.
        unsafe { std::slice::from_raw_parts(buffer.planes[0].cast::<f32>(), samples) }
    };
    AudioBufferView::interleaved(channels, pcm, time(buffer))
}

unsafe fn output_view<'a>(
    buffer: &VesperAudioBuffer,
    channels: NonZeroU16,
    samples: usize,
) -> PluginResult<AudioBufferMut<'a>> {
    let pcm = if samples == 0 {
        &mut []
    } else {
        // SAFETY: the caller validated the span and guarantees exclusive initialized PCM.
        unsafe { std::slice::from_raw_parts_mut(buffer.planes[0].cast::<f32>(), samples) }
    };
    AudioBufferMut::interleaved(channels, pcm)
}

unsafe extern "C" fn process_block(
    raw: *mut VesperSession,
    input: *const VesperAudioBuffer,
    out: *mut VesperAudioBuffer,
) -> VesperStatus {
    // SAFETY: this call exclusively owns the session and the writable output.
    // The ABI caller provides readable input and sized descriptors.
    unsafe {
        session_call(raw, |session| {
            let input_buffer = read_sized(input)?;
            let mut output = read_sized(out)?;
            let input_samples = audio_sample_count(&input_buffer, &session.input, false)?;
            audio_sample_count(&output, &session.output, true)?;
            if session.input.channels != session.output.channels
                || session.input.sample_rate != session.output.sample_rate
                || input_buffer.frame_count > output.frame_capacity
                || input_buffer.rate_epoch == 0
            {
                return Err(PluginError::Contract(
                    "fixed block requires a matching format, capacity and epoch",
                ));
            }
            let in_channels = validate_f32_audio_format(&session.input)?;
            let out_channels = validate_f32_audio_format(&session.output)?;
            let block = if ptr::eq(input, out) {
                AudioBlock::InPlace {
                    audio: output_view(&output, out_channels, input_samples)?,
                    time: time(&input_buffer),
                }
            } else {
                if !pcm_ranges_are_disjoint(
                    input_buffer.planes[0],
                    input_samples,
                    output.planes[0],
                    input_samples,
                ) {
                    return Err(PluginError::Contract("overlapping fixed-block PCM"));
                }
                AudioBlock::Split {
                    input: input_view(&input_buffer, in_channels, input_samples)?,
                    output: output_view(&output, out_channels, input_samples)?,
                }
            };
            session.inner.process_block(block)?;
            output.frame_count = input_buffer.frame_count;
            output.pts_us = input_buffer.pts_us;
            output.rate_epoch = input_buffer.rate_epoch;
            out.write(output);
            Ok(VESPER_OK)
        })
    }
}

unsafe extern "C" fn push(
    raw: *mut VesperSession,
    input: *const VesperAudioBuffer,
    consumed: *mut u32,
) -> VesperStatus {
    // SAFETY: the ABI caller owns this session, borrows input and supplies writable consumed.
    unsafe {
        session_call(raw, |session| {
            if !sized_pointer(consumed) {
                return Err(PluginError::Contract("invalid consumed-frames output"));
            }
            consumed.write(0);
            let input = if input.is_null() {
                None
            } else {
                let input = read_sized(input)?;
                let count = audio_sample_count(&input, &session.input, false)?;
                if input.rate_epoch == 0 || input.frame_count == 0 {
                    return Err(PluginError::Contract(
                        "buffered input requires frames and an epoch",
                    ));
                }
                Some(input_view(
                    &input,
                    validate_f32_audio_format(&session.input)?,
                    count,
                )?)
            };
            match session.inner.push(input)? {
                AudioPush::Consumed(frames)
                    if frames > 0
                        && input.is_some_and(|input| frames as usize <= input.frames()) =>
                {
                    consumed.write(frames);
                    Ok(VESPER_OK)
                }
                AudioPush::Backpressure => Ok(VESPER_TRY_AGAIN),
                AudioPush::EosAccepted if input.is_none() => Ok(VESPER_OK),
                _ => Err(PluginError::Contract("invalid buffered audio consumption")),
            }
        })
    }
}

unsafe extern "C" fn pull(
    raw: *mut VesperSession,
    out: *mut VesperAudioBuffer,
    produced: *mut u32,
) -> VesperStatus {
    // SAFETY: the ABI caller owns this session and supplies its writable output.
    unsafe {
        session_call(raw, |session| {
            if !sized_pointer(produced) {
                return Err(PluginError::Contract("invalid produced-frames output"));
            }
            produced.write(0);
            let mut output = read_sized(out)?;
            let samples = audio_sample_count(&output, &session.output, true)?;
            let buffer = output_view(
                &output,
                validate_f32_audio_format(&session.output)?,
                samples,
            )?;
            match session.inner.pull(buffer)? {
                AudioPull::Produced { frames, time }
                    if frames > 0 && frames <= output.frame_capacity && time.rate_epoch != 0 =>
                {
                    output.frame_count = frames;
                    output.pts_us = time.pts_us;
                    output.rate_epoch = time.rate_epoch;
                    out.write(output);
                    produced.write(frames);
                    Ok(VESPER_OK)
                }
                AudioPull::NeedInput => Ok(VESPER_NEED_MORE_INPUT),
                AudioPull::TryAgain => Ok(VESPER_TRY_AGAIN),
                AudioPull::Eof => Ok(VESPER_EOF),
                _ => Err(PluginError::Contract("invalid buffered audio production")),
            }
        })
    }
}

unsafe extern "C" fn latency_frames(raw: *mut VesperSession) -> u32 {
    let mut result = 0;
    // SAFETY: the ABI caller supplies its exclusively accessed live session.
    unsafe {
        session_call(raw, |session| {
            result = session.inner.latency_frames();
            Ok(VESPER_OK)
        })
    };
    result
}

unsafe extern "C" fn flush(raw: *mut VesperSession) -> VesperStatus {
    // SAFETY: the ABI caller supplies its exclusively accessed live session.
    unsafe { session_call(raw, |session| session.inner.flush().map(|()| VESPER_OK)) }
}

unsafe extern "C" fn destroy(raw: *mut VesperSession) {
    if sized_pointer(raw.cast::<Session>()) {
        // SAFETY: host relinquishes the one Box created by open.
        let _ = catch(|| unsafe { drop(Box::from_raw(raw.cast::<Session>())) });
    }
}
