//! D3D11VA для видео-MFT ОС (COMPAT-MEDIA-OSDEC-WIN-D3D): устройство D3D11 с
//! видеоподдержкой, менеджер устройств для MFT, пул выходных текстур и
//! считывание кадра с GPU (staging-текстура → NV12/P010 в память).
//!
//! Кадр на GPU не остаётся: рендер Lumen берёт RGBA из `DecodedFrame::to_rgba8`,
//! поэтому здесь только копия в системную память. Передача текстуры рендеру без
//! копирования — отдельная задача.

use std::ffi::c_void;

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D10::ID3D10Multithread;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BIND_DECODER, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice};
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFDXGIDeviceManager, IMFMediaType, IMFSample, IMFVideoSampleAllocatorEx, MF_SA_BUFFERS_PER_SAMPLE,
    MF_SA_D3D11_BINDFLAGS, MF_SA_D3D11_SHARED_WITHOUT_MUTEX, MF_SA_D3D11_USAGE,
};

use super::{blocklist, Api};

fn hr(what: &str, e: windows::core::Error) -> String {
    format!("{what}: {e}")
}

/// Адаптер, на котором создано устройство.
#[derive(Debug, Clone)]
pub(super) struct AdapterInfo {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    /// Версия драйвера пользовательского режима, `a.b.c.d` (0 — драйвер не сказал).
    pub driver: u64,
}

impl AdapterInfo {
    pub(super) fn describe(&self) -> String {
        format!(
            "{} (vendor {:#06x}, device {:#06x}, драйвер {})",
            self.name,
            self.vendor,
            self.device,
            blocklist::format_version(self.driver)
        )
    }
}

/// Прочитанный с GPU кадр: байты плоскостей Y и UV подряд.
pub(super) struct GpuFrame {
    pub data: Vec<u8>,
    /// Байтов между строками обеих плоскостей.
    pub stride: usize,
    /// Строк в плоскости Y (высота текстуры с выравниванием): плоскость UV
    /// начинается со смещения `stride * rows`.
    pub rows: usize,
}

struct Staging {
    texture: ID3D11Texture2D,
    desc: D3D11_TEXTURE2D_DESC,
}

/// Устройство D3D11 и всё, что нужно MFT для аппаратного декодирования.
pub(super) struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    manager: IMFDXGIDeviceManager,
    pub(super) adapter: AdapterInfo,
    staging: Option<Staging>,
    allocator: Option<IMFVideoSampleAllocatorEx>,
}

