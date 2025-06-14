#[cfg(feature = "v1_18")]
use std::num::NonZeroU16;
use std::os::raw::c_void;
use std::{ffi, ptr};

use four_cc::FourCC;
use libheif_sys as lh;

use crate::encoder::get_encoding_options_ptr;
use crate::reader::{Reader, HEIF_READER};
use crate::utils::str_to_cstring;
#[cfg(feature = "v1_19")]
use crate::SecurityLimits;
use crate::{
    Encoder, EncodingOptions, HeifError, HeifErrorCode, HeifErrorSubCode, Image, ImageHandle,
    ItemId, Result,
};

#[allow(dead_code)]
enum Source<'a> {
    None,
    File,
    Memory(&'a [u8]),
    Reader(Box<Box<dyn Reader>>),
}

pub struct HeifContext<'a> {
    pub(crate) inner: *mut lh::heif_context,
    source: Source<'a>,
}

impl HeifContext<'static> {
    /// Create a new empty context.
    pub fn new() -> Result<HeifContext<'static>> {
        let ctx = unsafe { lh::heif_context_alloc() };
        if ctx.is_null() {
            Err(HeifError {
                code: HeifErrorCode::ContextCreateFailed,
                sub_code: HeifErrorSubCode::Unspecified,
                message: String::from(""),
            })
        } else {
            Ok(HeifContext {
                inner: ctx,
                source: Source::None,
            })
        }
    }

    /// Create a new context from file.
    pub fn read_from_file(name: &str) -> Result<HeifContext<'static>> {
        let mut context = HeifContext::new()?;
        context.read_file(name)?;
        Ok(context)
    }

    /// Read a HEIF file from a named disk file.
    pub fn read_file(&mut self, name: &str) -> Result<()> {
        self.source = Source::File;
        let c_name = ffi::CString::new(name).unwrap();
        let err =
            unsafe { lh::heif_context_read_from_file(self.inner, c_name.as_ptr(), ptr::null()) };
        HeifError::from_heif_error(err)?;
        Ok(())
    }

    /// Create a new context from reader.
    pub fn read_from_reader(reader: Box<dyn Reader>) -> Result<HeifContext<'static>> {
        let mut context = HeifContext::new()?;
        context.read_reader(reader)?;
        Ok(context)
    }

    /// Read a HEIF file from the reader.
    pub fn read_reader(&mut self, reader: Box<dyn Reader>) -> Result<()> {
        let mut reader_box = Box::new(reader);
        let user_data = reader_box.as_mut() as *mut _ as *mut c_void;
        let err = unsafe {
            lh::heif_context_read_from_reader(self.inner, &HEIF_READER, user_data, ptr::null())
        };
        HeifError::from_heif_error(err)?;
        self.source = Source::Reader(reader_box);
        Ok(())
    }

    /// # Safety
    ///
    /// The given pointer must be valid.
    #[cfg(feature = "v1_18")]
    pub(crate) unsafe fn from_ptr(ctx: *mut lh::heif_context) -> HeifContext<'static> {
        HeifContext {
            inner: ctx,
            source: Source::None,
        }
    }
}

impl<'a> HeifContext<'a> {
    /// Create a new context from bytes.
    ///
    /// The provided memory buffer is not copied.
    /// That means, you will have to keep the memory buffer alive as
    /// long as you use the context.
    pub fn read_from_bytes(bytes: &[u8]) -> Result<HeifContext> {
        let mut context = HeifContext::new()?;
        context.read_bytes(bytes)?;
        Ok(context)
    }

    /// Read a HEIF file from bytes.
    ///
    /// The provided memory buffer is not copied.
    /// That means, you will have to keep the memory buffer alive as
    /// long as you use the context.
    pub fn read_bytes<'b: 'a>(&mut self, bytes: &'b [u8]) -> Result<()> {
        self.source = Source::Memory(bytes);
        let err = unsafe {
            lh::heif_context_read_from_memory_without_copy(
                self.inner,
                bytes.as_ptr() as _,
                bytes.len(),
                ptr::null(),
            )
        };
        HeifError::from_heif_error(err)?;
        Ok(())
    }

