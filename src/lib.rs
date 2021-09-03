/*
SPDX-License-Identifier: Apache-2.0 OR MIT

Copyright 2020 The arboard contributors

The project to which this file belongs is licensed under either of
the Apache 2.0 or the MIT license at the licensee's choice. The terms
and conditions of the chosen license apply to this file.
*/

#![crate_name = "arboard"]
#![crate_type = "lib"]
#![crate_type = "dylib"]
#![crate_type = "rlib"]

mod common;
#[cfg(feature = "image-data")]
pub use common::ImageData;
pub use common::{ContentType, Error};
use std::collections::HashMap;

#[cfg(all(unix, not(any(target_os = "macos", target_os = "android", target_os = "emscripten")),))]
pub(crate) mod common_linux;

#[cfg(all(unix, not(any(target_os = "macos", target_os = "android", target_os = "emscripten")),))]
pub mod x11_clipboard;

#[cfg(all(
	unix,
	not(any(target_os = "macos", target_os = "android", target_os = "emscripten")),
	feature = "wayland-data-control"
))]
pub mod wayland_data_control_clipboard;

#[cfg(windows)]
pub mod windows_clipboard;

#[cfg(target_os = "macos")]
pub mod osx_clipboard;

#[cfg(all(unix, not(any(target_os = "macos", target_os = "android", target_os = "emscripten")),))]
type PlatformClipboard = common_linux::LinuxClipboard;
#[cfg(windows)]
type PlatformClipboard = windows_clipboard::WindowsClipboardContext;
#[cfg(target_os = "macos")]
type PlatformClipboard = osx_clipboard::OSXClipboardContext;

use crate::common::GetContentResult;
#[cfg(all(
	unix,
	not(any(target_os = "macos", target_os = "android", target_os = "emscripten")),
))]
pub use common_linux::{ClipboardExtLinux, LinuxClipboardKind};

/// The OS independent struct for accessing the clipboard.
///
/// Any number of `Clipboard` instances are allowed to exist at a single point in time. Note however
/// that all `Clipboard`s must be 'dropped' before the program exits. In most scenarios this happens
/// automatically but there are frameworks (for example `winit`) that take over the execution
/// and where the objects don't get dropped when the application exits. In these cases you have to
/// make sure the object is dropped by taking ownership of it in a confined scope when detecting
/// that your application is about to quit.
///
/// It is also valid to have multiple `Clipboards` on separate threads at once but note that
/// executing multiple clipboard operations in parallel might fail with a `ClipboardOccupied` error.
///
/// ### Content types
///
/// The clipboard can contain several representations of the same data; for instance, if a user
/// selects text from a browser, the browser may write both an HTML and a plain text version of the
/// selection to the clipboard.
///
/// Each platform has its own convention for the content type descriptor. For convenience, this
/// library provides some standard aliases, which can be converted into platform-appropriate types:
/// see the [`ContentType`] struct. Even if a `ContentType` object is expected, it's always
/// possible to store a system-specific struct in `ContentType::Custom`.
///
/// In general, functions take in `ContentType` objects, but return unconverted system-specific
/// `String`s. This is because conversion from `String` to `ContentType` can be lossy, so it's
/// better for it to be user-controlled. The `normalize_content_type` function can be used to
/// convert a `String` into a `ContentType`.
pub struct Clipboard {
	pub(crate) platform: PlatformClipboard,
}

impl Clipboard {
	/// Creates an instance of the clipboard
	pub fn new() -> Result<Self, Error> {
		Ok(Clipboard { platform: PlatformClipboard::new()? })
	}

	/// Fetches utf-8 text from the clipboard and returns it.
	pub fn get_text(&mut self) -> Result<String, Error> {
		self.platform.get_text()
	}

	/// Places the text onto the clipboard. Any valid utf-8 string is accepted.
	pub fn set_text(&mut self, text: String) -> Result<(), Error> {
		self.platform.set_text(text)
	}

	/// Fetches image data from the clipboard, and returns the decoded pixels.
	///
	/// Any image data placed on the clipboard with `set_image` will be possible read back, using
	/// this function. However it's of not guaranteed that an image placed on the clipboard by any
	/// other application will be of a supported format.
	#[cfg(feature = "image-data")]
	pub fn get_image(&mut self) -> Result<ImageData, Error> {
		self.platform.get_image()
	}

	/// Places an image to the clipboard.
	///
	/// The chosen output format, depending on the platform is the following:
	///
	/// - On macOS: `NSImage` object
	/// - On Linux: PNG, under the atom `image/png`
	/// - On Windows: In order of priority `CF_DIB` and `CF_BITMAP`
	#[cfg(feature = "image-data")]
	pub fn set_image(&mut self, image: ImageData) -> Result<(), Error> {
		self.platform.set_image(image)
	}

	/// Get the list of content types supported by the current clipboard item.
	pub fn get_content_types(&mut self) -> Result<Vec<String>, Error> {
		self.platform.get_content_types()
	}

	/// Get data in the desired format. Tries each content type in `ct`, and returns the first one
	/// for which the clipboard has data.
	pub fn get_content_for_types(&mut self, ct: &[ContentType]) -> Result<GetContentResult, Error> {
		self.platform.get_content_for_types(ct)
	}

	/// Set the mapping of content types to data in the clipboard.
	pub fn set_content_types(&mut self, map: HashMap<ContentType, Vec<u8>>) -> Result<(), Error> {
		self.platform.set_content_types(map)
	}

	/// Normalize a content type, ensuring it is not a [`ContentType::Custom`] instance if it
	/// can be represented as another member of [`ContentType`].
	pub fn normalize_content_type(&self, s: String) -> ContentType {
		self.platform.normalize_content_type(s)
	}

