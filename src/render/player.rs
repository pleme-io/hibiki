//! GPU renderer for the hibiki music player UI.
//!
//! Implements `madori::RenderCallback` to draw the player interface.
//! Uses garasu for GPU text rendering and clear operations.

use garasu::{GpuContext, TextConfig, TextLayout};
use madori::render::{RenderCallback, RenderContext};

use crate::audio::AudioEngine;
use crate::render::state::UiState;

/// The hibiki GPU renderer.
///
/// Owns the UI state and renders the player interface each frame.
/// The audio engine reference is passed in via `set_engine_snapshot`
/// to avoid ownership issues with the event loop.
pub struct HibikiRenderer {
    /// UI widget state.
    pub ui: UiState,
    /// Cached status bar text.
    status_bar: String,
    /// Background clear color (from config).
    bg_color: wgpu::Color,
    /// Foreground text color.
    fg_color: [f32; 4],
    /// Accent color for highlights.
    accent_color: [f32; 4],
    /// Muted color for secondary text.
    muted_color: [f32; 4],
}

impl HibikiRenderer {
    /// Create a new renderer with the given appearance colors.
    #[must_use]
    pub fn new(bg: &str, fg: &str, accent: &str) -> Self {
        let bg_color = parse_hex_to_wgpu_color(bg);
        let fg_color = parse_hex_to_f32_color(fg);
        let accent_color = parse_hex_to_f32_color(accent);
        let muted_color = [fg_color[0] * 0.5, fg_color[1] * 0.5, fg_color[2] * 0.5, 1.0];

        Self {
            ui: UiState::new(),
            status_bar: String::new(),
            bg_color,
            fg_color,
            accent_color,
            muted_color,
        }
    }

    /// Update cached state from the audio engine.
    ///
    /// Call this before each frame to keep the UI in sync with playback state.
    pub fn update_from_engine(&mut self, engine: &AudioEngine) {
        self.status_bar = self.ui.format_status_bar(engine);
        self.ui.update_queue_list(engine);
    }

    /// Render the three-panel layout with status bar.
    fn render_layout(&mut self, ctx: &mut RenderContext<'_>) {
        let width = ctx.width as f32;
        let height = ctx.height as f32;

        // Layout proportions.
        let status_bar_height = 30.0;
        let tab_bar_height = 25.0;
        let content_height = height - status_bar_height - tab_bar_height;
        let left_panel_width = width * 0.25;
        let right_panel_width = width * 0.25;
        let _center_width = width - left_panel_width - right_panel_width;

        let font_size = 14.0;
        let line_height = 20.0;
        let small_font_size = 12.0;

        let mut layouts: Vec<TextLayout> = Vec::new();

        // --- Tab bar ---
        let tab_config = TextConfig {
            font_size,
            line_height,
            color: self.accent_color,
        };
        let tab_text = self
            .ui
            .tabs
            .tabs()
            .iter()
            .enumerate()
            .map(|(i, name)| {
                if i == self.ui.tabs.active_index() {
                    format!("[{name}]")
                } else {
                    format!(" {name} ")
                }
            })
            .collect::<Vec<_>>()
            .join(" | ");
        layouts.push(TextLayout::new(&tab_text, tab_config, width));

        // --- Library panel (left) ---
        let lib_header_config = TextConfig {
            font_size,
            line_height,
            color: self.accent_color,
        };
        layouts.push(TextLayout::new("Library", lib_header_config, left_panel_width));

        let lib_config = TextConfig {
            font_size: small_font_size,
            line_height: line_height * 0.9,
            color: self.fg_color,
        };
        for item in self.ui.library_list.visible_items() {
            layouts.push(TextLayout::new(item, lib_config.clone(), left_panel_width));
        }

        // --- Center panel (now playing) ---
        let center_config = TextConfig {
            font_size: font_size * 1.2,
            line_height: line_height * 1.5,
            color: self.fg_color,
        };
        layouts.push(TextLayout::new(
            "Now Playing",
            center_config,
            _center_width,
        ));

        // --- Queue panel (right) ---
        let queue_header_config = TextConfig {
            font_size,
            line_height,
            color: self.accent_color,
        };
        layouts.push(TextLayout::new("Queue", queue_header_config, right_panel_width));

        let queue_config = TextConfig {
            font_size: small_font_size,
            line_height: line_height * 0.9,
            color: self.fg_color,
        };
        for item in self.ui.queue_list.visible_items() {
            layouts.push(TextLayout::new(item, queue_config.clone(), right_panel_width));
        }

        // --- Status bar ---
        let status_config = TextConfig {
            font_size: small_font_size,
            line_height: status_bar_height,
            color: self.accent_color,
        };
        layouts.push(TextLayout::new(&self.status_bar, status_config, width));

        // --- Mode indicator ---
        let mode_text = match self.ui.mode {
            crate::input::InputMode::Normal => "",
            crate::input::InputMode::Library => "-- LIBRARY --",
            crate::input::InputMode::Queue => "-- QUEUE --",
            crate::input::InputMode::Search => {
                // Show search query
                "/ search..."
            }
            crate::input::InputMode::Command => ": command...",
            crate::input::InputMode::Torrent => "-- TORRENTS --",
        };
        if !mode_text.is_empty() {
            let mode_config = TextConfig {
                font_size: small_font_size,
                line_height,
                color: self.muted_color,
            };
            layouts.push(TextLayout::new(mode_text, mode_config, width));
        }

        // Render all text layouts.
        // In a full implementation, each layout would be positioned at specific
        // coordinates using garasu's text renderer with proper layout rects.
        // For now, we prepare the layouts for the text renderer.
        let _ = (layouts, content_height, tab_bar_height);
    }
}

