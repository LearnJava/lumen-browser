//! SVG-FILTER-5: `filter: url(#id)` в живом окне (wgpu).
//!
//! Граф SVG-фильтра исполняется на CPU ([`crate::svg_filter`]) — общая математика
//! с `--screenshot`. Окно кладёт слой группы в staging-буфер, гонит цепочку
//! фильтров над ним тем же [`crate::cpu_raster::svg_filter_chain_window`], что и
//! CPU-растр, и композитирует результат квадом в родителя. Цена — синхронное
//! чтение слоя на каждый кадр, где такой фильтр есть: пока граф не перенесён на
//! шейдеры, это единственный путь к одинаковой картинке в обоих бэкендах.

use super::*;

/// Что нужно композиту SVG-фильтра помимо самой цепочки (считается при обходе
/// списка, где известны смещение прокрутки, трансформ и клип).
#[derive(Clone, Copy)]
pub(super) struct SvgFilterPlan {
    /// Border box элемента в CSS px экрана (после прокрутки и трансформа).
    pub bounds: Rect,
    /// Клип композита в device px (`None` — без клипа).
    pub clip: Option<DeviceScissor>,
}

/// Цель композита SVG-фильтра: куда рисовать и в каком масштабе.
#[derive(Clone, Copy)]
pub(super) struct SvgFilterTarget<'a> {
    /// View кадра — родитель на уровне 1.
    pub frame_view: &'a wgpu::TextureView,
    /// Размер цели в device px.
    pub surface: (u32, u32),
    /// Device px на CSS px.
    pub dpr: f32,
}

impl Renderer {
    /// Применяет цепочку `filters` (с хотя бы одним `url(#id)`) к слою уровня
    /// `from_level` и рисует результат в родителя (`frame_view` на уровне 1).
    /// `false` — путь недоступен (формат цели не 8-битный, висячая ссылка,
    /// сбой чтения): вызывающий отрисует группу прежним путём.
    pub(super) fn composite_svg_filter_layer(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        from_level: usize,
        filters: &[FilterFn],
        plan: &SvgFilterPlan,
        target: &SvgFilterTarget<'_>,
    ) -> bool {
        let SvgFilterTarget { frame_view, surface, dpr } = *target;
        let swap_rb = match self.surface_format {
            wgpu::TextureFormat::Bgra8Unorm => true,
            wgpu::TextureFormat::Rgba8Unorm => false,
            _ => return false,
        };
        // Висячая ссылка делает всю цепочку недействительной (Filter Effects L1 §3).
        if filters.iter().any(|f| matches!(f, FilterFn::Url(u) if u.graph.is_none())) {
            return false;
        }
        let layer = &self.layer_textures[from_level - 1];
        let (w, h) = (layer.width.min(surface.0), layer.height.min(surface.1));
        if w == 0 || h == 0 {
            return false;
        }
        let Some(mut src) = self.read_layer_pixmap(encoder, layer, (w, h), swap_rb) else {
            return false;
        };
        let canvas = (0.0, 0.0, w as f32, h as f32);
        let images = std::collections::HashMap::new();
        let Some((out, origin)) = crate::cpu_raster::svg_filter_chain_window(
            &src, filters, plan.bounds, canvas, &images, dpr,
        ) else {
            return false;
        };
        src = out;
        let (ow, oh) = (src.width(), src.height());
        let data = src.data_mut();
        if swap_rb {
            for px in data.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("svg-filter-result"),
            size: wgpu::Extent3d { width: ow, height: oh, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ow * 4),
                rows_per_image: Some(oh),
            },
            wgpu::Extent3d { width: ow, height: oh, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("svg-filter-result-bg"),
            layout: &self.composite_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
            ],
        });
        let mut verts: Vec<CompositeVertex> = Vec::with_capacity(6);
        push_region_dst_quad(
            &mut verts,
            [origin.0.max(0) as u32, origin.1.max(0) as u32, ow, oh],
            surface.0 as f32,
            surface.1 as f32,
            1.0,
        );
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("svg-filter-result-vbuf"),
            size: std::mem::size_of_val(verts.as_slice()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&vbuf, 0, as_bytes(verts.as_slice()));
        let dst_view = if from_level == 1 {
            frame_view
        } else {
            &self.layer_textures[from_level - 2].view
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("svg-filter-composite-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: self.depth_view.as_ref().map(|dv| {
                wgpu::RenderPassDepthStencilAttachment {
                    view: dv,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(self.composite_pipeline());
        if let Some(s) = plan.clip {
            pass.set_scissor_rect(s.x, s.y, s.width, s.height);
        }
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(..));
        pass.draw(0..6, 0..1);
        true
    }

    /// Синхронное чтение `w × h` левого верхнего угла слоя в premultiplied RGBA
    /// pixmap. Закрывает и отправляет записанное в `encoder` (слой должен быть
    /// дорисован), оставляя в нём пустой энкодер для продолжения кадра.
    fn read_layer_pixmap(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        layer: &OffscreenLayer,
        (w, h): (u32, u32),
        swap_rb: bool,
    ) -> Option<tiny_skia::Pixmap> {
        let unpadded = w * 4;
        let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("svg-filter-readback"),
            size: u64::from(padded) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            layer.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let next = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("encoder-svg-filter") });
        self.queue.submit([std::mem::replace(encoder, next).finish()]);
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::Wait).ok()?;
        rx.recv().ok()?.ok()?;
        let mut pm = tiny_skia::Pixmap::new(w, h)?;
        {
            let mapped = slice.get_mapped_range();
            let out = pm.data_mut();
            for row in 0..h as usize {
                let s = row * padded as usize;
                let d = row * unpadded as usize;
                out[d..d + unpadded as usize].copy_from_slice(&mapped[s..s + unpadded as usize]);
            }
            if swap_rb {
                for px in out.chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
            }
        }
        staging.unmap();
        Some(pm)
    }
}
