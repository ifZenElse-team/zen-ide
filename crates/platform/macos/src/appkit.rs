#![allow(unsafe_code)]

use std::ffi::CStr;

use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};

use crate::{QuitAnswer, quit_gate};

const SHOULD_TERMINATE_TYPES: &CStr = c"Q@:@";

type ShouldTerminate = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> NSApplicationTerminateReply;

extern "C-unwind" fn should_terminate(
    _delegate: &AnyObject,
    _selector: Sel,
    _sender: *mut AnyObject,
) -> NSApplicationTerminateReply {
    match quit_gate().system_asks() {
        QuitAnswer::Terminate => NSApplicationTerminateReply::TerminateNow,
        QuitAnswer::Cancel => NSApplicationTerminateReply::TerminateCancel,
    }
}

pub(crate) fn install() -> Option<bool> {
    let main_thread = MainThreadMarker::new()?;
    let delegate = NSApplication::sharedApplication(main_thread).delegate()?;
    let object: &AnyObject = delegate.as_ref();
    let class: &'static AnyClass = object.class();
    let selector = sel!(applicationShouldTerminate:);
    if class.responds_to(selector) {
        return Some(false);
    }
    let hook: ShouldTerminate = should_terminate;
    // SAFETY: an `Imp` is an untyped function pointer that the runtime calls with the method's real
    let imp = unsafe { std::mem::transmute::<ShouldTerminate, Imp>(hook) };
    // SAFETY: `class` is a registered class (an object's class), passed as the runtime's mutable
    let added = unsafe {
        objc2::ffi::class_addMethod(
            (class as *const AnyClass).cast_mut(),
            selector,
            imp,
            SHOULD_TERMINATE_TYPES.as_ptr(),
        )
    };
    Some(added.as_bool())
}

#[cfg(test)]
mod tests {
    use objc2::Encode;
    use objc2::runtime::Sel;

    use super::*;

    #[test]
    fn the_type_encoding_matches_the_hooks_signature() {
        let expected = format!(
            "{}{}{}{}",
            NSApplicationTerminateReply::ENCODING,
            <&AnyObject>::ENCODING,
            Sel::ENCODING,
            <*mut AnyObject>::ENCODING
        );
        assert_eq!(SHOULD_TERMINATE_TYPES.to_str().unwrap(), expected);
    }

    #[test]
    fn off_the_main_thread_nothing_is_installed() {
        assert_eq!(install(), None);
    }
}
