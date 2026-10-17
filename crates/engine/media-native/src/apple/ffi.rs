//! Ручные объявления C-интерфейсов фреймворков Apple: CoreFoundation, CoreMedia,
//! CoreVideo, VideoToolbox и AudioToolbox.
//!
//! Сторонний крейт не нужен: нужно полсотни функций, а типы — указатели и
//! числа. Фреймворки линкуются только на macOS (`cfg_attr`), поэтому остальной
//! код модуля проверяется компилятором и на других платформах.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_void};

pub(super) type OSStatus = i32;
pub(super) type Boolean = u8;
pub(super) type CFIndex = isize;
pub(super) type CFTypeRef = *const c_void;
pub(super) type CFStringRef = *const c_void;
pub(super) type CFDataRef = *const c_void;
pub(super) type CFDictionaryRef = *const c_void;
pub(super) type CFAllocatorRef = *const c_void;
pub(super) type CFNumberRef = *const c_void;
pub(super) type CMFormatDescriptionRef = *const c_void;
pub(super) type CMBlockBufferRef = *const c_void;
pub(super) type CMSampleBufferRef = *const c_void;
pub(super) type CVPixelBufferRef = *const c_void;
pub(super) type VTDecompressionSessionRef = *mut c_void;
pub(super) type AudioConverterRef = *mut c_void;

/// `kCFStringEncodingUTF8`.
pub(super) const kCFStringEncodingUTF8: u32 = 0x0800_0100;
/// `kCFNumberSInt32Type`.
pub(super) const kCFNumberSInt32Type: i32 = 3;

/// `kCMVideoCodecType_H264` (`'avc1'`) и `kCMVideoCodecType_HEVC` (`'hvc1'`).
pub(super) const kCMVideoCodecType_H264: u32 = 0x6176_6331;
pub(super) const kCMVideoCodecType_HEVC: u32 = 0x6876_6331;
/// `kCMBlockBufferAssureMemoryNowFlag`.
pub(super) const kCMBlockBufferAssureMemoryNowFlag: u32 = 1;
/// `kCMTimeFlags_Valid`.
pub(super) const kCMTimeFlags_Valid: u32 = 1;

/// `kVTDecodeFrame_EnableTemporalProcessing`: выдавать кадры в порядке показа.
pub(super) const kVTDecodeFrame_EnableTemporalProcessing: u32 = 1 << 3;

/// Форматы пикселей CoreVideo: `'420v'`, `'420f'` (8 бит, ТВ-/полный диапазон)
/// и `'x420'`, `'xf20'` (10 бит в старших разрядах 16-битного сэмпла).
pub(super) const kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange: u32 = 0x3432_3076;
pub(super) const kCVPixelFormatType_420YpCbCr8BiPlanarFullRange: u32 = 0x3432_3066;
pub(super) const kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange: u32 = 0x7834_3230;
pub(super) const kCVPixelFormatType_420YpCbCr10BiPlanarFullRange: u32 = 0x7866_3230;
/// `kCVPixelBufferLock_ReadOnly`.
pub(super) const kCVPixelBufferLock_ReadOnly: u64 = 1;

/// `kAudioFormatMPEG4AAC` (`'aac '`) и `kAudioFormatLinearPCM` (`'lpcm'`).
pub(super) const kAudioFormatMPEG4AAC: u32 = 0x6161_6320;
pub(super) const kAudioFormatLinearPCM: u32 = 0x6C70_636D;
/// `kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked`.
pub(super) const kAudioFormatFlagsPackedS16: u32 = 4 | 8;
/// `kAudioConverterDecompressionMagicCookie` (`'dmgc'`).
pub(super) const kAudioConverterDecompressionMagicCookie: u32 = 0x646D_6763;

/// `CMTime`: `value / timescale` секунд.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct CMTime {
    pub value: i64,
    pub timescale: i32,
    pub flags: u32,
    pub epoch: i64,
}

impl CMTime {
    /// `kCMTimeInvalid`.
    pub(super) const INVALID: Self = Self { value: 0, timescale: 0, flags: 0, epoch: 0 };

    /// Микросекунды как `CMTime` (шкала — миллионные доли секунды).
    pub(super) fn from_micros(us: i64) -> Self {
        Self { value: us, timescale: 1_000_000, flags: kCMTimeFlags_Valid, epoch: 0 }
    }

    /// Значение в микросекундах; `None` — время недействительно.
    pub(super) fn micros(self) -> Option<i64> {
        if self.flags & kCMTimeFlags_Valid == 0 || self.timescale <= 0 {
            return None;
        }
        let us = i128::from(self.value) * 1_000_000 / i128::from(self.timescale);
        i64::try_from(us).ok()
    }
}

