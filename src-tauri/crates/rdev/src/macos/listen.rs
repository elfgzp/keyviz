#![allow(improper_ctypes_definitions)]
use crate::macos::common::*;
use crate::rdev::{Event, ListenError};
use cocoa::base::nil;
use cocoa::foundation::NSAutoreleasePool;
use core_graphics::event::{CGEventTapLocation, CGEventType};
use std::os::raw::c_void;

static mut GLOBAL_CALLBACK: Option<Box<dyn FnMut(Event)>> = None;
static mut EVENT_TAP: CFMachPortRef = std::ptr::null();

unsafe extern "C" fn raw_callback(
    _proxy: CGEventTapProxy,
    _type: CGEventType,
    cg_event: CGEventRef,
    _user_info: *mut c_void,
) -> CGEventRef {
    // Re-enable event tap if macOS disabled it (e.g. due to timeout)
    if !EVENT_TAP.is_null() && !CGEventTapIsEnabled(EVENT_TAP) {
        CGEventTapEnable(EVENT_TAP, true);
    }

    if let Ok(mut state) = KEYBOARD_STATE.lock() {
        if let Some(keyboard) = state.as_mut() {
            if let Some(event) = convert(_type, &cg_event, keyboard) {
                if let Some(callback) = &mut GLOBAL_CALLBACK {
                    callback(event);
                }
            }
        }
    }
    cg_event
}

pub fn listen<T>(callback: T) -> Result<(), ListenError>
where
    T: FnMut(Event) + 'static,
{
    let mut types = kCGEventMaskForAllEvents;
    if crate::keyboard_only() {
        types = (1 << CGEventType::KeyDown as u64)
            + (1 << CGEventType::KeyUp as u64)
            + (1 << CGEventType::FlagsChanged as u64);
    }
    unsafe {
        GLOBAL_CALLBACK = Some(Box::new(callback));
        let _pool = NSAutoreleasePool::new(nil);
        let tap = CGEventTapCreate(
            CGEventTapLocation::HID,
            kCGHeadInsertEventTap,
            CGEventTapOption::ListenOnly,
            types,
            raw_callback,
            nil,
        );
        if tap.is_null() {
            return Err(ListenError::EventTapError);
        }
        EVENT_TAP = tap;
        let _loop = CFMachPortCreateRunLoopSource(nil, tap, 0);
        if _loop.is_null() {
            return Err(ListenError::LoopSourceError);
        }

        let current_loop = CFRunLoopGetCurrent();
        CFRunLoopAddSource(current_loop, _loop, kCFRunLoopCommonModes);

        CGEventTapEnable(tap, true);

        // Periodically check and re-enable event tap to handle macOS background restrictions
        loop {
            let result = CFRunLoopRunInMode(kCFRunLoopDefaultMode, 5.0, false);
            if result == 2 || result == 3 {
                // kCFRunLoopRunStopped (2) or kCFRunLoopRunFinished (3)
                break;
            }
            if !EVENT_TAP.is_null() && !CGEventTapIsEnabled(EVENT_TAP) {
                CGEventTapEnable(EVENT_TAP, true);
            }
        }
    }
    Ok(())
}
