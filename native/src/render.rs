//! 绘制协议 v1 的统一 CPU 栅格化实现。

use crate::protocol::{self, PayloadReader};
use cosmic_text::{Attrs, Buffer, Color as TextColor, FontSystem, Metrics, Shaping, SwashCache};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use tiny_skia::{
    FillRule, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform,
};

pub const OP_CLEAR: u16 = 1;
pub const OP_SAVE: u16 = 2;
pub const OP_RESTORE: u16 = 3;
pub const OP_CLIP_RECT: u16 = 4;
pub const OP_TRANSFORM: u16 = 5;
pub const OP_LAYER: u16 = 6;
pub const OP_FILL_RECT: u16 = 7;
pub const OP_STROKE_RECT: u16 = 8;
pub const OP_ROUNDED_RECT: u16 = 9;
pub const OP_LINE: u16 = 10;
pub const OP_CIRCLE: u16 = 11;
pub const OP_SHADOW: u16 = 12;
pub const OP_TEXT: u16 = 13;
pub const OP_IMAGE: u16 = 14;
pub const OP_PATH: u16 = 15;

const MAX_DIMENSION: u32 = 16_384;
const MAX_STATE_DEPTH: usize = 256;
const MAX_PATH_VERBS: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    Protocol(protocol::ProtocolError),
    Size,
    Payload(u16),
    State,
    Path,
    Image,
}

impl Display for RenderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => Display::fmt(error, formatter),
            Self::Size => formatter.write_str("绘制表面尺寸无效或超限"),
            Self::Payload(opcode) => write!(formatter, "绘制命令 {opcode} 的负载无效"),
            Self::State => formatter.write_str("绘制状态栈无效或超限"),
            Self::Path => formatter.write_str("绘制路径无效或超限"),
            Self::Image => formatter.write_str("绘制图片数据无效"),
        }
    }
}

impl Error for RenderError {}

impl From<protocol::ProtocolError> for RenderError {
    fn from(value: protocol::ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

#[derive(Clone)]
struct RenderState {
    transform: Transform,
    clip: Option<Mask>,
    opacity: f32,
}

pub struct RenderedFrame {
    pixmap: Pixmap,
}

impl RenderedFrame {
    #[must_use]
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        self.pixmap.data()
    }

    #[must_use]
    pub fn xrgb(&self) -> Vec<u32> {
        self.pixmap
            .data()
            .chunks_exact(4)
            .map(|rgba| (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]))
            .collect()
    }
}