#[repr(C)]
pub(super) struct CMSampleTimingInfo {
    pub duration: CMTime,
    pub presentationTimeStamp: CMTime,
    pub decodeTimeStamp: CMTime,
}

/// Обратный вызов `VTDecompressionOutputCallback`.
pub(super) type VTDecompressionOutputCallback = unsafe extern "C" fn(
    refcon: *mut c_void,
    source_frame_refcon: *mut c_void,
    status: OSStatus,
    info_flags: u32,
    image_buffer: CVPixelBufferRef,
    presentation_time_stamp: CMTime,
    presentation_duration: CMTime,
);

#[repr(C)]
pub(super) struct VTDecompressionOutputCallbackRecord {
    pub decompressionOutputCallback: Option<VTDecompressionOutputCallback>,
    pub decompressionOutputRefCon: *mut c_void,
}

/// `AudioStreamBasicDescription`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct AudioStreamBasicDescription {
    pub mSampleRate: f64,
    pub mFormatID: u32,
    pub mFormatFlags: u32,
    pub mBytesPerPacket: u32,
    pub mFramesPerPacket: u32,
    pub mBytesPerFrame: u32,
    pub mChannelsPerFrame: u32,
    pub mBitsPerChannel: u32,
    pub mReserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct AudioBuffer {
    pub mNumberChannels: u32,
    pub mDataByteSize: u32,
    pub mData: *mut c_void,
}

/// `AudioBufferList` с одним буфером (интерливленный PCM).
#[repr(C)]
pub(super) struct AudioBufferList1 {
    pub mNumberBuffers: u32,
    pub mBuffers: [AudioBuffer; 1],
}

#[repr(C)]
pub(super) struct AudioStreamPacketDescription {
    pub mStartOffset: i64,
    pub mVariableFramesInPacket: u32,
    pub mDataByteSize: u32,
}

/// `AudioConverterComplexInputDataProc`.
pub(super) type AudioConverterComplexInputDataProc = unsafe extern "C" fn(
    converter: AudioConverterRef,
    io_number_data_packets: *mut u32,
    io_data: *mut AudioBufferList1,
    out_data_packet_description: *mut *mut AudioStreamPacketDescription,
    user_data: *mut c_void,
) -> OSStatus;

/// Раскладка `CFDictionaryKeyCallBacks` / `CFDictionaryValueCallBacks`:
/// из Rust берётся только адрес глобальной константы, поэтому тип непрозрачен.
#[repr(C)]
pub(super) struct CFCallBacks {
    _private: [usize; 6],
}

