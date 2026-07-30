//! The COM objects a VST3 host must supply to a plugin.
//!
//! All of them live on the main thread and are handed to the plugin as
//! reference-counted interfaces; `ComWrapper` does the counting. Interface
//! methods take `&self`, so state is behind `RefCell`.

// Parameter names mirror the C++ headers, which makes the impls checkable
// against the spec line by line.
#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_void, CStr, CString};

use vst3::Steinberg::IBStream_::IStreamSeekMode_ as SeekMode;
use vst3::Steinberg::Vst::IAttributeList_::AttrID;
use vst3::Steinberg::Vst::{
    IAttributeList, IAttributeListTrait, IComponentHandler, IComponentHandlerTrait,
    IHostApplication, IHostApplicationTrait, IMessage, IMessageTrait, ParamID, ParamValue,
    String128, TChar,
};
use vst3::Steinberg::{
    int32, int64, kInvalidArgument, kNotImplemented, kResultFalse, kResultOk, tresult, uint32,
    FIDString, IBStream, IBStreamTrait, ISizeableStream, ISizeableStreamTrait, TUID,
};
use vst3::{Class, ComPtr, ComWrapper, Interface};

/// An in-memory `IBStream`, used for plugin state in both directions.
pub struct MemoryStream {
    bytes: RefCell<Vec<u8>>,
    pos: Cell<usize>,
}

impl MemoryStream {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: RefCell::new(bytes),
            pos: Cell::new(0),
        }
    }

    pub fn take(&self) -> Vec<u8> {
        self.bytes.borrow().clone()
    }
}

impl Class for MemoryStream {
    type Interfaces = (IBStream, ISizeableStream);
}

impl IBStreamTrait for MemoryStream {
    unsafe fn read(&self, buffer: *mut c_void, numBytes: int32, numBytesRead: *mut int32) -> tresult {
        if buffer.is_null() || numBytes < 0 {
            return kInvalidArgument;
        }
        let bytes = self.bytes.borrow();
        let start = self.pos.get().min(bytes.len());
        let n = (numBytes as usize).min(bytes.len() - start);
        std::ptr::copy_nonoverlapping(bytes[start..].as_ptr(), buffer.cast::<u8>(), n);
        self.pos.set(start + n);
        if !numBytesRead.is_null() {
            *numBytesRead = n as int32;
        }
        kResultOk
    }