impl Gpu {
    /// Устройство на адаптере по умолчанию. Ошибка — D3D-пути нет, видео
    /// декодируется программно: нет `MFCreateDXGIDeviceManager`/D3D11, адаптер в
    /// блок-листе, драйвер не умеет видео.
    pub(super) fn create(api: &Api) -> Result<Self, String> {
        let levels: [D3D_FEATURE_LEVEL; 4] =
            [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
        let (mut device, mut context) = (None, None);
        // SAFETY: выходные параметры — наши `Option`; адаптер по умолчанию.
        unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(|e| hr("D3D11CreateDevice", e))?;
        let (device, context) = device.zip(context).ok_or("D3D11CreateDevice не вернул устройство")?;

        let adapter = adapter_info(&device)?;
        if let Some(rule) = blocklist::find(blocklist::builtin(), adapter.vendor, adapter.device, adapter.driver) {
            return Err(format!("адаптер {} в блок-листе: {}", adapter.describe(), rule.note()));
        }

        // MFT и наш поток обращаются к устройству одновременно.
        if let Ok(mt) = device.cast::<ID3D10Multithread>() {
            // SAFETY: простой переключатель защиты устройства.
            unsafe {
                let _ = mt.SetMultithreadProtected(true);
            }
        }
        let (manager, token) = api.new_dxgi_manager()?;
        // SAFETY: токен выдан этим же менеджером.
        unsafe { manager.ResetDevice(&device, token) }.map_err(|e| hr("ResetDevice", e))?;
        Ok(Self { device, context, manager, adapter, staging: None, allocator: None })
    }

    /// Указатель на `IMFDXGIDeviceManager` для `MFT_MESSAGE_SET_D3D_MANAGER`.
    pub(super) fn manager_param(&self) -> usize {
        self.manager.as_raw() as usize
    }

    /// Подготовить выходные образцы под текущий выходной тип MFT: MFT сам
    /// текстуры не выделяет, их даёт пул `IMFVideoSampleAllocatorEx`.
    pub(super) fn init_allocator(&mut self, api: &Api, output: &IMFMediaType) -> Result<(), String> {
        if let Some(old) = self.allocator.take() {
            // SAFETY: освобождаем пул прежнего типа (смена разрешения потока).
            unsafe {
                let _ = old.UninitializeSampleAllocator();
            }
        }
        let allocator = api.new_sample_allocator()?;
        let attrs = api.new_attributes(4)?;
        // SAFETY: атрибуты и пул созданы выше; ключи — константы Media Foundation.
        unsafe {
            attrs.SetUINT32(&MF_SA_BUFFERS_PER_SAMPLE, 1).map_err(|e| hr("BUFFERS_PER_SAMPLE", e))?;
            attrs.SetUINT32(&MF_SA_D3D11_USAGE, D3D11_USAGE_DEFAULT.0 as u32).map_err(|e| hr("D3D11_USAGE", e))?;
            attrs.SetUINT32(&MF_SA_D3D11_BINDFLAGS, D3D11_BIND_DECODER.0 as u32).map_err(|e| hr("D3D11_BINDFLAGS", e))?;
            attrs.SetUINT32(&MF_SA_D3D11_SHARED_WITHOUT_MUTEX, 1).map_err(|e| hr("SHARED_WITHOUT_MUTEX", e))?;
            allocator.SetDirectXManager(&self.manager).map_err(|e| hr("SetDirectXManager", e))?;
            allocator
                .InitializeSampleAllocatorEx(POOL_INITIAL, POOL_MAX, &attrs, output)
                .map_err(|e| hr("InitializeSampleAllocatorEx", e))?;
        }
        self.allocator = Some(allocator);
        Ok(())
    }

    /// Пустой выходной образец из пула (текстура для декодера).
    pub(super) fn allocate_sample(&self) -> Result<IMFSample, String> {
        let allocator = self.allocator.as_ref().ok_or("пул выходных образцов не создан")?;
        // SAFETY: пул инициализирован `init_allocator`.
        unsafe { allocator.AllocateSample() }.map_err(|e| hr("AllocateSample", e))
    }

    /// Скопировать декодированную текстуру образца в системную память.
    pub(super) fn read_frame(&mut self, sample: &IMFSample) -> Result<GpuFrame, String> {
        // SAFETY: образец от MFT с включённым D3D: его буфер — `IMFDXGIBuffer`.
        let (texture, subresource) = unsafe {
            let buffer = sample.GetBufferByIndex(0).map_err(|e| hr("GetBufferByIndex", e))?;
            let dxgi: IMFDXGIBuffer = buffer.cast().map_err(|e| hr("IMFDXGIBuffer", e))?;
            let mut raw: *mut c_void = std::ptr::null_mut();
            dxgi.GetResource(&ID3D11Texture2D::IID, &mut raw).map_err(|e| hr("GetResource", e))?;
            let texture = ID3D11Texture2D::from_raw(raw);
            (texture, dxgi.GetSubresourceIndex().map_err(|e| hr("GetSubresourceIndex", e))?)
        };
        let mut src = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: чтение описания живой текстуры.
        unsafe { texture.GetDesc(&mut src) };
        let staging = self.staging_for(&src)?;
        // SAFETY: копия одной подресурсы в staging того же формата и размера, затем
        // чтение через `Map`; Map/Unmap парные, срез не переживает Unmap.
        unsafe {
            self.context.CopySubresourceRegion(&staging, 0, 0, 0, 0, &texture, subresource, None);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|e| hr("Map", e))?;
            let stride = mapped.RowPitch as usize;
            let rows = src.Height as usize;
            let len = stride * rows * 3 / 2;
            let data = std::slice::from_raw_parts(mapped.pData as *const u8, len).to_vec();
            self.context.Unmap(&staging, 0);
            Ok(GpuFrame { data, stride, rows })
        }
    }

    /// Staging-текстура того же формата и размера, что у `src` (кэшируется).
    fn staging_for(&mut self, src: &D3D11_TEXTURE2D_DESC) -> Result<ID3D11Texture2D, String> {
        if let Some(s) = &self.staging
            && s.desc.Width == src.Width
            && s.desc.Height == src.Height
            && s.desc.Format == src.Format
        {
            return Ok(s.texture.clone());
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: src.Width,
            Height: src.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: src.Format,
            SampleDesc: src.SampleDesc,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut texture = None;
        // SAFETY: описание заполнено выше; результат — наш `Option`.
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut texture)) }.map_err(|e| hr("CreateTexture2D(staging)", e))?;
        let texture = texture.ok_or("CreateTexture2D не вернул текстуру")?;
        self.staging = Some(Staging { texture: texture.clone(), desc });
        Ok(texture)
    }
}

/// Образцов в пуле: с запасом на очередь переупорядочивания B-кадров.
const POOL_INITIAL: u32 = 4;
const POOL_MAX: u32 = 24;

fn adapter_info(device: &ID3D11Device) -> Result<AdapterInfo, String> {
    let dxgi: IDXGIDevice = device.cast().map_err(|e| hr("IDXGIDevice", e))?;
    // SAFETY: запросы описания адаптера живого устройства.
    unsafe {
        let adapter = dxgi.GetAdapter().map_err(|e| hr("GetAdapter", e))?;
        let desc = adapter.GetDesc().map_err(|e| hr("GetDesc", e))?;
        let end = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        // `CheckInterfaceSupport` возвращает версию драйвера как 64-битное число.
        let driver = adapter.CheckInterfaceSupport(&IDXGIDevice::IID).map_or(0, |v| v as u64);
        Ok(AdapterInfo {
            name: String::from_utf16_lossy(&desc.Description[..end]),
            vendor: desc.VendorId,
            device: desc.DeviceId,
            driver,
        })
    }
}
