/*
SPDX-License-Identifier: Apache-2.0 OR MIT

Copyright 2020 The arboard contributors

The project to which this file belongs is licensed under either of
the Apache 2.0 or the MIT license at the licensee's choice. The terms
and conditions of the chosen license apply to this file.
*/

#[cfg(feature = "image-data")]
use super::common::ImageData;
use super::common::{ContentType, Error};
use crate::common::GetContentResult;
#[cfg(feature = "image-data")]
use core_graphics::{
	base::{kCGBitmapByteOrderDefault, kCGImageAlphaLast, kCGRenderingIntentDefault, CGFloat},
	color_space::CGColorSpace,
	data_provider::{CGDataProvider, CustomData},
	image::CGImage,
};
use lazy_static::lazy_static;
use objc::runtime::{Class, Object};
#[cfg(feature = "image-data")]
use objc::runtime::{BOOL, NO};
use objc::{class, msg_send, sel, sel_impl};
use objc_foundation::{INSArray, INSData, INSFastEnumeration, INSObject, INSString, NSData};
use objc_foundation::{NSArray, NSDictionary, NSObject, NSString};
use objc_id::{Id, Owned};
use std::collections::HashMap;
use std::mem::transmute;
use std::sync::{Mutex, MutexGuard};

// creating or accessing the OSX pasteboard is not thread-safe, and needs to be protected
// see https://github.com/alacritty/copypasta/issues/11.
struct ClipboardMutexToken;
lazy_static! {
	static ref CLIPBOARD_CONTEXT_MUTEX: Mutex<ClipboardMutexToken> =
		Mutex::new(ClipboardMutexToken {});
}

// required to bring NSPasteboard into the path of the class-resolver
#[link(name = "AppKit", kind = "framework")]
extern "C" {}

/// Returns an NSImage object on success.
#[cfg(feature = "image-data")]
fn image_from_pixels(
	pixels: Vec<u8>,
	width: usize,
	height: usize,
) -> Result<Id<NSObject>, Box<dyn std::error::Error>> {
	#[repr(C)]
	#[derive(Copy, Clone)]
	pub struct NSSize {
		pub width: CGFloat,
		pub height: CGFloat,
	}

	#[derive(Debug, Clone)]
	struct PixelArray {
		data: Vec<u8>,
	}

	impl CustomData for PixelArray {
		unsafe fn ptr(&self) -> *const u8 {
			self.data.as_ptr()
		}
		unsafe fn len(&self) -> usize {
			self.data.len()
		}
	}

	let colorspace = CGColorSpace::create_device_rgb();
	let bitmap_info: u32 = kCGBitmapByteOrderDefault | kCGImageAlphaLast;
	let pixel_data: Box<Box<dyn CustomData>> = Box::new(Box::new(PixelArray { data: pixels }));
	let provider = unsafe { CGDataProvider::from_custom_data(pixel_data) };
	let rendering_intent = kCGRenderingIntentDefault;
	let cg_image = CGImage::new(
		width,
		height,
		8,
		32,
		4 * width,
		&colorspace,
		bitmap_info,
		&provider,
		false,
		rendering_intent,
	);
	let size = NSSize { width: width as CGFloat, height: height as CGFloat };
	let nsimage_class = Class::get("NSImage").ok_or("Class::get(\"NSImage\")")?;
	let image: Id<NSObject> = unsafe { Id::from_ptr(msg_send![nsimage_class, alloc]) };
	let () = unsafe { msg_send![image, initWithCGImage:cg_image size:size] };
	Ok(image)
}

pub struct OSXClipboardContext {
	pasteboard: Id<Object>,
}

