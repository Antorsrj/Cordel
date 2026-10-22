//! Platform-neutral video frames and pixel conversion.
//!
//! No OS or SDK dependency on purpose: this is the part of the media path that
//! can be tested without hardware, and the cost it measures (per-frame copy and
//! color conversion) is a real part of the Phase 0 question "does sharing a
//! screen hurt chat scrolling?".

use std::fmt;

/// Pixel layouts that cross the boundary between capture/decode and rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// Packed BGRA, 4 bytes per pixel per row. Produced by WebRTC's desktop capturer.
    Bgra8,
    /// Packed RGBA, 4 bytes per pixel per row. Consumed by the UI.
    Rgba8,
    /// Planar 4:2:0, BT.601 limited range. Produced by the video decoder.
    I420,
}

impl PixelFormat {
    /// Bytes per pixel for packed formats; `None` for planar formats.
    pub const fn bytes_per_pixel(self) -> Option<usize> {
        match self {
            PixelFormat::Bgra8 | PixelFormat::Rgba8 => Some(4),
            PixelFormat::I420 => None,
        }
    }
}

/// A single captured or decoded frame.
///
/// `stride` is the number of bytes per row for packed formats and is allowed to
/// exceed `width * bytes_per_pixel` (capturers pad rows). It is ignored for
/// [`PixelFormat::I420`], whose planes are assumed tight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    width: u32,
    height: u32,
    stride: u32,
    format: PixelFormat,
    data: Vec<u8>,
}

/// Reasons a frame cannot be represented.
#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    /// Width or height was zero.
    ZeroDimension { width: u32, height: u32 },
    /// The byte length does not match width, height, stride and format.
    LengthMismatch { expected: usize, actual: usize },
    /// A packed frame needs `stride >= width * bytes_per_pixel`.
    StrideTooSmall { stride: u32, minimum: u32 },
    /// A conversion that only accepts one layout was given another.
    UnexpectedFormat {
        expected: PixelFormat,
        actual: PixelFormat,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::ZeroDimension { width, height } => {
                write!(f, "zero frame dimension: {width}x{height}")
            }
            FrameError::LengthMismatch { expected, actual } => {
                write!(
                    f,
                    "frame buffer is {actual} bytes, expected exactly {expected}"
                )
            }
            FrameError::StrideTooSmall { stride, minimum } => {
                write!(
                    f,
                    "row stride {stride} is smaller than the packed row {minimum}"
                )
            }
            FrameError::UnexpectedFormat { expected, actual } => {
                write!(f, "expected a {expected:?} frame, got {actual:?}")
            }
        }
    }
}

impl std::error::Error for FrameError {}