	/// Denormalize content type. The resulting strings can be used to create
	/// [`ContentType::Custom`] instances.
	///
	/// A given content type can turn into more than one underlying system type; for instance,
	/// `html` might turn into both `text/html` and `text/html;charset=utf-8`.
	///
	/// The resulting vector is guaranteed to be non-empty.
	pub fn denormalize_content_type(&self, ct: ContentType) -> Vec<String> {
		self.platform.denormalize_content_type(ct)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	/// All tests are run serially because the windows clipboard cannot be open on
	/// multiple threads at once.
	use serial_test::serial;
	use std::array::IntoIter;
	use std::sync::Once;
	use std::collections::HashSet;
	use std::iter::FromIterator;

	static INIT: Once = Once::new();

	fn setup() {
		INIT.call_once(|| {
			env_logger::builder().is_test(true).try_init().unwrap();
		});
	}

	#[test]
	#[serial]
	fn set_and_get_text() {
		setup();
		let mut ctx = Clipboard::new().unwrap();
		let text = "some string";
		ctx.set_text(text.to_owned()).unwrap();
		assert_eq!(ctx.get_text().unwrap(), text);

		// We also need to check that the content persists after the drop; this is
		// especially important on X11
		drop(ctx);

		// Give any external mechanism a generous amount of time to take over
		// responsibility for the clipboard, in case that happens asynchronously
		// (it appears that this is the case on X11 plus Mutter 3.34+, see #4)
		use std::time::Duration;
		std::thread::sleep(Duration::from_millis(100));

		let mut ctx = Clipboard::new().unwrap();
		assert_eq!(ctx.get_text().unwrap(), text);
	}

	#[test]
	#[serial]
	fn set_and_get_unicode() {
		setup();
		let mut ctx = Clipboard::new().unwrap();
		let text = "Some utf8: 🤓 ∑φ(n)<ε 🐔";
		ctx.set_text(text.to_owned()).unwrap();
		assert_eq!(ctx.get_text().unwrap(), text);
	}

	#[cfg(feature = "image-data")]
	#[test]
	#[serial]
	fn set_and_get_image() {
		setup();
		let mut ctx = Clipboard::new().unwrap();
		#[rustfmt::skip]
					let bytes = [
					255, 100, 100, 255,
					100, 255, 100, 100,
					100, 100, 255, 100,
					0, 0, 0, 255,
				];
		let img_data = ImageData { width: 2, height: 2, bytes: bytes.as_ref().into() };
		ctx.set_image(img_data.clone()).unwrap();
		let got = ctx.get_image().unwrap();
		assert_eq!(img_data.bytes, got.bytes);
	}

	#[cfg(all(
		unix,
		not(any(target_os = "macos", target_os = "android", target_os = "emscripten")),
	))]
	#[test]
	#[serial]
	fn secondary_clipboard() {
		setup();
		let mut ctx = Clipboard::new().unwrap();

		const TEXT1: &str = "I'm a little teapot,";
		const TEXT2: &str = "short and stout,";
		const TEXT3: &str = "here is my handle";

		ctx.set_text_with_clipboard(TEXT1.to_string(), LinuxClipboardKind::Clipboard).unwrap();

		ctx.set_text_with_clipboard(TEXT2.to_string(), LinuxClipboardKind::Primary).unwrap();

		// The secondary clipboard is not available under wayland
		if !cfg!(feature = "wayland-data-control") || std::env::var_os("WAYLAND_DISPLAY").is_none()
		{
			ctx.set_text_with_clipboard(TEXT3.to_string(), LinuxClipboardKind::Secondary).unwrap();
		}

		assert_eq!(TEXT1, &ctx.get_text_with_clipboard(LinuxClipboardKind::Clipboard).unwrap());

		assert_eq!(TEXT2, &ctx.get_text_with_clipboard(LinuxClipboardKind::Primary).unwrap());

		// The secondary clipboard is not available under wayland
		if !cfg!(feature = "wayland-data-control") || std::env::var_os("WAYLAND_DISPLAY").is_none()
		{
			assert_eq!(TEXT3, &ctx.get_text_with_clipboard(LinuxClipboardKind::Secondary).unwrap());
		}
	}

	#[test]
	#[serial]
	fn set_several_types() {
		setup();
		let mut ctx = Clipboard::new().unwrap();
		ctx.set_content_types(
			IntoIter::new([
				(ContentType::Text,
				"hello, world".as_bytes().to_vec()),
				(ContentType::Html,
				"<span>hello, world!</span>".as_bytes().to_vec()),
			]).collect()
		)
		.unwrap();

		let result = ctx.get_content_for_types(&[ContentType::Rtf, ContentType::Html, ContentType::Text]).unwrap();
		assert_eq!(ctx.normalize_content_type(result.content_type), ContentType::Html);
		assert_eq!(result.data, "<span>hello, world!</span>".as_bytes().to_vec());
	}

	#[test]
	#[serial]
	fn list_content_types() {
		setup();
		let mut ctx = Clipboard::new().unwrap();
		ctx.set_content_types(
			IntoIter::new([
				(ContentType::Text,
				 "hello, world".as_bytes().to_vec()),
				(ContentType::Html,
				 "<span>hello, world!</span>".as_bytes().to_vec()),
			]).collect()
		)
			.unwrap();

		let result = ctx.get_content_types().unwrap().into_iter().map(|x| ctx.normalize_content_type(x)).collect::<HashSet<_>>();
		let reference = HashSet::<_>::from_iter(IntoIter::new([ContentType::Text, ContentType::Html]));
		// there can be other types that get added implicitly, for instance TARGETS on X11.
		assert!(reference.is_subset(&result));
	}
}