impl OSXClipboardContext {
	pub(crate) fn new() -> Result<OSXClipboardContext, Error> {
		let lock = CLIPBOARD_CONTEXT_MUTEX.lock();
		assert!(lock.is_ok(), "could not acquire mutex");

		let cls = Class::get("NSPasteboard")
			.ok_or(Error::Unknown { description: "Class::get(\"NSPasteboard\")".into() })?;
		let pasteboard: *mut Object = unsafe { msg_send![cls, generalPasteboard] };
		if pasteboard.is_null() {
			return Err(Error::Unknown {
				description: "NSPasteboard#generalPasteboard returned null".into(),
			});
		}
		let pasteboard: Id<Object> = unsafe { Id::from_ptr(pasteboard) };
		Ok(OSXClipboardContext { pasteboard })
	}
	pub(crate) fn get_text(&mut self) -> Result<String, Error> {
		let lock = CLIPBOARD_CONTEXT_MUTEX.lock();
		assert!(lock.is_ok(), "could not acquire mutex");

		let string_class: Id<NSObject> = {
			let cls: Id<Class> = unsafe { Id::from_ptr(class("NSString")) };
			unsafe { transmute(cls) }
		};
		let classes: Id<NSArray<NSObject, Owned>> = NSArray::from_vec(vec![string_class]);
		let options: Id<NSDictionary<NSObject, NSObject>> = NSDictionary::new();
		let string_array: Id<NSArray<NSString>> = unsafe {
			let obj: *mut NSArray<NSString> =
				msg_send![self.pasteboard, readObjectsForClasses:&*classes options:&*options];
			if obj.is_null() {
				return Err(Error::ContentNotAvailable);
			}
			Id::from_ptr(obj)
		};
		if string_array.count() == 0 {
			Err(Error::ContentNotAvailable)
		} else {
			Ok(string_array[0].as_str().to_owned())
		}
	}
	pub(crate) fn set_text(&mut self, data: String) -> Result<(), Error> {
		let lock = CLIPBOARD_CONTEXT_MUTEX.lock();
		assert!(lock.is_ok(), "could not acquire mutex");

		let string_array = NSArray::from_vec(vec![NSString::from_str(&data)]);
		let _: usize = unsafe { msg_send![self.pasteboard, clearContents] };
		let success: bool = unsafe { msg_send![self.pasteboard, writeObjects: string_array] };
		if success {
			Ok(())
		} else {
			Err(Error::Unknown { description: "NSPasteboard#writeObjects: returned false".into() })
		}
	}
	// fn get_binary_contents(&mut self) -> Result<Option<ClipboardContent>, Box<dyn std::error::Error>> {
	// 	let string_class: Id<NSObject> = {
	// 		let cls: Id<Class> = unsafe { Id::from_ptr(class("NSString")) };
	// 		unsafe { transmute(cls) }
	// 	};
	// 	let image_class: Id<NSObject> = {
	// 		let cls: Id<Class> = unsafe { Id::from_ptr(class("NSImage")) };
	// 		unsafe { transmute(cls) }
	// 	};
	// 	let url_class: Id<NSObject> = {
	// 		let cls: Id<Class> = unsafe { Id::from_ptr(class("NSURL")) };
	// 		unsafe { transmute(cls) }
	// 	};
	// 	let classes = vec![url_class, image_class, string_class];
	// 	let classes: Id<NSArray<NSObject, Owned>> = NSArray::from_vec(classes);
	// 	let options: Id<NSDictionary<NSObject, NSObject>> = NSDictionary::new();
	// 	let contents: Id<NSArray<NSObject>> = unsafe {
	// 		let obj: *mut NSArray<NSObject> =
	// 			msg_send![self.pasteboard, readObjectsForClasses:&*classes options:&*options];
	// 		if obj.is_null() {
	// 			return Err(err("pasteboard#readObjectsForClasses:options: returned null"));
	// 		}
	// 		Id::from_ptr(obj)
	// 	};
	// 	if contents.count() == 0 {
	// 		Ok(None)
	// 	} else {
	// 		let obj = &contents[0];
	// 		if obj.is_kind_of(Class::get("NSString").unwrap()) {
	// 			let s: &NSString = unsafe { transmute(obj) };
	// 			Ok(Some(ClipboardContent::Utf8(s.as_str().to_owned())))
	// 		} else if obj.is_kind_of(Class::get("NSImage").unwrap()) {
	// 			let tiff: &NSArray<NSObject> = unsafe { msg_send![obj, TIFFRepresentation] };
	// 			let len: usize = unsafe { msg_send![tiff, length] };
	// 			let bytes: *const u8 = unsafe { msg_send![tiff, bytes] };
	// 			let vec = unsafe { std::slice::from_raw_parts(bytes, len) };
	// 			// Here we copy the entire &[u8] into a new owned `Vec`
	// 			// Is there another way that doesn't copy multiple megabytes?
	// 			Ok(Some(ClipboardContent::Tiff(vec.into())))
	// 		} else if obj.is_kind_of(Class::get("NSURL").unwrap()) {
	// 			let s: &NSString = unsafe { msg_send![obj, absoluteString] };
	// 			Ok(Some(ClipboardContent::Utf8(s.as_str().to_owned())))
	// 		} else {
	// 			// let cls: &Class = unsafe { msg_send![obj, class] };
	// 			// println!("{}", cls.name());
	// 			Err(err("pasteboard#readObjectsForClasses:options: returned unknown class"))
	// 		}
	// 	}
	// }
	#[cfg(feature = "image-data")]
	pub(crate) fn get_image(&mut self) -> Result<ImageData, Error> {
		use std::io::Cursor;

		let lock = CLIPBOARD_CONTEXT_MUTEX.lock();
		assert!(lock.is_ok(), "could not acquire mutex");

		let image_class: Id<NSObject> = {
			let cls: Id<Class> = unsafe { Id::from_ptr(class("NSImage")) };
			unsafe { transmute(cls) }
		};
		let classes = vec![image_class];
		let classes: Id<NSArray<NSObject, Owned>> = NSArray::from_vec(classes);
		let options: Id<NSDictionary<NSObject, NSObject>> = NSDictionary::new();
		let contents: Id<NSArray<NSObject>> = unsafe {
			let obj: *mut NSArray<NSObject> =
				msg_send![self.pasteboard, readObjectsForClasses:&*classes options:&*options];
			if obj.is_null() {
				return Err(Error::ContentNotAvailable);
			}
			Id::from_ptr(obj)
		};
		let result;
		if contents.count() == 0 {
			result = Err(Error::ContentNotAvailable);
		} else {
			let obj = &contents[0];
			if obj.is_kind_of(Class::get("NSImage").unwrap()) {
				let tiff: &NSArray<NSObject> = unsafe { msg_send![obj, TIFFRepresentation] };
				let len: usize = unsafe { msg_send![tiff, length] };
				let bytes: *const u8 = unsafe { msg_send![tiff, bytes] };
				let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
				let data_cursor = Cursor::new(slice);
				let reader = image::io::Reader::with_format(data_cursor, image::ImageFormat::Tiff);
				let width;
				let height;
				let pixels;
				match reader.decode() {
					Ok(img) => {
						let rgba = img.into_rgba8();
						let (w, h) = rgba.dimensions();
						width = w;
						height = h;
						pixels = rgba.into_raw();
					}
					Err(_) => return Err(Error::ConversionFailure),
				};
				let data = ImageData {
					width: width as usize,
					height: height as usize,
					bytes: pixels.into(),
				};
				result = Ok(data);
			} else {
				// let cls: &Class = unsafe { msg_send![obj, class] };
				// println!("{}", cls.name());
				result = Err(Error::ContentNotAvailable);
			}
		}
		result
	}

