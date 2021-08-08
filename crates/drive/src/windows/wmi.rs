use std::os::windows::prelude::*;
use std::ptr;
use std::ffi::{
    OsString,
    c_void,
};

use winapi::um::oaidl::{
    VARIANT,
};
use winapi::um::combaseapi::{
    CoInitializeEx,
    CoInitializeSecurity,
    CoUninitialize,
    CoCreateInstance,
    CoSetProxyBlanket,
};
use winapi::shared::rpcdce::{
    RPC_C_AUTHN_LEVEL_DEFAULT,
    RPC_C_IMP_LEVEL_IMPERSONATE,
    RPC_C_AUTHN_WINNT,
    RPC_C_AUTHZ_NONE,
    RPC_C_AUTHN_LEVEL_CALL,
};
use winapi::um::unknwnbase::{
    IUnknown,
};
use winapi::um::objbase::{
    COINIT_MULTITHREADED,
};
use winapi::um::objidl::{
    EOAC_NONE,
};
use winapi::um::wbemcli::{
    WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY,
    WBEM_INFINITE,
    CLSID_WbemLocator,
    IID_IWbemLocator,
    IWbemLocator,
    IWbemServices,
    IEnumWbemClassObject,
    IWbemClassObject,
};
use winapi::shared::wtypesbase::{
    CLSCTX_INPROC_SERVER,
};

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug)]
pub enum DriveKind {
    SSD,
    HDD,
    Unknown,
}

macro_rules! encode_wide {
    ($s:tt) => {
        OsString::from($s).encode_wide().chain(std::iter::once(0)).collect::<Vec<_>>()
    }
}

#[allow(dead_code)]
fn u16_to_utf8(ptr: *mut u16) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        let mut i = 0;
        let len = loop {
            unsafe {
                if *ptr.add(i) == 0 {
                    break i;
                } else {
                    i += 1;
                }
            }
        };
        let slice = unsafe {
            std::slice::from_raw_parts(ptr, len)
        };
        Some(OsString::from_wide(slice).to_string_lossy().to_string())
    }
}