    unsafe extern "C" fn vector_writer(
        _ctx: *mut lh::heif_context,
        data: *const c_void,
        size: usize,
        user_data: *mut c_void,
    ) -> lh::heif_error {
        let vec: &mut Vec<u8> = &mut *(user_data as *mut Vec<u8>);
        vec.reserve(size);
        ptr::copy_nonoverlapping::<u8>(data as _, vec.as_mut_ptr(), size);
        vec.set_len(size);

        lh::heif_error {
            code: lh::heif_error_code_heif_error_Ok,
            subcode: lh::heif_suberror_code_heif_suberror_Unspecified,
            message: b"\0".as_ptr() as _,
        }
    }

    pub fn write_to_bytes(&self) -> Result<Vec<u8>> {
        let mut res = Vec::<u8>::new();
        let pointer_to_res = &mut res as *mut _ as *mut c_void;

        let mut writer = lh::heif_writer {
            writer_api_version: 1,
            write: Some(Self::vector_writer),
        };

        let err = unsafe { lh::heif_context_write(self.inner, &mut writer, pointer_to_res) };
        HeifError::from_heif_error(err)?;
        Ok(res)
    }

    pub fn write_to_file(&self, name: &str) -> Result<()> {
        let c_name = ffi::CString::new(name).unwrap();
        let err = unsafe { lh::heif_context_write_to_file(self.inner, c_name.as_ptr()) };
        HeifError::from_heif_error(err)
    }

    pub fn number_of_top_level_images(&self) -> usize {
        unsafe { lh::heif_context_get_number_of_top_level_images(self.inner) as _ }
    }

    pub fn top_level_image_ids(&self, item_ids: &mut [ItemId]) -> usize {
        if item_ids.is_empty() {
            0
        } else {
            unsafe {
                lh::heif_context_get_list_of_top_level_image_IDs(
                    self.inner,
                    item_ids.as_mut_ptr(),
                    item_ids.len() as _,
                ) as usize
            }
        }
    }

    pub fn image_handle(&self, item_id: ItemId) -> Result<ImageHandle> {
        let mut handle: *mut lh::heif_image_handle = ptr::null_mut();
        let err = unsafe { lh::heif_context_get_image_handle(self.inner, item_id, &mut handle) };
        HeifError::from_heif_error(err)?;
        Ok(ImageHandle::new(handle))
    }

    pub fn primary_image_handle(&self) -> Result<ImageHandle> {
        let mut handle: *mut lh::heif_image_handle = ptr::null_mut();
        let err = unsafe { lh::heif_context_get_primary_image_handle(self.inner, &mut handle) };
        HeifError::from_heif_error(err)?;
        Ok(ImageHandle::new(handle))
    }

    pub fn top_level_image_handles(&self) -> Vec<ImageHandle> {
        let max_count = self.number_of_top_level_images();
        let mut item_ids = Vec::with_capacity(max_count);
        unsafe {
            let count = lh::heif_context_get_list_of_top_level_image_IDs(
                self.inner,
                item_ids.as_mut_ptr(),
                max_count as _,
            ) as usize;
            item_ids.set_len(count);
        }
        let mut handles = Vec::with_capacity(item_ids.len());
        for item_id in item_ids {
            if let Ok(handle) = self.image_handle(item_id) {
                handles.push(handle);
            }
        }
        handles
    }

