//! Direct hid.dll / SetupAPI transport (no extra libraries).

use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetFeature,
    HidD_GetHidGuid, HidD_GetManufacturerString, HidD_GetPreparsedData, HidD_GetProductString,
    HidD_GetSerialNumberString, HidP_GetCaps,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::core::GUID;

use crate::poller::{DeviceInfo, Report, Transport};

/// A HID top-level collection that is a UPS (usage page 0x84, usage 0x04).
#[derive(Clone, Debug)]
pub struct UpsInterface {
    /// NUL-terminated device interface path.
    pub path: Vec<u16>,
    pub vid: u16,
    pub pid: u16,
    pub manufacturer: String,
    pub product: String,
    pub serial: String,
}

impl UpsInterface {
    /// Stable identity: "VID:PID:serial".
    pub fn id(&self) -> String {
        format!("{:04X}:{:04X}:{}", self.vid, self.pid, self.serial)
    }

    pub fn info(&self, reader: &'static str) -> DeviceInfo {
        DeviceInfo {
            manufacturer: self.manufacturer.clone(),
            product: self.product.clone(),
            serial: self.serial.clone(),
            id: self.id(),
            reader,
            quirks: crate::generic::quirks(self.vid, &self.product),
        }
    }
}

/// Owned HID handle, closed on drop.
pub struct Handle(pub HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

// The handle is only used from one thread at a time.
unsafe impl Send for Handle {}

fn wide_str(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n]).trim().to_string()
}

/// Open shared, read/write if allowed, else with no access rights (feature
/// reports still work then).
pub fn open_path(path: &[u16]) -> Option<Handle> {
    for access in [GENERIC_READ | GENERIC_WRITE, 0] {
        let h = unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE {
            return Some(Handle(h));
        }
    }
    None
}

/// Top-level collection caps, if this handle is a UPS collection.
pub fn ups_caps(h: &Handle) -> Option<HIDP_CAPS> {
    unsafe {
        let mut pp = zeroed();
        if !HidD_GetPreparsedData(h.0, &mut pp) {
            return None;
        }
        let mut caps: HIDP_CAPS = zeroed();
        let st = HidP_GetCaps(pp, &mut caps);
        HidD_FreePreparsedData(pp);
        (st == HIDP_STATUS_SUCCESS && caps.UsagePage == 0x84 && caps.Usage == 0x04).then_some(caps)
    }
}

/// (manufacturer, product, serial) strings.
pub fn device_strings(h: &Handle) -> (String, String, String) {
    type Getter = unsafe extern "system" fn(HANDLE, *mut core::ffi::c_void, u32) -> bool;
    let get = |f: Getter| {
        let mut s = [0u16; 128];
        if unsafe { f(h.0, s.as_mut_ptr().cast(), 256) } {
            wide_str(&s)
        } else {
            String::new()
        }
    };
    (
        get(HidD_GetManufacturerString),
        get(HidD_GetProductString),
        get(HidD_GetSerialNumberString),
    )
}

/// All connected HID UPS collections, any vendor.
pub fn ups_interfaces() -> Vec<UpsInterface> {
    let mut out = Vec::new();
    unsafe {
        let mut guid: GUID = zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set as isize == -1 {
            return out;
        }
        let mut idx = 0;
        loop {
            let mut ifd: SP_DEVICE_INTERFACE_DATA = zeroed();
            ifd.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, null(), &guid, idx, &mut ifd) == 0 {
                break;
            }
            idx += 1;

            let mut need = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &ifd, null_mut(), 0, &mut need, null_mut());
            if need < 6 {
                continue;
            }
            // u32 storage keeps the struct aligned.
            let mut buf = vec![0u32; (need as usize).div_ceil(4)];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &ifd, detail, need, null_mut(), null_mut()) == 0 {
                continue;
            }
            let path_ptr = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
            let max = (need as usize - 4) / 2;
            let raw = std::slice::from_raw_parts(path_ptr, max);
            let len = raw.iter().position(|&c| c == 0).unwrap_or(max);
            let mut path = raw[..len].to_vec();
            path.push(0);

            let Some(h) = open_path(&path) else { continue };
            if ups_caps(&h).is_none() {
                continue;
            }
            let mut attr: HIDD_ATTRIBUTES = zeroed();
            attr.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
            HidD_GetAttributes(h.0, &mut attr);
            let (manufacturer, product, serial) = device_strings(&h);
            out.push(UpsInterface {
                path,
                vid: attr.VendorID,
                pid: attr.ProductID,
                manufacturer,
                product,
                serial,
            });
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

pub const APC_VID: u16 = 0x051D;

/// Model-specific reader for APC Back-UPS (fixed report IDs, see decode.rs).
/// Interprets APC's vendor data too (sensitivity, status flag, dates…).
pub struct ApcHid {
    handle: Handle,
    feature_len: u32,
    info: DeviceInfo,
}

impl ApcHid {
    pub fn open_iface(iface: &UpsInterface) -> Result<Self, String> {
        let h = open_path(&iface.path).ok_or("cannot open UPS")?;
        let caps = ups_caps(&h).ok_or("not a UPS collection")?;
        Ok(ApcHid {
            handle: h,
            feature_len: u32::from(caps.FeatureReportByteLength),
            info: iface.info("APC reader"),
        })
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// Raw feature report `[id, data…]`.
    pub fn feature(&mut self, id: u8) -> Result<Report, String> {
        let mut buf = [0u8; 64];
        buf[0] = id;
        let len = self.feature_len.clamp(2, 64);
        let ok = unsafe { HidD_GetFeature(self.handle.0, buf.as_mut_ptr().cast(), len) };
        if !ok {
            return Err(format!("HidD_GetFeature(0x{id:02X}) failed ({})", unsafe {
                GetLastError()
            }));
        }
        let mut r = [0u8; 8];
        let n = (len as usize).min(8);
        r[..n].copy_from_slice(&buf[..n]);
        Ok(r)
    }
}

/// Any HID UPS: the model-specific reader when there is one, else the generic one.
pub enum UpsHid {
    Apc(ApcHid),
    Generic(crate::generic::GenericHid),
}

impl UpsHid {
    /// Choose the reader for a device.
    pub fn open_iface(iface: &UpsInterface) -> Result<Self, String> {
        if iface.vid == APC_VID {
            ApcHid::open_iface(iface).map(UpsHid::Apc)
        } else {
            crate::generic::GenericHid::open_iface(iface).map(UpsHid::Generic)
        }
    }
}

impl Transport for UpsHid {
    fn open(id: &str) -> Result<Self, String> {
        let list = ups_interfaces();
        let iface = if id.is_empty() {
            list.iter().find(|i| i.vid == APC_VID).or(list.first())
        } else {
            list.iter().find(|i| i.id() == id)
        };
        match iface {
            Some(i) => UpsHid::open_iface(i),
            None if id.is_empty() => Err("No UPS found".into()),
            None => Err("UPS not found".into()),
        }
    }

    fn info(&self) -> &DeviceInfo {
        match self {
            UpsHid::Apc(d) => d.info(),
            UpsHid::Generic(d) => d.info(),
        }
    }

    fn read(&mut self, snap: &mut crate::Snapshot, slow: bool, statics: bool) -> Result<(), String> {
        match self {
            UpsHid::Apc(d) => crate::decode::read_apc_reports(snap, slow, statics, |id| d.feature(id)),
            UpsHid::Generic(d) => d.read(snap, slow, statics),
        }
    }
}