pub struct RenderEngine {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl Default for RenderEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        }
    }

    pub fn render<F>(
        &mut self,
        bytes: &[u8],
        width: u32,
        height: u32,
        scale_factor: f32,
        mut image_lookup: F,
    ) -> Result<RenderedFrame, RenderError>
    where
        F: FnMut(u64) -> Option<ImageData>,
    {
        if width == 0
            || height == 0
            || width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || !scale_factor.is_finite()
            || scale_factor <= 0.0
        {
            return Err(RenderError::Size);
        }
        let frame = protocol::decode(bytes)?;
        let mut pixmap = Pixmap::new(width, height).ok_or(RenderError::Size)?;
        let mut state = RenderState {
            transform: Transform::from_scale(scale_factor, scale_factor),
            clip: None,
            opacity: 1.0,
        };
        let mut stack = Vec::new();

        for command in frame.commands {
            match command.opcode {
                OP_CLEAR => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let color = read_color(&mut reader, 1.0)?;
                    reader
                        .finish()
                        .map_err(|_| RenderError::Payload(OP_CLEAR))?;
                    pixmap.fill(color);
                }
                OP_SAVE => {
                    empty_payload(&command.payload, OP_SAVE)?;
                    if stack.len() >= MAX_STATE_DEPTH {
                        return Err(RenderError::State);
                    }
                    stack.push(state.clone());
                }
                OP_RESTORE => {
                    empty_payload(&command.payload, OP_RESTORE)?;
                    state = stack.pop().ok_or(RenderError::State)?;
                }
                OP_CLIP_RECT => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let rect = read_rect(&mut reader, OP_CLIP_RECT)?;
                    reader
                        .finish()
                        .map_err(|_| RenderError::Payload(OP_CLIP_RECT))?;
                    let path = PathBuilder::from_rect(rect);
                    if let Some(mask) = &mut state.clip {
                        mask.intersect_path(&path, FillRule::Winding, true, state.transform);
                    } else {
                        let mut mask = Mask::new(width, height).ok_or(RenderError::Size)?;
                        mask.fill_path(&path, FillRule::Winding, true, state.transform);
                        state.clip = Some(mask);
                    }
                }
                OP_TRANSFORM => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let transform = Transform::from_row(
                        reader.f32()?,
                        reader.f32()?,
                        reader.f32()?,
                        reader.f32()?,
                        reader.f32()?,
                        reader.f32()?,
                    );
                    reader
                        .finish()
                        .map_err(|_| RenderError::Payload(OP_TRANSFORM))?;
                    if !transform.is_valid() {
                        return Err(RenderError::Payload(OP_TRANSFORM));
                    }
                    state.transform = state.transform.post_concat(transform);
                }
                OP_LAYER => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let opacity = reader.f32()?;
                    reader
                        .finish()
                        .map_err(|_| RenderError::Payload(OP_LAYER))?;
                    if !(0.0..=1.0).contains(&opacity) {
                        return Err(RenderError::Payload(OP_LAYER));
                    }
                    state.opacity *= opacity;
                }
                OP_FILL_RECT => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let rect = read_rect(&mut reader, OP_FILL_RECT)?;
                    let color = read_color(&mut reader, state.opacity)?;
                    reader
                        .finish()
                        .map_err(|_| RenderError::Payload(OP_FILL_RECT))?;
                    let paint = paint(color);
                    pixmap.fill_rect(rect, &paint, state.transform, state.clip.as_ref());
                }
                OP_STROKE_RECT => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let rect = read_rect(&mut reader, OP_STROKE_RECT)?;
                    let width = positive(reader.f32()?, OP_STROKE_RECT)?;
                    let color = read_color(&mut reader, state.opacity)?;
                    reader
                        .finish()
                        .map_err(|_| RenderError::Payload(OP_STROKE_RECT))?;
                    let path = PathBuilder::from_rect(rect);
                    stroke_path(&mut pixmap, &state, &path, width, color);
                }
                OP_ROUNDED_RECT => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let rect = read_rect(&mut reader, OP_ROUNDED_RECT)?;
                    let radius = non_negative(reader.f32()?, OP_ROUNDED_RECT)?;
                    let color = read_color(&mut reader, state.opacity)?;
                    reader
                        .finish()
                        .map_err(|_| RenderError::Payload(OP_ROUNDED_RECT))?;
                    let path = rounded_rect(rect, radius)?;
                    pixmap.fill_path(
                        &path,
                        &paint(color),
                        FillRule::Winding,
                        state.transform,
                        state.clip.as_ref(),
                    );
                }
                OP_LINE => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let [x1, y1, x2, y2] = read_floats(&mut reader)?;
                    let width = positive(reader.f32()?, OP_LINE)?;
                    let color = read_color(&mut reader, state.opacity)?;
                    reader.finish().map_err(|_| RenderError::Payload(OP_LINE))?;
                    let mut builder = PathBuilder::new();
                    builder.move_to(x1, y1);
                    builder.line_to(x2, y2);
                    let path = builder.finish().ok_or(RenderError::Path)?;
                    stroke_path(&mut pixmap, &state, &path, width, color);
                }
                OP_CIRCLE => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let x = reader.f32()?;
                    let y = reader.f32()?;
                    let radius = positive(reader.f32()?, OP_CIRCLE)?;
                    let color = read_color(&mut reader, state.opacity)?;
                    reader
                        .finish()
                        .map_err(|_| RenderError::Payload(OP_CIRCLE))?;
                    let path = PathBuilder::from_circle(x, y, radius).ok_or(RenderError::Path)?;
                    pixmap.fill_path(
                        &path,
                        &paint(color),
                        FillRule::Winding,
                        state.transform,
                        state.clip.as_ref(),
                    );
                }
                OP_SHADOW => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let rect = read_rect(&mut reader, OP_SHADOW)?;
                    let radius = non_negative(reader.f32()?, OP_SHADOW)?;
                    let blur = non_negative(reader.f32()?, OP_SHADOW)?;
                    let offset_x = reader.f32()?;
                    let offset_y = reader.f32()?;
                    let color = read_color(&mut reader, state.opacity)?;
                    reader
                        .finish()
                        .map_err(|_| RenderError::Payload(OP_SHADOW))?;
                    draw_shadow(
                        &mut pixmap,
                        &state,
                        rect,
                        radius,
                        blur,
                        [offset_x, offset_y],
                        color,
                    )?;
                }
                OP_TEXT => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let x = reader.f32()?;
                    let y = reader.f32()?;
                    let max_width = positive(reader.f32()?, OP_TEXT)?;
                    let font_size = positive(reader.f32()?, OP_TEXT)?;
                    let line_height = positive(reader.f32()?, OP_TEXT)?;
                    let color = read_rgba(&mut reader, state.opacity)?;
                    let text = reader.text()?.to_owned();
                    reader.finish().map_err(|_| RenderError::Payload(OP_TEXT))?;
                    self.draw_text(
                        &mut pixmap,
                        &state,
                        [x, y],
                        max_width,
                        font_size,
                        line_height,
                        color,
                        &text,
                    );
                }
                OP_IMAGE => {
                    let mut reader = PayloadReader::new(&command.payload);
                    let handle = reader.u64()?;
                    let rect = read_rect(&mut reader, OP_IMAGE)?;
                    let opacity = reader.f32()?;
                    reader
                        .finish()
                        .map_err(|_| RenderError::Payload(OP_IMAGE))?;
                    if !(0.0..=1.0).contains(&opacity) {
                        return Err(RenderError::Payload(OP_IMAGE));
                    }
                    if let Some(image) = image_lookup(handle) {
                        draw_image(&mut pixmap, &state, rect, opacity, &image)?;
                    }
                }
                OP_PATH => draw_path(&mut pixmap, &state, &command.payload)?,
                _ => {
                    // 带长度记录允许同一主版本的旧后端安全跳过新操作码。
                }
            }
        }
        if !stack.is_empty() {
            return Err(RenderError::State);
        }
        Ok(RenderedFrame { pixmap })
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &mut self,
        pixmap: &mut Pixmap,
        state: &RenderState,
        origin: [f32; 2],
        max_width: f32,
        font_size: f32,
        line_height: f32,
        color: [u8; 4],
        text: &str,
    ) {
        let mut point = tiny_skia::Point::from_xy(origin[0], origin[1]);
        state.transform.map_point(&mut point);
        let (scale_x, scale_y) = state.transform.get_scale();
        let scale = ((scale_x.abs() + scale_y.abs()) * 0.5).max(0.01);
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(font_size * scale, line_height * scale),
        );
        let mut buffer = buffer.borrow_with(&mut self.font_system);
        buffer.set_size(Some(max_width * scale), None);
        buffer.set_text(text, &Attrs::new(), Shaping::Advanced, None);
        buffer.shape_until_scroll(true);
        let text_color = TextColor::rgba(color[0], color[1], color[2], color[3]);
        let clip = state.clip.as_ref().map(Mask::data);
        let surface_width = pixmap.width() as usize;
        let surface_height = pixmap.height() as usize;
        let data = pixmap.data_mut();
        buffer.draw(
            &mut self.swash_cache,
            text_color,
            |x, y, width, height, glyph_color| {
                blend_glyph(
                    data,
                    surface_width,
                    surface_height,
                    clip,
                    x + point.x.round() as i32,
                    y + point.y.round() as i32,
                    width,
                    height,
                    [
                        glyph_color.r(),
                        glyph_color.g(),
                        glyph_color.b(),
                        glyph_color.a(),
                    ],
                );
            },
        );
    }
}