	#[cfg(feature = "image-data")]
	pub(crate) fn set_image(&mut self, data: ImageData) -> Result<(), Error> {
		let lock = CLIPBOARD_CONTEXT_MUTEX.lock();
		assert!(lock.is_ok(), "could not acquire mutex");

		let pixels = data.bytes.into();
		let image = image_from_pixels(pixels, data.width, data.height)
			.map_err(|_| Error::ConversionFailure)?;
		let objects: Id<NSArray<NSObject, Owned>> = NSArray::from_vec(vec![image]);
		let _: usize = unsafe { msg_send![self.pasteboard, clearContents] };
		let success: BOOL = unsafe { msg_send![self.pasteboard, writeObjects: objects] };
		if success == NO {
			return Err(Error::Unknown {
				description:
					"Failed to write the image to the pasteboard (`writeObjects` returned NO)."
						.into(),
			});
		}
		Ok(())
	}

	pub fn get_content_types(&mut self) -> Result<Vec<String>, Error> {
		let lock = CLIPBOARD_CONTEXT_MUTEX.lock();
		assert!(lock.is_ok(), "could not acquire mutex");

		let first_item = self.first_item(&mut lock.unwrap());
		if first_item.is_none() {
			return Ok(Vec::new());
		}
		let types: Id<NSArray<NSString>> = unsafe {
			let types: *mut NSArray<NSString> = msg_send![first_item.unwrap(), types];
			Id::from_ptr(types)
		};
		Ok(types.enumerator().into_iter().map(|t| t.as_str().into()).collect())
	}

