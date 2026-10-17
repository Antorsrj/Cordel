/// Desktop frames from the LiveKit capturer use libyuv ARGB memory order: B, G, R, A.
pub fn bgra_to_rgba(src: &[u8], src_stride: usize, width: usize, height: usize) -> Vec<u8> {
    let mut dst = vec![0u8; width * height * 4];
    for y in 0..height {
        let row = &src[y * src_stride..y * src_stride + width * 4];
        let out = &mut dst[y * width * 4..(y + 1) * width * 4];
        for x in 0..width {
            let px = &row[x * 4..x * 4 + 4];
            let o = x * 4;
            out[o] = px[2];
            out[o + 1] = px[1];
            out[o + 2] = px[0];
            out[o + 3] = 255;
        }
    }
    dst
}

pub fn downscale_rgba(
    src: &[u8],
    width: usize,
    height: usize,
    max_width: usize,
) -> (Vec<u8>, usize, usize) {
    if width <= max_width || width == 0 || height == 0 {
        return (src.to_vec(), width, height);
    }
    let dst_w = max_width;
    let dst_h = ((height * max_width) / width).max(1);
    let mut dst = vec![0u8; dst_w * dst_h * 4];
    for y in 0..dst_h {
        let sy = y * height / dst_h;
        for x in 0..dst_w {
            let sx = x * width / dst_w;
            let s = (sy * width + sx) * 4;
            let d = (y * dst_w + x) * 4;
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    (dst, dst_w, dst_h)
}

pub struct I420Planes {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

/// One CPU conversion from BGRA into even-sized I420 planes.
pub fn bgra_to_i420(src: &[u8], src_stride: usize, width: usize, height: usize) -> I420Planes {
    let width = width & !1;
    let height = height & !1;
    let chroma_w = width / 2;
    let chroma_h = height / 2;
    let mut y = vec![0u8; width * height];
    let mut u = vec![128u8; chroma_w * chroma_h];
    let mut v = vec![128u8; chroma_w * chroma_h];

    for row in 0..height {
        let src_row = &src[row * src_stride..];
        for col in 0..width {
            let px = &src_row[col * 4..col * 4 + 4];
            let (b, g, r) = (px[0] as i32, px[1] as i32, px[2] as i32);
            y[row * width + col] = clamp_u8(((66 * r + 129 * g + 25 * b + 128) >> 8) + 16);
            if row % 2 == 0 && col % 2 == 0 {
                let chroma = (row / 2) * chroma_w + (col / 2);
                u[chroma] = clamp_u8(((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128);
                v[chroma] = clamp_u8(((112 * r - 94 * g - 18 * b + 128) >> 8) + 128);
            }
        }
    }

    I420Planes {
        width: width as u32,
        height: height as u32,
        y,
        u,
        v,
    }
}

pub struct I420View<'a> {
    pub y: &'a [u8],
    pub u: &'a [u8],
    pub v: &'a [u8],
    pub stride_y: usize,
    pub stride_u: usize,
    pub stride_v: usize,
    pub width: usize,
    pub height: usize,
}

pub fn i420_to_rgba(frame: I420View<'_>) -> Vec<u8> {
    let I420View {
        y: y_plane,
        u: u_plane,
        v: v_plane,
        stride_y,
        stride_u,
        stride_v,
        width,
        height,
    } = frame;
    let mut dst = vec![0u8; width * height * 4];
    for row in 0..height {
        for col in 0..width {
            let y = y_plane.get(row * stride_y + col).copied().unwrap_or(16) as i32;
            let u = u_plane
                .get((row / 2) * stride_u + col / 2)
                .copied()
                .unwrap_or(128) as i32;
            let v = v_plane
                .get((row / 2) * stride_v + col / 2)
                .copied()
                .unwrap_or(128) as i32;
            let c = y - 16;
            let d = u - 128;
            let e = v - 128;
            let o = (row * width + col) * 4;
            dst[o] = clamp_u8((298 * c + 409 * e + 128) >> 8);
            dst[o + 1] = clamp_u8((298 * c - 100 * d - 208 * e + 128) >> 8);
            dst[o + 2] = clamp_u8((298 * c + 516 * d + 128) >> 8);
            dst[o + 3] = 255;
        }
    }
    dst
}

fn clamp_u8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_bgra_pixel_maps_to_red_rgba_and_i420() {
        let bgra = [
            0u8, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255,
        ];
        let rgba = bgra_to_rgba(&bgra, 8, 2, 2);
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);

        let planes = bgra_to_i420(&bgra, 8, 2, 2);
        assert_eq!(planes.width, 2);
        assert_eq!(planes.height, 2);
        assert!(planes.y.iter().all(|sample| (60..=90).contains(sample)));
        assert!(planes.u[0] < 128);
        assert!(planes.v[0] > 128);

        let rgba = i420_to_rgba(I420View {
            y: &planes.y,
            u: &planes.u,
            v: &planes.v,
            stride_y: 2,
            stride_u: 1,
            stride_v: 1,
            width: 2,
            height: 2,
        });
        assert!(rgba[0] > 200);
        assert!(rgba[1] < 40);
        assert!(rgba[2] < 40);
    }

    #[test]
    fn downscale_limits_width() {
        let src = vec![0u8; 4 * 4 * 4];
        let (dst, width, height) = downscale_rgba(&src, 4, 4, 2);
        assert_eq!((width, height), (2, 2));
        assert_eq!(dst.len(), 2 * 2 * 4);
    }
}