    /// Compress the input image.
    /// The first image added to the context is also automatically set as the primary image, but
    /// you can change the primary image later with [`HeifContext::set_primary_image`] method.
    pub fn encode_image(
        &mut self,
        image: &Image,
        encoder: &mut Encoder,
        encoding_options: Option<EncodingOptions>,
    ) -> Result<ImageHandle> {
        let mut handle: *mut lh::heif_image_handle = ptr::null_mut();
        unsafe {
            let err = lh::heif_context_encode_image(
                self.inner,
                image.inner,
                encoder.inner,
                get_encoding_options_ptr(&encoding_options),
                &mut handle,
            );
            HeifError::from_heif_error(err)?;
        }
        Ok(ImageHandle::new(handle))
    }

    /// Encode the `image` as a scaled down thumbnail image.
    /// The image is scaled down to fit into a square area of width `bbox_size`.
    /// If the input image is already so small that it fits into this bounding
    /// box, no thumbnail image is encoded and `Ok(None)` is returned.
    /// No error is returned in this case.
    ///
    /// The encoded thumbnail is automatically assigned to the
    /// `master_image_handle`. Hence, you do not have to call
    /// [`HeifContext::assign_thumbnail()`] method.
    pub fn encode_thumbnail(
        &mut self,
        image: &Image,
        master_image_handle: &ImageHandle,
        bbox_size: u32,
        encoder: &mut Encoder,
        encoding_options: Option<EncodingOptions>,
    ) -> Result<Option<ImageHandle>> {
        let mut handle: *mut lh::heif_image_handle = ptr::null_mut();
        unsafe {
            let err = lh::heif_context_encode_thumbnail(
                self.inner,
                image.inner,
                master_image_handle.inner,
                encoder.inner,
                get_encoding_options_ptr(&encoding_options),
                bbox_size.min(i32::MAX as _) as _,
                &mut handle,
            );
            HeifError::from_heif_error(err)?;
        }
        Ok(Some(ImageHandle::new(handle)))
    }

    /// Encodes an array of images into a grid.
    ///
    /// # Arguments
    ///
    /// * `tiles` - User allocated array of images that will form the grid.
    /// * `rows` - The number of rows in the grid. The number of columns will
    ///   be calculated from the size of `tiles`.
    /// * `encoder` - Defines the encoder to use.
    ///   See [LibHeif::encoder_for_format()](crate::LibHeif::encoder_for_format).
    /// * `encoding_options` - Optional, may be None.
    ///
    /// Returns an error if `tiles` slice is empty.
    #[cfg(feature = "v1_18")]
    pub fn encode_grid(
        &mut self,
        tiles: &[Image],
        rows: NonZeroU16,
        encoder: &mut Encoder,
        encoding_options: Option<EncodingOptions>,
    ) -> Result<Option<ImageHandle>> {
        let mut handle: *mut lh::heif_image_handle = ptr::null_mut();
        let mut tiles_inners: Vec<*mut lh::heif_image> =
            tiles.iter().map(|img| img.inner).collect();
        let rows = rows.get();
        let columns = (tiles_inners.len() as u32 / rows as u32).min(u16::MAX as _) as u16;
        unsafe {
            let err = lh::heif_context_encode_grid(
                self.inner,
                tiles_inners.as_mut_ptr(),
                rows,
                columns,
                encoder.inner,
                get_encoding_options_ptr(&encoding_options),
                &mut handle,
            );
            HeifError::from_heif_error(err)?;
        }
        Ok(Some(ImageHandle::new(handle)))
    }

    /// Assign `master_image_handle` as the thumbnail image of `thumbnail_image_handle`.
    pub fn assign_thumbnail(
        &mut self,
        master_image_handle: &ImageHandle,
        thumbnail_image_handle: &ImageHandle,
    ) -> Result<()> {
        unsafe {
            let err = lh::heif_context_assign_thumbnail(
                self.inner,
                master_image_handle.inner,
                thumbnail_image_handle.inner,
            );
            HeifError::from_heif_error(err)
        }
    }

    pub fn set_primary_image(&mut self, image_handle: &mut ImageHandle) -> Result<()> {
        unsafe {
            let err = lh::heif_context_set_primary_image(self.inner, image_handle.inner);
            HeifError::from_heif_error(err)
        }
    }

