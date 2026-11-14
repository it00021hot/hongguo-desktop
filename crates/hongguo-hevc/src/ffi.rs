use std::ffi::{c_char, c_int, c_uint, c_void};

pub type Context = c_void;
pub type Image = c_void;
pub const OK: c_uint = 0;
pub const IMAGE_BUFFER_FULL: c_uint = 9;
pub const WAITING_FOR_INPUT: c_uint = 13;

unsafe extern "C" {
    pub fn de265_new_decoder() -> *mut Context;
    pub fn de265_free_decoder(context: *mut Context) -> c_uint;
    pub fn de265_start_worker_threads(context: *mut Context, threads: c_int) -> c_uint;
    pub fn de265_push_data(
        context: *mut Context,
        data: *const c_void,
        length: c_int,
        pts: i64,
        user: *mut c_void,
    ) -> c_uint;
    pub fn de265_push_end_of_NAL(context: *mut Context);
    pub fn de265_flush_data(context: *mut Context) -> c_uint;
    pub fn de265_decode(context: *mut Context, more: *mut c_int) -> c_uint;
    pub fn de265_peek_next_picture(context: *mut Context) -> *const Image;
    pub fn de265_release_next_picture(context: *mut Context);
    pub fn de265_get_error_text(error: c_uint) -> *const c_char;
    pub fn de265_get_image_width(image: *const Image, channel: c_int) -> c_int;
    pub fn de265_get_image_height(image: *const Image, channel: c_int) -> c_int;
    pub fn de265_get_bits_per_pixel(image: *const Image, channel: c_int) -> c_int;
    pub fn de265_get_chroma_format(image: *const Image) -> c_uint;
    pub fn de265_get_image_plane(
        image: *const Image,
        channel: c_int,
        stride: *mut c_int,
    ) -> *const u8;
    pub fn de265_get_image_PTS(image: *const Image) -> i64;
    pub fn de265_get_image_full_range_flag(image: *const Image) -> c_int;
    pub fn de265_get_image_colour_primaries(image: *const Image) -> c_int;
    pub fn de265_get_image_transfer_characteristics(image: *const Image) -> c_int;
    pub fn de265_get_image_matrix_coefficients(image: *const Image) -> c_int;
}
