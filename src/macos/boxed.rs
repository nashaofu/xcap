use core_foundation::{
    array::CFArrayRef,
    base::{CFRelease, ToVoid},
};
use std::ops::Deref;

#[derive(Debug)]
pub(super) struct BoxCFArrayRef {
    cf_array_ref: CFArrayRef,
}

impl Deref for BoxCFArrayRef {
    type Target = CFArrayRef;
    fn deref(&self) -> &Self::Target {
        &self.cf_array_ref
    }
}

impl Drop for BoxCFArrayRef {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.cf_array_ref.to_void());
        }
    }
}

impl BoxCFArrayRef {
    pub fn new(cf_array_ref: CFArrayRef) -> Self {
        BoxCFArrayRef { cf_array_ref }
    }
}