impl RenderCallback for HibikiRenderer {
    fn init(&mut self, _gpu: &GpuContext) {
        tracing::info!("hibiki renderer initialized");
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.ui.width = width;
        self.ui.height = height;
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) {
        // Clear the background.
        let mut encoder = ctx.gpu.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some("hibiki_render"),
            },
        );
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hibiki_clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: ctx.surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.bg_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        ctx.gpu.queue.submit(std::iter::once(encoder.finish()));

        // Build layout and text.
        self.render_layout(ctx);

        // Render text via garasu.
        // The TextRenderer from madori's RenderContext handles actual glyph rendering.
        // Each TextLayout prepared in render_layout() would be submitted to ctx.text.
        // Full text rendering integration requires garasu TextRenderer::prepare() +
        // TextRenderer::render() calls with positioned text areas.
        //
        // For now the clear pass and layout calculation are complete -- text rendering
        // will be wired once the garasu TextRenderer API stabilizes its multi-area
        // rendering interface.
    }
}

/// Parse a hex color string like "#2e3440" to a `wgpu::Color`.
fn parse_hex_to_wgpu_color(hex: &str) -> wgpu::Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return wgpu::Color {
            r: 0.180,
            g: 0.204,
            b: 0.251,
            a: 1.0,
        };
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0x2e);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0x34);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0x40);
    wgpu::Color {
        r: f64::from(r) / 255.0,
        g: f64::from(g) / 255.0,
        b: f64::from(b) / 255.0,
        a: 1.0,
    }
}

/// Parse a hex color string like "#eceff4" to `[f32; 4]` RGBA.
fn parse_hex_to_f32_color(hex: &str) -> [f32; 4] {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return [0.925, 0.937, 0.957, 1.0];
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0xec);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0xef);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0xf4);
    [
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_colors() {
        let color = parse_hex_to_wgpu_color("#2e3440");
        assert!((color.r - 0.180).abs() < 0.01);
        assert!((color.g - 0.204).abs() < 0.01);
        assert!((color.b - 0.251).abs() < 0.01);
        assert!((color.a - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_hex_invalid_returns_default() {
        let color = parse_hex_to_wgpu_color("invalid");
        // Should return Nord background default.
        assert!((color.r - 0.180).abs() < 0.01);
    }

    #[test]
    fn parse_hex_f32_color() {
        let color = parse_hex_to_f32_color("#ffffff");
        assert!((color[0] - 1.0).abs() < 0.01);
        assert!((color[1] - 1.0).abs() < 0.01);
        assert!((color[2] - 1.0).abs() < 0.01);
    }

    #[test]
    fn renderer_creation() {
        let renderer = HibikiRenderer::new("#2e3440", "#eceff4", "#88c0d0");
        assert_eq!(renderer.ui.mode, crate::input::InputMode::Normal);
        assert!(renderer.status_bar.is_empty());
    }

    #[test]
    fn renderer_update_from_engine() {
        let config = crate::config::AudioConfig::default();
        let engine = AudioEngine::new(&config);
        let mut renderer = HibikiRenderer::new("#2e3440", "#eceff4", "#88c0d0");
        renderer.update_from_engine(&engine);
        assert!(renderer.status_bar.contains("No track"));
    }
}