fn empty_payload(payload: &[u8], opcode: u16) -> Result<(), RenderError> {
    if payload.is_empty() {
        Ok(())
    } else {
        Err(RenderError::Payload(opcode))
    }
}

fn read_rect(reader: &mut PayloadReader<'_>, opcode: u16) -> Result<Rect, RenderError> {
    let [x, y, width, height] = read_floats(reader)?;
    if width <= 0.0 || height <= 0.0 {
        return Err(RenderError::Payload(opcode));
    }
    Rect::from_xywh(x, y, width, height).ok_or(RenderError::Payload(opcode))
}

fn read_floats<const N: usize>(reader: &mut PayloadReader<'_>) -> Result<[f32; N], RenderError> {
    let mut values = [0.0; N];
    for value in &mut values {
        *value = reader.f32()?;
    }
    Ok(values)
}

fn read_rgba(reader: &mut PayloadReader<'_>, opacity: f32) -> Result<[u8; 4], RenderError> {
    let red = reader.u8()?;
    let green = reader.u8()?;
    let blue = reader.u8()?;
    let alpha = ((f32::from(reader.u8()?) * opacity).round()).clamp(0.0, 255.0) as u8;
    Ok([red, green, blue, alpha])
}

fn read_color(
    reader: &mut PayloadReader<'_>,
    opacity: f32,
) -> Result<tiny_skia::Color, RenderError> {
    let [red, green, blue, alpha] = read_rgba(reader, opacity)?;
    Ok(tiny_skia::Color::from_rgba8(red, green, blue, alpha))
}

