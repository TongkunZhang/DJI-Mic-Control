//! Minimal macOS C ABI bindings. All borrowed CF/HID references remain on the
//! owning run-loop thread; owned Create/Copy objects are released with RAII.

use std::ffi::{c_char, c_double, c_int, c_long, c_void};
use std::ptr;

use crate::shortcut::{Modifiers, Shortcut, COMMAND, CONTROL, OPTION, SHIFT};

pub type Ref = *const c_void;
pub type HidCallback = unsafe extern "C" fn(*mut c_void, i32, *mut c_void, Ref);

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(value: Ref);
    fn CFStringCreateWithCString(allocator: Ref, value: *const c_char, encoding: u32) -> Ref;
    fn CFNumberCreate(allocator: Ref, kind: c_long, value: Ref) -> Ref;
    fn CFDictionaryCreate(
        allocator: Ref,
        keys: *const Ref,
        values: *const Ref,
        count: c_long,
        key_callbacks: Ref,
        value_callbacks: Ref,
    ) -> Ref;
    fn CFArrayCreate(allocator: Ref, values: *const Ref, count: c_long, callbacks: Ref) -> Ref;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(number: Ref, kind: c_long, value: *mut c_void) -> bool;
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopRunInMode(mode: Ref, seconds: c_double, return_after_source: bool) -> c_int;
    static kCFRunLoopDefaultMode: Ref;
    static kCFBooleanTrue: Ref;
    // CFDictionaryKeyCallBacks / CFDictionaryValueCallBacks: a version word plus five callbacks.
    static kCFTypeDictionaryKeyCallBacks: [usize; 6];
    static kCFTypeDictionaryValueCallBacks: [usize; 6];
    // CFArrayCallBacks: a version word plus four callbacks.
    static kCFTypeArrayCallBacks: [usize; 5];
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDManagerCreate(allocator: Ref, options: u32) -> Ref;
    fn IOHIDManagerSetDeviceMatchingMultiple(manager: Ref, multiple: Ref);
    fn IOHIDDeviceGetProperty(device: Ref, key: Ref) -> Ref;
    fn IOHIDManagerRegisterInputValueCallback(
        manager: Ref,
        callback: HidCallback,
        context: *mut c_void,
    );
    fn IOHIDManagerRegisterDeviceMatchingCallback(
        manager: Ref,
        callback: HidCallback,
        context: *mut c_void,
    );
    fn IOHIDManagerRegisterDeviceRemovalCallback(
        manager: Ref,
        callback: HidCallback,
        context: *mut c_void,
    );
    fn IOHIDManagerScheduleWithRunLoop(manager: Ref, run_loop: Ref, mode: Ref);
    fn IOHIDManagerUnscheduleFromRunLoop(manager: Ref, run_loop: Ref, mode: Ref);
    fn IOHIDManagerOpen(manager: Ref, options: u32) -> i32;
    fn IOHIDManagerClose(manager: Ref, options: u32) -> i32;
    fn IOHIDValueGetElement(value: Ref) -> Ref;
    fn IOHIDValueGetIntegerValue(value: Ref) -> c_long;
    fn IOHIDElementGetUsagePage(element: Ref) -> u32;
    fn IOHIDElementGetUsage(element: Ref) -> u32;
    fn IOHIDElementGetDevice(element: Ref) -> Ref;
    fn IOHIDCheckAccess(request: u32) -> u32;
    fn IOHIDRequestAccess(request: u32) -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: Ref) -> bool;
    fn CGEventCreateKeyboardEvent(source: Ref, key: u16, down: bool) -> Ref;
    fn CGEventSetFlags(event: Ref, flags: u64);
    fn CGEventPost(location: u32, event: Ref);
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    fn CGEventSourceFlagsState(state: i32) -> u64;
}

struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this wrapper only holds a Create/Copy CF object.
            unsafe { CFRelease(self.0) };
        }
    }
}

fn string(value: &std::ffi::CStr) -> Owned {
    // SAFETY: CStr is terminated; UTF-8 is the specified encoding.
    Owned(unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), 0x0800_0100) })
}