    unsafe fn write(
        &self,
        buffer: *mut c_void,
        numBytes: int32,
        numBytesWritten: *mut int32,
    ) -> tresult {
        if buffer.is_null() || numBytes < 0 {
            return kInvalidArgument;
        }
        let n = numBytes as usize;
        let mut bytes = self.bytes.borrow_mut();
        let start = self.pos.get();
        // A seek past the end then a write leaves a hole, which the spec says
        // reads back as zeroes.
        if bytes.len() < start + n {
            bytes.resize(start + n, 0);
        }
        std::ptr::copy_nonoverlapping(buffer.cast::<u8>(), bytes[start..].as_mut_ptr(), n);
        self.pos.set(start + n);
        if !numBytesWritten.is_null() {
            *numBytesWritten = n as int32;
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let len = self.bytes.borrow().len() as i64;
        // The generated constants are `c_int`, whose signedness differs per
        // target, so they are compared as values rather than matched as patterns.
        let base = if mode as i64 == SeekMode::kIBSeekSet as i64 {
            0
        } else if mode as i64 == SeekMode::kIBSeekCur as i64 {
            self.pos.get() as i64
        } else if mode as i64 == SeekMode::kIBSeekEnd as i64 {
            len
        } else {
            return kInvalidArgument;
        };
        let target = base.saturating_add(pos);
        if target < 0 {
            return kInvalidArgument;
        }
        self.pos.set(target as usize);
        if !result.is_null() {
            *result = target;
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if pos.is_null() {
            return kInvalidArgument;
        }
        *pos = self.pos.get() as int64;
        kResultOk
    }
}

impl ISizeableStreamTrait for MemoryStream {
    unsafe fn getStreamSize(&self, size: *mut int64) -> tresult {
        if size.is_null() {
            return kInvalidArgument;
        }
        *size = self.bytes.borrow().len() as int64;
        kResultOk
    }

    unsafe fn setStreamSize(&self, size: int64) -> tresult {
        if size < 0 {
            return kInvalidArgument;
        }
        self.bytes.borrow_mut().resize(size as usize, 0);
        kResultOk
    }
}

enum Attribute {
    Int(i64),
    Float(f64),
    String(Vec<u16>),
    Binary(Vec<u8>),
}

/// Attributes attached to a message passed between a plugin's component and
/// controller. Keys are the plugin's own C strings.
#[derive(Default)]
pub struct AttributeList {
    entries: RefCell<HashMap<CString, Attribute>>,
}

impl Class for AttributeList {
    type Interfaces = (IAttributeList,);
}

unsafe fn key(id: AttrID) -> Option<CString> {
    if id.is_null() {
        return None;
    }
    Some(CStr::from_ptr(id).to_owned())
}

impl IAttributeListTrait for AttributeList {
    unsafe fn setInt(&self, id: AttrID, value: int64) -> tresult {
        let Some(id) = key(id) else {
            return kInvalidArgument;
        };
        self.entries.borrow_mut().insert(id, Attribute::Int(value));
        kResultOk
    }

    unsafe fn getInt(&self, id: AttrID, value: *mut int64) -> tresult {
        let (Some(id), false) = (key(id), value.is_null()) else {
            return kInvalidArgument;
        };
        match self.entries.borrow().get(&id) {
            Some(Attribute::Int(v)) => {
                *value = *v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setFloat(&self, id: AttrID, value: f64) -> tresult {
        let Some(id) = key(id) else {
            return kInvalidArgument;
        };
        self.entries.borrow_mut().insert(id, Attribute::Float(value));
        kResultOk
    }

    unsafe fn getFloat(&self, id: AttrID, value: *mut f64) -> tresult {
        let (Some(id), false) = (key(id), value.is_null()) else {
            return kInvalidArgument;
        };
        match self.entries.borrow().get(&id) {
            Some(Attribute::Float(v)) => {
                *value = *v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setString(&self, id: AttrID, string: *const TChar) -> tresult {
        let (Some(id), false) = (key(id), string.is_null()) else {
            return kInvalidArgument;
        };
        let mut text = Vec::new();
        let mut p = string;
        while *p != 0 {
            text.push(*p as u16);
            p = p.add(1);
        }
        self.entries.borrow_mut().insert(id, Attribute::String(text));
        kResultOk
    }

    unsafe fn getString(&self, id: AttrID, string: *mut TChar, sizeInBytes: uint32) -> tresult {
        let (Some(id), false) = (key(id), string.is_null()) else {
            return kInvalidArgument;
        };
        let entries = self.entries.borrow();
        let Some(Attribute::String(text)) = entries.get(&id) else {
            return kResultFalse;
        };
        // Caller's size is in bytes, and one TChar is two of them; the last slot
        // is reserved for the terminator.
        let capacity = (sizeInBytes as usize / std::mem::size_of::<TChar>()).saturating_sub(1);
        let n = text.len().min(capacity);
        for (i, unit) in text[..n].iter().enumerate() {
            *string.add(i) = *unit as TChar;
        }
        *string.add(n) = 0;
        kResultOk
    }

    unsafe fn setBinary(&self, id: AttrID, data: *const c_void, sizeInBytes: uint32) -> tresult {
        let (Some(id), false) = (key(id), data.is_null()) else {
            return kInvalidArgument;
        };
        let bytes = std::slice::from_raw_parts(data.cast::<u8>(), sizeInBytes as usize).to_vec();
        self.entries.borrow_mut().insert(id, Attribute::Binary(bytes));
        kResultOk
    }

    unsafe fn getBinary(
        &self,
        id: AttrID,
        data: *mut *const c_void,
        sizeInBytes: *mut uint32,
    ) -> tresult {
        let (Some(id), false, false) = (key(id), data.is_null(), sizeInBytes.is_null()) else {
            return kInvalidArgument;
        };
        let entries = self.entries.borrow();
        let Some(Attribute::Binary(bytes)) = entries.get(&id) else {
            return kResultFalse;
        };
        // The pointer stays valid while the attribute lives, which is what the
        // spec promises the caller: read it before setting this key again.
        *data = bytes.as_ptr().cast();
        *sizeInBytes = bytes.len() as uint32;
        kResultOk
    }
}

/// One message in flight between a plugin's component and controller. The host
/// only carries it; the payload is the plugin's own business.
pub struct Message {
    id: RefCell<CString>,
    attributes: ComWrapper<AttributeList>,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            id: RefCell::new(CString::default()),
            attributes: ComWrapper::new(AttributeList::default()),
        }
    }
}

impl Class for Message {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.borrow().as_ptr()
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        let id = if id.is_null() {
            CString::default()
        } else {
            CStr::from_ptr(id).to_owned()
        };
        *self.id.borrow_mut() = id;
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        // Borrowed, not owned: the caller uses it for the life of the message.
        self.attributes
            .as_com_ref::<IAttributeList>()
            .map(|r| r.as_ptr())
            .unwrap_or(std::ptr::null_mut())
    }
}

/// A class id names an interface. The two spellings differ only in signedness:
/// `TUID` is the C API's `char[16]`, `Interface::IID` the bindings' `[u8; 16]`.
fn is_class<I: Interface>(cid: &TUID) -> bool {
    cid.iter().zip(I::IID).all(|(a, b)| *a as u8 == b)
}

/// The context object passed to `initialize`. Plugins ask it for the host's
/// name and for the message objects they exchange between their two halves.
pub struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        let out = &mut *name;
        out.fill(0);
        for (slot, unit) in out.iter_mut().zip("Splitwave".encode_utf16()).take(127) {
            *slot = unit as TChar;
        }
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        _iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        // The two classes a host is required to provide. Anything else is the
        // plugin asking for something we do not implement, and saying so is
        // better than handing back a wrong object.
        let created = if is_class::<IMessage>(&*cid) {
            ComWrapper::new(Message::default())
                .to_com_ptr::<IMessage>()
                .map(|p| p.into_raw().cast::<c_void>())
        } else if is_class::<IAttributeList>(&*cid) {
            ComWrapper::new(AttributeList::default())
                .to_com_ptr::<IAttributeList>()
                .map(|p| p.into_raw().cast::<c_void>())
        } else {
            None
        };

        match created {
            Some(ptr) => {
                *obj = ptr;
                kResultOk
            }
            None => {
                *obj = std::ptr::null_mut();
                kNotImplemented
            }
        }
    }
}

/// What the plugin's own editor reports back to the host. Every callback
/// arrives on the main thread.
pub trait EditListener {
    /// The user moved a control inside the plugin's window. Without acting on
    /// this the edit never reaches the audio processor.
    fn param_edited(&self, id: ParamID, value: ParamValue);
    /// The plugin's structure changed; `flags` is a `RestartFlags` mask.
    fn restart(&self, flags: i32);
}

impl EditListener for Box<dyn EditListener> {
    fn param_edited(&self, id: ParamID, value: ParamValue) {
        (**self).param_edited(id, value);
    }

    fn restart(&self, flags: i32) {
        (**self).restart(flags);
    }
}

pub struct ComponentHandler<L: EditListener> {
    listener: L,
}

impl<L: EditListener> ComponentHandler<L> {
    pub fn new(listener: L) -> Self {
        Self { listener }
    }
}

impl<L: EditListener + 'static> Class for ComponentHandler<L> {
    type Interfaces = (IComponentHandler,);
}

impl<L: EditListener> IComponentHandlerTrait for ComponentHandler<L> {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, valueNormalized: ParamValue) -> tresult {
        self.listener.param_edited(id, valueNormalized);
        kResultOk
    }

    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        self.listener.restart(flags);
        kResultOk
    }
}