pub fn get_device_index(letter: &str) -> Result<Option<u32>, &'static str> {
    let mut disk = None;

    let (loc, svc) = co_scan(r"root\cimv2")?;

    let mut storage_enumerator: Option<&mut IEnumWbemClassObject> = None;
    unsafe {
        if svc.ExecQuery(
            encode_wide!("WQL").as_mut_ptr(),
            encode_wide!("SELECT * FROM Win32_LogicalDiskToPartition").as_mut_ptr(),
            (WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY) as i32,
            ptr::null_mut(),
            &mut storage_enumerator as *mut _ as *mut *mut IEnumWbemClassObject
        ) != 0 {
            loc.Release();
            svc.Release();
            return Err("svc.ExecQuery fail");
        }
    }

    if let Some(ref storage_enumerator) = storage_enumerator {
        let search = format!("DeviceID=\"{}\"", letter);
        let mut storage: Option<&IWbemClassObject> = None;
        let mut ret = 0;
        loop {
            let res = unsafe {
                storage_enumerator.Next(
                    WBEM_INFINITE as i32,
                    1,
                    &mut storage as *mut _ as *mut *mut IWbemClassObject,
                    &mut ret
                )
            };
            if ret == 0 || res != 0 {
                break;
            }

            if let Some(storage) = storage {
                let mut ante = VARIANT::default();
                let mut depe = VARIANT::default();

                unsafe {
                    storage.Get(
                        encode_wide!("Dependent").as_mut_ptr(),
                        0,
                        &mut depe,
                        ptr::null_mut(),
                        ptr::null_mut()
                    );

                    if let Some(depe) = u16_to_utf8(*depe.n1.n2().n3.bstrVal()) {
                        if depe.contains(&search) {
                            storage.Get(
                                encode_wide!("Antecedent").as_mut_ptr(),
                                0,
                                &mut ante,
                                ptr::null_mut(),
                                ptr::null_mut()
                            );

                            if let Some(ante) = u16_to_utf8(*ante.n1.n2().n3.bstrVal()) {
                                let search = "DeviceID=\"Disk #";

                                if let Some(i) = ante.find(search) {
                                    let offset = i + search.len();
                                    let mut number = String::new();
                                    for digit in ante.chars().skip(offset) {
                                        if digit.is_ascii_digit() {
                                            number.push(digit);
                                        } else {
                                            break;
                                        }
                                    }

                                    disk = number.parse::<u32>().ok();
                                    if disk.is_some() {
                                        storage.Release();
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    storage.Release();
                }
            }
        }
    }

    unsafe {
        if let Some(storage_enumerator) = storage_enumerator {
            storage_enumerator.Release();
        }
        loc.Release();
        svc.Release();
    }

    Ok(disk)
}

pub fn get_media_type(target_id: u32) -> Result<Option<DriveKind>, &'static str> {
    let mut kind = None;

    let (loc, svc) = co_scan(r"root\microsoft\windows\storage")?;

    let mut storage_enumerator: Option<&mut IEnumWbemClassObject> = None;
    unsafe {
        if svc.ExecQuery(
            encode_wide!("WQL").as_mut_ptr(),
            encode_wide!("SELECT * FROM MSFT_PhysicalDisk").as_mut_ptr(),
            (WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY) as i32,
            ptr::null_mut(),
            &mut storage_enumerator as *mut _ as *mut *mut IEnumWbemClassObject
        ) != 0 {
            loc.Release();
            svc.Release();
            return Err("svc.ExecQuery fail");
        }
    }

    if let Some(ref storage_enumerator) = storage_enumerator {
        let mut storage: Option<&IWbemClassObject> = None;
        let mut ret = 0;
        loop {
            let res = unsafe {
                storage_enumerator.Next(
                    WBEM_INFINITE as i32,
                    1,
                    &mut storage as *mut _ as *mut *mut IWbemClassObject,
                    &mut ret
                )
            };
            if ret == 0 || res != 0 {
                break;
            }

            if let Some(storage) = storage {
                let mut device_id = VARIANT::default();
                let mut media_type = VARIANT::default();

                unsafe {
                    storage.Get(
                        encode_wide!("DeviceId").as_mut_ptr(),
                        0,
                        &mut device_id,
                        ptr::null_mut(),
                        ptr::null_mut()
                    );
                    storage.Get(
                        encode_wide!("MediaType").as_mut_ptr(),
                        0,
                        &mut media_type,
                        ptr::null_mut(),
                        ptr::null_mut()
                    );

                    let device_id = u16_to_utf8(*device_id.n1.n2().n3.bstrVal());
                    let media_type = *media_type.n1.n2().n3.uintVal();
                    storage.Release();

                    let device_id = match device_id {
                        Some(device_id) => device_id.parse::<u32>()
                            .unwrap_or_else(|e| panic!("{} invalid device_id {}", e, device_id)),
                        None => continue,
                    };

                    if target_id == device_id {
                        if kind.is_some() {
                            loc.Release();
                            svc.Release();
                            return Err("target_id had multiple matches");
                        }

                        kind = Some(match media_type {
                            4 => DriveKind::SSD,
                            3 => DriveKind::HDD,
                            _ => DriveKind::Unknown,
                        });
                    }
                }
            } else {
                break;
            }
        }
    }

    unsafe {
        if let Some(storage_enumerator) = storage_enumerator {
            storage_enumerator.Release();
        }
        loc.Release();
        svc.Release();
    }

    Ok(kind)
}

fn co_scan(server: &str) -> Result<(&'static mut IWbemLocator, &'static mut IWbemServices), &'static str> {
    let mut loc: Option<&'static mut IWbemLocator> = None;
    let mut svc: Option<&'static mut IWbemServices> = None;
    unsafe {
        if CoCreateInstance(
            &CLSID_WbemLocator,
            ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_IWbemLocator,
            &mut loc as *mut _ as *mut *mut c_void
        ) != 0 {
            return Err("CoCreateInstance fail");
        }

        let loc = loc.unwrap();

        if loc.ConnectServer(
            encode_wide!(server).as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut svc as *mut _ as *mut *mut IWbemServices
        ) != 0 {
            loc.Release();
            return Err("loc.ConnectServer fail");
        }

        let svc = svc.unwrap();

        if CoSetProxyBlanket(
            &mut *svc as *mut _ as *mut IUnknown,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            ptr::null_mut(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            ptr::null_mut(),
            EOAC_NONE
        ) != 0 {
            loc.Release();
            svc.Release();
            return Err("CoSetProxyBlanket fail");
        }

        Ok((loc, svc))
    }
}

pub fn co_exit() {
    unsafe {
        CoUninitialize();
    }
}

pub fn co_init() -> Result<(), &'static str> {
    unsafe {
        if CoInitializeEx(ptr::null_mut(), COINIT_MULTITHREADED) != 0 {
            return Err("CoInitializeEx fail");
        }

        if CoInitializeSecurity(
            ptr::null_mut(),
            -1,
            ptr::null_mut(),
            ptr::null_mut(),
            RPC_C_AUTHN_LEVEL_DEFAULT,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            ptr::null_mut(),
            EOAC_NONE,
            ptr::null_mut()
        ) != 0 {
            CoUninitialize();
            return Err("CoInitializeSecurity fail");
        }

    }

    Ok(())
}