/// Builds a CF dictionary that compares keys with CFEqual and retains entries.
/// Null callbacks would compare keys by pointer, so framework lookups by
/// their own constant keys silently miss.
///
/// # Safety
/// `keys` and `values` are equal-length arrays of valid CF objects.
unsafe fn dictionary(keys: &[Ref], values: &[Ref]) -> Owned {
    Owned(CFDictionaryCreate(
        ptr::null(),
        keys.as_ptr(),
        values.as_ptr(),
        keys.len() as c_long,
        ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
        ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
    ))
}

/// `(input_monitoring, accessibility)` grants for this process.
pub fn permissions() -> (bool, bool) {
    // SAFETY: permission queries have no preconditions; IOHIDAccessTypeGranted = 0.
    unsafe { (IOHIDCheckAccess(1) == 0, AXIsProcessTrusted()) }
}

/// Prompt for Input Monitoring and Accessibility (macOS shows its own dialogs
/// and adds the app to the Privacy lists).
pub fn request_permissions() {
    let key = string(c"AXTrustedCheckOptionPrompt");
    // SAFETY: the dictionary retains its entries for the synchronous AX call.
    // Input Monitoring is requested first so an AX failure cannot skip it.
    unsafe {
        IOHIDRequestAccess(1);
        let options = dictionary(&[key.0], &[kCFBooleanTrue]);
        AXIsProcessTrustedWithOptions(options.0);
    }
}

/// kCFNumberSInt32Type
const SINT32: c_long = 3;

fn number(value: i32) -> Owned {
    // SAFETY: `value` outlives the call, which copies it.
    Owned(unsafe { CFNumberCreate(ptr::null(), SINT32, (&value as *const i32).cast()) })
}

/// A seized IOHIDManager matching specific receivers' Consumer Control
/// interfaces.
pub struct Manager {
    value: Owned,
    run_loop: Ref,
    _matching: Owned,
}

impl Manager {
    /// Open and seize the `vendor:product` Consumer Control interface of each
    /// receiver at `locations` (IOKit location IDs), delivering callbacks on
    /// the current thread's run loop. Receivers elsewhere are left alone.
    ///
    /// # Safety
    /// The caller keeps `context` alive until this manager is dropped, and
    /// pumps/drops the manager on the same thread.
    pub unsafe fn open(
        vendor: u16,
        product: u16,
        locations: &[u32],
        context: *mut c_void,
        input: HidCallback,
        attached: HidCallback,
        removed: HidCallback,
    ) -> Result<Self, String> {
        let keys = [
            string(c"VendorID"),
            string(c"ProductID"),
            string(c"PrimaryUsagePage"),
            string(c"PrimaryUsage"),
            string(c"LocationID"),
        ];
        let key_refs: Vec<Ref> = keys.iter().map(|k| k.0).collect();
        // One matching dictionary per receiver. Dictionaries retain their
        // entries and the array retains the dictionaries.
        let dictionaries: Vec<Owned> = locations
            .iter()
            .map(|&location| {
                let values = [
                    number(vendor as i32),
                    number(product as i32),
                    number(0x0c),
                    number(1),
                    number(location as i32),
                ];
                let value_refs: Vec<Ref> = values.iter().map(|v| v.0).collect();
                dictionary(&key_refs, &value_refs)
            })
            .collect();
        let dictionary_refs: Vec<Ref> = dictionaries.iter().map(|d| d.0).collect();
        let matching = Owned(CFArrayCreate(
            ptr::null(),
            dictionary_refs.as_ptr(),
            dictionary_refs.len() as c_long,
            ptr::addr_of!(kCFTypeArrayCallBacks).cast(),
        ));
        let manager = Owned(IOHIDManagerCreate(ptr::null(), 0));
        if manager.0.is_null() || matching.0.is_null() {
            return Err("IOHIDManagerCreate failed".into());
        }
        IOHIDManagerSetDeviceMatchingMultiple(manager.0, matching.0);
        IOHIDManagerRegisterInputValueCallback(manager.0, input, context);
        IOHIDManagerRegisterDeviceMatchingCallback(manager.0, attached, context);
        IOHIDManagerRegisterDeviceRemovalCallback(manager.0, removed, context);
        let run_loop = CFRunLoopGetCurrent();
        IOHIDManagerScheduleWithRunLoop(manager.0, run_loop, kCFRunLoopDefaultMode);
        let result = Self {
            value: manager,
            run_loop,
            _matching: matching,
        };
        let code = IOHIDManagerOpen(result.value.0, 1); // kIOHIDOptionsTypeSeizeDevice
        if code != 0 {
            return Err(format!(
                "cannot exclusively open the receiver's button interface (IOReturn {code:#x}); \
                 is another app such as dji-mic using it?"
            ));
        }
        Ok(result)
    }
}