fn paint(color: tiny_skia::Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    paint
}

fn positive(value: f32, opcode: u16) -> Result<f32, RenderError> {
    if value > 0.0 {
        Ok(value)
    } else {
        Err(RenderError::Payload(opcode))
    }
}

fn non_negative(value: f32, opcode: u16) -> Result<f32, RenderError> {
    if value >= 0.0 {
        Ok(value)
    } else {
        Err(RenderError::Payload(opcode))
    }
}

fn stroke_path(
    pixmap: &mut Pixmap,
    state: &RenderState,
    path: &Path,
    width: f32,
    color: tiny_skia::Color,
) {
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pixmap.stroke_path(
        path,
        &paint(color),
        &stroke,
        state.transform,
        state.clip.as_ref(),
    );
}

fn rounded_rect(rect: Rect, radius: f32) -> Result<Path, RenderError> {
    let radius = radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
    if radius <= 0.0 {
        return Ok(PathBuilder::from_rect(rect));
    }
    let k = radius * 0.552_284_8;
    let left = rect.left();
    let top = rect.top();
    let right = rect.right();
    let bottom = rect.bottom();
    let mut path = PathBuilder::new();
    path.move_to(left + radius, top);
    path.line_to(right - radius, top);
    path.cubic_to(
        right - radius + k,
        top,
        right,
        top + radius - k,
        right,
        top + radius,
    );
    path.line_to(right, bottom - radius);
    path.cubic_to(
        right,
        bottom - radius + k,
        right - radius + k,
        bottom,
        right - radius,
        bottom,
    );
    path.line_to(left + radius, bottom);
    path.cubic_to(
        left + radius - k,
        bottom,
        left,
        bottom - radius + k,
        left,
        bottom - radius,
    );
    path.line_to(left, top + radius);
    path.cubic_to(
        left,
        top + radius - k,
        left + radius - k,
        top,
        left + radius,
        top,
    );
    path.close();
    path.finish().ok_or(RenderError::Path)
}

#[allow(clippy::too_many_arguments)]
fn draw_shadow(
    pixmap: &mut Pixmap,
    state: &RenderState,
    rect: Rect,
    radius: f32,
    blur: f32,
    offset: [f32; 2],
    color: tiny_skia::Color,
) -> Result<(), RenderError> {
    let steps = if blur <= 0.0 { 1 } else { 12 };
    for index in (0..steps).rev() {
        let ratio = (index + 1) as f32 / steps as f32;
        let spread = blur * ratio;
        let shadow_rect = Rect::from_xywh(
            rect.x() + offset[0] - spread,
            rect.y() + offset[1] - spread,
            rect.width() + spread * 2.0,
            rect.height() + spread * 2.0,
        )
        .ok_or(RenderError::Path)?;
        let mut shadow_color = color;
        shadow_color.set_alpha(color.alpha() * (1.0 - ratio * 0.75) / steps as f32);
        let path = rounded_rect(shadow_rect, radius + spread)?;
        pixmap.fill_path(
            &path,
            &paint(shadow_color),
            FillRule::Winding,
            state.transform,
            state.clip.as_ref(),
        );
    }
    Ok(())
}

fn draw_image(
    pixmap: &mut Pixmap,
    state: &RenderState,
    rect: Rect,
    opacity: f32,
    image: &ImageData,
) -> Result<(), RenderError> {
    let expected = usize::try_from(image.width)
        .ok()
        .and_then(|width| {
            usize::try_from(image.height)
                .ok()
                .map(|height| width * height)
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(RenderError::Image)?;
    if image.width == 0 || image.height == 0 || image.rgba.len() != expected {
        return Err(RenderError::Image);
    }
    let mut premultiplied = image.rgba.clone();
    for pixel in premultiplied.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        pixel[0] = ((u16::from(pixel[0]) * alpha + 127) / 255) as u8;
        pixel[1] = ((u16::from(pixel[1]) * alpha + 127) / 255) as u8;
        pixel[2] = ((u16::from(pixel[2]) * alpha + 127) / 255) as u8;
    }
    let source_size =
        tiny_skia::IntSize::from_wh(image.width, image.height).ok_or(RenderError::Image)?;
    let source = Pixmap::from_vec(premultiplied, source_size).ok_or(RenderError::Image)?;
    let image_transform = Transform::from_row(
        rect.width() / image.width as f32,
        0.0,
        0.0,
        rect.height() / image.height as f32,
        rect.x(),
        rect.y(),
    );
    let transform = state.transform.post_concat(image_transform);
    pixmap.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &PixmapPaint {
            opacity: opacity * state.opacity,
            ..PixmapPaint::default()
        },
        transform,
        state.clip.as_ref(),
    );
    Ok(())
}