    /// Add generic, proprietary metadata to an image. You have to specify
    /// an `item_type` that will identify your metadata. `content_type` can be
    /// an additional type.
    ///
    /// For example, this function can be used to add IPTC metadata
    /// (IIM stream, not XMP) to an image. Although not standard, we propose
    /// to store IPTC data with `item_type=b"iptc"` and `content_type=None`.
    pub fn add_generic_metadata<T>(
        &mut self,
        image_handle: &ImageHandle,
        data: &[u8],
        item_type: T,
        content_type: Option<&str>,
    ) -> Result<()>
    where
        T: Into<FourCC>,
    {
        let c_item_type = str_to_cstring(&item_type.into().to_string(), "item_type")?;
        let c_content_type = match content_type {
            Some(s) => Some(str_to_cstring(s, "content_type")?),
            None => None,
        };
        let c_content_type_ptr = c_content_type.map(|s| s.as_ptr()).unwrap_or(ptr::null());
        let error = unsafe {
            lh::heif_context_add_generic_metadata(
                self.inner,
                image_handle.inner,
                data.as_ptr() as _,
                data.len() as _,
                c_item_type.as_ptr(),
                c_content_type_ptr,
            )
        };
        HeifError::from_heif_error(error)
    }

    /// Add EXIF metadata to an image.
    pub fn add_exif_metadata(&mut self, master_image: &ImageHandle, data: &[u8]) -> Result<()> {
        let error = unsafe {
            lh::heif_context_add_exif_metadata(
                self.inner,
                master_image.inner,
                data.as_ptr() as _,
                data.len() as _,
            )
        };
        HeifError::from_heif_error(error)
    }

    /// Add XMP metadata to an image.
    pub fn add_xmp_metadata(&mut self, master_image: &ImageHandle, data: &[u8]) -> Result<()> {
        let error = unsafe {
            lh::heif_context_add_XMP_metadata(
                self.inner,
                master_image.inner,
                data.as_ptr() as _,
                data.len() as _,
            )
        };
        HeifError::from_heif_error(error)
    }

    /// If the maximum threads number is set to 0, the image tiles are
    /// decoded in the main thread. This is different from setting it to 1,
    /// which will generate a single background thread to decode the tiles.
    ///
    /// Note that this setting only affects `libheif` itself. The codecs itself
    /// may still use multi-threaded decoding. You can use it, for example,
    /// in cases where you are decoding several images in parallel anyway you
    /// thus want to minimize parallelism in each decoder.
    pub fn set_max_decoding_threads(&mut self, max_threads: u32) {
        let max_threads = max_threads.min(libc::c_int::MAX as u32) as libc::c_int;
        unsafe { lh::heif_context_set_max_decoding_threads(self.inner, max_threads) };
    }

    /// Returns the security limits for a context.
    ///
    /// By default, the limits are set to the global limits,
    /// but you can change them with the help of [`HeifContext::set_security_limits()`] method.
    #[cfg(feature = "v1_19")]
    pub fn security_limits(&self) -> SecurityLimits {
        let inner_ptr = unsafe { lh::heif_context_get_security_limits(self.inner) };
        let inner = ptr::NonNull::new(inner_ptr).unwrap();
        SecurityLimits::from_inner(inner)
    }

    /// Overwrites the security limits of a context.
    #[cfg(feature = "v1_19")]
    pub fn set_security_limits(&mut self, limits: &SecurityLimits) -> Result<()> {
        let err = unsafe { lh::heif_context_set_security_limits(self.inner, limits.as_inner()) };
        HeifError::from_heif_error(err)
    }
}

impl Drop for HeifContext<'_> {
    fn drop(&mut self) {
        unsafe { lh::heif_context_free(self.inner) };
    }
}

unsafe impl Send for HeifContext<'_> {}