/// Creates the host context every plugin `initialize` call needs.
pub fn host_context() -> ComPtr<IHostApplication> {
    ComWrapper::new(HostApplication)
        .to_com_ptr::<IHostApplication>()
        .expect("HostApplication implements IHostApplication")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vst3::ComRef;

    fn as_tuid(iid: [u8; 16]) -> TUID {
        iid.map(|b| b as _)
    }

    fn stream(bytes: Vec<u8>) -> (ComWrapper<MemoryStream>, ComPtr<IBStream>) {
        let wrapper = ComWrapper::new(MemoryStream::new(bytes));
        let ptr = wrapper.to_com_ptr::<IBStream>().unwrap();
        (wrapper, ptr)
    }

    #[test]
    fn memory_stream_round_trips_through_the_com_interface() {
        let (wrapper, s) = stream(Vec::new());
        unsafe {
            let payload = *b"state";
            let mut written = 0;
            assert_eq!(
                s.write(payload.as_ptr() as *mut c_void, 5, &mut written),
                kResultOk
            );
            assert_eq!(written, 5);

            let mut pos = 0;
            s.tell(&mut pos);
            assert_eq!(pos, 5);

            let mut landed = 0;
            assert_eq!(s.seek(0, SeekMode::kIBSeekSet as i32, &mut landed), kResultOk);
            assert_eq!(landed, 0);

            let mut back = [0u8; 8];
            let mut read = 0;
            assert_eq!(
                s.read(back.as_mut_ptr().cast(), 8, &mut read),
                kResultOk,
                "a short read is success, not failure"
            );
            assert_eq!(read, 5);
            assert_eq!(&back[..5], b"state");
        }
        assert_eq!(wrapper.take(), b"state");
    }

    #[test]
    fn memory_stream_reports_and_resizes() {
        let (wrapper, s) = stream(vec![1, 2, 3, 4]);
        unsafe {
            let sizeable = s.cast::<ISizeableStream>().unwrap();
            let mut size = 0;
            sizeable.getStreamSize(&mut size);
            assert_eq!(size, 4);

            assert_eq!(sizeable.setStreamSize(2), kResultOk);
            sizeable.getStreamSize(&mut size);
            assert_eq!(size, 2);

            // Seeking past the end and writing leaves a zero hole.
            let mut landed = 0;
            s.seek(4, SeekMode::kIBSeekSet as i32, &mut landed);
            let tail = [9u8];
            let mut written = 0;
            s.write(tail.as_ptr() as *mut c_void, 1, &mut written);
        }
        assert_eq!(wrapper.take(), vec![1, 2, 0, 0, 9]);
    }

    #[test]
    fn attribute_list_round_trips_every_type() {
        let list = ComWrapper::new(AttributeList::default());
        let a = list.to_com_ptr::<IAttributeList>().unwrap();
        let id = CString::new("key").unwrap();
        unsafe {
            a.setInt(id.as_ptr(), -7);
            let mut i = 0;
            assert_eq!(a.getInt(id.as_ptr(), &mut i), kResultOk);
            assert_eq!(i, -7);

            // Reading a key back as the wrong type is a miss, not a lie.
            let mut f = 0.0;
            assert_eq!(a.getFloat(id.as_ptr(), &mut f), kResultFalse);

            let fid = CString::new("f").unwrap();
            a.setFloat(fid.as_ptr(), 0.25);
            assert_eq!(a.getFloat(fid.as_ptr(), &mut f), kResultOk);
            assert_eq!(f, 0.25);

            let sid = CString::new("s").unwrap();
            let text: Vec<TChar> = "hi".encode_utf16().map(|u| u as TChar).chain([0]).collect();
            a.setString(sid.as_ptr(), text.as_ptr());
            let mut out = [0 as TChar; 8];
            assert_eq!(
                a.getString(sid.as_ptr(), out.as_mut_ptr(), 16),
                kResultOk
            );
            assert_eq!(out[0] as u16, b'h' as u16);
            assert_eq!(out[1] as u16, b'i' as u16);
            assert_eq!(out[2], 0);

            let bid = CString::new("b").unwrap();
            let blob = [1u8, 2, 3];
            a.setBinary(bid.as_ptr(), blob.as_ptr().cast(), 3);
            let mut data = std::ptr::null();
            let mut size = 0;
            assert_eq!(a.getBinary(bid.as_ptr(), &mut data, &mut size), kResultOk);
            assert_eq!(size, 3);
            assert_eq!(std::slice::from_raw_parts(data.cast::<u8>(), 3), &blob);

            let missing = CString::new("nope").unwrap();
            assert_eq!(a.getInt(missing.as_ptr(), &mut i), kResultFalse);
        }
    }

    #[test]
    fn attribute_list_truncates_a_string_to_the_callers_buffer() {
        let list = ComWrapper::new(AttributeList::default());
        let a = list.to_com_ptr::<IAttributeList>().unwrap();
        let id = CString::new("s").unwrap();
        unsafe {
            let text: Vec<TChar> = "abcdef".encode_utf16().map(|u| u as TChar).chain([0]).collect();
            a.setString(id.as_ptr(), text.as_ptr());
            let mut out = [0x7f as TChar; 4];
            // 6 bytes is 3 TChars, so two characters plus a terminator.
            assert_eq!(a.getString(id.as_ptr(), out.as_mut_ptr(), 6), kResultOk);
            assert_eq!(out[0] as u16, b'a' as u16);
            assert_eq!(out[1] as u16, b'b' as u16);
            assert_eq!(out[2], 0);
        }
    }

    #[test]
    fn host_application_creates_the_two_required_classes() {
        let host = host_context();
        unsafe {
            let mut name: String128 = [0; 128];
            assert_eq!(host.getName(&mut name), kResultOk);
            let len = name.iter().position(|c| *c == 0).unwrap();
            let text: String = char::decode_utf16(name[..len].iter().map(|c| *c as u16))
                .map(|c| c.unwrap())
                .collect();
            assert_eq!(text, "Splitwave");

            for mut iid in [IMessage::IID, IAttributeList::IID].map(as_tuid) {
                let mut obj = std::ptr::null_mut();
                assert_eq!(
                    host.createInstance(&mut iid, &mut iid, &mut obj),
                    kResultOk
                );
                assert!(!obj.is_null());
                drop(ComPtr::<IMessage>::from_raw(obj.cast()));
            }

            let mut unknown: TUID = [0; 16];
            let mut obj = std::ptr::null_mut();
            assert_eq!(
                host.createInstance(&mut unknown, &mut unknown, &mut obj),
                kNotImplemented
            );
            assert!(obj.is_null());
        }
    }

    #[test]
    fn message_carries_an_id_and_its_attributes() {
        let message = ComWrapper::new(Message::default());
        let m = message.to_com_ptr::<IMessage>().unwrap();
        unsafe {
            let id = CString::new("hello").unwrap();
            m.setMessageID(id.as_ptr());
            assert_eq!(CStr::from_ptr(m.getMessageID()), id.as_c_str());

            let attributes = m.getAttributes();
            assert!(!attributes.is_null());
            let a = ComRef::from_raw(attributes).unwrap();
            let key = CString::new("k").unwrap();
            a.setInt(key.as_ptr(), 42);
            let mut v = 0;
            // The same list must come back on a second call, not a fresh one.
            let again = ComRef::from_raw(m.getAttributes()).unwrap();
            assert_eq!(again.getInt(key.as_ptr(), &mut v), kResultOk);
            assert_eq!(v, 42);
        }
    }

    #[test]
    fn component_handler_forwards_edits_to_the_listener() {
        struct Spy {
            edits: RefCell<Vec<(ParamID, ParamValue)>>,
            restarts: Cell<i32>,
        }
        impl EditListener for std::rc::Rc<Spy> {
            fn param_edited(&self, id: ParamID, value: ParamValue) {
                self.edits.borrow_mut().push((id, value));
            }
            fn restart(&self, flags: i32) {
                self.restarts.set(self.restarts.get() | flags);
            }
        }

        let handler = ComWrapper::new(ComponentHandler::new(std::rc::Rc::new(Spy {
            edits: RefCell::new(Vec::new()),
            restarts: Cell::new(0),
        })));
        let spy = handler.listener.clone();
        let h = handler.to_com_ptr::<IComponentHandler>().unwrap();
        unsafe {
            h.beginEdit(3);
            h.performEdit(3, 0.75);
            h.endEdit(3);
            h.restartComponent(8);
        }
        assert_eq!(*spy.edits.borrow(), vec![(3, 0.75)]);
        assert_eq!(spy.restarts.get(), 8);
    }
}