/// The IOKit location ID of an IOHIDDevice — the same value as its USB
/// device's location, which is how a HID callback is tied back to a receiver.
///
/// # Safety
/// `device` is a valid IOHIDDeviceRef (e.g. one passed to a HID callback).
pub unsafe fn device_location(device: Ref) -> Option<u32> {
    let key = string(c"LocationID");
    let value = IOHIDDeviceGetProperty(device, key.0); // Get rule: not owned
    if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
        return None;
    }
    let mut out: i32 = 0;
    CFNumberGetValue(value, SINT32, (&mut out as *mut i32).cast()).then_some(out as u32)
}

impl Drop for Manager {
    fn drop(&mut self) {
        // SAFETY: called on the same thread; close/unschedule before releasing the manager.
        unsafe {
            IOHIDManagerClose(self.value.0, 0);
            IOHIDManagerUnscheduleFromRunLoop(self.value.0, self.run_loop, kCFRunLoopDefaultMode);
        }
    }
}

/// Pump the current thread's run loop briefly so HID callbacks fire.
pub fn pump() {
    // SAFETY: pumps the current thread's run loop with a bounded timeout.
    unsafe {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, false);
    }
    // With no sources scheduled the run loop returns immediately; don't spin.
    std::thread::sleep(std::time::Duration::from_millis(25));
}

/// The `(device, pressed)` state if `value` is the side button (Consumer
/// Control Volume Up, `0x0C/0xE9`).
///
/// # Safety
/// Only call within an IOHID input-value callback.
pub unsafe fn button_value(value: Ref) -> Option<(usize, bool)> {
    let element = IOHIDValueGetElement(value);
    if IOHIDElementGetUsagePage(element) != 0x0c || IOHIDElementGetUsage(element) != 0xe9 {
        return None;
    }
    Some((
        IOHIDElementGetDevice(element) as usize,
        IOHIDValueGetIntegerValue(value) != 0,
    ))
}

/// Right-hand keycodes paired with the left-hand ones in `shortcut.rs`.
const RIGHT_CONTROL: u16 = 62;
const RIGHT_OPTION: u16 = 61;
const RIGHT_SHIFT: u16 = 60;
const RIGHT_COMMAND: u16 = 54;

/// Post `shortcut` as synthetic keyboard events.
pub fn send_shortcut(shortcut: &Shortcut) -> Result<(), String> {
    if !permissions().1 {
        return Err("Accessibility permission is not granted".into());
    }
    // kCGEventSourceStateHIDSystemState = 1: physical HID state, which
    // excludes our own synthetic events.
    let down = |left: u16, right: u16| {
        // SAFETY: plain state queries with valid virtual keycodes.
        unsafe { CGEventSourceKeyState(1, left) || CGEventSourceKeyState(1, right) }
    };
    let held = Modifiers {
        ctrl: down(CONTROL.1, RIGHT_CONTROL),
        option: down(OPTION.1, RIGHT_OPTION),
        shift: down(SHIFT.1, RIGHT_SHIFT),
        command: down(COMMAND.1, RIGHT_COMMAND),
    };
    // SAFETY: as above.
    let flags = unsafe { CGEventSourceFlagsState(1) };

    let sequence = shortcut.sequence(held, flags);
    if sequence.is_empty() {
        return Err(format!("unsupported key: {:?}", shortcut.key));
    }
    let mut events = Vec::with_capacity(sequence.len());
    for (key, key_down, flags) in sequence {
        // SAFETY: valid virtual keycode and a default (null) event source.
        let event = Owned(unsafe { CGEventCreateKeyboardEvent(ptr::null(), key, key_down) });
        if event.0.is_null() {
            return Err("CGEvent allocation failed (no events sent)".into());
        }
        // SAFETY: event is a valid, owned CGEvent.
        unsafe { CGEventSetFlags(event.0, flags) };
        events.push(event);
    }
    // Allocate the entire sequence before posting so an allocation failure
    // cannot leave a key held.
    for event in &events {
        // SAFETY: event is valid and owned until after posting.
        unsafe { CGEventPost(0, event.0) };
    }
    Ok(())
}
