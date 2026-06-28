use anyhow::{Context, Result};
use base64::Engine;
use image::DynamicImage;
use xcap::Monitor;

pub fn capture_screen(monitor_index: usize) -> Result<String> {
    let monitors = Monitor::all().context("无法获取屏幕列表")?;

    let monitor = if monitor_index == 0 {
        monitors.first().context("没有可用的屏幕")?
    } else {
        monitors
            .get(monitor_index - 1)
            .unwrap_or_else(|| &monitors[0])
    };

    let image = monitor.capture_image().context("截取屏幕失败")?;

    let dyn_img = DynamicImage::ImageRgba8(image);

    let mut png_data = Vec::new();
    dyn_img
        .write_to(&mut std::io::Cursor::new(&mut png_data), image::ImageFormat::Png)
        .context("PNG编码失败")?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);
    Ok(b64)
}