fn draw_path(pixmap: &mut Pixmap, state: &RenderState, payload: &[u8]) -> Result<(), RenderError> {
    let mut reader = PayloadReader::new(payload);
    let style = reader.u8()?;
    let even_odd = reader.u8()? != 0;
    let verb_count = usize::from(reader.u16()?);
    if verb_count > MAX_PATH_VERBS {
        return Err(RenderError::Path);
    }
    let stroke_width = reader.f32()?;
    let color = read_color(&mut reader, state.opacity)?;
    let mut builder = PathBuilder::new();
    for _ in 0..verb_count {
        match reader.u8()? {
            1 => builder.move_to(reader.f32()?, reader.f32()?),
            2 => builder.line_to(reader.f32()?, reader.f32()?),
            3 => builder.quad_to(reader.f32()?, reader.f32()?, reader.f32()?, reader.f32()?),
            4 => builder.cubic_to(
                reader.f32()?,
                reader.f32()?,
                reader.f32()?,
                reader.f32()?,
                reader.f32()?,
                reader.f32()?,
            ),
            5 => builder.close(),
            _ => return Err(RenderError::Path),
        }
    }
    reader.finish().map_err(|_| RenderError::Path)?;
    let path = builder.finish().ok_or(RenderError::Path)?;
    match style {
        0 => pixmap.fill_path(
            &path,
            &paint(color),
            if even_odd {
                FillRule::EvenOdd
            } else {
                FillRule::Winding
            },
            state.transform,
            state.clip.as_ref(),
        ),
        1 => stroke_path(
            pixmap,
            state,
            &path,
            positive(stroke_width, OP_PATH)?,
            color,
        ),
        _ => return Err(RenderError::Path),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn blend_glyph(
    data: &mut [u8],
    surface_width: usize,
    surface_height: usize,
    clip: Option<&[u8]>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: [u8; 4],
) {
    for row in 0..height {
        let destination_y = y + row as i32;
        let Ok(destination_y) = usize::try_from(destination_y) else {
            continue;
        };
        if destination_y >= surface_height {
            continue;
        }
        for column in 0..width {
            let destination_x = x + column as i32;
            let Ok(destination_x) = usize::try_from(destination_x) else {
                continue;
            };
            if destination_x >= surface_width {
                continue;
            }
            let pixel_index = destination_y * surface_width + destination_x;
            let clip_alpha = clip.map_or(255, |mask| mask[pixel_index]);
            if clip_alpha == 0 {
                continue;
            }
            let byte_index = pixel_index * 4;
            blend_pixel(&mut data[byte_index..byte_index + 4], color, clip_alpha);
        }
    }
}

fn blend_pixel(destination: &mut [u8], color: [u8; 4], clip_alpha: u8) {
    let source_alpha = (u32::from(color[3]) * u32::from(clip_alpha) + 127) / u32::from(u8::MAX);
    let inverse = u32::from(u8::MAX) - source_alpha;
    for channel in 0..3 {
        let source = (u32::from(color[channel]) * source_alpha + 127) / 255;
        let existing = u32::from(destination[channel]);
        destination[channel] = (source + (existing * inverse + 127) / 255).min(255) as u8;
    }
    let existing_alpha = u32::from(destination[3]);
    destination[3] = (source_alpha + (existing_alpha * inverse + 127) / 255).min(255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Command, Frame, PayloadWriter, encode};

    fn command(opcode: u16, write: impl FnOnce(&mut PayloadWriter)) -> Command {
        let mut writer = PayloadWriter::new();
        write(&mut writer);
        Command::new(opcode, writer.finish())
    }

    fn rgba(writer: &mut PayloadWriter, color: [u8; 4]) {
        for channel in color {
            writer.u8(channel);
        }
    }

    fn rect(writer: &mut PayloadWriter, value: [f32; 4]) {
        for number in value {
            writer.f32(number).unwrap();
        }
    }

    #[test]
    fn clear_fill_clip_and_transform_produce_expected_pixels() {
        let frame = Frame {
            commands: vec![
                command(OP_CLEAR, |writer| rgba(writer, [255, 255, 255, 255])),
                command(OP_SAVE, |_| {}),
                command(OP_CLIP_RECT, |writer| rect(writer, [0.0, 0.0, 4.0, 8.0])),
                command(OP_TRANSFORM, |writer| {
                    for value in [1.0, 0.0, 0.0, 1.0, 1.0, 0.0] {
                        writer.f32(value).unwrap();
                    }
                }),
                command(OP_FILL_RECT, |writer| {
                    rect(writer, [0.0, 0.0, 8.0, 8.0]);
                    rgba(writer, [255, 0, 0, 255]);
                }),
                command(OP_RESTORE, |_| {}),
            ],
            ..Frame::default()
        };
        let bytes = encode(&frame).unwrap();
        let rendered = RenderEngine::new()
            .render(&bytes, 8, 8, 1.0, |_| None)
            .unwrap();
        let pixel = |x: usize, y: usize| &rendered.rgba()[(y * 8 + x) * 4..][..4];
        assert_eq!(pixel(2, 2), &[255, 0, 0, 255]);
        assert_eq!(pixel(6, 2), &[255, 255, 255, 255]);
    }

    #[test]
    fn rounded_shapes_paths_and_text_render_non_empty_frame() {
        let frame = Frame {
            commands: vec![
                command(OP_CLEAR, |writer| rgba(writer, [0, 0, 0, 255])),
                command(OP_ROUNDED_RECT, |writer| {
                    rect(writer, [2.0, 2.0, 28.0, 16.0]);
                    writer.f32(4.0).unwrap();
                    rgba(writer, [30, 120, 240, 255]);
                }),
                command(OP_CIRCLE, |writer| {
                    for value in [38.0, 10.0, 7.0] {
                        writer.f32(value).unwrap();
                    }
                    rgba(writer, [240, 80, 50, 255]);
                }),
                command(OP_TEXT, |writer| {
                    for value in [2.0, 22.0, 44.0, 10.0, 12.0] {
                        writer.f32(value).unwrap();
                    }
                    rgba(writer, [255, 255, 255, 255]);
                    writer.text("言台 UI").unwrap();
                }),
            ],
            ..Frame::default()
        };
        let bytes = encode(&frame).unwrap();
        let rendered = RenderEngine::new()
            .render(&bytes, 48, 40, 1.0, |_| None)
            .unwrap();
        assert!(rendered.rgba().chunks_exact(4).any(|pixel| pixel[2] > 100));
        assert_eq!(rendered.xrgb().len(), 48 * 40);
    }

    #[test]
    fn image_command_resolves_abi_resource_handle() {
        let frame = Frame {
            commands: vec![
                command(OP_CLEAR, |writer| rgba(writer, [0, 0, 0, 255])),
                command(OP_IMAGE, |writer| {
                    writer.u64(77);
                    rect(writer, [0.0, 0.0, 2.0, 2.0]);
                    writer.f32(1.0).unwrap();
                }),
            ],
            ..Frame::default()
        };
        let bytes = encode(&frame).unwrap();
        let image = ImageData {
            width: 1,
            height: 1,
            rgba: vec![10, 200, 30, 255],
        };
        let rendered = RenderEngine::new()
            .render(&bytes, 2, 2, 1.0, |handle| {
                (handle == 77).then(|| image.clone())
            })
            .unwrap();
        assert_eq!(&rendered.rgba()[..4], &[10, 200, 30, 255]);
    }

    #[test]
    fn state_stack_and_payload_damage_are_rejected() {
        let bytes = encode(&Frame {
            commands: vec![Command::new(OP_RESTORE, Vec::new())],
            ..Frame::default()
        })
        .unwrap();
        assert!(matches!(
            RenderEngine::new().render(&bytes, 2, 2, 1.0, |_| None),
            Err(RenderError::State)
        ));

        let damaged = encode(&Frame {
            commands: vec![Command::new(OP_FILL_RECT, vec![0; 3])],
            ..Frame::default()
        })
        .unwrap();
        assert!(matches!(
            RenderEngine::new().render(&damaged, 2, 2, 1.0, |_| None),
            Err(RenderError::Protocol(protocol::ProtocolError::Truncated))
        ));
    }
}