#[cfg_attr(target_os = "macos", link(name = "CoreFoundation", kind = "framework"))]
unsafe extern "C" {
    pub(super) static kCFBooleanTrue: CFTypeRef;
    pub(super) static kCFBooleanFalse: CFTypeRef;
    pub(super) static kCFTypeDictionaryKeyCallBacks: CFCallBacks;
    pub(super) static kCFTypeDictionaryValueCallBacks: CFCallBacks;

    pub(super) fn CFRelease(cf: CFTypeRef);
    pub(super) fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> Boolean;
    pub(super) fn CFDataCreate(allocator: CFAllocatorRef, bytes: *const u8, length: CFIndex) -> CFDataRef;
    pub(super) fn CFNumberCreate(allocator: CFAllocatorRef, the_type: i32, value: *const c_void) -> CFNumberRef;
    pub(super) fn CFStringCreateWithCString(
        allocator: CFAllocatorRef,
        c_str: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    pub(super) fn CFDictionaryCreate(
        allocator: CFAllocatorRef,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: CFIndex,
        key_callbacks: *const CFCallBacks,
        value_callbacks: *const CFCallBacks,
    ) -> CFDictionaryRef;
}

#[cfg_attr(target_os = "macos", link(name = "CoreMedia", kind = "framework"))]
unsafe extern "C" {
    pub(super) static kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms: CFStringRef;

    pub(super) fn CMVideoFormatDescriptionCreate(
        allocator: CFAllocatorRef,
        codec_type: u32,
        width: i32,
        height: i32,
        extensions: CFDictionaryRef,
        format_description_out: *mut CMFormatDescriptionRef,
    ) -> OSStatus;
    pub(super) fn CMBlockBufferCreateWithMemoryBlock(
        structure_allocator: CFAllocatorRef,
        memory_block: *mut c_void,
        block_length: usize,
        block_allocator: CFAllocatorRef,
        custom_block_source: *const c_void,
        offset_to_data: usize,
        data_length: usize,
        flags: u32,
        block_buffer_out: *mut CMBlockBufferRef,
    ) -> OSStatus;
    pub(super) fn CMBlockBufferReplaceDataBytes(
        source_bytes: *const c_void,
        destination_buffer: CMBlockBufferRef,
        offset_into_destination: usize,
        data_length: usize,
    ) -> OSStatus;
    pub(super) fn CMSampleBufferCreate(
        allocator: CFAllocatorRef,
        data_buffer: CMBlockBufferRef,
        data_ready: Boolean,
        make_data_ready_callback: *const c_void,
        make_data_ready_refcon: *mut c_void,
        format_description: CMFormatDescriptionRef,
        num_samples: isize,
        num_sample_timing_entries: isize,
        sample_timing_array: *const CMSampleTimingInfo,
        num_sample_size_entries: isize,
        sample_size_array: *const usize,
        sample_buffer_out: *mut CMSampleBufferRef,
    ) -> OSStatus;
}

#[cfg_attr(target_os = "macos", link(name = "CoreVideo", kind = "framework"))]
unsafe extern "C" {
    pub(super) static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    pub(super) static kCVImageBufferYCbCrMatrixKey: CFStringRef;
    pub(super) static kCVImageBufferYCbCrMatrix_ITU_R_709_2: CFStringRef;

    pub(super) fn CVPixelBufferLockBaseAddress(buffer: CVPixelBufferRef, flags: u64) -> i32;
    pub(super) fn CVPixelBufferUnlockBaseAddress(buffer: CVPixelBufferRef, flags: u64) -> i32;
    pub(super) fn CVPixelBufferGetPixelFormatType(buffer: CVPixelBufferRef) -> u32;
    pub(super) fn CVPixelBufferGetWidth(buffer: CVPixelBufferRef) -> usize;
    pub(super) fn CVPixelBufferGetHeight(buffer: CVPixelBufferRef) -> usize;
    pub(super) fn CVPixelBufferGetPlaneCount(buffer: CVPixelBufferRef) -> usize;
    pub(super) fn CVPixelBufferGetBaseAddressOfPlane(buffer: CVPixelBufferRef, plane: usize) -> *const u8;
    pub(super) fn CVPixelBufferGetBytesPerRowOfPlane(buffer: CVPixelBufferRef, plane: usize) -> usize;
    pub(super) fn CVBufferGetAttachment(buffer: CVPixelBufferRef, key: CFStringRef, mode: *mut u32) -> CFTypeRef;
}

#[cfg_attr(target_os = "macos", link(name = "VideoToolbox", kind = "framework"))]
unsafe extern "C" {
    pub(super) static kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder: CFStringRef;

    pub(super) fn VTIsHardwareDecodeSupported(codec_type: u32) -> Boolean;
    pub(super) fn VTDecompressionSessionCreate(
        allocator: CFAllocatorRef,
        video_format_description: CMFormatDescriptionRef,
        video_decoder_specification: CFDictionaryRef,
        destination_image_buffer_attributes: CFDictionaryRef,
        output_callback: *const VTDecompressionOutputCallbackRecord,
        decompression_session_out: *mut VTDecompressionSessionRef,
    ) -> OSStatus;
    pub(super) fn VTDecompressionSessionDecodeFrame(
        session: VTDecompressionSessionRef,
        sample_buffer: CMSampleBufferRef,
        decode_flags: u32,
        source_frame_refcon: *mut c_void,
        info_flags_out: *mut u32,
    ) -> OSStatus;
    pub(super) fn VTDecompressionSessionFinishDelayedFrames(session: VTDecompressionSessionRef) -> OSStatus;
    pub(super) fn VTDecompressionSessionWaitForAsynchronousFrames(session: VTDecompressionSessionRef) -> OSStatus;
    pub(super) fn VTDecompressionSessionInvalidate(session: VTDecompressionSessionRef);
}

#[cfg_attr(target_os = "macos", link(name = "AudioToolbox", kind = "framework"))]
unsafe extern "C" {
    pub(super) fn AudioConverterNew(
        source_format: *const AudioStreamBasicDescription,
        destination_format: *const AudioStreamBasicDescription,
        converter_out: *mut AudioConverterRef,
    ) -> OSStatus;
    pub(super) fn AudioConverterDispose(converter: AudioConverterRef) -> OSStatus;
    pub(super) fn AudioConverterReset(converter: AudioConverterRef) -> OSStatus;
    pub(super) fn AudioConverterSetProperty(
        converter: AudioConverterRef,
        property_id: u32,
        data_size: u32,
        data: *const c_void,
    ) -> OSStatus;
    pub(super) fn AudioConverterFillComplexBuffer(
        converter: AudioConverterRef,
        input_data_proc: AudioConverterComplexInputDataProc,
        input_data_proc_user_data: *mut c_void,
        io_output_data_packet_size: *mut u32,
        out_output_data: *mut AudioBufferList1,
        out_packet_description: *mut AudioStreamPacketDescription,
    ) -> OSStatus;
}

unsafe extern "C" {
    /// `libSystem`: версия ядра для проверки наличия программного HEVC.
    pub(super) fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> i32;
}
