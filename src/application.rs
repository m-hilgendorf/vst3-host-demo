use crate::error::{Error, ToCodeExt as _};
use std::os::raw::c_void;
use vst3::Steinberg::{
    tresult,
    Vst::{IHostApplicationTrait, String128},
    TUID,
};

pub trait HostApplication {
    fn name(&self) -> &str;
}

impl IHostApplicationTrait for &dyn HostApplication {
    unsafe fn createInstance(
        &self,
        _cid: *mut TUID,
        _iid: *mut TUID,
        _obj: *mut *mut c_void,
    ) -> tresult {
        Err(Error::NotImplemented).to_code()
    }

    unsafe fn getName(&self, name: *mut String128) -> tresult {
        let name_ = self.name().encode_utf16().take(128).enumerate();
        unsafe {
            let ptr = (&mut *name).as_mut_ptr();
            for (n, ch) in name_ {
                *ptr.add(n) = ch as i16;
            }
        }
        0
    }
}
