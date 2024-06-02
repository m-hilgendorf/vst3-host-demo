use crate::error::{Error, ToCodeExt as _};
use std::os::raw::c_void;
use vst3::{
    Class,
    Steinberg::{
        tresult,
        Vst::{IHostApplication, IHostApplicationTrait, String128},
        TUID,
    },
};

pub trait HostApplication {
    fn name(&self) -> &str;
}

pub(crate) struct HostApplicationWrapper {
    pub host: Box<dyn HostApplication>,
}

impl IHostApplicationTrait for HostApplicationWrapper {
    unsafe fn createInstance(
        &self,
        _cid: *mut TUID,
        _iid: *mut TUID,
        _obj: *mut *mut c_void,
    ) -> tresult {
        Err(Error::NoInterface).to_code()
    }

    unsafe fn getName(&self, name: *mut String128) -> tresult {
        let name_ = self.host.name().encode_utf16().take(128).enumerate();
        unsafe {
            let ptr = (&mut *name).as_mut_ptr();
            for (n, ch) in name_ {
                *ptr.add(n) = ch as i16;
            }
        }
        0
    }
}

impl Class for HostApplicationWrapper {
    type Interfaces = (IHostApplication,);
}