impl Frame {
    /// Validates the dimensions and buffer length, then wraps the bytes.
    pub fn new(
        width: u32,
        height: u32,
        stride: u32,
        format: PixelFormat,
        data: Vec<u8>,
    ) -> Result<Self, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::ZeroDimension { width, height });
        }
        if let Some(bpp) = format.bytes_per_pixel() {
            let minimum = width * bpp as u32;
            if stride < minimum {
                return Err(FrameError::StrideTooSmall { stride, minimum });
            }
        }
        let expected = Self::expected_len(width, height, stride, format);
        if data.len() != expected {
            return Err(FrameError::LengthMismatch {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            stride,
            format,
            data,
        })
    }

    /// Convenience constructor for tightly packed frames.
    pub fn packed(
        width: u32,
        height: u32,
        format: PixelFormat,
        data: Vec<u8>,
    ) -> Result<Self, FrameError> {
        let bpp = format.bytes_per_pixel().unwrap_or(1) as u32;
        Self::new(width, height, width * bpp, format, data)
    }

    fn expected_len(width: u32, height: u32, stride: u32, format: PixelFormat) -> usize {
        match format {
            PixelFormat::Bgra8 | PixelFormat::Rgba8 => stride as usize * height as usize,
            PixelFormat::I420 => {
                let luma = width as usize * height as usize;
                let chroma = width.div_ceil(2) as usize * height.div_ceil(2) as usize;
                luma + 2 * chroma
            }
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn stride(&self) -> u32 {
        self.stride
    }

    pub fn format(&self) -> PixelFormat {
        self.format
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Pixel count, useful for per-megapixel measurements.
    pub fn pixels(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Number of bytes an RGBA8 conversion of this frame occupies.
    pub fn rgba8_len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    /// Converts to tightly packed RGBA8, the layout the UI consumes.
    ///
    /// Infalible: [`Frame::new`] already validated the layout.
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.rgba8_len());
        self.to_rgba8_into(&mut out);
        out
    }

    /// Converts into a reusable vector, keeping its allocation alive across frames.
    ///
    /// The render loop calls this every frame; reusing the buffer is what makes
    /// the difference between "allocation per frame" and "copy per frame"
    /// measurable in the Phase 0 spike.
    pub fn to_rgba8_into(&self, out: &mut Vec<u8>) {
        out.resize(self.rgba8_len(), 0);
        self.write_rgba8(out)
            .expect("a validated frame converts with the exact size");
    }

    /// Writes tightly packed RGBA8 into `out`, which must be exactly
    /// [`Frame::rgba8_len`] bytes. Allocates nothing, so the UI path can write
    /// straight into the pixel buffer it hands to Slint.
    pub fn write_rgba8(&self, out: &mut [u8]) -> Result<(), FrameError> {
        let expected = self.rgba8_len();
        if out.len() != expected {
            return Err(FrameError::LengthMismatch {
                expected,
                actual: out.len(),
            });
        }
        let row = self.width as usize * 4;
        match self.format {
            PixelFormat::Rgba8 => {
                for y in 0..self.height as usize {
                    let start = y * self.stride as usize;
                    let dst = &mut out[y * row..y * row + row];
                    dst.copy_from_slice(&self.data[start..start + row]);
                }
            }
            PixelFormat::Bgra8 => {
                for y in 0..self.height as usize {
                    let start = y * self.stride as usize;
                    let src = &self.data[start..start + row];
                    let dst = &mut out[y * row..y * row + row];
                    let (src_pixels, _) = src.as_chunks::<4>();
                    let (dst_pixels, _) = dst.as_chunks_mut::<4>();
                    for (px, out_px) in src_pixels.iter().zip(dst_pixels.iter_mut()) {
                        out_px[0] = px[2];
                        out_px[1] = px[1];
                        out_px[2] = px[0];
                        out_px[3] = px[3];
                    }
                }
            }
            PixelFormat::I420 => self.i420_to_rgba8(out),
        }
        Ok(())
    }

    /// BT.601 limited-range conversion, the range decoders actually deliver.
    fn i420_to_rgba8(&self, out: &mut [u8]) {
        let (w, h) = (self.width as usize, self.height as usize);
        let cw = w.div_ceil(2);
        let ch = h.div_ceil(2);
        let y_plane = &self.data[..w * h];
        let u_plane = &self.data[w * h..w * h + cw * ch];
        let v_plane = &self.data[w * h + cw * ch..];

        for y in 0..h {
            for x in 0..w {
                let yy = y_plane[y * w + x] as i32;
                let cx = x / 2;
                let cy = y / 2;
                let u = u_plane[cy * cw + cx] as i32 - 128;
                let v = v_plane[cy * cw + cx] as i32 - 128;
                let c = yy - 16;
                let r = ((298 * c + 409 * v + 128) >> 8).clamp(0, 255) as u8;
                let g = ((298 * c - 100 * u - 208 * v + 128) >> 8).clamp(0, 255) as u8;
                let b = ((298 * c + 516 * u + 128) >> 8).clamp(0, 255) as u8;
                let i = (y * w + x) * 4;
                out[i] = r;
                out[i + 1] = g;
                out[i + 2] = b;
                out[i + 3] = 255;
            }
        }
    }
}

/// Reduces a frame to at most `max_width` pixels wide, with the nearest
/// neighbour.
///
/// The interface paints a frame scaled, so the picture does not need the
/// captured resolution. A frame of a 2560x1440 screen is about 14 MB, and
/// the queue that carries it to the interface thread cannot afford that at
/// fifteen frames a second. The source keeps its own resolution; only the copy
/// that travels is smaller.
pub fn resize_bgra(frame: Frame, max_width: u32) -> Frame {
    if max_width == 0 || frame.width() <= max_width {
        // The frame is already small enough, and copying it would cost as much
        // as the picture itself.
        return frame;
    }
    let width = max_width;
    let height = (frame.height() as u64 * width as u64 / frame.width() as u64) as u32;
    if height == 0 {
        return frame.clone();
    }
    let source = frame.data();
    let source_stride = frame.stride() as usize;
    let target_stride = width as usize * 4;
    let mut data = Vec::with_capacity(target_stride * height as usize);
    for target_row in 0..height as usize {
        let source_row = target_row * frame.height() as usize / height as usize;
        let start = source_row * source_stride;
        if start + source_stride > source.len() {
            break;
        }
        let row = &source[start..start + source_stride];
        for target_column in 0..width as usize {
            let source_column = target_column * frame.width() as usize / width as usize;
            let pixel = source_column * 4;
            if pixel + 4 <= row.len() {
                data.extend_from_slice(&row[pixel..pixel + 4]);
            }
        }
    }
    match Frame::new(
        width,
        height,
        target_stride as u32,
        PixelFormat::Bgra8,
        data,
    ) {
        Ok(frame) => frame,
        // The frame is already valid, so the copy failing means the copy is
        // not usable; the original still is.
        Err(_) => frame,
    }
}

/// Shrinks a frame so the interface never converts a full screen.
///
/// `resize_bgra` only understands packed BGRA. A remote screen arrives as
/// I420, and that function then gives the original back, so the interface
/// converts every pixel of a 2560x1440 frame. This is the copy the interface
/// is willing to paint: neither side longer than `max_edge`, in the frame's
/// own layout. I420 keeps even dimensions, which is what its chroma planes need.
pub fn fit_display(frame: Frame, max_edge: u32) -> Frame {
    let Some(width) = fitted_width(frame.width(), frame.height(), max_edge) else {
        return frame;
    };
    match frame.format() {
        PixelFormat::Bgra8 | PixelFormat::Rgba8 => resize_bgra(frame, width),
        PixelFormat::I420 => fit_i420(frame, width, max_edge),
    }
}

/// The width of a copy that fits inside `max_edge` on both sides.
/// `None` when the frame is already inside that box, or when there is nothing
/// to scale.
fn fitted_width(width: u32, height: u32, max_edge: u32) -> Option<u32> {
    if max_edge == 0 || width == 0 || height == 0 {
        return None;
    }
    if width <= max_edge && height <= max_edge {
        return None;
    }
    let by_width = width.min(max_edge);
    let by_height = ((u64::from(max_edge) * u64::from(width)) / u64::from(height)) as u32;
    Some(by_width.min(by_height.max(1)).max(1))
}

/// Nearest-neighbour I420 shrink. The planes stay tight, so [`Frame::packed`]
/// can check the result. `width` is the fitted width; `max_edge` caps the
/// height when rounding would leave a side outside the box.
fn fit_i420(frame: Frame, width: u32, max_edge: u32) -> Frame {
    let mut width = width & !1;
    let mut height = ((u64::from(frame.height()) * u64::from(width)
        / u64::from(frame.width().max(1))) as u32)
        & !1;
    if height > max_edge {
        height = max_edge & !1;
        width = ((u64::from(frame.width()) * u64::from(height) / u64::from(frame.height().max(1)))
            as u32)
            & !1;
    }
    if width == 0 || height == 0 {
        return frame;
    }
    let src_w = frame.width() as usize;
    let src_h = frame.height() as usize;
    let src_cw = src_w.div_ceil(2);
    let src_ch = src_h.div_ceil(2);
    let (dst_w, dst_h) = (width as usize, height as usize);
    let (dst_cw, dst_ch) = (dst_w / 2, dst_h / 2);
    let mut out = Vec::with_capacity(dst_w * dst_h + 2 * dst_cw * dst_ch);
    {
        let data = frame.data();
        let y_plane = &data[..src_w * src_h];
        let u_plane = &data[src_w * src_h..src_w * src_h + src_cw * src_ch];
        let v_plane = &data[src_w * src_h + src_cw * src_ch..];
        for y in 0..dst_h {
            let sy = y * src_h / dst_h;
            for x in 0..dst_w {
                out.push(y_plane[sy * src_w + x * src_w / dst_w]);
            }
        }
        for y in 0..dst_ch {
            let sy = y * src_ch / dst_ch;
            for x in 0..dst_cw {
                out.push(u_plane[sy * src_cw + x * src_cw / dst_cw]);
            }
        }
        for y in 0..dst_ch {
            let sy = y * src_ch / dst_ch;
            for x in 0..dst_cw {
                out.push(v_plane[sy * src_cw + x * src_cw / dst_cw]);
            }
        }
    }
    Frame::packed(width, height, PixelFormat::I420, out).unwrap_or(frame)
}

/// Builds a synthetic BGRA frame. Used by the probe to exercise the render path
/// without a capture backend, so a missing portal or permission cannot hide a
/// regression in the conversion code.
pub fn synthetic_bgra(width: u32, height: u32, phase: u8) -> Frame {
    let stride = width * 4;
    let mut data = vec![0u8; (stride * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let i = (y * stride + x * 4) as usize;
            data[i] = x.wrapping_add(phase as u32) as u8;
            data[i + 1] = y.wrapping_add(phase as u32) as u8;
            data[i + 2] = phase;
            data[i + 3] = 255;
        }
    }
    Frame::new(width, height, stride, PixelFormat::Bgra8, data).expect("synthetic frame is valid")
}
