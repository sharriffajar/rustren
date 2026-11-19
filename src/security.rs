use anyhow::{anyhow, bail, Result};
use std::net::{IpAddr, ToSocketAddrs};

use crate::schema::{Layer, LayerKind, Scene};

/// Validates that a remote URL uses http/https and does not resolve to private, loopback,
/// link-local, or otherwise non-routable IP addresses (SSRF protection).
pub fn validate_remote_url(raw_url: &str) -> Result<()> {
    let trimmed = raw_url.trim();
    let rest = if let Some(r) = trimmed.strip_prefix("https://") {
        r
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        r
    } else {
        bail!("Only http:// and https:// URLs are allowed: {raw_url}");
    };

    let authority = rest.split('/').next().unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or("");
    if host_port.is_empty() {
        bail!("Missing host in URL: {raw_url}");
    }

    let (host, port) = if host_port.starts_with('[') {
        let end = host_port
            .find(']')
            .ok_or_else(|| anyhow!("Invalid IPv6 host in URL: {raw_url}"))?;
        let h = &host_port[1..end];
        let p = host_port[end + 1..]
            .strip_prefix(':')
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(if trimmed.starts_with("https://") { 443 } else { 80 });
        (h, p)
    } else if let Some((h, p_str)) = host_port.rsplit_once(':') {
        let p = p_str.parse::<u16>().unwrap_or(80);
        (h, p)
    } else {
        (host_port, if trimmed.starts_with("https://") { 443 } else { 80 })
    };

    if host.eq_ignore_ascii_case("localhost") {
        bail!("Blocked SSRF target host: {host}");
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(ip) {
            bail!("Blocked SSRF target IP: {ip}");
        }
        return Ok(());
    }

    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| anyhow!("Failed to resolve host '{host}': {e}"))?;

    let mut resolved_any = false;
    for addr in addrs {
        resolved_any = true;
        if is_blocked_ip(addr.ip()) {
            bail!("Blocked SSRF resolved IP {} for host '{host}'", addr.ip());
        }
    }
    if !resolved_any {
        bail!("Host '{host}' resolved to no addresses");
    }
    Ok(())
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.octets()[0] == 0
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0b1100_0000) == 64) // CGNAT 100.64.0.0/10
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || ((v6.segments()[0] & 0xfe00) == 0xfc00) // Unique local fc00::/7
                || ((v6.segments()[0] & 0xffc0) == 0xfe80) // Link-local fe80::/10
        }
    }
}

#[cfg_attr(not(feature = "pro"), allow(dead_code))]
#[derive(Clone)]
pub struct ServiceLimits {
    pub allow_local_paths: bool,
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: u64,
    pub max_fps: u32,
    pub max_duration_secs: f32,
}

impl Default for ServiceLimits {
    fn default() -> Self {
        Self {
            allow_local_paths: false,
            max_width: 1920,
            max_height: 1920,
            max_pixels: 1080 * 1920,
            max_fps: 60,
            max_duration_secs: 60.0,
        }
    }
}

/// Validates scene limits and asset sources for untrusted API submissions.
#[cfg_attr(not(feature = "pro"), allow(dead_code))]
pub fn validate_scene_for_service(scene: &Scene, limits: &ServiceLimits) -> Result<()> {
    if scene.canvas.width == 0 || scene.canvas.height == 0 {
        bail!("Canvas dimensions must be > 0");
    }
    if scene.canvas.width > limits.max_width || scene.canvas.height > limits.max_height {
        bail!(
            "Canvas dimensions {}x{} exceed maximum allowed {}x{}",
            scene.canvas.width,
            scene.canvas.height,
            limits.max_width,
            limits.max_height
        );
    }
    let pixels = scene.canvas.width as u64 * scene.canvas.height as u64;
    if pixels > limits.max_pixels {
        bail!(
            "Canvas pixel count ({}) exceeds maximum allowed ({})",
            pixels,
            limits.max_pixels
        );
    }
    if scene.canvas.fps == 0 || scene.canvas.fps > limits.max_fps {
        bail!("Canvas fps must be between 1 and {}", limits.max_fps);
    }
    if scene.canvas.duration_secs <= 0.0 || scene.canvas.duration_secs > limits.max_duration_secs {
        bail!(
            "Canvas duration_secs ({:.2}s) must be between 0 and {:.2}s",
            scene.canvas.duration_secs,
            limits.max_duration_secs
        );
    }

    for font in &scene.fonts {
        validate_asset_ref(font, limits.allow_local_paths, false)?;
    }
    for audio in &scene.audio {
        validate_asset_ref(&audio.src, limits.allow_local_paths, false)?;
    }
    validate_layers_assets(&scene.layers, limits.allow_local_paths)?;
    Ok(())
}

#[cfg_attr(not(feature = "pro"), allow(dead_code))]
fn validate_layers_assets(layers: &[Layer], allow_local_paths: bool) -> Result<()> {
    for layer in layers {
        match &layer.kind {
            LayerKind::Image { src, .. } | LayerKind::Video { src, .. } => {
                validate_asset_ref(src, allow_local_paths, false)?
            }
            LayerKind::Svg { src, .. } => validate_asset_ref(src, allow_local_paths, true)?,
            LayerKind::Container { children, .. } => {
                validate_layers_assets(children, allow_local_paths)?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg_attr(not(feature = "pro"), allow(dead_code))]
fn validate_asset_ref(src: &str, allow_local_paths: bool, allow_inline_svg: bool) -> Result<()> {
    let trimmed = src.trim();
    if allow_inline_svg && trimmed.starts_with("<svg") {
        return Ok(());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return validate_remote_url(trimmed);
    }
    if !allow_local_paths {
        bail!("Local file paths are disabled in service mode: {src}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssrf_blocks_private_and_loopback() {
        assert!(validate_remote_url("http://127.0.0.1:8080/secret").is_err());
        assert!(validate_remote_url("http://localhost/admin").is_err());
        assert!(validate_remote_url("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(validate_remote_url("http://192.168.1.10/img.png").is_err());
        assert!(validate_remote_url("http://10.0.0.1/audio.mp3").is_err());
        assert!(validate_remote_url("http://[::1]/test").is_err());
        assert!(validate_remote_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn test_ssrf_allows_public_ip() {
        assert!(validate_remote_url("https://93.184.216.34/image.png").is_ok());
    }
}
