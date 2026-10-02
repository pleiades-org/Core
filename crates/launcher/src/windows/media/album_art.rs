//! Album art for the now-playing bar: decoded at the bar's size on the media worker, with the
//! window's rounded-corner coverage, into an icon the bar draws like any app icon.
use super::read_sessions::{ReadSession, OPERATION_TIMEOUT};
use crate::windows::{
    application_icon::ApplicationIcon,
    window_placement::{Corner, CornerMask},
};
use std::sync::Arc;
use windows::{
    core::Result,
    Graphics::Imaging::{
        BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat,
        BitmapTransform, ColorManagementMode, ExifOrientationMode,
    },
    Storage::Streams::IRandomAccessStreamReference,
    Win32::{
        Foundation::{E_FAIL, TRUE},
        Graphics::Gdi::*,
        UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO},
    },
};

/// Decoded images kept for tracks shown recently, so an event that changes nothing visible
/// does not decode the same art again.
const CACHE_ENTRIES: usize = 4;
/// Largest edge decoded, in pixels; the bar's art is 40 px at 100% scaling.
const MAX_EDGE: u32 = 256;
/// Corner rounding as a share of the edge, matching the result icons' rounded tiles.
const CORNER_DIVISOR: u32 = 6;

type ArtKey = (Arc<str>, Arc<str>, Arc<str>, Arc<str>, u32);

#[derive(Default)]
pub(super) struct ArtCache {
    entries: Vec<(ArtKey, Arc<ApplicationIcon>)>,
}

impl ArtCache {
    /// None when the player provides no art or it cannot be decoded; the bar then shows the
    /// player's own icon.
    pub fn get(&mut self, entry: &ReadSession, edge: u32) -> Option<Arc<ApplicationIcon>> {
        let info = &entry.info;
        let key = (
            info.app_id.clone(),
            info.title.clone(),
            info.artist.clone(),
            info.album.clone(),
            edge,
        );
        if let Some(index) = self.entries.iter().position(|(cached, _)| *cached == key) {
            let found = self.entries.remove(index);
            let icon = found.1.clone();
            self.entries.push(found);
            return Some(icon);
        }
        let reference = entry.properties.as_ref()?.Thumbnail().ok()?;
        let icon = match decode(&reference, edge.clamp(1, MAX_EDGE)) {
            Ok(icon) => Arc::new(icon),
            Err(error) => {
                eprintln!("Could not decode album art from {}: {error}", info.app_name);
                return None;
            }
        };
        if self.entries.len() == CACHE_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push((key, icon.clone()));
        Some(icon)
    }
}

fn decode(reference: &IRandomAccessStreamReference, edge: u32) -> Result<ApplicationIcon> {
    let stream = finish!(reference.OpenReadAsync()?, OPERATION_TIMEOUT)?;
    let decoder = finish!(BitmapDecoder::CreateAsync(&stream)?, OPERATION_TIMEOUT)?;
    let (width, height) = (decoder.PixelWidth()?.max(1), decoder.PixelHeight()?.max(1));
    let short_side = width.min(height);
    let scaled_width = (u64::from(width) * u64::from(edge) / u64::from(short_side)) as u32;
    let scaled_height = (u64::from(height) * u64::from(edge) / u64::from(short_side)) as u32;
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(scaled_width.max(edge))?;
    transform.SetScaledHeight(scaled_height.max(edge))?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    // Bounds apply after scaling: wide video thumbnails are cropped to their centre square.
    transform.SetBounds(BitmapBounds {
        X: scaled_width.saturating_sub(edge) / 2,
        Y: scaled_height.saturating_sub(edge) / 2,
        Width: edge,
        Height: edge,
    })?;
    let provider = finish!(
        decoder.GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?,
        OPERATION_TIMEOUT
    )?;
    let mut pixels = provider.DetachPixelData()?.to_vec();
    let edge = edge as usize;
    if pixels.len() != edge * edge * 4 {
        return Err(windows::core::Error::new(
            E_FAIL,
            "Unexpected album art size.",
        ));
    }
    round_corners(&mut pixels, edge, edge / CORNER_DIVISOR as usize);
    icon_from_pixels(&pixels, edge as i32)
}

/// Straight-alpha BGRA, so only the alpha channel changes.
fn round_corners(pixels: &mut [u8], edge: usize, radius: usize) {
    if radius == 0 || radius * 2 > edge {
        return;
    }
    let mask = CornerMask::new(radius as i32);
    for corner in Corner::ALL {
        let (left, top) = match corner {
            Corner::TopLeft => (0, 0),
            Corner::TopRight => (edge - radius, 0),
            Corner::BottomLeft => (0, edge - radius),
            Corner::BottomRight => (edge - radius, edge - radius),
        };
        for y in 0..radius {
            for x in 0..radius {
                let alpha = &mut pixels[((top + y) * edge + left + x) * 4 + 3];
                let coverage = u16::from(mask.coverage(corner, x as i32, y as i32));
                *alpha = (u16::from(*alpha) * coverage / 255) as u8;
            }
        }
    }
}

fn icon_from_pixels(pixels: &[u8], edge: i32) -> Result<ApplicationIcon> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: edge,
            // Negative: rows run top to bottom, as decoded.
            biHeight: -edge,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let color = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
    unsafe {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
    }
    // The alpha channel decides transparency; the monochrome mask is required but unused.
    let mask_stride = ((edge as usize).div_ceil(16)) * 2;
    let mask_bits = vec![0_u8; mask_stride * edge as usize];
    let mask = unsafe { CreateBitmap(edge, edge, 1, 1, Some(mask_bits.as_ptr().cast())) };
    let icon = unsafe {
        CreateIconIndirect(&ICONINFO {
            fIcon: TRUE,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        })
    };
    unsafe {
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
    }
    ApplicationIcon::from_handle(icon?).ok_or_else(windows::core::Error::from_win32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_corners_fade_only_the_corner_pixels() {
        let edge = 24;
        let mut pixels = vec![255_u8; edge * edge * 4];
        round_corners(&mut pixels, edge, 6);
        let alpha = |x: usize, y: usize| pixels[(y * edge + x) * 4 + 3];
        for (x, y) in [(0, 0), (edge - 1, 0), (0, edge - 1), (edge - 1, edge - 1)] {
            assert!(alpha(x, y) < 64, "corner {x},{y}");
        }
        assert_eq!(alpha(edge / 2, edge / 2), 255);
        assert_eq!(alpha(edge / 2, 0), 255);
        // Colour channels are untouched.
        assert!(pixels.chunks(4).all(|pixel| pixel[..3] == [255, 255, 255]));
    }

    #[test]
    fn decoded_pixels_become_an_icon_and_release_their_bitmaps() {
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS,
        };
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let objects = || unsafe { GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) };
        let before = objects();
        let icon = icon_from_pixels(&vec![200_u8; 40 * 40 * 4], 40).unwrap();
        drop(icon);
        assert_eq!(objects(), before);
    }
}
