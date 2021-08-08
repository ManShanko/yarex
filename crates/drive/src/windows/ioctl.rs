use std::os::windows::io::AsRawHandle;
use std::fs::File;
use std::ffi::c_void;

use winapi::um::winioctl::{
    FSCTL_GET_RETRIEVAL_POINTERS,
    RETRIEVAL_POINTERS_BUFFER,
    STARTING_VCN_INPUT_BUFFER,
};
use winapi::um::minwinbase::{
    OVERLAPPED,
};
use winapi::um::ioapiset::{
    DeviceIoControl,
};

pub fn starting_vcn(fd: &File) -> Option<u64> {
    let mut ov = OVERLAPPED::default();
    let mut svib = STARTING_VCN_INPUT_BUFFER::default();
    let mut rpb = RETRIEVAL_POINTERS_BUFFER::default();

    let handle = fd.as_raw_handle();
    let mut returned = 0;
    unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_RETRIEVAL_POINTERS,
            &mut svib as *mut _ as *mut c_void,
            std::mem::size_of::<STARTING_VCN_INPUT_BUFFER>() as u32,
            &mut rpb as *mut _ as *mut c_void,
            std::mem::size_of::<RETRIEVAL_POINTERS_BUFFER>() as u32,
            &mut returned,
            &mut ov
        );
    }

    if rpb.ExtentCount == 0 {
        Some(0)
    } else {
        let lcn = unsafe {
            *rpb.Extents[0].Lcn.QuadPart() as u64
        };
        Some(lcn)
    }
}