	pub fn get_content_for_types(&mut self, ct: &[ContentType]) -> Result<GetContentResult, Error> {
		// TODO this is 100% broken
		let lock = CLIPBOARD_CONTEXT_MUTEX.lock();
		assert!(lock.is_ok(), "could not acquire mutex");

		let first_item = self.first_item(&mut lock.unwrap());
		if first_item.is_none() {
			return Err(Error::ContentNotAvailable);
		}
		let typ: Id<NSString> = NSString::from_str(&self.denormalize_content_type(ct.clone()));
		let data: Id<NSData> = unsafe {
			let data: *mut NSData = msg_send![self.pasteboard, dataForType: typ];
			if data.is_null() {
				return Err(Error::ContentNotAvailable);
			}
			Id::from_ptr(data)
		};
		Ok(data.bytes().to_vec())
	}

	pub fn set_content_types(&mut self, map: HashMap<ContentType, Vec<u8>>) -> Result<(), Error> {
		let lock = CLIPBOARD_CONTEXT_MUTEX.lock();
		assert!(lock.is_ok(), "could not acquire mutex");

		let cls = class!(NSPasteboardItem);
		let pasteboard_item: Id<NSObject> = unsafe {
			let item: *mut NSObject = msg_send![cls, new];
			Id::from_ptr(item)
		};
		for (ct, data) in map.into_iter() {
			let data = NSData::from_vec(data);
			let typ: Id<NSString> = NSString::from_str(&self.denormalize_content_type(ct));
			unsafe { msg_send![pasteboard_item, setData:data forType:typ] }
		}
		let items = NSArray::from_vec(vec![pasteboard_item]);
		let result: BOOL = unsafe {
			let _: () = msg_send![self.pasteboard, clearContents];
			msg_send![self.pasteboard, writeObjects: items]
		};
		if result == NO {
			Err(Error::ClipboardOccupied)
		} else {
			Ok(())
		}
	}

	pub fn normalize_content_type(&self, s: String) -> ContentType {
		match s.as_str() {
			"public.file-url" => ContentType::Url,
			"public.html" => ContentType::Html,
			"com.adobe.pdf" => ContentType::Pdf,
			"public.png" => ContentType::Png,
			"public.rtf" => ContentType::Rtf,
			"public.utf8-plain-text" => ContentType::Text,
			_ => ContentType::Custom(s),
		}
	}

	/// On OSX, all supported CTs have a single system type
	fn denormalize_ct_single(&self, ct: ContentType) -> String {
		match ct {
			ContentType::Url => "public.file-url",
			ContentType::Html => "public.html",
			ContentType::Pdf => "com.adobe.pdf",
			ContentType::Png => "public.png",
			ContentType::Rtf => "public.rtf",
			ContentType::Text => "public.utf8-plain-text",
			ContentType::Custom(s) => return s,
		}
		.into()
	}

	pub fn denormalize_content_type(&self, ct: ContentType) -> Vec<String> {
		vec![self.denormalize_ct_single(ct)]
	}

	/// Gets the first item from the pasteboard.
	///
	/// Requires a mutex guard to prove that we hold the mutex. This method is called from methods
	/// which might need to hold the mutex, so it doesn't lock it itself.
	fn first_item(&self, _guard: &mut MutexGuard<ClipboardMutexToken>) -> Option<&NSObject> {
		unsafe {
			// TODO I don't understand the memory model here. The NSArray we get is a copy, as
			//      seen in https://developer.apple.com/documentation/appkit/nspasteboard/1529995-pasteboarditems?language=objc
			//      but the elements themselves are not. So when are they deallocated? On the next
			//      call to pasteboardItems? That seems wasteful?
			let items: *mut NSArray<NSObject> = msg_send![self.pasteboard, pasteboardItems];
			if items.is_null() {
				return None;
			}
			let _id_items: Id<NSArray<NSObject>> = Id::from_ptr(items);
			(&*items).first_object()
		}
	}
}

// this is a convenience function that both cocoa-rs and
//  glutin define, which seems to depend on the fact that
//  Option::None has the same representation as a null pointer
#[inline]
pub fn class(name: &str) -> *mut Class {
	unsafe { transmute(Class::get(name)) }
}
